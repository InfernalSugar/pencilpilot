//! Relevance fixtures: per reference, (a) good copies, (b) rough or shifted
//! genuine attempts and (c) unrelated drawings that cover a similar area.
//! Every drawing is a set of polylines, so it can be rendered or replayed as
//! strokes (sketch mode).

use super::*;
use grader::{Stroke, StrokePoint};

pub type Paths = Vec<Vec<(f32, f32)>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Good,
    Rough,
    Unrelated,
}

pub struct Drawing {
    pub name: String,
    pub group: Group,
    pub paths: Paths,
    /// pen width, canvas px
    pub width: f32,
    /// extra filled shading (genuine shaded copy), drawn on top of the lines
    pub fill: Option<fn(&mut Gray)>,
}

impl Drawing {
    pub fn render(&self) -> Gray {
        let mut g = white();
        for p in &self.paths {
            for w in p.windows(2) {
                line(&mut g, w[0], w[1], self.width, 25);
            }
        }
        if let Some(f) = self.fill {
            f(&mut g);
        }
        g
    }

    pub fn strokes(&self) -> Vec<Stroke> {
        let mut t = 0.0;
        self.paths
            .iter()
            .filter(|p| p.len() >= 2)
            .map(|p| Stroke {
                brush_id: "pencil".into(),
                eraser: false,
                points: p
                    .iter()
                    .map(|&(x, y)| {
                        t += 16.0;
                        StrokePoint {
                            x,
                            y,
                            pressure: 0.5,
                            t,
                        }
                    })
                    .collect(),
            })
            .collect()
    }
}

pub struct Case {
    pub name: &'static str,
    pub reference: Gray,
    pub drawings: Vec<Drawing>,
}

// ── Path helpers ────────────────────────────────────────────────────────

fn resample(p: &[(f32, f32)], step: f32) -> Vec<(f32, f32)> {
    let mut out = vec![p[0]];
    for w in p.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = ((b.0 - a.0).hypot(b.1 - a.1) / step).ceil().max(1.0) as usize;
        for k in 1..=n {
            let t = k as f32 / n as f32;
            out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
    }
    out
}

fn arc(c: (f32, f32), rx: f32, ry: f32, a0: f32, a1: f32) -> Vec<(f32, f32)> {
    (0..=40)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / 40.0;
            (c.0 + rx * a.cos(), c.1 + ry * a.sin())
        })
        .collect()
}

fn ring(c: (f32, f32), rx: f32, ry: f32) -> Vec<(f32, f32)> {
    arc(c, rx, ry, 0.0, std::f32::consts::TAU)
}

fn quad(a: (f32, f32), c: (f32, f32), b: (f32, f32)) -> Vec<(f32, f32)> {
    (0..=24)
        .map(|i| {
            let t = i as f32 / 24.0;
            let u = 1.0 - t;
            (
                u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0,
                u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1,
            )
        })
        .collect()
}

/// Perpendicular, smoothly varying wobble of `amp` px.
pub fn wobble(paths: &Paths, amp: f32, seed: u32) -> Paths {
    paths
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let p = resample(p, 4.0);
            let n = p.len();
            (0..n)
                .map(|i| {
                    let a = p[i.saturating_sub(1)];
                    let b = p[(i + 1).min(n - 1)];
                    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                    let l = dx.hypot(dy).max(1e-6);
                    let o = amp * value_noise(i as f32 * 4.0, k as f32 * 97.0, 40.0, seed);
                    (p[i].0 - dy / l * o, p[i].1 + dx / l * o)
                })
                .collect()
        })
        .collect()
}

pub fn transform(paths: &Paths, scale: f32, dx: f32, dy: f32, c: (f32, f32)) -> Paths {
    paths
        .iter()
        .map(|p| {
            p.iter()
                .map(|&(x, y)| (c.0 + (x - c.0) * scale + dx, c.1 + (y - c.1) * scale + dy))
                .collect()
        })
        .collect()
}

/// Each line drawn as `passes` scratchy, offset passes (a rough sketch).
pub fn hairy(paths: &Paths, passes: usize, seed: u32) -> Paths {
    let mut out = Vec::new();
    for k in 0..passes {
        let off = (k as f32 - (passes - 1) as f32 / 2.0) * 3.5;
        for p in wobble(paths, 2.5, seed + k as u32) {
            let n = p.len();
            out.push(
                (0..n)
                    .map(|i| {
                        let a = p[i.saturating_sub(1)];
                        let b = p[(i + 1).min(n - 1)];
                        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                        let l = dx.hypot(dy).max(1e-6);
                        (p[i].0 - dy / l * off, p[i].1 + dx / l * off)
                    })
                    .collect(),
            );
        }
    }
    out
}

