//! PencilPilot feedback engine: grader measurements → ranked, human-written tips.
//!
//! No AI (ADR-0002). Rules and tip text are data in `content/feedback/`
//! (`rules.toml`, `tips.toml`), embedded at compile time so the same engine
//! runs natively and in the browser (via `grader-wasm`).
//!
//! Output: the top fixes (at most one per skill, ranked by severity scaled by
//! how far past the threshold the measurement is) plus one piece of praise for
//! the student's strongest skill.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const RULES_TOML: &str = include_str!("../../../content/feedback/rules.toml");
const TIPS_TOML: &str = include_str!("../../../content/feedback/tips.toml");

/// How the measured value becomes the `{n}` placeholder.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NumberFormat {
    /// |value − 1| × 100
    RatioPct,
    /// |value| × 100
    AbsPct,
    /// value × 100
    Pct,
    /// value as-is (degrees)
    Deg,
    /// value × 180 / π (radians → degrees)
    RadDeg,
    /// value as-is (a count, e.g. strokes)
    Count,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub id: String,
    pub measurement: String,
    pub above: Option<f32>,
    pub below: Option<f32>,
    #[serde(default)]
    pub abs: bool,
    /// only fire when the value is at least this
    pub min: Option<f32>,
    /// only fire when the value is at most this
    pub max: Option<f32>,
    pub severity: f32,
    /// how far past the threshold counts as maximally bad (default: |threshold|)
    pub span: Option<f32>,
    pub skill: String,
    pub number: NumberFormat,
    pub dir_pos: Option<String>,
    pub dir_neg: Option<String>,
    pub overlay: Option<String>,
    pub lesson: Option<String>,
    pub drill: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    /// Optional second measurement that must be in range for the rule to apply,
    /// e.g. `gate = "shading"`, `gate_min = 0.1` (only judge shading that exists).
    pub gate: Option<String>,
    pub gate_min: Option<f32>,
    pub gate_max: Option<f32>,
    /// Drawing modes the rule applies to (`finished`, `sketch`). Empty = both,
    /// except rules on [`FINISHED_ONLY`] measurements, which skip sketches.
    #[serde(default)]
    pub modes: Vec<String>,
}

/// Measurements a sketch isn't judged on (ADR-0007): line neatness and
/// shading. Their rules and praise only apply to finished drawings unless a
/// rule lists `modes` itself.
pub const FINISHED_ONLY: &[&str] = &[
    "tone",
    "contrast_ratio",
    "ssim",
    "shading",
    "line_confidence",
    "smoothness",
    "curvature_jitter",
    "retrace_ratio",
    "speed_consistency",
    "speed_cv",
    "stroke_count",
];

