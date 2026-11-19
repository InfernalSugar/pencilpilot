//! Synthetic fixtures generated in code: line-art drawings and "photo-like"
//! shaded references on an 800×600 canvas.
#![allow(dead_code)]

use grader::Gray;

pub const W: u32 = 800;
pub const H: u32 = 600;

pub fn white() -> Gray {
    Gray::new(W, H, vec![255; (W * H) as usize]).unwrap()
}

fn put(g: &mut Gray, x: i32, y: i32, v: u8) {
    if x >= 0 && y >= 0 && (x as u32) < g.width && (y as u32) < g.height {
        let i = y as usize * g.width as usize + x as usize;
        g.pixels[i] = g.pixels[i].min(v);
    }
}

/// Deterministic per-pixel noise in -1..1.
pub fn noise(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393)
        ^ (y as u32).wrapping_mul(668_265_263)
        ^ seed.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 32767.5 - 1.0
}

/// A tiny LCG for random scribbles.
pub struct Lcg(pub u64);
impl Lcg {
    pub fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as f32) / (1u64 << 31) as f32
    }
}

/// Anti-aliased-ish thick line segment.
pub fn line(g: &mut Gray, (x0, y0): (f32, f32), (x1, y1): (f32, f32), width: f32, v: u8) {
    let r = width / 2.0;
    let (minx, maxx) = (x0.min(x1) - r - 1.0, x0.max(x1) + r + 1.0);
    let (miny, maxy) = (y0.min(y1) - r - 1.0, y0.max(y1) + r + 1.0);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len2 = (dx * dx + dy * dy).max(1e-6);
    for y in miny.floor() as i32..=maxy.ceil() as i32 {
        for x in minx.floor() as i32..=maxx.ceil() as i32 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let t = (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0);
            let d = ((px - x0 - t * dx).powi(2) + (py - y0 - t * dy).powi(2)).sqrt();
            if d <= r {
                put(g, x, y, v);
            }
        }
    }
}

/// Polyline through `pts`.
pub fn polyline(g: &mut Gray, pts: &[(f32, f32)], width: f32, closed: bool) {
    for w in pts.windows(2) {
        line(g, w[0], w[1], width, 20);
    }
    if closed && pts.len() > 2 {
        line(g, pts[pts.len() - 1], pts[0], width, 20);
    }
}

/// Ellipse outline with optional radial wobble (amplitude px, frequency).
pub fn ellipse_points(c: (f32, f32), rx: f32, ry: f32, wobble: f32, freq: f32) -> Vec<(f32, f32)> {
    (0..240)
        .map(|i| {
            let a = i as f32 / 240.0 * std::f32::consts::TAU;
            let k = 1.0 + wobble / rx.max(ry) * (freq * a).sin();
            (c.0 + rx * k * a.cos(), c.1 + ry * k * a.sin())
        })
        .collect()
}

pub fn circle(g: &mut Gray, c: (f32, f32), r: f32, width: f32) {
    polyline(g, &ellipse_points(c, r, r, 0.0, 0.0), width, true);
}

/// Photo-like shaded disc: light from top-left, soft texture.
pub fn shaded_disc(g: &mut Gray, c: (f32, f32), r: f32, seed: u32) {
    for y in (c.1 - r) as i32 - 1..=(c.1 + r) as i32 + 1 {
        for x in (c.0 - r) as i32 - 1..=(c.0 + r) as i32 + 1 {
            let (px, py) = (x as f32 + 0.5 - c.0, y as f32 + 0.5 - c.1);
            if px * px + py * py <= r * r {
                let light = ((px + py) / (2.0 * r) + 0.5).clamp(0.0, 1.0);
                let v = 70.0 + 80.0 * light + 6.0 * noise(x, y, seed);
                put(g, x, y, v.clamp(0.0, 255.0) as u8);
            }
        }
    }
}

