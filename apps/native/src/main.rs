//! PencilPilot, native.
//!
//!   pencilpilot                                  the app (the shell, Phase 2)
//!   pencilpilot --render animate --out a.png     a workspace drawn off-screen, as PNG
//!               [--size 1440x900] [--scale 1]    (no window anywhere: for checks)
//!   pencilpilot --pen-check [--replay 10] …      Phase 1's pen check (see pencheck.rs)

mod pencheck;
mod shell;
mod state;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let val = |name: &str| {
        a.iter()
            .position(|x| x == name)
            .and_then(|i| a.get(i + 1))
            .cloned()
    };
    if a.iter().any(|x| x == "--pen-check") {
        return pencheck::run();
    }
    if let Some(ws) = val("--render") {
        let mut s = state::State::demo();
        s.workspace = state::Workspace::parse(&ws)
            .ok_or_else(|| format!("no workspace {ws}: animate, draw, colour or review"))?;
        let (w, h) = val("--size")
            .and_then(|v| {
                v.split_once('x')
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
            })
            .unwrap_or((1440, 900));
        let scale = val("--scale").and_then(|v| v.parse().ok()).unwrap_or(1.0);
        let out = val("--out").unwrap_or_else(|| format!("{ws}.png"));
        return shell::render_png(&mut s, w, h, scale, &out);
    }
    shell::run_window()
}
