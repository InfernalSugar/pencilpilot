//! Strands: one stroke paints a bundle of thin, tapered, roughly parallel lines that
//! follow it (hair, fur, grass, wood grain).
//!
//! Each strand is the brush's own stroke with its own brush state, offset sideways
//! (perpendicular to the way the stroke is going) by its own amount across the
//! bundle's width. The width is a multiple of the brush's radius and grows with
//! pressure. A slow wobble, from a seeded random, bends each strand a little, and each
//! starts and ends thinner over its own lengths, so the tips don't line up.
//!
//! The thinner ends: dabs are scaled down as they're drawn (see
//! `UndoSurface::draw_dab`). To know where a stroke ends, a strand trails the pen by
//! its longest end taper: the points are kept until the pen is that far past them,
//! and the rest are drawn, tapering, when the stroke ends.
//!
//! Everything comes from the points and a seed, so a line stroke replayed from its
//! points paints the same pixels (see [`crate::lines`]). With symmetry, every copy of
//! the stroke is a bundle of its own (seeded as symmetry seeds its copies).

use std::collections::VecDeque;

use hokusai::{Brush, BrushSetting, BrushState};

use crate::UndoSurface;

/// The most strands one stroke paints (each is a whole brush stroke of dabs).
pub const MAX_STRANDS: u32 = 24;

/// How a brush paints in strands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strands {
    /// strands per stroke, 2–[`MAX_STRANDS`]
    pub count: u32,
    /// the bundle's width at full pressure, as a multiple of the brush's radius
    pub spread: f32,
    /// how much each strand bends and strays from its place, 0–1
    pub wobble: f32,
    /// how long the thin ends are, 0 (none) – 1
    pub taper: f32,
}

impl Strands {
    /// Settings checked and clamped; `None` for fewer than two strands (a plain stroke).
    pub fn new(count: u32, spread: f32, wobble: f32, taper: f32) -> Option<Self> {
        let ok = |v: f32, d: f32| if v.is_finite() { v } else { d };
        (count >= 2).then(|| Self {
            count: count.min(MAX_STRANDS),
            spread: ok(spread, 6.0).clamp(0.5, 40.0),
            wobble: ok(wobble, 0.0).clamp(0.0, 1.0),
            taper: ok(taper, 0.0).clamp(0.0, 1.0),
        })
    }

    /// How far from the stroke's path a strand's centre can go, as a multiple of the
    /// brush's radius (add the brush's own reach for where its dabs paint).
    pub fn reach(&self) -> f32 {
        self.spread * (0.5 + 0.6 * self.wobble)
    }
}

/// A small integer hash → 0–1.
fn hash01(a: u32, b: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9e37_79b9) ^ b.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h & 0xff_ffff) as f32 / 0xff_ffff as f32
}

/// The thin ends: from 0 at the tip to 1, quickly at first (a sharp point).
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * (2.0 - t)
}

/// How much a dab is scaled where a strand is tapering: along the segment the strand
/// just moved over, from `s0` at its start to `s1` at its end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Taper {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub s0: f32,
    pub s1: f32,
}

