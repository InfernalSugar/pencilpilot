//! PencilPilot feedback engine: measurements → diagnoses → tips.
//!
//! Rules are data (content/rules/*.toml, milestone M4). This M0 skeleton
//! defines the core types and a threshold check.

use serde::{Deserialize, Serialize};

/// One diagnosis rule: if `measurement` crosses `threshold`, report `mistake`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub measurement: String,
    pub below: f32,
    pub mistake: String,
    pub severity: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnosis {
    pub rule_id: String,
    pub mistake: String,
    pub severity: u8,
    pub value: f32,
}

/// Apply rules to measurements; return diagnoses sorted most severe first.
pub fn diagnose(rules: &[Rule], measurements: &[(String, f32)]) -> Vec<Diagnosis> {
    let mut out: Vec<Diagnosis> = rules
        .iter()
        .filter_map(|r| {
            let (_, v) = measurements
                .iter()
                .find(|(name, _)| *name == r.measurement)?;
            (*v < r.below).then(|| Diagnosis {
                rule_id: r.id.clone(),
                mistake: r.mistake.clone(),
                severity: r.severity,
                value: *v,
            })
        })
        .collect();
    out.sort_by_key(|d| std::cmp::Reverse(d.severity));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, m: &str, below: f32, sev: u8) -> Rule {
        Rule {
            id: id.into(),
            measurement: m.into(),
            below,
            mistake: id.into(),
            severity: sev,
        }
    }

    #[test]
    fn fires_and_sorts_by_severity() {
        let rules = [
            rule("a", "x", 0.5, 1),
            rule("b", "y", 0.9, 3),
            rule("c", "z", 0.1, 5),
        ];
        let m = vec![
            ("x".to_string(), 0.2),
            ("y".to_string(), 0.5),
            ("z".to_string(), 0.5),
        ];
        let d = diagnose(&rules, &m);
        assert_eq!(
            d.iter().map(|d| d.rule_id.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
    }
}
