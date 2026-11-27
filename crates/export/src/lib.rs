//! Writing an animation out (the current app's and the native app's exports): frames come
//! here one after another, rendered elsewhere; this end writes them as a PNG sequence, a GIF, an APNG, or a
//! video through ffmpeg (MP4 / H.264, ProRes 4444 with alpha, WebM / VP9).
//!
//! A frame comes with how many output frames it's held for (`repeat`), so a drawing on
//! twos crosses over once. GIF and APNG keep a held frame as one frame shown longer;
//! a PNG sequence and video write it out each time.
//!
//! The pixels are sRGB, and files say so (PNGs carry an sRGB chunk, videos sRGB / BT.709
//! colour tags). Text about the work (software, author, credits) goes into PNGs as
//! `tEXt` chunks and into videos as metadata; GIFs get none.

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
    /// video: the frames come at this size and FFmpeg scales them to `width` × `height`
    /// (None: they come at that size)
    #[serde(default)]
    pub input: Option<(u32, u32)>,
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
    /// the sound (a WAV file) to put in a video, if any
    #[serde(skip)]
    pub audio: Option<PathBuf>,
    /// video: how hard to squeeze (smaller files, softer lines)
    #[serde(default)]
    pub quality: Quality,
    /// what to write into the file about the work: (key, text), e.g. ("Author", "Ana")
    #[serde(default)]
    pub meta: Vec<(String, String)>,
    /// Videos: encode faster and with far less memory. MP4: x264's medium preset, sliced
    /// threads, two B-frames and references, a 5-frame lookahead (past 1080p: one reference,
    /// no B-frames or lookahead; 4K from YUV: 22 ms a frame and 0.33 GB in our exports,
    /// against 60 ms and 2 GB; files bigger at the same quality). WebM:
    /// no frames held back and four threads (VP9 `lag-in-frames` 0, `threads` 4: 1080p
    /// 0.27 GB instead of 0.35, 4K 0.86 GB instead of 1.1)
    #[serde(default)]
    pub lean: bool,
    /// Videos: frames come as YUV 4:2:0 (BT.709, limited range: [`rgba_to_yuv420`]) rather
    /// than RGBA, 2.7 times smaller through the pipe and in FFmpeg's queues
    #[serde(default)]
    pub yuv: bool,
    /// run FFmpeg at a lower priority than the app (the app's window stays smooth while it
    /// encodes on every core)
    #[serde(default)]
    pub background: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    #[default]
    Best,
    Good,
    Small,
}

impl Quality {
    /// x264's and VP9's constant quality (lower is better)
    fn crf(self, format: Format) -> u32 {
        match (format, self) {
            (Format::Webm, Quality::Best) => 28,
            (Format::Webm, Quality::Good) => 33,
            (Format::Webm, Quality::Small) => 40,
            (_, Quality::Best) => 16,
            (_, Quality::Good) => 20,
            (_, Quality::Small) => 26,
        }
    }
}

/// PNG's keywords are Latin-1, 1–79 characters; text goes in as Latin-1 where it can.
fn png_text(key: &str, text: &str) -> Option<(String, String)> {
    let key: String = key
        .chars()
        .filter(|c| (' '..='~').contains(c))
        .take(79)
        .collect();
    let text: String = text
        .chars()
        .map(|c| if (c as u32) < 256 { c } else { '?' })
        .collect();
    (!key.trim().is_empty()).then_some((key, text))
}