impl Taper {
    /// The scale at dab (x, y): its place along the segment, clamped to the ends.
    pub fn at(&self, x: f32, y: f32) -> f32 {
        if self.s0 == self.s1 {
            return self.s1;
        }
        let (dx, dy) = (self.to.0 - self.from.0, self.to.1 - self.from.1);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 1e-6 {
            (((x - self.from.0) * dx + (y - self.from.1) * dy) / len2).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.s0 + (self.s1 - self.s0) * t
    }
}

/// One strand of a bundle.
struct Strand {
    state: BrushState,
    /// its place across the bundle, −1 (one edge) … 1 (the other)
    place: f32,
    /// its wobble: two slow waves (radians per px of path, and phases)
    freq: (f32, f32),
    phase: (f32, f32),
    /// random 0–1 numbers for its taper lengths and how late it starts / early it ends
    r_in: f32,
    r_out: f32,
    r_late: f32,
    r_early: f32,
    /// where it was last drawn to, and its scale there
    last: Option<((f32, f32), f32)>,
}

/// A point of the stroke waiting to be drawn: x, y, pressure, x tilt, y tilt,
/// seconds; how far along the path it is; the way the stroke goes there.
struct Sample {
    p: [f32; 6],
    dist: f32,
    dir: Option<(f32, f32)>,
}

/// Lengths that follow from the brush's size (taken at the stroke's first point).
#[derive(Clone, Copy)]
struct Sized {
    /// the brush's radius (px)
    radius: f32,
    /// the base taper length (px)
    taper: f32,
    /// how far the pen leads the strands (the longest end taper and early end)
    tail: f32,
}

/// One stroke painted as strands (live, or replayed from its points).
pub(crate) struct StrandPen {
    cfg: Strands,
    strands: Vec<Strand>,
    queue: VecDeque<Sample>,
    /// the last point given, and the path's length there
    last: Option<(f32, f32)>,
    dist: f32,
    /// the way the stroke is going (smoothed); None until it moves
    dir: Option<(f32, f32)>,
    sized: Option<Sized>,
}

impl StrandPen {
    pub fn new(cfg: Strands, seed: u32) -> Self {
        let n = cfg.count.clamp(2, MAX_STRANDS);
        let strands = (0..n)
            .map(|j| {
                let r = |k: u32| hash01(seed, j.wrapping_mul(16).wrapping_add(k));
                let even = 2.0 * j as f32 / (n - 1) as f32 - 1.0;
                // (each strand strays a little from its even place)
                let stray = (r(0) - 0.5) * cfg.wobble * 0.7 / (n - 1) as f32;
                let wave = |k: u32| std::f32::consts::TAU / (60.0 + 160.0 * r(k));
                Strand {
                    state: BrushState::new(seed.wrapping_mul(31).wrapping_add(j + 1)),
                    place: (even + stray * 2.0).clamp(-1.2, 1.2),
                    freq: (wave(1), wave(2)),
                    phase: (r(3) * std::f32::consts::TAU, r(4) * std::f32::consts::TAU),
                    r_in: r(5),
                    r_out: r(6),
                    r_late: r(7),
                    r_early: r(8),
                    last: None,
                }
            })
            .collect();
        Self {
            cfg,
            strands,
            queue: VecDeque::new(),
            last: None,
            dist: 0.0,
            dir: None,
            sized: None,
        }
    }

    fn sized(&mut self, brush: &Brush) -> Sized {
        *self.sized.get_or_insert_with(|| {
            let radius = brush
                .get(BrushSetting::Radius)
                .base_value
                .exp()
                .clamp(0.2, 1000.0);
            let width = self.cfg.spread * radius;
            let taper = self.cfg.taper * (8.0 * radius + 1.5 * width);
            // (an end taper is 0.6–1.4 of the base, and a strand ends up to 0.35 early)
            let tail = taper * (1.4 + 0.35);
            Sized {
                radius,
                taper,
                tail,
            }
        })
    }

    /// One point of the stroke (6 numbers, as line strokes keep them).
    pub fn stroke_to(&mut self, brush: &Brush, s: &mut UndoSurface, p: &[f32]) {
        let Some(p) = p.get(..6) else {
            return;
        };
        let p: [f32; 6] = p.try_into().unwrap_or([0.0; 6]);
        let sz = self.sized(brush);
        if let Some((lx, ly)) = self.last {
            let (dx, dy) = (p[0] - lx, p[1] - ly);
            let len = (dx * dx + dy * dy).sqrt();
            if len > 1e-4 {
                self.dist += len;
                let (ux, uy) = (dx / len, dy / len);
                self.dir = Some(match self.dir {
                    None => (ux, uy),
                    Some((ox, oy)) => {
                        // smoothed over about two brush widths of path
                        let a = (len / (4.0 * sz.radius).max(4.0)).min(1.0);
                        let (vx, vy) = (ox * (1.0 - a) + ux * a, oy * (1.0 - a) + uy * a);
                        let l = (vx * vx + vy * vy).sqrt();
                        if l > 1e-4 { (vx / l, vy / l) } else { (ux, uy) }
                    }
                });
            }
        }
        self.last = Some((p[0], p[1]));
        self.queue.push_back(Sample {
            p,
            dist: self.dist,
            dir: self.dir,
        });
        // the points far enough behind the pen can't be in the end taper
        while self
            .queue
            .front()
            .is_some_and(|q| self.dist - q.dist >= sz.tail)
        {
            if let Some(q) = self.queue.pop_front() {
                self.draw(brush, s, &q, None);
            }
        }
    }

