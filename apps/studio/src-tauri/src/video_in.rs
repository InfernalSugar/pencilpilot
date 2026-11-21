//! Video frames in, for rotoscoping: ffmpeg decodes a video at the project's frame rate,
//! fitted inside the canvas (centred, transparent around it), and the window takes the
//! frames one at a time as drawings.

use std::io::Read;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

/// The most frames one import takes (a minute and a bit at 24 fps).
pub const MAX_FRAMES: u32 = 2000;

/// A video being read.
pub struct Reader {
    child: Child,
    out: ChildStdout,
    frame: usize,
    pub read: u32,
}

/// What a video is: its length in seconds (when ffmpeg says), and its picture size.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Probe {
    pub seconds: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Read the length and size from ffmpeg's description of the input (`ffmpeg -i`).
pub fn parse_probe(text: &str) -> Probe {
    let seconds = text.split("Duration: ").nth(1).and_then(|s| {
        let t = s.split(',').next()?.trim();
        let mut it = t.split(':');
        let (h, m, sec) = (it.next()?, it.next()?, it.next()?);
        Some(
            h.parse::<f64>().ok()? * 3600.0
                + m.parse::<f64>().ok()? * 60.0
                + sec.parse::<f64>().ok()?,
        )
    });
    // "Stream #0:0…: Video: h264 …, yuv420p(…), 1920x1080 [SAR …], …"
    let size = text.lines().find(|l| l.contains("Video:")).and_then(|l| {
        l.split([',', ' '])
            .filter_map(|w| {
                let (a, b) = w.split_once('x')?;
                Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?))
            })
            .find(|&(w, h)| w > 0 && h > 0)
    });
    Probe {
        seconds,
        width: size.map(|s| s.0),
        height: size.map(|s| s.1),
    }
}

pub fn probe(ffmpeg: &Path, video: &Path) -> Probe {
    let out = Command::new(ffmpeg)
        .arg("-hide_banner")
        .arg("-i")
        .arg(video)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    out.map(|o| parse_probe(&String::from_utf8_lossy(&o.stderr)))
        .unwrap_or(Probe {
            seconds: None,
            width: None,
            height: None,
        })
}

/// ffmpeg's arguments: frames at `fps`, fitted inside `width`×`height`, as raw RGBA.
pub fn args(video: &Path, width: u32, height: u32, fps: u32) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        video.to_string_lossy().into_owned(),
        "-an".into(),
        "-frames:v".into(),
        MAX_FRAMES.to_string(),
        "-vf".into(),
        format!(
            "fps={fps},scale={width}:{height}:force_original_aspect_ratio=decrease,\
             format=rgba,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=black@0"
        ),
        "-f".into(),
        "rawvideo".into(),
        "-pix_fmt".into(),
        "rgba".into(),
        "-".into(),
    ]
}

impl Reader {
    pub fn open(
        ffmpeg: &Path,
        video: &Path,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<Self, String> {
        let mut child = Command::new(ffmpeg)
            .args(args(video, width, height, fps))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Couldn’t start ffmpeg: {e}"))?;
        let out = child.stdout.take().expect("piped");
        Ok(Self {
            child,
            out,
            frame: width as usize * height as usize * 4,
            read: 0,
        })
    }

    /// The next frame's RGBA, or none at the end.
    pub fn next(&mut self) -> Result<Option<Vec<u8>>, String> {
        let mut buf = vec![0u8; self.frame];
        let mut got = 0;
        while got < buf.len() {
            match self.out.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        if got == buf.len() {
            self.read += 1;
            return Ok(Some(buf));
        }
        // the end: fine if frames came, else ffmpeg says why
        let ok = self.child.wait().map(|s| s.success()).unwrap_or(false);
        if self.read == 0 || !ok {
            let mut err = String::new();
            if let Some(mut e) = self.child.stderr.take() {
                let _ = e.read_to_string(&mut err);
            }
            if self.read == 0 {
                let why = err
                    .lines()
                    .last()
                    .unwrap_or("no frames came")
                    .trim()
                    .to_string();
                return Err(format!("ffmpeg couldn’t read the video: {why}"));
            }
        }
        Ok(None)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_length_and_size() {
        let text = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'walk.mp4':\n  Duration: 00:01:02.50, start: 0.000000, bitrate: 900 kb/s\n  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1920x1080 [SAR 1:1 DAR 16:9], 800 kb/s, 25 fps\n";
        assert_eq!(
            parse_probe(text),
            Probe {
                seconds: Some(62.5),
                width: Some(1920),
                height: Some(1080)
            }
        );
        let none = parse_probe("walk.txt: Invalid data found when processing input");
        assert_eq!(none.seconds, None);
        assert_eq!(none.width, None);
    }

    #[test]
    fn asks_for_fitted_rgba_frames() {
        let a = args(Path::new("/v/walk.mp4"), 1280, 720, 24);
        let vf = &a[a.iter().position(|x| x == "-vf").unwrap() + 1];
        assert!(vf.starts_with("fps=24,scale=1280:720:force_original_aspect_ratio=decrease"));
        assert!(vf.contains("pad=1280:720"));
        assert!(a.ends_with(&["rgba".to_string(), "-".to_string()]));
        assert!(a.contains(&MAX_FRAMES.to_string()));
    }

    /// ffmpeg itself, where it's installed: a made-up two-second video comes back as 48
    /// frames at 24 fps, fitted with transparent bars.
    #[test]
    fn ffmpeg_reads_a_video() {
        let Some(ffmpeg) = crate::export::find_ffmpeg() else {
            eprintln!("no ffmpeg: skipped");
            return;
        };
        let dir = std::env::temp_dir().join(format!("pp-video-in-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("bars.mp4");
        let made = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
            ])
            .arg("testsrc=size=160x90:rate=30:duration=2")
            .args(["-pix_fmt", "yuv420p"])
            .arg(&video)
            .status()
            .unwrap();
        if !made.success() {
            eprintln!("ffmpeg can't make a test video here: skipped");
            return;
        }
        let p = probe(&ffmpeg, &video);
        assert!((p.seconds.unwrap() - 2.0).abs() < 0.1);
        assert_eq!((p.width, p.height), (Some(160), Some(90)));
        // into a square canvas: bars above and below
        let mut r = Reader::open(&ffmpeg, &video, 64, 64, 24).unwrap();
        let mut n = 0;
        while let Some(f) = r.next().unwrap() {
            assert_eq!(f.len(), 64 * 64 * 4);
            if n == 0 {
                assert_eq!(f[3], 0, "the top row is a transparent bar");
                assert_eq!(f[(32 * 64 + 32) * 4 + 3], 255, "the middle is picture");
            }
            n += 1;
        }
        assert_eq!(n, 48);
        let _ = std::fs::remove_dir_all(dir);
    }
}
