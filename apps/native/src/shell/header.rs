//! The header: the mark and the project's title, the File and View menus, the workspace
//! tabs, Undo and Redo, Draw together and Export (the design's FullApp header).

use pencilpilot_gpu::Quad;
use pencilpilot_ui::text::{Laid, Style};
use pencilpilot_ui::theme::{alpha, hex};
use pencilpilot_ui::ui::{Frame, Rect, id};

use crate::shell::engine::files;
use crate::state::{Action, State, Workspace};

/// The mark's pencil, Undo's and Redo's arrows.
const PENCIL: &str = "M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z";
const UNDO: &str = "M9 14 4 9l5-5 M4 9h11a5 5 0 0 1 0 10h-3";
const REDO: &str = "m15 14 5-5-5-5 M20 9H9a5 5 0 0 0 0 10h3";

const FILE_MENU: &str = "header/file";
const VIEW_MENU: &str = "header/view";

/// The File menu: (item, shortcut); an empty item is a separator. Each item is pushed as
/// [`Action::File`] with its text. The recent files go after Open… (each pushed as
/// [`files::OPEN_RECENT`] and its path).
pub const FILE_ITEMS: &[(&str, &str)] = &[
    ("New…", "Ctrl+N"),
    ("Open…", "Ctrl+O"),
    ("", ""),
    ("Save", "Ctrl+S"),
    ("Save as…", "Ctrl+Shift+S"),
    ("", ""),
    ("Import…", "Ctrl+I"),
    ("Export…", "Ctrl+E"),
    ("", ""),
    ("Project settings…", ""),
    ("", ""),
    ("Quit", "Ctrl+Q"),
];

/// The View menu (Onion skin flips [`State::onion`]; the rest are pushed as
/// [`Action::Other`]).
pub const VIEW_ITEMS: &[(&str, &str)] = &[
    ("Zoom in", "Ctrl++"),
    ("Zoom out", "Ctrl+−"),
    ("Fit to window", "0"),
    ("", ""),
    ("Rotate view", "R"),
    ("Flip view", "H"),
    ("Reset view", ""),
    ("", ""),
    ("Onion skin", ""),
    ("Full screen", "F11"),
];

// the design's colours that the theme doesn't name
const TABLIST: [f32; 4] = hex(0xEFECE4);
const PRESSED: [f32; 4] = hex(0xE4E0D6);
const SUBTLE: [f32; 4] = hex(0x5E5D63);
const TAB_OFF: [f32; 4] = hex(0x44434A);
const LILAC: [f32; 4] = hex(0xD9D2FF);
const DISABLED: [f32; 4] = hex(0xA9A8AE);

/// Where the header's parts were drawn in the last pass.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HeaderLayout {
    pub mark: Rect,
    pub file: Rect,
    pub view: Rect,
    pub tablist: Rect,
    /// in [`Workspace::ALL`] order
    pub tabs: [Rect; 4],
    pub undo: Rect,
    pub redo: Rect,
    pub together: Rect,
    pub export: Rect,
}

/// This piece's own state.
#[derive(Debug, Clone)]
pub struct HeaderState {
    /// the project's name, and where it is (scene, cut, saved or not)
    pub title: String,
    pub subtitle: String,
    /// whether the menu under a press was open at the press (clicking its button again
    /// then closes it rather than opening it again)
    open_at_press: bool,
    pub layout: HeaderLayout,
}

impl Default for HeaderState {
    fn default() -> Self {
        HeaderState {
            title: "Ember".into(),
            subtitle: "Scene 1 · Cut 2 · saved".into(),
            open_at_press: false,
            layout: HeaderLayout::default(),
        }
    }
}