    /// The stroke ended: the points still waiting are drawn (their strands tapering
    /// off), then every strand finished.
    pub fn finish(&mut self, brush: &Brush, s: &mut UndoSurface) {
        let total = self.dist;
        while let Some(q) = self.queue.pop_front() {
            self.draw(brush, s, &q, Some(total));
        }
        for st in self.strands.iter_mut() {
            // (a brush that trails the pen catches up now: as thin as the strand ended)
            s.taper = st.last.map(|(at, k)| Taper {
                from: at,
                to: at,
                s0: k,
                s1: k,
            });
            brush.finish_stroke(&mut st.state, s);
        }
        s.taper = None;
    }

    /// Draw one point into every strand (`end`: the stroke's length, once it's known).
    fn draw(&mut self, brush: &Brush, s: &mut UndoSurface, q: &Sample, end: Option<f32>) {
        let sz = self.sized(brush);
        // (a point before the stroke moved goes the way it went first; a dot, across)
        let (dx, dy) = q.dir.or(self.dir).unwrap_or((1.0, 0.0));
        let (nx, ny) = (-dy, dx);
        let [x, y, pressure, xt, yt, dt] = q.p;
        let width = self.cfg.spread * sz.radius * (0.3 + 0.7 * pressure.clamp(0.0, 1.0));
        let half = width * 0.5;
        let slot = width / (self.strands.len() - 1) as f32;
        let wobble = self.cfg.wobble;
        let base = sz.taper;
        for st in self.strands.iter_mut() {
            let wave = 0.6 * (q.dist * st.freq.0 + st.phase.0).sin()
                + 0.4 * (q.dist * st.freq.1 + st.phase.1).sin();
            let off = st.place * half + wobble * 0.6 * slot * wave;
            let at = (x + nx * off, y + ny * off);
            let mut scale = 1.0f32;
            if base > 0.0 {
                let late = base * 0.35 * st.r_late;
                let len_in = base * (0.6 + 0.8 * st.r_in);
                scale = scale.min(ease((q.dist - late) / len_in));
                if let Some(total) = end {
                    let early = base * 0.35 * st.r_early;
                    let len_out = base * (0.6 + 0.8 * st.r_out);
                    scale = scale.min(ease((total - early - q.dist) / len_out));
                }
            }
            let (from, s0) = st.last.unwrap_or((at, scale));
            s.taper = (s0 < 1.0 || scale < 1.0).then_some(Taper {
                from,
                to: at,
                s0,
                s1: scale,
            });
            brush.stroke_to(&mut st.state, s, at.0, at.1, pressure, xt, yt, dt as f64);
            st.last = Some((at, scale));
        }
        s.taper = None;
    }
}

/// A brush state for a stroke: plain, or a bundle of strands.
// (a plain pen is the common case: kept unboxed)
#[allow(clippy::large_enum_variant)]
pub(crate) enum Pen {
    Plain(BrushState),
    Strands(Box<StrandPen>),
}

impl Pen {
    pub fn new(seed: u32, strands: Option<Strands>) -> Self {
        match strands {
            Some(cfg) => Pen::Strands(Box::new(StrandPen::new(cfg, seed))),
            None => Pen::Plain(BrushState::new(seed)),
        }
    }

