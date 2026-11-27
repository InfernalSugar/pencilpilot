//! Menus, right-click menus, text fields, scrolling, icons and tooltips with simulated
//! input. Skips without Vulkan. `PP_SAVE` = a folder for the picture.

#![cfg(target_os = "linux")]

use pencilpilot_gpu::{DmabufImage, Error, Gpu, Renderer2d};
use pencilpilot_ui::ui::{Frame, Key, Kind, Rect, Ui};

const PLAY: &str = "M7 4v16l13-8Z";
const ONION: &str = "M10 5a7 7 0 1 0 0.01 0Z M16 5a7 7 0 1 0 0.01 0Z";

#[derive(Default)]
struct App {
    locked: bool,
    under: u32,
    name: String,
    picked_row: Option<usize>,
    played: u32,
}

fn key(name: &str, text: Option<&str>) -> Key {
    Key {
        name: name.into(),
        text: text.map(Into::into),
        ctrl: false,
        shift: false,
    }
}

fn panel(f: &mut Frame, app: &mut App) {
    f.card(Rect::new(20.0, 20.0, 560.0, 440.0), |f| {
        f.heading("Layers");
        // a layer row with a right-click menu
        let row = f.alloc(0.0, 36.0);
        f.context_area("layer", row);
        f.row(44.0, |f| {
            if f.button("under", "Under the menu", Kind::Secondary).clicked {
                app.under += 1;
            }
            f.then_right();
            if f.icon_button("play", PLAY, "Play", false, false).clicked {
                app.played += 1;
            }
            f.icon_button("onion", ONION, "Onion skin", true, false);
        });
        f.text_field("name", &mut app.name, "Name this cut", 260.0);
        f.scroll_area("list", 120.0, 10.0 * 44.0 + 9.0 * 8.0, |f| {
            for i in 0..10 {
                if f.button(
                    &format!("row{i}"),
                    &format!("Drawing {}", i + 1),
                    Kind::Ghost,
                )
                .clicked
                {
                    app.picked_row = Some(i);
                }
            }
        });
    });
    let picked = f.menu("layer", |m| {
        m.head("Clean");
        m.item("Lock layer", "");
        m.item("Show as outlines", "");
        m.separator();
        m.item("All on twos", "2s");
    });
    if picked.as_deref() == Some("Lock layer") {
        app.locked = true;
    }
}

#[test]
fn overlays_take_the_pointer_and_close_as_they_should() {
    let gpu = match Gpu::new() {
        Ok(g) => g,
        Err(e @ (Error::NoLoader(_) | Error::NoDevice(_))) => return eprintln!("skipped: {e}"),
        Err(e) => panic!("{e}"),
    };
    let mut r = Renderer2d::new(&gpu).unwrap();
    let mut ui = Ui::new(1.0);
    ui.text.wait_for_fonts();
    let mut app = App::default();
    let (w, h) = (600.0, 480.0);
    let mut pass = |ui: &mut Ui, app: &mut App| ui.pass(&mut r, w, h, |f| panel(f, app));
    let click = |ui: &mut Ui,
                 app: &mut App,
                 pass: &mut dyn FnMut(&mut Ui, &mut App) -> Option<Vec<pencilpilot_gpu::Quad>>,
                 x: f32,
                 y: f32| {
        ui.pointer_moved(x, y);
        pass(ui, app);
        ui.pointer_button(true);
        pass(ui, app);
        ui.pointer_button(false);
        pass(ui, app)
    };
    pass(&mut ui, &mut app);

    // right-click the layer row: the menu opens where the pointer is, over the button below
    let row_y = 20.0 + 20.0 + 31.0 + 12.0 + 18.0;
    ui.pointer_moved(60.0, row_y);
    pass(&mut ui, &mut app);
    ui.pointer_context();
    assert!(pass(&mut ui, &mut app).is_some());
    assert!(ui.overlay_open(), "the right-click opened the menu");
    // the menu: padding 6, the head 26 tall, then "Lock layer" 32 tall
    let lock_y = row_y + 6.0 + 26.0 + 16.0;
    // the first pass with the menu open marks its area: the click goes to it, not the button below
    pass(&mut ui, &mut app);
    click(&mut ui, &mut app, &mut pass, 100.0, lock_y);
    assert!(app.locked, "Lock layer was picked");
    assert_eq!(
        app.under, 0,
        "the button under the menu wasn't clicked through it"
    );
    assert!(!ui.overlay_open(), "picking closed the menu");

    // opened again, a click outside closes it (and reaches nothing)
    ui.pointer_moved(60.0, row_y);
    ui.pointer_context();
    pass(&mut ui, &mut app);
    pass(&mut ui, &mut app);
    click(&mut ui, &mut app, &mut pass, 560.0, 440.0);
    assert!(!ui.overlay_open(), "a click outside closed it");
    // and Escape closes it
    ui.pointer_moved(60.0, row_y);
    ui.pointer_context();
    pass(&mut ui, &mut app);
    ui.key(key("Escape", None));
    pass(&mut ui, &mut app);
    assert!(!ui.overlay_open(), "Escape closed it");

    // the text field: focus, type, delete
    let field_y = row_y + 18.0 + 12.0 + 44.0 + 12.0 + 22.0;
    click(&mut ui, &mut app, &mut pass, 100.0, field_y);
    for c in ["C", "u", "t", " ", "2"] {
        ui.key(key(c, Some(c)));
    }
    pass(&mut ui, &mut app);
    ui.key(key("BackSpace", None));
    pass(&mut ui, &mut app);
    assert_eq!(app.name, "Cut ", "typed, then one deleted");

    // scrolling: the wheel moves the list; a click then picks what's under the pointer
    let list_y = field_y + 22.0 + 12.0;
    ui.pointer_moved(100.0, list_y + 20.0);
    pass(&mut ui, &mut app);
    ui.scrolled(110.0); // rows are 52 apart (44 + 8): the pointer, 20 down, is then 130 in: the third row
    pass(&mut ui, &mut app);
    click(&mut ui, &mut app, &mut pass, 100.0, list_y + 20.0);
    assert_eq!(
        app.picked_row,
        Some(2),
        "after scrolling 110 px, the third drawing is under the pointer"
    );

    // a tooltip after a moment's hover
    let icons_y = row_y + 18.0 + 12.0 + 22.0;
    ui.pointer_moved(540.0, icons_y); // the play icon (from the right: 34 wide)
    pass(&mut ui, &mut app);
    let due = ui.next_pass_at().expect("the tooltip's time is asked for");
    std::thread::sleep(
        due.saturating_duration_since(std::time::Instant::now())
            + std::time::Duration::from_millis(20),
    );
    assert!(pass(&mut ui, &mut app).is_some(), "the tooltip showed");
    assert!(
        pass(&mut ui, &mut app).is_none(),
        "and then nothing more: idle draws nothing"
    );

    // the picture: the menu open over the card
    ui.pointer_moved(60.0, row_y);
    ui.pointer_context();
    pass(&mut ui, &mut app);
    pass(&mut ui, &mut app);
    assert!(ui.overlay_open());
    if let Ok(dir) = std::env::var("PP_SAVE") {
        let frame = DmabufImage::new(&gpu, w as u32, h as u32, &[]).unwrap();
        r.draw(&gpu, &frame, ui.theme.paper, ui.shown()).unwrap();
        let px = gpu.read_back(&frame).unwrap();
        let mut ppm = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
        ppm.extend(px.chunks(4).flat_map(|p| [p[2], p[1], p[0]]));
        std::fs::write(format!("{dir}/overlays.ppm"), ppm).unwrap();
    }
}
