//! together.test.ts, ported: every test, in its order.

mod common;

use std::sync::Arc;

use common::*;
use pencilpilot_together::edit::{StyleStroke, TileSet};
use pencilpilot_together::lines::{
    LineStroke, apply_line_edit, line_edits, read_line_edit, split_base, stroke_from_share,
    stroke_to_share, with_base,
};
use pencilpilot_together::shared::Ask;
use pencilpilot_together::timeline::{
    AudioClip, Palette, PaletteColour, Timeline, from_shared, to_shared,
};
use pencilpilot_together::{
    CanvasPort, Edit, GroupSettings, LayerStyle, Look, SavedMember, SavedShare, Shared,
    StrokePosition, TILE_BYTES, TileMap, Why, decode_tile, diff_tiles, encode_tile, pack,
    parse_edit, unpack,
};
use serde_json::{Value, json};

fn layer_edit(lid: &str, name: &str) -> Edit {
    Edit::Layer {
        lid: lid.into(),
        name: name.into(),
        adjust: None,
        lines: false,
    }
}

fn ask(lid: &str, from: &str, why: Why, own: bool, with: Option<&str>) -> Ask {
    Ask {
        lid: lid.into(),
        from: from.into(),
        why,
        own,
        with: with.map(str::to_string),
    }
}

// ---- drawing together: the shared layers ----

#[test]
fn takes_only_the_listed_edits_no_ai_nothing_else() {
    assert_eq!(
        parse_edit(&json!({"k": "layer", "lid": "aaaaaaaaaaaa:1", "name": "Hi"})),
        Some(layer_edit("aaaaaaaaaaaa:1", "Hi"))
    );
    assert_eq!(
        parse_edit(&json!({"k": "ai-image", "lid": "aaaaaaaaaaaa:1", "png": "..."})),
        None
    );
    assert_eq!(parse_edit(&json!({"k": "layer", "lid": "../etc"})), None);
    assert_eq!(
        parse_edit(&json!({"k": "grant", "lid": "aaaaaaaaaaaa:1", "to": "nope"})),
        None
    );
    assert_eq!(parse_edit(&Value::Null), None);
}

#[test]
fn checks_a_layer_style_in_a_look() {
    let lid = "aaaaaaaaaaaa:1";
    let look = json!({"visible": true, "opacity": 1, "blend": "normal", "alphaLock": false, "clip": false});
    let mut outline = LayerStyle::default().to_json();
    outline["stroke"] =
        json!({"on": true, "size": 500, "colour": "#FF0000", "position": "sideways", "opacity": 1});
    let mut l = look.clone();
    l["style"] = outline;
    let Some(Edit::Look { look: got, .. }) =
        parse_edit(&json!({"k": "look", "lid": lid, "look": l}))
    else {
        panic!()
    };
    let s = got.style.unwrap().unwrap();
    assert_eq!(
        s.stroke,
        StyleStroke {
            on: true,
            size: 100.0,
            colour: "#ff0000".into(),
            position: StrokePosition::Outside,
            opacity: 1.0
        }
    );
    // junk is no style; an app without styles leaves it out (and the style alone)
    let mut l = look.clone();
    l["style"] = "glow".into();
    let Some(Edit::Look { look: got, .. }) =
        parse_edit(&json!({"k": "look", "lid": lid, "look": l}))
    else {
        panic!()
    };
    assert_eq!(got.style, Some(None));
    let Some(Edit::Look { look: old, .. }) =
        parse_edit(&json!({"k": "look", "lid": lid, "look": look}))
    else {
        panic!()
    };
    assert_eq!(old.style, None);
}

#[test]
fn lets_only_writers_change_a_layer_and_only_owners_give_access() {
    let mut s = Shared::default();
    let lid = "aaaaaaaaaaaa:1";
    assert!(!s.apply(B, &layer_edit(lid, "x"))); // not B's id
    assert!(s.apply(A, &layer_edit(lid, "A")));
    let t = Edit::Tiles {
        lid: lid.into(),
        did: lid.into(),
        set: vec![],
        clear: vec![],
    };
    assert!(!s.apply(B, &t));
    let ask_draw = Edit::Ask {
        lid: lid.into(),
        why: Why::Draw,
        own: false,
        with: None,
    };
    assert!(s.apply(B, &ask_draw));
    assert_eq!(s.asks, vec![ask(lid, B, Why::Draw, false, None)]);
    let grant = |handover| Edit::Grant {
        lid: lid.into(),
        to: B.into(),
        handover,
    };
    assert!(!s.apply(B, &grant(false))); // not owner
    assert!(s.apply(A, &grant(false)));
    assert_eq!(s.asks, vec![]);
    assert!(s.apply(B, &t));
    let revoke = Edit::Revoke {
        lid: lid.into(),
        to: B.into(),
    };
    assert!(s.apply(A, &revoke));
    assert!(!s.apply(B, &t));
    // handing over: B owns it, A can't draw any more
    assert!(s.apply(A, &grant(true)));
    assert_eq!(s.layers[lid].owner, B);
    assert!(!s.can_draw(lid, A));
    assert!(!s.apply(A, &revoke));
}

// ---- drawing together: groups and asking first ----

#[test]
fn lets_groups_change_only_with_their_owners_leave_and_layers_join_only_if_drawable() {
    let mut s = Shared::default();
    let la = "aaaaaaaaaaaa:1";
    let lb = "bbbbbbbbbbbb:1";
    let g = "aaaaaaaaaaaa:2";
    s.apply(A, &layer_edit(la, "A"));
    s.apply(B, &layer_edit(lb, "B"));
    let group = |gid: &str, lids: &[&str]| Edit::Group {
        lid: la.into(),
        gid: gid.into(),
        name: "G".into(),
        lids: lids.iter().map(|x| x.to_string()).collect(),
    };
    // A can't put B's layer in a group, nor use B's id for the group
    assert!(!s.apply(A, &group(g, &[la, lb])));
    assert!(!s.apply(A, &group("bbbbbbbbbbbb:9", &[la])));
    assert!(s.apply(A, &group(g, &[la])));
    assert_eq!(s.members(g), vec![la]);
    // B can't rename A's group, or move his layer into it, until A says yes
    let gset = Edit::GroupSet {
        lid: g.into(),
        name: "Mine".into(),
        visible: true,
        opacity: 0.5,
    };
    let place = |to, gid: &str| Edit::Place {
        lid: lb.into(),
        to,
        gid: gid.into(),
    };
    assert!(!s.apply(B, &gset));
    assert!(!s.apply(B, &place(0, g)));
    assert!(s.apply(
        B,
        &Edit::Ask {
            lid: g.into(),
            why: Why::Group,
            own: false,
            with: None
        }
    ));
    assert_eq!(s.asks, vec![ask(g, B, Why::Group, false, None)]);
    assert!(s.apply(
        A,
        &Edit::Grant {
            lid: g.into(),
            to: B.into(),
            handover: false
        }
    ));
    assert!(s.apply(B, &place(0, g)));
    assert_eq!(s.order, vec![lb, la]);
    assert_eq!(s.members(g), vec![lb, la]);
    assert!(s.apply(B, &gset));
    // only the group's owner takes it apart; a group left empty goes
    assert!(!s.apply(B, &Edit::Ungroup { lid: g.into() }));
    assert!(s.apply(
        A,
        &Edit::Block {
            lid: g.into(),
            to: 0
        }
    ));
    assert!(s.apply(B, &place(1, "")));
    assert!(s.apply(A, &Edit::Remove { lid: la.into() }));
    assert!(!s.groups.contains_key(g));
    // asking for a layer to be handed over (to merge it) says so; an owner can't ask
    let merge = |with: Option<&str>| Edit::Ask {
        lid: lb.into(),
        why: Why::Merge,
        own: true,
        with: with.map(str::to_string),
    };
    assert!(s.apply(A, &merge(Some(la))));
    assert!(!s.apply(B, &merge(None)));
    assert_eq!(s.asks, vec![ask(lb, A, Why::Merge, true, Some(la))]);
    let snap = s.snapshot();
    let mut back = Shared::default();
    back.load(&snap.0, &snap.1, &snap.2);
    assert_eq!(back.snapshot(), snap);
}

// ---- drawing together: tiles ----

#[test]
fn packs_diffs_and_travels_compressed() {
    let m: TileMap = [((0, 0), tile(1)), ((-1, 2), tile(2))]
        .into_iter()
        .collect();
    assert_eq!(unpack(&pack(&m)), m);
    let next: TileMap = [((0, 0), tile(1)), ((3, 3), tile(9))].into_iter().collect();
    let d = diff_tiles(&m, &next);
    assert_eq!(
        d.set.iter().map(|s| (s.0, s.1)).collect::<Vec<_>>(),
        vec![(3, 3)]
    );
    assert_eq!(d.clear, vec![(-1, 2)]);
    let enc = encode_tile(&tile(7));
    assert!(enc.len() < 200); // a flat tile compresses to almost nothing
    assert_eq!(decode_tile(&enc), Some(tile(7)));
    assert_eq!(decode_tile("z:garbage!"), None);
}

// ---- drawing together: apps ----

