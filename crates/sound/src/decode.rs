//! A sound file's bytes to [`Pcm`] at [`RATE`]: Symphonia (pure Rust) reads WAV, AIFF, MP3,
//! FLAC, Ogg Vorbis, AAC-LC (raw ADTS or in MP4/M4A), ALAC and Vorbis in WebM/Matroska. What it
//! can't (Opus, in Ogg, WebM or `.opus` files; HE-AAC), FFmpeg decodes when it's installed.
//! Anything at another rate is resampled once, here (a windowed-sinc filter), so mixing and
//! export never resample.

use std::io::{Cursor, Read};
use std::path::Path;
use std::process::{Command, Stdio};

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::{Pcm, RATE};

/// The longest sound decoded (an hour: 690 MB at 48 kHz stereo).
pub const MAX_SECONDS: u64 = 3600;

/// Decodes a sound file (`ext`: its extension, a hint for what it is). With `ffmpeg`, what
/// Symphonia can't read goes through it.
pub fn decode(bytes: &[u8], ext: &str, ffmpeg: Option<&Path>) -> Result<Pcm, String> {
    match symphonia_decode(bytes, ext) {
        Ok((rate, chans)) => Ok(to_pcm(rate, chans)),
        Err(e) => match ffmpeg {
            Some(ff) => ffmpeg_decode(ff, bytes, ext).map_err(|f| format!("{e}; FFmpeg: {f}")),
            None => Err(e),
        },
    }
}

/// Symphonia's decoding: the rate and each channel's samples (at most two channels kept).
fn symphonia_decode(bytes: &[u8], ext: &str) -> Result<(u32, Vec<Vec<f32>>), String> {
    let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    if !ext.is_empty() {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| format!("not a sound file this can read ({e})"))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or("no sound in the file")?;
    let id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("no sound in the file")?
        .clone();
    let mut dec = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| format!("its sound is in a form this can't decode ({e})"))?;
    let mut rate = params.sample_rate.unwrap_or(0);
    let mut chans: Vec<Vec<f32>> = Vec::new();
    let mut inter: Vec<f32> = Vec::new();
    let mut limit = usize::MAX;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            // (a damaged end: what came before is kept)
            Err(SymError::IoError(_)) => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id != id {
            continue;
        }
        let buf = match dec.decode(&packet) {
            Ok(b) => b,
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        };
        let n = buf.spec().channels().count().max(1);
        if rate == 0 {
            rate = buf.spec().rate();
        }
        if chans.is_empty() {
            chans = vec![Vec::new(); n.min(2)];
            limit = (MAX_SECONDS * rate.max(1) as u64) as usize;
        }
        buf.copy_to_vec_interleaved(&mut inter);
        for (c, ch) in chans.iter_mut().enumerate() {
            ch.extend(inter.iter().skip(c).step_by(n));
        }
        if chans[0].len() > limit {
            return Err("it's longer than an hour".into());
        }
    }
    if chans.is_empty() || chans[0].is_empty() || rate == 0 {
        return Err("no sound in the file".into());
    }
    Ok((rate, chans))
}

/// Decoding through FFmpeg (to 48 kHz stereo floats). The bytes go through a file (MP4's
/// index may be at its end, which a pipe can't reach).
fn ffmpeg_decode(ff: &Path, bytes: &[u8], ext: &str) -> Result<Pcm, String> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let ext: String = ext.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let tmp = std::env::temp_dir().join(format!(
        "pencilpilot-sound-{}-{}.{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed),
        if ext.is_empty() { "bin" } else { &ext }
    ));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    let out = Command::new(ff)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(&tmp)
        .args(["-vn", "-f", "f32le", "-ac", "2", "-ar"])
        .arg(RATE.to_string())
        .arg("pipe:1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            let mut v = Vec::new();
            let limit = MAX_SECONDS * RATE as u64 * 8;
            c.stdout
                .take()
                .map(|o| o.take(limit + 8).read_to_end(&mut v));
            let mut err = String::new();
            if let Some(mut e) = c.stderr.take() {
                let _ = e.read_to_string(&mut err);
            }
            let st = c.wait()?;
            Ok((st, v, err))
        });
    let _ = std::fs::remove_file(&tmp);
    let (status, raw, err) = out.map_err(|e| e.to_string())?;
    if !status.success() || raw.len() < 8 {
        return Err(err.lines().next().unwrap_or("it couldn't decode it").into());
    }
    if raw.len() as u64 > MAX_SECONDS * RATE as u64 * 8 {
        return Err("it's longer than an hour".into());
    }
    let n = raw.len() / 8;
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for f in raw.as_chunks::<8>().0 {
        left.push(f32::from_le_bytes([f[0], f[1], f[2], f[3]]));
        right.push(f32::from_le_bytes([f[4], f[5], f[6], f[7]]));
    }
    Ok(mono_if_same(left, right))
}

