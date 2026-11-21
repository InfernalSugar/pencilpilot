//! Exporting an animation: the window renders each frame and sends its pixels here,
//! one after another; this end writes them as a PNG sequence, a GIF, an APNG, or a
//! video through ffmpeg (MP4 / H.264, ProRes 4444 with alpha, WebM / VP9).
//!
//! A frame comes with how many output frames it's held for (`repeat`), so a drawing on
//! twos crosses over once. GIF and APNG keep a held frame as one frame shown longer;
//! a PNG sequence and video write it out each time.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Png,
    Gif,
    Apng,
    Mp4,
    Prores,
    Webm,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png | Format::Apng => "png",
            Format::Gif => "gif",
            Format::Mp4 => "mp4",
            Format::Prores => "mov",
            Format::Webm => "webm",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub format: Format,
    /// where it goes (a PNG sequence: the first file's name; the others are numbered)
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// keep transparency (PNG, GIF, APNG, ProRes, WebM); otherwise frames are opaque
    #[serde(default)]
    pub transparent: bool,
    /// distinct frames to come (APNG needs the count up front)
    pub frames: u32,
    /// output frames in all (for numbering a PNG sequence)
    #[serde(default)]
    pub total: u32,
    /// play once instead of looping (GIF, APNG)
    #[serde(default)]
    pub once: bool,
}

enum Writer {
    Png {
        stem: PathBuf,
        digits: usize,
        n: u32,
        written: Vec<PathBuf>,
    },
    Gif {
        enc: Box<gif::Encoder<BufWriter<File>>>,
        /// time so far, in output frames and in GIF hundredths (to spread rounding)
        frames: u64,
        cs: u64,
    },
    Apng {
        enc: Box<png::StreamWriter<'static, BufWriter<File>>>,
    },
    Video {
        child: Child,
        stdin: Option<ChildStdin>,
    },
}

pub struct Job {
    spec: Spec,
    writer: Writer,
    path: PathBuf,
    sent: u32,
}

/// Where ffmpeg is: `PENCILPILOT_FFMPEG`, the one installed with the app (next to it,
/// as `pencilpilot-ffmpeg`: see scripts/fetch-ffmpeg.mjs), the PATH, or a usual folder.
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PENCILPILOT_FFMPEG").map(PathBuf::from)
        && p.is_file()
    {
        return Some(p);
    }
    let bundled = if cfg!(windows) {
        "pencilpilot-ffmpeg.exe"
    } else {
        "pencilpilot-ffmpeg"
    };
    if let Some(p) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(bundled)))
        && p.is_file()
    {
        return Some(p);
    }
    let exe = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    // (apps opened from the Dock or Start menu don't get the shell's PATH)
    for d in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "C:\\ffmpeg\\bin",
        "C:\\Program Files\\ffmpeg\\bin",
    ] {
        dirs.push(PathBuf::from(d));
    }
    dirs.into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}

