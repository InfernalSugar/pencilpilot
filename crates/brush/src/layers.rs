//! Layers: properties, blend modes and compositing.
//!
//! Layers store premultiplied linear fix15 RGBA tiles (what the MyPaint engine
//! paints). Compositing happens in sRGB space, like Photoshop and Krita do for
//! 8-bit sRGB documents, so blend modes look the way artists expect.

use std::sync::OnceLock;

use hokusai::color::{linear_to_srgb, srgb_to_linear};
use hokusai::{TilePixels, fix15};

/// How a layer mixes with what's below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Blend {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    Add,
}

impl Blend {
    pub const ALL: [Blend; 7] = [
        Blend::Normal,
        Blend::Multiply,
        Blend::Screen,
        Blend::Overlay,
        Blend::Darken,
        Blend::Lighten,
        Blend::Add,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Blend::Normal => "normal",
            Blend::Multiply => "multiply",
            Blend::Screen => "screen",
            Blend::Overlay => "overlay",
            Blend::Darken => "darken",
            Blend::Lighten => "lighten",
            Blend::Add => "add",
        }
    }

    pub fn parse(s: &str) -> Option<Blend> {
        Blend::ALL.into_iter().find(|b| b.as_str() == s)
    }

    /// Separable blend function B(backdrop, source), both 0–1 sRGB.
    #[inline]
    pub fn mix(self, cb: f32, cs: f32) -> f32 {
        match self {
            Blend::Normal => cs,
            Blend::Multiply => cb * cs,
            Blend::Screen => cb + cs - cb * cs,
            Blend::Overlay => {
                if cb <= 0.5 {
                    2.0 * cb * cs
                } else {
                    1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
                }
            }
            Blend::Darken => cb.min(cs),
            Blend::Lighten => cb.max(cs),
            Blend::Add => (cb + cs).min(1.0),
        }
    }
}

/// Everything about a layer except its pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerProps {
    pub name: String,
    pub visible: bool,
    /// 0–1
    pub opacity: f32,
    pub blend: Blend,
    /// no painting at all
    pub locked: bool,
    /// paint only where the layer already has paint ("preserve transparency")
    pub alpha_lock: bool,
    /// clipping mask: only show where the layer below (the nearest unclipped one) has paint
    pub clip: bool,
    /// holds an imported picture (or paint merged with one): set by the engine only
    pub imported: bool,
    /// an adjustment layer: no pixels of its own, it changes the colours of
    /// everything below it (through its mask and opacity); set by the engine only
    pub adjust: Option<Box<crate::adjust::ColourAdjust>>,
    /// the app's description of the adjustment (curve points etc.), kept as given
    pub adjust_spec: String,
}

impl LayerProps {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            visible: true,
            opacity: 1.0,
            blend: Blend::Normal,
            locked: false,
            alpha_lock: false,
            clip: false,
            imported: false,
            adjust: None,
            adjust_spec: String::new(),
        }
    }
}

/// fix15 linear (0..=32768) → sRGB 0–1, as a table (compositing runs per pixel per layer).
fn lut() -> &'static [f32] {
    static LUT: OnceLock<Vec<f32>> = OnceLock::new();
    LUT.get_or_init(|| {
        (0..=fix15::FIX15_ONE)
            .map(|i| linear_to_srgb(i as f32 / fix15::FIX15_ONE as f32).clamp(0.0, 1.0))
            .collect()
    })
}

/// A premultiplied fix15 pixel as (unpremultiplied sRGB colour, alpha), alpha 0–1.
#[inline]
pub fn unpremul_srgb(p: [u16; 4]) -> ([f32; 3], f32) {
    let a = p[3] as u32;
    if a == 0 {
        return ([0.0; 3], 0.0);
    }
    let t = lut();
    let ch = |c: u16| t[((c as u32 * fix15::FIX15_ONE) / a).min(fix15::FIX15_ONE) as usize];
    (
        [ch(p[0]), ch(p[1]), ch(p[2])],
        a as f32 / fix15::FIX15_ONE as f32,
    )
}

/// Inverse of [`unpremul_srgb`].
pub fn premul_fix15(c: [f32; 3], a: f32) -> [u16; 4] {
    let a = a.clamp(0.0, 1.0);
    if a <= 0.0 {
        return [0; 4];
    }
    let f =
        |v: f32| (srgb_to_linear(v.clamp(0.0, 1.0)) * a * fix15::FIX15_ONE as f32).round() as u16;
    [
        f(c[0]),
        f(c[1]),
        f(c[2]),
        (a * fix15::FIX15_ONE as f32).round() as u16,
    ]
}

/// One visible layer's contribution to one tile.
pub struct TileSource<'a> {
    /// `None`: this layer has nothing in this tile
    pub tile: Option<&'a TilePixels>,
    pub opacity: f32,
    pub blend: Blend,
    pub visible: bool,
    pub clip: bool,
    /// the layer mask here: `None` = no mask; `Some(None)` = a mask with nothing
    /// painted in this tile (fully shown)
    pub mask: Option<Option<&'a TilePixels>>,
    /// an adjustment layer's change (it has no pixels of its own)
    pub adjust: Option<&'a crate::adjust::ColourAdjust>,
}

