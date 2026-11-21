//! PencilPilot, the app: a Tauri 2 shell around the studio (apps/studio/src).
//!
//! The studio runs in the window (the paint engine as WebAssembly for now). Here: saving
//! and opening `.ppstudio` projects (see [`project`]), recovery copies and version
//! history ([`store`]), exports, and later the native engine (crates/brush) and the
//! animation core (crates/anim).

mod export;
mod lipsync;
mod project;
mod store;

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use anim::FrameStore;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Manager, State};

/// The animation's drawings (crates/anim): the window keeps only the frames it's working
/// on; the rest live here, in memory up to [`FRAMES_IN_MEMORY`] and on disk past it.
struct Frames(Mutex<FrameStore>);

/// The export being written, if any (one at a time), and the sound for the next one.
#[derive(Default)]
struct Exporting(Mutex<Option<export::Job>>, Mutex<Option<PathBuf>>);

/// Lip-sync: the recording to listen to next, and the run going.
#[derive(Default)]
struct LipSync(Mutex<Option<PathBuf>>, lipsync::Running);

/// Bytes of drawing tiles kept in memory before the oldest go to disk.
const FRAMES_IN_MEMORY: usize = 1 << 30;

fn frames<'a>(state: &'a State<'_, Frames>) -> MutexGuard<'a, FrameStore> {
    // a panic mid-change can't leave the store half-done in a way that matters more
    // than losing it, so carry on with what's there
    state.0.lock().unwrap_or_else(|e| e.into_inner())
}

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
fn project_save(
    app: AppHandle,
    request: Request<'_>,
    store: State<'_, Frames>,
) -> Result<String, String> {
    let path = path_header(&request)?;
    let packet = raw_body(&request)?;
    let data = data_dir(&app)?;
    store::keep_version(&data, &path, store::versions_kept(&data))?;
    let name = format!("PencilPilot {}", env!("CARGO_PKG_VERSION"));
    project::save(&path, packet, &name, Some(&mut frames(&store)))?;
    // the home screen's list; a failure there mustn't fail the save
    let _ = store::recent_add(&data, &path);
    Ok(path.to_string_lossy().into_owned())
}

/// Open the project at the `path` header: the packet the window restores from.
#[tauri::command]
fn project_open(
    app: AppHandle,
    request: Request<'_>,
    store: State<'_, Frames>,
) -> Result<Response, String> {
    let path = path_header(&request)?;
    let packet = project::open(&path, Some(&mut frames(&store)))?;
    if let Ok(data) = data_dir(&app) {
        let _ = store::recent_add(&data, &path);
    }
    Ok(Response::new(packet))
}

#[tauri::command]
fn settings_get(app: AppHandle) -> Result<serde_json::Value, String> {
    Ok(store::settings_get(&data_dir(&app)?))
}

#[tauri::command]
fn settings_set(app: AppHandle, settings: serde_json::Value) -> Result<(), String> {
    store::settings_set(&data_dir(&app)?, &settings)
}

