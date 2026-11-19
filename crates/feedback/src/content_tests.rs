//! Checks on the built-in rule + tip library (`content/feedback/`): links,
//! non-overlapping bands, category overrides, wording, and realistic
//! per-category scenarios.

use std::collections::{BTreeMap, BTreeSet};

use super::*;

const CATEGORIES: &[&str] = &[
    "anime",
    "cartoon",
    "real-life",
    "animals",
    "environment",
    "fundamentals",
];

/// Lessons that existed before M8 (`content/lessons/`).
const OLD_LESSONS: &[&str] = &[
    "big-shapes-first",
    "checking-as-you-go",
    "comparing-width-and-height",
    "drawing-from-the-shoulder",
    "draw-what-you-see",
    "finding-angles",
    "measuring-with-units",
    "one-confident-stroke",
    "planning-the-page",
    "seeing-the-whole-subject",
    "squint-for-values",
    "value-range",
];

/// Drills that existed before M8 (`apps/web/src/lib/drills.ts`).
const OLD_DRILLS: &[&str] = &[
    "ghosted-lines",
    "angle-matching",
    "ellipse-pages",
    "block-in-boxes",
    "four-corner-marks",
    "envelope-shapes",
    "silhouette-fill",
    "grid-copy",
    "five-step-value-scale",
    "three-value-thumbnail",
    "head-units",
    "contour-in-one-minute",
    "blind-contour",
];

/// Measurement names the grader emits (crates/grader `MEASUREMENT_DOCS`).
const MEASUREMENTS: &[&str] = &[
    "angles",
    "passes",
    "placement",
    "scale_ratio",
    "aspect_error",
    "offset_x",
    "offset_y",
    "outline",
    "outline_precision",
    "outline_recall",
    "outline_distance",
    "silhouette",
    "grid",
    "worst_cell",
    "tone",
    "contrast_ratio",
    "ssim",
    "ink_coverage",
    "structure",
    "relevance",
    "line_confidence",
    "smoothness",
    "curvature_jitter",
    "retrace_ratio",
    "speed_consistency",
    "speed_cv",
    "stroke_count",
    "keypoints",
    "keypoint_error",
    "keypoint_max_error",
    "angle_error",
];

const PLAN: &str = include_str!("../../../docs/planning/M8_CONTENT_PLAN.md");

/// New lesson and drill ids from the plan's two tables.
fn plan_ids() -> (BTreeSet<String>, BTreeSet<String>) {
    let mut lessons = BTreeSet::new();
    let mut drills = BTreeSet::new();
    let mut section = "";
    for line in PLAN.lines() {
        if line.starts_with("## ") {
            section = line;
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        // | id | … | → ["", "id", …, ""]
        if cells.len() < 3 || cells[1] == "id" || cells[1].starts_with("---") {
            continue;
        }
        if section.starts_with("## New lessons") {
            lessons.insert(cells[1].to_string());
            // the drill a lesson points to must be in the drill list too
            drills.insert(cells[cells.len() - 2].to_string());
        } else if section.starts_with("## New drills") {
            drills.insert(cells[1].to_string());
        }
    }
    (lessons, drills)
}

fn all_lessons() -> BTreeSet<String> {
    let (new, _) = plan_ids();
    OLD_LESSONS
        .iter()
        .map(|s| s.to_string())
        .chain(new)
        .collect()
}

fn all_drills() -> BTreeSet<String> {
    let (_, new) = plan_ids();
    OLD_DRILLS
        .iter()
        .map(|s| s.to_string())
        .chain(new)
        .collect()
}

fn lib() -> &'static Library {
    Library::builtin()
}

fn applies(r: &Rule, category: &str) -> bool {
    r.categories.is_empty() || r.categories.iter().any(|c| c == category)
}

fn m(pairs: &[(&str, f32)]) -> Vec<(String, f32)> {
    pairs.iter().map(|(n, v)| (n.to_string(), *v)).collect()
}

fn ids(f: &Feedback) -> Vec<&str> {
    f.fixes.iter().map(|i| i.id.as_str()).collect()
}

