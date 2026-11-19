//! `calibrate fit`: tune the grader to human ratings (M8-T01).
//!
//! Each rating saved on /dev/rate carries the grader's measurements, so a score
//! can be recomputed with other weights and another curve
//! ([`grader::rescore`]) without grading again. Per category (and sketch mode)
//! we search for weights and a curve that bring the scores closest to the
//! human ratings (mean absolute error), held near the current values so a few
//! ratings can't pull them anywhere wild. "Left out" error refits without each
//! rating in turn and scores it: the honest estimate of how well it generalises.

use std::collections::{BTreeMap, HashMap};

use grader::{Category, SKETCH_WEIGHTS, ScoreCurve, Weights, rescore};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct Rating {
    /// the category it was graded as (own pictures: the kind the student picked)
    pub category: String,
    #[serde(default)]
    pub mode: Option<String>,
    pub human: f32,
    #[serde(default)]
    pub measurements: HashMap<String, f32>,
}

/// The tunable numbers: 9 weights, then the curve's floor and pass mark.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params([f32; 11]);

const WEIGHT_NAMES: [&str; 9] = [
    "placement",
    "outline",
    "silhouette",
    "grid",
    "tone",
    "ssim",
    "lineConfidence",
    "keypoints",
    "angles",
];

impl Params {
    pub fn new(w: &Weights, c: &ScoreCurve) -> Self {
        Self([
            w.placement,
            w.outline,
            w.silhouette,
            w.grid,
            w.tone,
            w.ssim,
            w.line_confidence,
            w.keypoints,
            w.angles,
            c.floor,
            c.pass,
        ])
    }
    pub fn weights(&self) -> Weights {
        let p = self.0;
        Weights {
            placement: p[0],
            outline: p[1],
            silhouette: p[2],
            grid: p[3],
            tone: p[4],
            ssim: p[5],
            line_confidence: p[6],
            keypoints: p[7],
            angles: p[8],
        }
    }
    pub fn curve(&self) -> ScoreCurve {
        ScoreCurve {
            floor: self.0[9],
            pass: self.0[10],
            gamma: 1.0,
        }
    }
    fn clamp(mut self) -> Self {
        for w in &mut self.0[..9] {
            *w = w.clamp(0.0, 1.0);
        }
        self.0[9] = self.0[9].clamp(0.10, 0.75);
        self.0[10] = self.0[10].clamp(self.0[9] + 0.10, 0.99);
        self
    }
}

/// The grader's starting point for a group.
pub fn start(category: &str, sketch: bool) -> Params {
    let w = if sketch {
        SKETCH_WEIGHTS
    } else {
        // the web app grades cartoons like anime and masters like real life
        let id = match category {
            "cartoon" => "anime",
            "masters" | "own" => "real-life",
            c => c,
        };
        let cat: Category = serde_json::from_value(serde_json::Value::String(id.into()))
            .unwrap_or(Category::Fundamentals);
        Weights::for_category(cat)
    };
    Params::new(&w, &ScoreCurve::default())
}

pub fn mae(r: &[&Rating], p: &Params) -> f32 {
    let (w, c) = (p.weights(), p.curve());
    r.iter()
        .map(|x| (rescore(&x.measurements, &w, &c).1 - x.human).abs())
        .sum::<f32>()
        / r.len().max(1) as f32
}

/// MAE plus a pull back towards the starting values.
fn objective(r: &[&Rating], p: &Params, p0: &Params, pull: f32) -> f32 {
    let drift: f32 =
        p.0.iter()
            .zip(p0.0.iter())
            .map(|(a, b)| (a - b) * (a - b))
            .sum();
    mae(r, p) + pull * drift
}

/// Coordinate descent from `p0`.
pub fn fit(r: &[&Rating], p0: &Params, rounds: usize) -> Params {
    let pull = 0.5;
    let mut p = *p0;
    let mut best = objective(r, &p, p0, pull);
    let mut step = 0.04f32;
    for _ in 0..rounds {
        let mut improved = false;
        for i in 0..p.0.len() {
            for dir in [1.0f32, -1.0] {
                let mut q = p;
                q.0[i] += dir * step;
                let q = q.clamp();
                let e = objective(r, &q, p0, pull);
                if e + 1e-6 < best {
                    (p, best, improved) = (q, e, true);
                }
            }
        }
        if !improved {
            step *= 0.5;
            if step < 0.002 {
                break;
            }
        }
    }
    p
}