/// The host (A, Ana) with `names`, started; then Bo (B) joins with an empty canvas.
fn hosted(names: &[&str], before: impl FnOnce(&mut FakeCanvas)) -> FakeSession {
    let mut s = FakeSession::default();
    let mut hc = FakeCanvas::new(names);
    before(&mut hc);
    s.add(A, "Ana", hc);
    s.with(A, |t, c| t.start_host(c));
    s.settle();
    s
}

fn join(s: &mut FakeSession, who: &str, name: &str, c: FakeCanvas) {
    s.add(who, name, c);
    s.with(who, |t, c| t.start_join(c, A));
    s.settle();
}

#[test]
fn shares_the_hosts_drawing_with_a_joiner_then_each_draws_on_their_own_layer() {
    let mut s = hosted(&["Sky"], |hc| {
        let sky = hc.by_name("Sky").id;
        hc.paint(sky, 0, 0, 50);
    });
    // Bo joins: gets the project
    let mut bc = FakeCanvas::new(&["Bo’s old work"]);
    bc.paint(1, 5, 5, 99);
    s.add(B, "Bo", bc);
    s.with(B, |t, c| t.start_join(c, A));
    s.settle();
    s.t(A).take_notices();
    assert!(s.t(B).synced());
    let bc = s.c(B);
    let sky = bc.by_name("Sky").clone();
    assert_eq!(bc.pixel(sky.id, 0, 0), Some(50));
    assert!(sky.locked);
    // Bo's own work before joining stays here, hidden, not shared
    assert!(!bc.layer_list[0].visible && bc.layer_list[0].locked);
    // Bo got a layer of his own, selected, and the host has it too
    let bo_layer = bc.by_name("Bo’s layer").clone();
    assert_eq!(bc.selected, bo_layer.id);
    assert!(!bo_layer.locked);
    assert!(s.c(A).by_name("Bo’s layer").locked);

    // Bo draws on his layer: the host sees it
    s.c(B).paint(bo_layer.id, 1, 1, 77);
    // (the page sends its timeline when a drawing starts on it, as here)
    s.with(B, |t, c| t.timeline_changed(c));
    s.with(B, |t, c| t.local_changed(c));
    s.settle();
    let hb = s.c(A).by_name("Bo’s layer").id;
    assert_eq!(s.c(A).pixel(hb, 1, 1), Some(77));
    // (and the host was told Bo's real pixels came, for clearing his live preview)
    let tiles: Vec<_> = s
        .t(A)
        .take_notices()
        .into_iter()
        .filter_map(|n| match n {
            pencilpilot_together::Notice::Tiles { from } => Some(from),
            _ => None,
        })
        .collect();
    assert_eq!(tiles, vec![B.to_string()]);
    // Bo can't draw on the host's layer
    assert!(!s.c(B).paint(sky.id, 2, 2, 1));

    // the same layer order everywhere
    assert_eq!(s.c(A).names(), vec!["Sky", "Bo’s layer"]);
    assert_eq!(s.c(B).names()[1..], ["Sky", "Bo’s layer"]);
}

fn notes(s: &mut FakeSession, who: &str) -> Vec<String> {
    s.t(who)
        .take_notices()
        .into_iter()
        .filter_map(|n| match n {
            pencilpilot_together::Notice::Note(t) => Some(t),
            _ => None,
        })
        .collect()
}

#[test]
fn merges_into_someone_elses_layer_once_they_allow_it() {
    let mut s = hosted(&["Sky"], |hc| {
        hc.paint(1, 0, 0, 10);
    });
    join(&mut s, B, "Bo", FakeCanvas::new(&["Layer 1"]));
    let mine = s.c(B).by_name("Bo’s layer").id;
    s.c(B).paint(mine, 1, 1, 77);
    s.with(B, |t, c| t.local_changed(c));
    s.settle();
    notes(&mut s, B);

    // Bo merges his layer down into Ana's Sky: Ana is asked first
    assert_eq!(
        s.with(B, |t, c| t.merge_down(c, mine)),
        Some(vec![A.to_string()])
    );
    s.settle();
    let sky_lid = {
        let id = s.c(A).by_name("Sky").id;
        s.t(A).lid_of(id).unwrap()
    };
    let asks = s.t(A).shared.asks.clone();
    assert_eq!(asks.len(), 1);
    assert_eq!(
        (asks[0].lid.as_str(), asks[0].from.as_str(), asks[0].why),
        (sky_lid.as_str(), B, Why::Merge)
    );
    assert!(s.c(B).has("Bo’s layer"));

    // she allows it: merged, for both
    s.with(A, |t, _| t.grant(&sky_lid, B, false));
    s.settle();
    for who in [A, B] {
        let c = s.c(who);
        assert!(!c.has("Bo’s layer"));
        let sky = c.by_name("Sky").id;
        assert_eq!(c.pixel(sky, 1, 1), Some(77));
        assert_eq!(c.pixel(sky, 0, 0), Some(10));
    }
    assert_eq!(notes(&mut s, B), Vec::<String>::new());

    // asked again and refused: nothing changes, and Bo is told
    s.c(B).add_layer("Extra", None);
    s.with(B, |t, c| t.local_changed(c));
    s.settle();
    s.with(A, |t, _| t.revoke(&sky_lid, B));
    s.settle();
    let extra = s.c(B).by_name("Extra").id;
    let sky = s.c(B).by_name("Sky").id;
    assert_eq!(s.c(B).index_of(extra), s.c(B).index_of(sky) + 1);
    assert_eq!(
        s.with(B, |t, c| t.merge_down(c, extra)),
        Some(vec![A.to_string()])
    );
    s.settle();
    s.with(A, |t, _| t.deny(&sky_lid, B));
    s.settle();
    assert_eq!(notes(&mut s, B), vec!["They said no: nothing was changed."]);
    assert!(s.c(A).has("Extra"));
}

#[test]
fn shares_groups_and_groups_and_merges_others_layers_with_their_leave() {
    let mut s = hosted(&["Sky", "Hills"], |hc| {
        // Ana's Hills are in a group already when she starts
        let hills = hc.by_name("Hills").id;
        hc.add_group(hills);
        hc.set_group_settings(
            1,
            GroupSettings {
                name: "Land".into(),
                visible: true,
                opacity: 0.8,
            },
        );
        hc.paint(hc.by_name("Sky").id, 0, 0, 10);
    });
    join(&mut s, B, "Bo", FakeCanvas::new(&["Layer 1"]));
    // Bo has the group, as Ana does
    let land = s
        .c(B)
        .group_list
        .iter()
        .find(|g| g.settings.name == "Land")
        .unwrap()
        .clone();
    assert_eq!(land.settings.opacity, 0.8);
    assert_eq!(s.c(B).by_name("Hills").group, land.id);

    // Bo puts his layer in Ana's group: asks; once she allows it, it's in, for both
    let mine = s.c(B).by_name("Bo’s layer").id;
    s.c(B).paint(mine, 2, 2, 55);
    s.with(B, |t, c| t.timeline_changed(c));
    s.with(B, |t, c| t.local_changed(c));
    s.settle();
    let hills = s.c(B).by_name("Hills").id;
    let hills_lid = s.t(B).lid_of(hills).unwrap();
    let at = s
        .t(B)
        .shared
        .order
        .iter()
        .position(|x| *x == hills_lid)
        .unwrap() as u32
        + 1;
    assert_eq!(
        s.with(B, |t, c| t.move_layer_to(c, mine, at, land.id)),
        Some(vec![A.to_string()])
    );
    s.settle();
    let gid = s.t(B).gid_of(land.id).unwrap();
    s.with(A, |t, _| t.grant(&gid, B, false));
    s.settle();
    assert_eq!(s.c(B).by_name("Bo’s layer").group, land.id);
    assert_eq!(s.c(A).by_name("Bo’s layer").group, 1);
    assert_eq!(s.c(A).names(), vec!["Sky", "Hills", "Bo’s layer"]);

    // Bo changes the group's opacity (he may now): Ana sees it
    s.c(B).set_group_settings(
        land.id,
        GroupSettings {
            name: "Land".into(),
            visible: true,
            opacity: 0.3,
        },
    );
    s.with(B, |t, c| t.local_changed(c));
    s.settle();
    assert!((s.c(A).group_list[0].settings.opacity - 0.3).abs() < 1e-9);

    // Ana merges the group into one layer: Bo's layer must be handed over first
    assert_eq!(
        s.with(A, |t, c| t.merge_group(c, 1)),
        Some(vec![B.to_string()])
    );
    s.settle();
    let bo_lid = {
        let id = s.c(A).by_name("Bo’s layer").id;
        s.t(A).lid_of(id).unwrap()
    };
    let asks = s.t(B).shared.asks.clone();
    assert_eq!(asks.len(), 1);
    assert_eq!(
        (
            asks[0].lid.as_str(),
            asks[0].from.as_str(),
            asks[0].why,
            asks[0].own
        ),
        (bo_lid.as_str(), A, Why::Merge, true)
    );
    s.with(B, |t, _| t.grant(&bo_lid, A, true));
    s.settle();
    for who in [A, B] {
        let c = s.c(who);
        assert!(c.names().contains(&"Land".to_string()));
        assert!(!c.has("Bo’s layer") && !c.has("Hills"));
        let land = c.by_name("Land").id;
        assert_eq!(c.pixel(land, 2, 2), Some(55));
        assert_eq!(c.by_name("Land").group, 0);
        assert!(c.group_list.is_empty());
    }
    assert!(s.t(A).shared.groups.is_empty());
}