/// What ffmpeg can encode, of the three video formats.
pub fn video_formats(ffmpeg: &Path) -> Vec<Format> {
    let Ok(out) = Command::new(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .stdin(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let list = String::from_utf8_lossy(&out.stdout);
    let has = |name: &str| list.split_whitespace().any(|w| w == name);
    let mut v = Vec::new();
    if has("libx264") {
        v.push(Format::Mp4);
    }
    if has("prores_ks") {
        v.push(Format::Prores);
    }
    if has("libvpx-vp9") {
        v.push(Format::Webm);
    }
    v
}

fn ffmpeg_args(spec: &Spec, out: &Path) -> Vec<String> {
    let mut a: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-s",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(format!("{}x{}", spec.width, spec.height));
    a.extend(["-r".into(), spec.fps.to_string(), "-i".into(), "-".into()]);
    let enc: &[&str] = match (spec.format, spec.transparent) {
        (Format::Mp4, _) => &[
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "16",
            "-preset",
            "slow",
            "-tune",
            "animation",
            "-movflags",
            "+faststart",
        ],
        (Format::Prores, true) => &[
            "-c:v",
            "prores_ks",
            "-profile:v",
            "4444",
            "-pix_fmt",
            "yuva444p10le",
            "-vendor",
            "apl0",
        ],
        (Format::Prores, false) => &[
            "-c:v",
            "prores_ks",
            "-profile:v",
            "4444",
            "-pix_fmt",
            "yuv444p10le",
            "-vendor",
            "apl0",
        ],
        (Format::Webm, true) => &[
            "-c:v",
            "libvpx-vp9",
            "-pix_fmt",
            "yuva420p",
            "-b:v",
            "0",
            "-crf",
            "28",
            "-row-mt",
            "1",
        ],
        (Format::Webm, false) => &[
            "-c:v",
            "libvpx-vp9",
            "-pix_fmt",
            "yuv420p",
            "-b:v",
            "0",
            "-crf",
            "28",
            "-row-mt",
            "1",
        ],
        _ => &[],
    };
    a.extend(enc.iter().map(|s| s.to_string()));
    a.push(out.to_string_lossy().into_owned());
    a
}

impl Job {
    pub fn begin(spec: Spec) -> Result<Job, String> {
        let mut path = PathBuf::from(&spec.path);
        if !path.is_absolute() {
            return Err("exports go to an absolute path".into());
        }
        if spec.width == 0 || spec.height == 0 || spec.width > 16384 || spec.height > 16384 {
            return Err(format!("a {}×{} export", spec.width, spec.height));
        }
        if spec.fps == 0 || spec.fps > 120 || spec.frames == 0 {
            return Err("nothing to export".into());
        }
        if path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            != Some(spec.format.extension().into())
        {
            path.set_extension(spec.format.extension());
        }
        let file = || -> Result<BufWriter<File>, String> {
            Ok(BufWriter::new(
                File::create(&path).map_err(|e| e.to_string())?,
            ))
        };
        let writer = match spec.format {
            Format::Png => {
                let stem = path.with_extension("");
                let digits = spec.total.max(spec.frames).to_string().len().max(4);
                Writer::Png {
                    stem,
                    digits,
                    n: 0,
                    written: Vec::new(),
                }
            }
            Format::Gif => {
                let mut enc =
                    gif::Encoder::new(file()?, spec.width as u16, spec.height as u16, &[])
                        .map_err(|e| e.to_string())?;
                enc.set_repeat(if spec.once {
                    gif::Repeat::Finite(0)
                } else {
                    gif::Repeat::Infinite
                })
                .map_err(|e| e.to_string())?;
                if spec.width > 65535 || spec.height > 65535 {
                    return Err("too big for a GIF".into());
                }
                Writer::Gif {
                    enc: Box::new(enc),
                    frames: 0,
                    cs: 0,
                }
            }
            Format::Apng => {
                let mut enc = png::Encoder::new(file()?, spec.width, spec.height);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                enc.set_animated(spec.frames, if spec.once { 1 } else { 0 })
                    .map_err(|e| e.to_string())?;
                let w = enc.write_header().map_err(|e| e.to_string())?;
                Writer::Apng {
                    enc: Box::new(w.into_stream_writer().map_err(|e| e.to_string())?),
                }
            }
            Format::Mp4 | Format::Prores | Format::Webm => {
                let ffmpeg = find_ffmpeg().ok_or("ffmpeg isn't installed")?;
                let mut child = Command::new(ffmpeg)
                    .args(ffmpeg_args(&spec, &path))
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("couldn't start ffmpeg: {e}"))?;
                let stdin = child.stdin.take();
                Writer::Video { child, stdin }
            }
        };
        Ok(Job {
            spec,
            writer,
            path,
            sent: 0,
        })
    }

    /// One frame (straight RGBA8, width × height), held for `repeat` output frames.
    pub fn frame(&mut self, rgba: &[u8], repeat: u32) -> Result<(), String> {
        let (w, h) = (self.spec.width, self.spec.height);
        if rgba.len() != (w * h * 4) as usize {
            return Err(format!("a frame of {} bytes for {w}×{h}", rgba.len()));
        }
        if self.sent >= self.spec.frames {
            return Err("more frames than were announced".into());
        }
        self.sent += 1;
        let repeat = repeat.max(1);
        let fps = self.spec.fps as u64;
        // opaque formats: anything see-through goes over white
        let flat;
        let rgba = if self.spec.transparent && !matches!(self.spec.format, Format::Mp4) {
            rgba
        } else {
            flat = over_white(rgba);
            &flat[..]
        };
        match &mut self.writer {
            Writer::Png {
                stem,
                digits,
                n,
                written,
            } => {
                let mut bytes = Vec::new();
                {
                    let mut enc = png::Encoder::new(&mut bytes, w, h);
                    enc.set_color(png::ColorType::Rgba);
                    enc.set_depth(png::BitDepth::Eight);
                    enc.set_compression(png::Compression::Fast);
                    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
                    wr.write_image_data(rgba).map_err(|e| e.to_string())?;
                }
                for _ in 0..repeat {
                    *n += 1;
                    let name = format!(
                        "{}_{:0width$}.png",
                        stem.file_name().unwrap_or_default().to_string_lossy(),
                        n,
                        width = *digits
                    );
                    let p = stem.with_file_name(name);
                    std::fs::write(&p, &bytes).map_err(|e| e.to_string())?;
                    written.push(p);
                }
            }
            Writer::Gif { enc, frames, cs } => {
                // GIF delays are in hundredths: spread the rounding over the frames
                *frames += repeat as u64;
                let until = (*frames * 100 + fps / 2) / fps;
                let delay = (until - *cs).clamp(2, 65535);
                *cs += delay;
                let mut px = rgba.to_vec();
                let mut f = gif::Frame::from_rgba_speed(w as u16, h as u16, &mut px, 10);
                f.delay = delay as u16;
                f.dispose = gif::DisposalMethod::Background;
                enc.write_frame(&f).map_err(|e| e.to_string())?;
            }
            Writer::Apng { enc } => {
                enc.set_frame_delay(repeat.min(65535) as u16, self.spec.fps as u16)
                    .map_err(|e| e.to_string())?;
                enc.set_dispose_op(png::DisposeOp::Background)
                    .map_err(|e| e.to_string())?;
                enc.set_blend_op(png::BlendOp::Source)
                    .map_err(|e| e.to_string())?;
                enc.write_all(rgba).map_err(|e| e.to_string())?;
            }
            Writer::Video { child, stdin } => {
                let pipe = stdin.as_mut().ok_or("ffmpeg stopped")?;
                for _ in 0..repeat {
                    if pipe.write_all(rgba).is_err() {
                        let err = ffmpeg_error(child);
                        return Err(format!("ffmpeg stopped: {err}"));
                    }
                }
            }
        }
        Ok(())
    }

    /// Finish the file; returns where it went (a PNG sequence: its first file).
    pub fn end(self) -> Result<String, String> {
        if self.sent != self.spec.frames {
            return Err(format!("{} of {} frames came", self.sent, self.spec.frames));
        }
        match self.writer {
            Writer::Png { written, .. } => Ok(written
                .first()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()),
            Writer::Gif { enc, .. } => {
                enc.into_inner()
                    .map_err(|e| e.to_string())?
                    .flush()
                    .map_err(|e| e.to_string())?;
                Ok(self.path.to_string_lossy().into_owned())
            }
            Writer::Apng { enc } => {
                enc.finish().map_err(|e| e.to_string())?;
                Ok(self.path.to_string_lossy().into_owned())
            }
            Writer::Video {
                mut child,
                mut stdin,
            } => {
                drop(stdin.take());
                let status = child.wait().map_err(|e| e.to_string())?;
                if !status.success() {
                    return Err(format!("ffmpeg failed: {}", ffmpeg_error(&mut child)));
                }
                Ok(self.path.to_string_lossy().into_owned())
            }
        }
    }

    /// Stop, and take away what was written.
    pub fn cancel(self) {
        match self.writer {
            Writer::Png { written, .. } => {
                for p in written {
                    let _ = std::fs::remove_file(p);
                }
            }
            Writer::Video { mut child, stdin } => {
                drop(stdin);
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&self.path);
            }
            Writer::Gif { enc, .. } => {
                drop(enc);
                let _ = std::fs::remove_file(&self.path);
            }
            Writer::Apng { enc } => {
                drop(enc);
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

fn ffmpeg_error(child: &mut Child) -> String {
    use std::io::Read;
    let mut s = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut s);
    }
    s.lines().last().unwrap_or("it stopped").trim().to_string()
}

/// Straight RGBA over white, opaque (for formats without transparency).
fn over_white(rgba: &[u8]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    for px in out.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a == 255 {
            continue;
        }
        for v in &mut px[..3] {
            *v = ((*v as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        }
        px[3] = 255;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(format: Format, dir: &Path, name: &str, frames: u32) -> Spec {
        Spec {
            format,
            path: dir.join(name).to_string_lossy().into_owned(),
            width: 8,
            height: 6,
            fps: 12,
            transparent: true,
            frames,
            total: 0,
            once: false,
        }
    }

    fn frame(r: u8, a: u8) -> Vec<u8> {
        [r, 0, 0, a].repeat(8 * 6)
    }

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "pp-export-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn png_sequences_number_every_frame() {
        let d = tmp();
        let mut j = Job::begin(spec(Format::Png, &d, "walk.png", 2)).unwrap();
        j.frame(&frame(255, 255), 2).unwrap();
        j.frame(&frame(10, 0), 1).unwrap();
        let first = j.end().unwrap();
        assert!(first.ends_with("walk_0001.png"), "{first}");
        let mut names: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["walk_0001.png", "walk_0002.png", "walk_0003.png"]);
        // the last one kept its transparency
        let dec = png::Decoder::new(std::io::BufReader::new(
            File::open(d.join("walk_0003.png")).unwrap(),
        ));
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        r.next_frame(&mut buf).unwrap();
        assert_eq!(buf[3], 0);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn gifs_and_apngs_hold_frames_by_time() {
        let d = tmp();
        // 12 fps: held 2 frames = 1/6 s ≈ 17 hundredths (rounding spread out)
        let mut g = Job::begin(spec(Format::Gif, &d, "a.gif", 2)).unwrap();
        g.frame(&frame(255, 255), 2).unwrap();
        g.frame(&frame(0, 0), 1).unwrap();
        let p = g.end().unwrap();
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::RGBA);
        let mut dec = opts.read_info(File::open(&p).unwrap()).unwrap();
        let mut delays = Vec::new();
        while let Some(f) = dec.read_next_frame().unwrap() {
            delays.push(f.delay);
        }
        assert_eq!(delays, [17, 8]);

        let mut a = Job::begin(spec(Format::Apng, &d, "a.png", 2)).unwrap();
        a.frame(&frame(255, 255), 3).unwrap();
        a.frame(&frame(0, 128), 1).unwrap();
        let p = a.end().unwrap();
        let dec = png::Decoder::new(std::io::BufReader::new(File::open(&p).unwrap()));
        let r = dec.read_info().unwrap();
        let ctl = r.info().animation_control.unwrap();
        assert_eq!(ctl.num_frames, 2);
        // announced frames must all come
        let mut short = Job::begin(spec(Format::Gif, &d, "b.gif", 3)).unwrap();
        short.frame(&frame(1, 255), 1).unwrap();
        assert!(short.end().is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn videos_go_through_ffmpeg_when_it_is_there() {
        let Some(ff) = find_ffmpeg() else {
            return; // (not installed here)
        };
        let formats = video_formats(&ff);
        let d = tmp();
        for f in formats {
            let mut s = spec(f, &d, "clip", 2);
            s.width = 16;
            s.height = 16;
            s.transparent = f != Format::Mp4;
            let mut j = Job::begin(s).unwrap();
            j.frame(&[200, 10, 10, 255].repeat(256), 3).unwrap();
            j.frame(&[10, 10, 200, 128].repeat(256), 2).unwrap();
            let p = j.end().unwrap();
            assert!(p.ends_with(f.extension()), "{p}");
            assert!(std::fs::metadata(&p).unwrap().len() > 100, "{p}");
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn see_through_pixels_go_over_white_when_flattened() {
        assert_eq!(over_white(&[0, 0, 0, 0]), [255, 255, 255, 255]);
        assert_eq!(over_white(&[0, 0, 0, 255]), [0, 0, 0, 255]);
        assert_eq!(over_white(&[0, 0, 0, 128])[0], 127);
    }
}