/// Keep every other path (half the drawing left out).
pub fn half(paths: &Paths) -> Paths {
    paths.iter().step_by(2).cloned().collect()
}

/// Lines stopping short at both ends (a hesitant beginner).
pub fn trimmed(paths: &Paths, keep: f32) -> Paths {
    paths
        .iter()
        .map(|p| {
            let p = resample(p, 4.0);
            let n = p.len();
            let cut = ((1.0 - keep) / 2.0 * n as f32) as usize;
            p[cut..n - cut.min(n / 2)].to_vec()
        })
        .filter(|p: &Vec<_>| p.len() >= 2)
        .collect()
}

// ── Unrelated drawings ──────────────────────────────────────────────────

type Rect = (f32, f32, f32, f32);

/// Dense scratchy scribbles: random-walk strokes inside `area` (the user's
/// report: lines and scribbles roughly covering the reference's area).
pub fn scribbles(area: Rect, n: usize, seed: u64, inside: &dyn Fn(f32, f32) -> bool) -> Paths {
    let mut r = Lcg(seed);
    let mut out = Vec::new();
    let mut tries = 0;
    while out.len() < n && tries < n * 50 {
        tries += 1;
        let mut p = (
            area.0 + r.next() * (area.2 - area.0),
            area.1 + r.next() * (area.3 - area.1),
        );
        if !inside(p.0, p.1) {
            continue;
        }
        let mut a = r.next() * std::f32::consts::TAU;
        let steps = 8 + (r.next() * 30.0) as usize;
        let mut path = vec![p];
        for _ in 0..steps {
            a += (r.next() - 0.5) * 1.6;
            let q = (p.0 + 7.0 * a.cos(), p.1 + 7.0 * a.sin());
            if !inside(q.0, q.1) || q.0 < area.0 || q.0 > area.2 || q.1 < area.1 || q.1 > area.3 {
                // bounce back like a hand filling an area
                a += std::f32::consts::PI * (0.6 + 0.4 * r.next());
                continue;
            }
            p = q;
            path.push(p);
        }
        if path.len() >= 2 {
            out.push(path);
        }
    }
    out
}

/// Parallel hatching over `area`, `spacing` px apart, at `angle` radians.
pub fn hatching(area: Rect, spacing: f32, angle: f32) -> Paths {
    let (cx, cy) = ((area.0 + area.2) / 2.0, (area.1 + area.3) / 2.0);
    let r = (area.2 - area.0).hypot(area.3 - area.1) / 2.0;
    let (dx, dy) = (angle.cos(), angle.sin());
    let mut out = Vec::new();
    let mut o = -r;
    while o <= r {
        // line: centre + o·normal + t·dir, clipped to the rect
        let (px, py) = (cx - dy * o, cy + dx * o);
        let pts: Vec<(f32, f32)> = (-200..=200)
            .map(|i| {
                let t = i as f32 / 200.0 * r;
                (px + dx * t, py + dy * t)
            })
            .filter(|&(x, y)| x >= area.0 && x <= area.2 && y >= area.1 && y <= area.3)
            .collect();
        if pts.len() >= 2 {
            out.push(vec![pts[0], pts[pts.len() - 1]]);
        }
        o += spacing;
    }
    out
}

/// A different object (a house) filling `area`.
pub fn house(area: Rect) -> Paths {
    let (x0, y0, x1, y1) = area;
    let (w, h) = (x1 - x0, y1 - y0);
    let roof = y0 + 0.4 * h;
    vec![
        vec![
            (x0 + 0.1 * w, roof),
            (x0 + 0.5 * w, y0),
            (x0 + 0.9 * w, roof),
        ],
        vec![
            (x0 + 0.15 * w, roof),
            (x0 + 0.15 * w, y1),
            (x0 + 0.85 * w, y1),
            (x0 + 0.85 * w, roof),
            (x0 + 0.15 * w, roof),
        ],
        vec![
            (x0 + 0.42 * w, y1),
            (x0 + 0.42 * w, y0 + 0.72 * h),
            (x0 + 0.58 * w, y0 + 0.72 * h),
            (x0 + 0.58 * w, y1),
        ],
        vec![
            (x0 + 0.22 * w, y0 + 0.5 * h),
            (x0 + 0.35 * w, y0 + 0.5 * h),
            (x0 + 0.35 * w, y0 + 0.62 * h),
            (x0 + 0.22 * w, y0 + 0.62 * h),
            (x0 + 0.22 * w, y0 + 0.5 * h),
        ],
    ]
}