pub fn build(f: &mut Frame, r: Rect, s: &mut State) {
    let t = f.theme();
    let cy = r.y + r.h / 2.0;
    let mut l = HeaderLayout::default();
    let label = |c| Style::body(14.0, c).weight(600);
    // the project's name, and where it is and whether it's saved
    s.header.title = s.project.name.clone();
    s.header.subtitle = s.project.subtitle(&s.cut_name);
    // the File menu's shortcuts (not while typing)
    if !f.typing() {
        let shift_s = f
            .keys()
            .iter()
            .any(|k| k.ctrl && k.shift && k.name.eq_ignore_ascii_case("s"));
        for (key, item) in [
            ("n", files::NEW),
            ("o", files::OPEN),
            ("s", if shift_s { files::SAVE_AS } else { files::SAVE }),
            ("q", files::QUIT),
        ] {
            if f.take_key(key, true) {
                s.actions.push(Action::File(item.into()));
            }
        }
    }

    // ---- left: the mark, the title, File and View ----
    l.mark = Rect::new(r.x, cy - 22.0, 44.0, 44.0);
    f.put(Quad::rect(l.mark.x, l.mark.y, 44.0, 44.0, t.coral).radius(14.0));
    f.icon(PENCIL, l.mark.x + 12.0, l.mark.y + 12.0, 20.0, t.on_coral);
    let title = f.lay_out(
        &s.header.title,
        &Style::display(20.0, t.ink).weight(800),
        None,
    );
    let sub = f.lay_out(
        &s.header.subtitle,
        &Style::body(12.0, SUBTLE).weight(600),
        None,
    );
    let tx = l.mark.right() + 12.0;
    let th = title.height + sub.height;
    f.put_text(&title, tx, cy - th / 2.0);
    f.put_text(&sub, tx, cy - th / 2.0 + title.height);
    let file_text = f.lay_out("File", &label(t.ink), None);
    let view_text = f.lay_out("View", &label(t.ink), None);
    l.file = Rect::new(
        tx + title.width.max(sub.width) + 24.0,
        cy - 20.0,
        file_text.width + 28.0,
        40.0,
    );
    l.view = Rect::new(
        l.file.right() + 4.0,
        cy - 20.0,
        view_text.width + 28.0,
        40.0,
    );
    menu_button(f, s, FILE_MENU, l.file, &file_text);
    menu_button(f, s, VIEW_MENU, l.view, &view_text);

    // ---- right: Undo, Redo, Draw together, Export ----
    let export_text = f.lay_out("Export", &label(t.on_coral), None);
    // (in a session: how many are here)
    let together = match &s.together.info {
        Some(i) => format!("Together · {}", i.members.len()),
        None => "Draw together".to_string(),
    };
    let together_text = f.lay_out(&together, &label(t.ink), None);
    let ew = export_text.width + 36.0;
    l.export = Rect::new(r.right() - ew, cy - 22.0, ew, 44.0);
    let tw = together_text.width + 32.0;
    l.together = Rect::new(l.export.x - 8.0 - tw, cy - 22.0, tw, 44.0);
    l.redo = Rect::new(l.together.x - 8.0 - 44.0, cy - 22.0, 44.0, 44.0);
    l.undo = Rect::new(l.redo.x - 8.0 - 44.0, cy - 22.0, 44.0, 44.0);
    if icon_button(f, "header/undo", l.undo, UNDO, "Undo (Ctrl+Z)", s.can_undo) {
        s.actions.push(Action::Undo);
    }
    let redo = "Redo (Ctrl+Shift+Z)";
    if icon_button(f, "header/redo", l.redo, REDO, redo, s.can_redo) {
        s.actions.push(Action::Redo);
    }
    if text_button(f, "header/together", l.together, &together_text, LILAC) {
        s.actions.push(Action::Other("Draw together".into()));
    }
    if text_button(f, "header/export", l.export, &export_text, t.coral) {
        s.actions.push(Action::File("Export…".into()));
    }

    // ---- the workspace tabs, midway between the two (the design's space-between) ----
    let widths: Vec<f32> = Workspace::ALL
        .iter()
        .map(|w| f.lay_out(w.name(), &label(t.ink), None).width + 32.0)
        .collect();
    let total = widths.iter().sum::<f32>() + 3.0 * 4.0 + 8.0;
    let free = (l.undo.x - l.view.right() - total).max(0.0);
    l.tablist = Rect::new(l.view.right() + free / 2.0, cy - 24.0, total, 48.0);
    let tl = l.tablist;
    f.put(Quad::rect(tl.x, tl.y, tl.w, tl.h, TABLIST).radius(16.0));
    let mut x = tl.x + 4.0;
    for (i, w) in Workspace::ALL.into_iter().enumerate() {
        let tab = Rect::new(x, tl.y + 4.0, widths[i], 40.0);
        l.tabs[i] = tab;
        x += widths[i] + 4.0;
        let res = f.interact(id(&format!("header/tab/{i}")), tab);
        if res.clicked {
            s.workspace = w;
        }
        let (bg, fg) = if s.workspace == w {
            (t.ink, t.on_dark)
        } else if res.held {
            (alpha(t.card, 0.9), TAB_OFF)
        } else if res.hovered {
            (alpha(t.card, 0.6), TAB_OFF)
        } else {
            (alpha(t.card, 0.0), TAB_OFF)
        };
        f.put(Quad::rect(tab.x, tab.y, tab.w, tab.h, bg).radius(12.0));
        let text = f.lay_out(w.name(), &label(fg), None);
        f.put_text(
            &text,
            tab.x + (tab.w - text.width) / 2.0,
            tab.y + (tab.h - text.height) / 2.0,
        );
    }

    // ---- the menus (overlays) ----
    let recent = s.project.recent.clone();
    let mut recent_picked = Vec::new();
    let picked = f.menu(FILE_MENU, |m| {
        for (text, hint) in FILE_ITEMS {
            if text.is_empty() {
                m.separator();
            } else {
                m.item(text, hint);
            }
            // the recent files, after Open…
            if *text == "Open…" && !recent.is_empty() {
                m.separator();
                m.head("Recent");
                for p in &recent {
                    let name = p
                        .file_stem()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    let dir = p
                        .parent()
                        .and_then(|d| d.file_name())
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    recent_picked.push((m.item(&name, &dir), p.clone()));
                }
            }
        }
    });
    if let Some((_, p)) = recent_picked.iter().find(|(k, _)| k.get()) {
        s.actions.push(Action::File(format!(
            "{}{}",
            files::OPEN_RECENT,
            p.to_string_lossy()
        )));
    } else if let Some(item) = picked {
        s.actions.push(Action::File(item));
    }
    let onion = s.onion;
    let picked = f.menu(VIEW_MENU, |m| {
        for (text, hint) in VIEW_ITEMS {
            if text.is_empty() {
                m.separator();
            } else if *text == "Onion skin" {
                m.item(text, if onion { "✓" } else { "" });
            } else {
                m.item(text, hint);
            }
        }
    });
    match picked.as_deref() {
        Some("Onion skin") => s.onion = !s.onion,
        Some(item) => s.actions.push(Action::Other(item.into())),
        None => {}
    }
    s.header.layout = l;
    // "Save first?" (New or Open with unsaved changes), over the window
    files::popup::build(f, Rect::new(r.x, r.y, r.w, r.w * 0.6), s);
}

