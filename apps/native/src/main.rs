//! PencilPilot, native.
//!
//!   pencilpilot                                  the app (the shell, Phase 2)
//!   pencilpilot --help | --version               this text, or the version (no window)
//!   pencilpilot path.ppstudio                    …with that project open
//!   pencilpilot --render animate --out a.png     a workspace drawn off-screen, as PNG
//!               [--size 1440x900] [--scale 1]    (no window anywhere: for checks)
//!   pencilpilot --pen-check [--replay 10] …      Phase 1's pen check (see pencheck.rs)
//!   pencilpilot --quit-after 5 --stats s.json    the app, closing after 5 s and saving how
//!                                                it ran (startup, each frame: its input,
//!                                                when shown; the view; the state): for checks
//!               [--save-doc d.png]               …and the document as a PNG on quitting
//!               [--workspace draw]               …starting in that workspace
//!               [--host] [--join pp1-…]          …hosting a session (the invite printed to
//!               [--name Ana]                     stdout) or joining one, as Ana: checks
//!                                                only (no relay: the same machine or network)

mod pencheck;

use pencilpilot_native::{shell, state};

/// Options that take a value, and those that don't.
const WITH_VALUE: &[&str] = &[
    "--render",
    "--size",
    "--scale",
    "--out",
    "--quit-after",
    "--stats",
    "--save-doc",
    "--workspace",
    "--join",
    "--name",
];
const ON_THEIR_OWN: &[&str] = &["--host"];

/// What's wrong with the command line, if anything: an option the app doesn't know, one
/// missing its value, or an argument that isn't a project. (Anything unknown stops here,
/// before a window could open.)
fn check_args(a: &[String]) -> Result<(), String> {
    let mut i = 0;
    while i < a.len() {
        let x = a[i].as_str();
        if WITH_VALUE.contains(&x) {
            if a.get(i + 1).is_none_or(|v| v.starts_with("--")) {
                return Err(format!("{x} needs a value"));
            }
            i += 2;
            continue;
        }
        if ON_THEIR_OWN.contains(&x) || x.ends_with(".ppstudio") {
            i += 1;
            continue;
        }
        return Err(if x.starts_with('-') {
            format!("unknown option {x}")
        } else {
            format!("{x} isn't a project (.ppstudio)")
        });
    }
    Ok(())
}

/// The usage: this file's own header.
fn usage() -> String {
    include_str!("main.rs")
        .lines()
        .take_while(|l| l.starts_with("//!"))
        .map(|l| l.trim_start_matches("//!").strip_prefix(' ').unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.iter().any(|x| x == "--help" || x == "-h") {
        println!("{}", usage());
        return Ok(());
    }
    if a.iter().any(|x| x == "--version" || x == "-V") {
        println!("pencilpilot {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let val = |name: &str| {
        a.iter()
            .position(|x| x == name)
            .and_then(|i| a.get(i + 1))
            .cloned()
    };
    if a.iter().any(|x| x == "--pen-check") {
        return pencheck::run();
    }
    if let Err(e) = check_args(&a) {
        eprintln!("pencilpilot: {e}\n\n{}", usage());
        std::process::exit(2);
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
    let workspace = match val("--workspace") {
        Some(ws) => Some(
            state::Workspace::parse(&ws)
                .ok_or_else(|| format!("no workspace {ws}: animate, draw, colour or review"))?,
        ),
        None => None,
    };
    // the system's own windows (file dialog, file manager) only for a normal start, never for
    // a check (which closes itself or records how it ran)
    let check = a.iter().any(|x| x == "--quit-after" || x == "--stats");
    if !check {
        shell::dialog::allow_system_windows();
        // (and the system's sound output: a check plays to a silent one)
        shell::engine::sound::allow_sound();
    }
    // drawing together without the interface: for checks only (they never use a relay)
    let together = if a.iter().any(|x| x == "--host") {
        Some(shell::engine::together::net::Start::Host)
    } else {
        val("--join").map(shell::engine::together::net::Start::Join)
    };
    if together.is_some() && !check {
        return Err("--host and --join are for checks: with --quit-after or --stats".into());
    }
    shell::run_window(shell::RunOptions {
        quit_after: val("--quit-after").and_then(|v| v.parse::<f64>().ok()),
        stats: val("--stats"),
        save_doc: val("--save-doc"),
        workspace,
        open: a.iter().find(|x| x.ends_with(".ppstudio")).cloned(),
        together,
        name: val("--name"),
    })
}

#[cfg(test)]
mod tests {
    use super::check_args;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn anything_unknown_is_refused_before_a_window() {
        assert!(check_args(&args("")).is_ok());
        assert!(check_args(&args("a.ppstudio")).is_ok());
        assert!(check_args(&args("--quit-after 5 --stats s.json --host --name Ana")).is_ok());
        assert!(check_args(&args("--frobnicate")).is_err());
        assert!(check_args(&args("notes.txt")).is_err());
        assert!(check_args(&args("--stats")).is_err(), "a value is missing");
        assert!(
            check_args(&args("--stats --host")).is_err(),
            "a value is missing"
        );
    }
}
