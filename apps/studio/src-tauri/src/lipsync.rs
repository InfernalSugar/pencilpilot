//! Lip-sync: Rhubarb Lip Sync (MIT, github.com/DanielSWolf/rhubarb-lip-sync) listens to
//! a line of dialogue and says which mouth shape shows when: A–F, and G, H and X when
//! the mouth chart has them. The window turns that into exposures on the mouth layer.
//!
//! Rhubarb ships with the installers (`scripts/fetch-rhubarb.mjs`), in the app's
//! resources as `rhubarb/rhubarb` beside its speech models (`rhubarb/res/sphinx`, which it
//! finds next to itself). When running from source, `PENCILPILOT_RHUBARB`, a fetched copy
//! in `src-tauri/rhubarb` or one on the PATH is used.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

const EXE: &str = if cfg!(windows) {
    "rhubarb.exe"
} else {
    "rhubarb"
};

/// Where Rhubarb is: given by `PENCILPILOT_RHUBARB`, in the app's resources, fetched
/// into the source tree, or on the PATH.
pub fn find(resources: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PENCILPILOT_RHUBARB").map(PathBuf::from)
        && p.is_file()
    {
        return Some(p);
    }
    let mut places: Vec<PathBuf> = Vec::new();
    if let Some(r) = resources {
        places.push(r.join("rhubarb").join(EXE));
    }
    if cfg!(debug_assertions) {
        places.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("rhubarb")
                .join(EXE),
        );
    }
    if let Some(path) = std::env::var_os("PATH") {
        places.extend(std::env::split_paths(&path).map(|d| d.join(EXE)));
    }
    places.into_iter().find(|p| p.is_file())
}

/// Rhubarb as something that can run. Unix packages that lose the executable bit on
/// resources get a runnable copy in `cache` (with a link to its speech models).
pub fn runnable(exe: &Path, cache: &Path) -> std::io::Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(exe)?.permissions().mode();
        if mode & 0o111 == 0 {
            let dir = cache.join("rhubarb");
            std::fs::create_dir_all(&dir)?;
            let copy = dir.join(EXE);
            std::fs::copy(exe, &copy)?;
            std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o755))?;
            let res = dir.join("res");
            if let Some(src) = exe.parent().map(|d| d.join("res"))
                && std::fs::symlink_metadata(&res).is_err()
            {
                std::os::unix::fs::symlink(src, &res)?;
            }
            return Ok(copy);
        }
    }
    let _ = cache;
    Ok(exe.to_path_buf())
}

/// How to listen.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    /// English: recognise words (better); otherwise: sounds only (any language)
    pub english: bool,
    /// what's said, if known (helps English recognition)
    #[serde(default)]
    pub dialog: String,
    /// the extended shapes the chart has, of "GHX"
    #[serde(default)]
    pub extended: String,
}

/// A mouth shape from `start` to `end`, in seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    mouth_cues: Vec<Cue>,
}

/// Rhubarb's command line for a WAV file (and a dialogue file, when there's text).
pub fn args(opts: &Options, wav: &Path, dialog: Option<&Path>) -> Vec<String> {
    let extended: String = opts
        .extended
        .chars()
        .filter(|c| "GHX".contains(*c))
        .collect();
    let mut a = vec![
        "--machineReadable".into(),
        "-f".into(),
        "json".into(),
        "-r".into(),
        if opts.english {
            "pocketSphinx"
        } else {
            "phonetic"
        }
        .into(),
        "--extendedShapes".into(),
        extended,
    ];
    if let Some(d) = dialog {
        a.push("-d".into());
        a.push(d.to_string_lossy().into_owned());
    }
    a.push(wav.to_string_lossy().into_owned());
    a
}

/// The mouth cues in Rhubarb's JSON output.
pub fn parse(json: &str) -> Result<Vec<Cue>, String> {
    let out: Output = serde_json::from_str(json).map_err(|e| format!("Rhubarb's answer: {e}"))?;
    Ok(out.mouth_cues)
}

/// One of Rhubarb's status lines (`--machineReadable`): progress (0–1) or why it failed.
pub enum Status {
    Progress(f64),
    Failure(String),
    Other,
}

pub fn status(line: &str) -> Status {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return Status::Other;
    };
    match v["type"].as_str() {
        Some("progress") => Status::Progress(v["value"].as_f64().unwrap_or(0.0)),
        Some("failure") => Status::Failure(v["reason"].as_str().unwrap_or("").to_string()),
        _ => Status::Other,
    }
}

/// The run going (to cancel it).
#[derive(Default)]
pub struct Running(pub Mutex<Option<Child>>);