    /// One point (6 numbers: x, y, pressure, x tilt, y tilt, seconds).
    pub fn stroke_to(&mut self, brush: &Brush, s: &mut UndoSurface, p: &[f32]) {
        match self {
            Pen::Plain(st) => {
                brush.stroke_to(st, s, p[0], p[1], p[2], p[3], p[4], p[5] as f64);
            }
            Pen::Strands(sp) => sp.stroke_to(brush, s, p),
        }
    }

    pub fn finish(&mut self, brush: &Brush, s: &mut UndoSurface) {
        match self {
            Pen::Plain(st) => {
                brush.finish_stroke(st, s);
            }
            Pen::Strands(sp) => sp.finish(brush, s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Painter, Symmetry, SymmetryMode, set_brush_rgb};
    use hokusai::TILE_SIZE;

    /// a hard round pen, `log_radius` its radius's logarithm
    fn pen(log_radius: f32) -> Brush {
        let mut b = Brush::new();
        b.get_mut(BrushSetting::Radius).base_value = log_radius;
        b.get_mut(BrushSetting::Opaque).base_value = 1.0;
        b.get_mut(BrushSetting::Hardness).base_value = 0.9;
        b.get_mut(BrushSetting::DabsPerActualRadius).base_value = 3.0;
        set_brush_rgb(&mut b, 10, 10, 10);
        b
    }

    /// a straight stroke from (x0, y) to (x1, y), at `pressure` all the way
    fn line(x0: f32, x1: f32, y: f32, pressure: f32) -> Vec<f32> {
        (0..=120)
            .flat_map(|i| {
                let t = i as f32 / 120.0;
                [x0 + (x1 - x0) * t, y, pressure, 0.0, 0.0, 0.01]
            })
            .collect()
    }

    fn draw(p: &mut Painter, b: &Brush, pts: &[f32]) {
        assert!(p.begin_stroke());
        for q in pts.as_chunks::<6>().0 {
            p.stroke_to(b, q[0], q[1], q[2], q[3], q[4], q[5] as f64);
        }
        p.end_stroke(b);
    }

    /// the active layer's alpha at (x, y)
    fn alpha(p: &Painter, x: i32, y: i32) -> u16 {
        let ts = TILE_SIZE as i32;
        let l = &p.layers[p.index_of(p.active).unwrap()];
        l.surface
            .tiles
            .get(&(x.div_euclid(ts), y.div_euclid(ts)))
            .map_or(0, |t| {
                t[y.rem_euclid(ts) as usize][x.rem_euclid(ts) as usize][3]
            })
    }

    const INKED: u16 = 1 << 12;

    /// the inked runs down column `x` (y0..y1): how many, and their total ink
    fn column(p: &Painter, x: i32, y0: i32, y1: i32) -> (usize, u64) {
        let (mut runs, mut ink, mut inside) = (0, 0u64, false);
        for y in y0..y1 {
            let a = alpha(p, x, y);
            ink += a as u64;
            let on = a > INKED;
            if on && !inside {
                runs += 1;
            }
            inside = on;
        }
        (runs, ink)
    }

    #[test]
    fn settings_are_checked() {
        assert_eq!(Strands::new(1, 6.0, 0.5, 0.5), None);
        let s = Strands::new(99, f32::NAN, 7.0, -1.0).unwrap();
        assert_eq!(s.count, MAX_STRANDS);
        assert_eq!((s.spread, s.wobble, s.taper), (6.0, 1.0, 0.0));
        assert!(s.reach() >= s.spread * 0.5);
    }

    #[test]
    fn strands_paint_separate_lines_across_the_stroke() {
        let b = pen(0.4);
        let r = 0.4f32.exp();
        for n in [3u32, 5, 8] {
            let mut p = Painter::new(300, 200);
            p.set_strands(n, 40.0, 0.0, 0.0);
            draw(&mut p, &b, &line(30.0, 270.0, 100.0, 1.0));
            for x in [80, 150, 220] {
                let (runs, _) = column(&p, x, 40, 160);
                assert_eq!(runs, n as usize, "{n} strands at x {x}");
            }
            // the bundle is about as wide as asked (40 × the radius)
            let ys: Vec<i32> = (0..200).filter(|&y| alpha(&p, 150, y) > INKED).collect();
            let w = (ys.last().unwrap() - ys.first().unwrap()) as f32;
            assert!((w - 40.0 * r).abs() < 2.0 * r + 2.0, "width {w}");
        }
    }

    #[test]
    fn one_strand_is_the_plain_brush() {
        let b = pen(0.8);
        let pts = line(30.0, 270.0, 100.0, 0.8);
        let mut plain = Painter::new(300, 200);
        draw(&mut plain, &b, &pts);
        let mut one = Painter::new(300, 200);
        one.set_strands(1, 20.0, 1.0, 1.0);
        assert_eq!(one.strands(), None);
        draw(&mut one, &b, &pts);
        assert_eq!(plain.layers[0].surface.tiles, one.layers[0].surface.tiles);
        // and strands off again after a bundle: plain
        let mut again = Painter::new(300, 200);
        again.set_strands(6, 20.0, 1.0, 1.0);
        again.set_strands(0, 20.0, 1.0, 1.0);
        draw(&mut again, &b, &pts);
        assert_eq!(plain.layers[0].surface.tiles, again.layers[0].surface.tiles);
    }

    #[test]
    fn the_taper_makes_the_ends_thinner() {
        let mut b = pen(1.1);
        // (a brush that trails the pen still ends thin)
        b.get_mut(BrushSetting::SlowTracking).base_value = 1.2;
        let pts = line(20.0, 280.0, 100.0, 1.0);
        let ends = |taper: f32| {
            let mut p = Painter::new(300, 200);
            p.set_strands(2, 8.0, 0.0, taper);
            draw(&mut p, &b, &pts);
            let ink = |x| column(&p, x, 40, 160).1;
            (ink(26), ink(150), ink(274).max(ink(279)))
        };
        let (start, middle, end) = ends(1.0);
        assert!(middle > 0);
        assert!(start * 3 < middle, "start {start} vs middle {middle}");
        assert!(end * 3 < middle, "end {end} vs middle {middle}");
        // without a taper the ends are as full as the middle
        let (start, middle, end) = ends(0.0);
        assert!(start * 10 > middle * 8 && end * 10 > middle * 8);
    }

    #[test]
    fn the_tips_do_not_line_up() {
        let b = pen(0.6);
        let mut p = Painter::new(300, 200);
        p.set_strands(6, 30.0, 0.3, 1.0);
        draw(&mut p, &b, &line(20.0, 280.0, 100.0, 1.0));
        // where each row's ink starts: not all at one x
        let starts: Vec<i32> = (30..170)
            .filter_map(|y| (0..300).find(|&x| alpha(&p, x, y) > INKED))
            .collect();
        let (lo, hi) = (starts.iter().min().unwrap(), starts.iter().max().unwrap());
        assert!(hi - lo > 4, "starts {lo}..{hi}");
    }

    #[test]
    fn pressure_widens_the_bundle() {
        let b = pen(0.4);
        let width = |pressure: f32| {
            let mut p = Painter::new(300, 200);
            p.set_strands(4, 40.0, 0.0, 0.0);
            draw(&mut p, &b, &line(30.0, 270.0, 100.0, pressure));
            let ys: Vec<i32> = (0..200).filter(|&y| alpha(&p, 150, y) > INKED).collect();
            ys.last().unwrap() - ys.first().unwrap()
        };
        assert!(width(0.2) * 3 < width(1.0) * 2);
    }

    #[test]
    fn symmetry_copies_are_bundles_too() {
        let b = pen(0.4);
        let mut p = Painter::new(300, 300);
        p.set_symmetry(Some(Symmetry {
            mode: SymmetryMode::Horizontal,
            cx: 150.0,
            cy: 150.0,
        }));
        p.set_strands(4, 30.0, 0.0, 0.0);
        draw(&mut p, &b, &line(30.0, 270.0, 60.0, 1.0));
        assert_eq!(column(&p, 150, 10, 110).0, 4);
        assert_eq!(column(&p, 150, 190, 290).0, 4, "the mirrored copy");
    }
}
