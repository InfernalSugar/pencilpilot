//! Golden tests on synthetic fixtures. Run with `--nocapture` to see the
//! score table.

mod common;

use common::*;
use grader::{Grade, GradeOptions, Gray, Stroke, StrokePoint, grade, grade_with};

fn show(name: &str, g: &Grade) {
    let m = |n: &str| g.measurement(n).unwrap_or(f32::NAN);
    eprintln!(
        "{name:<28} score {:5.2}  sim {:.3}  place {:.2} scale {:.2} asp {:+.2} off ({:+.2},{:+.2})  outline {:.2} sil {:.2} grid {:.2} worst {:.2} tone {:.2} contrast {:.2} ssim {:.2} structure {:+.2}  align s{:.2} r{:+.1} ({:+.0},{:+.0}) {:?}",
        g.score,
        g.similarity,
        m("placement"),
        m("scale_ratio"),
        m("aspect_error"),
        m("offset_x"),
        m("offset_y"),
        m("outline"),
        m("silhouette"),
        m("grid"),
        m("worst_cell"),
        m("tone"),
        m("contrast_ratio"),
        m("ssim"),
        m("structure"),
        g.alignment.scale,
        g.alignment.rotation,
        g.alignment.dx,
        g.alignment.dy,
        g.flags,
    );
}

fn run(name: &str, d: &Gray, r: &Gray) -> Grade {
    let g = grade(d, r).unwrap();
    show(name, &g);
    let ungated = GradeOptions {
        relevance_gate: Some(false),
        ..Default::default()
    };
    let before = grade_with(d, r, &ungated).unwrap().score;
    if (before - g.score).abs() > 0.01 {
        eprintln!("{:<28} (without the relevance gate: {before:.2})", "");
    }
    g
}

fn disc_ref() -> Gray {
    let mut r = white();
    shaded_disc(&mut r, (400.0, 300.0), 150.0, 1);
    r
}

fn circle_drawing(c: (f32, f32), r: f32) -> Gray {
    let mut d = white();
    circle(&mut d, c, r, 3.0);
    d
}

#[test]
fn identical_circle_scores_high() {
    let g = run(
        "circle identical",
        &circle_drawing((400.0, 300.0), 150.0),
        &disc_ref(),
    );
    assert!(g.score >= 8.5, "{}", g.score);
    assert!(g.passed);
}

#[test]
fn identical_figure_scores_high() {
    let f = Figure::default();
    let g = run("figure identical", &f.line_art(), &f.photo());
    assert!(g.score >= 8.5, "{}", g.score);
    let sr = g.measurement("scale_ratio").unwrap();
    assert!((sr - 1.0).abs() < 0.06, "{sr}");
}

#[test]
fn letterboxed_photo_reference_scores_high() {
    let g = run(
        "circle vs letterboxed photo",
        &circle_drawing((400.0, 300.0), 150.0),
        &letterboxed_disc(),
    );
    assert!(g.score >= 8.5, "{}", g.score);
}

#[test]
fn shifted_drawing_is_aligned() {
    let f = Figure::default();
    let shifted = Figure { dx: 80.0, ..f };
    let g = run("figure shifted 10%", &shifted.line_art(), &f.photo());
    assert!(g.score >= 8.0, "{}", g.score);
    let ox = g.measurement("offset_x").unwrap();
    assert!((ox - 0.10).abs() < 0.02, "offset_x {ox}");
    assert!(g.measurement("offset_y").unwrap().abs() < 0.02);
    assert!((g.alignment.dx + 80.0).abs() < 8.0, "{:?}", g.alignment);

    let gc = run(
        "circle shifted 10%",
        &circle_drawing((480.0, 360.0), 150.0),
        &disc_ref(),
    );
    assert!(gc.score >= 8.0, "{}", gc.score);
    assert!((gc.measurement("offset_y").unwrap() - 0.10).abs() < 0.02);
}