/// A different object (a car-ish side view) filling `area`.
pub fn car(area: Rect) -> Paths {
    let (x0, y0, x1, y1) = area;
    let (w, h) = (x1 - x0, y1 - y0);
    let at = |fx: f32, fy: f32| (x0 + fx * w, y0 + fy * h);
    vec![
        vec![
            at(0.0, 0.75),
            at(0.0, 0.5),
            at(0.25, 0.45),
            at(0.35, 0.2),
            at(0.7, 0.2),
            at(0.85, 0.45),
            at(1.0, 0.5),
            at(1.0, 0.75),
            at(0.0, 0.75),
        ],
        ring(at(0.22, 0.8), 0.09 * w, 0.09 * w),
        ring(at(0.78, 0.8), 0.09 * w, 0.09 * w),
        vec![at(0.4, 0.25), at(0.4, 0.45), at(0.8, 0.45)],
    ]
}

/// Bounding box of the reference's dark pixels (tone < `cut`).
pub fn dark_bbox(r: &Gray, cut: u8) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, 0f32, 0f32);
    for y in 0..r.height {
        for x in 0..r.width {
            if r.pixels[(y * r.width + x) as usize] < cut {
                x0 = x0.min(x as f32);
                y0 = y0.min(y as f32);
                x1 = x1.max(x as f32);
                y1 = y1.max(y as f32);
            }
        }
    }
    (x0, y0, x1, y1)
}

/// Coarse silhouette of the reference: dark pixels (< `cut`) on an 8 px grid,
/// grown by `grow` cells.
pub fn silhouette_of(r: &Gray, cut: u8, grow: i32) -> impl Fn(f32, f32) -> bool {
    let (cw, ch) = ((r.width / 8) as i32, (r.height / 8) as i32);
    let mut cells = vec![false; (cw * ch) as usize];
    for y in 0..r.height as i32 {
        for x in 0..r.width as i32 {
            if r.pixels[(y as u32 * r.width + x as u32) as usize] < cut {
                cells[((y / 8).min(ch - 1) * cw + (x / 8).min(cw - 1)) as usize] = true;
            }
        }
    }
    let mut grown = cells.clone();
    for y in 0..ch {
        for x in 0..cw {
            if cells[(y * cw + x) as usize] {
                for dy in -grow..=grow {
                    for dx in -grow..=grow {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx >= 0
                            && ny >= 0
                            && nx < cw
                            && ny < ch
                            && dx * dx + dy * dy <= grow * grow
                        {
                            grown[(ny * cw + nx) as usize] = true;
                        }
                    }
                }
            }
        }
    }
    move |x: f32, y: f32| {
        let (cx, cy) = (x as i32 / 8, y as i32 / 8);
        x >= 0.0 && y >= 0.0 && cx < cw && cy < ch && grown[(cy * cw + cx) as usize]
    }
}

// ── References ──────────────────────────────────────────────────────────

impl Figure {
    pub fn paths(&self) -> Paths {
        let tf = |p: (f32, f32)| {
            (
                400.0 + (p.0 - 400.0) * self.scale + self.dx,
                330.0 + (p.1 - 330.0) * self.scale + self.dy,
            )
        };
        let r = self.head_r;
        let mut out = vec![ring((400.0, 220.0 - r), r, r).into_iter().map(tf).collect()];
        let (neck, hip, sh) = ((400.0, 220.0), (400.0, 390.0), (400.0, 260.0));
        for (a, b) in [
            (neck, hip),
            (sh, self.left_hand),
            (sh, (480.0, 330.0)),
            (hip, (340.0, 510.0)),
            (hip, (460.0, 510.0)),
        ] {
            out.push(vec![tf(a), tf(b)]);
        }
        out
    }
}

