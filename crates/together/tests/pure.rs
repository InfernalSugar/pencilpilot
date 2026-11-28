//! Golden tests of the parts on their own (`fixtures/pure.json`, written by the studio's
//! code through `golden/record.ts`): every edit read as together.ts reads it, line
//! strokes in its bytes, bases split as it splits them, tiles on the wire, timelines
//! (odd ones too) as it puts them in, and the chat's checks.

mod common;

use std::collections::{BTreeMap, HashMap};

use common::{normalise, timeline_json};
use pencilpilot_together::chat::{chat_body, chat_text, clean_invite, member_colour};
use pencilpilot_together::js::{from_base64, to_base64};
use pencilpilot_together::lines::{
    apply_line_edit, decode_lines, diff_lines, encode_lines, line_edits, split_base, with_base,
};
use pencilpilot_together::timeline::{
    AudioClip, Camera, Cut, Exposure, Marker, MouthChart, Palette, PaletteColour, Scene, Timeline,
    from_shared, to_shared,
};
use pencilpilot_together::{Edit, decode_tile, encode_tile, parse_edit};
use serde_json::{Value, json};

fn pure() -> Value {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/pure.json"
    ))
    .unwrap();
    serde_json::from_str(&text).unwrap()
}

/// An edit as `parseEdit` returns it (the timeline and sound pieces with an empty lid).
fn parsed(e: &Edit) -> Value {
    let mut v = e.to_json();
    if matches!(e, Edit::Timeline { .. } | Edit::Sound { .. }) {
        v["lid"] = "".into();
    }
    v
}

fn same_numbers(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_numbers(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, p)| y.get(k).is_some_and(|q| same_numbers(p, q)))
        }
        _ => a == b,
    }
}

