//! `ppstudio`: make and read PencilPilot projects from the command line.
//!
//! - `ppstudio build <spec.json> <out.ppstudio>`: a project from a description of its
//!   layers, scenes, cuts and frames, each frame a picture file (PNG, JPEG or WebP),
//!   fitted inside the canvas. The same picture used twice is one drawing.
//! - `ppstudio render <project.ppstudio> <out-dir> [--layers A,B] [--every]`: each
//!   drawing change of the timeline as a PNG (`0001.png`…), the layers composited over
//!   white (normal blending, their opacity; hidden layers left out). With `--every`, one
//!   PNG per frame. Prints what it wrote as JSON.
//! - `ppstudio info <project.ppstudio>`: the timeline as JSON (scenes, cuts, lengths).
//!
//! The director plugin (`plugins/pencilpilot-director`) uses it to put generated
//! frames into a project and to look at what the artist drew.

mod build;
mod render;

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run = || -> Result<serde_json::Value, String> {
        match args.first().map(String::as_str) {
            Some("build") if args.len() == 3 => {
                let spec = std::fs::read_to_string(&args[1]).map_err(|e| format!("{}: {e}", args[1]))?;
                let spec: build::Spec = serde_json::from_str(&spec).map_err(|e| format!("the spec: {e}"))?;
                let base = Path::new(&args[1]).parent().unwrap_or(Path::new("."));
                build::build(&spec, base, Path::new(&args[2]))
            }
            Some("render") if args.len() >= 3 => {
                let mut layers: Option<Vec<String>> = None;
                let mut every = false;
                let mut i = 3;
                while i < args.len() {
                    match args[i].as_str() {
                        "--layers" => {
                            layers = args.get(i + 1).map(|s| s.split(',').map(str::to_string).collect());
                            i += 1;
                        }
                        "--every" => every = true,
                        other => return Err(format!("unknown option {other}")),
                    }
                    i += 1;
                }
                render::render(Path::new(&args[1]), Path::new(&args[2]), layers.as_deref(), every)
            }
            Some("info") if args.len() == 2 => render::info(Path::new(&args[1])),
            _ => Err("usage: ppstudio build <spec.json> <out.ppstudio> | render <project> <out-dir> [--layers A,B] [--every] | info <project>".into()),
        }
    };
    match run() {
        Ok(v) => {
            println!("{v}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ppstudio: {e}");
            ExitCode::FAILURE
        }
    }
}
