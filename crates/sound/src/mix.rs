//! Clips placed on a scene's frames, mixed: sample-exactly for an export (the current app's
//! `mixdown`), as 16-bit stereo WAV (its `wav`).
//!
//! An export is a run of parts, one after another, each a stretch of a scene: its clips,
//! from a scene frame, for some frames. A clip starting before the stretch is heard from
//! where the stretch is in it; one starting later comes in on its frame.

use std::io::{self, Write};
use std::sync::Arc;

use crate::{Pcm, RATE, frame_sample};

/// A sound placed in a scene.
#[derive(Debug, Clone)]
pub struct Clip {
    pub pcm: Arc<Pcm>,
    /// the scene frame it starts on (negative: before the scene)
    pub start: i64,
    /// loudness (1: as recorded)
    pub gain: f32,
    pub muted: bool,
}

impl Clip {
    fn heard(&self) -> bool {
        !self.muted && self.gain > 0.0 && !self.pcm.is_empty()
    }
}

/// A stretch of a scene in an export.
#[derive(Debug, Clone, Copy)]
pub struct Part<'a> {
    pub clips: &'a [Clip],
    /// the scene frame it starts on
    pub from: i64,
    pub frames: u32,
}

/// Where each part's sound starts in the mix (samples), and the mix's length: the parts'
/// frames at `fps`, rounded once (so parts don't drift from the picture).
fn bounds(parts: &[Part], fps: u32) -> Vec<usize> {
    let mut at = Vec::with_capacity(parts.len() + 1);
    let mut frames = 0i64;
    at.push(0);
    for p in parts {
        frames += p.frames as i64;
        at.push(frame_sample(frames, fps).max(0) as usize);
    }
    at
}

/// The mix's length in samples (per channel).
pub fn len(parts: &[Part], fps: u32) -> usize {
    *bounds(parts, fps).last().unwrap_or(&0)
}

/// Whether anything is heard (a clip not muted, with sound).
pub fn audible(parts: &[Part]) -> bool {
    parts.iter().any(|p| p.clips.iter().any(Clip::heard))
}

/// Mixes samples `[from, from + out.len() / 2)` of the whole mix into `out` (stereo,
/// interleaved; it's cleared first).
pub fn mix_block(parts: &[Part], fps: u32, from: usize, out: &mut [f32]) {
    out.fill(0.0);
    let n = out.len() / 2;
    let to = from + n;
    let at = bounds(parts, fps);
    for (i, p) in parts.iter().enumerate() {
        let (a, b) = (at[i], at[i + 1]);
        let (lo, hi) = (a.max(from), b.min(to));
        if lo >= hi {
            continue;
        }
        // the part's first output sample is this scene sample
        let scene0 = frame_sample(p.from, fps);
        for c in p.clips.iter().filter(|c| c.heard()) {
            let c0 = frame_sample(c.start, fps);
            let (l, r) = (&c.pcm.left, c.pcm.right());
            // output k ↔ clip sample scene0 + (k − a) − c0
            let off = scene0 - a as i64 - c0;
            let k0 = (lo as i64).max(-off) as usize;
            let k1 = (hi as i64).min(l.len() as i64 - off).max(0) as usize;
            for k in k0.max(lo)..k1.min(hi) {
                let s = (k as i64 + off) as usize;
                let o = (k - from) * 2;
                out[o] += l[s] * c.gain;
                out[o + 1] += r[s] * c.gain;
            }
        }
    }
}

/// The whole mix (stereo, interleaved), or None when nothing is heard.
pub fn mixdown(parts: &[Part], fps: u32) -> Option<Vec<f32>> {
    if !audible(parts) {
        return None;
    }
    let mut out = vec![0f32; len(parts, fps).max(1) * 2];
    mix_block(parts, fps, 0, &mut out);
    Some(out)
}

/// A sample as 16 bits, as the current app writes them (clipped; full scale is 32 767 up and
/// 32 768 down).
pub fn to_i16(s: f32) -> i16 {
    let s = s.clamp(-1.0, 1.0);
    if s < 0.0 {
        (s * 32768.0) as i16
    } else {
        (s * 32767.0) as i16
    }
}

