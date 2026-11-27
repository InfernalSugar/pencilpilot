//! The bottom strip, one per workspace: the timeline (Animate), the frame strip (Draw), the
//! drawings to colour (Colour), the review player (Review).
//! (A working stub: Agent "bottom strips" builds the strips to the design; the full
//! timeline is step 4.)

use pencilpilot_gpu::Quad;
use pencilpilot_ui::text::Style;
use pencilpilot_ui::ui::{Frame, Rect};

use crate::state::State;

/// This piece's own state (anything it needs that the rest of the app doesn't).
#[derive(Debug, Clone, Default)]
pub struct StripsState {}

pub fn build(f: &mut Frame, r: Rect, s: &mut State) {
    let t = f.theme();
    f.put(Quad::rect(r.x, r.y, r.w, r.h, t.dark).radius(22.0));
    let laid = f.lay_out(
        &format!(
            "{} · frame {} of {}",
            s.workspace.name(),
            s.frame + 1,
            s.frames
        ),
        &Style::body(13.0, t.on_dark).weight(600),
        None,
    );
    f.put_text(&laid, r.x + 16.0, r.y + 14.0);
}
