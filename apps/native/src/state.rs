//! The app's state as the interface sees it, and the actions the interface asks for.
//!
//! The shell's pieces (header, tools, panel, strips) read and change this, and push
//! [`Action`]s for what the engine must do (undo, play, a new drawing…). The interface never
//! touches the engine directly.

// (some of it is for the pieces still being built)
#![allow(dead_code)]

/// The four workspaces: tool and panel layouts over the same canvas and drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Workspace {
    Animate,
    Draw,
    Colour,
    Review,
}

impl Workspace {
    pub const ALL: [Workspace; 4] = [
        Workspace::Draw,
        Workspace::Animate,
        Workspace::Colour,
        Workspace::Review,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Workspace::Animate => "Animate",
            Workspace::Draw => "Draw",
            Workspace::Colour => "Colour",
            Workspace::Review => "Review",
        }
    }

    pub fn index(self) -> usize {
        Workspace::ALL.iter().position(|w| *w == self).unwrap_or(0)
    }

    /// From a name, as `--render` takes it ("animate", "Draw"…).
    pub fn parse(s: &str) -> Option<Workspace> {
        Workspace::ALL
            .into_iter()
            .find(|w| w.name().eq_ignore_ascii_case(s))
    }
}

/// A tool in the tool column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    Brush,
    Eraser,
    Fill,
    Lasso,
    Select,
    Transform,
    Eyedropper,
    Text,
    Shape,
    Hand,
}

/// A layer as the panels and the timeline show it.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    /// 'C': a drawing per exposure (cels); 'S': one drawing all cut long (static)
    pub kind: char,
}

/// A review note pinned to a frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub author: String,
    pub text: String,
    pub frame: u32,
    /// the author's colour (straight RGBA)
    pub colour: [f32; 4],
    /// dealt with (it stays, greyed)
    pub resolved: bool,
}

/// A drawing's state in the Colour workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColourStatus {
    ToDo,
    Flatted,
    Done,
}

/// What the interface asks the app to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Undo,
    Redo,
    /// a File menu item, by its text ("New…", "Open…", "Save"…)
    File(String),
    NewDrawing,
    Play,
    Stop,
    PreviousFrame,
    NextFrame,
    GoToFrame(u32),
    PreviousDrawing,
    NextDrawing,
    NewLayer,
    /// anything else, by name (for menu items not wired yet)
    Other(String),
}

/// Everything the shell shows and changes.
#[derive(Debug, Clone)]
pub struct State {
    pub workspace: Workspace,
    pub tool: Tool,
    /// the current colour (straight RGBA) and its name
    pub colour: [f32; 4],
    pub colour_name: String,
    /// the palette: its name, and each colour's name and colour
    pub palette_name: String,
    pub palette: Vec<(String, [f32; 4])>,
    /// the cut being worked on
    pub cut_name: String,
    pub brushes: Vec<String>,
    pub brush: usize,
    pub brush_size: f32,
    /// the side panel's tab, per workspace
    pub panel_tab: [usize; 4],
    pub layers: Vec<Layer>,
    pub active_layer: usize,
    /// the current frame (0-based), the cut's length, its frame rate
    pub frame: u32,
    pub frames: u32,
    pub fps: u32,
    pub playing: bool,
    pub onion: bool,
    /// the timeline's frame width (its zoom), logical pixels
    pub frame_width: f32,
    /// drawings in this cut on the active layer (for the Draw workspace's strip)
    pub drawings: u32,
    pub drawing: u32,
    /// the Colour workspace's drawings and their state
    pub to_colour: Vec<ColourStatus>,
    pub notes: Vec<Note>,
    pub can_undo: bool,
    pub can_redo: bool,
    /// what the interface asked for this pass, for the app to carry out
    pub actions: Vec<Action>,
    /// how the document sits in the canvas card
    pub view: crate::shell::view::View,
    /// pixels being moved, shown over the document (from the engine)
    pub moving: Option<crate::shell::view::Moving>,
    /// the canvas tools' state the interface shows (`shell/engine/tools.rs`)
    pub tooling: crate::shell::engine::tools::ToolsUi,
    /// the open project on disk (`shell/engine/files.rs`)
    pub project: crate::shell::engine::files::ProjectUi,
    /// the export dialog and progress (`shell/engine/export.rs`)
    pub export: crate::shell::engine::export::ExportUi,
    /// the cut's exposure sheet (`shell/engine/frames.rs`)
    pub sheet: crate::shell::engine::frames::Sheet,
    /// each shell piece's own state (each defined in its piece's file)
    pub header: crate::shell::header::HeaderState,
    pub tools: crate::shell::tools::ToolsState,
    pub panel: crate::shell::panel::PanelState,
    pub strips: crate::shell::strips::StripsState,
}

