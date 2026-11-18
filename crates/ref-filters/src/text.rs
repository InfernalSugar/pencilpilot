//! "Is this mostly text?" heuristic (posters, quotes, infographics).
//!
//! Plain maths, no OCR, on the ≤ 800 px working image:
//! 1. Local-contrast binarisation (pixel vs. 41×41 box mean ± 28 gray
//!    levels), separately for dark-on-light and light-on-dark ink.
//! 2. 8-connected components; keep glyph-sized ones (height 7–64 px,
//!    width 0.2–1.8 × height, fill 15–85 %).
//! 3. Link left/right neighbours with similar heights (≤ 1.6×), a small gap
//!    (≤ 0.9 × height) and a shared baseline, top or centre (± 0.15 × h):
//!    letters of a word, words of a line.
//! 4. A chain of ≥ [`MIN_LINE`] glyphs that is ≥ 3 glyph-heights long,
//!    nearly horizontal and straight is a *text line*.
//! 5. Only lines **stacked in rows** count: another line of similar height
//!    0.9–3 line-heights above or below, overlapping in x. A lone caption
//!    or title is fine; paragraphs, quotes and infographics are not.
//!
//! Score = √(c/(c+0.05) · n/(n+3)), where `c` is the fraction of the picture
//! covered by text-line boxes and `n` the number of such lines.

use crate::work::Work;

const WINDOW_R: usize = 20;
const CONTRAST: f32 = 28.0;
const MIN_H: usize = 7;
const MAX_H: usize = 64;
/// Glyphs in a chain before it counts as a line of text.
pub(crate) const MIN_LINE: usize = 6;
/// Max tilt of a text line (rise over run).
const MAX_SLOPE: f32 = 0.12;
/// Max spread of glyph centres around the fitted line, × glyph height.
const MAX_RESID: f32 = 0.2;

#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct TextStats {
    /// Text lines found.
    pub lines: usize,
    /// Glyphs that belong to a text line.
    pub glyphs: usize,
    /// Fraction of the picture covered by text-line boxes.
    pub coverage: f32,
    /// 0–1 score.
    pub score: f32,
}

#[derive(Debug, Clone, Copy)]
struct Blob {
    x0: usize,
    y0: usize,
    x1: usize, // inclusive
    y1: usize, // inclusive
}

impl Blob {
    fn h(&self) -> usize {
        self.y1 - self.y0 + 1
    }
    fn w(&self) -> usize {
        self.x1 - self.x0 + 1
    }
    fn cy(&self) -> f32 {
        (self.y0 + self.y1) as f32 / 2.0
    }
}

pub(crate) fn text_stats(work: &Work) -> TextStats {
    let (w, h) = (work.w, work.h);
    if w < 3 * MIN_H || h < 3 * MIN_H {
        return TextStats::default();
    }
    let mean = box_mean(&work.d, w, h, WINDOW_R);
    let (mut lines, mut glyphs, mut area) = (0, 0, 0.0);
    for dark in [true, false] {
        let mask: Vec<bool> = work
            .d
            .iter()
            .zip(&mean)
            .map(|(&v, &m)| {
                if dark {
                    v < m - CONTRAST
                } else {
                    v > m + CONTRAST
                }
            })
            .collect();
        let blobs = glyph_blobs(&mask, w, h);
        for l in text_lines(&blobs) {
            lines += 1;
            glyphs += l.glyphs;
            area += (l.x1 - l.x0 + 1) as f32 * l.h;
        }
    }
    let coverage = area / (w * h) as f32;
    // Half-way points: text rows covering 5 % of the picture, 3 rows.
    let c = coverage / (coverage + 0.05);
    let l = lines as f32 / (lines as f32 + 3.0);
    TextStats {
        lines,
        glyphs,
        coverage,
        score: (c * l).sqrt(),
    }
}

/// Mean over a (2r+1)² window, clamped at the borders (summed-area table).
fn box_mean(d: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut sat = vec![0f64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0f64;
        for x in 0..w {
            row += d[y * w + x] as f64;
            sat[(y + 1) * (w + 1) + x + 1] = sat[y * (w + 1) + x + 1] + row;
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let (ya, yb) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            let (xa, xb) = (x.saturating_sub(r), (x + r + 1).min(w));
            let s = sat[yb * (w + 1) + xb] - sat[ya * (w + 1) + xb] - sat[yb * (w + 1) + xa]
                + sat[ya * (w + 1) + xa];
            out[y * w + x] = (s / ((yb - ya) * (xb - xa)) as f64) as f32;
        }
    }
    out
}

