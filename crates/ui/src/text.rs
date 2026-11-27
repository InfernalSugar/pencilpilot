//! Text: shaped and laid out by cosmic-text (right-to-left and complex scripts, fallback
//! fonts, emoji), each glyph drawn once into the GPU's atlas and then reused.
//!
//! The design's fonts come with the app: Inter for the interface, Bricolage Grotesque for
//! headings. Anything they lack (Arabic, emoji…) falls back to the system's fonts.

use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer, CacheKey, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Weight,
};
use pencilpilot_gpu::{ATLAS_SIZE, Quad, Renderer2d};

const INTER: &[u8] = include_bytes!("../fonts/Inter.ttf");
const BRICOLAGE: &[u8] = include_bytes!("../fonts/BricolageGrotesque.ttf");

/// Which of the design's fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Font {
    /// Inter: the interface
    Body,
    /// Bricolage Grotesque: headings
    Display,
}

/// How a piece of text looks. Sizes are in device pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub font: Font,
    pub size: f32,
    /// 100–900
    pub weight: u16,
    /// straight RGBA, 0–1
    pub color: [f32; 4],
    /// line height as a multiple of the size
    pub line_height: f32,
}

impl Style {
    pub fn body(size: f32, color: [f32; 4]) -> Style {
        Style {
            font: Font::Body,
            size,
            weight: 400,
            color,
            line_height: 1.3,
        }
    }
    pub fn display(size: f32, color: [f32; 4]) -> Style {
        Style {
            font: Font::Display,
            size,
            weight: 700,
            color,
            line_height: 1.1,
        }
    }
    pub fn weight(mut self, w: u16) -> Style {
        self.weight = w;
        self
    }
}

/// Laid-out text: its quads (placed from the origin) and its size.
#[derive(Debug, Clone, Default)]
pub struct Laid {
    pub quads: Vec<Quad>,
    pub width: f32,
    pub height: f32,
}

impl Laid {
    /// The quads moved to (x, y).
    pub fn at(&self, x: f32, y: f32) -> impl Iterator<Item = Quad> + '_ {
        self.quads.iter().map(move |q| q.moved(x, y))
    }
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    left: i32,
    top: i32,
    color: bool,
}

/// Shelves in the atlas, each glyph put on the first that fits.
#[derive(Default)]
struct Shelves {
    shelves: Vec<(u32, u32, u32)>, // (y, height, x used)
    bottom: u32,
}

impl Shelves {
    fn place(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let (w, h) = (w + 1, h + 1); // a pixel between glyphs, so filtering never bleeds
        for s in &mut self.shelves {
            if s.1 >= h && s.1 <= h + 8 && s.2 + w <= ATLAS_SIZE {
                let at = (s.2, s.0);
                s.2 += w;
                return Some(at);
            }
        }
        if self.bottom + h > ATLAS_SIZE || w > ATLAS_SIZE {
            return None;
        }
        self.shelves.push((self.bottom, h, w));
        let at = (0, self.bottom);
        self.bottom += h;
        Some(at)
    }
}

/// Fonts, shaping, and the glyphs already in the atlas.
pub struct Text {
    fonts: FontSystem,
    raster: SwashCache,
    shelves: Shelves,
    glyphs: HashMap<CacheKey, Option<Slot>>,
    body: String,
    display: String,
}

impl Default for Text {
    fn default() -> Self {
        Self::new()
    }
}

impl Text {
    /// The system's fonts and the design's.
    pub fn new() -> Text {
        let mut fonts = FontSystem::new();
        let db = fonts.db_mut();
        let before = db.faces().count();
        db.load_font_data(INTER.to_vec());
        db.load_font_data(BRICOLAGE.to_vec());
        // their family names, as the files give them
        let name = |i: usize| {
            db.faces()
                .nth(i)
                .and_then(|f| f.families.first())
                .map(|(n, _)| n.clone())
        };
        let body = name(before).unwrap_or_else(|| "Inter".into());
        let display = name(before + 1).unwrap_or_else(|| "Bricolage Grotesque".into());
        Text {
            fonts,
            raster: SwashCache::new(),
            shelves: Shelves::default(),
            glyphs: HashMap::new(),
            body,
            display,
        }
    }

