//! Widgets with simulated input: they react, and nothing is drawn unless something looks
//! different. Skips without Vulkan. `PP_SAVE` = a folder for the picture.

#![cfg(target_os = "linux")]

use pencilpilot_gpu::{DmabufImage, Error, Gpu, Renderer2d};
use pencilpilot_ui::ui::{Frame, Kind, Rect, Ui};

#[derive(Default)]
struct App {
    onion: bool,
    zoom: f32,
    tab: usize,
    clicks: u32,
}

fn panel(f: &mut Frame, app: &mut App) {
    f.card(Rect::new(24.0, 24.0, 472.0, 312.0), |f| {
        f.heading("Animate");
        f.tabs(
            "workspace",
            &["Animate", "Draw", "Colour", "Review"],
            &mut app.tab,
        );
        f.row(44.0, |f| {
            if f.button("new", "New drawing", Kind::Primary).clicked {
                app.clicks += 1;
            }
            f.button("dup", "Duplicate", Kind::Secondary);
            f.then_right();
            f.button("more", "More", Kind::Ghost);
        });
        f.toggle("onion", "Onion skin", &mut app.onion);
        f.slider("zoom", &mut app.zoom, 8.0, 44.0, 240.0);
    });
}

#[test]
fn widgets_react_and_idle_draws_nothing() {
    let gpu = match Gpu::new() {
        Ok(g) => g,
        Err(e @ (Error::NoLoader(_) | Error::NoDevice(_))) => return eprintln!("skipped: {e}"),
        Err(e) => panic!("{e}"),
    };
    let mut r = Renderer2d::new(&gpu).unwrap();
    let scale = 1.5;
    let mut ui = Ui::new(scale);
    ui.text.wait_for_fonts();
    let mut app = App {
        zoom: 22.0,
        ..Default::default()
    };
    let (w, h) = (520.0, 360.0);
    let mut pass = |ui: &mut Ui, app: &mut App| ui.pass(&mut r, w, h, |f| panel(f, app));

    assert!(pass(&mut ui, &mut app).is_some(), "the first pass draws");
    assert!(
        pass(&mut ui, &mut app).is_none(),
        "nothing changed: nothing drawn"
    );
    ui.pointer_moved(505.0, 350.0); // paper, outside the card
    assert!(
        pass(&mut ui, &mut app).is_none(),
        "the pointer over nothing that reacts: nothing drawn"
    );

    // find the widgets by where the layout put them: the card's padding is 20, its gap 12
    let row_y = 24.0 + 20.0 + 31.0 + 12.0 + 44.0 + 12.0 + 22.0; // heading, tabs, then the row's middle
    ui.pointer_moved(110.0, row_y);
    assert!(
        pass(&mut ui, &mut app).is_some(),
        "hovering the button draws it lighter"
    );
    ui.pointer_button(true);
    pass(&mut ui, &mut app);
    ui.pointer_button(false);
    pass(&mut ui, &mut app);
    assert_eq!(app.clicks, 1, "the button was clicked");

    let toggle_y = row_y + 22.0 + 12.0 + 13.0;
    ui.pointer_moved(60.0, toggle_y);
    pass(&mut ui, &mut app);
    ui.pointer_button(true);
    pass(&mut ui, &mut app);
    ui.pointer_button(false);
    assert!(pass(&mut ui, &mut app).is_some());
    assert!(app.onion, "the toggle flipped");

    let slider_y = toggle_y + 13.0 + 12.0 + 14.0;
    ui.pointer_moved(50.0, slider_y);
    pass(&mut ui, &mut app);
    ui.pointer_button(true);
    pass(&mut ui, &mut app);
    ui.pointer_moved(44.0 + 180.0, slider_y + 30.0); // dragging: it follows even off the track
    pass(&mut ui, &mut app);
    ui.pointer_button(false);
    pass(&mut ui, &mut app);
    let want = 8.0 + (180.0 / 240.0) * 36.0;
    assert!(
        (app.zoom - want).abs() < 0.5,
        "the slider followed the drag: {} (want {want})",
        app.zoom
    );

    let tabs_y = 24.0 + 20.0 + 31.0 + 12.0 + 22.0;
    ui.pointer_moved(220.0, tabs_y);
    pass(&mut ui, &mut app);
    ui.pointer_button(true);
    pass(&mut ui, &mut app);
    ui.pointer_button(false);
    let last = pass(&mut ui, &mut app);
    assert_eq!(app.tab, 2, "the third tab (Colour) was picked");
    ui.pointer_moved(505.0, 350.0);
    let quads = pass(&mut ui, &mut app).or(last).expect("something to show");

    let frame = DmabufImage::new(&gpu, (w * scale) as u32, (h * scale) as u32, &[]).unwrap();
    r.draw(&gpu, &frame, ui.theme.paper, &quads).unwrap();
    if let Ok(dir) = std::env::var("PP_SAVE") {
        let px = gpu.read_back(&frame).unwrap();
        let mut ppm = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
        ppm.extend(px.chunks(4).flat_map(|p| [p[2], p[1], p[0]]));
        std::fs::write(format!("{dir}/widgets.ppm"), ppm).unwrap();
    }
}

/// A click that changes what an earlier part already drew shows complete in that same pass:
/// two buttons lit by one choice; clicking the second unlights the first at once.
#[test]
fn a_click_shows_complete_in_its_own_pass() {
    let gpu = match Gpu::new() {
        Ok(g) => g,
        Err(e @ (Error::NoLoader(_) | Error::NoDevice(_))) => return eprintln!("skipped: {e}"),
        Err(e) => panic!("{e}"),
    };
    let mut r = Renderer2d::new(&gpu).unwrap();
    let mut ui = Ui::new(1.0);
    ui.text.wait_for_fonts();
    let mut picked = 0usize;
    const PATH: &str = "M4 12h16";
    let mut build = |f: &mut Frame, picked: &mut usize| {
        f.row(34.0, |f| {
            for i in 0..2 {
                if f.icon_button(&format!("b{i}"), PATH, "", *picked == i, false)
                    .clicked
                {
                    *picked = i;
                }
            }
        });
    };
    ui.pass(&mut r, 200.0, 60.0, |f| build(f, &mut picked));
    // lit buttons are ink-filled squares; count them
    let lit = |q: &[pencilpilot_gpu::Quad], ui: &Ui| {
        let ink = ui.theme.ink;
        q.iter()
            .filter(|q| q.fill_rgba() == [ink[0], ink[1], ink[2], 1.0] && q.rect_xywh()[2] == 34.0)
            .count()
    };
    assert_eq!(lit(ui.shown(), &ui), 1);
    // click the second button (34 px squares from x 0)
    ui.pointer_moved(51.0, 17.0);
    ui.pointer_button(true);
    ui.pass(&mut r, 200.0, 60.0, |f| build(f, &mut picked));
    // the click is the release: its pass alone (no settling pass by hand)
    ui.pointer_button(false);
    ui.pass(&mut r, 200.0, 60.0, |f| build(f, &mut picked));
    let shown = ui.shown().to_vec();
    assert_eq!(picked, 1);
    let ink = ui.theme.ink;
    let lit_at: Vec<f32> = shown
        .iter()
        .filter(|q| q.fill_rgba() == [ink[0], ink[1], ink[2], 1.0] && q.rect_xywh()[2] == 34.0)
        .map(|q| q.rect_xywh()[0])
        .collect();
    assert_eq!(
        lit_at,
        [42.0],
        "the second button lit, the first not, in the click's own pass"
    );
}
