//! Exporting the cut: video (MP4 and the like, through FFmpeg), GIF, a PNG sequence; the
//! export dialog's settings and its progress.

use super::Engine;
use crate::state::{Action, State};

/// An export in progress, between calls.
#[derive(Debug, Default)]
pub struct Exporter {}

/// What the interface shows (in [`State::export`]): the dialog's settings, progress.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportUi {}

/// An export action (Export…, the dialog's buttons). Returns whether it was one and the
/// picture may have changed.
pub fn apply(_e: &mut Engine, _s: &mut State, _a: &Action) -> bool {
    false
}

/// Once a loop: an export's progress. Returns whether the interface should redraw.
pub fn tick(_e: &mut Engine, _s: &mut State, _now: std::time::Instant) -> bool {
    false
}

/// When [`tick`] next has something to do.
pub fn next_tick_at(_e: &Engine, _s: &State) -> Option<std::time::Instant> {
    None
}
