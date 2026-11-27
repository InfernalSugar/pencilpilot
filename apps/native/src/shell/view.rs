//! How the document sits in the canvas card: zoom, pan, rotation and flip.
//!
//! A document point `d` shows at
//!   `card centre + pan + R(angle) · F · (d − doc centre) · zoom`
//! where F mirrors x when flipped. Screen positions are the interface's logical pixels;
//! document positions are the document's pixels.

use pencilpilot_ui::ui::Rect;

/// The document's size when none is open (an HD cut).
pub const DOC_W: u32 = 1920;
pub const DOC_H: u32 = 1080;
pub const ZOOM_MIN: f32 = 0.02;
pub const ZOOM_MAX: f32 = 64.0;
/// Room left around the document when it's fitted to the card.
const FIT_MARGIN: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub doc_w: u32,
    pub doc_h: u32,
    /// logical pixels per document pixel
    pub zoom: f32,
    /// radians, clockwise on screen
    pub angle: f32,
    pub flip: bool,
    /// the document centre's offset from the card's centre, logical pixels
    pub pan: (f32, f32),
    /// follows the card's size (until the view is zoomed, panned or turned by hand)
    pub fitted: bool,
}

impl Default for View {
    fn default() -> View {
        View {
            doc_w: DOC_W,
            doc_h: DOC_H,
            zoom: 1.0,
            angle: 0.0,
            flip: false,
            pan: (0.0, 0.0),
            fitted: true,
        }
    }
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

impl View {
    /// The zoom that shows the whole document in `card`.
    pub fn fit_zoom(&self, card: Rect) -> f32 {
        let w = (card.w - 2.0 * FIT_MARGIN).max(1.0);
        let h = (card.h - 2.0 * FIT_MARGIN).max(1.0);
        (w / self.doc_w as f32).min(h / self.doc_h as f32)
    }

    /// Back to the whole document, upright, unflipped, centred.
    pub fn reset(&mut self, card: Rect) {
        self.zoom = self.fit_zoom(card);
        self.angle = 0.0;
        self.flip = false;
        self.pan = (0.0, 0.0);
        self.fitted = true;
    }

    /// Keeps a fitted view fitted when the card changes size.
    pub fn follow(&mut self, card: Rect) {
        if self.fitted {
            self.zoom = self.fit_zoom(card);
            self.pan = (0.0, 0.0);
        }
    }

    fn rot(&self) -> (f32, f32) {
        (self.angle.cos(), self.angle.sin())
    }

    /// Where a document point shows (logical pixels).
    pub fn to_screen(&self, card: Rect, dx: f32, dy: f32) -> (f32, f32) {
        let (cx, cy) = centre(card);
        let mut vx = (dx - self.doc_w as f32 / 2.0) * self.zoom;
        let vy = (dy - self.doc_h as f32 / 2.0) * self.zoom;
        if self.flip {
            vx = -vx;
        }
        let (c, s) = self.rot();
        (
            cx + self.pan.0 + c * vx - s * vy,
            cy + self.pan.1 + s * vx + c * vy,
        )
    }

    /// The document point under a screen point (logical pixels).
    pub fn to_doc(&self, card: Rect, sx: f32, sy: f32) -> (f32, f32) {
        let (cx, cy) = centre(card);
        let (ox, oy) = (sx - cx - self.pan.0, sy - cy - self.pan.1);
        let (c, s) = self.rot();
        let mut vx = (c * ox + s * oy) / self.zoom;
        let vy = (-s * ox + c * oy) / self.zoom;
        if self.flip {
            vx = -vx;
        }
        (vx + self.doc_w as f32 / 2.0, vy + self.doc_h as f32 / 2.0)
    }

    /// The document's size on screen of one document pixel (for brush previews etc.).
    pub fn screen_len(&self, doc_len: f32) -> f32 {
        doc_len * self.zoom
    }

    /// Moves the view so document point `d` shows at screen point `at`.
    fn pin(&mut self, card: Rect, d: (f32, f32), at: (f32, f32)) {
        let now = self.to_screen(card, d.0, d.1);
        self.pan.0 += at.0 - now.0;
        self.pan.1 += at.1 - now.1;
    }

    /// Zooms by `factor`, keeping what's under `at` (logical pixels) in place.
    pub fn zoom_at(&mut self, card: Rect, at: (f32, f32), factor: f32) {
        let d = self.to_doc(card, at.0, at.1);
        self.zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        self.pin(card, d, at);
        self.fitted = false;
    }

    /// Sets the zoom (1.0 = 100 %), about the card's centre.
    pub fn set_zoom(&mut self, card: Rect, zoom: f32) {
        let c = centre(card);
        self.zoom_at(card, c, zoom / self.zoom);
    }

