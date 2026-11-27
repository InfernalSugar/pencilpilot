//! The canvas tools other than the Brush and the Eraser (Fill, Lasso, Select, Transform,
//! Eyedropper, Shape, Text), the selection's and the canvas's menus, and the clipboard.

use super::{Engine, Phase, Sample};
use crate::state::{Action, State};

/// What these tools keep between events (a lasso's points, a transform in progress…).
#[derive(Debug, Default)]
pub struct Tools {}

/// A pointer on the canvas with one of these tools.
pub fn pointer(_e: &mut Engine, _s: &mut State, _phase: Phase, _p: Sample) {}

/// An action these tools carry out. Returns whether it was theirs and the picture may have
/// changed.
pub fn apply(_e: &mut Engine, _s: &mut State, _a: &Action) -> bool {
    false
}

/// What the interface shows of these tools (in [`State::tooling`]): the selection's outline,
/// a transform's box, a shape being drawn… in document pixels.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolsUi {}
