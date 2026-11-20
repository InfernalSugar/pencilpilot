//! PencilPilot, the app: a Tauri 2 shell around the studio (apps/studio/src).
//!
//! The studio runs in the window (the paint engine as WebAssembly for now). Here: saving
//! and opening `.ppstudio` projects (see [`project`]), and later the native engine
//! (crates/brush) and the animation core (crates/anim).

mod project;

use tauri::ipc::{InvokeBody, Request, Response};

/// What the window can ask about the app it runs in (shown on the pen test page).
#[derive(serde::Serialize)]
struct AppInfo {
    version: &'static str,
    os: &'static str,
    arch: &'static str,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

/// The path a project command was given (in the `path` header, URI-encoded).
fn path_header(request: &Request<'_>) -> Result<std::path::PathBuf, String> {
    let raw = request
        .headers()
        .get("path")
        .and_then(|v| v.to_str().ok())
        .ok_or("no path given")?;
    let decoded = project::percent_decode(raw)?;
    project::project_path(&decoded)
}

/// Save the window's document (a raw packet, see [`project`]) to the `path` header.
/// Returns the path it was saved to.
#[tauri::command]
fn project_save(request: Request<'_>) -> Result<String, String> {
    let path = path_header(&request)?;
    let InvokeBody::Raw(packet) = request.body() else {
        return Err("the document must be sent as bytes".into());
    };
    let app = format!("PencilPilot {}", env!("CARGO_PKG_VERSION"));
    project::save(&path, packet, &app)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Open the project at the `path` header: the packet the window restores from.
#[tauri::command]
fn project_open(request: Request<'_>) -> Result<Response, String> {
    let path = path_header(&request)?;
    Ok(Response::new(project::open(&path)?))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            app_info,
            project_save,
            project_open
        ])
        .run(tauri::generate_context!())
        .expect("error while running PencilPilot");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_names_this_build() {
        let a = app_info();
        assert_eq!(a.version, env!("CARGO_PKG_VERSION"));
        assert!(!a.os.is_empty() && !a.arch.is_empty());
    }
}