/// A 16-bit stereo WAV header for `n` samples a channel.
pub fn wav_header(n: usize) -> [u8; 44] {
    let data = (n * 4) as u32;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&2u16.to_le_bytes());
    h[24..28].copy_from_slice(&RATE.to_le_bytes());
    h[28..32].copy_from_slice(&(RATE * 4).to_le_bytes());
    h[32..34].copy_from_slice(&4u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

/// A mix (stereo, interleaved) as a WAV file's bytes.
pub fn wav(mix: &[f32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(44 + mix.len() * 2);
    v.extend_from_slice(&wav_header(mix.len() / 2));
    for s in mix {
        v.extend_from_slice(&to_i16(*s).to_le_bytes());
    }
    v
}

/// Writes the mix as a WAV file a second at a time (a long export never holds it all).
/// Returns the samples a channel written.
pub fn write_wav<W: Write>(mut w: W, parts: &[Part], fps: u32) -> io::Result<usize> {
    let n = len(parts, fps).max(1);
    w.write_all(&wav_header(n))?;
    let block = RATE as usize;
    let mut buf = vec![0f32; block * 2];
    let mut bytes = Vec::with_capacity(block * 4);
    let mut at = 0;
    while at < n {
        let k = block.min(n - at);
        mix_block(parts, fps, at, &mut buf[..k * 2]);
        bytes.clear();
        for s in &buf[..k * 2] {
            bytes.extend_from_slice(&to_i16(*s).to_le_bytes());
        }
        w.write_all(&bytes)?;
        at += k;
    }
    w.flush()?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clip whose sample k is `(k + 1) / 1e6` on the left (and minus that on the right):
    /// where each sample lands in the mix shows exactly.
    fn ramp(n: usize, start: i64) -> Clip {
        let left: Vec<f32> = (0..n).map(|k| (k + 1) as f32 * 1e-6).collect();
        let right = left.iter().map(|v| -v).collect();
        Clip {
            pcm: Arc::new(Pcm {
                left,
                right: Some(right),
            }),
            start,
            gain: 1.0,
            muted: false,
        }
    }

    /// The clip sample (1-based, 0: silence) at each mix sample, from the left channel.
    fn which(mix: &[f32]) -> Vec<i64> {
        mix.chunks(2)
            .map(|s| (s[0] as f64 * 1e6).round() as i64)
            .collect()
    }

    #[test]
    fn clips_land_on_their_frames_to_the_sample() {
        // 24 fps: 2000 samples a frame; a clip on scene frame 3, a part of frames 0–9
        let c = [ramp(5000, 3)];
        let parts = [Part {
            clips: &c,
            from: 0,
            frames: 10,
        }];
        let mix = mixdown(&parts, 24).unwrap();
        assert_eq!(mix.len() / 2, 20_000);
        let w = which(&mix);
        assert!(w[..6000].iter().all(|v| *v == 0));
        assert_eq!(w[6000], 1);
        assert_eq!(w[10_999], 5000);
        assert!(w[11_000..].iter().all(|v| *v == 0));
        // the right channel is the right's
        assert!((mix[6000 * 2 + 1] + 1e-6).abs() < 1e-9);
    }

    #[test]
    fn a_negative_start_is_heard_from_where_the_scene_is_in_it() {
        // 25 fps: 1920 samples a frame; the clip began 2 frames before the scene
        let c = [ramp(20_000, -2)];
        let parts = [Part {
            clips: &c,
            from: 0,
            frames: 4,
        }];
        let w = which(&mixdown(&parts, 25).unwrap());
        assert_eq!(w.len(), 7680);
        assert_eq!(w[0], 3841);
        assert_eq!(w[7679], 3840 + 7680);
    }

    #[test]
    fn cuts_play_one_after_another_under_one_clip() {
        // a scene's cuts 0–9 and 10–19; the export plays frames 5–9 of the first cut, then
        // frames 12–15 of the scene (another stretch): music runs under both, from where each
        // stretch is in it
        let c = [ramp(100_000, 0)];
        let parts = [
            Part {
                clips: &c,
                from: 5,
                frames: 5,
            },
            Part {
                clips: &c,
                from: 12,
                frames: 4,
            },
        ];
        let w = which(&mixdown(&parts, 24).unwrap());
        assert_eq!(w.len(), 9 * 2000);
        assert_eq!(w[0], 5 * 2000 + 1);
        assert_eq!(w[9999], 10 * 2000);
        // the second part jumps to scene frame 12
        assert_eq!(w[10_000], 12 * 2000 + 1);
        assert_eq!(w[17_999], 16 * 2000);
    }

    #[test]
    fn gain_mute_and_several_clips_add_up() {
        let mut a = ramp(4000, 0);
        a.gain = 0.5;
        let b = ramp(4000, 1);
        let mut m = ramp(4000, 0);
        m.muted = true;
        let c = [a, b, m];
        let parts = [Part {
            clips: &c,
            from: 0,
            frames: 2,
        }];
        let mix = mixdown(&parts, 24).unwrap();
        // sample 2500: a's 2501st at half, b's 501st
        let want = 2501.0 * 1e-6 * 0.5 + 501.0 * 1e-6;
        assert!((mix[2500 * 2] - want).abs() < 1e-9);
        // nothing heard: no mix
        let mut m2 = ramp(10, 0);
        m2.muted = true;
        let only = [m2];
        assert!(
            mixdown(
                &[Part {
                    clips: &only,
                    from: 0,
                    frames: 3
                }],
                24
            )
            .is_none()
        );
    }

    #[test]
    fn mono_plays_on_both_sides_and_blocks_match_the_whole() {
        let c = [Clip {
            pcm: Arc::new(Pcm {
                left: (0..90_000)
                    .map(|k| ((k % 97) as f32 - 48.0) / 50.0)
                    .collect(),
                right: None,
            }),
            start: -7,
            gain: 0.8,
            muted: false,
        }];
        let parts = [
            Part {
                clips: &c,
                from: 3,
                frames: 30,
            },
            Part {
                clips: &c,
                from: 40,
                frames: 13,
            },
        ];
        let whole = mixdown(&parts, 30).unwrap();
        assert!(whole.chunks(2).all(|s| s[0] == s[1]));
        let mut wav_bytes = Vec::new();
        let n = write_wav(&mut wav_bytes, &parts, 30).unwrap();
        assert_eq!(n, whole.len() / 2);
        assert_eq!(wav_bytes, wav(&whole));
    }

    #[test]
    fn wav_is_16_bit_stereo_as_the_current_app_writes_it() {
        let w = wav(&[0.0, 0.0, 1.0, 1.0, -1.0, -1.0, 0.5, 0.5]);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([w[22], w[23]]), 2);
        assert_eq!(u32::from_le_bytes([w[24], w[25], w[26], w[27]]), 48_000);
        assert_eq!(w.len(), 44 + 16);
        assert_eq!(i16::from_le_bytes([w[48], w[49]]), 32767);
        assert_eq!(i16::from_le_bytes([w[52], w[53]]), -32768);
        assert_eq!(i16::from_le_bytes([w[56], w[57]]), 16383);
    }
}
