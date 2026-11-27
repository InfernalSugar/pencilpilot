//! The header: the title and File menu, the workspace tabs, Undo and Redo.
//! (A working stub: Agent "header and tools" builds it to the design.)

use pencilpilot_ui::ui::{Frame, Kind, Rect};

use crate::state::{Action, State, Workspace};

pub fn build(f: &mut Frame, r: Rect, s: &mut State) {
    f.row_in(r, 12.0, |f| {
        f.heading("PencilPilot");
        let mut tab = s.workspace.index();
        let names: Vec<&str> = Workspace::ALL.iter().map(|w| w.name()).collect();
        if f.tabs("workspace", &names, &mut tab).changed {
            s.workspace = Workspace::ALL[tab];
        }
        f.then_right();
        if f.button("redo", "Redo", Kind::Secondary).clicked {
            s.actions.push(Action::Redo);
        }
        if f.button("undo", "Undo", Kind::Secondary).clicked {
            s.actions.push(Action::Undo);
        }
    });
}
