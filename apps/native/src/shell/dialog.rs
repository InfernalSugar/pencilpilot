//! The system's file dialog (the desktop portal's FileChooser), asked without blocking: the
//! answer arrives later through [`Asked::answer`].
//!
//! For checks with no one to click: `PENCILPILOT_DIALOG_ANSWER` = a path answers every dialog
//! with it at once (an empty value: cancelled).

use std::path::PathBuf;
use std::sync::mpsc;

/// What to ask for.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub title: String,
    /// save (a new name) rather than open (an existing file)
    pub save: bool,
    /// the name suggested when saving
    pub name: Option<String>,
    /// file types offered: a label and its patterns ("PencilPilot project", ["*.ppstudio"])
    pub filters: Vec<(String, Vec<String>)>,
}

/// A dialog asked; its answer comes later.
#[derive(Debug)]
pub struct Asked {
    rx: mpsc::Receiver<Option<PathBuf>>,
}

impl Asked {
    /// The answer, once there is one: Some(Some(path)) chosen, Some(None) cancelled, None not
    /// yet.
    pub fn answer(&self) -> Option<Option<PathBuf>> {
        self.rx.try_recv().ok()
    }
}

/// Opens the dialog (on another thread) and returns at once.
pub fn ask(a: Ask) -> Asked {
    let (tx, rx) = mpsc::channel();
    if let Some(v) = std::env::var_os("PENCILPILOT_DIALOG_ANSWER") {
        let _ = tx.send((!v.is_empty()).then(|| PathBuf::from(v)));
        return Asked { rx };
    }
    std::thread::spawn(move || {
        let _ = tx.send(portal(&a));
    });
    Asked { rx }
}

/// The portal's FileChooser (filled in by the lead: for now, cancelled).
fn portal(_a: &Ask) -> Option<PathBuf> {
    None
}
