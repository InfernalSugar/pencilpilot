//! Projects on disk: New, Open, Save, Save as, autosave, recent files. A `.ppstudio` file
//! (`crates/ppstudio`) maps to and from the engine (layers, drawings, the sheet, palette…).

use super::Engine;
use crate::state::{Action, State};

/// What the engine keeps about the open project between calls.
#[derive(Debug, Default)]
pub struct Files {}

/// What the interface shows of the project (in [`State::project`]): its name, path, whether
/// it has unsaved changes, recent files…
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectUi {}

/// A File action (New, Open…, Save…). Returns whether it was one and the picture may have
/// changed.
pub fn apply(_e: &mut Engine, _s: &mut State, _a: &Action) -> bool {
    false
}

/// Once a loop: answers from the file dialog, autosave. Returns whether the picture changed.
pub fn tick(_e: &mut Engine, _s: &mut State, _now: std::time::Instant) -> bool {
    false
}

/// When [`tick`] next has something to do (autosave).
pub fn next_tick_at(_e: &Engine, _s: &State) -> Option<std::time::Instant> {
    None
}