/// A PNG encoder's colour and text: sRGB, and the work's metadata.
fn tag_png<W: Write>(enc: &mut png::Encoder<'_, W>, meta: &[(String, String)]) {
    enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    for (k, v) in meta {
        if let Some((k, v)) = png_text(k, v) {
            let _ = enc.add_text_chunk(k, v);
        }
    }
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

/// How much lower a background export runs (Unix nice: 0 is normal, 19 the lowest).
pub const BACKGROUND_NICE: i32 = 15;

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
        if spec.yuv { "yuv420p" } else { "rgba" },
        "-s",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let (iw, ih) = spec.input.unwrap_or((spec.width, spec.height));
    a.push(format!("{iw}x{ih}"));
    a.extend(["-r".into(), spec.fps.to_string()]);
    if spec.yuv {
        // (the frames are BT.709 already: said so, FFmpeg leaves them as they are)
        a.extend(
            [
                "-colorspace",
                "bt709",
                "-color_range",
                "tv",
                "-color_primaries",
                "bt709",
                "-color_trc",
                "iec61966-2-1",
            ]
            .map(String::from),
        );
    }
    a.extend(["-i".into(), "-".into()]);
    if (iw, ih) != (spec.width, spec.height) {
        a.extend([
            "-vf".into(),
            format!("scale={}:{}:flags=lanczos", spec.width, spec.height),
        ]);
    }
    if let Some(wav) = &spec.audio {
        a.extend([
            "-i".into(),
            wav.to_string_lossy().into_owned(),
            "-map".into(),
            "0:v".into(),
            "-map".into(),
            "1:a".into(),
            "-shortest".into(),
        ]);
        let sound: &[&str] = match spec.format {
            Format::Mp4 => &["-c:a", "aac", "-b:a", "192k"],
            Format::Prores => &["-c:a", "pcm_s16le"],
            _ => &["-c:a", "libopus", "-b:a", "160k"],
        };
        a.extend(sound.iter().map(|s| s.to_string()));
    }
    let enc: &[&str] = match (spec.format, spec.transparent) {
        (Format::Mp4, _) => &[
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "{crf}",
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
            "{crf}",
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
            "{crf}",
            "-row-mt",
            "1",
        ],
        _ => &[],
    };
    let crf = spec.quality.crf(spec.format).to_string();
    let lean = spec.lean && spec.format == Format::Mp4;
    a.extend(enc.iter().map(|s| match *s {
        "slow" if lean => "medium".to_string(),
        s => s.replace("{crf}", &crf),
    }));
    if lean {
        // past 1080p every reference frame costs tens of MB: one, and none held back (4K:
        // FFmpeg at 0.33 GB instead of 0.57, as fast, files about 40% bigger at the same
        // quality)
        let big = spec.width as u64 * spec.height as u64 > 2 * 1920 * 1080;
        let p = if big {
            "sliced-threads=1:rc-lookahead=0:ref=1:bframes=0"
        } else {
            "sliced-threads=1:rc-lookahead=5:ref=2:bframes=2"
        };
        a.extend(["-x264-params".into(), p.into()]);
    }
    if spec.lean && spec.format == Format::Webm {
        // (all of libvpx's threads weigh on an app drawing at the display's rate: four)
        a.extend(["-lag-in-frames", "0", "-threads", "4"].map(String::from));
    }
    // the pixels are sRGB: say so (BT.709 primaries are sRGB's)
    a.extend(
        [
            "-color_primaries",
            "bt709",
            "-color_trc",
            "iec61966-2-1",
            "-colorspace",
            "bt709",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    for (k, v) in &spec.meta {
        let key = match k.as_str() {
            "Author" => "artist",
            "Software" => "encoder",
            "Copyright" => "copyright",
            "Creation Time" => "creation_time",
            "Title" => "title",
            _ => "comment",
        };
        a.extend(["-metadata".into(), format!("{key}={v}")]);
    }
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
        if let Some((iw, ih)) = spec.input
            && (!matches!(spec.format, Format::Mp4 | Format::Prores | Format::Webm)
                || iw == 0
                || ih == 0
                || iw > 16384
                || ih > 16384)
        {
            return Err("only videos are scaled while they're written".into());
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
                tag_png(&mut enc, &spec.meta);
                enc.set_animated(spec.frames, if spec.once { 1 } else { 0 })
                    .map_err(|e| e.to_string())?;
                let w = enc.write_header().map_err(|e| e.to_string())?;
                Writer::Apng {
                    enc: Box::new(w.into_stream_writer().map_err(|e| e.to_string())?),
                }
            }
            Format::Mp4 | Format::Prores | Format::Webm => {
                let ffmpeg = find_ffmpeg().ok_or("ffmpeg isn't installed")?;
                let mut cmd = Command::new(ffmpeg);
                #[cfg(unix)]
                if spec.background {
                    use std::os::unix::process::CommandExt;
                    // SAFETY: setpriority is async-signal-safe; it only lowers this child's
                    // priority (its threads inherit it)
                    unsafe {
                        cmd.pre_exec(|| {
                            libc::setpriority(libc::PRIO_PROCESS, 0, BACKGROUND_NICE);
                            Ok(())
                        });
                    }
                }
                let mut child = cmd
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
        let (iw, ih) = self.spec.input.unwrap_or((w, h));
        let video = matches!(self.writer, Writer::Video { .. });
        let want = if self.spec.yuv && video {
            yuv420_len(iw, ih)
        } else {
            (iw * ih * 4) as usize
        };
        if rgba.len() != want {
            return Err(format!("a frame of {} bytes for {iw}×{ih}", rgba.len()));
        }
        if self.sent >= self.spec.frames {
            return Err("more frames than were announced".into());
        }
        self.sent += 1;
        let repeat = repeat.max(1);
        let fps = self.spec.fps as u64;
        // opaque formats: anything see-through goes over white
        let flat;
        let rgba = if (self.spec.transparent && !matches!(self.spec.format, Format::Mp4))
            || (self.spec.yuv && video)
            || rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255)
        {
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
                    tag_png(&mut enc, &self.spec.meta);
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

    /// Where it goes (a PNG sequence: its first file's name, before the number).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Finish the file; returns where it went (a PNG sequence: its first file).
    pub fn end(self) -> Result<String, String> {
        if let Some(wav) = &self.spec.audio {
            // (ffmpeg has read it by the end; it goes either way)
            let wav = wav.clone();
            let r = self.finish();
            let _ = std::fs::remove_file(wav);
            return r;
        }
        self.finish()
    }

    fn finish(self) -> Result<String, String> {
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
        if let Some(wav) = &self.spec.audio {
            let _ = std::fs::remove_file(wav);
        }
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

/// Bytes of a `w` × `h` YUV 4:2:0 frame (chroma rounded up on odd sides).
pub fn yuv420_len(w: u32, h: u32) -> usize {
    let (w, h) = (w as usize, h as usize);
    w * h + 2 * w.div_ceil(2) * h.div_ceil(2)
}

/// Rows `2·pair` and `2·pair + 1` of straight RGBA8 (`w` wide, `h` tall; see-through goes
/// over white) as YUV 4:2:0, BT.709 limited range: their two luma rows and their one row of
/// each chroma. A frame is all its row pairs (on many threads at once, say), laid out as
/// [`rgba_to_yuv420`] does.
pub fn yuv420_row_pair(
    rgba: &[u8],
    w: u32,
    h: u32,
    pair: usize,
    y: &mut [u8],
    u: &mut [u8],
    v: &mut [u8],
) {
    let (w, h) = (w as usize, h as usize);
    let (kr, kb) = (0.2126f32, 0.0722f32);
    let kg = 1.0 - kr - kb;
    let rows = (2 * pair..(2 * pair + 2).min(h)).collect::<Vec<_>>();
    let px = |x: usize, row: usize| {
        let p = &rgba[(row * w + x) * 4..][..4];
        let a = p[3] as f32 / 255.0;
        let c = |i: usize| (p[i] as f32 * a + 255.0 * (1.0 - a)) / 255.0;
        (c(0), c(1), c(2))
    };
    for (k, &row) in rows.iter().enumerate() {
        for x in 0..w {
            let (r, g, b) = px(x, row);
            let l = kr * r + kg * g + kb * b;
            y[k * w + x] = (16.0 + 219.0 * l).round().clamp(0.0, 255.0) as u8;
        }
    }
    for cx in 0..w.div_ceil(2) {
        let (mut cb, mut cr, mut n) = (0.0f32, 0.0f32, 0.0f32);
        for &row in &rows {
            for x in (2 * cx)..(2 * cx + 2).min(w) {
                let (r, g, b) = px(x, row);
                let l = kr * r + kg * g + kb * b;
                cb += (b - l) / (2.0 * (1.0 - kb));
                cr += (r - l) / (2.0 * (1.0 - kr));
                n += 1.0;
            }
        }
        u[cx] = (128.0 + 224.0 * cb / n).round().clamp(0.0, 255.0) as u8;
        v[cx] = (128.0 + 224.0 * cr / n).round().clamp(0.0, 255.0) as u8;
    }
}

/// Straight RGBA8 as one YUV 4:2:0 frame (all of Y, then U, then V), BT.709 limited range,
/// for a video's [`Spec::yuv`].
pub fn rgba_to_yuv420(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (wu, hu) = (w as usize, h as usize);
    let (cw, ch) = (wu.div_ceil(2), hu.div_ceil(2));
    let mut out = vec![0u8; yuv420_len(w, h)];
    let (ys, uv) = out.split_at_mut(wu * hu);
    let (us, vs) = uv.split_at_mut(cw * ch);
    for pair in 0..ch {
        let y0 = 2 * pair * wu;
        let y1 = ((2 * pair + 2).min(hu)) * wu;
        yuv420_row_pair(
            rgba,
            w,
            h,
            pair,
            &mut ys[y0..y1],
            &mut us[pair * cw..(pair + 1) * cw],
            &mut vs[pair * cw..(pair + 1) * cw],
        );
    }
    out
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
            input: None,
            fps: 12,
            transparent: true,
            frames,
            total: 0,
            once: false,
            audio: None,
            quality: Quality::Best,
            meta: Vec::new(),
            lean: false,
            yuv: false,
            background: false,
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
    fn pngs_say_srgb_and_carry_the_credits() {
        let d = tmp();
        let mut s = spec(Format::Png, &d, "walk.png", 1);
        s.meta = vec![
            ("Software".into(), "PencilPilot".into()),
            ("Author".into(), "Zoë".into()),
            ("Credits".into(), "Drawn with Ana and 李".into()),
        ];
        let mut j = Job::begin(s).unwrap();
        j.frame(&frame(255, 255), 1).unwrap();
        let p = j.end().unwrap();
        let r = png::Decoder::new(std::io::BufReader::new(File::open(&p).unwrap()))
            .read_info()
            .unwrap();
        let info = r.info();
        assert!(info.srgb.is_some(), "an sRGB chunk");
        let text: Vec<_> = info
            .uncompressed_latin1_text
            .iter()
            .map(|t| (t.keyword.as_str(), t.text.as_str()))
            .collect();
        assert_eq!(
            text,
            [
                ("Software", "PencilPilot"),
                ("Author", "Zoë"),
                // (PNG text is Latin-1)
                ("Credits", "Drawn with Ana and ?")
            ]
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn videos_get_their_quality_colour_tags_and_metadata() {
        let d = tmp();
        let mut s = spec(Format::Mp4, &d, "a.mp4", 1);
        let args = |s: &Spec| ffmpeg_args(s, Path::new("/x/a.mp4")).join(" ");
        assert!(args(&s).contains("-crf 16"));
        assert!(args(&s).contains("-color_primaries bt709 -color_trc iec61966-2-1"));
        assert!(!args(&s).contains("-metadata"));
        s.quality = Quality::Small;
        s.meta = vec![("Author".into(), "Ana".into())];
        assert!(args(&s).contains("-crf 26"));
        assert!(args(&s).contains("-metadata artist=Ana"));
        assert!(!args(&s).contains("x264-params"));
        s.lean = true;
        assert!(args(&s).contains("-x264-params sliced-threads=1:rc-lookahead=5:ref=2:bframes=2"));
        assert!(args(&s).contains("-preset medium"));
        (s.width, s.height) = (3840, 2160);
        assert!(args(&s).contains("-x264-params sliced-threads=1:rc-lookahead=0:ref=1:bframes=0"));
        (s.width, s.height) = (8, 6);
        s.format = Format::Webm;
        s.quality = Quality::Good;
        assert!(args(&s).contains("-crf 33"));
        assert!(!args(&s).contains("x264-params"), "x264's only");
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
        // a second of silence-ish sound to go with them
        let wav = d.join("sound.wav");
        let mut w = Vec::new();
        let n: u32 = 48000;
        for (tag, v) in [(b"RIFF", 36 + n * 4), (b"WAVE", 0)] {
            w.extend_from_slice(tag);
            if tag == b"RIFF" {
                w.extend_from_slice(&v.to_le_bytes());
            }
        }
        w.extend_from_slice(b"fmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        for v in [1u16, 2] {
            w.extend_from_slice(&v.to_le_bytes());
        }
        for v in [48000u32, 48000 * 4] {
            w.extend_from_slice(&v.to_le_bytes());
        }
        for v in [4u16, 16] {
            w.extend_from_slice(&v.to_le_bytes());
        }
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(n * 4).to_le_bytes());
        w.extend(std::iter::repeat_n(0u8, (n * 4) as usize));
        for f in formats {
            std::fs::write(&wav, &w).unwrap();
            let mut s = spec(f, &d, "clip", 2);
            s.audio = Some(wav.clone());
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
    fn videos_are_scaled_from_the_size_frames_come_at() {
        let mut s = spec(Format::Mp4, Path::new("/x"), "a.mp4", 1);
        s.input = Some((16, 12));
        let a = ffmpeg_args(&s, Path::new("/x/a.mp4")).join(" ");
        assert!(
            a.contains("-s 16x12") && a.contains("-vf scale=8:6:flags=lanczos"),
            "{a}"
        );
        // only videos
        s.format = Format::Png;
        assert!(Job::begin(s).is_err());
        let Some(ff) = find_ffmpeg() else { return };
        if !video_formats(&ff).contains(&Format::Mp4) {
            return;
        }
        let d = tmp();
        let mut s = spec(Format::Mp4, &d, "half.mp4", 1);
        (s.width, s.height, s.input) = (16, 8, Some((32, 16)));
        let mut j = Job::begin(s).unwrap();
        // a wrong size is refused; the right one goes
        assert!(j.frame(&[0, 0, 0, 255].repeat(16 * 8), 1).is_err());
        j.frame(&[0, 0, 0, 255].repeat(32 * 16), 1).unwrap();
        let p = j.end().unwrap();
        let probe = ff.with_file_name("ffprobe");
        if let Ok(out) = Command::new(probe)
            .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
            .args(["stream=width,height", "-of", "csv=p=0", &p])
            .output()
        {
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "16,8");
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    /// YUV frames: BT.709's colours, and FFmpeg reads them back to the same picture.
    #[test]
    fn yuv_frames_are_bt709_and_go_through_ffmpeg() {
        // white, black, and pure red (BT.709 limited: Y 63, Cb 102, Cr 240)
        let px = [
            [255, 255, 255, 255],
            [0, 0, 0, 255],
            [255, 0, 0, 255],
            [0, 0, 0, 0],
        ];
        for (p, want) in px.iter().zip([
            [235, 128, 128],
            [16, 128, 128],
            [63, 102, 240],
            [235, 128, 128],
        ]) {
            let f = rgba_to_yuv420(&p.repeat(4), 2, 2);
            assert_eq!(f.len(), 6);
            assert_eq!([f[0], f[4], f[5]], want, "{p:?}");
        }
        // odd sides: chroma rounded up
        assert_eq!(rgba_to_yuv420(&[9; 3 * 3 * 4], 3, 3).len(), 9 + 2 * 4);
        let Some(ff) = find_ffmpeg() else { return };
        if !video_formats(&ff).contains(&Format::Mp4) {
            return;
        }
        let d = tmp();
        let mut s = spec(Format::Mp4, &d, "y.mp4", 1);
        (s.width, s.height, s.yuv, s.lean) = (32, 16, true, true);
        // four flat colours (H.264's loss on flat areas is small)
        let mut rgba = Vec::new();
        for y in 0..16 {
            for x in 0..32 {
                rgba.extend(match (x < 16, y < 8) {
                    (true, true) => [232, 73, 47, 255],
                    (false, true) => [90, 150, 210, 255],
                    (true, false) => [246, 244, 239, 255],
                    (false, false) => [22, 22, 26, 255],
                });
            }
        }
        let mut j = Job::begin(s).unwrap();
        assert!(j.frame(&rgba, 1).is_err(), "RGBA isn't taken");
        j.frame(&rgba_to_yuv420(&rgba, 32, 16), 1).unwrap();
        let p = j.end().unwrap();
        let out = Command::new(&ff)
            .args([
                "-v", "error", "-i", &p, "-f", "rawvideo", "-pix_fmt", "rgba", "-",
            ])
            .output()
            .unwrap();
        assert_eq!(out.stdout.len(), rgba.len());
        // each colour's middle (its edges ring a little)
        let worst = [(4, 3), (27, 3), (4, 12), (27, 12)]
            .iter()
            .flat_map(|&(x, y)| (0..3).map(move |c| (y * 32 + x) * 4 + c))
            .map(|i| (out.stdout[i] as i32 - rgba[i] as i32).abs())
            .max()
            .unwrap();
        assert!(worst <= 6, "decoded back within {worst}");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn see_through_pixels_go_over_white_when_flattened() {
        assert_eq!(over_white(&[0, 0, 0, 0]), [255, 255, 255, 255]);
        assert_eq!(over_white(&[0, 0, 0, 255]), [0, 0, 0, 255]);
        assert_eq!(over_white(&[0, 0, 0, 128])[0], 127);
    }
}