/// Run Rhubarb on a WAV file; `progress` hears how far it is. Blocks until it's done.
pub fn run(
    exe: &Path,
    wav: &Path,
    opts: &Options,
    running: &Running,
    mut progress: impl FnMut(f64),
) -> Result<Vec<Cue>, String> {
    let dialog = if opts.dialog.trim().is_empty() {
        None
    } else {
        let p = wav.with_extension("txt");
        std::fs::write(&p, opts.dialog.trim()).map_err(|e| e.to_string())?;
        Some(p)
    };
    let mut child = Command::new(exe)
        .args(args(opts, wav, dialog.as_deref()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if cfg!(target_os = "macos") && e.raw_os_error() == Some(86) {
                "Lip-sync needs Rosetta on this Mac (Rhubarb is built for Intel Macs). \
                 Install it with: softwareupdate --install-rosetta"
                    .to_string()
            } else {
                format!("Couldn’t start Rhubarb: {e}")
            }
        })?;
    let mut stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    *running.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let mut failure = None;
    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
        match status(&line) {
            Status::Progress(v) => progress(v),
            Status::Failure(r) => failure = Some(r),
            Status::Other => {}
        }
    }
    let out = reader.join().unwrap_or_default();
    let child = running.0.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(d) = dialog {
        let _ = std::fs::remove_file(d);
    }
    let Some(mut child) = child else {
        return Err("cancelled".into());
    };
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    if !ok {
        return Err(failure.unwrap_or_else(|| "Rhubarb stopped".into()));
    }
    parse(&out)
}

/// Stop the run going, if any.
pub fn cancel(running: &Running) {
    if let Some(mut c) = running.0.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_follows_the_options() {
        let o = Options {
            english: false,
            dialog: String::new(),
            extended: "XQG".into(),
        };
        let a = args(&o, Path::new("/t/a.wav"), None);
        assert_eq!(
            a,
            [
                "--machineReadable",
                "-f",
                "json",
                "-r",
                "phonetic",
                "--extendedShapes",
                "XG",
                "/t/a.wav"
            ]
        );
        let o = Options { english: true, ..o };
        let a = args(&o, Path::new("a.wav"), Some(Path::new("a.txt")));
        assert!(a.contains(&"pocketSphinx".to_string()));
        assert_eq!(a[a.len() - 3..], ["-d", "a.txt", "a.wav"]);
    }

    #[test]
    fn reads_rhubarbs_answer_and_status() {
        let cues = parse(
            r#"{"metadata":{"soundFile":"hi.wav","duration":0.47},"mouthCues":[
            {"start":0.00,"end":0.05,"value":"X"},{"start":0.05,"end":0.27,"value":"D"}]}"#,
        )
        .unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[1].value, "D");
        assert!((cues[1].end - 0.27).abs() < 1e-9);
        assert!(parse("nope").is_err());
        assert!(matches!(
            status(r#"{ "type": "progress", "value": 0.69, "log": {} }"#),
            Status::Progress(v) if (v - 0.69).abs() < 1e-9
        ));
        assert!(matches!(
            status(r#"{ "type": "failure", "reason": "bad file" }"#),
            Status::Failure(r) if r == "bad file"
        ));
        assert!(matches!(status("plain text"), Status::Other));
    }

    /// Rhubarb itself on a made-up recording: runs where it's installed (CI fetches it
    /// and sets `PENCILPILOT_RHUBARB`).
    #[test]
    fn rhubarb_listens_to_a_recording() {
        let Some(exe) = std::env::var_os("PENCILPILOT_RHUBARB").map(PathBuf::from) else {
            eprintln!("PENCILPILOT_RHUBARB isn't set: skipped");
            return;
        };
        // a second and a half of vowel-ish bursts (two tones) with pauses, 16 kHz mono
        let rate = 16_000u32;
        let n = rate as usize * 3 / 2;
        let mut pcm = Vec::with_capacity(n * 2);
        for i in 0..n {
            let t = i as f64 / rate as f64;
            let on = ((t * 4.0) as u32).is_multiple_of(2);
            let v = if on {
                0.3 * (t * 2.0 * std::f64::consts::PI * 220.0).sin()
                    + 0.2 * (t * 2.0 * std::f64::consts::PI * 880.0).sin()
            } else {
                0.0
            };
            pcm.extend_from_slice(&((v * 32767.0) as i16).to_le_bytes());
        }
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(&pcm);
        let path = std::env::temp_dir().join(format!("pp-lipsync-test-{}.wav", std::process::id()));
        std::fs::write(&path, wav).unwrap();
        for english in [false, true] {
            let opts = Options {
                english,
                dialog: if english {
                    "hello there".into()
                } else {
                    String::new()
                },
                extended: "GHX".into(),
            };
            let mut seen = 0.0;
            let cues = run(&exe, &path, &opts, &Running::default(), |v| seen = v).unwrap();
            assert!(!cues.is_empty());
            assert!(seen > 0.99, "progress reached {seen}");
            assert!(cues.iter().all(|c| "ABCDEFGHX".contains(c.value.as_str())));
            let end = cues.last().unwrap().end;
            assert!((end - 1.5).abs() < 0.05, "ends at {end}");
        }
        let _ = std::fs::remove_file(path);
    }
}
