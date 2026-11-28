//! A project's sounds: each file decoded once (off the window's thread) to 48 kHz, its
//! waveform for the timeline, played in step with the picture, and mixed for a video export.
//!
//! - [`decode`]: a sound file's bytes to [`Pcm`] (Symphonia; FFmpeg for what it can't read);
//! - [`mix`]: clips placed on a scene's frames, mixed sample-exactly (exports), and WAV;
//! - [`peaks`]: the waveform the timeline draws (the loudest point of each frame);
//! - [`player`]: playing them: a real-time mixer behind the system's sound output (cpal) or a
//!   silent one for tests and checks, following the picture's clock.
//!
//! Clips are placed by scene frame (a clip's `start`, negative when it began before the
//! scene); a scene's cuts play one after another, so music runs under several cuts. This is
//! the current app's behaviour (`apps/studio/src/lib/audio.ts`).

pub mod decode;
pub mod mix;
pub mod peaks;
pub mod player;

/// Every sound is kept, mixed and played at this rate (the export's too).
pub const RATE: u32 = 48_000;

/// A decoded sound at [`RATE`]: one channel (mono) or two.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pcm {
    pub left: Vec<f32>,
    /// None: mono (the left channel plays on both sides)
    pub right: Option<Vec<f32>>,
}

impl Pcm {
    /// Its length in samples (per channel).
    pub fn len(&self) -> usize {
        self.left.len()
    }

    pub fn is_empty(&self) -> bool {
        self.left.is_empty()
    }

    /// The right channel (the left one for mono).
    pub fn right(&self) -> &[f32] {
        self.right.as_deref().unwrap_or(&self.left)
    }

    /// Its length in frames at `fps`, rounded up (as the current app shows a sound).
    pub fn frames(&self, fps: u32) -> u32 {
        (self.len() as u64 * fps.max(1) as u64).div_ceil(RATE as u64) as u32
    }

    /// Bytes its samples take.
    pub fn bytes(&self) -> usize {
        (self.left.len() + self.right.as_ref().map_or(0, Vec::len)) * 4
    }
}

/// A scene frame's first sample at [`RATE`] (rounded half up; exact for 12, 24, 25, 30, 60
/// and every rate dividing 48 000).
pub fn frame_sample(frame: i64, fps: u32) -> i64 {
    let fps = fps.max(1) as i128;
    ((frame as i128 * RATE as i128 * 2 + fps).div_euclid(2 * fps)) as i64
}
