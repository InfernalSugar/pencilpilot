//! Text layers. The app sets the text (with the system's fonts) and hands the engine
//! its pixels, with a description of the text it keeps on the layer (`LayerProps::text`).
//! Typing is previewed live on the layer; the result is one undo step. Painting on a
//! text layer any other way turns it into a plain layer (undo brings the text back).

use hokusai::TILE_SIZE;
use hokusai::tile::empty_tile;

use crate::layers::premul_fix15;
use crate::{Edit, Layer, LayerProps, MAX_LAYERS, Painter, Snapshot, TileKey, TileMap};

/// Text being typed: the layer, and its tiles and settings from before (none: the layer
/// was made for this text and goes again if it's cancelled).
pub(crate) struct TextEdit {
    layer: u32,
    before: Option<(TileMap, LayerProps)>,
    /// the text's description, as last previewed
    spec: String,
}

impl Painter {
    /// Start typing on text layer `id`, or (0) on a new layer `name` above the active
    /// one. Returns the layer's id: 0 if it can't be (busy, locked, hidden, an
    /// adjustment layer, or no room for another layer).
    pub fn text_begin(&mut self, id: u32, name: &str) -> u32 {
        if !self.idle() {
            return 0;
        }
        if id != 0 {
            let Some(idx) = self.index_of(id) else {
                return 0;
            };
            let l = &self.layers[idx];
            if l.props.locked || l.props.adjust.is_some() || !self.shown(idx) {
                return 0;
            }
            self.layers[idx].set_editing(false);
            let l = &self.layers[idx];
            self.text_editing = Some(TextEdit {
                layer: id,
                before: Some((l.surface.tiles.clone(), l.props.clone())),
                spec: l.props.text.clone(),
            });
            self.active = id;
            return id;
        }
        if self.layers.len() >= MAX_LAYERS {
            return 0;
        }
        let id = self.next_id;
        self.next_id += 1;
        let index = self
            .index_of(self.active)
            .map_or(self.layers.len(), |i| i + 1);
        let name = if name.trim().is_empty() {
            format!("Text {id}")
        } else {
            name.chars().take(60).collect()
        };
        let mut layer = Layer::new(id, LayerProps::named(name));
        // in the selected layer's group, as a new layer goes
        layer.group = index
            .checked_sub(1)
            .and_then(|i| self.layers.get(i))
            .map_or(0, |l| l.group);
        self.layers.insert(index, layer);
        self.mark_layer(index);
        self.active = id;
        self.text_editing = Some(TextEdit {
            layer: id,
            before: None,
            spec: String::new(),
        });
        id
    }

    /// The text layer being typed on (0: none).
    pub fn text_layer(&self) -> u32 {
        self.text_editing.as_ref().map_or(0, |t| t.layer)
    }

    /// Show the text as it is now: the layer's pixels become `rgba` (straight-alpha
    /// sRGB, `w`×`h`) placed at (`x`, `y`), nothing else; `spec` describes it. Not an
    /// undo step (see [`Self::text_commit`]).
    pub fn text_preview(
        &mut self,
        spec: &str,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        rgba: &[u8],
    ) -> bool {
        let (w, h) = (w as usize, h as usize);
        if rgba.len() != w * h * 4 {
            return false;
        }
        let Some(layer) = self.text_editing.as_ref().map(|t| t.layer) else {
            return false;
        };
        let tiles = self.placed_tiles(x, y, w, h, rgba);
        let Some(l) = self.layer_mut(layer) else {
            return false;
        };
        let old = std::mem::replace(&mut l.surface.tiles, tiles);
        let keys: Vec<TileKey> = old.keys().chain(l.surface.tiles.keys()).copied().collect();
        l.surface.dirty.extend(keys);
        if let Some(t) = self.text_editing.as_mut() {
            t.spec = spec.to_string();
        }
        true
    }

