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

/// The portal's FileChooser (org.freedesktop.portal.FileChooser): OpenFile or SaveFile, then
/// its Request's Response signal carries the chosen file. None: cancelled, or no portal.
fn portal(a: &Ask) -> Option<PathBuf> {
    match portal_call(a) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("the file dialog couldn't open: {e}");
            None
        }
    }
}

fn portal_call(a: &Ask) -> zbus::Result<Option<PathBuf>> {
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    const DEST: &str = "org.freedesktop.portal.Desktop";
    let conn = zbus::blocking::Connection::session()?;
    // the Request's path is known before calling (from a token of ours), so its Response
    // can't be missed
    let token = format!(
        "pencilpilot{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() % 1_000_000_000)
    );
    let sender = conn
        .unique_name()
        .map(|n| n.trim_start_matches(':').replace('.', "_"))
        .unwrap_or_default();
    let path = request_path(&sender, &token);
    let request =
        zbus::blocking::Proxy::new(&conn, DEST, path.as_str(), "org.freedesktop.portal.Request")?;
    let mut responses = request.receive_signal("Response")?;
    let chooser = zbus::blocking::Proxy::new(
        &conn,
        DEST,
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.FileChooser",
    )?;
    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("handle_token", Value::from(token.as_str()));
    options.insert("modal", Value::from(true));
    let filters: Vec<(String, Vec<(u32, String)>)> = a
        .filters
        .iter()
        .map(|(label, pats)| {
            (
                label.clone(),
                pats.iter().map(|p| (0u32, p.clone())).collect(),
            )
        })
        .collect();
    if !filters.is_empty() {
        options.insert("filters", Value::from(filters));
    }
    if let Some(n) = &a.name {
        options.insert("current_name", Value::from(n.as_str()));
    }
    let method = if a.save { "SaveFile" } else { "OpenFile" };
    let _: OwnedObjectPath = chooser.call(method, &("", a.title.as_str(), options))?;
    let Some(msg) = responses.next() else {
        return Ok(None);
    };
    let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize()?;
    if code != 0 {
        return Ok(None); // cancelled (1) or ended otherwise (2)
    }
    let uris: Vec<String> = results
        .get("uris")
        .and_then(|v| Vec::<String>::try_from(v.clone()).ok())
        .unwrap_or_default();
    Ok(uris.first().and_then(|u| path_from_uri(u)))
}

/// Where the portal puts a Request it makes for `sender` (its unique name, ':' dropped and '.'
/// as '_') with our `token`.
fn request_path(sender: &str, token: &str) -> String {
    format!("/org/freedesktop/portal/desktop/request/{sender}/{token}")
}

/// A `file://` URI's path, percent-escapes decoded.
fn path_from_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // (file://host/path: the path starts at the first '/')
    let rest = &rest[rest.find('/')?..];
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).ok()?, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_become_paths() {
        assert_eq!(
            path_from_uri("file:///home/ali/My%20Films/Ember%E2%80%99s%20cut.ppstudio"),
            Some(PathBuf::from("/home/ali/My Films/Ember’s cut.ppstudio"))
        );
        assert_eq!(
            path_from_uri("file://localhost/tmp/a.mp4"),
            Some(PathBuf::from("/tmp/a.mp4"))
        );
        assert_eq!(path_from_uri("https://example.org/a"), None);
        // a stray % stays as it is
        assert_eq!(
            path_from_uri("file:///tmp/100%"),
            Some(PathBuf::from("/tmp/100%"))
        );
    }

    #[test]
    fn the_request_path_follows_the_portal_rule() {
        assert_eq!(
            request_path("1_42", "pencilpilot7"),
            "/org/freedesktop/portal/desktop/request/1_42/pencilpilot7"
        );
    }

    #[test]
    fn checks_answer_at_once() {
        // (the variable is read at each ask; set for this test only)
        // SAFETY: tests touching the environment run in this one test
        unsafe { std::env::set_var("PENCILPILOT_DIALOG_ANSWER", "/tmp/x.ppstudio") };
        let a = ask(Ask {
            title: "Open".into(),
            save: false,
            name: None,
            filters: vec![],
        });
        assert_eq!(a.answer(), Some(Some(PathBuf::from("/tmp/x.ppstudio"))));
        unsafe { std::env::set_var("PENCILPILOT_DIALOG_ANSWER", "") };
        let a = ask(Ask {
            title: "Open".into(),
            save: false,
            name: None,
            filters: vec![],
        });
        assert_eq!(a.answer(), Some(None), "empty: cancelled");
        unsafe { std::env::remove_var("PENCILPILOT_DIALOG_ANSWER") };
    }
}
