//! PencilPilot, the app: a Tauri 2 shell around the studio (apps/studio/src).
//!
//! The studio runs in the window (the paint engine as WebAssembly for now). Here: saving
//! and opening `.ppstudio` projects (see [`project`]), recovery copies and version
//! history ([`store`]), exports, and later the native engine (crates/brush) and the
//! animation core (crates/anim).

mod project;
mod store;

use std::path::PathBuf;

use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Manager};

/// The app's own data folder (recovery copies, versions).
fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

/// A URI-encoded header's value.
fn header(request: &Request<'_>, name: &str) -> Result<String, String> {
    let raw = request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| format!("no {name} given"))?;
    project::percent_decode(raw)
}

fn raw_body<'a>(request: &'a Request<'_>) -> Result<&'a [u8], String> {
    match request.body() {
        InvokeBody::Raw(bytes) => Ok(bytes),
        _ => Err("the document must be sent as bytes".into()),
    }
}

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
fn path_header(request: &Request<'_>) -> Result<PathBuf, String> {
    project::project_path(&header(request, "path")?)
}

/// Save the window's document (a raw packet, see [`project`]) to the `path` header,
/// keeping the file it replaces as a version. Returns the path it was saved to.
#[tauri::command]
fn project_save(app: AppHandle, request: Request<'_>) -> Result<String, String> {
    let path = path_header(&request)?;
    let packet = raw_body(&request)?;
    let data = data_dir(&app)?;
    store::keep_version(&data, &path)?;
    let name = format!("PencilPilot {}", env!("CARGO_PKG_VERSION"));
    project::save(&path, packet, &name)?;
    // the home screen's list; a failure there mustn't fail the save
    let _ = store::recent_add(&data, &path);
    Ok(path.to_string_lossy().into_owned())
}

/// Open the project at the `path` header: the packet the window restores from.
#[tauri::command]
fn project_open(app: AppHandle, request: Request<'_>) -> Result<Response, String> {
    let path = path_header(&request)?;
    let packet = project::open(&path)?;
    if let Ok(data) = data_dir(&app) {
        let _ = store::recent_add(&data, &path);
    }
    Ok(Response::new(packet))
}

#[tauri::command]
fn recent_list(app: AppHandle) -> Result<Vec<store::RecentItem>, String> {
    Ok(store::recent_list(&data_dir(&app)?))
}

#[tauri::command]
fn recent_remove(app: AppHandle, path: String) -> Result<(), String> {
    store::recent_remove(&data_dir(&app)?, &path)
}

/// Keep an unsaved document as recovery copy `id` (header), for the file `path` (header,
/// optional).
#[tauri::command]
fn recovery_write(app: AppHandle, request: Request<'_>) -> Result<(), String> {
    let id = header(&request, "id")?;
    let path = header(&request, "path").ok().filter(|p| !p.is_empty());
    store::recovery_write(&data_dir(&app)?, &id, raw_body(&request)?, path.as_deref())
}

#[tauri::command]
fn recovery_list(app: AppHandle) -> Result<Vec<store::RecoveryItem>, String> {
    Ok(store::recovery_list(&data_dir(&app)?))
}

#[tauri::command]
fn recovery_open(app: AppHandle, id: String) -> Result<Response, String> {
    Ok(Response::new(store::recovery_open(&data_dir(&app)?, &id)?))
}

#[tauri::command]
fn recovery_discard(app: AppHandle, id: String) -> Result<(), String> {
    store::recovery_discard(&data_dir(&app)?, &id)
}

#[tauri::command]
fn versions_list(app: AppHandle, path: String) -> Result<Vec<store::VersionItem>, String> {
    Ok(store::versions_list(
        &data_dir(&app)?,
        &project::project_path(&path)?,
    ))
}

#[tauri::command]
fn version_open(app: AppHandle, path: String, id: String) -> Result<Response, String> {
    let file = project::project_path(&path)?;
    Ok(Response::new(store::version_open(
        &data_dir(&app)?,
        &file,
        &id,
    )?))
}

/// Write an export (the body) to the `path` header: a PNG or a PSD only.
#[tauri::command]
fn export_write(request: Request<'_>) -> Result<String, String> {
    let path = PathBuf::from(header(&request, "path")?);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if !path.is_absolute() || !matches!(ext.as_deref(), Some("png" | "psd")) {
        return Err("exports go to an absolute .png or .psd path".into());
    }
    std::fs::write(&path, raw_body(&request)?).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            app_info,
            project_save,
            project_open,
            recovery_write,
            recovery_list,
            recovery_open,
            recovery_discard,
            versions_list,
            version_open,
            export_write,
            recent_list,
            recent_remove
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