/// An anime-style bust as clean line art: face, hair strands, eyes, neck,
/// shoulders.
pub fn anime_paths() -> Paths {
    let mut p: Paths = vec![
        // face
        quad((330.0, 190.0), (335.0, 300.0), (400.0, 330.0)),
        quad((470.0, 190.0), (465.0, 300.0), (400.0, 330.0)),
        // eyes
        ring((365.0, 245.0), 18.0, 13.0),
        ring((435.0, 245.0), 18.0, 13.0),
        // brows, nose, mouth
        quad((345.0, 220.0), (365.0, 212.0), (385.0, 220.0)),
        quad((415.0, 220.0), (435.0, 212.0), (455.0, 220.0)),
        vec![(400.0, 265.0), (396.0, 282.0), (402.0, 284.0)],
        quad((385.0, 303.0), (400.0, 309.0), (415.0, 303.0)),
        // neck and shoulders
        vec![(378.0, 322.0), (376.0, 370.0)],
        vec![(422.0, 322.0), (424.0, 370.0)],
        quad((376.0, 370.0), (300.0, 380.0), (250.0, 470.0)),
        quad((424.0, 370.0), (500.0, 380.0), (550.0, 470.0)),
        // collar
        vec![(376.0, 372.0), (400.0, 405.0), (424.0, 372.0)],
        // hair: crown
        arc((400.0, 200.0), 95.0, 95.0, 3.3, 6.12),
    ];
    // hair: strands falling on both sides and bangs
    for i in 0..6 {
        let o = i as f32 * 9.0;
        p.push(quad(
            (318.0 + o * 0.3, 170.0 + o),
            (290.0 - o * 0.5, 300.0),
            (300.0 + o, 430.0 - o),
        ));
        p.push(quad(
            (482.0 - o * 0.3, 170.0 + o),
            (510.0 + o * 0.5, 300.0),
            (500.0 - o, 430.0 - o),
        ));
    }
    for i in 0..5 {
        let x = 345.0 + i as f32 * 28.0;
        p.push(quad((x + 10.0, 120.0), (x - 5.0, 170.0), (x + 4.0, 205.0)));
    }
    p
}

fn render_lines(paths: &Paths, width: f32, v: u8) -> Gray {
    let mut g = white();
    for p in paths {
        for w in p.windows(2) {
            line(&mut g, w[0], w[1], width, v);
        }
    }
    g
}

fn hatch_shade_left(g: &mut Gray) {
    // genuine shading: hatching on the left half of a disc (r 150 at centre)
    for k in 0..30 {
        let x = 255.0 + k as f32 * 5.0;
        let dy = (150.0f32.powi(2) - (x - 400.0).powi(2)).max(0.0).sqrt() - 6.0;
        if dy > 4.0 {
            line(g, (x, 300.0 - dy), (x - 4.0, 300.0 + dy), 2.0, 70);
        }
    }
}

/// A painted copy of the shaded disc: grey paper tone everywhere (no white
/// left), a darker disc shaded from the top left.
fn painted_disc(g: &mut Gray) {
    for y in 0..600 {
        for x in 0..800 {
            let (px, py) = (x as f32 + 0.5 - 400.0, y as f32 + 0.5 - 300.0);
            let v = if px * px + py * py <= 150.0 * 150.0 {
                90.0 + 60.0 * ((px + py) / 300.0 + 0.5).clamp(0.0, 1.0)
            } else {
                205.0
            };
            g.pixels[y * 800 + x] = g.pixels[y * 800 + x].min(v as u8);
        }
    }
}

/// A painted copy of the figure: grey ground, dark head and limbs.
fn painted_figure(g: &mut Gray) {
    for p in g.pixels.iter_mut() {
        *p = (*p).min(200);
    }
    let f = Figure::default();
    let photo = f.photo();
    for (p, q) in g.pixels.iter_mut().zip(&photo.pixels) {
        if *q < 200 {
            *p = (*p).min(70);
        }
    }
}

fn solid_shade_left(g: &mut Gray) {
    for y in 150..450 {
        for x in 250..400 {
            let (px, py) = (x as f32 - 400.0, y as f32 - 300.0);
            if px * px + py * py <= 148.0 * 148.0 {
                let i = y * 800 + x;
                g.pixels[i] = g.pixels[i].min(110);
            }
        }
    }
}

fn genuine(paths: &Paths, centre: (f32, f32)) -> Vec<Drawing> {
    let d = |name: &str, group, paths: Paths| Drawing {
        name: name.into(),
        group,
        paths,
        width: 3.0,
        fill: None,
    };
    vec![
        d("exact copy", Group::Good, paths.clone()),
        d("slight wobble", Group::Good, wobble(paths, 4.0, 3)),
        d("wobbly 10px", Group::Rough, wobble(paths, 10.0, 5)),
        d("wobbly 18px", Group::Rough, wobble(paths, 18.0, 6)),
        d(
            "shifted+small",
            Group::Rough,
            transform(paths, 0.82, 70.0, 40.0, centre),
        ),
        d(
            "big+wobbly",
            Group::Rough,
            wobble(&transform(paths, 1.15, -30.0, 0.0, centre), 8.0, 7),
        ),
        d("half missing", Group::Rough, half(paths)),
        d("short lines", Group::Rough, trimmed(paths, 0.6)),
        d("hairy sketch", Group::Rough, hairy(paths, 4, 11)),
        d(
            "hairy+wobbly",
            Group::Rough,
            hairy(&wobble(paths, 8.0, 12), 4, 13),
        ),
    ]
}