#[test]
fn asks_grants_and_takes_back_access_to_a_layer() {
    let mut s = hosted(&["Sky"], |_| {});
    join(&mut s, B, "Bo", FakeCanvas::new(&["Layer 1"]));
    let sky_lid = {
        let id = s.c(A).by_name("Sky").id;
        s.t(A).lid_of(id).unwrap()
    };
    s.with(B, |t, _| t.ask(&sky_lid));
    s.settle();
    assert_eq!(
        s.t(A).shared.asks,
        vec![ask(&sky_lid, B, Why::Draw, false, None)]
    );
    s.with(A, |t, _| t.grant(&sky_lid, B, false));
    s.settle();
    let sky = s.c(B).by_name("Sky").clone();
    assert!(!sky.locked);
    s.c(B).paint(sky.id, 3, 3, 33);
    s.with(B, |t, c| t.local_changed(c));
    s.settle();
    let hs = s.c(A).by_name("Sky").id;
    assert_eq!(s.c(A).pixel(hs, 3, 3), Some(33));

    s.with(A, |t, _| t.revoke(&sky_lid, B));
    s.settle();
    assert!(s.c(B).by_name("Sky").locked);
}

#[test]
fn ignores_tiles_from_someone_who_may_not_draw_there_and_retries_when_busy() {
    let mut s = hosted(&["Sky"], |_| {});
    let sky = s.c(A).by_name("Sky").id;
    let sky_lid = s.t(A).lid_of(sky).unwrap();
    // a forged change from C (not a writer)
    let evil = json!({"k": "tiles", "lid": sky_lid, "did": sky_lid, "set": [[0, 0, encode_tile(&tile(1))]], "clear": []});
    s.with(A, |t, c| t.on_edit(c, 99, C, &evil));
    assert_eq!(s.c(A).pixel(sky, 0, 0), None);

    // a joiner whose canvas is busy (mid-stroke) gets the tiles once it's free
    join(&mut s, B, "Bo", FakeCanvas::new(&["Layer 1"]));
    s.c(A).paint(sky, 4, 4, 44);
    s.c(B).busy = true;
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    let bs = s.c(B).by_name("Sky").id;
    assert_eq!(s.c(B).pixel(bs, 4, 4), None);
    s.c(B).busy = false;
    s.with(B, |t, c| t.local_changed(c));
    assert_eq!(s.c(B).pixel(bs, 4, 4), Some(44));
}

#[test]
fn deletes_and_moves_shared_layers_for_everyone_only_their_owner_can() {
    let mut s = hosted(&["Sky", "Hills"], |_| {});
    join(&mut s, B, "Bo", FakeCanvas::new(&[]));
    assert_eq!(s.c(A).visible_names(), vec!["Sky", "Hills", "Bo’s layer"]);
    assert_eq!(s.c(B).visible_names(), vec!["Sky", "Hills", "Bo’s layer"]);

    // Bo can't move or delete Ana's layers
    let sky = {
        let id = s.c(A).by_name("Sky").id;
        s.t(A).lid_of(id).unwrap()
    };
    s.send_edit(B, json!({"k": "remove", "lid": sky}));
    s.settle();
    assert!(s.c(B).visible_names().contains(&"Sky".to_string()));

    // Ana moves Sky to the top, everywhere
    s.with(A, |t, _| t.move_own(&sky, 2));
    s.settle();
    assert_eq!(s.c(A).visible_names(), vec!["Hills", "Bo’s layer", "Sky"]);
    assert_eq!(s.c(B).visible_names(), vec!["Hills", "Bo’s layer", "Sky"]);

    // and deletes Hills; Bo's canvas is mid-stroke, so it goes once he's done
    s.c(B).busy = true;
    let hills = {
        let id = s.c(A).by_name("Hills").id;
        s.t(A).lid_of(id).unwrap()
    };
    s.with(A, |t, _| t.remove_own(&hills));
    s.settle();
    assert_eq!(s.c(A).visible_names(), vec!["Bo’s layer", "Sky"]);
    assert!(s.c(B).visible_names().contains(&"Hills".to_string()));
    s.c(B).busy = false;
    s.with(B, |t, c| t.local_changed(c));
    assert_eq!(s.c(B).visible_names(), vec!["Bo’s layer", "Sky"]);
}

#[test]
fn shares_the_timeline_and_every_frames_drawing() {
    let mut s = hosted(&["Ball"], |hc| {
        let ball = hc.by_name("Ball").id;
        // two frames: drawing 1 for 2 frames, drawing 2 for 2 frames
        let d1 = hc.new_drawing();
        let d2 = hc.new_drawing();
        hc.drawings.get_mut(&d1).unwrap().insert((0, 0), tile(11));
        hc.drawings.get_mut(&d2).unwrap().insert((1, 0), tile(22));
        hc.tl = new_timeline(&[(ball, d1)]);
        hc.tl.scenes[0].cuts[0].tracks.insert(
            ball,
            vec![
                pencilpilot_together::timeline::Exposure {
                    drawing: d1,
                    length: 2,
                },
                pencilpilot_together::timeline::Exposure {
                    drawing: d2,
                    length: 2,
                },
            ],
        );
        hc.show_frame(0);
    });
    join(&mut s, B, "Bo", FakeCanvas::new(&[]));
    let ball = s.c(A).by_name("Ball").id;
    let b_ball = s.c(B).by_name("Ball").id;
    let track = s.c(B).track(b_ball);
    assert_eq!(
        track.iter().map(|e| e.length).collect::<Vec<_>>(),
        vec![2, 2]
    );
    // both frames' pixels arrived, though only the first was showing on the host
    assert_eq!(
        s.c(B).drawings[&track[0].drawing]
            .get(&(0, 0))
            .map(|t| t[0]),
        Some(11)
    );
    assert_eq!(
        s.c(B).drawings[&track[1].drawing]
            .get(&(1, 0))
            .map(|t| t[0]),
        Some(22)
    );

    // the host times frame 1 on threes: Bo's timeline follows
    s.c(A).tl.scenes[0].cuts[0].tracks.get_mut(&ball).unwrap()[0].length = 3;
    s.with(A, |t, c| t.timeline_changed(c));
    s.settle();
    assert_eq!(
        s.c(B)
            .track(b_ball)
            .iter()
            .map(|e| e.length)
            .collect::<Vec<_>>(),
        vec![3, 2]
    );

    // the host puts the ball in clean-up: Bo's ball layer is in it too
    set_stage(&mut s.c(A).tl, ball, "clean");
    s.with(A, |t, c| t.timeline_changed(c));
    s.settle();
    assert_eq!(
        s.c(B).tl.stages,
        [(b_ball, "clean".to_string())].into_iter().collect()
    );

    // Ana draws on frame 2 (showing it): Bo's frame 2 changes, not frame 1
    s.c(A).show_frame(3);
    s.c(A).paint(ball, 2, 2, 33);
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    assert_eq!(
        s.c(B).drawings[&track[1].drawing]
            .get(&(2, 2))
            .map(|t| t[0]),
        Some(33)
    );
    assert!(!s.c(B).drawings[&track[0].drawing].contains_key(&(2, 2)));
}

#[test]
fn shares_a_layer_made_from_another_above_it_timed_as_it_in_its_stage() {
    use pencilpilot_together::timeline::Exposure;
    let mut s = hosted(&["Rough", "Sky"], |hc| {
        let rough = hc.by_name("Rough").id;
        let (d1, d2) = (hc.new_drawing(), hc.new_drawing());
        hc.drawings.get_mut(&d1).unwrap().insert((0, 0), tile(11));
        hc.tl = new_timeline(&[(rough, d1)]);
        hc.tl.scenes[0].cuts[0].tracks.insert(
            rough,
            vec![
                Exposure {
                    drawing: d1,
                    length: 2,
                },
                Exposure {
                    drawing: 0,
                    length: 1,
                },
                Exposure {
                    drawing: d2,
                    length: 1,
                },
            ],
        );
        hc.show_frame(0);
    });
    join(&mut s, B, "Bo", FakeCanvas::new(&[]));
    let rough = s.c(A).by_name("Rough").id;

    // Ana makes a clean-up layer from the rough (on her canvas, as the studio does)
    let clean = s.c(A).add_layer("Clean-up (Rough)", None);
    assert!(s.with(A, |t, c| t.share_above(c, clean, rough)).is_some());
    {
        let c = s.c(A);
        let mut tl = std::mem::take(&mut c.tl);
        follow_timing(&mut tl, rough, clean, &mut || c.new_drawing());
        set_stage(&mut tl, clean, "clean");
        c.tl = tl;
    }
    s.with(A, |t, c| t.timeline_changed(c));
    s.settle();
    let order = vec!["Rough", "Clean-up (Rough)", "Sky", "Bo’s layer"];
    assert_eq!(s.c(A).visible_names(), order);
    assert_eq!(s.c(B).visible_names(), order);
    let b_clean = s.c(B).by_name("Clean-up (Rough)").id;
    let t = s.c(B).track(b_clean);
    assert_eq!(
        t.iter().map(|e| e.length).collect::<Vec<_>>(),
        vec![2, 1, 1]
    );
    assert!(t[0].drawing != 0 && t[2].drawing != 0 && t[0].drawing != t[2].drawing);
    assert_eq!(t[1].drawing, 0);
    assert_eq!(
        s.c(B).tl.stages.get(&b_clean).map(String::as_str),
        Some("clean")
    );
}

