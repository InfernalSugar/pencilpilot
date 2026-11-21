//! Brush textures, painted on top of what libmypaint draws:
//!
//! - **Grain**: a paper texture fixed to the canvas (Procreate's "texturized"
//!   grain, Photoshop's Texture); the paint thins out in its low spots.
//! - **Tip**: a picture as the dab's shape (Photoshop sampled brushes, Procreate
//!   shapes), turned with the dab's angle and squashed by its aspect ratio.
//! - **Dual brush**: a second tip, stamped a few times at random inside each
//!   dab; paint lands only where the two overlap (Photoshop's Dual Brush).
//!
//! Each dab is drawn by libmypaint as usual; then the change it made to every
//! pixel is scaled by [`DabStyle::factor`] (see `UndoSurface::draw_dab`).

use hokusai::Dab;

/// An 8-bit grey picture (255 = full paint).
#[derive(Debug, Clone, PartialEq)]
pub struct Gray {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Gray {
    pub fn new(w: u32, h: u32, px: Vec<u8>) -> Option<Self> {
        (w > 0 && h > 0 && px.len() == (w * h) as usize).then_some(Self { w, h, px })
    }

    #[inline]
    fn at(&self, x: i64, y: i64) -> f32 {
        self.px[y as usize * self.w as usize + x as usize] as f32 / 255.0
    }

    /// Bilinear sample at texel (x, y), repeating the picture.
    pub fn wrap(&self, x: f32, y: f32) -> f32 {
        let (w, h) = (self.w as i64, self.h as i64);
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let s = |x: i64, y: i64| self.at(x.rem_euclid(w), y.rem_euclid(h));
        let top = s(x0, y0) * (1.0 - tx) + s(x0 + 1, y0) * tx;
        let bottom = s(x0, y0 + 1) * (1.0 - tx) + s(x0 + 1, y0 + 1) * tx;
        top * (1.0 - ty) + bottom * ty
    }

    /// Bilinear sample at (u, v) in 0–1 over the whole picture; 0 outside.
    pub fn clamp(&self, u: f32, v: f32) -> f32 {
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return 0.0;
        }
        let (w, h) = (self.w as i64, self.h as i64);
        let (fx, fy) = (u * self.w as f32 - 0.5, v * self.h as f32 - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let s = |x: i64, y: i64| self.at(x.clamp(0, w - 1), y.clamp(0, h - 1));
        let top = s(x0, y0) * (1.0 - tx) + s(x0 + 1, y0) * tx;
        let bottom = s(x0, y0 + 1) * (1.0 - tx) + s(x0 + 1, y0 + 1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Grain {
    pub tex: std::sync::Arc<Gray>,
    /// canvas pixels per texel
    pub scale: f32,
    /// 0 (no grain) … 1 (the low spots get no paint)
    pub strength: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dual {
    pub tip: std::sync::Arc<Gray>,
    /// the second tip's size, as a fraction of the dab's radius
    pub size: f32,
    /// how far the stamps spread, as a fraction of the dab's radius
    pub scatter: f32,
    /// stamps per dab
    pub count: u32,
}

/// What a stroke's dabs are textured with.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DabStyle {
    pub grain: Option<Grain>,
    pub tip: Option<std::sync::Arc<Gray>>,
    pub dual: Option<Dual>,
}

/// A small integer hash → 0–1 (the dual brush's stamp positions).
fn hash01(mut h: u32) -> f32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h & 0xff_ffff) as f32 / 0xff_ffff as f32
}

impl DabStyle {
    pub fn is_plain(&self) -> bool {
        self.grain.is_none() && self.tip.is_none() && self.dual.is_none()
    }

    /// Where this dab's dual-brush stamps sit: (x, y, radius).
    pub fn stamps(&self, dab: &Dab) -> Vec<(f32, f32, f32)> {
        let Some(d) = &self.dual else {
            return Vec::new();
        };
        let seed = ((dab.x * 8.0) as i32 as u32).wrapping_mul(73_856_093)
            ^ ((dab.y * 8.0) as i32 as u32).wrapping_mul(19_349_663);
        let r = (dab.radius * d.size).max(0.75);
        (0..d.count.max(1))
            .map(|k| {
                let a = hash01(seed.wrapping_add(k * 2 + 1)) * std::f32::consts::TAU;
                let dist = hash01(seed.wrapping_add(k * 2 + 2)).sqrt() * d.scatter * dab.radius;
                (dab.x + a.cos() * dist, dab.y + a.sin() * dist, r)
            })
            .collect()
    }