/// Two identical channels kept as one.
fn mono_if_same(left: Vec<f32>, right: Vec<f32>) -> Pcm {
    if left == right {
        Pcm { left, right: None }
    } else {
        Pcm {
            left,
            right: Some(right),
        }
    }
}

fn to_pcm(rate: u32, mut chans: Vec<Vec<f32>>) -> Pcm {
    let mut chans: Vec<Vec<f32>> = chans
        .drain(..)
        .map(|c| {
            if rate == RATE {
                c
            } else {
                resample(&c, rate, RATE)
            }
        })
        .collect();
    let right = (chans.len() > 1).then(|| chans.pop().unwrap_or_default());
    let left = chans.pop().unwrap_or_default();
    match right {
        Some(r) => mono_if_same(left, r),
        None => Pcm { left, right: None },
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The modified Bessel function I0 (for the Kaiser window).
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    let q = x * x / 4.0;
    for k in 1..50 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < sum * 1e-12 {
            break;
        }
    }
    sum
}

/// Phases a resampling filter's table has at most (rates whose ratio needs more use the
/// nearest of this many).
const PHASES_MAX: u64 = 4096;
/// Zero crossings each side of the filter (at the input's rate, or the output's when
/// downsampling).
const HALF: f64 = 16.0;

/// One channel from `from` Hz to `to` Hz: a Kaiser-windowed sinc (β 8.6, 16 zero crossings a
/// side, cut off at 95 % of the lower Nyquist frequency), from a table of the filter's
/// phases; on every core.
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    use rayon::prelude::*;
    if x.is_empty() || from == to || from == 0 || to == 0 {
        return x.to_vec();
    }
    let (from, to) = (from as u64, to as u64);
    let g = gcd(from, to);
    let (up, down) = (to / g, from / g);
    // exact phases when the ratio allows, else the nearest of PHASES_MAX
    let phases = up.min(PHASES_MAX);
    let fc = (to as f64 / from as f64).min(1.0) * 0.95;
    let half = (HALF / fc).ceil() as i64;
    let taps = (2 * half) as usize;
    let beta = 8.6;
    let i0b = bessel_i0(beta);
    let mut table = vec![0f32; phases as usize * taps];
    for p in 0..phases as usize {
        let frac = p as f64 / phases as f64;
        let row = &mut table[p * taps..(p + 1) * taps];
        let mut sum = 0.0;
        let mut w = vec![0f64; taps];
        for (k, v) in w.iter_mut().enumerate() {
            let t = (k as i64 - half + 1) as f64 - frac;
            let u = t / half as f64;
            if u.abs() >= 1.0 {
                continue;
            }
            let s = if t == 0.0 {
                1.0
            } else {
                let a = std::f64::consts::PI * fc * t;
                a.sin() / a
            };
            *v = fc * s * bessel_i0(beta * (1.0 - u * u).sqrt()) / i0b;
            sum += *v;
        }
        for (r, v) in row.iter_mut().zip(&w) {
            *r = (*v / sum) as f32;
        }
    }
    let n_out = ((x.len() as u64 * to).div_ceil(from)) as usize;
    let mut out = vec![0f32; n_out];
    let len = x.len() as i64;
    out.par_chunks_mut(1 << 14)
        .enumerate()
        .for_each(|(ci, chunk)| {
            for (j, o) in chunk.iter_mut().enumerate() {
                let n = (ci * (1 << 14) + j) as u64;
                let (i, p) = if phases == up {
                    ((n * down / up) as i64, (n * down % up) as usize)
                } else {
                    let pos = n as f64 * from as f64 / to as f64;
                    let i = pos.floor();
                    let p = ((pos - i) * phases as f64).round() as u64;
                    if p >= phases {
                        (i as i64 + 1, 0)
                    } else {
                        (i as i64, p as usize)
                    }
                };
                let row = &table[p * taps..(p + 1) * taps];
                let first = i - half + 1;
                let mut acc = 0f32;
                if first >= 0 && first + taps as i64 <= len {
                    let s = &x[first as usize..first as usize + taps];
                    for (a, b) in s.iter().zip(row) {
                        acc += a * b;
                    }
                } else {
                    for (k, w) in row.iter().enumerate() {
                        let at = first + k as i64;
                        if (0..len).contains(&at) {
                            acc += x[at as usize] * w;
                        }
                    }
                }
                *o = acc;
            }
        });
    out
}