#[test]
fn shares_layer_stages_by_shared_layer_keeping_unshared_layers_own() {
    let mut local = new_timeline(&[(1, 0), (2, 0)]);
    set_stage(&mut local, 1, "rough");
    set_stage(&mut local, 2, "comp");
    let lid = |l: u32| (l == 1).then(|| "m:1".to_string());
    let out = to_shared(&local, &lid, &mut |_| String::new());
    assert_eq!(out["stages"], json!({"m:1": "rough"}));
    let back = |stages: Option<Value>| {
        let mut o = out.clone();
        match stages {
            Some(s) => o["stages"] = s,
            None => {
                o.as_object_mut().unwrap().remove("stages");
            }
        }
        from_shared(
            &o,
            &|l| (l == "m:1").then_some(1),
            &mut |_| 0,
            &local,
            &|l| l == 1,
        )
        .stages
    };
    let map = |v: &[(u32, &str)]| v.iter().map(|(k, s)| (*k, s.to_string())).collect();
    // theirs for the shared layer (unknown stages and layers left out), mine for my own
    assert_eq!(
        back(Some(json!({"m:1": "clean", "m:9": "rough"}))),
        map(&[(1, "clean"), (2, "comp")])
    );
    assert_eq!(back(Some(json!({"m:1": "wobbly"}))), map(&[(2, "comp")]));
    // an app that sends no stages leaves mine as they were
    assert_eq!(back(None), map(&[(1, "rough"), (2, "comp")]));
}

#[test]
fn shares_character_palettes_an_app_that_sends_none_leaves_mine() {
    let mut local = new_timeline(&[(1, 0)]);
    local.palettes = vec![Palette {
        name: "Mina".into(),
        colours: vec![PaletteColour {
            name: "Skin".into(),
            hex: "#f2c9a8".into(),
        }],
    }];
    let out = to_shared(&local, &|_| Some("m:1".into()), &mut |_| String::new());
    assert_eq!(
        out["palettes"],
        json!([{"name": "Mina", "colours": [{"name": "Skin", "hex": "#f2c9a8"}]}])
    );
    let back = |palettes: Option<Value>| {
        let mut o = out.clone();
        match palettes {
            Some(p) => o["palettes"] = p,
            None => {
                o.as_object_mut().unwrap().remove("palettes");
            }
        }
        from_shared(&o, &|_| Some(1), &mut |_| 0, &local, &|_| true).palettes
    };
    // theirs (cleaned: a colour that isn't one goes)
    assert_eq!(
        back(Some(
            json!([{"name": "Rin", "colours": [{"name": "Hair", "hex": "#223344"}, {"name": "x", "hex": "red"}]}])
        )),
        vec![Palette {
            name: "Rin".into(),
            colours: vec![PaletteColour {
                name: "Hair".into(),
                hex: "#223344".into()
            }]
        }]
    );
    assert_eq!(back(Some(json!([]))), vec![]);
    assert_eq!(back(None), local.palettes);
}

#[test]
fn comes_back_after_a_break_own_work_kept_the_others_caught_up() {
    let mut s = hosted(&["Sky"], |_| {});
    join(&mut s, B, "Bo", FakeCanvas::new(&[]));
    let bo_layer = s.c(B).by_name("Bo’s layer").id;
    s.c(B).paint(bo_layer, 0, 0, 1);
    s.with(B, |t, c| t.timeline_changed(c));
    s.with(B, |t, c| t.local_changed(c));
    s.settle();

    // Bo saves (the file keeps the session) and leaves
    let member = SavedMember {
        id: A.into(),
        name: "Ana".into(),
        addr: json!({}),
    };
    let json = s.t(B).save("pp1-code", &[member]).to_json();
    let saved = SavedShare::read(&serde_json::from_str(&json.to_string()).unwrap()).unwrap();
    assert_eq!(saved.ids.len(), 2);
    let App { c: mut bc, .. } = s.remove(B);

    // apart: Ana draws on her sky, Bo on his layer
    let sky = s.c(A).by_name("Sky").id;
    s.c(A).paint(sky, 5, 5, 55);
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    bc.paint(bo_layer, 6, 6, 66);

    // Bo comes back
    s.add(B, "Bo", bc);
    s.with(B, |t, c| t.restore(c, &saved));
    s.with(B, |t, _| t.rejoin(A));
    s.settle();
    assert!(s.t(B).synced());
    let bs = s.c(B).by_name("Sky").id;
    assert_eq!(s.c(B).pixel(bs, 5, 5), Some(55));
    // his work from the break is still his, and Ana got it
    assert_eq!(s.c(B).pixel(bo_layer, 6, 6), Some(66));
    let hb = s.c(A).by_name("Bo’s layer").id;
    assert_eq!(s.c(A).pixel(hb, 6, 6), Some(66));
    // no extra layer for him
    assert_eq!(
        s.c(B)
            .layer_list
            .iter()
            .filter(|l| l.name == "Bo’s layer")
            .count(),
        1
    );
}

#[test]
fn the_first_one_back_hosts_the_project_from_their_copy() {
    let mut s = hosted(&["Sky"], |hc| {
        hc.paint(1, 1, 1, 9);
    });
    let json = s.t(A).save("pp1-code", &[]).to_json();
    let saved = SavedShare::read(&serde_json::from_str(&json.to_string()).unwrap()).unwrap();
    let App { c: hc, .. } = s.remove(A);

    // next day: Ana alone hosts it again, Cy joins and gets it
    s.add(A, "Ana", hc);
    s.with(A, |t, c| t.restore(c, &saved));
    s.with(A, |t, c| t.resume_host(c));
    join(&mut s, C, "Cy", FakeCanvas::new(&[]));
    let cs = s.c(C).by_name("Sky").id;
    assert_eq!(s.c(C).pixel(cs, 1, 1), Some(9));
    assert_eq!(SavedShare::read(&json!({"code": "nope"})), None);
}

#[test]
fn shares_layer_settings_opacity_blend_visibility_from_whoever_may_draw_there() {
    let mut s = hosted(&["Sky"], |_| {});
    join(&mut s, B, "Bo", FakeCanvas::new(&[]));
    {
        let l = s.c(A).by_name_mut("Sky");
        l.opacity = 0.4;
        l.blend = "multiply".into();
    }
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    let bs = s.c(B).by_name("Sky").clone();
    assert_eq!((bs.opacity, bs.blend.as_str()), (0.4, "multiply"));

    // Bo changing Ana's layer here doesn't go anywhere (he may not draw there)
    s.c(B).by_name_mut("Sky").opacity = 1.0;
    s.with(B, |t, c| t.local_changed(c));
    s.settle();
    assert_eq!(s.c(A).by_name("Sky").opacity, 0.4);
    // a layer style (an outline) goes too
    let mut outline = LayerStyle::default();
    outline.stroke.on = true;
    outline.stroke.colour = "#ff0000".into();
    s.c(A).by_name_mut("Sky").style = Some(outline.clone());
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    assert_eq!(s.c(B).by_name("Sky").style, Some(outline));
    // and nothing echoes back and forth
    let before = s.seq;
    s.with(A, |t, c| t.local_changed(c));
    s.with(B, |t, c| t.local_changed(c));
    s.settle();
    assert_eq!(s.seq, before);
}

#[test]
fn shares_masks_and_adjustment_layers() {
    let mut s = hosted(&["Sky"], |_| {});
    join(&mut s, B, "Bo", FakeCanvas::new(&[]));

    // Ana masks part of the sky
    s.c(A).by_name_mut("Sky").mask = Some([((0, 0), tile(200))].into_iter().collect());
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    let m = |s: &mut FakeSession, who: &str, k| {
        s.c(who)
            .by_name("Sky")
            .mask
            .as_ref()
            .and_then(|m: &TileMap| m.get(&k).map(|t| t[0]))
    };
    assert_eq!(m(&mut s, B, (0, 0)), Some(200));

    // and adds a warm adjustment layer (made here above the sky: it goes on top)
    s.c(A).add_layer("Warm", Some("{\"hue\":12}"));
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    assert_eq!(
        s.c(B).by_name("Warm").adjust.as_deref(),
        Some("{\"hue\":12}")
    );
    assert_eq!(s.c(B).layer_list.last().unwrap().name, "Warm");
    // changes it
    let warm = s.c(A).by_name("Warm").id;
    s.c(A).set_adjust(warm, "{\"hue\":30}");
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    assert_eq!(
        s.c(B).by_name("Warm").adjust.as_deref(),
        Some("{\"hue\":30}")
    );

    // removes the mask
    s.c(A).by_name_mut("Sky").mask = None;
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    assert!(s.c(B).by_name("Sky").mask.is_none());

    // a joiner later gets both
    s.add(C, "Cy", FakeCanvas::new(&[]));
    s.c(A).by_name_mut("Sky").mask = Some([((1, 1), tile(99))].into_iter().collect());
    s.with(A, |t, c| t.local_changed(c));
    s.with(C, |t, c| t.start_join(c, A));
    s.settle();
    assert_eq!(m(&mut s, C, (1, 1)), Some(99));
    assert_eq!(
        s.c(C).by_name("Warm").adjust.as_deref(),
        Some("{\"hue\":30}")
    );
}