fn unrelated(reference: &Gray, cut: u8) -> Vec<Drawing> {
    let area = dark_bbox(reference, cut);
    let sil = silhouette_of(reference, cut, 2);
    let any = |_: f32, _: f32| true;
    let (w, h) = (area.2 - area.0, area.3 - area.1);
    let d = |name: &str, paths: Paths| Drawing {
        name: name.into(),
        group: Group::Unrelated,
        paths,
        width: 3.0,
        fill: None,
    };
    let dense = ((w * h) / 900.0) as usize;
    vec![
        d("scribbles in area", scribbles(area, dense, 3, &any)),
        d("sparse scribbles", scribbles(area, dense / 4, 4, &any)),
        d("scribbles in silhouette", scribbles(area, dense, 5, &sil)),
        d(
            "hatching 45°",
            hatching(area, 12.0, std::f32::consts::FRAC_PI_4),
        ),
        d(
            "hatching vertical",
            hatching(area, 14.0, std::f32::consts::FRAC_PI_2),
        ),
        d(
            "cross-hatching",
            [
                hatching(area, 16.0, std::f32::consts::FRAC_PI_4),
                hatching(area, 16.0, 2.356),
            ]
            .concat(),
        ),
        d("a house", house(area)),
        d("a car", car(area)),
        d(
            "scribble+hatching",
            [
                scribbles(area, dense / 2, 9, &any),
                hatching(area, 18.0, 2.5),
            ]
            .concat(),
        ),
    ]
}

pub fn cases() -> Vec<Case> {
    let mut out = Vec::new();

    let f = Figure::default();
    let mut drawings = genuine(&f.paths(), (400.0, 330.0));
    drawings.push(Drawing {
        name: "painted".into(),
        group: Group::Rough,
        paths: Vec::new(),
        width: 3.0,
        fill: Some(painted_figure),
    });
    drawings.extend(unrelated(&f.photo(), 200));
    out.push(Case {
        name: "figure photo",
        reference: f.photo(),
        drawings,
    });

    let mut disc = white();
    shaded_disc(&mut disc, (400.0, 300.0), 150.0, 1);
    let circle: Paths = vec![ring((400.0, 300.0), 150.0, 150.0)];
    let mut drawings = genuine(&circle, (400.0, 300.0));
    // shaded genuine copies
    for (name, fill) in [
        ("hatched shading", hatch_shade_left as fn(&mut Gray)),
        ("solid shading", solid_shade_left as fn(&mut Gray)),
        ("painted", painted_disc as fn(&mut Gray)),
    ] {
        drawings.push(Drawing {
            name: name.into(),
            group: Group::Rough,
            paths: if name == "painted" {
                Vec::new()
            } else {
                circle.clone()
            },
            width: 3.0,
            fill: Some(fill),
        });
    }
    drawings.extend(unrelated(&disc, 200));
    out.push(Case {
        name: "shaded disc",
        reference: disc,
        drawings,
    });

    let mut photo = white();
    textured_background(&mut photo, 5);
    shaded_subject(&mut photo);
    let mut subj: Paths = vec![ring((400.0, 300.0), 150.0, 150.0)];
    subj.extend(subject_details());
    let mut drawings = genuine(&subj, (400.0, 300.0));
    // unrelated drawings over the subject's area (the background is grey)
    let mut sil_ref = white();
    shaded_disc(&mut sil_ref, (400.0, 300.0), 150.0, 1);
    drawings.extend(unrelated(&sil_ref, 200));
    out.push(Case {
        name: "textured photo",
        reference: photo,
        drawings,
    });

    let anime = anime_paths();
    let reference = render_lines(&anime, 4.0, 30);
    let mut drawings = genuine(&anime, (400.0, 300.0));
    drawings.extend(unrelated(&reference, 128));
    out.push(Case {
        name: "anime bust",
        reference,
        drawings,
    });
    out
}
