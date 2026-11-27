//! The tool column (dark), with the colour swatch at its foot.
//! (A working stub: Agent "header and tools" builds it to the design.)

use pencilpilot_gpu::Quad;
use pencilpilot_ui::ui::{Frame, Rect};

use crate::state::{State, Tool};

const BRUSH: &str = "M4 20l1-4L15 6l3 3L8 19Z M13 8l3 3";
const ERASER: &str = "M4 15l8-8 7 7-6 6H8Z M11 20h9";
const FILL: &str = "M5 12l7-7 7 7-7 7Z M19 15c1 2 2 3 2 4a2 2 0 0 1-4 0c0-1 1-2 2-4Z";

pub fn build(f: &mut Frame, r: Rect, s: &mut State) {
    let t = f.theme();
    f.put(Quad::rect(r.x, r.y, r.w, r.h, t.dark).radius(24.0));
    f.column_in(
        Rect::new(r.x + 15.0, r.y + 10.0, 34.0, r.h - 20.0),
        8.0,
        |f| {
            for (name, path, tool) in [
                ("Brush", BRUSH, Tool::Brush),
                ("Eraser", ERASER, Tool::Eraser),
                ("Fill", FILL, Tool::Fill),
            ] {
                if f.icon_button(&format!("tool/{name}"), path, name, s.tool == tool, true)
                    .clicked
                {
                    s.tool = tool;
                }
            }
        },
    );
}