#[test]
fn shares_sound_clips_and_their_files_big_ones_in_pieces() {
    let mut s = hosted(&["Sky"], |_| {});
    join(&mut s, B, "Bo", FakeCanvas::new(&[]));

    // Ana adds a 600 KB song to the scene (three pieces on the way)
    let song: Vec<u8> = (0..600 * 1024).map(|i| (i % 251) as u8).collect();
    s.c(A).sounds.insert(
        "s1-abc.mp3".into(),
        (Arc::new(song.clone()), "song.mp3".into()),
    );
    s.c(A).tl.scenes[0].audio = vec![AudioClip {
        id: 1,
        name: "song.mp3".into(),
        file: "s1-abc.mp3".into(),
        start: 4.0,
        gain: 0.8,
        muted: false,
    }];
    s.with(A, |t, c| t.timeline_changed(c));
    s.settle();
    let a = s.c(B).tl.scenes[0].audio[0].clone();
    assert_eq!((a.file.as_str(), a.start, a.gain), ("s1-abc.mp3", 4.0, 0.8));
    assert_eq!(*s.c(B).sounds["s1-abc.mp3"].0, song);

    // sent once: moving the clip doesn't send the file again
    let before = s.seq;
    s.c(A).tl.scenes[0].audio[0].start = 8.0;
    s.with(A, |t, c| t.timeline_changed(c));
    s.settle();
    assert_eq!(s.seq - before, 1);
    assert_eq!(s.c(B).tl.scenes[0].audio[0].start, 8.0);

    // a joiner later gets the song with the project
    join(&mut s, C, "Cy", FakeCanvas::new(&[]));
    assert_eq!(*s.c(C).sounds["s1-abc.mp3"].0, song);
    assert_eq!(s.c(C).tl.scenes[0].audio[0].start, 8.0);
}

fn showing(s: &mut FakeSession, who: &str, id: u32) -> u32 {
    s.c(who)
        .layer_list
        .iter()
        .find(|l| l.id == id)
        .unwrap()
        .drawing
}

#[test]
fn shares_line_layers_their_strokes_stay_editable_everywhere_from_their_drawer_only() {
    let mut s = hosted(&["Ink"], |hc| {
        let ink = hc.by_name("Ink").id;
        hc.set_line_layer(ink, true);
        hc.paint(ink, 0, 0, 10);
        let d = hc.drawing_of(ink);
        hc.set_strokes(d, vec![stroke("a", 1.0), stroke("b", 2.0)], false);
    });
    let ink = s.c(A).by_name("Ink").id;
    let d = s.c(A).drawing_of(ink);

    // Bo joins: a line layer, with the strokes (and the pixels, as tiles)
    join(&mut s, B, "Bo", FakeCanvas::new(&[]));
    let b_ink = s.c(B).by_name("Ink").id;
    assert!(s.c(B).is_line_layer(b_ink));
    assert_eq!(s.c(B).pixel(b_ink, 0, 0), Some(10));
    let bd = showing(&mut s, B, b_ink);
    assert_eq!(s.c(B).stroke_ids(bd), vec!["a", "b"]);
    assert_eq!(s.c(B).line_strokes(bd)[1].pts, stroke("b", 2.0).pts);

    // Ana moves a stroke and adds one: only those travel, in place
    let list = s.c(A).line_strokes(d);
    let (a, b) = (list[0].clone(), list[1].clone());
    s.c(A)
        .set_strokes(d, vec![a, moved(&b, 5.0, 0.0), stroke("c", 3.0)], false);
    s.c(A).paint(ink, 1, 0, 20);
    let from = s.log.len();
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    let line_sent: Vec<Value> = s.log[from..]
        .iter()
        .filter(|(_, e)| e["k"] == "lines")
        .map(|(_, e)| e.clone())
        .collect();
    assert_eq!(line_sent.len(), 1);
    assert_eq!(line_sent[0]["del"], json!(["b"]));
    let at: Vec<u64> = line_sent[0]["put"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p[0].as_u64().unwrap())
        .collect();
    assert_eq!(at, vec![1, 2]);
    assert_eq!(s.c(B).stroke_ids(bd), vec!["a", "b", "c"]);
    assert_eq!(s.c(B).line_strokes(bd)[1].pts[0], 7.0);
    assert_eq!(s.c(B).pixel(b_ink, 1, 0), Some(20));
    // nothing new: nothing sent
    let from = s.log.len();
    s.with(A, |t, c| t.local_changed(c));
    assert!(s.log[from..].iter().all(|(_, e)| e["k"] != "lines"));

    // Bo may not change Ana's strokes
    let lid = s.t(B).lid_of(b_ink).unwrap();
    let did = s.t(A).did_of(d);
    s.send_edit(
        B,
        json!({"k": "lines", "lid": lid, "did": did, "del": ["a"], "put": []}),
    );
    s.settle();
    assert_eq!(s.c(A).stroke_ids(d), vec!["a", "b", "c"]);
    assert_eq!(s.c(B).stroke_ids(bd), vec!["a", "b", "c"]);

    // Ana converts it to pixels: a plain layer for Bo too
    s.c(A).set_line_layer(ink, false);
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    assert!(!s.c(B).is_line_layer(b_ink));
    assert!(!s.t(A).shared.layers[lid.as_str()].lines);
}

#[test]
fn shares_a_plain_layers_editable_strokes_with_the_pixels_under_them() {
    let mut s = hosted(&["Paint"], |hc| {
        let paint = hc.by_name("Paint").id;
        hc.paint(paint, 0, 0, 10);
        let d = hc.drawing_of(paint);
        hc.set_strokes(d, vec![stroke("a", 1.0)], true);
        hc.base_full.insert(d, base_pack(&[(0, 0)], &[]));
        hc.base_out.insert(d, base_pack(&[(0, 0)], &[]));
    });
    let paint = s.c(A).by_name("Paint").id;
    let d = s.c(A).drawing_of(paint);

    // Bo joins: the strokes, and the base whole
    join(&mut s, B, "Bo", FakeCanvas::new(&[]));
    let b_paint = s.c(B).by_name("Paint").id;
    assert!(!s.c(B).is_line_layer(b_paint));
    let bd = showing(&mut s, B, b_paint);
    assert_eq!(s.c(B).stroke_ids(bd), vec!["a"]);
    assert!(s.c(B).plain_strokes(bd));
    assert!(s.c(B).base_in.iter().any(|(x, _)| *x == bd));
    assert_eq!(s.c(B).base_in.last().unwrap().1, base_pack(&[(0, 0)], &[]));

    // Ana draws another: the stroke and the base's change travel in one edit
    s.c(B).base_in.clear();
    let mut list = (*s.c(A).line_strokes(d)).clone();
    list.push(stroke("b", 2.0));
    s.c(A).set_strokes(d, list, true);
    s.c(A).base_out.insert(d, base_pack(&[(1, 0)], &[]));
    s.c(A).paint(paint, 1, 0, 20);
    let from = s.log.len();
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    let line_sent: Vec<Value> = s.log[from..]
        .iter()
        .filter(|(_, e)| e["k"] == "lines")
        .map(|(_, e)| e.clone())
        .collect();
    assert_eq!(line_sent.len(), 1);
    assert_eq!(line_sent[0]["put"].as_array().unwrap().len(), 1);
    assert!(line_sent[0]["base"].as_str().is_some_and(|b| !b.is_empty()));
    assert_eq!(s.c(B).stroke_ids(bd), vec!["a", "b"]);
    assert_eq!(s.c(B).base_in[0].1, base_pack(&[(1, 0)], &[]));
    // a base change alone (strokes baked by a fill) still travels
    s.c(A).base_out.insert(d, base_pack(&[], &[(1, 0)]));
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    assert_eq!(s.c(B).base_in.last().unwrap().1, base_pack(&[], &[(1, 0)]));

    // Bo may not change them
    let lid = s.t(B).lid_of(b_paint).unwrap();
    let did = s.t(A).did_of(d);
    s.send_edit(
        B,
        json!({"k": "lines", "lid": lid, "did": did, "del": ["a"], "put": []}),
    );
    s.settle();
    assert_eq!(s.c(A).stroke_ids(d), vec!["a", "b"]);
}

// ---- drawing together: line edits ----