    /// Lay `text` out in `style`, wrapped at `max_width` (device pixels; None: a line per
    /// paragraph). New glyphs go into the renderer's atlas.
    pub fn lay_out(
        &mut self,
        r: &mut Renderer2d,
        text: &str,
        style: &Style,
        max_width: Option<f32>,
    ) -> Laid {
        let metrics = Metrics::new(style.size, (style.size * style.line_height).ceil());
        let mut buffer = Buffer::new(&mut self.fonts, metrics);
        buffer.set_size(max_width, None);
        let family = match style.font {
            Font::Body => self.body.clone(),
            Font::Display => self.display.clone(),
        };
        let attrs = Attrs::new()
            .family(Family::Name(&family))
            .weight(Weight(style.weight));
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        let mut out = Laid::default();
        let runs: Vec<_> = buffer
            .layout_runs()
            .map(|run| {
                let glyphs: Vec<_> = run
                    .glyphs
                    .iter()
                    .map(|g| g.physical((0.0, run.line_y), 1.0))
                    .collect();
                (run.line_w, run.line_top + run.line_height, glyphs)
            })
            .collect();
        for (w, bottom, glyphs) in runs {
            out.width = out.width.max(w);
            out.height = out.height.max(bottom);
            for p in glyphs {
                let Some(slot) = self.glyph(r, p.cache_key) else {
                    continue;
                };
                let (x, y) = ((p.x + slot.left) as f32, (p.y - slot.top) as f32);
                let s = ATLAS_SIZE as f32;
                let uv = [
                    slot.x as f32 / s,
                    slot.y as f32 / s,
                    (slot.x + slot.w) as f32 / s,
                    (slot.y + slot.h) as f32 / s,
                ];
                out.quads.push(if slot.color {
                    Quad::image(x, y, slot.w as f32, slot.h as f32, uv, style.color[3])
                } else {
                    Quad::glyph(x, y, slot.w as f32, slot.h as f32, uv, style.color)
                });
            }
        }
        out
    }

    /// A glyph's place in the atlas, drawing it there the first time.
    fn glyph(&mut self, r: &mut Renderer2d, key: CacheKey) -> Option<Slot> {
        if let Some(s) = self.glyphs.get(&key) {
            return *s;
        }
        let slot = self
            .raster
            .get_image_uncached(&mut self.fonts, key)
            .and_then(|img| {
                let (w, h) = (img.placement.width, img.placement.height);
                if w == 0 || h == 0 {
                    return None;
                }
                let (rgba, color) = match img.content {
                    // coverage: white, premultiplied
                    SwashContent::Mask => (
                        img.data
                            .iter()
                            .flat_map(|&a| [a, a, a, a])
                            .collect::<Vec<u8>>(),
                        false,
                    ),
                    // emoji and other colour glyphs: premultiplied here
                    SwashContent::Color => (
                        img.data
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .flat_map(|p| {
                                let a = p[3] as u32;
                                [
                                    (p[0] as u32 * a / 255) as u8,
                                    (p[1] as u32 * a / 255) as u8,
                                    (p[2] as u32 * a / 255) as u8,
                                    p[3],
                                ]
                            })
                            .collect(),
                        true,
                    ),
                    // (never asked for: whole-pixel masks are)
                    SwashContent::SubpixelMask => return None,
                };
                let (x, y) = self.shelves.place(w, h)?;
                r.atlas_put(x, y, w, h, &rgba);
                Some(Slot {
                    x,
                    y,
                    w,
                    h,
                    left: img.placement.left,
                    top: img.placement.top,
                    color,
                })
            });
        self.glyphs.insert(key, slot);
        slot
    }
}