/// A text button in the nav (File, View): it opens its menu under it, and closes it when
/// clicked again.
fn menu_button(f: &mut Frame, s: &mut State, menu: &str, rect: Rect, text: &Laid) {
    let t = f.theme();
    let res = f.interact(id(&format!("{menu}/button")), rect);
    if res.hovered && f.pointer().pressed {
        s.header.open_at_press = f.is_open(menu);
    }
    if res.clicked && !s.header.open_at_press {
        f.open(menu, rect.x, rect.bottom() + 6.0);
    }
    let bg = if f.is_open(menu) || res.held {
        PRESSED
    } else if res.hovered {
        TABLIST
    } else {
        alpha(t.card, 0.0)
    };
    f.put(Quad::rect(rect.x, rect.y, rect.w, rect.h, bg).radius(12.0));
    put_centred(f, text, rect);
}

/// A 44 × 44 white button with an icon (Undo, Redo): true when clicked while enabled.
fn icon_button(
    f: &mut Frame,
    name: &str,
    rect: Rect,
    path: &str,
    tip: &str,
    enabled: bool,
) -> bool {
    let t = f.theme();
    let res = f.interact(id(name), rect);
    let bg = match (enabled, res.held, res.hovered) {
        (true, true, _) => PRESSED,
        (true, false, true) => TABLIST,
        _ => t.card,
    };
    f.put(Quad::rect(rect.x, rect.y, rect.w, rect.h, bg).radius(14.0));
    let fg = if enabled { t.ink } else { DISABLED };
    f.icon(path, rect.x + 12.0, rect.y + 12.0, 20.0, fg);
    if res.hovered {
        f.tooltip(id(name), tip, rect);
    }
    enabled && res.clicked
}

/// A 44-tall filled button with text (Draw together, Export): true when clicked.
fn text_button(f: &mut Frame, name: &str, rect: Rect, text: &Laid, fill: [f32; 4]) -> bool {
    let t = f.theme();
    let res = f.interact(id(name), rect);
    let bg = if res.held {
        mix(fill, t.ink, 0.15)
    } else if res.hovered {
        mix(fill, t.ink, 0.07)
    } else {
        fill
    };
    f.put(Quad::rect(rect.x, rect.y, rect.w, rect.h, bg).radius(14.0));
    put_centred(f, text, rect);
    res.clicked
}

