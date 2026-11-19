//! Print the picture sorter's raw probabilities for a list of pictures, one
//! TSV row each, for calibrating the `_sorting` thresholds in topics.json.
//!
//! ```sh
//! # list.tsv: id <TAB> category <TAB> topic <TAB> image path
//! cargo run --release -p pencilpilot-ref-filters --example sort_scan -- \
//!     crates/ref-filters/models/clip-vit-b32 content/topics.json list.tsv > scores.tsv
//! ```
//!
//! Output columns: id, category, topic, decision, own topic, top styles, top
//! topics, subject check (keep class vs the strongest reject class), franchise
//! check (keep class vs the strongest franchise prompt).

use std::path::Path;

use ref_filters::clip::{ClipText, ClipVision};
use ref_filters::sort::{SortSpec, Sorter};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = Path::new(&args[0]);
    let topics: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&args[1]).expect("topics.json"))
            .expect("JSON");
    let spec = SortSpec::from_topics_json(&topics).expect("_sorting");
    let text = ClipText::load(dir).expect("text tower");
    let sorter = Sorter::build(spec, |p| text.embed(p)).expect("prompts");
    let vision = ClipVision::load(dir).expect("vision tower");
    let list = std::fs::read_to_string(&args[2]).expect("list");
    for line in list.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let [id, cat, topic, path] = f[..] else {
            continue;
        };
        let Some(img) = std::fs::read(path)
            .ok()
            .and_then(|b| ref_filters::decode(&b).ok())
        else {
            eprintln!("{id}: unreadable");
            continue;
        };
        let e = vision.embed(&img).expect("embed");
        let styles = sorter
            .styles(&e)
            .iter()
            .map(|(s, p)| format!("{s}={p:.3}"))
            .collect::<Vec<_>>()
            .join(" ");
        let ranked = sorter.topics(&e, cat);
        let own = ranked
            .iter()
            .find(|((c, t), _)| c == cat && t == topic)
            .map_or(0.0, |r| r.1);
        let tops = ranked
            .iter()
            .take(3)
            .map(|((c, t), p)| format!("{c}/{t}={p:.3}"))
            .collect::<Vec<_>>()
            .join(" ");
        let subject = sorter.subject(&e, cat, topic).map_or(String::new(), |s| {
            format!(
                "keep={:.3} {}={:.3} margin={}",
                s.p_keep, s.worst, s.p_worst, s.margin
            )
        });
        let franchise = sorter.franchise(&e, cat, topic).map_or(String::new(), |s| {
            format!(
                "keep={:.3} {}={:.3} margin={}",
                s.p_keep, s.worst, s.p_worst, s.margin
            )
        });
        println!(
            "{id}\t{cat}\t{topic}\t{:?}\town={own:.3}\t{styles}\t{tops}\t{subject}\t{franchise}",
            sorter.decide(&e, cat, topic)
        );
    }
}