/// Photo-like thick shaded limb (a capsule).
pub fn shaded_limb(g: &mut Gray, a: (f32, f32), b: (f32, f32), width: f32, seed: u32) {
    let r = width / 2.0;
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = (dx * dx + dy * dy).max(1e-6);
    for y in (a.1.min(b.1) - r - 1.0) as i32..=(a.1.max(b.1) + r + 1.0) as i32 {
        for x in (a.0.min(b.0) - r - 1.0) as i32..=(a.0.max(b.0) + r + 1.0) as i32 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let t = (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0);
            let d = ((px - a.0 - t * dx).powi(2) + (py - a.1 - t * dy).powi(2)).sqrt();
            if d <= r {
                let v = 60.0 + 50.0 * (d / r) + 6.0 * noise(x, y, seed);
                put(g, x, y, v as u8);
            }
        }
    }
}

/// Photo-like shaded filled rectangle.
pub fn shaded_rect(g: &mut Gray, x0: f32, y0: f32, x1: f32, y1: f32, seed: u32) {
    for y in y0 as i32..y1 as i32 {
        for x in x0 as i32..x1 as i32 {
            let t = (y as f32 - y0) / (y1 - y0);
            let v = 80.0 + 60.0 * t + 6.0 * noise(x, y, seed);
            put(g, x, y, v as u8);
        }
    }
}

pub type Segment = ((f32, f32), (f32, f32));

/// Stick figure description.
#[derive(Clone, Copy)]
pub struct Figure {
    pub head_r: f32,
    /// x offset / y offset / uniform scale about (400, 330)
    pub dx: f32,
    pub dy: f32,
    pub scale: f32,
    /// left arm end point (before transform)
    pub left_hand: (f32, f32),
}

impl Default for Figure {
    fn default() -> Self {
        Self {
            head_r: 50.0,
            dx: 0.0,
            dy: 0.0,
            scale: 1.0,
            left_hand: (320.0, 330.0),
        }
    }
}

impl Figure {
    fn tf(&self, p: (f32, f32)) -> (f32, f32) {
        (
            400.0 + (p.0 - 400.0) * self.scale + self.dx,
            330.0 + (p.1 - 330.0) * self.scale + self.dy,
        )
    }

    /// Head centre and the limb segments, before transform.
    fn parts(&self) -> ((f32, f32), Vec<Segment>) {
        let neck = (400.0, 220.0);
        let head = (400.0, 220.0 - self.head_r);
        let hip = (400.0, 390.0);
        let shoulder = (400.0, 260.0);
        (
            head,
            vec![
                (neck, hip),
                (shoulder, self.left_hand),
                (shoulder, (480.0, 330.0)),
                (hip, (340.0, 510.0)),
                (hip, (460.0, 510.0)),
            ],
        )
    }

    pub fn line_art(&self) -> Gray {
        let mut g = white();
        let (head, limbs) = self.parts();
        circle(&mut g, self.tf(head), self.head_r * self.scale, 3.0);
        for (a, b) in limbs {
            line(&mut g, self.tf(a), self.tf(b), 3.0, 20);
        }
        g
    }

    pub fn photo(&self) -> Gray {
        let mut g = white();
        let (head, limbs) = self.parts();
        shaded_disc(&mut g, self.tf(head), self.head_r * self.scale, 1);
        for (i, (a, b)) in limbs.into_iter().enumerate() {
            shaded_limb(
                &mut g,
                self.tf(a),
                self.tf(b),
                14.0 * self.scale,
                i as u32 + 2,
            );
        }
        g
    }
}

/// Letterbox a picture: white bars top and bottom, textured grey band.
pub fn letterboxed_disc() -> Gray {
    let mut g = white();
    for y in 75..525 {
        for x in 0..W as i32 {
            put(&mut g, x, y, (200.0 + 5.0 * noise(x, y, 9)) as u8);
        }
    }
    shaded_disc(&mut g, (400.0, 300.0), 150.0, 3);
    g
}

