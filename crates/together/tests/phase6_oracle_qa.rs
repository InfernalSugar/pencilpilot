//! Phase 6 QA: the port against the current app's own code. `tools/native-qa/together_oracle.mjs`
//! runs the studio's `together.ts` (bundled as is) over a seeded corpus of edits (well made and
//! broken on purpose, unknown kinds and AI-made content among them); here the same corpus goes
//! through [`parse_edit`] and [`Shared::apply`], and every line must come out the same: the
//! parsed edit (or dropped), whether it counted, and the shared state after it.
//!
//! Needs node and `pnpm install` (for esbuild):
//!
//! ```sh
//! cargo test --release -p pencilpilot-together --test phase6_oracle_qa -- --include-ignored
//! ```
//!
//! `PP_QA_ORACLE_SEEDS=1,2,3` (default 7,11,23) and `PP_QA_ORACLE_N` (default 3000) pick the
//! corpora; the files go to `target/q6/oracle`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use pencilpilot_together::{Shared, parse_edit};
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn node(args: &[&str], stdin: Option<&Path>, stdout: &Path) {
    let out = std::fs::File::create(stdout).unwrap();
    let mut c = Command::new("node");
    c.arg(root().join("tools/native-qa/together_oracle.mjs"))
        .args(args)
        .stdout(out);
    if let Some(i) = stdin {
        c.stdin(std::fs::File::open(i).unwrap());
    }
    assert!(c.status().expect("node").success(), "node {args:?}");
}

/// JSON with every number as an f64 and objects keyed in order (JavaScript writes 1 for
/// 1.0; key order doesn't matter to a reader).
fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => serde_json::Number::from_f64(n.as_f64().unwrap())
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Object(o) => {
            let m: BTreeMap<String, Value> = o.iter().map(|(k, v)| (k.clone(), norm(v))).collect();
            Value::Object(m.into_iter().collect())
        }
        _ => v.clone(),
    }
}

fn state(s: &Shared) -> Value {
    let (layers, groups, asks) = s.snapshot();
    let mut layers: Vec<Value> = layers.iter().map(norm).collect();
    layers.sort_by_key(|l| l["lid"].as_str().unwrap_or("").to_string());
    let mut groups: Vec<Value> = groups.iter().map(norm).collect();
    groups.sort_by_key(|l| l["gid"].as_str().unwrap_or("").to_string());
    serde_json::json!({
        "layers": layers,
        "order": s.order,
        "groups": groups,
        "asks": asks.iter().map(norm).collect::<Vec<_>>(),
    })
}

fn js_state(v: &Value) -> Value {
    let mut layers: Vec<Value> = v["layers"].as_array().unwrap().iter().map(norm).collect();
    layers.sort_by_key(|l| l["lid"].as_str().unwrap_or("").to_string());
    let mut groups: Vec<Value> = v["groups"].as_array().unwrap().iter().map(norm).collect();
    groups.sort_by_key(|l| l["gid"].as_str().unwrap_or("").to_string());
    serde_json::json!({
        "layers": layers,
        "order": v["order"],
        "groups": groups,
        "asks": v["asks"].as_array().unwrap().iter().map(norm).collect::<Vec<_>>(),
    })
}