fn put_centred(f: &mut Frame, text: &Laid, rect: Rect) {
    f.put_text(
        text,
        rect.x + (rect.w - text.width) / 2.0,
        rect.y + (rect.h - text.height) / 2.0,
    );
}

pub(crate) fn mix(a: [f32; 4], b: [f32; 4], k: f32) -> [f32; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * k)
}

/// Drives the header and the tool column with simulated input, as the app would (for this
/// file's tests and the tool column's).
#[cfg(test)]
pub(crate) mod harness {
    use pencilpilot_gpu::{DmabufImage, Error, Gpu, Quad, Renderer2d};
    use pencilpilot_ui::ui::{Key, Rect, Ui};

    use crate::shell::layout;
    use crate::state::State;

    pub const W: f32 = 1440.0;
    pub const H: f32 = 900.0;

    /// (the renderer goes before the GPU it was made on: fields drop in order)
    pub struct Harness {
        pub r: Renderer2d,
        pub ui: Ui,
        pub s: State,
        pub gpu: Gpu,
    }

    impl Harness {
        /// None (and a note) without Vulkan.
        pub fn new() -> Option<Harness> {
            let gpu = match Gpu::new() {
                Ok(g) => g,
                Err(e @ (Error::NoLoader(_) | Error::NoDevice(_))) => {
                    eprintln!("skipped: {e}");
                    return None;
                }
                Err(e) => panic!("{e}"),
            };
            let r = Renderer2d::new(&gpu).unwrap();
            let mut ui = Ui::new(1.0);
            ui.text.wait_for_fonts();
            Some(Harness {
                r,
                ui,
                s: State::demo(),
                gpu,
            })
        }

        /// One pass of the header and the tool column (where the workspace has one).
        pub fn pass(&mut self) -> Option<Vec<Quad>> {
            let s = &mut self.s;
            self.ui.pass(&mut self.r, W, H, |f| {
                let reg = layout::regions(W, H, s.workspace);
                super::build(f, reg.header, s);
                if let Some(t) = reg.tools {
                    crate::shell::tools::build(f, t, s);
                }
            })
        }

        pub fn click(&mut self, x: f32, y: f32) {
            self.ui.pointer_moved(x, y);
            self.pass();
            self.ui.pointer_button(true);
            self.pass();
            self.ui.pointer_button(false);
            self.pass();
        }

        pub fn click_in(&mut self, r: Rect) {
            self.click(r.x + r.w / 2.0, r.y + r.h / 2.0);
        }

        pub fn right_click(&mut self, x: f32, y: f32) {
            self.ui.pointer_moved(x, y);
            self.pass();
            self.ui.pointer_context();
            self.pass();
            // the first pass with the menu open marks its area
            self.pass();
        }

        pub fn drag(&mut self, from: (f32, f32), to: (f32, f32)) {
            self.ui.pointer_moved(from.0, from.1);
            self.pass();
            self.ui.pointer_button(true);
            self.pass();
            self.ui.pointer_moved(to.0, to.1);
            self.pass();
            self.ui.pointer_button(false);
            self.pass();
        }

        pub fn key(&mut self, name: &str, text: Option<&str>) {
            self.ui.key(Key {
                name: name.into(),
                text: text.map(Into::into),
                ctrl: false,
                shift: false,
            });
        }

        /// Settled: a pass with no input draws nothing.
        pub fn idle(&mut self) -> bool {
            self.pass();
            self.pass().is_none()
        }