/// Where the app keeps its own files (shown in Settings).
#[tauri::command]
fn data_folder(app: AppHandle) -> Result<String, String> {
    Ok(data_dir(&app)?.to_string_lossy().into_owned())
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
fn recovery_write(
    app: AppHandle,
    request: Request<'_>,
    store: State<'_, Frames>,
) -> Result<(), String> {
    let id = header(&request, "id")?;
    let path = header(&request, "path").ok().filter(|p| !p.is_empty());
    store::recovery_write(
        &data_dir(&app)?,
        &id,
        raw_body(&request)?,
        path.as_deref(),
        Some(&mut frames(&store)),
    )
}

#[tauri::command]
fn recovery_list(app: AppHandle) -> Result<Vec<store::RecoveryItem>, String> {
    Ok(store::recovery_list(&data_dir(&app)?))
}

#[tauri::command]
fn recovery_open(app: AppHandle, id: String, store: State<'_, Frames>) -> Result<Response, String> {
    Ok(Response::new(store::recovery_open(
        &data_dir(&app)?,
        &id,
        Some(&mut frames(&store)),
    )?))
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
fn version_open(
    app: AppHandle,
    path: String,
    id: String,
    store: State<'_, Frames>,
) -> Result<Response, String> {
    let file = project::project_path(&path)?;
    Ok(Response::new(store::version_open(
        &data_dir(&app)?,
        &file,
        &id,
        Some(&mut frames(&store)),
    )?))
}

// ---- the frame store ----

/// Keep drawings from the window (a packet: see [`anim::pack`]).
#[tauri::command]
fn frames_put(request: Request<'_>, store: State<'_, Frames>) -> Result<(), String> {
    let drawings = anim::unpack(raw_body(&request)?).map_err(|e| e.to_string())?;
    let mut s = frames(&store);
    for (id, tiles) in drawings {
        s.put(id, tiles).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Drawings for the window (a packet); unknown ones are left out.
#[tauri::command]
fn frames_get(ids: Vec<u32>, store: State<'_, Frames>) -> Result<Response, String> {
    let mut s = frames(&store);
    let mut out = Vec::new();
    for id in ids {
        if let Some(tiles) = s.get(id).map_err(|e| e.to_string())? {
            out.push((id, tiles));
        }
    }
    Ok(Response::new(anim::pack(&out)))
}

/// Forget every drawing not in `keep` (a new document starts with none).
#[tauri::command]
fn frames_retain(keep: Vec<u32>, store: State<'_, Frames>) {
    frames(&store).retain(&keep.into_iter().collect());
}

/// How much the store holds (for the developer page and tests).
#[tauri::command]
fn frames_stats(store: State<'_, Frames>) -> serde_json::Value {
    let st = frames(&store).stats();
    serde_json::json!({
        "drawings": st.drawings,
        "tiles": st.tiles,
        "placed": st.placed,
        "bytesInMemory": st.bytes_in_memory,
        "bytesOnDisk": st.bytes_on_disk,
    })
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

/// What can be exported here: ffmpeg's video formats (none without ffmpeg).
#[tauri::command]
fn export_tools() -> serde_json::Value {
    let ffmpeg = export::find_ffmpeg();
    let video = ffmpeg
        .as_deref()
        .map(export::video_formats)
        .unwrap_or_default();
    serde_json::json!({
        "ffmpeg": ffmpeg.map(|p| p.to_string_lossy().into_owned()),
        "video": video,
    })
}

fn exporting(state: &Exporting) -> MutexGuard<'_, Option<export::Job>> {
    state.0.lock().unwrap_or_else(|e| e.into_inner())
}

/// The sound for the next video export: a WAV file (the body), kept until it's used.
#[tauri::command]
fn export_audio(request: Request<'_>, state: State<'_, Exporting>) -> Result<(), String> {
    let bytes = raw_body(&request)?;
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" {
        return Err("not a WAV file".into());
    }
    let path = std::env::temp_dir().join(format!("pencilpilot-export-{}.wav", std::process::id()));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    *state.1.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    Ok(())
}

/// Where Rhubarb (lip-sync) is, if it's here.
#[tauri::command]
fn lipsync_tools(app: AppHandle) -> Option<String> {
    let res = app.path().resource_dir().ok();
    lipsync::find(res.as_deref()).map(|p| p.to_string_lossy().into_owned())
}

/// The recording to listen to (a WAV file, as bytes).
#[tauri::command]
fn lipsync_audio(request: Request<'_>, state: State<'_, LipSync>) -> Result<(), String> {
    let bytes = raw_body(&request)?;
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" {
        return Err("not a WAV file".into());
    }
    let path = std::env::temp_dir().join(format!("pencilpilot-lipsync-{}.wav", std::process::id()));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    *state.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    Ok(())
}

/// Listen to the recording sent: the mouth shapes, in time. Sends "lipsync-progress"
/// (0–1) while it works.
#[tauri::command]
async fn lipsync_run(app: AppHandle, opts: lipsync::Options) -> Result<Vec<lipsync::Cue>, String> {
    use tauri::Emitter;
    let res = app.path().resource_dir().ok();
    let exe = lipsync::find(res.as_deref()).ok_or("Rhubarb (lip-sync) isn’t installed")?;
    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    let exe = lipsync::runnable(&exe, &cache).map_err(|e| e.to_string())?;
    let wav = app
        .state::<LipSync>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or("no recording sent")?;
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<LipSync>();
        let r = lipsync::run(&exe, &wav, &opts, &state.1, |v| {
            let _ = handle.emit("lipsync-progress", v);
        });
        let _ = std::fs::remove_file(&wav);
        r
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Stop listening.
#[tauri::command]
fn lipsync_cancel(state: State<'_, LipSync>) {
    lipsync::cancel(&state.1);
}

/// Start an export (one going already is cancelled). A video takes the sound sent
/// with `export_audio` just before.
#[tauri::command]
fn export_begin(mut spec: export::Spec, state: State<'_, Exporting>) -> Result<(), String> {
    let sound = state.1.lock().unwrap_or_else(|e| e.into_inner()).take();
    let mut slot = exporting(&state);
    if let Some(old) = slot.take() {
        old.cancel();
    }
    match sound {
        Some(wav)
            if matches!(
                spec.format,
                export::Format::Mp4 | export::Format::Prores | export::Format::Webm
            ) =>
        {
            spec.audio = Some(wav)
        }
        Some(wav) => {
            let _ = std::fs::remove_file(wav);
        }
        None => {}
    }
    *slot = Some(export::Job::begin(spec)?);
    Ok(())
}

/// One frame of the export (the body: straight RGBA8), held for the `repeat` header.
#[tauri::command]
fn export_frame(request: Request<'_>, state: State<'_, Exporting>) -> Result<(), String> {
    let repeat = header(&request, "repeat")?
        .parse::<u32>()
        .map_err(|e| e.to_string())?;
    let mut slot = exporting(&state);
    let job = slot.as_mut().ok_or("no export is going")?;
    if let Err(e) = job.frame(raw_body(&request)?, repeat) {
        if let Some(j) = slot.take() {
            j.cancel();
        }
        return Err(e);
    }
    Ok(())
}

/// Finish the export; where it went.
#[tauri::command]
fn export_end(state: State<'_, Exporting>) -> Result<String, String> {
    let job = exporting(&state).take().ok_or("no export is going")?;
    job.end()
}

/// Stop the export and take away what it wrote.
#[tauri::command]
fn export_cancel(state: State<'_, Exporting>) {
    if let Some(j) = exporting(&state).take() {
        j.cancel();
    }
}

/// The engine benchmark (/dev/bench): bytes in, the same bytes back (an IPC round trip
/// with a dirty rect's worth of pixels).
#[tauri::command]
fn bench_echo(request: Request<'_>) -> Result<Response, String> {
    Ok(Response::new(raw_body(&request)?.to_vec()))
}

/// The benchmark's results, printed to the app's output; with `PP_BENCH` set, the app
/// then closes (it was started just to measure).
#[tauri::command]
fn bench_report(app: AppHandle, report: String) {
    println!("PP_BENCH {}", report.replace('\n', " "));
    if std::env::var_os("PP_BENCH").is_some() {
        app.exit(0);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Exporting::default())
        .manage(LipSync::default())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // PP_BENCH=1: open the engine benchmark straight away
            if std::env::var_os("PP_BENCH").is_some()
                && let Some(w) = app.get_webview_window("main")
            {
                w.eval("location.replace('/dev/bench')")?;
            }
            // the frame store spills to this run's own folder in the app's cache
            let dir = app
                .path()
                .app_cache_dir()
                .ok()
                .map(|d| d.join("frames").join(std::process::id().to_string()));
            app.manage(Frames(Mutex::new(FrameStore::new(FRAMES_IN_MEMORY, dir)?)));
            Ok(())
        })
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
            export_tools,
            export_audio,
            export_begin,
            export_frame,
            export_end,
            export_cancel,
            lipsync_tools,
            lipsync_audio,
            lipsync_run,
            lipsync_cancel,
            recent_list,
            recent_remove,
            settings_get,
            settings_set,
            data_folder,
            bench_echo,
            bench_report,
            frames_put,
            frames_get,
            frames_retain,
            frames_stats
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