#[test]
fn reads_every_edit_as_the_studio_does() {
    let p = pure();
    let cases = p["parsed"].as_array().unwrap();
    assert!(cases.len() > 80);
    let mut bad = Vec::new();
    for c in cases {
        let got = parse_edit(&c["body"]).map_or(Value::Null, |e| parsed(&e));
        if !same_numbers(&got, &c["edit"]) {
            bad.push(format!(
                "{}\n  rust   {}\n  studio {}",
                c["body"], got, c["edit"]
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn writes_line_strokes_in_the_studios_bytes() {
    let p = pure();
    for s in p["strokes"].as_array().unwrap() {
        let bytes = from_base64(s["b64"].as_str().unwrap()).unwrap();
        let strokes = decode_lines(&bytes).unwrap();
        // read back and written again: the studio's bytes for the same strokes
        assert_eq!(
            to_base64(&encode_lines(&strokes)),
            s["back"].as_str().unwrap()
        );
    }
    let all = decode_lines(&from_base64(p["all"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(all.len(), 8);
    assert_eq!(
        to_base64(&encode_lines(&all)),
        p["allBack"].as_str().unwrap()
    );
    // (every part read as the studio reads it)
    let tex = all[1].texture.as_ref().unwrap();
    assert_eq!(
        tex.strands.as_ref().map(|s| (s.count, s.spread, s.taper)),
        Some((9.0, 30.0, 0.0))
    );
    assert_eq!(all[1].color, "#abcdef");
    assert_eq!(all[1].seed, 4294967295);
    assert!(all[4].erase && all[5].dabs == Some((2, 9)) && all[5].span == Some((0, 2)));
    assert_eq!(all[6].clip.len(), 2);
    // strokes from the studio's plain list (as it writes them) are its own bytes as they are
    assert_eq!(
        to_base64(&encode_lines(&all[..1])),
        p["strokes"][0]["b64"].as_str().unwrap()
    );
}

#[test]
fn sends_the_same_line_edits() {
    let p = pure();
    let all = decode_lines(&from_base64(p["all"].as_str().unwrap()).unwrap()).unwrap();
    let (lid, did) = ("aaaaaaaaaaaa:1", "aaaaaaaaaaaa:5");
    let before = &all[..5];
    let after: Vec<_> = [all[0].clone(), all[2].clone(), all[1].clone()]
        .into_iter()
        .chain(all[5..].iter().cloned())
        .collect();
    // the studio's edit, put in here: the strokes it ends with
    let theirs = &p["lineEdits"][0];
    let del: Vec<String> = theirs["del"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_str().unwrap().into())
        .collect();
    let put: Vec<(u32, String)> = theirs["put"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| (x[0].as_u64().unwrap() as u32, x[1].as_str().unwrap().into()))
        .collect();
    // (the studio's list kept its own unread copy of a stroke the edit left alone: read)
    let applied = decode_lines(&from_base64(p["applied"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(apply_line_edit(before, &del, &put), applied);
    // ours: the same, less the stroke the studio sent because it was a copy (it compares
    // strokes as objects; here they're compared as they are)
    let ours = line_edits(lid, did, before, &after);
    let Edit::Lines {
        del: d2, put: p2, ..
    } = &ours[0]
    else {
        panic!()
    };
    assert_eq!(d2, &["ell", "er"]);
    assert_eq!(p2.iter().map(|x| x.0).collect::<Vec<_>>(), vec![3, 4, 5]);
    for (i, enc) in p2 {
        let studio = put.iter().find(|x| x.0 == *i).unwrap();
        // (the studio's stroke as it had it, unread; read and written again they're the same)
        let s = decode_lines(&from_base64(&studio.1).unwrap()).unwrap();
        assert_eq!(*enc, to_base64(&encode_lines(&s)));
    }
    let _ = diff_lines(before, &after);

    // a base riding with the strokes
    let base = common::base_pack(&[(0, 0), (1, 0)], &[(5, 5)]);
    let wb: Vec<Value> = with_base(line_edits(lid, did, &[], &all[..1]), lid, did, Some(&base))
        .iter()
        .map(Edit::to_json)
        .collect();
    assert_eq!(Value::from(wb), p["withBase"]);

    // a base too big for one edit
    let mut big = Vec::new();
    big.extend_from_slice(&70u32.to_le_bytes());
    for i in 0..70i32 {
        big.extend_from_slice(&i.to_le_bytes());
        big.extend_from_slice(&(-i).to_le_bytes());
    }
    big.extend_from_slice(&70u32.to_le_bytes());
    for i in 0..70i32 {
        big.extend_from_slice(&i.to_le_bytes());
        big.extend_from_slice(&(-i).to_le_bytes());
        big.extend(std::iter::repeat_n((i * 3) as u8, 16384));
    }
    big.extend_from_slice(&0u32.to_le_bytes());
    let parts: Vec<String> = split_base(&big, 900_000)
        .unwrap()
        .iter()
        .map(|b| to_base64(b))
        .collect();
    assert_eq!(
        Value::from(parts.iter().map(String::len).collect::<Vec<_>>()),
        p["bigParts"]
    );
    assert_eq!(
        Value::from(
            parts
                .iter()
                .map(|b| b[..64].to_string())
                .collect::<Vec<_>>()
        ),
        p["bigPartsHead"]
    );
    let edits: Vec<Value> = with_base(vec![], lid, did, Some(&big))
        .iter()
        .map(|e| {
            let mut v = e.to_json();
            v["base"] = v["base"].as_str().unwrap().len().into();
            v
        })
        .collect();
    assert_eq!(Value::from(edits), p["bigEdits"]);
}

#[test]
fn tiles_go_on_the_wire_in_the_studios_bytes() {
    let p = pure();
    for t in p["tiles"].as_array().unwrap() {
        let raw = from_base64(t["raw"].as_str().unwrap()).unwrap();
        assert_eq!(encode_tile(&raw), t["wire"].as_str().unwrap());
        assert_eq!(decode_tile(t["wire"].as_str().unwrap()), Some(raw));
    }
    for d in p["decoded"].as_array().unwrap() {
        let got = decode_tile(d["wire"].as_str().unwrap()).map(|b| to_base64(&b));
        assert_eq!(got.as_deref(), d["px"].as_str(), "{}", d["wire"]);
    }
}

/// record.ts's timeline of this app.
fn local() -> Timeline {
    let mut tracks = BTreeMap::new();
    tracks.insert(
        1,
        vec![
            Exposure {
                drawing: 11,
                length: 2,
            },
            Exposure {
                drawing: 0,
                length: 1,
            },
        ],
    );
    tracks.insert(
        2,
        vec![Exposure {
            drawing: 21,
            length: 3,
        }],
    );
    tracks.insert(
        9,
        vec![Exposure {
            drawing: 91,
            length: 1,
        }],
    );
    Timeline {
        scenes: vec![Scene {
            id: 1,
            name: "Opening".into(),
            markers: vec![Marker {
                frame: 3,
                label: "beat".into(),
            }],
            audio: vec![AudioClip {
                id: 1,
                name: "song".into(),
                file: "s1-a.mp3".into(),
                start: -2.0,
                gain: 0.5,
                muted: true,
            }],
            cuts: vec![Cut {
                id: 1,
                name: "Wide".into(),
                camera: Some(Camera {
                    x: 1.0,
                    y: 2.5,
                    width: 640.0,
                    height: 360.0,
                }),
                action: [(0, "runs".to_string()), (12, "stops".to_string())]
                    .into_iter()
                    .collect(),
                dialogue: [(4, "Hi!".to_string())].into_iter().collect(),
                transition: Some("dissolve".into()),
                tracks,
            }],
        }],
        statics: vec![2, 9],
        mouths: vec![MouthChart {
            layer: 1,
            shapes: [("A".to_string(), 11), ("X".to_string(), 0)]
                .into_iter()
                .collect(),
        }],
        kinds: [
            (11, "key".to_string()),
            (21, "breakdown".to_string()),
            (0, "key".to_string()),
        ]
        .into_iter()
        .collect(),
        stages: [(1, "rough".to_string()), (9, "comp".to_string())]
            .into_iter()
            .collect(),
        palettes: vec![Palette {
            name: "Mina".into(),
            colours: vec![PaletteColour {
                name: "Skin".into(),
                hex: "#f2c9a8".into(),
            }],
        }],
    }
}

#[test]
fn shares_the_timeline_as_the_studio_does() {
    let p = pure();
    let lid_of = |l: u32| match l {
        1 => Some("aaaaaaaaaaaa:1".to_string()),
        2 => Some("aaaaaaaaaaaa:2".to_string()),
        _ => None,
    };
    let out = to_shared(&local(), &lid_of, &mut |d| format!("aaaaaaaaaaaa:{d}"));
    assert!(
        same_numbers(&out, &p["toShared"]),
        "rust   {out}\nstudio {}",
        p["toShared"]
    );
}

#[test]
fn puts_the_sessions_timelines_in_as_the_studio_does_odd_ones_too() {
    let p = pure();
    let local = local();
    for case in p["fromShared"].as_array().unwrap() {
        let mut made: BTreeMap<String, u32> = BTreeMap::new();
        let mut next = 500;
        let back = from_shared(
            &case["tl"],
            &|l| match l {
                "aaaaaaaaaaaa:1" => Some(1),
                "aaaaaaaaaaaa:2" => Some(2),
                _ => None,
            },
            &mut |d| {
                *made.entry(d.to_string()).or_insert_with(|| {
                    next += 1;
                    next - 1
                })
            },
            &local,
            &|l| l == 1 || l == 2,
        );
        // (drawings made here are named by their shared id: each side numbers them in the
        // order it meets them)
        let names = |m: &Value| -> HashMap<u64, String> {
            m.as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (v.as_u64().unwrap(), k.clone()))
                .collect()
        };
        let ours = names(&json!(made));
        let theirs = names(&case["made"]);
        let mut a = timeline_json(&back);
        let mut b = case["back"].clone();
        rename(&mut a, &ours);
        rename(&mut b, &theirs);
        if b.get("statics").is_none() {
            b["statics"] = json!([]);
        }
        normalise(&mut a);
        normalise(&mut b);
        assert!(
            same_numbers(&a, &b),
            "from {}\n  rust   {a}\n  studio {b}",
            case["tl"]
        );
        let mut k1: Vec<_> = made.keys().cloned().collect();
        let mut k2: Vec<_> = case["made"].as_object().unwrap().keys().cloned().collect();
        k1.sort();
        k2.sort();
        assert_eq!(k1, k2);
    }
}

/// Drawings ≥ 500 (made for the session's) by their shared ids.
fn rename(tl: &mut Value, names: &HashMap<u64, String>) {
    let name = |v: &mut Value| {
        if let Some(n) = v.as_u64().and_then(|n| names.get(&n)) {
            *v = n.clone().into();
        }
    };
    for sc in tl["scenes"].as_array_mut().unwrap() {
        for c in sc["cuts"].as_array_mut().unwrap() {
            for t in c["tracks"].as_object_mut().unwrap().values_mut() {
                for e in t.as_array_mut().unwrap() {
                    name(&mut e["drawing"]);
                }
            }
        }
    }
    if let Some(m) = tl.get_mut("mouths").and_then(Value::as_array_mut) {
        for m in m {
            m["shapes"]
                .as_object_mut()
                .unwrap()
                .values_mut()
                .for_each(name);
        }
    }
    if let Some(k) = tl.get_mut("kinds").and_then(Value::as_object_mut) {
        *k = std::mem::take(k)
            .into_iter()
            .map(|(d, v)| {
                (
                    d.parse()
                        .ok()
                        .and_then(|n: u64| names.get(&n).cloned())
                        .unwrap_or(d),
                    v,
                )
            })
            .collect();
    }
}

#[test]
fn chats_as_the_studio_does() {
    let p = pure();
    for c in p["chats"].as_array().unwrap() {
        assert_eq!(
            chat_text(&c["body"])
                .map(Value::from)
                .unwrap_or(Value::Null),
            c["text"],
            "{}",
            c["body"]
        );
    }
    for t in p["typed"].as_array().unwrap() {
        assert_eq!(
            chat_body(t["typed"].as_str().unwrap()).unwrap_or(Value::Null),
            t["body"]
        );
    }
    for c in p["colours"].as_array().unwrap() {
        assert_eq!(
            member_colour(c["id"].as_str().unwrap()),
            c["colour"].as_str().unwrap()
        );
    }
    for i in p["invites"].as_array().unwrap() {
        assert_eq!(
            clean_invite(i["text"].as_str().unwrap())
                .map(Value::from)
                .unwrap_or(Value::Null),
            i["code"]
        );
    }
}