#[test]
fn scaled_drawing_is_aligned() {
    let f = Figure::default();
    let small = Figure { scale: 0.85, ..f };
    let g = run("figure scaled 85%", &small.line_art(), &f.photo());
    assert!(g.score >= 8.0, "{}", g.score);
    // Relative to the same figure drawn at full size (the thick reference
    // limbs make the reference box a little wider than any line drawing).
    let full = grade(&f.line_art(), &f.photo()).unwrap();
    let sr = g.measurement("scale_ratio").unwrap() / full.measurement("scale_ratio").unwrap();
    assert!((sr - 0.85).abs() < 0.04, "scale_ratio {sr}");
    assert!(
        (g.alignment.scale - 1.0 / 0.85).abs() < 0.06,
        "{:?}",
        g.alignment
    );

    let gc = run(
        "circle scaled 85%",
        &circle_drawing((400.0, 300.0), 127.5),
        &disc_ref(),
    );
    assert!(gc.score >= 8.0, "{}", gc.score);
    let sr = gc.measurement("scale_ratio").unwrap();
    assert!((sr - 0.85).abs() < 0.04, "scale_ratio {sr}");
}

#[test]
fn head_too_big_is_detected() {
    let f = Figure::default();
    let good = grade(&f.line_art(), &f.photo()).unwrap();
    let big = Figure { head_r: 65.0, ..f };
    let g = run("figure head +30%", &big.line_art(), &f.photo());
    assert!(g.score < good.score - 0.5, "{} vs {}", g.score, good.score);
    // Taller figure: bigger and narrower than when drawn right.
    let m = |g: &Grade, n: &str| g.measurement(n).unwrap();
    assert!(
        m(&g, "scale_ratio") > m(&good, "scale_ratio") + 0.02,
        "{} vs {}",
        m(&g, "scale_ratio"),
        m(&good, "scale_ratio")
    );
    assert!(m(&g, "aspect_error") < m(&good, "aspect_error") - 0.04);
    assert!(
        m(&g, "offset_y") < m(&good, "offset_y") - 0.01,
        "centre moves up"
    );
    // The head region (top row of the grid) gets worse. Alignment rescales
    // the whole figure, so part of the error spreads to the body too.
    let top = |g: &Grade| g.grid.cells[..4].iter().sum::<f32>();
    assert!(
        top(&g) < top(&good) - 0.1,
        "{:?} vs {:?}",
        g.grid,
        good.grid
    );
}

#[test]
fn wrong_shape_scores_mid_low() {
    let mut d = white();
    polyline(
        &mut d,
        &[
            (250.0, 150.0),
            (550.0, 150.0),
            (550.0, 450.0),
            (250.0, 450.0),
        ],
        3.0,
        true,
    );
    let g = run("square vs circle", &d, &disc_ref());
    assert!(g.score <= 6.5, "{}", g.score);
    let good = grade(&circle_drawing((400.0, 300.0), 150.0), &disc_ref()).unwrap();
    assert!(g.score < good.score - 2.0);
}

#[test]
fn random_scribble_scores_low() {
    let f = Figure::default();
    for seed in [1u64, 7, 42] {
        let g = run(
            &format!("scribble seed {seed}"),
            &scribble(seed, 30),
            &f.photo(),
        );
        assert!(g.score <= 3.0, "seed {seed}: {}", g.score);
        let g = run(
            &format!("scribble vs disc {seed}"),
            &scribble(seed, 30),
            &disc_ref(),
        );
        assert!(g.score <= 3.0, "seed {seed}: {}", g.score);
    }
}

#[test]
fn blank_scores_one() {
    let g = run("blank", &white(), &disc_ref());
    assert_eq!(g.score, 1.0);
    assert!(g.flags.contains(&"blank".to_string()));
}

#[test]
fn closer_drawings_score_higher() {
    // Squash (alignment can't fix aspect) and wobble series.
    let r = disc_ref();
    let mut last = f32::MAX;
    for ry in [150.0, 135.0, 115.0, 90.0, 60.0] {
        let mut d = white();
        polyline(
            &mut d,
            &ellipse_points((400.0, 300.0), 150.0, ry, 0.0, 0.0),
            3.0,
            true,
        );
        let g = run(&format!("ellipse ry={ry}"), &d, &r);
        assert!(g.similarity < last, "ry {ry}");
        last = g.similarity;
    }
    let mut last = f32::MAX;
    for amp in [0.0, 10.0, 20.0, 35.0, 55.0] {
        let mut d = white();
        polyline(
            &mut d,
            &ellipse_points((400.0, 300.0), 150.0, 150.0, amp, 7.0),
            3.0,
            true,
        );
        let g = run(&format!("wobble amp={amp}"), &d, &r);
        assert!(g.similarity < last, "amp {amp}");
        last = g.similarity;
    }
}