    /// Turns by `radians` about screen point `at`.
    pub fn rotate_about(&mut self, card: Rect, at: (f32, f32), radians: f32) {
        let d = self.to_doc(card, at.0, at.1);
        self.angle = (self.angle + radians).rem_euclid(std::f32::consts::TAU);
        self.pin(card, d, at);
        self.fitted = false;
    }

    /// Mirrors the view left to right about the card's centre.
    pub fn flip_view(&mut self, card: Rect) {
        let c = centre(card);
        let d = self.to_doc(card, c.0, c.1);
        self.flip = !self.flip;
        // a mirrored view turns the other way
        self.angle = (-self.angle).rem_euclid(std::f32::consts::TAU);
        self.pin(card, d, c);
    }

    /// Moves the view by (dx, dy) logical pixels.
    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.pan.0 += dx;
        self.pan.1 += dy;
        self.fitted = false;
    }

    /// For the GPU: the frame pixel → document pixel map, as `[a, b, c, d, tx, ty]` with
    /// `doc = (a·x + b·y + tx, c·x + d·y + ty)`; `scale` is frame pixels per logical pixel.
    pub fn inverse_affine(&self, card: Rect, scale: f32) -> [f32; 6] {
        let (c, s) = self.rot();
        let k = 1.0 / (self.zoom * scale);
        let f = if self.flip { -1.0 } else { 1.0 };
        // logical offset o = p/scale − centre − pan; v = R⁻¹ o / zoom; v.x *= f
        let (a, b) = (f * c * k, f * s * k);
        let (cc, d) = (-s * k, c * k);
        let (cx, cy) = centre(card);
        let (ox, oy) = ((cx + self.pan.0) * scale, (cy + self.pan.1) * scale);
        let tx = self.doc_w as f32 / 2.0 - (a * ox + b * oy);
        let ty = self.doc_h as f32 / 2.0 - (cc * ox + d * oy);
        [a, b, cc, d, tx, ty]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: Rect = Rect {
        x: 100.0,
        y: 80.0,
        w: 1000.0,
        h: 600.0,
    };

    fn near(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-2 && (a.1 - b.1).abs() < 1e-2
    }

    fn views() -> Vec<View> {
        let mut v = View::default();
        v.reset(CARD);
        let mut out = vec![v];
        v.zoom_at(CARD, (300.0, 200.0), 3.0);
        out.push(v);
        v.rotate_about(CARD, (700.0, 400.0), 0.7);
        out.push(v);
        v.flip_view(CARD);
        out.push(v);
        v.pan_by(-40.0, 25.0);
        out.push(v);
        out
    }

    #[test]
    fn screen_and_document_are_inverses() {
        for v in views() {
            for d in [(0.0, 0.0), (1920.0, 1080.0), (333.0, 777.0)] {
                let s = v.to_screen(CARD, d.0, d.1);
                assert!(near(v.to_doc(CARD, s.0, s.1), d), "{v:?} {d:?}");
            }
        }
    }

    #[test]
    fn the_gpu_map_matches_to_doc() {
        for v in views() {
            for scale in [1.0, 1.5, 2.0] {
                let m = v.inverse_affine(CARD, scale);
                for p in [(150.0, 90.0), (640.0, 380.0), (1050.0, 650.0)] {
                    let (x, y) = (p.0 * scale, p.1 * scale);
                    let got = (m[0] * x + m[1] * y + m[4], m[2] * x + m[3] * y + m[5]);
                    assert!(near(got, v.to_doc(CARD, p.0, p.1)), "{v:?} {scale}");
                }
            }
        }
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let mut v = View::default();
        v.reset(CARD);
        let at = (321.0, 456.0);
        let before = v.to_doc(CARD, at.0, at.1);
        v.zoom_at(CARD, at, 4.0);
        assert!(near(v.to_doc(CARD, at.0, at.1), before));
        v.rotate_about(CARD, at, 1.1);
        assert!(near(v.to_doc(CARD, at.0, at.1), before));
        assert!(!v.fitted);
    }

    #[test]
    fn fitted_shows_the_whole_document_centred() {
        let mut v = View::default();
        v.reset(CARD);
        let tl = v.to_screen(CARD, 0.0, 0.0);
        let br = v.to_screen(CARD, 1920.0, 1080.0);
        assert!(tl.0 >= CARD.x && tl.1 >= CARD.y && br.0 <= CARD.right() && br.1 <= CARD.bottom());
        assert!(near(
            ((tl.0 + br.0) / 2.0, (tl.1 + br.1) / 2.0),
            (600.0, 380.0)
        ));
        // flipped, the left edge shows on the right
        v.flip_view(CARD);
        assert!(v.to_screen(CARD, 0.0, 540.0).0 > 600.0);
    }
}