    /// How much of the dab's paint lands at canvas point (x, y): 0–1.
    pub fn factor(&self, dab: &Dab, stamps: &[(f32, f32, f32)], x: f32, y: f32) -> f32 {
        let mut f = 1.0;
        if let Some(tip) = &self.tip {
            // round brushes still carry an angle (90° by default); it only means
            // something for oval dabs, so only those turn the tip
            let a = if dab.aspect_ratio > 1.01 {
                dab.angle.to_radians()
            } else {
                0.0
            };
            let (cs, sn) = (a.cos(), a.sin());
            let (dx, dy) = (x - dab.x, y - dab.y);
            let r = dab.radius.max(0.1);
            let u = (dx * cs + dy * sn) / r;
            let v = (-dx * sn + dy * cs) * dab.aspect_ratio.max(1.0) / r;
            f *= tip.clamp((u + 1.0) / 2.0, (v + 1.0) / 2.0);
            if f <= 0.0 {
                return 0.0;
            }
        }
        if let Some(d) = &self.dual {
            let mut best = 0.0f32;
            for &(sx, sy, sr) in stamps {
                let (u, v) = ((x - sx) / sr, (y - sy) / sr);
                best = best.max(d.tip.clamp((u + 1.0) / 2.0, (v + 1.0) / 2.0));
            }
            f *= best;
            if f <= 0.0 {
                return 0.0;
            }
        }
        if let Some(g) = &self.grain {
            let s = g.scale.max(0.05);
            let t = g.tex.wrap(x / s, y / s);
            f *= 1.0 - g.strength.clamp(0.0, 1.0) * (1.0 - t);
        }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn dab(x: f32, y: f32, r: f32) -> Dab {
        Dab {
            x,
            y,
            radius: r,
            color: hokusai::color::RgbaF32 {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            opaque: 1.0,
            hardness: 1.0,
            alpha_eraser: 1.0,
            aspect_ratio: 1.0,
            angle: 0.0,
            lock_alpha: 0.0,
            colorize: 0.0,
            posterize: 0.0,
            posterize_num: 0.0,
            paint: 0.0,
            anti_aliasing: 1.0,
        }
    }

    #[test]
    fn grain_repeats_and_scales() {
        // a 2×1 texture: black, white
        let g = Gray::new(2, 1, vec![0, 255]).unwrap();
        assert!(g.wrap(0.5, 0.5) < 0.01);
        assert!(g.wrap(1.5, 0.5) > 0.99);
        assert!(g.wrap(2.5, 0.5) < 0.01, "repeats");
        let s = DabStyle {
            grain: Some(Grain {
                tex: Arc::new(g),
                scale: 10.0,
                strength: 1.0,
            }),
            ..Default::default()
        };
        let d = dab(0.0, 0.0, 5.0);
        assert!(s.factor(&d, &[], 5.0, 5.0) < 0.01);
        assert!(s.factor(&d, &[], 15.0, 5.0) > 0.99);
    }

    #[test]
    fn a_tip_shapes_the_dab_and_turns_with_it() {
        // left half painted, right half empty
        let mut px = vec![0u8; 16];
        for y in 0..4 {
            for x in 0..2 {
                px[y * 4 + x] = 255;
            }
        }
        let tip = Arc::new(Gray::new(4, 4, px).unwrap());
        let s = DabStyle {
            tip: Some(tip),
            ..Default::default()
        };
        let mut d = dab(50.0, 50.0, 10.0);
        assert!(s.factor(&d, &[], 44.0, 50.0) > 0.9, "left");
        assert!(s.factor(&d, &[], 56.0, 50.0) < 0.1, "right");
        assert_eq!(s.factor(&d, &[], 80.0, 50.0), 0.0, "outside the dab");
        d.angle = 180.0;
        assert!(
            s.factor(&d, &[], 56.0, 50.0) < 0.1,
            "a round dab's angle is ignored"
        );
        d.aspect_ratio = 2.0;
        assert!(
            s.factor(&d, &[], 56.0, 50.0) > 0.9,
            "an oval one turns the tip"
        );
    }

    #[test]
    fn dual_stamps_stay_near_the_dab_and_are_repeatable() {
        let s = DabStyle {
            dual: Some(Dual {
                tip: Arc::new(Gray::new(1, 1, vec![255]).unwrap()),
                size: 0.3,
                scatter: 1.0,
                count: 4,
            }),
            ..Default::default()
        };
        let d = dab(100.0, 100.0, 20.0);
        let st = s.stamps(&d);
        assert_eq!(st.len(), 4);
        assert_eq!(st, s.stamps(&d));
        for (x, y, r) in &st {
            assert!(((x - 100.0).powi(2) + (y - 100.0).powi(2)).sqrt() <= 20.01);
            assert!((r - 6.0).abs() < 1e-4);
        }
        // paint only on (or next to) a stamp
        let (sx, sy, _) = st[0];
        assert!(s.factor(&d, &st, sx, sy) > 0.9);
    }
}
