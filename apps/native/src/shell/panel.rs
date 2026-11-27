//! The side panel: its tabs and their content change with the workspace.
//! (A working stub: Agent "side panel" builds it to the design.)

use pencilpilot_gpu::Quad;
use pencilpilot_ui::ui::{Frame, Rect};

use crate::state::State;

pub fn build(f: &mut Frame, r: Rect, s: &mut State) {
    let t = f.theme();
    f.put(Quad::rect(r.x, r.y, r.w, r.h, t.card).radius(24.0));
    let ws = s.workspace.index();
    f.column_in(r.inset(16.0), 14.0, |f| {
        let mut tab = s.panel_tab[ws];
        if f.tabs("panel", &["Brushes", "Colour", "Layers"], &mut tab)
            .changed
        {
            s.panel_tab[ws] = tab;
        }
        f.label(&format!("{} · {}", s.brushes[s.brush], s.workspace.name()));
    });
}
