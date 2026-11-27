//! Layers, drawings and their exposures on the timeline, onion skin, playback, and the
//! picture the canvas shows.

use super::Engine;
use crate::state::{Action, State};

/// What playback and the drawings keep between frames.
#[derive(Debug, Default)]
pub struct Frames {}

/// An action about layers, drawings or frames. Returns whether it was one and the picture
/// may have changed.
pub fn apply(_e: &mut Engine, _s: &mut State, _a: &Action) -> bool {
    false
}

/// Playback: advances the frame on time. Returns whether the picture changed.
pub fn tick(_e: &mut Engine, _s: &mut State, _now: std::time::Instant) -> bool {
    false
}

/// When [`tick`] next has something to do.
pub fn next_tick_at(_e: &Engine, _s: &State) -> Option<std::time::Instant> {
    None
}

/// The picture the canvas shows and what changed in it: for now, the painter's.
pub fn picture<'a>(e: &'a mut Engine, _s: &State) -> Option<(&'a [u8], [u32; 4])> {
    let d = e.painter.flush()?;
    Some((e.painter.rgba(), [d.x, d.y, d.w, d.h]))
}

/// The cut's exposure sheet as the interface shows it (in [`State::sheet`]): each layer's
/// cels (drawing, start, hold, mark).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sheet {}