/// Values worth probing for a measurement: every threshold/bound and its
/// neighbours, plus a dense sweep (and negatives for `abs` rules).
fn probe_values(rules: &[&Rule]) -> Vec<f32> {
    let mut points: Vec<f32> = rules
        .iter()
        .flat_map(|r| [r.above, r.below, r.min, r.max])
        .flatten()
        .collect();
    points.push(0.0);
    let hi = points.iter().cloned().fold(0.0f32, f32::max) * 2.0 + 1.0;
    let mut out = Vec::new();
    for p in &points {
        for d in [-1e-3, -1e-5, 0.0, 1e-5, 1e-3] {
            out.push(p + d);
        }
    }
    let steps = 4000;
    for i in 0..=steps {
        out.push(-0.5 + (hi + 0.5) * i as f32 / steps as f32);
    }
    if rules.iter().any(|r| r.abs) {
        let neg: Vec<f32> = out.iter().map(|v| -v).collect();
        out.extend(neg);
    }
    out
}

#[test]
fn library_has_75_plus_rules_with_known_measurements_and_categories() {
    let l = lib();
    assert!(l.rules.len() >= 75, "{} rules", l.rules.len());
    for r in &l.rules {
        assert!(
            MEASUREMENTS.contains(&r.measurement.as_str()),
            "{}: unknown measurement {}",
            r.id,
            r.measurement
        );
        for c in &r.categories {
            assert!(CATEGORIES.contains(&c.as_str()), "{}: category {c}", r.id);
        }
        assert!(
            PRAISE_SKILLS.iter().any(|(s, _)| *s == r.skill),
            "{}: skill {}",
            r.id,
            r.skill
        );
        assert!((1.0..=5.0).contains(&r.severity), "{}", r.id);
        assert!(r.lesson.is_some() && r.drill.is_some(), "{}", r.id);
    }
    // every category gets rules for every skill (proportions/line rules only
    // fire when keypoints/strokes exist, but they must be there)
    for c in CATEGORIES {
        for (skill, _) in PRAISE_SKILLS {
            assert!(
                l.rules.iter().any(|r| applies(r, c) && r.skill == *skill),
                "no {skill} rule for {c}"
            );
        }
    }
}

#[test]
fn plan_lists_have_30_lessons_and_30_drills() {
    assert_eq!(all_lessons().len(), 30, "{:?}", all_lessons());
    assert_eq!(all_drills().len(), 30, "{:?}", all_drills());
}

#[test]
fn every_link_is_in_the_plan_and_every_new_id_is_reachable() {
    let (lessons, drills) = (all_lessons(), all_drills());
    let (new_lessons, new_drills) = plan_ids();
    let mut used_l = BTreeSet::new();
    let mut used_d = BTreeSet::new();
    for r in &lib().rules {
        let (l, d) = (r.lesson.clone().unwrap(), r.drill.clone().unwrap());
        assert!(lessons.contains(&l), "{}: lesson {l}", r.id);
        assert!(drills.contains(&d), "{}: drill {d}", r.id);
        used_l.insert(l);
        used_d.insert(d);
    }
    for id in &new_lessons {
        assert!(used_l.contains(id), "new lesson {id} is not linked");
    }
    for id in new_drills
        .iter()
        .filter(|d| !OLD_DRILLS.contains(&d.as_str()))
    {
        assert!(used_d.contains(id), "new drill {id} is not linked");
    }
}

#[test]
fn at_most_one_rule_fires_per_measurement_category_and_value() {
    let l = lib();
    let by_m: BTreeMap<&str, Vec<&Rule>> = l.rules.iter().fold(BTreeMap::new(), |mut acc, r| {
        acc.entry(r.measurement.as_str()).or_default().push(r);
        acc
    });
    for (meas, rules) in &by_m {
        let values = probe_values(rules);
        for c in CATEGORIES {
            let rs: Vec<&&Rule> = rules.iter().filter(|r| applies(r, c)).collect();
            for &v in &values {
                let fired: Vec<&str> = rs
                    .iter()
                    .filter(|r| severity(r, v).is_some())
                    .map(|r| r.id.as_str())
                    .collect();
                assert!(fired.len() <= 1, "{meas}={v} in {c}: {fired:?}");
            }
        }
    }
}

