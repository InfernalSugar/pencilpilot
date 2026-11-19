//! "Does the drawing depict the reference?" (the relevance gate).
//!
//! Fixtures (`common/fixtures.rs`): for four references, (a) good copies,
//! (b) rough, shifted, partial or sketchy genuine attempts and (c) unrelated
//! drawings covering the same area (scribbles, hatching, other objects).
//! Run with `--nocapture` to see the table: score without the gate → with
//! it, plus the measurements the gate replaced as a signal.

mod common;

use common::fixtures::*;
use grader::{Grade, GradeOptions, Mode, grade_with};

struct Row {
    case: &'static str,
    name: String,
    group: Group,
    mode: Mode,
    before: Grade,
    after: Grade,
}

/// Every drawing of every case, graded with and without the gate (one
/// thread per case: hundreds of 800×600 grades are slow in debug builds).
fn rows() -> Vec<Row> {
    let cases = cases();
    std::thread::scope(|s| {
        let handles: Vec<_> = cases
            .iter()
            .map(|case| {
                s.spawn(move || {
                    let mut out = Vec::new();
                    for d in &case.drawings {
                        let img = d.render();
                        for mode in [Mode::Finished, Mode::Sketch] {
                            let opts = |gate| GradeOptions {
                                mode,
                                strokes: d.strokes(),
                                relevance_gate: Some(gate),
                                ..Default::default()
                            };
                            out.push(Row {
                                case: case.name,
                                name: d.name.clone(),
                                group: d.group,
                                mode,
                                before: grade_with(&img, &case.reference, &opts(false)).unwrap(),
                                after: grade_with(&img, &case.reference, &opts(true)).unwrap(),
                            });
                        }
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    })
}

#[test]
fn unrelated_drawings_score_low_and_genuine_ones_keep_their_score() {
    let rows = rows();
    let mut current = "";
    for r in &rows {
        if r.case != current {
            current = r.case;
            eprintln!("== {current}");
        }
        let m = |n: &str| r.after.measurement(n).unwrap_or(f32::NAN);
        eprintln!(
            "{:<9} {:<24} {:<8} score {:5.2} → {:5.2}  structure {:+.2} relevance {:.2}  outline {:.2} silhouette {:.2} tone {:.2} {:?}",
            format!("{:?}", r.group),
            r.name,
            format!("{:?}", r.mode),
            r.before.score,
            r.after.score,
            m("structure"),
            m("relevance"),
            m("outline"),
            m("silhouette"),
            m("tone"),
            r.after.flags,
        );
    }
    // Summary per group and mode: mean (min–max) before → after.
    for group in [Group::Good, Group::Rough, Group::Unrelated] {
        for mode in [Mode::Finished, Mode::Sketch] {
            let sel: Vec<&Row> = rows
                .iter()
                .filter(|r| r.group == group && r.mode == mode)
                .collect();
            let stat = |f: &dyn Fn(&Row) -> f32| {
                let v: Vec<f32> = sel.iter().map(|r| f(r)).collect();
                let mean = v.iter().sum::<f32>() / v.len() as f32;
                let lo = v.iter().copied().fold(f32::MAX, f32::min);
                let hi = v.iter().copied().fold(f32::MIN, f32::max);
                format!("{mean:.2} ({lo:.2}–{hi:.2})")
            };
            eprintln!(
                "{group:?} {mode:?}: before {}  after {}  relevance {}",
                stat(&|r| r.before.score),
                stat(&|r| r.after.score),
                stat(&|r| r.after.measurement("relevance").unwrap()),
            );
        }
    }

    for r in &rows {
        let what = format!("{} / {} / {:?}", r.case, r.name, r.mode);
        let flagged = r.after.flags.iter().any(|f| f == "unrelated");
        match r.group {
            // Genuine attempts: the gate changes nothing.
            Group::Good | Group::Rough => {
                assert!(!flagged, "{what}: flagged unrelated");
                assert!(
                    (r.after.score - r.before.score).abs() < 0.05,
                    "{what}: {} → {}",
                    r.before.score,
                    r.after.score
                );
            }
            Group::Unrelated => {}
        }
    }
    // Unrelated drawings: scribbles and hatching that cover the subject's
    // area are flagged and land at about 1–2 (≤ 2.2). (Drawing a house over a disc, or
    // scribbling inside a stick figure's thin limbs, can resemble the
    // reference a little; those are only required to score lower.)
    for r in rows.iter().filter(|r| r.group == Group::Unrelated) {
        let what = format!("{} / {} / {:?}", r.case, r.name, r.mode);
        let strict = (r.name.contains("scribbles") && !r.name.contains("silhouette"))
            || r.name.starts_with("hatching 45")
            || r.name == "scribble+hatching"
            || r.name == "a car";
        if strict {
            assert!(
                r.after.flags.iter().any(|f| f == "unrelated"),
                "{what}: not flagged ({:?})",
                r.after.measurement("structure")
            );
            assert!(r.after.score <= 2.2, "{what}: {}", r.after.score);
        }
        assert!(r.after.score <= r.before.score + 1e-4, "{what}");
    }
    // The user's report: dense scribbles over an anime picture.
    let user = rows
        .iter()
        .find(|r| {
            r.case == "anime bust" && r.name == "scribbles in area" && r.mode == Mode::Finished
        })
        .unwrap();
    assert!(user.after.score <= 2.0, "{}", user.after.score);
}

/// Value drills grade shading, not lines: with the gate off the score is
/// left alone, but the measurement is still reported.
#[test]
fn gate_can_be_turned_off() {
    let case = &cases()[3];
    let d = case
        .drawings
        .iter()
        .find(|d| d.name == "scribbles in area")
        .unwrap();
    let off = grade_with(
        &d.render(),
        &case.reference,
        &GradeOptions {
            relevance_gate: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!off.flags.iter().any(|f| f == "unrelated"));
    assert!(off.measurement("relevance").unwrap() < 0.5);
    let on = grade_with(&d.render(), &case.reference, &GradeOptions::default()).unwrap();
    assert!(on.score < off.score);
    let json: GradeOptions = serde_json::from_str(r#"{"relevanceGate":false}"#).unwrap();
    assert_eq!(json.relevance_gate, Some(false));
}
