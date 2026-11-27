//! Where the shell's regions go, from the window's size and the workspace (the design's
//! FullApp artboard at 1440 × 900: 16 px around, 12 px between regions).

use pencilpilot_ui::ui::Rect;

use crate::state::Workspace;

pub const MARGIN: f32 = 16.0;
pub const GAP: f32 = 12.0;
pub const HEADER_H: f32 = 56.0;
pub const TOOLS_W: f32 = 64.0;
pub const PANEL_W: f32 = 320.0;

/// The bottom strip's height in each workspace.
pub fn strip_height(w: Workspace) -> f32 {
    match w {
        // the timeline: its two toolbars and the grid (four layers and the sound row)
        Workspace::Animate => 268.0,
        Workspace::Draw => 52.0,
        Workspace::Colour => 132.0,
        Workspace::Review => 118.0,
    }
}

/// Whether the workspace shows the tool column.
pub fn has_tools(w: Workspace) -> bool {
    !matches!(w, Workspace::Review)
}

/// The shell's regions, logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Regions {
    pub header: Rect,
    pub tools: Option<Rect>,
    /// the white card the drawing sits in
    pub canvas: Rect,
    pub panel: Rect,
    pub strip: Rect,
}

pub fn regions(width: f32, height: f32, w: Workspace) -> Regions {
    let inner = Rect::new(
        MARGIN,
        MARGIN,
        (width - 2.0 * MARGIN).max(0.0),
        (height - 2.0 * MARGIN).max(0.0),
    );
    let header = Rect::new(inner.x, inner.y, inner.w, HEADER_H);
    let sh = strip_height(w);
    let strip = Rect::new(inner.x, inner.bottom() - sh, inner.w, sh);
    let body_y = header.bottom() + GAP;
    let body_h = (strip.y - GAP - body_y).max(0.0);
    let tools = has_tools(w).then(|| Rect::new(inner.x, body_y, TOOLS_W, body_h));
    let panel = Rect::new(inner.right() - PANEL_W, body_y, PANEL_W, body_h);
    let left = tools.map_or(inner.x, |t| t.right() + GAP);
    let canvas = Rect::new(left, body_y, (panel.x - GAP - left).max(0.0), body_h);
    Regions {
        header,
        tools,
        canvas,
        panel,
        strip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_fill_the_window_without_overlapping() {
        for w in Workspace::ALL {
            let r = regions(1440.0, 900.0, w);
            assert_eq!(r.header.x, MARGIN);
            assert!(
                r.canvas.w > 600.0 && r.canvas.h > 300.0,
                "{w:?}: {:?}",
                r.canvas
            );
            assert!(r.canvas.right() + GAP <= r.panel.x + 0.01);
            assert!(r.canvas.bottom() + GAP <= r.strip.y + 0.01);
            if let Some(t) = r.tools {
                assert!(t.right() + GAP <= r.canvas.x + 0.01);
            }
            assert!((r.strip.bottom() - (900.0 - MARGIN)).abs() < 0.01);
        }
    }
}