/// Some measurements are functions of each other; their rules must not
/// overlap either.
#[test]
fn linked_stroke_measurements_do_not_double_up() {
    let l = lib();
    let fired = |c: &str, ms: &[(&str, f32)]| -> Vec<&str> {
        l.rules
            .iter()
            .filter(|r| applies(r, c))
            .filter(|r| {
                ms.iter()
                    .any(|(n, v)| *n == r.measurement && severity(r, *v).is_some())
            })
            .map(|r| r.id.as_str())
            .collect()
    };
    for c in CATEGORIES {
        for i in 0..=3000 {
            // smoothness = exp(−jitter ÷ 0.35)
            let j = i as f32 * 0.0005;
            let got = fired(
                c,
                &[("curvature_jitter", j), ("smoothness", (-j / 0.35).exp())],
            );
            assert!(got.len() <= 1, "{c} jitter {j}: {got:?}");
            // speed_consistency = exp(−speed_cv)
            let cv = i as f32 * 0.001;
            let got = fired(c, &[("speed_cv", cv), ("speed_consistency", (-cv).exp())]);
            assert!(got.len() <= 1, "{c} speed_cv {cv}: {got:?}");
        }
    }
}

#[test]
fn bands_meet_without_gaps() {
    // (measurement, category, mild, strong, a value in each band)
    let cases: &[(&str, &str, &str, &str, f32, f32)] = &[
        (
            "scale_ratio",
            "anime",
            "a-bit-too-big",
            "too-big",
            1.2,
            1.31,
        ),
        (
            "scale_ratio",
            "real-life",
            "a-bit-too-small",
            "too-small",
            0.75,
            0.69,
        ),
        (
            "aspect_error",
            "animals",
            "wrong-width",
            "width-way-off",
            -0.2,
            -0.3,
        ),
        (
            "offset_x",
            "fundamentals",
            "off-sideways",
            "far-off-sideways",
            0.2,
            0.3,
        ),
        (
            "offset_y",
            "environment",
            "off-vertically",
            "far-off-vertically",
            -0.2,
            -0.3,
        ),
        (
            "outline_recall",
            "anime",
            "a-few-parts-missing",
            "missing-parts",
            0.6,
            0.5,
        ),
        (
            "outline_precision",
            "anime",
            "anime-stray-lines",
            "anime-messy-lineart",
            0.5,
            0.3,
        ),
        (
            "silhouette",
            "animals",
            "animal-shape-off",
            "animal-shape-way-off",
            0.45,
            0.3,
        ),
        (
            "worst_cell",
            "cartoon",
            "one-area-off",
            "one-area-way-off",
            0.3,
            0.2,
        ),
        (
            "contrast_ratio",
            "real-life",
            "a-bit-flat",
            "flat-shading",
            0.7,
            0.5,
        ),
        (
            "retrace_ratio",
            "animals",
            "sketchy-lines",
            "hairy-lines",
            0.3,
            0.4,
        ),
        (
            "keypoint_error",
            "real-life",
            "face-proportions-off",
            "face-proportions-way-off",
            0.1,
            0.13,
        ),
        (
            "angle_error",
            "environment",
            "perspective-angles-off",
            "perspective-angles-way-off",
            10.0,
            20.0,
        ),
    ];
    for &(meas, cat, mild, strong, v_mild, v_strong) in cases {
        let rules: Vec<&Rule> = lib()
            .rules
            .iter()
            .filter(|r| r.measurement == meas && applies(r, cat))
            .collect();
        let only = |v: f32| -> Vec<&str> {
            rules
                .iter()
                .filter(|r| severity(r, v).is_some())
                .map(|r| r.id.as_str())
                .collect()
        };
        assert_eq!(only(v_mild), [mild], "{meas} {v_mild} in {cat}");
        assert_eq!(only(v_strong), [strong], "{meas} {v_strong} in {cat}");
        // walking from the mild value to the strong one never leaves a gap
        for i in 0..=1000 {
            let v = v_mild + (v_strong - v_mild) * i as f32 / 1000.0;
            assert_eq!(only(v).len(), 1, "gap at {meas}={v} in {cat}");
        }
        // the strong band starts at least as severe as the mild band ends
        let mild_rule = rules.iter().find(|r| r.id == mild).unwrap();
        let strong_rule = rules.iter().find(|r| r.id == strong).unwrap();
        assert!(
            strong_rule.severity >= mild_rule.severity,
            "{strong} vs {mild}"
        );
    }
}