impl Rule {
    /// Whether this rule is used for drawings of `mode` ("finished" or "sketch").
    pub fn applies_to_mode(&self, mode: &str) -> bool {
        if !self.modes.is_empty() {
            return self.modes.iter().any(|m| m == mode);
        }
        mode != "sketch"
            || !(FINISHED_ONLY.contains(&self.measurement.as_str())
                || self
                    .gate
                    .as_deref()
                    .is_some_and(|g| FINISHED_ONLY.contains(&g)))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tip {
    pub title: String,
    pub variants: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RulesFile {
    rule: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
struct TipsFile {
    tip: BTreeMap<String, Tip>,
}

/// The parsed rule + tip library.
#[derive(Debug)]
pub struct Library {
    pub rules: Vec<Rule>,
    pub tips: BTreeMap<String, Tip>,
}

impl Library {
    pub fn parse(rules: &str, tips: &str) -> Result<Self, String> {
        let r: RulesFile = toml::from_str(rules).map_err(|e| format!("rules.toml: {e}"))?;
        let t: TipsFile = toml::from_str(tips).map_err(|e| format!("tips.toml: {e}"))?;
        let lib = Self {
            rules: r.rule,
            tips: t.tip,
        };
        lib.validate()?;
        Ok(lib)
    }

    /// The library built into the binary from `content/feedback/`.
    pub fn builtin() -> &'static Library {
        static LIB: OnceLock<Library> = OnceLock::new();
        LIB.get_or_init(|| {
            Library::parse(RULES_TOML, TIPS_TOML).expect("built-in feedback content")
        })
    }

    fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for r in &self.rules {
            if !seen.insert(&r.id) {
                return Err(format!("duplicate rule id {}", r.id));
            }
            if r.above.is_none() == r.below.is_none() {
                return Err(format!("rule {} needs exactly one of above/below", r.id));
            }
            if r.gate.is_some() && r.gate_min.is_none() && r.gate_max.is_none() {
                return Err(format!("rule {} has a gate but no gate_min/gate_max", r.id));
            }
            if let (Some(lo), Some(hi)) = (r.min, r.max)
                && lo > hi
            {
                return Err(format!("rule {} has min > max", r.id));
            }
            let tip = self
                .tips
                .get(&r.id)
                .ok_or_else(|| format!("rule {} has no tip", r.id))?;
            if tip.variants.is_empty() {
                return Err(format!("tip {} has no variants", r.id));
            }
            for text in tip.variants.iter().chain([&tip.title]) {
                if text.contains("{dir}") && (r.dir_pos.is_none() || r.dir_neg.is_none()) {
                    return Err(format!(
                        "tip {} uses {{dir}} but the rule has no dir_pos/dir_neg",
                        r.id
                    ));
                }
            }
        }
        Ok(())
    }
}

/// One piece of feedback.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// rule id, or `praise-<skill>`
    pub id: String,
    pub skill: String,
    pub title: String,
    pub text: String,
    /// 0–10 ranking weight (praise: 0)
    pub severity: f32,
    pub measurement: String,
    pub value: f32,
    pub overlay: Option<String>,
    pub lesson: Option<String>,
    pub drill: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Feedback {
    pub fixes: Vec<Item>,
    pub praise: Option<Item>,
}

/// Skills that can earn praise, with the 0–1 similarity that measures each.
pub const PRAISE_SKILLS: &[(&str, &str)] = &[
    ("placement", "placement"),
    ("outline", "outline"),
    ("silhouette", "silhouette"),
    ("tone", "tone"),
    ("line_confidence", "line_confidence"),
    ("proportions", "keypoints"),
];
/// A skill needs at least this similarity to be praised. The praise text
/// comes from `praise-<skill>-<category>` when that tip exists, else from
/// `praise-<skill>`; the item id is always `praise-<skill>`.
pub const PRAISE_MIN: f32 = 0.7;
pub const MAX_FIXES: usize = 3;

/// Severity of a fired rule: base × (1 + min(1, distance past the threshold ÷ span)).
fn severity(rule: &Rule, v: f32) -> Option<f32> {
    let x = if rule.abs { v.abs() } else { v };
    if rule.min.is_some_and(|m| x < m) || rule.max.is_some_and(|m| x > m) {
        return None;
    }
    let (fired, excess) = match (rule.above, rule.below) {
        (Some(t), _) => (x > t, (x - t) / rule.span.unwrap_or(t.abs()).max(1e-3)),
        (_, Some(t)) => (x < t, (t - x) / rule.span.unwrap_or(t.abs()).max(1e-3)),
        _ => (false, 0.0),
    };
    fired.then(|| rule.severity * (1.0 + excess.clamp(0.0, 1.0)))
}

fn fill(template: &str, rule: &Rule, v: f32) -> String {
    let n = match rule.number {
        NumberFormat::RatioPct => (v - 1.0).abs() * 100.0,
        NumberFormat::AbsPct => v.abs() * 100.0,
        NumberFormat::Pct => v * 100.0,
        NumberFormat::Deg | NumberFormat::Count => v,
        NumberFormat::RadDeg => v.to_degrees(),
    };
    let dir = if v >= 0.0 {
        rule.dir_pos.as_deref()
    } else {
        rule.dir_neg.as_deref()
    }
    .unwrap_or("");
    template
        .replace("{n}", &format!("{}", n.round() as i64))
        .replace("{dir}", dir)
}

/// Pick a variant deterministically from `seed`, so one result reads the
/// same on reload but different attempts get different wordings.
fn variant(tip: &Tip, seed: u32, salt: usize) -> &str {
    let i = (seed as usize).wrapping_add(salt.wrapping_mul(7919)) % tip.variants.len();
    &tip.variants[i]
}

/// Build feedback for one graded finished drawing.
pub fn feedback(
    lib: &Library,
    measurements: &[(String, f32)],
    category: &str,
    seed: u32,
) -> Feedback {
    feedback_for(lib, measurements, category, "finished", seed)
}

/// Build feedback for one graded drawing of `mode` ("finished" or "sketch").
pub fn feedback_for(
    lib: &Library,
    measurements: &[(String, f32)],
    category: &str,
    mode: &str,
    seed: u32,
) -> Feedback {
    let get = |name: &str| {
        measurements
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
            .filter(|v| v.is_finite())
    };

    // A blank canvas only gets the blank tip.
    let blank = lib.rules.iter().find(|r| r.id == "blank-canvas");
    let mut fired: Vec<(f32, usize, &Rule, f32)> = Vec::new();
    for (i, r) in lib.rules.iter().enumerate() {
        if !r.categories.is_empty() && !r.categories.iter().any(|c| c == category) {
            continue;
        }
        if !r.applies_to_mode(mode) {
            continue;
        }
        if let Some(g) = &r.gate {
            // the gate measurement must exist and be inside [gate_min, gate_max]
            let Some(gv) = get(g) else { continue };
            if r.gate_min.is_some_and(|m| gv < m) || r.gate_max.is_some_and(|m| gv > m) {
                continue;
            }
        }
        let Some(v) = get(&r.measurement) else {
            continue;
        };
        if let Some(s) = severity(r, v) {
            if Some(r) == blank.filter(|b| b.id == r.id) {
                let tip = &lib.tips[&r.id];
                return Feedback {
                    fixes: vec![item(r, tip, v, s, seed, i)],
                    praise: None,
                };
            }
            fired.push((s, i, r, v));
        }
    }
    // Highest severity first; ties keep file order.
    fired.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut fixes = Vec::new();
    let mut skills = Vec::new();
    for (s, i, r, v) in fired {
        if skills.contains(&r.skill) {
            continue;
        }
        skills.push(r.skill.clone());
        fixes.push(item(r, &lib.tips[&r.id], v, s, seed, i));
        if fixes.len() == MAX_FIXES {
            break;
        }
    }

    let praise = PRAISE_SKILLS
        .iter()
        .filter(|(skill, _)| !skills.iter().any(|s| s == skill))
        .filter(|(_, m)| mode != "sketch" || !FINISHED_ONLY.contains(m))
        .filter_map(|(skill, m)| get(m).map(|v| (*skill, *m, v)))
        .filter(|(_, _, v)| *v >= PRAISE_MIN)
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .and_then(|(skill, m, v)| {
            let tip = lib
                .tips
                .get(&format!("praise-{skill}-{category}"))
                .or_else(|| lib.tips.get(&format!("praise-{skill}")))?;
            Some(Item {
                id: format!("praise-{skill}"),
                skill: skill.to_string(),
                title: tip.title.clone(),
                text: variant(tip, seed, 99).to_string(),
                severity: 0.0,
                measurement: m.to_string(),
                value: v,
                overlay: None,
                lesson: None,
                drill: None,
            })
        });

    Feedback { fixes, praise }
}

#[cfg(test)]
mod mode_tests {
    use super::*;

    fn m(v: &[(&str, f32)]) -> Vec<(String, f32)> {
        v.iter().map(|(n, x)| (n.to_string(), *x)).collect()
    }

    #[test]
    fn sketches_skip_line_and_shading_tips_and_get_their_own() {
        let lib = Library::builtin();
        let shaky = m(&[
            ("line_confidence", 0.1),
            ("smoothness", 0.1),
            ("shading", 0.5),
            ("tone", 0.2),
        ]);
        assert!(
            !feedback_for(lib, &shaky, "animals", "finished", 0)
                .fixes
                .is_empty()
        );
        assert!(
            feedback_for(lib, &shaky, "animals", "sketch", 0)
                .fixes
                .is_empty()
        );

        let rough = m(&[("passes", 5.0), ("angles", 0.6)]);
        let ids: Vec<String> = feedback_for(lib, &rough, "animals", "sketch", 0)
            .fixes
            .into_iter()
            .map(|f| f.id)
            .collect();
        assert!(ids.contains(&"sketch-angles-off".to_string()), "{ids:?}");
        assert!(
            feedback_for(lib, &rough, "animals", "finished", 0)
                .fixes
                .is_empty()
        );
    }
}

impl PartialEq for Rule {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

fn item(r: &Rule, tip: &Tip, v: f32, s: f32, seed: u32, salt: usize) -> Item {
    Item {
        id: r.id.clone(),
        skill: r.skill.clone(),
        title: fill(&tip.title, r, v),
        text: fill(variant(tip, seed, salt), r, v),
        severity: (s * 10.0).round() / 10.0,
        measurement: r.measurement.clone(),
        value: v,
        overlay: r.overlay.clone(),
        lesson: r.lesson.clone(),
        drill: r.drill.clone(),
    }
}

#[cfg(test)]
mod content_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(&str, f32)]) -> Vec<(String, f32)> {
        pairs.iter().map(|(n, v)| (n.to_string(), *v)).collect()
    }
    fn ids(f: &Feedback) -> Vec<&str> {
        f.fixes.iter().map(|i| i.id.as_str()).collect()
    }
    fn lib() -> &'static Library {
        Library::builtin()
    }

    #[test]
    fn builtin_content_parses_and_is_consistent() {
        let l = lib();
        assert!(l.rules.len() >= 20, "{} rules", l.rules.len());
        for (skill, _) in PRAISE_SKILLS {
            assert!(
                l.tips.contains_key(&format!("praise-{skill}")),
                "praise for {skill}"
            );
        }
    }

    #[test]
    fn a_good_drawing_gets_praise_and_no_fixes() {
        let f = feedback(
            lib(),
            &m(&[
                ("scale_ratio", 1.02),
                ("aspect_error", 0.01),
                ("offset_x", 0.0),
                ("offset_y", 0.01),
                ("placement", 0.95),
                ("outline", 0.9),
                ("outline_recall", 0.9),
                ("outline_precision", 0.85),
                ("silhouette", 0.88),
                ("worst_cell", 0.8),
                ("ink_coverage", 0.05),
                ("line_confidence", 0.8),
                ("smoothness", 0.8),
            ]),
            "animals",
            1,
        );
        assert!(f.fixes.is_empty(), "{:?}", ids(&f));
        let p = f.praise.unwrap();
        assert_eq!(p.skill, "placement");
    }

    #[test]
    fn numbers_and_directions_are_filled_in() {
        let f = feedback(
            lib(),
            &m(&[("scale_ratio", 1.32), ("aspect_error", -0.2)]),
            "anime",
            0,
        );
        // one fix per skill: both are placement → only the more severe one
        assert_eq!(f.fixes.len(), 1);
        let fix = &f.fixes[0];
        assert_eq!(fix.id, "too-big");
        assert_eq!(fix.title, "Your drawing is 32% too big");
        assert!(fix.text.contains("32%"), "{}", fix.text);
        let f = feedback(lib(), &m(&[("aspect_error", -0.2)]), "anime", 0);
        assert_eq!(f.fixes[0].title, "It’s 20% too narrow");
        assert!(!f.fixes[0].text.contains('{'), "{}", f.fixes[0].text);
    }

    #[test]
    fn ranks_by_severity_one_per_skill_max_three() {
        let f = feedback(
            lib(),
            &m(&[
                ("scale_ratio", 1.5),    // placement, sev 4 → 8
                ("outline_recall", 0.2), // outline, sev 4 → ~6.5
                ("silhouette", 0.5),     // silhouette, sev 4 → ~4.4
                ("smoothness", 0.1),     // line_confidence, sev 3 → 5.4
                ("outline", 0.3),        // outline again: skipped (skill taken)
            ]),
            "animals",
            0,
        );
        assert_eq!(ids(&f), ["too-big", "missing-parts", "shaky-lines"]);
        assert!(f.fixes.windows(2).all(|w| w[0].severity >= w[1].severity));
    }

    #[test]
    fn category_limited_rules() {
        let flat = m(&[("contrast_ratio", 0.3), ("shading", 0.5)]);
        assert_eq!(
            ids(&feedback(lib(), &flat, "real-life", 0)),
            ["flat-shading"]
        );
        assert!(feedback(lib(), &flat, "anime", 0).fixes.is_empty());
    }

    #[test]
    fn blank_canvas_short_circuits() {
        let f = feedback(
            lib(),
            &m(&[("ink_coverage", 0.0), ("scale_ratio", 3.0)]),
            "animals",
            0,
        );
        assert_eq!(ids(&f), ["blank-canvas"]);
        assert!(f.praise.is_none());
    }

    #[test]
    fn variants_rotate_with_the_seed_but_are_stable() {
        let ms = m(&[("scale_ratio", 1.3)]);
        let a = feedback(lib(), &ms, "anime", 1).fixes[0].text.clone();
        assert_eq!(a, feedback(lib(), &ms, "anime", 1).fixes[0].text);
        let texts: std::collections::HashSet<String> = (0..6)
            .map(|s| feedback(lib(), &ms, "anime", s).fixes[0].text.clone())
            .collect();
        assert!(texts.len() > 1);
    }

    #[test]
    fn invalid_content_is_rejected() {
        let rules = "[[rule]]\nid='x'\nmeasurement='m'\nseverity=1\nskill='s'\nnumber='pct'\n";
        assert!(
            Library::parse(rules, "[tip.x]\ntitle='t'\nvariants=['v']").is_err(),
            "no above/below"
        );
        let rules =
            "[[rule]]\nid='x'\nmeasurement='m'\nabove=1\nseverity=1\nskill='s'\nnumber='pct'\n";
        assert!(
            Library::parse(rules, "[tip.y]\ntitle='t'\nvariants=['v']").is_err(),
            "missing tip"
        );
        assert!(
            Library::parse(rules, "[tip.x]\ntitle='{dir}'\nvariants=['v']").is_err(),
            "dir without words"
        );
        assert!(Library::parse(rules, "[tip.x]\ntitle='t'\nvariants=['v']").is_ok());
    }

    #[test]
    fn shading_rules_only_judge_drawings_that_are_shaded() {
        let lib = Library::builtin();
        // pure line art: flat contrast, no fill → no "flat shading" verdict
        let line_art = m(&[("contrast_ratio", 0.4), ("tone", 0.1), ("shading", 0.02)]);
        let f = feedback(lib, &line_art, "real-life", 0);
        assert!(
            !f.fixes.iter().any(|x| x.id.contains("flat")
                || x.id.contains("misplaced")
                || x.id.contains("wrong")),
            "{:?}",
            f.fixes.iter().map(|x| &x.id).collect::<Vec<_>>()
        );
        // the same numbers on a shaded drawing are judged
        let shaded = m(&[("contrast_ratio", 0.4), ("tone", 0.1), ("shading", 0.6)]);
        assert!(!feedback(lib, &shaded, "real-life", 0).fixes.is_empty());
    }

    #[test]
    fn gate_needs_bounds() {
        let rules = "[[rule]]\nid='x'\nmeasurement='m'\nabove=1\nseverity=1\nskill='s'\nnumber='pct'\ngate='g'\n";
        assert!(Library::parse(rules, "[tip.x]\ntitle='t'\nvariants=['v {n}','w {n}']").is_err());
    }
}