/// Composite the sources (bottom → top) for one pixel over white paper → sRGB8.
#[inline]
pub fn composite_pixel(sources: &[TileSource<'_>], row: usize, col: usize) -> [u8; 3] {
    composite_pixel_on(sources, row, col, [1.0; 3])
}

/// Composite the sources for one pixel over a paper of colour `paper` (sRGB, 0–1).
#[inline]
pub fn composite_pixel_on(
    sources: &[TileSource<'_>],
    row: usize,
    col: usize,
    paper: [f32; 3],
) -> [u8; 3] {
    let mut dst = paper;
    // alpha of the nearest unclipped layer: what clipped layers above it show through
    let mut base = 0.0f32;
    for s in sources {
        if let Some(adj) = s.adjust {
            let m = match s.mask {
                Some(Some(t)) => mask_value(t[row][col]),
                _ => 1.0,
            };
            let a = s.opacity * m * if s.clip { base } else { 1.0 };
            if s.visible && a > 0.0 {
                let c = crate::adjust::colour_op(adj, dst);
                for i in 0..3 {
                    dst[i] += (c[i] - dst[i]) * a;
                }
            }
            continue;
        }
        let (cs, a) = s
            .tile
            .map_or(([0.0; 3], 0.0), |t| unpremul_srgb(t[row][col]));
        let m = match s.mask {
            Some(Some(t)) => mask_value(t[row][col]),
            _ => 1.0,
        };
        let mut a = a * s.opacity * m;
        if s.clip {
            a *= base;
        } else {
            base = if s.visible { a } else { 0.0 };
        }
        if !s.visible || a <= 0.0 {
            continue;
        }
        for i in 0..3 {
            let b = s.blend.mix(dst[i], cs[i]);
            dst[i] += (b - dst[i]) * a;
        }
    }
    dst.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// How much a layer mask pixel lets through (0–1): white paint shows, black
/// hides, greys in between; unpainted mask (transparent) shows everything.
pub fn mask_value(px: [u16; 4]) -> f32 {
    let (c, a) = unpremul_srgb(px);
    let lum = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    (1.0 - a + a * lum).clamp(0.0, 1.0)
}

/// W3C compositing of `src` (with extra `opacity` and `blend`) onto a possibly
/// transparent `dst`, both premultiplied fix15. Used by "merge down".
pub fn merge_pixel(dst: [u16; 4], src: [u16; 4], opacity: f32, blend: Blend) -> [u16; 4] {
    let (cs, a_s) = unpremul_srgb(src);
    let a_s = a_s * opacity;
    if a_s <= 0.0 {
        return dst;
    }
    let (cb, a_b) = unpremul_srgb(dst);
    let a_o = a_s + a_b * (1.0 - a_s);
    let mut co = [0.0f32; 3];
    for i in 0..3 {
        // premultiplied result, then back to straight colour
        let p = a_s * (1.0 - a_b) * cs[i]
            + a_s * a_b * blend.mix(cb[i], cs[i])
            + (1.0 - a_s) * a_b * cb[i];
        co[i] = p / a_o;
    }
    premul_fix15(co, a_o)
}

/// Minimal JSON string escaping (layer names come from people).
pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_names_round_trip() {
        for b in Blend::ALL {
            assert_eq!(Blend::parse(b.as_str()), Some(b));
        }
        assert_eq!(Blend::parse("nope"), None);
    }

    #[test]
    fn premul_round_trip() {
        for (c, a) in [([0.2, 0.5, 0.9], 1.0), ([0.7, 0.1, 0.3], 0.5)] {
            let (back, ab) = unpremul_srgb(premul_fix15(c, a));
            assert!((ab - a).abs() < 1e-3);
            for i in 0..3 {
                assert!((back[i] - c[i]).abs() < 0.01, "{back:?} vs {c:?}");
            }
        }
    }

    #[test]
    fn merging_onto_empty_keeps_the_source() {
        let src = premul_fix15([0.2, 0.4, 0.6], 0.8);
        for b in Blend::ALL {
            // a transparent backdrop shows the source unchanged, whatever the mode
            let m = merge_pixel([0; 4], src, 1.0, b);
            let (c, a) = unpremul_srgb(m);
            assert!((a - 0.8).abs() < 1e-3, "{b:?}");
            assert!((c[0] - 0.2).abs() < 0.01, "{b:?}");
        }
    }

    #[test]
    fn multiply_darkens_screen_lightens() {
        assert!(Blend::Multiply.mix(0.5, 0.5) < 0.5);
        assert!(Blend::Screen.mix(0.5, 0.5) > 0.5);
        assert_eq!(Blend::Add.mix(0.8, 0.8), 1.0);
    }

    #[test]
    fn json_escaping() {
        assert_eq!(json_str("a\"b\\c\n"), r#""a\"b\\c\u000a""#);
    }
}