#[test]
fn category_overrides_replace_the_generic_rule() {
    // (measurement, value, category → expected rule)
    // (measurement, value, [(category, expected rule)])
    type Case<'a> = (&'a str, f32, &'a [(&'a str, &'a str)]);
    let cases: &[Case] = &[
        (
            "scale_ratio",
            1.4,
            &[
                ("anime", "too-big"),
                ("cartoon", "cartoon-too-big"),
                ("environment", "scene-cropped"),
            ],
        ),
        (
            "outline_precision",
            0.45,
            &[
                ("real-life", "extra-lines"),
                ("anime", "anime-stray-lines"),
                ("cartoon", "cartoon-stray-lines"),
            ],
        ),
        (
            "silhouette",
            0.45,
            &[
                ("real-life", "big-shapes-off"),
                ("animals", "animal-shape-off"),
                ("cartoon", "cartoon-shape-off"),
                ("environment", "scene-masses-off"),
            ],
        ),
        (
            "contrast_ratio",
            0.4,
            &[
                ("real-life", "flat-shading"),
                ("animals", "flat-shading"),
                ("environment", "scene-flat"),
            ],
        ),
        (
            "keypoint_error",
            0.09,
            &[
                ("anime", "anime-proportions-off"),
                ("real-life", "face-proportions-off"),
                ("animals", "animal-proportions-off"),
                ("environment", "scene-landmarks-off"),
                ("fundamentals", "proportions-off"),
            ],
        ),
        (
            "angle_error",
            13.0,
            &[
                ("real-life", "angles-off"),
                ("environment", "perspective-angles-off"),
                ("animals", "animal-pose-off"),
                ("cartoon", "cartoon-angles-off"),
            ],
        ),
    ];
    for (meas, v, expect) in cases {
        for (cat, id) in *expect {
            // a shaded drawing, so shading rules (gated on `shading`) are judged too
            let f = feedback(lib(), &m(&[(meas, *v), ("shading", 0.5)]), cat, 0);
            assert_eq!(ids(&f), [*id], "{meas}={v} in {cat}");
        }
    }
    // cartoon tolerates more: these fire for anime but not for cartoon
    for (meas, v) in [
        ("scale_ratio", 1.2),
        ("aspect_error", 0.18),
        ("offset_x", 0.15),
        ("keypoint_error", 0.08),
        ("angle_error", 10.0),
    ] {
        assert!(
            !feedback(lib(), &m(&[(meas, v)]), "anime", 0)
                .fixes
                .is_empty()
        );
        assert!(
            feedback(lib(), &m(&[(meas, v)]), "cartoon", 0)
                .fixes
                .is_empty()
        );
    }
    // line art is held to a higher standard than real-life sketching
    for (meas, v) in [
        ("outline_precision", 0.55),
        ("retrace_ratio", 0.15),
        ("smoothness", 0.55),
    ] {
        assert!(
            !feedback(lib(), &m(&[(meas, v)]), "anime", 0)
                .fixes
                .is_empty()
        );
        assert!(
            feedback(lib(), &m(&[(meas, v)]), "real-life", 0)
                .fixes
                .is_empty()
        );
    }
}

#[test]
fn tips_have_variants_numbers_and_no_unfilled_placeholders() {
    let l = lib();
    for r in &l.rules {
        let tip = &l.tips[&r.id];
        assert!(
            (2..=4).contains(&tip.variants.len()),
            "{}: {} variants",
            r.id,
            tip.variants.len()
        );
        // ink coverage is a tiny fraction and exclusive tips (not the reference) quote no number
        if r.measurement != "ink_coverage" && !r.exclusive {
            assert!(
                tip.variants.iter().all(|v| v.contains("{n}")),
                "{}: every variant should quote the number",
                r.id
            );
        }
        let probes = probe_values(&[r]);
        let v = probes
            .iter()
            .copied()
            .find(|v| severity(r, *v).is_some())
            .unwrap_or_else(|| panic!("{} never fires", r.id));
        for (i, text) in tip.variants.iter().enumerate() {
            let words = text.split_whitespace().count();
            assert!(words <= 60, "{} variant {i}: {words} words", r.id);
            let filled = fill(text, r, v);
            assert!(!filled.contains(['{', '}']), "{}: {filled}", r.id);
            if r.dir_pos.is_some() {
                assert!(
                    text.contains("{dir}"),
                    "{} has dir words but no {{dir}}",
                    r.id
                );
            }
        }
        assert!(!fill(&tip.title, r, v).contains(['{', '}']), "{}", r.id);
        // the UI strips {n} from titles: keep them readable without it
        assert!(!tip.title.contains("({n}"), "{}: {}", r.id, tip.title);
    }
    // every fired fix in a full sweep reads cleanly
    for c in CATEGORIES {
        for r in l.rules.iter().filter(|r| applies(r, c)) {
            for v in probe_values(&[r]).into_iter().step_by(97) {
                let f = feedback(l, &[(r.measurement.clone(), v)], c, v.to_bits());
                for item in f.fixes.iter().chain(f.praise.iter()) {
                    assert!(!(item.title.clone() + &item.text).contains(['{', '}']));
                }
            }
        }
    }
}