/// A 16-bit PCM WAV file's bytes (for tests and fixtures): `chans` channels interleaved.
pub fn wav_i16(samples: &[i16], chans: u16, rate: u32) -> Vec<u8> {
    let data = samples.len() as u32 * 2;
    let mut w = Vec::with_capacity(44 + data as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&chans.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * chans as u32 * 2).to_le_bytes());
    w.extend_from_slice(&(chans * 2).to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        w.extend_from_slice(&s.to_le_bytes());
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_files_decode_exactly() {
        let s: Vec<i16> = (0..4800).map(|i| ((i * 37) % 2000 - 1000) as i16).collect();
        let st: Vec<i16> = s.iter().flat_map(|v| [*v, -*v]).collect();
        let p = decode(&wav_i16(&st, 2, RATE), "wav", None).unwrap();
        assert_eq!(p.len(), 4800);
        for (i, v) in s.iter().enumerate() {
            assert!((p.left[i] - *v as f32 / 32768.0).abs() < 1e-6);
            assert!((p.right()[i] + *v as f32 / 32768.0).abs() < 1e-6);
        }
        // mono stays one channel
        let m = decode(&wav_i16(&s, 1, RATE), "wav", None).unwrap();
        assert!(m.right.is_none());
        assert_eq!(m.len(), 4800);
    }

    #[test]
    fn other_rates_are_resampled_keeping_pitch_and_level() {
        // a 1 kHz tone at 44.1 kHz, one second
        let n = 44_100;
        let s: Vec<i16> = (0..n)
            .map(|i| {
                ((i as f64 * 1000.0 * std::f64::consts::TAU / 44_100.0).sin() * 16_000.0) as i16
            })
            .collect();
        let p = decode(&wav_i16(&s, 1, 44_100), "wav", None).unwrap();
        assert_eq!(p.len(), 48_000);
        // compared with the tone at 48 kHz, away from the ends
        let mut err: f64 = 0.0;
        for (i, v) in p.left.iter().enumerate().take(47_000).skip(1000) {
            let want =
                (i as f64 * 1000.0 * std::f64::consts::TAU / 48_000.0).sin() * 16_000.0 / 32768.0;
            err = err.max((*v as f64 - want).abs());
        }
        assert!(err < 2e-3, "{err}");
    }

    #[test]
    fn what_isnt_sound_is_refused() {
        assert!(decode(b"not a sound at all, really", "mp3", None).is_err());
        assert!(decode(b"RIFF....", "wav", None).is_err());
    }

    /// The formats the current app takes, made by FFmpeg (when it's here): which decode.
    #[test]
    fn formats_the_current_app_takes() {
        let Some(ff) = which_ffmpeg() else { return };
        let dir = std::env::temp_dir().join(format!("pp-sound-fmt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut report = Vec::new();
        for (ext, args) in [
            ("mp3", vec!["-c:a", "libmp3lame"]),
            ("ogg", vec!["-c:a", "libvorbis"]),
            ("oga", vec!["-c:a", "flac"]),
            ("opus", vec!["-c:a", "libopus"]),
            ("m4a", vec!["-c:a", "aac"]),
            ("aac", vec!["-c:a", "aac"]),
            ("flac", vec!["-c:a", "flac"]),
            ("webm", vec!["-c:a", "libopus"]),
        ] {
            let path = dir.join(format!("t.{ext}"));
            let ok = Command::new(&ff)
                .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi"])
                .args(["-i", "sine=frequency=440:duration=1:sample_rate=44100"])
                .args(&args)
                .arg(&path)
                .status()
                .is_ok_and(|s| s.success());
            if !ok {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            let own = symphonia_decode(&bytes, ext).is_ok();
            let p = decode(&bytes, ext, Some(&ff)).unwrap_or_else(|e| panic!("{ext}: {e}"));
            // about a second (encoders pad a little)
            assert!(
                (47_000..50_500).contains(&p.len()),
                "{ext}: {} samples",
                p.len()
            );
            report.push(format!(
                "{ext}: {}",
                if own { "symphonia" } else { "ffmpeg" }
            ));
        }
        let _ = std::fs::remove_dir_all(&dir);
        eprintln!("{}", report.join(", "));
    }

    fn which_ffmpeg() -> Option<std::path::PathBuf> {
        std::env::var_os("PATH").and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("ffmpeg"))
                .find(|f| f.is_file())
        })
    }
}