#[test]
#[ignore = "needs node and pnpm install: see the file's header"]
fn every_edit_reads_and_counts_as_the_current_app_has_it() {
    let dir = root().join("target/q6/oracle");
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("together.mjs");
    let status = Command::new("node")
        .arg(root().join("tools/native-qa/together_oracle.mjs"))
        .arg("bundle")
        .arg(&bundle)
        .status()
        .expect("node");
    assert!(status.success(), "bundling together.ts");
    let seeds = std::env::var("PP_QA_ORACLE_SEEDS").unwrap_or("7,11,23".into());
    let n = std::env::var("PP_QA_ORACLE_N").unwrap_or("3000".into());
    let mut report = Vec::new();
    let mut total = 0;
    for seed in seeds.split(',') {
        let corpus = dir.join(format!("corpus-{n}-{seed}.jsonl"));
        let expected = dir.join(format!("expected-{n}-{seed}.jsonl"));
        node(&["gen", &n, seed], None, &corpus);
        node(&["run", bundle.to_str().unwrap()], Some(&corpus), &expected);
        let corpus = std::fs::read_to_string(&corpus).unwrap();
        let expected = std::fs::read_to_string(&expected).unwrap();
        let mut shared = Shared::default();
        for (i, (c, e)) in corpus.lines().zip(expected.lines()).enumerate() {
            total += 1;
            let c: Value = serde_json::from_str(c).unwrap();
            let e: Value = serde_json::from_str(e).unwrap();
            let from = c["from"].as_str().unwrap();
            let parsed = parse_edit(&c["body"]);
            let got = parsed
                .as_ref()
                .map(|p| norm(&p.to_json()))
                .unwrap_or(Value::Null);
            let mut want = norm(&e["parsed"]);
            // (the current app keeps a timeline's `lid` as ""; the port leaves it out: the
            // same edit, it never goes back on the wire)
            if want["k"] == "timeline"
                && got.get("lid").is_none()
                && let Some(o) = want.as_object_mut()
            {
                o.remove("lid");
            }
            if got != want {
                report.push(format!(
                    "seed {seed} line {i}: parse\n  body {}\n  js   {}\n  rust {}",
                    c["body"], want, got
                ));
            }
            let applied = parsed.as_ref().map(|p| shared.apply(from, p));
            let want_applied = e["applied"].as_bool();
            if applied != want_applied {
                report.push(format!(
                    "seed {seed} line {i}: apply {:?} (js {:?}) from {} body {}",
                    applied,
                    want_applied,
                    &from[..12],
                    c["body"]
                ));
            }
            {
                let (s, w) = (state(&shared), js_state(&e["state"]));
                if s != w {
                    report.push(format!(
                        "seed {seed} line {i}: state after {}\n  js   {w}\n  rust {s}",
                        c["body"]
                    ));
                    // from here on, carry on from the current app's state
                    shared = Shared::default();
                    let (layers, groups, asks) = (
                        e["state"]["layers"].as_array().unwrap().clone(),
                        e["state"]["groups"].as_array().unwrap().clone(),
                        e["state"]["asks"].as_array().unwrap().clone(),
                    );
                    shared.load(&layers, &groups, &asks);
                    shared.order = e["state"]["order"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|x| x.as_str().unwrap().to_string())
                        .collect();
                }
            }
        }
    }
    for r in report.iter().take(40) {
        eprintln!("{r}\n");
    }
    assert!(
        report.is_empty(),
        "{} of {total} lines differ from the current app",
        report.len()
    );
}

fn fnv1a64(b: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &c in b {
        h = (h ^ c as u64).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Tiles the current app sent (captured from its window by tools/native-qa/phase6.py,
/// `studio-wire`): the port reads each to the same pixels, and what it sends back reads
/// to them too.
#[test]
fn tiles_from_the_current_app_read_the_same() {
    let f: Value = serde_json::from_str(include_str!("qa/studio_tiles.json")).unwrap();
    let tiles = f["tiles"].as_array().unwrap();
    assert!(tiles.len() >= 20);
    for t in tiles {
        let px = pencilpilot_together::decode_tile(t["wire"].as_str().unwrap()).expect("decoded");
        assert_eq!(px.len(), pencilpilot_together::TILE_BYTES);
        assert_eq!(
            fnv1a64(&px),
            t["fnv1a64"].as_str().unwrap(),
            "tile {}",
            t["x"]
        );
        let back = pencilpilot_together::encode_tile(&px);
        assert_eq!(pencilpilot_together::decode_tile(&back).unwrap(), px);
    }
}