    /// Keep the text typed: one undo step (with the layer, if it was made for it).
    /// False if nothing was being typed or nothing changed.
    pub fn text_commit(&mut self) -> bool {
        let Some(t) = self.text_editing.take() else {
            return false;
        };
        let Some(idx) = self.index_of(t.layer) else {
            return false;
        };
        match t.before {
            None => {
                let l = &mut self.layers[idx];
                l.props.text = t.spec;
                if !l.surface.tiles.is_empty() {
                    l.drawing = self.next_drawing;
                    self.next_drawing += 1;
                }
                // undoing it takes the layer away again
                self.push_step(Edit::Remove { index: idx });
            }
            Some((tiles, props)) => {
                let l = &mut self.layers[idx];
                if l.props.text == t.spec && l.surface.tiles == tiles {
                    return false;
                }
                let mut snap: Snapshot = l.surface.tiles.keys().map(|k| (*k, None)).collect();
                snap.extend(tiles.into_iter().map(|(k, v)| (k, Some(v))));
                l.props.text = t.spec;
                self.push_step(Edit::Batch(vec![
                    Edit::Tiles {
                        layer: t.layer,
                        snap,
                        mask: false,
                        drawing: 0,
                    },
                    Edit::Props {
                        layer: t.layer,
                        props,
                    },
                ]));
            }
        }
        self.mark_layer(idx);
        true
    }

    /// Drop the text typed: the layer goes back to how it was (or goes, if it was made
    /// for it).
    pub fn text_cancel(&mut self) {
        let Some(t) = self.text_editing.take() else {
            return;
        };
        let Some(idx) = self.index_of(t.layer) else {
            return;
        };
        match t.before {
            None => {
                // (as removing a layer does: the one below becomes active)
                self.apply(Edit::Remove { index: idx });
            }
            Some((tiles, props)) => {
                let l = &mut self.layers[idx];
                let old = std::mem::replace(&mut l.surface.tiles, tiles);
                let keys: Vec<TileKey> =
                    old.keys().chain(l.surface.tiles.keys()).copied().collect();
                l.surface.dirty.extend(keys);
                l.props = props;
                self.mark_layer(idx);
            }
        }
    }

    /// Turn a text layer into a plain one (one undo step). False if it isn't text.
    pub fn rasterize_text(&mut self, id: u32) -> bool {
        if !self.idle() {
            return false;
        }
        let Some(idx) = self.index_of(id) else {
            return false;
        };
        if self.layers[idx].props.text.is_empty() {
            return false;
        }
        let before = self.layers[idx].props.clone();
        self.layers[idx].props.text.clear();
        self.push_step(Edit::Props {
            layer: id,
            props: before,
        });
        true
    }

    /// Set a layer's text description without an undo step (opening a project).
    pub fn set_text_silent(&mut self, id: u32, spec: &str) -> bool {
        match self.layer_mut(id) {
            Some(l) if l.props.adjust.is_none() => {
                l.props.text = spec.to_string();
                true
            }
            _ => false,
        }
    }

    /// Text layers whose colour `e` changes become plain layers; `e` then puts their
    /// text back when it's undone.
    pub(crate) fn rasterize_painted(&mut self, e: &mut Edit) {
        fn painted(e: &Edit, out: &mut Vec<u32>) {
            match e {
                Edit::Tiles {
                    layer, mask: false, ..
                } => out.push(*layer),
                Edit::Batch(list) => list.iter().for_each(|e| painted(e, out)),
                _ => {}
            }
        }
        let mut ids = Vec::new();
        painted(e, &mut ids);
        ids.dedup();
        let mut back = Vec::new();
        for id in ids {
            if let Some(l) = self.layer_mut(id)
                && !l.props.text.is_empty()
            {
                back.push(Edit::Props {
                    layer: id,
                    props: l.props.clone(),
                });
                l.props.text.clear();
            }
        }
        if !back.is_empty() {
            // first when undoing: the step's own settings changes (a merge's) come after
            back.push(std::mem::replace(e, Edit::Batch(Vec::new())));
            *e = Edit::Batch(back);
        }
    }