#[test]
fn checks_line_edits_and_new_line_layers() {
    let lid = "aaaaaaaaaaaa:1";
    let did = "aaaaaaaaaaaa:5";
    let enc = stroke_to_share(&stroke("s1", 4.0));
    assert_eq!(
        parse_edit(&json!({"k": "layer", "lid": lid, "name": "Ink", "lines": true})),
        Some(Edit::Layer {
            lid: lid.into(),
            name: "Ink".into(),
            adjust: None,
            lines: true
        })
    );
    // (an adjustment layer is never a line layer)
    assert_eq!(
        parse_edit(&json!({"k": "layer", "lid": lid, "name": "A", "adjust": "{}", "lines": true})),
        Some(Edit::Layer {
            lid: lid.into(),
            name: "A".into(),
            adjust: Some("{}".into()),
            lines: false
        })
    );
    assert_eq!(
        parse_edit(
            &json!({"k": "lines", "lid": lid, "did": did, "del": ["x", 7, "../"], "put": [[0, enc]]})
        ),
        Some(Edit::Lines {
            lid: lid.into(),
            did: did.into(),
            del: vec!["x".into()],
            put: vec![(0, enc.clone())],
            base: None
        })
    );
    assert_eq!(
        parse_edit(&json!({"k": "lines", "lid": lid, "did": "nope", "del": [], "put": []})),
        None
    );
    assert_eq!(
        parse_edit(&json!({"k": "lines", "lid": lid, "did": did, "del": [], "put": [[-1, enc]]})),
        None
    );
    assert_eq!(
        parse_edit(
            &json!({"k": "lines", "lid": lid, "did": did, "del": [], "put": [[0, "<script>"]]})
        ),
        None
    );
    let many: Vec<Value> = (0..401).map(|_| json!([0, "AA=="])).collect();
    assert_eq!(
        read_line_edit(json!({"del": [], "put": many}).as_object().unwrap()),
        None
    );
    assert_eq!(
        parse_edit(&json!({"k": "pixels", "lid": lid})),
        Some(Edit::Pixels { lid: lid.into() })
    );
    // a stroke that isn't one is dropped as it's put in
    assert_eq!(stroke_from_share("AAAA"), None);
    let got = apply_line_edit(&[], &[], &[(0, "AAAA".into()), (1, enc)]);
    assert_eq!(
        got.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec!["s1"]
    );
}

#[test]
fn lets_only_writers_send_strokes_on_any_layer_but_adjustment_layers() {
    let mut s = Shared::default();
    let lid = "aaaaaaaaaaaa:1";
    let did = "aaaaaaaaaaaa:5";
    s.apply(
        A,
        &Edit::Layer {
            lid: lid.into(),
            name: "Ink".into(),
            adjust: None,
            lines: true,
        },
    );
    s.apply(A, &layer_edit("aaaaaaaaaaaa:2", "Paint"));
    let e = |lid: &str| Edit::Lines {
        lid: lid.into(),
        did: did.into(),
        del: vec![],
        put: vec![],
        base: None,
    };
    assert!(s.apply(A, &e(lid)));
    assert!(!s.apply(B, &e(lid)));
    // a plain layer's editable strokes too, from its writers only
    assert!(s.apply(A, &e("aaaaaaaaaaaa:2")));
    assert!(!s.apply(B, &e("aaaaaaaaaaaa:2")));
    s.apply(
        A,
        &Edit::Layer {
            lid: "aaaaaaaaaaaa:3".into(),
            name: "Tone".into(),
            adjust: Some("{}".into()),
            lines: false,
        },
    );
    assert!(!s.apply(A, &e("aaaaaaaaaaaa:3")));
    assert!(!s.apply(B, &Edit::Pixels { lid: lid.into() }));
    assert!(s.apply(A, &Edit::Pixels { lid: lid.into() }));
    assert!(s.apply(A, &e(lid)));
    // kept across a save
    let mut t = Shared::default();
    t.load(
        &[json!({"lid": lid, "owner": A, "name": "Ink", "writers": [A], "lines": true})],
        &[],
        &[],
    );
    assert!(t.layers[lid].lines);
}

#[test]
fn reads_a_base_with_the_strokes_and_splits_big_bases_into_parts() {
    let lid = "aaaaaaaaaaaa:1";
    let did = "aaaaaaaaaaaa:5";
    assert_eq!(
        parse_edit(
            &json!({"k": "lines", "lid": lid, "did": did, "del": [], "put": [], "base": "AAAA"})
        ),
        Some(Edit::Lines {
            lid: lid.into(),
            did: did.into(),
            del: vec![],
            put: vec![],
            base: Some("AAAA".into())
        })
    );
    assert_eq!(
        parse_edit(
            &json!({"k": "lines", "lid": lid, "did": did, "del": [], "put": [], "base": "<x>"})
        ),
        None
    );
    assert_eq!(
        parse_edit(&json!({"k": "lines", "lid": lid, "did": did, "del": [], "put": [], "base": 5})),
        None
    );
    // 40 tiles with paint, 3 clear, 2 gone: parts under the size, the same entries in all
    let tiles = 40i32;
    let mut big = Vec::new();
    big.extend_from_slice(&43u32.to_le_bytes());
    for i in 0..43i32 {
        big.extend_from_slice(&i.to_le_bytes());
        big.extend_from_slice(&1i32.to_le_bytes());
    }
    big.extend_from_slice(&(tiles as u32).to_le_bytes());
    for i in 0..tiles {
        big.extend_from_slice(&i.to_le_bytes());
        big.extend_from_slice(&1i32.to_le_bytes());
        let mut px = vec![0u8; TILE_BYTES];
        px[0] = i as u8 + 1;
        big.extend_from_slice(&px);
    }
    big.extend_from_slice(&2u32.to_le_bytes());
    big.extend_from_slice(&0i32.to_le_bytes());
    big.extend_from_slice(&99i32.to_le_bytes());
    big.extend_from_slice(&0i32.to_le_bytes());
    big.extend_from_slice(&98i32.to_le_bytes());
    let parts = split_base(&big, 200_000).unwrap();
    assert!(parts.len() > 3);
    assert!(parts.iter().all(|p| p.len() <= 200_000));
    let count = |p: &Vec<u8>| u32::from_le_bytes(p[..4].try_into().unwrap());
    assert_eq!(parts.iter().map(count).sum::<u32>(), 43);
    assert_eq!(split_base(&big[..big.len() - 1], 900_000), None);
    assert_eq!(split_base(&big, 900_000).unwrap().len(), 1);
    // edits: the first part rides with the strokes, the rest on their own
    let base = base_pack(&[(0, 0)], &[]);
    let edits = with_base(
        line_edits(lid, did, &[], &[stroke("a", 1.0)]),
        lid,
        did,
        Some(&base),
    );
    assert_eq!(edits.len(), 1);
    let Edit::Lines { put, base: b, .. } = &edits[0] else {
        panic!()
    };
    assert_eq!(put.len(), 1);
    assert!(b.is_some());
    assert_eq!(with_base(vec![], lid, did, Some(&base)).len(), 1);
    assert_eq!(with_base(vec![], lid, did, None), vec![]);
}

#[test]
fn splits_big_changes_into_several_edits_that_put_back_the_same_strokes() {
    let many: Vec<LineStroke> = (0..900)
        .map(|i| stroke(&format!("s{i}"), i as f32))
        .collect();
    let edits = line_edits("aaaaaaaaaaaa:1", "aaaaaaaaaaaa:5", &[], &many);
    assert_eq!(edits.len(), 3);
    let mut list: Vec<LineStroke> = vec![];
    for e in &edits {
        let Edit::Lines { del, put, .. } = e else {
            panic!()
        };
        list = apply_line_edit(&list, del, put);
    }
    assert_eq!(
        list.iter().map(|s| &s.id).collect::<Vec<_>>(),
        many.iter().map(|s| &s.id).collect::<Vec<_>>()
    );
    assert_eq!(
        line_edits("aaaaaaaaaaaa:1", "aaaaaaaaaaaa:5", &many, &many),
        vec![]
    );
    // a deletion alone is one edit
    let gone = line_edits("aaaaaaaaaaaa:1", "aaaaaaaaaaaa:5", &many, &many[1..]);
    assert_eq!(
        gone,
        vec![Edit::Lines {
            lid: "aaaaaaaaaaaa:1".into(),
            did: "aaaaaaaaaaaa:5".into(),
            del: vec!["s0".into()],
            put: vec![],
            base: None
        }]
    );
}

// ---- drawing together: both drawing at once ----