#[test]
fn every_tip_belongs_to_a_rule_or_praise() {
    let l = lib();
    for key in l.tips.keys() {
        if l.rules.iter().any(|r| &r.id == key) {
            continue;
        }
        let rest = key
            .strip_prefix("praise-")
            .unwrap_or_else(|| panic!("tip {key} has no rule"));
        let ok = PRAISE_SKILLS.iter().any(|(s, _)| {
            rest == *s
                || rest
                    .strip_prefix(s)
                    .and_then(|c| c.strip_prefix('-'))
                    .is_some_and(|c| CATEGORIES.contains(&c))
        });
        assert!(ok, "praise tip {key} names no skill/category");
    }
    for (skill, _) in PRAISE_SKILLS {
        assert!(
            l.tips[&format!("praise-{skill}")].variants.len() >= 2,
            "{skill}"
        );
    }
}

#[test]
fn category_praise_replaces_generic_praise() {
    let good = m(&[("outline", 0.92), ("silhouette", 0.8)]);
    let anime = feedback(lib(), &good, "anime", 3).praise.unwrap();
    assert_eq!(anime.id, "praise-outline");
    assert!(
        lib().tips["praise-outline-anime"]
            .variants
            .contains(&anime.text)
    );
    let plain = feedback(lib(), &good, "real-life", 3).praise.unwrap();
    assert_eq!(plain.id, "praise-outline");
    assert!(lib().tips["praise-outline"].variants.contains(&plain.text));
}

#[test]
fn radians_and_counts_are_formatted() {
    let f = feedback(lib(), &m(&[("curvature_jitter", 0.2)]), "real-life", 0);
    assert_eq!(ids(&f), ["a-bit-wobbly"]);
    assert!(f.fixes[0].text.contains("11°"), "{}", f.fixes[0].text);
    let f = feedback(lib(), &m(&[("stroke_count", 420.0)]), "anime", 0);
    assert_eq!(ids(&f), ["many-tiny-strokes"]);
    assert!(f.fixes[0].text.contains("420"), "{}", f.fixes[0].text);
}

// ── Realistic drawings, one per category ────────────────────────────

/// A decent drawing: nothing here should fire anywhere.
fn decent() -> Vec<(&'static str, f32)> {
    vec![
        // a shaded drawing, so shading rules (gated on `shading`) are judged
        ("shading", 0.5),
        ("placement", 0.85),
        ("scale_ratio", 1.06),
        ("aspect_error", -0.05),
        ("offset_x", 0.04),
        ("offset_y", -0.03),
        ("outline", 0.74),
        ("outline_precision", 0.7),
        ("outline_recall", 0.75),
        ("outline_distance", 0.015),
        ("silhouette", 0.72),
        ("grid", 0.66),
        ("worst_cell", 0.5),
        ("tone", 0.6),
        ("contrast_ratio", 0.9),
        ("ssim", 0.55),
        ("ink_coverage", 0.06),
        ("line_confidence", 0.75),
        ("smoothness", 0.75),
        ("curvature_jitter", 0.1),
        ("retrace_ratio", 0.08),
        ("speed_consistency", 0.6),
        ("speed_cv", 0.51),
        ("stroke_count", 45.0),
        ("keypoints", 0.85),
        ("keypoint_error", 0.035),
        ("keypoint_max_error", 0.08),
        ("angle_error", 4.0),
    ]
}

