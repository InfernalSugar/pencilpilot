//! The File menu's actions through the file dialog, answered by `PENCILPILOT_DIALOG_ANSWER`
//! (its own process: the environment is set once, before anything reads it), with the
//! config and state folders in a temporary one.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use pencilpilot_native::shell::engine::{Engine, files};
use pencilpilot_native::state::{Action, State};

fn settle(e: &mut Engine, s: &mut State) {
    // (a save is written on another thread)
    let t = Instant::now();
    for _ in 0..3 {
        e.tick(s, Instant::now());
        std::thread::sleep(Duration::from_millis(5));
    }
    while s.project.saving && t.elapsed() < Duration::from_secs(10) {
        e.tick(s, Instant::now());
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn open_save_as_and_cancel_through_the_dialog() {
    let dir = std::env::temp_dir().join(format!("pp-files-flow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // (one test in this process: nothing else reads the environment meanwhile)
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        std::env::set_var("XDG_STATE_HOME", dir.join("state"));
    }
    // a project made as other tools make them
    let src = dir.join("walk.ppstudio");
    let mut p = ppstudio::Project::new("test", "2026-11-27T00:00:00Z", 640, 360, 30);
    p.manifest.layers.push(ppstudio::Layer {
        id: 1,
        name: "Lines".into(),
        visible: true,
        opacity: 1.0,
        blend: "normal".into(),
        locked: false,
        alpha_lock: false,
        clip: false,
        imported: false,
        group: 0,
        adjust: None,
        style: None,
        text: None,
        lines: false,
    });
    let mut px = vec![0u8; 640 * 360 * 4];
    for y in 100..120 {
        for x in 50..300 {
            let i = (y * 640 + x) * 4;
            px[i..i + 4].copy_from_slice(&[20, 20, 30, 255]);
        }
    }
    p.put_drawing("D1", &px, None).unwrap();
    p.manifest.timeline.scenes = vec![ppstudio::Scene {
        name: "Walk".into(),
        cuts: vec![ppstudio::Cut {
            name: "Cut 1".into(),
            tracks: [(
                1,
                vec![ppstudio::Exposure {
                    drawing: Some("D1".into()),
                    length: 6,
                }],
            )]
            .into(),
            ..Default::default()
        }],
        ..Default::default()
    }];
    p.manifest.timeline.frames = ppstudio::Timeline::flatten(&p.manifest.timeline.scenes, &[]);
    p.save(&src).unwrap();

    let mut s = State::demo();
    let mut e = Engine::new(s.view.doc_w, s.view.doc_h);
    settle(&mut e, &mut s);

    // Open…: the dialog answers with the file
    unsafe { std::env::set_var("PENCILPILOT_DIALOG_ANSWER", &src) };
    assert!(e.apply(&mut s, &Action::File(files::OPEN.into())));
    settle(&mut e, &mut s);
    assert_eq!(s.project.name, "walk", "{}", s.project.note);
    assert_eq!((e.painter.width(), e.painter.height()), (640, 360));
    assert_eq!((s.fps, s.frames), (30, 6));
    assert_eq!(s.layers[0].name, "Lines");

    // Save as…, to a name without the extension: it gets one
    let to = dir.join("walk two");
    unsafe { std::env::set_var("PENCILPILOT_DIALOG_ANSWER", &to) };
    assert!(e.apply(&mut s, &Action::File(files::SAVE_AS.into())));
    settle(&mut e, &mut s);
    let saved = PathBuf::from(format!("{}.ppstudio", to.display()));
    assert!(saved.is_file(), "{}", s.project.note);
    assert_eq!(s.project.path.as_deref(), Some(saved.as_path()));
    assert_eq!(s.project.name, "walk two");
    let back = ppstudio::Project::open(&saved).unwrap();
    assert_eq!(back.drawing("D1").unwrap().0, px);

    // cancelled: nothing changes
    unsafe { std::env::set_var("PENCILPILOT_DIALOG_ANSWER", "") };
    assert!(e.apply(&mut s, &Action::File(files::OPEN.into())));
    settle(&mut e, &mut s);
    assert_eq!(s.project.name, "walk two");
    assert!(s.project.note.is_empty());

    // a new project with no file: Save asks where
    assert!(e.apply(&mut s, &Action::File(files::NEW.into())));
    let fresh = dir.join("fresh.ppstudio");
    unsafe { std::env::set_var("PENCILPILOT_DIALOG_ANSWER", &fresh) };
    assert!(e.apply(&mut s, &Action::File(files::SAVE.into())));
    settle(&mut e, &mut s);
    assert!(fresh.is_file());
    let back = ppstudio::Project::open(&fresh).unwrap();
    assert_eq!(
        (back.manifest.canvas.width, back.manifest.canvas.height),
        (1920, 1080)
    );
    assert_eq!(back.manifest.layers.len(), 1);

    // the recent files, newest first, in the config folder
    let recent = std::fs::read_to_string(dir.join("config/pencilpilot/recent")).unwrap();
    let lines: Vec<&str> = recent.lines().collect();
    assert!(lines[0].ends_with("fresh.ppstudio"), "{lines:?}");
    assert!(lines[1].ends_with("walk two.ppstudio"));
    assert!(lines[2].ends_with("walk.ppstudio"));
    assert_eq!(s.project.recent.len(), 3);
    unsafe { std::env::remove_var("PENCILPILOT_DIALOG_ANSWER") };
}