fn rng(mut seed: u64) -> impl FnMut() -> f64 {
    move || {
        seed = (seed.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7fff_ffff;
        seed as f64 / 0x7fff_ffff as f64
    }
}

/// The tiles of the last layer called `name` that differ between two canvases.
fn differ(a: &FakeCanvas, b: &FakeCanvas, name: &str) -> Vec<String> {
    let by = |c: &FakeCanvas| {
        c.layer_list
            .iter()
            .rfind(|l| l.name == name)
            .unwrap()
            .drawing
    };
    let empty = TileMap::new();
    let da = a.drawings.get(&by(a)).unwrap_or(&empty);
    let db = b.drawings.get(&by(b)).unwrap_or(&empty);
    let mut bad = vec![];
    for k in da.keys().chain(db.keys()) {
        let (x, y) = (da.get(k).map(|t| t[0]), db.get(k).map(|t| t[0]));
        if x != y {
            bad.push(format!("{k:?}:{x:?}/{y:?}"));
        }
    }
    bad
}

fn both_drawing_at_once(seed: u64) {
    let mut r = rng(seed);
    let mut s = FakeSession::default();
    let mut hc = FakeCanvas::new(&["Layer 1"]);
    let hl = hc.by_name("Layer 1").id;
    hc.paint(hl, 0, 0, 9);
    s.add(A, "Nova", hc);
    s.with(A, |t, c| t.start_host(c));
    s.settle();
    join(&mut s, B, "Bo", FakeCanvas::new(&["Layer 1"]));
    let nl = s.c(A).by_name("Layer 1").id;
    let bl = s.c(B).by_name("Bo’s layer").id;
    s.c(B).paint(bl, 30, 30, 9);
    s.with(B, |t, c| t.timeline_changed(c));
    s.with(B, |t, c| t.local_changed(c));
    s.settle();
    let mut v = 1u32;
    let mut val = || {
        v += 1;
        ((v - 1) % 250 + 1) as u8
    };
    for _ in 0..300 {
        let x = r();
        if x < 0.2 {
            // a big soft stroke: a block of tiles
            let x0 = (r() * 20.0).floor() as i32;
            let y0 = (r() * 20.0).floor() as i32;
            s.c(A).busy = true;
            for i in 0..8 {
                for j in 0..8 {
                    let v = val();
                    s.c(A).paint(nl, x0 + i, y0 + j, v);
                }
            }
        } else if x < 0.3 {
            let x0 = (r() * 20.0).floor() as i32;
            s.c(B).busy = true;
            for j in 0..6 {
                let v = val();
                s.c(B).paint(bl, x0, j, v);
            }
        } else if x < 0.45 {
            s.c(A).busy = false;
            s.with(A, |t, c| t.local_changed(c));
        } else if x < 0.6 {
            s.c(B).busy = false;
            s.with(B, |t, c| t.local_changed(c));
        } else {
            let n = (r() * 5.0).floor() as usize;
            s.step(n);
        }
        let _ = r();
    }
    s.c(A).busy = false;
    s.c(B).busy = false;
    for _ in 0..2 {
        s.with(A, |t, c| t.local_changed(c));
        s.with(B, |t, c| t.local_changed(c));
        s.settle();
    }
    let (hc, bc) = (&s.apps[A].c, &s.apps[B].c);
    assert_eq!(differ(hc, bc, "Layer 1"), Vec::<String>::new());
    assert_eq!(differ(hc, bc, "Bo’s layer"), Vec::<String>::new());
}

macro_rules! seeds {
    ($($name:ident: $seed:expr),*) => {$(
        #[test]
        fn $name() {
            both_drawing_at_once($seed);
        }
    )*};
}

seeds!(
    ends_the_same_everywhere_seed_1: 1, ends_the_same_everywhere_seed_2: 2,
    ends_the_same_everywhere_seed_3: 3, ends_the_same_everywhere_seed_4: 4,
    ends_the_same_everywhere_seed_5: 5, ends_the_same_everywhere_seed_6: 6,
    ends_the_same_everywhere_seed_7: 7, ends_the_same_everywhere_seed_8: 8,
    ends_the_same_everywhere_seed_9: 9, ends_the_same_everywhere_seed_10: 10,
    ends_the_same_everywhere_seed_11: 11, ends_the_same_everywhere_seed_12: 12
);

// ---- drawing together: edits arriving together ----

#[test]
fn are_put_in_the_order_the_session_numbered_them_not_as_each_finishes() {
    let mut s = hosted(&["Layer 1"], |_| {});
    join(&mut s, B, "Bo", FakeCanvas::new(&["Layer 1"]));
    let nl = s.c(A).by_name("Layer 1").id;
    // a big stroke (many tiles) ending on the tile at 30,30...
    for i in 0..40 {
        s.c(A).paint(nl, i % 8, i / 8, i as u8 + 1);
    }
    s.c(A).paint(nl, 30, 30, 100);
    s.with(A, |t, c| t.local_changed(c));
    // ...then a small one over that same tile
    s.c(A).paint(nl, 30, 30, 200);
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    // (Bo's own "Layer 1" from before joining is kept, hidden: the shared one is last)
    let shared = s
        .c(B)
        .layer_list
        .iter()
        .rfind(|l| l.name == "Layer 1")
        .unwrap()
        .id;
    assert_eq!(s.c(B).pixel(shared, 30, 30), Some(200));
    let (hc, bc) = (&s.apps[A].c, &s.apps[B].c);
    assert_eq!(differ(hc, bc, "Layer 1"), Vec::<String>::new());
}

// ---- drawing together: timelines crossing ----

#[test]
fn two_new_drawings_started_at_once_both_stay_on_the_timeline() {
    use pencilpilot_together::timeline::Exposure;
    let mut s = hosted(&["Layer 1"], |_| {});
    join(&mut s, B, "Bo", FakeCanvas::new(&["Layer 1"]));
    let nl = {
        let c = s.c(A);
        c.layer_list[c.layer_list.len() - 2].id
    }; // Nova's Layer 1 (Bo's layer is on top)
    let bl = s.c(B).by_name("Bo’s layer").id;
    // (first a stroke each, synced: both layers have a drawing everyone knows)
    s.c(A).paint(nl, 0, 0, 1);
    s.c(B).paint(bl, 0, 0, 2);
    s.with(A, |t, c| t.timeline_changed(c));
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    s.with(B, |t, c| t.timeline_changed(c));
    s.with(B, |t, c| t.local_changed(c));
    s.settle();
    // then each starts a new drawing on the frame (as the page does on an empty cel:
    // a new drawing, put on the timeline here) at about the same time
    let fresh = |c: &mut FakeCanvas, id: u32| {
        let d = c.new_drawing();
        c.layer_list
            .iter_mut()
            .find(|l| l.id == id)
            .unwrap()
            .drawing = d;
        c.tl.scenes[0].cuts[0].tracks.insert(
            id,
            vec![Exposure {
                drawing: d,
                length: 1,
            }],
        );
    };
    fresh(s.c(A), nl);
    fresh(s.c(B), bl);
    s.c(A).paint(nl, 1, 1, 11);
    s.c(B).paint(bl, 5, 5, 55);
    s.with(A, |t, c| t.timeline_changed(c));
    s.with(B, |t, c| t.timeline_changed(c));
    s.with(A, |t, c| t.local_changed(c));
    s.with(B, |t, c| t.local_changed(c));
    s.settle();
    let b_nova = s
        .c(B)
        .layer_list
        .iter()
        .rfind(|l| l.name == "Layer 1")
        .unwrap()
        .id;
    assert_eq!(s.c(A).pixel(nl, 1, 1), Some(11));
    assert_eq!(s.c(B).pixel(bl, 5, 5), Some(55));
    assert_eq!(s.c(B).pixel(b_nova, 1, 1), Some(11));
    let hb = s.c(A).by_name("Bo’s layer").id;
    assert_eq!(s.c(A).pixel(hb, 5, 5), Some(55));
}

#[allow(dead_code)]
fn _types(_: TileSet, _: Timeline, _: Look) {}

// ---- drawing together: mixed sessions (fixes from the native app's QA) ----

#[test]
fn answers_a_joiner_who_asks_before_the_host_has_shared_the_project() {
    let mut s = FakeSession::default();
    let mut hc = FakeCanvas::new(&["Sky", "Sea"]);
    hc.paint(1, 0, 0, 50);
    s.add(A, "Ana", hc);
    // (Bo's ask is in before Ana's app has shared it all)
    s.add(B, "Bo", FakeCanvas::new(&[]));
    s.with(B, |t, c| t.start_join(c, A));
    s.step(1);
    assert!(!s.t(B).synced());
    s.with(A, |t, c| t.start_host(c));
    s.settle();
    assert!(s.t(B).synced());
    let sky = s.c(B).by_name("Sky").id;
    assert_eq!(s.c(B).pixel(sky, 0, 0), Some(50));
    assert_eq!(s.c(B).names(), vec!["Sky", "Sea", "Bo’s layer"]);
}

#[test]
fn takes_a_snapshots_tiles_that_come_after_its_end_for_drawings_it_has_nothing_of() {
    let mut s = hosted(&["Sky"], |_| {});
    let lid = s.t(A).lid_of(1).unwrap();
    let layers = s.t(A).shared.snapshot().0;
    let mut bc = FakeCanvas::new(&[]);
    let mut bo = pencilpilot_together::Together::with_start(B, "Bo", 0);
    bo.start_join(&mut bc, A);
    bo.on_direct(
        &mut bc,
        A,
        &json!({"k": "snap.begin", "seq": 0, "layers": layers, "asks": [], "tl": null}),
    );
    bo.on_direct(&mut bc, A, &json!({"k": "snap.end"}));
    let late = "aaaaaaaaaaaa:777";
    bo.on_direct(&mut bc, A, &json!({"k": "snap.tiles", "lid": lid, "did": late, "set": [[2, 2, encode_tile(&tile(9))]]}));
    let d = bo.local_of(&mut bc, late);
    assert_eq!(bc.drawings[&d].get(&(2, 2)).map(|t| t[0]), Some(9));
    // a drawing it has already: not taken (what it has may be newer)
    bo.on_direct(&mut bc, A, &json!({"k": "snap.tiles", "lid": lid, "did": late, "set": [[2, 2, encode_tile(&tile(1))]]}));
    assert_eq!(bc.drawings[&d].get(&(2, 2)).map(|t| t[0]), Some(9));
}

#[test]
fn taken_back_mid_stroke_what_was_drawn_and_not_sent_goes_here_as_for_everyone() {
    let mut s = hosted(&["Sky"], |hc| {
        hc.paint(1, 0, 0, 10);
    });
    join(&mut s, B, "Bo", FakeCanvas::new(&[]));
    let sky = s.t(A).lid_of(1).unwrap();
    s.with(A, |t, _| t.grant(&sky, B, false));
    s.settle();
    let bs = s.c(B).by_name("Sky").id;
    s.c(B).paint(bs, 3, 3, 33);
    s.with(B, |t, c| t.local_changed(c));
    s.settle();
    // mid-stroke when it's taken back
    s.c(B).paint(bs, 4, 4, 44);
    s.c(B).paint(bs, 0, 0, 99);
    s.c(B).busy = true;
    s.with(A, |t, _| t.revoke(&sky, B));
    s.settle();
    s.c(B).busy = false;
    s.with(B, |t, c| t.local_changed(c));
    s.settle();
    for who in [A, B] {
        let id = s.c(who).by_name("Sky").id;
        let c = s.c(who);
        assert_eq!(
            [c.pixel(id, 0, 0), c.pixel(id, 3, 3), c.pixel(id, 4, 4)],
            [Some(10), Some(33), None]
        );
    }
}

#[test]
fn hosting_a_saved_session_again_shares_its_line_strokes_with_joiners() {
    let mut s = hosted(&["Ink"], |hc| {
        hc.set_line_layer(1, true);
        let d = hc.drawing_of(1);
        hc.set_strokes(d, vec![stroke("a", 1.0)], false);
    });
    let saved = SavedShare::read(
        &serde_json::from_str(&s.t(A).save("pp1-code", &[]).to_json().to_string()).unwrap(),
    )
    .unwrap();
    let App { c: hc, .. } = s.remove(A);
    s.add(A, "Ana", hc);
    s.with(A, |t, c| t.restore(c, &saved));
    s.with(A, |t, c| t.resume_host(c));
    join(&mut s, C, "Cy", FakeCanvas::new(&[]));
    let ink = s.c(C).by_name("Ink").id;
    let cd = showing(&mut s, C, ink);
    assert_eq!(s.c(C).stroke_ids(cd), vec!["a"]);
}

#[test]
fn a_line_layer_duplicated_or_pasted_is_a_line_layer_for_everyone() {
    let mut s = hosted(&["Sky"], |_| {});
    let id = s.c(A).add_layer("Ink copy", None);
    s.c(A).set_line_layer(id, true);
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    let lid = s.t(A).lid_of(id).unwrap();
    assert!(s.t(A).shared.layers[lid.as_str()].lines);
}

#[test]
fn strokes_put_in_another_order_none_changed_end_in_that_order_everywhere() {
    let list: Vec<LineStroke> = ["a", "b", "c", "d", "e"]
        .iter()
        .enumerate()
        .map(|(i, id)| stroke(id, i as f32))
        .collect();
    let now = vec![
        list[3].clone(),
        list[0].clone(),
        list[1].clone(),
        list[2].clone(),
        list[4].clone(),
    ];
    let edits = line_edits("aaaaaaaaaaaa:1", "aaaaaaaaaaaa:5", &list, &now);
    assert_eq!(edits.len(), 1);
    let Edit::Lines { del, put, .. } = &edits[0] else {
        panic!()
    };
    assert_eq!(del, &["d"]);
    assert_eq!(put.iter().map(|p| p.0).collect::<Vec<_>>(), vec![0]);
    let got = apply_line_edit(&list, del, put);
    assert_eq!(
        got.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec!["d", "a", "b", "c", "e"]
    );
    assert_eq!(
        line_edits("aaaaaaaaaaaa:1", "aaaaaaaaaaaa:5", &list, &list),
        vec![]
    );
}

#[test]
fn a_snapshot_with_odd_parts_still_joins_what_isnt_a_layer_is_left_out() {
    let mut bc = FakeCanvas::new(&[]);
    let mut bo = pencilpilot_together::Together::with_start(B, "Bo", 0);
    bo.start_join(&mut bc, A);
    bo.on_direct(&mut bc, A, &json!({
        "k": "snap.begin",
        "layers": [null, {"lid": "aaaaaaaaaaaa:1", "owner": A, "name": "Sky", "writers": [A]}, {"lid": "aaaaaaaaaaaa:2"}, 5],
        "groups": [null, {"gid": 7}], "asks": "x", "tl": null
    }));
    bo.on_direct(&mut bc, A, &json!({"k": "snap.tiles", "lid": "aaaaaaaaaaaa:1", "did": "aaaaaaaaaaaa:3", "set": "nope"}));
    bo.on_direct(&mut bc, A, &json!({"k": "snap.tiles", "lid": "aaaaaaaaaaaa:1", "did": "aaaaaaaaaaaa:3", "set": [["a", "b", "z:"], null]}));
    bo.on_direct(&mut bc, A, &json!({"k": "snap.end"}));
    assert!(bo.synced());
    assert_eq!(bc.names(), vec!["Sky"]);
    // (and edits after it, with no number on the snapshot, are put in)
    bo.on_edit(
        &mut bc,
        1,
        A,
        &json!({"k": "rename", "lid": "aaaaaaaaaaaa:1", "name": "Sea"}),
    );
    assert!(bc.has("Sea"));
    bo.on_edit(
        &mut bc,
        2,
        A,
        &json!({"k": "sound", "file": "x.mp3", "i": 0, "n": 1, "data": "@@@"}),
    );
    bo.on_edit(&mut bc, 3, A, &json!({"k": "lines", "lid": "aaaaaaaaaaaa:1", "did": "aaaaaaaaaaaa:3", "del": [], "put": [], "base": "A"}));
}

#[test]
fn names_never_keep_half_a_character() {
    let e = parse_edit(
        &json!({"k": "layer", "lid": "aaaaaaaaaaaa:1", "name": format!("{}😀😀", "a".repeat(59))}),
    );
    assert_eq!(e, Some(layer_edit("aaaaaaaaaaaa:1", &"a".repeat(59))));
}

#[test]
fn clears_go_in_edits_every_app_takes_however_many() {
    use pencilpilot_together::tile_parts;
    let set: Vec<u32> = (0..50).collect();
    let clear: Vec<u32> = (0..300).collect();
    let parts: Vec<(usize, usize)> = tile_parts(&set, &clear)
        .iter()
        .map(|(s, c)| (s.len(), c.len()))
        .collect();
    assert_eq!(parts, vec![(48, 144), (2, 144), (0, 12)]);
    assert_eq!(tile_parts::<u32, u32>(&[], &[]).len(), 1);
    // erasing a big area: every app takes every edit
    let mut s = hosted(&["Sky"], |hc| {
        for i in 0..400 {
            hc.paint(1, i % 20, i / 20, 5);
        }
    });
    join(&mut s, B, "Bo", FakeCanvas::new(&[]));
    let d = s.c(A).drawing_of(1);
    s.c(A).drawings.get_mut(&d).unwrap().clear();
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    let sky = s.c(B).by_name("Sky").id;
    let bd = showing(&mut s, B, sky);
    assert!(s.c(B).drawings[&bd].is_empty());
}

// ---- drawing together: a busy canvas ----

#[test]
fn takes_in_what_came_while_it_was_busy_when_retried_with_no_change_of_its_own() {
    let mut s = hosted(&["Sky", "Hills"], |hc| {
        hc.add_group(2);
    });
    // Bo's canvas is busy (someone's live stroke being drawn) as the project comes in
    let mut bc = FakeCanvas::new(&[]);
    bc.busy = true;
    s.add(B, "Bo", bc);
    s.with(B, |t, c| t.start_join(c, A));
    s.settle();
    s.c(A).paint(1, 7, 7, 70);
    s.with(A, |t, c| t.local_changed(c));
    s.settle();
    assert!(s.t(B).waiting());
    assert!(s.c(B).group_list.is_empty());
    // the stroke ends: the app retries (no change of its own)
    s.c(B).busy = false;
    s.with(B, |t, c| t.retry(c));
    assert!(!s.t(B).waiting());
    let sky = s.c(B).by_name("Sky").id;
    assert_eq!(s.c(B).pixel(sky, 7, 7), Some(70));
    assert_eq!(s.c(B).group_list.len(), 1);
    let g = s.c(B).group_list[0].id;
    assert_eq!(s.c(B).by_name("Hills").group, g);
    // retries while busy don't use up a change's tries
    s.c(B).busy = true;
    let hills = s.t(A).lid_of(2).unwrap();
    s.with(A, |t, _| t.move_own(&hills, 0));
    s.settle();
    for _ in 0..200 {
        s.with(B, |t, c| t.retry(c));
    }
    assert!(s.t(B).waiting());
    s.c(B).busy = false;
    s.with(B, |t, c| t.retry(c));
    assert_eq!(s.c(B).names()[..2], ["Hills", "Sky"]);
}