fn with(changes: &[(&'static str, f32)]) -> Vec<(String, f32)> {
    let mut base = decent();
    for (n, v) in changes {
        base.iter_mut().find(|(k, _)| k == n).unwrap().1 = *v;
    }
    m(&base)
}

#[test]
fn a_decent_drawing_gets_no_fixes_in_any_category() {
    for c in CATEGORIES {
        let f = feedback(lib(), &with(&[]), c, 0);
        assert!(f.fixes.is_empty(), "{c}: {:?}", ids(&f));
        assert!(f.praise.is_some(), "{c}");
    }
}

#[test]
fn anime_drawing_with_messy_line_art() {
    let f = feedback(
        lib(),
        &with(&[
            ("outline_precision", 0.55),
            ("smoothness", 0.55),
            ("curvature_jitter", 0.21),
            ("retrace_ratio", 0.2),
            ("keypoint_error", 0.07),
        ]),
        "anime",
        2,
    );
    assert_eq!(
        ids(&f),
        [
            "anime-proportions-off",
            "anime-stray-lines",
            "shaky-lineart"
        ]
    );
}

#[test]
fn cartoon_drawing_with_some_exaggeration() {
    let ms = with(&[
        ("scale_ratio", 1.2),
        ("aspect_error", 0.18),
        ("keypoint_error", 0.09),
        ("outline_precision", 0.5),
        ("silhouette", 0.45),
    ]);
    let f = feedback(lib(), &ms, "cartoon", 0);
    assert_eq!(ids(&f), ["cartoon-stray-lines", "cartoon-shape-off"]);
    // the same drawing judged as anime gets placement and proportion fixes
    let f = feedback(lib(), &ms, "anime", 0);
    let got = ids(&f);
    assert!(got.contains(&"anime-proportions-off"), "{got:?}");
    assert!(got.contains(&"big-shapes-off"), "{got:?}");
    assert_eq!(f.fixes.len(), 3);
}

#[test]
fn real_life_drawing_with_timid_shading_and_a_hand_off() {
    let f = feedback(
        lib(),
        &with(&[
            ("contrast_ratio", 0.7),
            ("tone", 0.3),
            ("retrace_ratio", 0.4),
            ("keypoint_error", 0.05),
            ("keypoint_max_error", 0.34),
        ]),
        "real-life",
        0,
    );
    // retracing while shading is allowed for real life (no line fix)
    assert_eq!(ids(&f), ["hand-or-foot-off", "lights-and-darks-misplaced"]);
    let f = feedback(lib(), &with(&[("contrast_ratio", 0.7)]), "real-life", 0);
    assert_eq!(ids(&f), ["a-bit-flat"]);
    assert_eq!(f.fixes[0].lesson.as_deref(), Some("hatching"));
}

#[test]
fn animal_drawing_with_stiff_lines_and_body_off() {
    let f = feedback(
        lib(),
        &with(&[
            ("silhouette", 0.42),
            ("speed_cv", 2.0),
            ("speed_consistency", 0.135),
            ("angle_error", 12.0),
        ]),
        "animals",
        0,
    );
    assert_eq!(
        ids(&f),
        [
            "animal-shape-off",
            "animal-stiff-strokes",
            "animal-pose-off"
        ]
    );
    assert_eq!(
        f.fixes[0].drill.as_deref(),
        Some("animal-bean-construction")
    );
}

#[test]
fn environment_drawing_without_atmosphere() {
    let f = feedback(
        lib(),
        &with(&[
            ("contrast_ratio", 1.7),
            ("angle_error", 10.0),
            ("outline_recall", 0.5),
            ("scale_ratio", 1.25),
        ]),
        "environment",
        0,
    );
    assert_eq!(
        ids(&f),
        [
            "perspective-angles-off",
            "scene-too-contrasty",
            "scene-cropped"
        ]
    );
    assert_eq!(
        f.fixes[1].lesson.as_deref(),
        Some("atmospheric-perspective")
    );
}

#[test]
fn fundamentals_drill_with_many_scratchy_strokes() {
    let f = feedback(
        lib(),
        &with(&[
            ("outline_distance", 0.05),
            ("stroke_count", 100.0),
            ("retrace_ratio", 0.25),
        ]),
        "fundamentals",
        0,
    );
    assert_eq!(ids(&f), ["lines-off-target", "many-strokes-simple-shape"]);
}