        /// What's on screen, as PNG in the `PP_SAVE` folder (if set).
        pub fn save(&mut self, name: &str) {
            let Ok(dir) = std::env::var("PP_SAVE") else {
                return;
            };
            let frame = DmabufImage::new(&self.gpu, W as u32, H as u32, &[]).unwrap();
            self.r
                .draw(&self.gpu, &frame, self.ui.theme.paper, self.ui.shown())
                .unwrap();
            let px = self.gpu.read_back(&frame).unwrap();
            let rgba: Vec<u8> = px.chunks(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
            let file = std::fs::File::create(format!("{dir}/{name}.png")).unwrap();
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W as u32, H as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&rgba).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::harness::Harness;
    use super::*;

    /// The y of a menu item's middle: the menu opens at `top`; padding 6, items 32 tall,
    /// separators 9 (the toolkit's menu).
    fn item_y(top: f32, items: &[(&str, &str)], text: &str) -> f32 {
        let mut y = top + 6.0;
        for (t, _) in items {
            if t.is_empty() {
                y += 9.0;
            } else if *t == text {
                return y + 16.0;
            } else {
                y += 32.0;
            }
        }
        panic!("no item {text}");
    }

    #[test]
    fn header_lays_out_as_the_design() {
        let Some(mut h) = Harness::new() else { return };
        assert!(h.pass().is_some(), "the first pass draws");
        let l = h.s.header.layout;
        assert_eq!((l.mark.w, l.mark.h), (44.0, 44.0));
        assert_eq!((l.undo.w, l.undo.h, l.redo.w), (44.0, 44.0, 44.0));
        assert_eq!(l.tablist.h, 48.0);
        assert!(l.tabs.iter().all(|t| t.h == 40.0));
        assert!(l.file.right() < l.view.x && l.view.right() < l.tablist.x);
        assert!(l.tablist.right() < l.undo.x && l.undo.right() < l.redo.x);
        assert!(l.redo.right() < l.together.x && l.together.right() < l.export.x);
        assert!((l.export.right() - W_RIGHT).abs() < 0.01);
        assert!(h.idle(), "idle: nothing drawn");
    }
    const W_RIGHT: f32 = super::harness::W - 16.0;

    #[test]
    fn tabs_switch_the_workspace() {
        let Some(mut h) = Harness::new() else { return };
        h.pass();
        for (i, w) in Workspace::ALL.into_iter().enumerate().rev() {
            let tab = h.s.header.layout.tabs[i];
            h.click_in(tab);
            assert_eq!(h.s.workspace, w, "the {} tab picks it", w.name());
        }
        assert!(h.idle());
    }

    #[test]
    fn undo_and_redo_push_actions_when_they_can() {
        let Some(mut h) = Harness::new() else { return };
        h.s.can_undo = true;
        h.pass();
        let l = h.s.header.layout;
        h.click_in(l.undo);
        assert_eq!(h.s.actions, vec![Action::Undo]);
        h.s.actions.clear();
        // nothing to redo: Redo does nothing
        h.click_in(l.redo);
        assert!(h.s.actions.is_empty(), "{:?}", h.s.actions);
        h.s.can_redo = true;
        h.click_in(l.redo);
        assert_eq!(h.s.actions, vec![Action::Redo]);
        h.s.actions.clear();
        h.click_in(l.export);
        assert_eq!(h.s.actions, vec![Action::File("Export…".into())]);
        assert!(h.idle());
    }

    fn ctrl(h: &mut Harness, name: &str, shift: bool) {
        h.ui.key(pencilpilot_ui::ui::Key {
            name: name.into(),
            text: None,
            ctrl: true,
            shift,
        });
        h.pass();
    }

    /// The title and the line under it follow the project; Ctrl+N, Ctrl+O, Ctrl+S and
    /// Ctrl+Shift+S push the File menu's items; the recent files are in the menu.
    #[test]
    fn the_project_in_the_header_and_its_shortcuts() {
        let Some(mut h) = Harness::new() else { return };
        h.s.project.name = "Fox chase".into();
        h.s.project.unsaved = true;
        h.pass();
        assert_eq!(h.s.header.title, "Fox chase");
        assert_eq!(h.s.header.subtitle, "Scene 1 · Cut 2 · unsaved");
        h.s.project.unsaved = false;
        h.pass();
        assert_eq!(h.s.header.subtitle, "Scene 1 · Cut 2 · saved");
        for (k, shift, item) in [
            ("n", false, files::NEW),
            ("o", false, files::OPEN),
            ("s", false, files::SAVE),
            ("S", true, files::SAVE_AS),
            ("q", false, files::QUIT),
        ] {
            ctrl(&mut h, k, shift);
            assert_eq!(h.s.actions, vec![Action::File(item.into())], "{k}");
            h.s.actions.clear();
        }
        // the recent files, under Open…
        h.s.project.recent = vec![
            "/home/a/films/fox.ppstudio".into(),
            "/home/a/old/crow.ppstudio".into(),
        ];
        let file = h.s.header.layout.file;
        h.click_in(file);
        h.pass();
        h.save("header-file-menu-recent");
        let top = file.bottom() + 6.0;
        // New, Open…, a separator, the "Recent" head, then the files
        let crow = top + 6.0 + 32.0 * 2.0 + 9.0 + 26.0 + 32.0 + 16.0;
        h.click(file.x + 60.0, crow);
        assert_eq!(
            h.s.actions,
            vec![Action::File(format!(
                "{}/home/a/old/crow.ppstudio",
                files::OPEN_RECENT
            ))]
        );
        h.s.actions.clear();
        // and the items after them where they were, moved down
        h.click_in(file);
        h.pass();
        let save = item_y(top, FILE_ITEMS, "Save") + 9.0 + 26.0 + 64.0;
        h.click(file.x + 60.0, save);
        assert_eq!(h.s.actions, vec![Action::File("Save".into())]);
        assert!(h.idle());
    }

    fn save_first() -> files::Question {
        use pencilpilot_ui::ui::Kind;
        files::Question {
            title: "Save the changes to Ember first?".into(),
            detail: "Your changes are lost if you don’t save them.".into(),
            buttons: vec![
                ("Save".into(), files::CONFIRM_SAVE.into(), Kind::Primary),
                (
                    "Don’t save".into(),
                    files::CONFIRM_DISCARD.into(),
                    Kind::Secondary,
                ),
                ("Cancel".into(), files::CONFIRM_CANCEL.into(), Kind::Ghost),
            ],
            dismiss: files::CONFIRM_CANCEL.into(),
        }
    }

    /// "Save first?": Escape or a click outside cancels; its buttons answer.
    #[test]
    fn save_first_asks_and_answers() {
        let Some(mut h) = Harness::new() else { return };
        h.s.project.asking = Some(save_first());
        h.pass();
        h.pass();
        assert!(h.ui.overlay_open(), "the pop-up shows");
        h.save("header-save-first");
        h.key("Escape", None);
        h.pass();
        assert_eq!(
            h.s.actions,
            vec![Action::File(files::CONFIRM_CANCEL.into())]
        );
        h.s.actions.clear();
        // (the engine clears the question; ask again) a click outside cancels too
        h.s.project.asking = None;
        h.pass();
        h.s.project.asking = Some(save_first());
        h.pass();
        h.pass();
        assert!(h.ui.overlay_open());
        h.click(20.0, 880.0);
        h.pass();
        assert_eq!(
            h.s.actions,
            vec![Action::File(files::CONFIRM_CANCEL.into())]
        );
        h.s.actions.clear();
        h.s.project.asking = None;
        h.pass();
        // the buttons: Save, Don't save, Cancel along the bottom of the card
        for (i, answer) in [
            files::CONFIRM_SAVE,
            files::CONFIRM_DISCARD,
            files::CONFIRM_CANCEL,
        ]
        .into_iter()
        .enumerate()
        {
            h.s.project.asking = Some(save_first());
            h.pass();
            h.pass();
            let r = h.s.project.ask_buttons[i];
            h.click_in(r);
            assert_eq!(h.s.actions, vec![Action::File(answer.into())], "button {i}");
            assert!(!h.ui.overlay_open());
            h.s.actions.clear();
            h.s.project.asking = None;
            h.pass();
        }
        assert!(h.idle());
    }

    #[test]
    fn the_file_menu_opens_picks_and_closes() {
        let Some(mut h) = Harness::new() else { return };
        h.pass();
        let file = h.s.header.layout.file;
        h.click_in(file);
        assert!(h.ui.overlay_open(), "File opened its menu");
        h.pass();
        h.save("header-file-menu");
        let top = file.bottom() + 6.0;
        h.click(file.x + 60.0, item_y(top, FILE_ITEMS, "Save"));
        assert_eq!(h.s.actions, vec![Action::File("Save".into())]);
        assert!(!h.ui.overlay_open(), "picking closed it");
        h.s.actions.clear();

        // File again closes it; a click outside does too, and reaches nothing
        h.click_in(file);
        assert!(h.ui.overlay_open());
        h.click_in(file);
        assert!(!h.ui.overlay_open(), "File clicked again closed it");
        h.click_in(file);
        h.click(700.0, 600.0);
        assert!(!h.ui.overlay_open());
        assert!(h.s.actions.is_empty());

        // View: Onion skin flips it
        let view = h.s.header.layout.view;
        let onion = h.s.onion;
        h.click_in(view);
        h.pass();
        h.click(
            view.x + 60.0,
            item_y(view.bottom() + 6.0, VIEW_ITEMS, "Onion skin"),
        );
        assert_eq!(h.s.onion, !onion);
        assert!(h.idle());
    }
}