/// Four shaded discs, one per corner cell; the drawing gets the
/// bottom-right one wrong.
#[test]
fn grid_worst_cell_points_at_the_change() {
    let centres = [
        (100.0, 100.0),
        (700.0, 100.0),
        (100.0, 500.0),
        (700.0, 500.0),
    ];
    let mut r = white();
    let mut d = white();
    for (i, &c) in centres.iter().enumerate() {
        shaded_disc(&mut r, c, 70.0, i as u32);
        if i == 3 {
            polyline(
                &mut d,
                &[(640.0, 470.0), (760.0, 470.0), (700.0, 560.0)],
                3.0,
                true,
            );
        } else {
            circle(&mut d, c, 70.0, 3.0);
        }
    }
    let g = run("grid: bottom-right wrong", &d, &r);
    let w = g.grid.worst.unwrap();
    assert_eq!((w.col, w.row), (3, 2), "{:?}", g.grid);
    assert_eq!(w.index, 11);
    let worst = g.grid.cells[11];
    assert!(g.grid.cells[0] > worst + 0.1, "{:?}", g.grid.cells);
}

fn strokes_from(paths: &[Vec<(f32, f32)>], dt: impl Fn(usize) -> f64) -> Vec<Stroke> {
    let mut t = 0.0;
    paths
        .iter()
        .map(|p| Stroke {
            brush_id: "pencil".into(),
            eraser: false,
            points: p
                .iter()
                .enumerate()
                .map(|(i, &(x, y))| {
                    t += dt(i);
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

#[test]
fn smooth_strokes_beat_jittery_retraced_ones() {
    let circle_pts = ellipse_points((400.0, 300.0), 150.0, 150.0, 0.0, 0.0);
    let smooth = strokes_from(std::slice::from_ref(&circle_pts), |_| 16.0);
    // Shaky hand: noisy points, drawn three times over in short dashes,
    // with stop-and-go speed.
    let mut paths = Vec::new();
    for pass in 0..3 {
        for chunk in circle_pts.chunks(20) {
            paths.push(
                chunk
                    .iter()
                    .enumerate()
                    .map(|(i, &(x, y))| {
                        (
                            x + 3.0 * noise(i as i32, pass, 1),
                            y + 3.0 * noise(i as i32, pass, 2),
                        )
                    })
                    .collect(),
            );
        }
    }
    let jittery = strokes_from(&paths, |i| if i % 4 == 0 { 60.0 } else { 6.0 });
    let d = circle_drawing((400.0, 300.0), 150.0);
    let r = disc_ref();
    let gs = grade_with(
        &d,
        &r,
        &GradeOptions {
            strokes: smooth,
            ..Default::default()
        },
    )
    .unwrap();
    let gj = grade_with(
        &d,
        &r,
        &GradeOptions {
            strokes: jittery,
            ..Default::default()
        },
    )
    .unwrap();
    let m = |g: &Grade, n: &str| g.measurement(n).unwrap();
    eprintln!(
        "strokes smooth: conf {:.2} smooth {:.2} retrace {:.2} speed {:.2} score {:.2}",
        m(&gs, "line_confidence"),
        m(&gs, "smoothness"),
        m(&gs, "retrace_ratio"),
        m(&gs, "speed_consistency"),
        gs.score
    );
    eprintln!(
        "strokes jittery: conf {:.2} smooth {:.2} retrace {:.2} speed {:.2} score {:.2}",
        m(&gj, "line_confidence"),
        m(&gj, "smoothness"),
        m(&gj, "retrace_ratio"),
        m(&gj, "speed_consistency"),
        gj.score
    );
    assert!(m(&gs, "line_confidence") > m(&gj, "line_confidence") + 0.3);
    assert!(m(&gs, "smoothness") > m(&gj, "smoothness"));
    assert!(m(&gs, "retrace_ratio") < m(&gj, "retrace_ratio"));
    assert!(m(&gs, "speed_consistency") > m(&gj, "speed_consistency"));
    assert!(gs.similarity > gj.similarity);
}

#[test]
fn keypoints_feed_the_score() {
    use grader::{KeypointPair, Point};
    let f = Figure::default();
    let pts = [
        (400.0, 170.0),
        (400.0, 390.0),
        (320.0, 330.0),
        (480.0, 330.0),
        (340.0, 510.0),
        (460.0, 510.0),
    ];
    let pairs = |off: f32| {
        pts.iter()
            .enumerate()
            .map(|(i, &(x, y))| KeypointPair {
                reference: Point { x, y },
                drawing: Point {
                    x: x + if i == 2 { off } else { 0.0 },
                    y: y - if i == 2 { off } else { 0.0 },
                },
            })
            .collect::<Vec<_>>()
    };
    let opts = |off| GradeOptions {
        keypoints: pairs(off),
        ..Default::default()
    };
    let good = grade_with(&f.line_art(), &f.photo(), &opts(0.0)).unwrap();
    let bad = grade_with(&f.line_art(), &f.photo(), &opts(80.0)).unwrap();
    assert!(good.measurement("keypoint_error").unwrap() < 1e-3);
    assert!(bad.measurement("keypoint_error").unwrap() > 0.1);
    assert!(bad.measurement("angle_error").unwrap() > 1.0);
    assert!(bad.score < good.score);
    let k = bad.keypoints.unwrap();
    let worst = (0..k.pairs.len())
        .max_by(|&a, &b| k.pairs[a].error.total_cmp(&k.pairs[b].error))
        .unwrap();
    assert_eq!(worst, 2);
}

#[test]
fn output_serialises_to_camel_case_json() {
    let f = Figure::default();
    let g = grade(&f.line_art(), &f.photo()).unwrap();
    let v = serde_json::to_value(&g).unwrap();
    for key in [
        "score",
        "measurements",
        "grid",
        "alignment",
        "bbox",
        "flags",
        "passed",
    ] {
        assert!(v.get(key).is_some(), "{key}");
    }
    assert!(v["grid"].get("worst").is_some());
    assert!(v["measurements"][0].get("name").is_some());
    let opts: GradeOptions = serde_json::from_str(
        r#"{"category":"real-life","strokes":[{"brushId":"b","eraser":false,"points":[{"x":1,"y":2,"pressure":0.5,"t":3}]}],"keypoints":[{"reference":{"x":1,"y":2},"drawing":{"x":3,"y":4}}]}"#,
    )
    .unwrap();
    assert_eq!(opts.strokes.len(), 1);
    assert_eq!(opts.keypoints.len(), 1);
}

#[test]
fn category_weights_change_the_score() {
    use grader::Category;
    // A shaded drawing with the light coming from the wrong side: the
    // outline is right, the tone is not. Tone-heavy categories rate it lower.
    let (d, r) = (two_tone_disc(false), two_tone_disc(true));
    let score = |category| {
        grade_with(
            &d,
            &r,
            &GradeOptions {
                category,
                ..Default::default()
            },
        )
        .unwrap()
        .similarity
    };
    assert!(score(Category::Anime) > score(Category::Environment));
}

/// A disc (r 150, centred) shaded dark on one half and light on the other.
fn two_tone_disc(dark_left: bool) -> Gray {
    let mut g = white();
    for y in 150..450 {
        for x in 250..550 {
            let (px, py) = (x as f32 + 0.5 - 400.0, y as f32 + 0.5 - 300.0);
            if px * px + py * py <= 150.0 * 150.0 {
                let dark = (px < 0.0) == dark_left;
                g.pixels[y * 800 + x] = if dark { 50 } else { 190 };
            }
        }
    }
    g
}

/// M8-T01: tone counts in proportion to how much the student shaded. Pure
/// line art is not marked down for having no shading, but shading drawn the
/// wrong way round is.
#[test]
fn tone_counts_in_proportion_to_shading() {
    use grader::Category;
    let r = two_tone_disc(true);
    let opts = GradeOptions {
        category: Category::RealLife,
        ..Default::default()
    };
    let g = |d: &Gray, name: &str| {
        let g = grade_with(d, &r, &opts).unwrap();
        show(name, &g);
        g
    };
    let line = g(&circle_drawing((400.0, 300.0), 150.0), "tone: line art");
    let right = g(&r, "tone: shaded right");
    let wrong = g(&two_tone_disc(false), "tone: shaded wrong");
    let m = |g: &Grade, n: &str| g.measurement(n).unwrap();
    assert!(m(&line, "shading") < 0.1, "{}", m(&line, "shading"));
    assert!(m(&right, "shading") > 0.5, "{}", m(&right, "shading"));
    assert!(m(&wrong, "tone") < m(&right, "tone") - 0.3);
    assert!(line.score >= 8.5, "{}", line.score);
    assert!(right.similarity > wrong.similarity + 0.05);
    assert!(line.similarity > wrong.similarity);
}

#[test]
#[cfg_attr(debug_assertions, ignore = "timing only meaningful in release")]
fn grading_800x600_is_fast() {
    let f = Figure::default();
    let (d, r) = (
        Figure {
            dx: 40.0,
            scale: 0.9,
            ..f
        }
        .line_art(),
        f.photo(),
    );
    let pts = ellipse_points((400.0, 300.0), 150.0, 150.0, 0.0, 0.0);
    let opts = GradeOptions {
        strokes: strokes_from(&[pts.clone(), pts.clone(), pts], |_| 16.0),
        ..Default::default()
    };
    let _ = grade_with(&d, &r, &opts).unwrap(); // warm-up
    let n = 5;
    let t0 = std::time::Instant::now();
    for _ in 0..n {
        let _ = grade_with(&d, &r, &opts).unwrap();
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
    eprintln!("grade 800x600: {ms:.1} ms");
    assert!(ms < 300.0, "{ms} ms");
}

#[test]
fn faint_pencil_lines_count_as_ink() {
    // A light-grey circle (like a mouse-drawn pencil stroke) must not read as blank.
    let mut d = white();
    let pts = ellipse_points((400.0, 300.0), 150.0, 150.0, 0.0, 0.0);
    for w in pts.windows(2) {
        line(&mut d, w[0], w[1], 2.0, 215);
    }
    let g = run("faint circle", &d, &disc_ref());
    assert!(!g.flags.iter().any(|f| f == "blank"), "{:?}", g.flags);
    assert!(g.score >= 8.0, "{}", g.score);
}

/// M8-T01: a careful line copy of a shaded, photo-like reference (texture in
/// the background, moderate-contrast detail lines inside the subject) is a
/// good drawing. It has no shading, so tone must not drag it down, and its
/// correct detail lines must not count as "extra".
#[test]
fn line_copy_of_shaded_textured_photo_scores_high() {
    use grader::Category;
    let mut r = white();
    textured_background(&mut r, 5);
    shaded_subject(&mut r);
    let d = subject_line_art();
    for category in [Category::RealLife, Category::Animals, Category::Anime] {
        let g = grade_with(
            &d,
            &r,
            &GradeOptions {
                category,
                ..Default::default()
            },
        )
        .unwrap();
        show(&format!("line copy of photo {category:?}"), &g);
        assert!(g.score >= 8.0, "{category:?}: {}", g.score);
        assert!(g.measurement("shading").unwrap() < 0.1);
        assert!(g.measurement("outline_precision").unwrap() >= 0.8);
    }
    // The same copy without its detail lines is still decent but not better.
    let mut outline_only = white();
    circle(&mut outline_only, (400.0, 300.0), 150.0, 3.0);
    let full = grade(&d, &r).unwrap();
    let bare = run("outline only of photo", &outline_only, &r);
    assert!(bare.similarity <= full.similarity + 0.01);
}

/// M8-T01: the subject drawn right, on a reference whose background is busy
/// (grass, twigs): placement compares the subject, not the clutter.
#[test]
fn busy_reference_background_still_places_correctly() {
    let mut r = white();
    busy_background(&mut r, 11);
    shaded_disc(&mut r, (400.0, 300.0), 150.0, 4);
    let g = run(
        "circle vs busy background",
        &circle_drawing((400.0, 300.0), 150.0),
        &r,
    );
    let m = |n: &str| g.measurement(n).unwrap();
    assert!(m("placement") >= 0.85, "placement {}", m("placement"));
    assert!((m("scale_ratio") - 1.0).abs() < 0.1, "{}", m("scale_ratio"));
    assert!(m("offset_x").abs() < 0.03 && m("offset_y").abs() < 0.03);
    assert!(g.score >= 7.5, "{}", g.score);
    // Drawn small and off-centre on the same busy reference: placement sees it.
    let off = run(
        "small+shifted vs busy bg",
        &circle_drawing((480.0, 340.0), 110.0),
        &r,
    );
    let mo = |n: &str| off.measurement(n).unwrap();
    assert!(mo("scale_ratio") < 0.85, "{}", mo("scale_ratio"));
    assert!(mo("offset_x") > 0.07, "{}", mo("offset_x"));
    assert!(mo("placement") < m("placement") - 0.3);
}