/// Random scribble: `n` random segments all over the canvas.
pub fn scribble(seed: u64, n: usize) -> Gray {
    let mut g = white();
    let mut r = Lcg(seed);
    let mut p = (r.next() * W as f32, r.next() * H as f32);
    for _ in 0..n {
        let q = (r.next() * W as f32, r.next() * H as f32);
        line(&mut g, p, q, 3.0, 20);
        p = q;
    }
    g
}

/// Smooth value noise in -1..1 with features about `cell` px across.
pub fn value_noise(x: f32, y: f32, cell: f32, seed: u32) -> f32 {
    let (gx, gy) = (x / cell, y / cell);
    let (x0, y0) = (gx.floor(), gy.floor());
    let (tx, ty) = (gx - x0, gy - y0);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(tx), s(ty));
    let n = |dx: i32, dy: i32| noise(x0 as i32 + dx, y0 as i32 + dy, seed);
    let a = n(0, 0) + (n(1, 0) - n(0, 0)) * sx;
    let b = n(0, 1) + (n(1, 1) - n(0, 1)) * sx;
    a + (b - a) * sy
}

/// Photo-like background over the whole canvas: soft light falloff,
/// blotchy mid-scale texture (out-of-focus foliage) and grain.
pub fn textured_background(g: &mut Gray, seed: u32) {
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let (fx, fy) = (x as f32, y as f32);
            let v = 195.0 - 0.03 * fy
                + 14.0 * value_noise(fx, fy, 60.0, seed)
                + 7.0 * value_noise(fx, fy, 14.0, seed + 1)
                + 4.0 * noise(x, y, seed + 2);
            put(g, x, y, v.clamp(0.0, 255.0) as u8);
        }
    }
}

const G0: f32 = 120.0;
const G1: f32 = 40.0;
/// Busy background: many short, mid-contrast strokes all over the canvas
/// (grass, twigs), on top of [`textured_background`].
pub fn busy_background(g: &mut Gray, seed: u64) {
    textured_background(g, seed as u32);
    let mut r = Lcg(seed);
    for _ in 0..260 {
        let p = (r.next() * W as f32, r.next() * H as f32);
        let a = r.next() * std::f32::consts::TAU;
        let len = 15.0 + 30.0 * r.next();
        let q = (p.0 + len * a.cos(), p.1 + len * a.sin());
        line(g, p, q, 2.0, (G0 + G1 * r.next()) as u8);
    }
}

/// Inner detail lines of [`shaded_subject`] (fold / fur / feature lines),
/// as polylines.
pub fn subject_details() -> Vec<Vec<(f32, f32)>> {
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| -> Vec<(f32, f32)> {
        (0..=24)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / 24.0;
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect()
    };
    vec![
        arc(400.0, 300.0, 95.0, 0.3, 1.9),
        arc(400.0, 300.0, 60.0, 3.4, 4.9),
        vec![(340.0, 240.0), (370.0, 250.0), (395.0, 275.0)],
        vec![(430.0, 360.0), (455.0, 330.0), (470.0, 290.0)],
    ]
}

/// Photo-like subject: shaded disc (r 150 at the centre) with darker inner
/// detail lines of moderate contrast (folds, fur), on `g`.
pub fn shaded_subject(g: &mut Gray) {
    shaded_disc(g, (400.0, 300.0), 150.0, 4);
    for d in subject_details() {
        for w in d.windows(2) {
            // darker than the disc around it, but much fainter than its rim
            line(g, w[0], w[1], 5.0, 55);
        }
    }
}

/// A careful line copy of [`shaded_subject`]: outline plus detail lines.
pub fn subject_line_art() -> Gray {
    let mut d = white();
    circle(&mut d, (400.0, 300.0), 150.0, 3.0);
    for p in subject_details() {
        polyline(&mut d, &p, 3.0, false);
    }
    d
}
pub mod fixtures;
