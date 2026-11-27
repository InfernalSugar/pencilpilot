//! The tools through the whole shell: clicks, right-clicks and keys as the window gives
//! them (presses on the canvas go to the engine, the rest to the interface), and pictures
//! of it (set `PP_SAVE` to a folder to keep them).

use pencilpilot_gpu::{Canvas, DmabufImage, Error, Gpu, Renderer2d};
use pencilpilot_ui::ui::{Key, Rect, Ui};

use super::overlay::{SELECTION_MENU, SETTINGS};
use super::*;
use crate::shell::engine::{Engine, Phase, Sample};
use crate::state::{State, Tool, Workspace};

const W: f32 = 1440.0;
const H: f32 = 900.0;

struct Shell {
    r: Renderer2d,
    ui: Ui,
    s: State,
    e: Engine,
    canvas: Rect,
    /// the input's clock, ms
    ms: std::cell::Cell<u32>,
    /// the pictures drawn, kept as the window keeps them (made once; the moving pixels'
    /// once per lift)
    doc: Option<Canvas>,
    moving: Option<(Canvas, u64)>,
    gpu: Gpu,
}

impl Shell {
    fn new(ws: Workspace) -> Option<Shell> {
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
        let mut s = State::demo();
        s.workspace = ws;
        let e = Engine::new(s.view.doc_w, s.view.doc_h);
        let mut sh = Shell {
            r,
            ui,
            s,
            e,
            canvas: Rect::default(),
            ms: std::cell::Cell::new(0),
            doc: None,
            moving: None,
            gpu,
        };
        sh.pass();
        sh.s.view.reset(sh.canvas);
        sh.pass();
        Some(sh)
    }

    /// A pass of the whole shell, then what it asked for, as the window does.
    fn pass(&mut self) {
        self.follow_float();
        let (s, canvas) = (&mut self.s, &mut self.canvas);
        self.ui.pass(&mut self.r, W, H, |f| {
            *canvas = crate::shell::build(f, s, W, H).1;
        });
        for a in std::mem::take(&mut self.s.actions) {
            self.e.apply(&mut self.s, &a);
        }
        self.follow_float();
    }

    /// The pixels being moved, where the engine has them (as the window does before a pass).
    fn follow_float(&mut self) {
        self.s.moving = self.e.float.as_ref().map(|f| crate::shell::view::Moving {
            at: f.at,
            w: f.w,
            h: f.h,
            m: f.m,
        });
    }

    fn screen(&self, d: (f32, f32)) -> (f32, f32) {
        self.s.view.to_screen(self.canvas, d.0, d.1)
    }

    fn sample(&self, d: (f32, f32)) -> Sample {
        Sample {
            x: d.0,
            y: d.1,
            pressure: 0.5,
            tilt: (0.0, 0.0),
            ms: {
                self.ms.set(self.ms.get() + 8);
                self.ms.get()
            },
        }
    }

    /// A press and drag on the canvas (document points), as the window routes it.
    fn canvas_drag(&mut self, pts: &[(f32, f32)]) {
        let n = pts.len();
        for (i, p) in pts.iter().enumerate() {
            let phase = match i {
                0 => Phase::Down,
                _ if i == n - 1 => Phase::Up,
                _ => Phase::Move,
            };
            let smp = self.sample(*p);
            self.e.pointer(&mut self.s, phase, smp);
            self.pass();
        }
    }

    fn click(&mut self, p: (f32, f32)) {
        self.ui.pointer_moved(p.0, p.1);
        self.pass();
        self.ui.pointer_button(true);
        self.pass();
        self.ui.pointer_button(false);
        self.pass();
    }

    fn right_click(&mut self, p: (f32, f32)) {
        self.ui.pointer_moved(p.0, p.1);
        self.pass();
        self.ui.pointer_context();
        self.pass();
        self.pass();
    }

