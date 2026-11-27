//! Text for the Text tool, drawn into pixels (coverage) with the app's own font, Inter:
//! the same everywhere, whatever fonts the system has.

use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Weight,
};

const INTER: &[u8] = include_bytes!("../../../../../../crates/ui/fonts/Inter.ttf");

/// Lines of text drawn as coverage (0–255 per pixel).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Drawn {
    /// where the box starts relative to the text's origin (its top left), px
    pub x: i32,
    pub y: i32,
    pub w: usize,
    pub h: usize,
    pub coverage: Vec<u8>,
    /// the caret's place after the last character, from the origin: (x, top, height)
    pub caret: (f32, f32, f32),
}

pub struct Typesetter {
    fonts: FontSystem,
    raster: SwashCache,
    family: String,
}

impl Default for Typesetter {
    fn default() -> Self {
        Typesetter::new()
    }
}

impl Typesetter {
    pub fn new() -> Typesetter {
        let mut db = cosmic_text::fontdb::Database::new();
        db.load_font_data(INTER.to_vec());
        let family = db
            .faces()
            .next()
            .and_then(|f| f.families.first())
            .map(|(n, _)| n.clone())
            .unwrap_or_else(|| "Inter".into());
        Typesetter {
            fonts: FontSystem::new_with_locale_and_db("en-GB".into(), db),
            raster: SwashCache::new(),
            family,
        }
    }

    /// `text` (lines split at '\n') at `size` px.
    pub fn draw(&mut self, text: &str, size: f32) -> Drawn {
        let size = size.max(1.0);
        let lh = (size * 1.25).ceil();
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(size, lh));
        buffer.set_size(None, None);
        let attrs = Attrs::new()
            .family(Family::Name(&self.family))
            .weight(Weight(500));
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        let mut glyphs = Vec::new();
        let mut caret = (0.0, 0.0, lh);
        for run in buffer.layout_runs() {
            caret = (run.line_w, run.line_top, run.line_height);
            for g in run.glyphs {
                glyphs.push(g.physical((0.0, run.line_y), 1.0));
            }
        }
        // lines after a final '\n' (no glyphs, no run)
        let lines = text.split('\n').count();
        let runs = buffer.layout_runs().count();
        if lines > runs {
            caret = (0.0, (lines - 1) as f32 * lh, lh);
        }
        let mut pieces = Vec::new();
        for p in glyphs {
            let Some(img) = self.raster.get_image_uncached(&mut self.fonts, p.cache_key) else {
                continue;
            };
            let (w, h) = (img.placement.width as usize, img.placement.height as usize);
            if w == 0 || h == 0 {
                continue;
            }
            let cov: Vec<u8> = match img.content {
                SwashContent::Mask => img.data.clone(),
                SwashContent::Color => img.data.chunks(4).map(|c| c[3]).collect(),
                SwashContent::SubpixelMask => continue,
            };
            pieces.push((p.x + img.placement.left, p.y - img.placement.top, w, h, cov));
        }
        if pieces.is_empty() {
            return Drawn {
                caret,
                ..Default::default()
            };
        }
        let x0 = pieces.iter().map(|p| p.0).min().unwrap_or(0);
        let y0 = pieces.iter().map(|p| p.1).min().unwrap_or(0);
        let x1 = pieces.iter().map(|p| p.0 + p.2 as i32).max().unwrap_or(0);
        let y1 = pieces.iter().map(|p| p.1 + p.3 as i32).max().unwrap_or(0);
        let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mut coverage = vec![0u8; w * h];
        for (px, py, gw, gh, cov) in pieces {
            for y in 0..gh {
                for x in 0..gw {
                    let i = (py - y0) as usize * w + y * w + (px - x0) as usize + x;
                    coverage[i] = coverage[i].max(cov[y * gw + x]);
                }
            }
        }
        Drawn {
            x: x0,
            y: y0,
            w,
            h,
            coverage,
            caret,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_draws_and_grows_with_size() {
        let mut t = Typesetter::new();
        let a = t.draw("Hello", 24.0);
        assert!(a.w > 30 && a.h > 10, "{}×{}", a.w, a.h);
        let ink = a.coverage.iter().filter(|&&v| v > 128).count();
        assert!(ink > 60, "{ink}");
        let b = t.draw("Hello", 48.0);
        assert!(b.w > a.w * 3 / 2);
        // two lines are taller; the caret goes down a line
        let c = t.draw("Hello\nthere", 24.0);
        assert!(c.h > a.h * 3 / 2);
        assert!(c.caret.1 > 20.0);
        assert!(t.draw("", 24.0).coverage.is_empty());
    }
}