/// Error on ratings the fit didn't see (refit without each one).
pub fn left_out_mae(r: &[&Rating], p0: &Params) -> f32 {
    let mut total = 0.0;
    for i in 0..r.len() {
        let rest: Vec<&Rating> = r
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, x)| *x)
            .collect();
        let p = fit(&rest, p0, 60);
        total += mae(&[r[i]], &p);
    }
    total / r.len().max(1) as f32
}

#[derive(Serialize)]
pub struct Fitted {
    pub labels: usize,
    pub current_mae: f32,
    pub fitted_mae: f32,
    pub left_out_mae: f32,
    pub weights: BTreeMap<&'static str, f32>,
    pub floor: f32,
    pub pass: f32,
}

/// Fit every group with enough ratings; groups are "category" or "category/sketch".
pub fn fit_all(ratings: &[Rating], min: usize) -> BTreeMap<String, Result<Fitted, usize>> {
    let mut groups: BTreeMap<String, Vec<&Rating>> = BTreeMap::new();
    for r in ratings.iter().filter(|r| !r.measurements.is_empty()) {
        let sketch = r.mode.as_deref() == Some("sketch");
        let key = if sketch {
            format!("{}/sketch", r.category)
        } else {
            r.category.clone()
        };
        groups.entry(key).or_default().push(r);
    }
    groups
        .into_iter()
        .map(|(k, v)| {
            if v.len() < min {
                return (k, Err(v.len()));
            }
            let sketch = k.ends_with("/sketch");
            let p0 = start(k.trim_end_matches("/sketch"), sketch);
            let p = fit(&v, &p0, 300);
            let f = Fitted {
                labels: v.len(),
                current_mae: mae(&v, &p0),
                fitted_mae: mae(&v, &p),
                left_out_mae: left_out_mae(&v, &p0),
                weights: WEIGHT_NAMES
                    .iter()
                    .zip(p.0.iter())
                    .map(|(n, w)| (*n, (w * 1000.0).round() / 1000.0))
                    .collect(),
                floor: p.0[9],
                pass: p.0[10],
            };
            (k, Ok(f))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ratings from a "teacher" who cares about outlines more than the grader does.
    fn teacher(n: usize) -> Vec<Rating> {
        let truth = {
            let mut p = start("anime", false);
            p.0[1] = 0.8; // outline
            p.0[2] = 0.05; // silhouette
            p
        };
        (0..n)
            .map(|i| {
                let f = |k: usize| (((i * 7919 + k * 104_729) % 1000) as f32) / 1000.0;
                let m: HashMap<String, f32> = [
                    ("placement", f(1)),
                    ("outline", f(2)),
                    ("silhouette", f(3)),
                    ("grid", f(4)),
                    ("tone", f(5)),
                    ("ssim", f(6)),
                    ("angles", 0.0),
                    ("shading", 0.0),
                    ("line_confidence", f(7)),
                ]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
                let human = rescore(&m, &truth.weights(), &truth.curve()).1;
                Rating {
                    category: "anime".into(),
                    mode: None,
                    human,
                    measurements: m,
                }
            })
            .collect()
    }

    #[test]
    fn fitting_moves_towards_the_teacher_and_helps_on_unseen_ratings() {
        let r = teacher(40);
        let refs: Vec<&Rating> = r.iter().collect();
        let p0 = start("anime", false);
        let p = fit(&refs, &p0, 300);
        assert!(
            mae(&refs, &p) < mae(&refs, &p0) * 0.6,
            "{} vs {}",
            mae(&refs, &p),
            mae(&refs, &p0)
        );
        // outline now counts for more relative to silhouette
        assert!(p.0[1] / p.0[2].max(1e-3) > p0.0[1] / p0.0[2]);
        let lo = left_out_mae(&refs[..20], &p0);
        assert!(lo < mae(&refs[..20], &p0), "left out {lo}");
    }

    #[test]
    fn small_groups_are_reported_not_fitted() {
        let r = teacher(4);
        let out = fit_all(&r, 12);
        assert!(matches!(out.get("anime"), Some(Err(4))));
        let old = Rating {
            measurements: HashMap::new(),
            ..r[0].clone()
        };
        assert!(
            fit_all(&[old], 1).is_empty(),
            "ratings without measurements are skipped"
        );
    }
}