/// Connected components that are shaped like letters.
fn glyph_blobs(mask: &[bool], w: usize, h: usize) -> Vec<Blob> {
    let mut seen = vec![false; mask.len()];
    let mut stack = Vec::new();
    let mut out = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let (mut b, mut area) = (
            Blob {
                x0: usize::MAX,
                y0: usize::MAX,
                x1: 0,
                y1: 0,
            },
            0usize,
        );
        while let Some(i) = stack.pop() {
            area += 1;
            let (x, y) = (i % w, i / w);
            b.x0 = b.x0.min(x);
            b.x1 = b.x1.max(x);
            b.y0 = b.y0.min(y);
            b.y1 = b.y1.max(y);
            for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    let j = ny * w + nx;
                    if mask[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let (bw, bh) = (b.w(), b.h());
        let fill = area as f32 / (bw * bh) as f32;
        if (MIN_H..=MAX_H).contains(&bh)
            && bw * 5 >= bh
            && bw * 10 <= bh * 18
            && (0.15..=0.85).contains(&fill)
            && area >= 12
        {
            out.push(b);
        }
    }
    out
}

#[derive(Debug, Clone, Copy)]
struct Line {
    x0: usize,
    x1: usize,
    cy: f32,
    h: f32,
    glyphs: usize,
}

/// A chain of glyphs is a line of text if it is long and straight:
/// several glyph-heights wide, nearly horizontal, centres on one line.
fn line_of(blobs: &[Blob], m: &[usize]) -> Option<Line> {
    let c = m.len() as f32;
    let x0 = m.iter().map(|&i| blobs[i].x0).min()?;
    let x1 = m.iter().map(|&i| blobs[i].x1).max()?;
    let h = m.iter().map(|&i| blobs[i].h() as f32).sum::<f32>() / c;
    if ((x1 - x0 + 1) as f32) < 3.0 * h {
        return None;
    }
    // Least-squares fit cy = a + b·cx.
    let cx: Vec<f32> = m
        .iter()
        .map(|&i| (blobs[i].x0 + blobs[i].x1) as f32 / 2.0)
        .collect();
    let cy: Vec<f32> = m.iter().map(|&i| blobs[i].cy()).collect();
    let (mx, my) = (cx.iter().sum::<f32>() / c, cy.iter().sum::<f32>() / c);
    let sxx: f32 = cx.iter().map(|x| (x - mx).powi(2)).sum();
    let sxy: f32 = cx.iter().zip(&cy).map(|(x, y)| (x - mx) * (y - my)).sum();
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    let resid = (cx
        .iter()
        .zip(&cy)
        .map(|(x, y)| (y - my - slope * (x - mx)).powi(2))
        .sum::<f32>()
        / c)
        .sqrt();
    if slope.abs() > MAX_SLOPE || resid > MAX_RESID * h {
        return None;
    }
    Some(Line {
        x0,
        x1,
        cy: my,
        h,
        glyphs: m.len(),
    })
}

/// Chains glyphs into lines, then keeps the lines that are stacked in rows
/// (another line of similar height just above or below, overlapping in x).
fn text_lines(blobs: &[Blob]) -> Vec<Line> {
    let n = blobs.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| blobs[i].x0);
    for (k, &i) in order.iter().enumerate() {
        let a = blobs[i];
        for &j in &order[k + 1..] {
            let b = blobs[j];
            // Gaps never exceed one max glyph height.
            if b.x0 > a.x1 + MAX_H {
                break;
            }
            let (ha, hb) = (a.h() as f32, b.h() as f32);
            let (hmin, hmax) = (ha.min(hb), ha.max(hb));
            if hmax > 1.6 * hmin {
                continue;
            }
            let gap = b.x0 as f32 - a.x1 as f32 - 1.0;
            if gap < -0.15 * hmax || gap > 0.9 * hmax {
                continue;
            }
            // Letters share a baseline (a/l), a top (a/p) or a centre (l/p).
            let base = (a.y1 as f32 - b.y1 as f32).abs();
            let top = (a.y0 as f32 - b.y0 as f32).abs();
            let mid = (a.cy() - b.cy()).abs();
            if base.min(top).min(mid) > 0.15 * hmax {
                continue;
            }
            let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
            if ri != rj {
                parent[ri] = rj;
            }
        }
    }

    // Collect chains long enough to be lines.
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        let r = find(&mut parent, i);
        members[r].push(i);
    }
    let lines: Vec<Line> = members
        .iter()
        .filter(|m| m.len() >= MIN_LINE)
        .filter_map(|m| line_of(blobs, m))
        .collect();

    // Keep lines that have a neighbouring line (rows of text).
    lines
        .iter()
        .enumerate()
        .filter(|&(i, a)| {
            lines.iter().enumerate().any(|(j, b)| {
                let (hmin, hmax) = (a.h.min(b.h), a.h.max(b.h));
                let dy = (a.cy - b.cy).abs();
                let overlap = a.x1.min(b.x1) as f32 - a.x0.max(b.x0) as f32;
                let wmin = (a.x1 - a.x0).min(b.x1 - b.x0) as f32;
                i != j
                    && hmax <= 1.35 * hmin
                    && dy >= 0.9 * hmax
                    && dy <= 3.0 * hmax
                    && overlap >= 0.3 * wmin
            })
        })
        .map(|(_, l)| *l)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5×7 bitmap "font": enough different shapes to look like words.
    const GLYPHS: [[&str; 7]; 8] = [
        [
            ".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
        ],
        [
            "####.", "#...#", "#...#", "####.", "#...#", "#...#", "####.",
        ],
        [
            ".###.", "#...#", "#....", "#....", "#....", "#...#", ".###.",
        ],
        [
            "#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
        ],
        [
            "#....", "#....", "#....", "#....", "#....", "#....", "#####",
        ],
        [
            "#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#",
        ],
        [
            ".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.",
        ],
        [
            "####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#",
        ],
    ];

    /// White page with `rows` lines of dark "text", glyphs `scale`× the
    /// 5×7 font, optionally over a soft background gradient.
    pub(crate) fn page(w: usize, h: usize, rows: usize, scale: usize, light_on_dark: bool) -> Work {
        let (bg, ink) = if light_on_dark {
            (40.0, 230.0)
        } else {
            (245.0, 20.0)
        };
        let mut d = vec![bg; w * h];
        let (gw, gh) = (5 * scale, 7 * scale);
        let line_h = gh * 2;
        let mut seed = 7u32;
        for row in 0..rows {
            let y0 = 40 + row * line_h;
            if y0 + gh >= h {
                break;
            }
            let mut x = 40;
            while x + gw < w - 40 {
                seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                if (seed >> 16).is_multiple_of(6) {
                    x += 3 * scale; // word gap
                    continue;
                }
                let g = &GLYPHS[(seed >> 20) as usize % GLYPHS.len()];
                for (gy, line) in g.iter().enumerate() {
                    for (gx, c) in line.bytes().enumerate() {
                        if c == b'#' {
                            for yy in 0..scale {
                                for xx in 0..scale {
                                    d[(y0 + gy * scale + yy) * w + x + gx * scale + xx] = ink;
                                }
                            }
                        }
                    }
                }
                x += gw + scale; // letter spacing
            }
        }
        Work { w, h, d }
    }

    #[test]
    fn rows_of_glyphs_are_text() {
        for (scale, inverse) in [(2, false), (3, false), (4, true)] {
            let t = text_stats(&page(800, 560, 8, scale, inverse));
            assert!(t.lines >= 5, "scale {scale}: {t:?}");
            assert!(t.score >= 0.6, "scale {scale}: {t:?}");
            assert!(t.glyphs >= 100 && t.coverage > 0.1, "scale {scale}: {t:?}");
        }
    }

    #[test]
    fn a_single_caption_is_not_text_heavy() {
        let t = text_stats(&page(800, 560, 1, 3, false));
        assert!(t.score < 0.3, "{t:?}");
    }

    #[test]
    fn shapes_are_not_text() {
        let mut d = vec![240.0; 800 * 600];
        for y in 0..600 {
            for x in 0..800 {
                let (dx, dy) = (x as f32 - 400.0, y as f32 - 300.0);
                if (dx * dx + dy * dy).sqrt() < 180.0 {
                    d[y * 800 + x] = 90.0;
                }
            }
        }
        let t = text_stats(&Work { w: 800, h: 600, d });
        assert_eq!(t.score, 0.0, "{t:?}");
    }
}