impl State {
    /// The design's example project (the artboards' content), for the shell before a real
    /// project is open and for screenshots.
    pub fn demo() -> State {
        use pencilpilot_ui::theme::hex;
        State {
            workspace: Workspace::Animate,
            tool: Tool::Brush,
            colour: hex(0x16161A),
            colour_name: "Ink".into(),
            palette_name: "Mina’s palette".into(),
            cut_name: "Cut 2".into(),
            palette: vec![
                ("Ink".into(), hex(0x16161A)),
                ("Coral".into(), hex(0xE8492F)),
                ("Sun".into(), hex(0xFFD84D)),
                ("Leaf".into(), hex(0x5FD08A)),
                ("Sky".into(), hex(0x3D6BFF)),
                ("Plum".into(), hex(0x7C6CF0)),
                ("Skin".into(), hex(0xF2C3A0)),
                ("Shadow".into(), hex(0x8C7B9E)),
                ("Paper".into(), hex(0xF6F4EF)),
                ("Grey".into(), hex(0xA9A8AE)),
                ("Brick".into(), hex(0xA8432C)),
                ("Night".into(), hex(0x2A2F4A)),
            ],
            brushes: vec![
                "2B pencil".into(),
                "Rough pencil".into(),
                "Liner".into(),
                "Marker".into(),
                "Airbrush".into(),
            ],
            brush: 0,
            brush_size: 6.0,
            panel_tab: [0; 4],
            layers: vec![
                Layer {
                    name: "Clean".into(),
                    visible: true,
                    locked: false,
                    kind: 'C',
                },
                Layer {
                    name: "Tie-down".into(),
                    visible: true,
                    locked: false,
                    kind: 'C',
                },
                Layer {
                    name: "Rough".into(),
                    visible: true,
                    locked: false,
                    kind: 'C',
                },
                Layer {
                    name: "Background".into(),
                    visible: true,
                    locked: true,
                    kind: 'S',
                },
            ],
            active_layer: 0,
            frame: 14,
            frames: 48,
            fps: 24,
            playing: false,
            onion: true,
            frame_width: 22.0,
            // (as the sheet has them: Clean's six drawings, frame 15 on drawing 3)
            drawings: 6,
            drawing: 2,
            to_colour: vec![
                ColourStatus::Done,
                ColourStatus::Done,
                ColourStatus::Done,
                ColourStatus::Flatted,
                ColourStatus::Flatted,
                ColourStatus::ToDo,
            ],
            notes: vec![
                Note {
                    author: "Sam".into(),
                    text: "Hold the anticipation two frames longer".into(),
                    frame: 14,
                    colour: hex(0xE8492F),
                    resolved: false,
                },
                Note {
                    author: "Rin".into(),
                    text: "Arc on the hand: a touch higher".into(),
                    frame: 22,
                    colour: hex(0x7C6CF0),
                    resolved: false,
                },
            ],
            can_undo: false,
            can_redo: false,
            actions: Vec::new(),
            view: Default::default(),
            moving: None,
            tooling: Default::default(),
            project: Default::default(),
            export: Default::default(),
            sheet: crate::shell::engine::frames::Sheet::demo(),
            header: Default::default(),
            tools: Default::default(),
            panel: Default::default(),
            strips: Default::default(),
        }
    }
}