    /// `rgba` (`w`×`h`, straight-alpha sRGB) placed at (`x0`, `y0`) as tiles, cut to
    /// the canvas.
    fn placed_tiles(&self, x0: i32, y0: i32, w: usize, h: usize, rgba: &[u8]) -> TileMap {
        let ts = TILE_SIZE as i32;
        let (cw, ch) = (self.width as i32, self.height as i32);
        let mut tiles = TileMap::new();
        for y in 0..h {
            let cy = y0 + y as i32;
            if cy < 0 || cy >= ch {
                continue;
            }
            for x in 0..w {
                let cx = x0 + x as i32;
                let i = (y * w + x) * 4;
                if cx < 0 || cx >= cw || rgba[i + 3] == 0 {
                    continue;
                }
                let key = (cx.div_euclid(ts), cy.div_euclid(ts));
                let t = tiles.entry(key).or_insert_with(empty_tile);
                let c = [rgba[i], rgba[i + 1], rgba[i + 2]].map(|v| v as f32 / 255.0);
                t[(cy - key.1 * ts) as usize][(cx - key.0 * ts) as usize] =
                    premul_fix15(c, rgba[i + 3] as f32 / 255.0);
            }
        }
        tiles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// a `w`×`h` block of opaque red
    fn red(w: usize, h: usize) -> Vec<u8> {
        [255, 0, 0, 255].repeat(w * h)
    }

    fn info(p: &Painter) -> String {
        p.layers_json()
    }

    #[test]
    fn new_text_is_one_step_with_its_layer() {
        let mut p = Painter::new(200, 100);
        let id = p.text_begin(0, "Hello");
        assert!(id > 1);
        assert_eq!(p.text_layer(), id);
        assert!(p.text_preview(r#"{"text":"Hi"}"#, 10, 10, 20, 8, &red(20, 8)));
        assert!(p.text_commit());
        assert_eq!(p.text_layer(), 0);
        assert!(info(&p).contains(r#""text":"{\"text\":\"Hi\"}""#));
        assert_eq!(
            p.layer_rgba(id)[(12 * 200 + 15) * 4..][..4],
            [255, 0, 0, 255]
        );
        // one undo takes the layer away, redo brings it back with its text
        assert!(p.undo());
        assert_eq!(p.layers.len(), 1);
        assert!(p.redo());
        assert!(info(&p).contains(r#"\"Hi\""#));
    }

    #[test]
    fn cancelling_a_new_text_leaves_nothing() {
        let mut p = Painter::new(200, 100);
        p.text_begin(0, "");
        p.text_preview("{}", 0, 0, 4, 4, &red(4, 4));
        p.text_cancel();
        assert_eq!(p.layers.len(), 1);
        assert!(!p.can_undo());
    }

    #[test]
    fn editing_text_swaps_pixels_and_spec_in_one_step() {
        let mut p = Painter::new(200, 100);
        let id = p.text_begin(0, "");
        p.text_preview("A", 0, 0, 10, 10, &red(10, 10));
        p.text_commit();
        assert_eq!(p.text_begin(id, ""), id);
        p.text_preview("B", 100, 50, 10, 10, &red(10, 10));
        assert!(p.text_commit());
        let px = |p: &Painter, x: usize, y: usize| p.layer_rgba(id)[(y * 200 + x) * 4 + 3];
        assert_eq!((px(&p, 5, 5), px(&p, 105, 55)), (0, 255));
        assert!(info(&p).contains(r#""text":"B""#));
        p.undo();
        assert_eq!((px(&p, 5, 5), px(&p, 105, 55)), (255, 0));
        assert!(info(&p).contains(r#""text":"A""#));
        // typing and cancelling changes nothing
        p.text_begin(id, "");
        p.text_preview("C", 0, 0, 1, 1, &red(1, 1));
        p.text_cancel();
        assert_eq!(px(&p, 5, 5), 255);
        assert!(info(&p).contains(r#""text":"A""#));
    }

    #[test]
    fn painting_rasterizes_and_undo_brings_the_text_back() {
        let mut p = Painter::new(200, 100);
        let id = p.text_begin(0, "");
        p.text_preview("A", 0, 0, 10, 10, &red(10, 10));
        p.text_commit();
        assert!(p.clear());
        assert!(info(&p).contains(r#""text":null"#));
        p.undo();
        assert!(info(&p).contains(r#""text":"A""#));
        assert!(p.rasterize_text(id));
        assert!(!p.rasterize_text(id));
        p.undo();
        assert!(info(&p).contains(r#""text":"A""#));
    }

    #[test]
    fn copies_keep_the_text_and_locked_layers_refuse() {
        let mut p = Painter::new(200, 100);
        let id = p.text_begin(0, "");
        p.text_preview("A", 0, 0, 2, 2, &red(2, 2));
        p.text_commit();
        let copy = p.duplicate_layer(id);
        assert_eq!(info(&p).matches(r#""text":"A""#).count(), 2);
        let mut props = p.layers[p.index_of(copy).unwrap()].props.clone();
        props.locked = true;
        p.set_layer_props(copy, props, true);
        assert_eq!(p.text_begin(copy, ""), 0);
        // (settings changes keep the text)
        assert_eq!(info(&p).matches(r#""text":"A""#).count(), 2);
    }

    #[test]
    fn merging_onto_text_rasterizes_it() {
        let mut p = Painter::new(100, 100);
        let id = p.text_begin(0, "");
        p.text_preview("A", 0, 0, 2, 2, &red(2, 2));
        p.text_commit();
        let top = p.add_image_layer("pic", &red(100, 100));
        assert!(p.merge_down(top));
        assert!(info(&p).contains(r#""text":null"#));
        p.undo();
        let l = &p.layers[p.index_of(id).unwrap()];
        assert_eq!((l.props.text.as_str(), l.props.imported), ("A", false));
    }

    /// a text layer "A" (a 10×10 block at the top-left), active
    fn text_layer(p: &mut Painter) -> u32 {
        let id = p.text_begin(0, "");
        p.text_preview("A", 0, 0, 10, 10, &red(10, 10));
        p.text_commit();
        id
    }

    fn text_of(p: &Painter, id: u32) -> String {
        p.layers[p.index_of(id).unwrap()].props.text.clone()
    }

    #[test]
    fn a_text_layer_keeps_its_style_through_edits_and_rasterizing() {
        let mut p = Painter::new(100, 100);
        let id = text_layer(&mut p);
        let mut props = p.layers[p.index_of(id).unwrap()].props.clone();
        props.style.stroke.on = true;
        p.set_layer_props(id, props, true);
        p.text_begin(id, "");
        p.text_preview("B", 20, 20, 4, 4, &red(4, 4));
        p.text_commit();
        let l = &p.layers[p.index_of(id).unwrap()];
        assert!(l.props.style.stroke.on);
        assert_eq!(l.props.text, "B");
        assert!(p.rasterize_text(id));
        assert!(p.layers[p.index_of(id).unwrap()].props.style.stroke.on);
        p.undo();
        p.undo();
        assert_eq!(text_of(&p, id), "A");
        assert!(p.layers[p.index_of(id).unwrap()].props.style.stroke.on);
    }

    #[test]
    fn filters_and_clone_strokes_rasterize_and_undo_brings_the_text_back() {
        let mut p = Painter::new(100, 100);
        let id = text_layer(&mut p);
        assert!(p.adjust_begin());
        p.filter_preview(
            crate::filters::Filter::from_parts("noise", 1.0, 0.0, 0.0, true, 1).unwrap(),
        );
        assert!(p.adjust_commit());
        assert_eq!(text_of(&p, id), "");
        p.undo();
        assert_eq!(text_of(&p, id), "A");

        let mut b = hokusai::Brush::new();
        b.get_mut(hokusai::BrushSetting::Radius).base_value = 2.0;
        b.get_mut(hokusai::BrushSetting::Opaque).base_value = 1.0;
        assert!(p.begin_clone_stroke(-40.0, -40.0, false, false));
        for i in 0..10 {
            p.stroke_to(&b, 42.0 + i as f32, 45.0, 1.0, 0.0, 0.0, 0.01);
        }
        p.end_stroke(&b);
        assert_eq!(text_of(&p, id), "");
        p.undo();
        assert_eq!(text_of(&p, id), "A");
    }

    #[test]
    fn text_off_the_canvas_is_cut() {
        let mut p = Painter::new(100, 100);
        p.text_begin(0, "");
        assert!(p.text_preview("A", -5, 95, 10, 10, &red(10, 10)));
        assert!(!p.text_preview("A", 0, 0, 10, 10, &red(3, 3)));
        p.text_commit();
        let id = p.active_layer();
        assert_eq!(p.layer_rgba(id)[(99 * 100 + 4) * 4 + 3], 255);
    }
}