    fn key(&mut self, name: &str, text: Option<&str>, ctrl: bool, shift: bool) {
        self.ui.key(Key {
            name: name.into(),
            text: text.map(Into::into),
            ctrl,
            shift,
        });
        self.pass();
    }

    fn typed(&mut self, text: &str) {
        for c in text.chars() {
            let t = c.to_string();
            self.key(&t, Some(&t), false, false);
        }
    }

    /// The menu item `item`'s middle, in the menu open at `at` (each 32 tall after a
    /// 26 tall head, 6 in; separators 9).
    fn menu_item(at: (f32, f32), items: &[&str], item: &str) -> (f32, f32) {
        let mut y = at.1 + 6.0 + 26.0;
        for it in items {
            if *it == item {
                return (at.0 + 40.0, y + 16.0);
            }
            y += if *it == "-" { 9.0 } else { 32.0 };
        }
        panic!("no {item}");
    }

    /// What's on screen, with the drawing (and pixels being moved), as PNG in `PP_SAVE`.
    fn save(&mut self, name: &str) {
        let Ok(dir) = std::env::var("PP_SAVE") else {
            return;
        };
        // what the last actions changed shows in the next pass, as in the window
        self.pass();
        let (dw, dh) = (self.s.view.doc_w, self.s.view.doc_h);
        if self.doc.is_none() {
            self.doc = Some(Canvas::new(&self.gpu, dw, dh).unwrap());
        }
        self.e.painter.flush();
        if let Some(c) = &mut self.doc {
            c.upload_all(self.e.painter.rgba());
        }
        match (&self.e.float, &self.moving) {
            (Some(f), Some((_, lift))) if *lift == f.lift => {}
            (Some(f), _) => {
                let mut c = Canvas::new(&self.gpu, f.w.max(1), f.h.max(1)).unwrap();
                c.upload_all(&f.rgba);
                self.moving = Some((c, f.lift));
            }
            (None, _) => self.moving = None,
        }
        if std::env::var("PP_SAVE_DOC").is_ok() {
            let f = std::fs::File::create(format!("{dir}/{name}-doc.png")).unwrap();
            let mut enc = png::Encoder::new(std::io::BufWriter::new(f), dw, dh);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()
                .unwrap()
                .write_image_data(self.e.painter.rgba())
                .unwrap();
        }
        let frame = DmabufImage::new(&self.gpu, W as u32, H as u32, &[]).unwrap();
        self.r
            .draw_canvases(
                &self.gpu,
                &frame,
                self.ui.theme.paper,
                self.ui.shown(),
                self.doc.as_mut(),
                self.moving.as_mut().map(|(c, _)| c),
            )
            .unwrap();
        let px = self.gpu.read_back(&frame).unwrap();
        let rgba: Vec<u8> = px.chunks(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
        let file = std::fs::File::create(format!("{dir}/{name}.png")).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W as u32, H as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    }

    /// Paints a rough figure with the brush (strokes through the engine).
    fn scribble(&mut self, cx: f32, cy: f32, r: f32) {
        let tool = self.s.tool;
        self.s.tool = Tool::Brush;
        self.s.brush = 3; // Marker
        self.s.brush_size = 24.0;
        for k in 0..6 {
            let rr = r * (1.0 - k as f32 * 0.14);
            let pts: Vec<(f32, f32)> = (0..=120)
                .map(|i| {
                    let a = i as f32 / 120.0 * std::f32::consts::TAU;
                    (cx + rr * a.cos(), cy + rr * 0.7 * a.sin())
                })
                .collect();
            self.canvas_drag(&pts);
        }
        self.s.tool = tool;
    }
}

fn painted(e: &Engine) -> usize {
    e.painter
        .layer_rgba(e.painter.active_layer())
        .chunks(4)
        .filter(|p| p[3] > 128)
        .count()
}

#[test]
fn a_right_click_in_the_selection_opens_its_menu_and_its_items_work() {
    let Some(mut sh) = Shell::new(Workspace::Draw) else {
        return;
    };
    sh.scribble(700.0, 500.0, 220.0);
    sh.s.tool = Tool::Select;
    sh.canvas_drag(&[(440.0, 300.0), (700.0, 500.0), (960.0, 700.0)]);
    assert!(sh.e.painter.has_selection());
    sh.save("tools-selection");
    // inside: the selection's menu
    let inside = sh.screen((700.0, 500.0));
    sh.right_click(inside);
    assert!(sh.ui.overlay_open());
    sh.save("tools-selection-menu");
    let items = [
        "Cut",
        "Copy",
        "-",
        "Transform",
        "Flip horizontally",
        "Flip vertically",
        "-",
        "Fill selection",
        "Clear",
    ];
    let before = painted(&sh.e);
    sh.click(Shell::menu_item(inside, &items, "Clear"));
    assert!(!sh.ui.overlay_open());
    assert!(painted(&sh.e) < before, "Clear took the selected paint away");
    // Ctrl+Z (the shell's) brings it back
    sh.e.apply(&mut sh.s, &Action::Undo);
    assert_eq!(painted(&sh.e), before);
    // outside: the canvas's menu, whose Symmetry turns it on
    let outside = sh.screen((150.0, 150.0));
    sh.right_click(outside);
    let canvas_items = [
        "Paste",
        "Select all",
        "-",
        "Guides",
        "Symmetry",
        "Snap to guides",
    ];
    sh.click(Shell::menu_item(outside, &canvas_items, "Symmetry"));
    assert_eq!(sh.s.tooling.symmetry, Sym::Vertical);
    sh.right_click(outside);
    sh.click(Shell::menu_item(outside, &canvas_items, "Guides"));
    assert!(sh.s.tooling.grid);
    sh.save("tools-grid-symmetry");
    let _ = SELECTION_MENU;
}

#[test]
fn keys_select_copy_paste_clear_and_transform() {
    let Some(mut sh) = Shell::new(Workspace::Draw) else {
        return;
    };
    sh.scribble(500.0, 400.0, 150.0);
    let before = painted(&sh.e);
    sh.key("a", Some("a"), true, false);
    assert!(sh.e.painter.has_selection(), "Ctrl+A selects all");
    sh.key("c", Some("c"), true, false);
    assert!(sh.s.tooling.has_clip, "Ctrl+C copies");
    sh.key("Delete", None, false, false);
    assert_eq!(painted(&sh.e), 0, "Delete clears");
    sh.key("v", Some("v"), true, true);
    assert_eq!(painted(&sh.e), before, "Ctrl+Shift+V pastes in place");
    assert_ne!(sh.s.tool, Tool::Transform);
    sh.key("Escape", None, false, false);
    assert!(!sh.e.painter.has_selection(), "Escape deselects");
    // V picks Transform, which lifts the whole layer; a drag moves it on the GPU
    sh.key("v", Some("v"), false, false);
    assert_eq!(sh.s.tool, Tool::Transform);
    assert!(sh.e.float.is_some() && sh.s.tooling.transform.is_some());
    sh.canvas_drag(&[(500.0, 400.0), (650.0, 450.0), (800.0, 500.0)]);
    assert!(sh.s.moving.is_some());
    // the box's handles are drawn, and the lifted pixels move with it
    sh.save("tools-transform-moving");
    sh.key("Return", None, false, false);
    assert!(sh.e.float.is_none() && sh.s.tooling.transform.is_none());
    assert_eq!(sh.s.tool, Tool::Transform);
    sh.save("tools-transform-kept");
    // U picks Shape, again changes the kind
    sh.key("u", Some("u"), false, false);
    assert_eq!(sh.s.tool, Tool::Shape);
    sh.key("u", Some("u"), false, false);
    assert_eq!(sh.s.tooling.shape_kind, ShapeKind::Rectangle);
    sh.canvas_drag(&[(1200.0, 200.0), (1500.0, 350.0), (1700.0, 500.0)]);
    sh.key("u", Some("u"), false, false);
    sh.canvas_drag(&[(1200.0, 600.0), (1500.0, 800.0)]);
    let (a, b) = (sh.sample((1200.0, 900.0)), sh.sample((1700.0, 1000.0)));
    sh.e.pointer(&mut sh.s, Phase::Down, a);
    sh.e.pointer(&mut sh.s, Phase::Move, b);
    sh.pass();
    assert!(sh.s.tooling.shape.is_some());
    sh.save("tools-shapes");
    sh.e.pointer(&mut sh.s, Phase::Up, b);
}

#[test]
fn text_is_typed_with_the_keys_tool_letters_included() {
    let Some(mut sh) = Shell::new(Workspace::Draw) else {
        return;
    };
    sh.s.tool = Tool::Text;
    sh.canvas_drag(&[(300.0, 300.0), (300.0, 300.0)]);
    assert!(sh.s.tooling.text.is_some());
    // "b", "e", "i", "v"… are tools' keys: while typing they're letters
    sh.typed("Bevel, mind it");
    assert_eq!(sh.s.tooling.text.as_ref().unwrap().text, "Bevel, mind it");
    assert_eq!(sh.s.tool, Tool::Text);
    sh.key("BackSpace", None, false, false);
    sh.key("Return", None, false, true);
    sh.typed("Hi");
    assert_eq!(sh.s.tooling.text.as_ref().unwrap().text, "Bevel, mind i\nHi");
    assert!(painted(&sh.e) > 1000);
    sh.save("tools-text-typing");
    sh.key("Return", None, false, false);
    assert!(sh.s.tooling.text.is_none());
    let kept = painted(&sh.e);
    assert!(kept > 1000);
    // the keys are tools' keys again
    sh.key("b", Some("b"), false, false);
    assert_eq!(sh.s.tool, Tool::Brush);
    sh.e.apply(&mut sh.s, &Action::Undo);
    assert_eq!(painted(&sh.e), 0, "the text was one undo step");
}

#[test]
fn the_pills_button_opens_the_brush_settings_and_they_change_the_brush() {
    let Some(mut sh) = Shell::new(Workspace::Draw) else {
        return;
    };
    // the pill's button: 32 px, 8 in from the pill's right; find it by its tooltip area
    let r = crate::shell::layout::regions(W, H, Workspace::Draw).canvas;
    // press along the pill's row until the pop-up opens
    let mut opened = false;
    for dx in (100..420).step_by(8) {
        sh.click((r.x + 16.0 + dx as f32, r.y + 14.0 + 22.0));
        if sh.ui.overlay_open() {
            opened = true;
            break;
        }
    }
    assert!(opened, "the pill's settings button opens the pop-up");
    sh.save("tools-brush-settings");
    // its opacity slider (the second): drag it to the left end
    let x = r.x + 16.0 + 16.0;
    let pop_y = r.y + 66.0;
    // heading, Size label+slider, Opacity label+slider (the toolkit's column, 18 apart)
    let mut found = false;
    for y in (0..240).step_by(4) {
        let p = (x + 2.0, pop_y + y as f32);
        let before = sh.s.panel.opacity;
        sh.ui.pointer_moved(p.0, p.1);
        sh.pass();
        sh.ui.pointer_button(true);
        sh.pass();
        sh.ui.pointer_button(false);
        sh.pass();
        if sh.s.panel.opacity != before {
            found = true;
            break;
        }
        if sh.s.brush_size != 6.0 {
            // that was the size slider
            sh.s.brush_size = 6.0;
        }
    }
    assert!(found, "a slider in the pop-up sets the opacity");
    assert!(sh.s.panel.opacity < 10.0, "{}", sh.s.panel.opacity);
    let _ = SETTINGS;
    // the brush then paints see-through
    let mut b = Engine::brush(&sh.s).unwrap();
    tune_brush(&mut b, &sh.s);
    assert!(b.get(hokusai::BrushSetting::OpaqueMultiply).base_value < 0.1);
}
