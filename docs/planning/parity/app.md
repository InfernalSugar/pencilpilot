# Parity inventory: the app (files, export, settings, platform)

Phase 7 ("full parity, then switch"). Each row is one capability a user of the current app
(`apps/studio`, Svelte/TS + Tauri) would notice. The status is the native app's (`apps/native`),
checked by reading its code on `native/phase7` (2026-11-28). Parity means the capability
exists, not the same interface.

- **Status:** Done / Partial (what's missing) / Missing. Partial? means unsure, with the reason.
- **Size** (the work left): S under half a day, M 1–2 days, L more. "—" when Done.
- **Paths:** current app files are under `apps/studio/src/` (`lib/…`; `page` is
  `routes/studio/+page.svelte`; `home` is `routes/+page.svelte`); `src-tauri/` is
  `apps/studio/src-tauri/src/`. Native files are under `apps/native/src/` (`header.rs` is
  `shell/header.rs`, `files/` is `shell/engine/files/`, `export/` is `shell/engine/export/`,
  and so on).
- **Stub:** a native menu item that's drawn but not wired. It pushes an action that nothing
  handles.
- **Self-update** isn't on `dev` yet: it's commit `d943e1f` on the `self-update` branch
  (`lib/updates.svelte.ts`, `lib/UpdatePrompt.svelte`, `deploy/updates/README.md`).
- **Not repeated here:** the sibling inventories have these rows, so they're referenced, not
  counted:
  - [animation.md](animation.md): export by stage (§9), export through the camera (§6), the
    sound mix in videos and lip-sync's Rhubarb (§8), the timelapse and drawn-live films,
    picture-sequence and video import (§12), workspaces and their bars and panels (§10), frame
    memory (§12).
  - [drawing.md](drawing.md): tool keys and rebinding shortcuts (§1), pasting a picture from the
    clipboard and dropping a file on the canvas (§7), importing a picture as a layer (§11),
    canvas size and crop (§15), opening a `.psd` (§16), references (§16), the cursor, the wheel
    and touch (§15), undo history (§14).

---

## 1. Projects and files

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| New project from a template (HD, 4K, Square, Vertical, Storyboard, Illustration) or Custom (64–8192 px a side, 1–60 fps, in the units Settings names) | `home` New; `lib/project.ts` `TEMPLATES`, `SIDE_MIN`/`SIDE_MAX` | **Partial**: File › New… (Ctrl+N) always makes 1920 × 1080 at 24 fps. There's no choice of size or rate. | `files/mod.rs:545–585`; `header.rs:24` | S | The defaults also come from Settings (§4). Design question 1. |
| Open a project (the system's dialog, `.ppstudio` only, starting in the projects folder) | `lib/project.ts` `askOpenPath`, `startFolder`; `src-tauri/lib.rs` `project_open` | **Done** (the portal's FileChooser). It doesn't start in a chosen folder. | `files/mod.rs:464–480`; `shell/dialog.rs:70–130` | — | The projects-folder setting is in §4. |
| Save and Save as (Ctrl+S, Ctrl+Shift+S; `.ppstudio` added to the name) | `lib/project.ts` `saveProject`, `askSavePath`; `src-tauri/lib.rs` `project_save` | **Done**, written off the window thread | `files/mod.rs:497–515`, `628–700`, `944–950` | — | The native app re-encodes only the tiles that changed (ADR-0014, Phase 2 step 5). |
| The project's name and whether it's saved, in the header | page `docName`, `savedVersion` | **Done** ("Scene 1 · Cut 2 · saved / unsaved / saving…") | `files/mod.rs:261–275`; `header.rs:104–106` | — | |
| "Save first?" before New, Open, a recent file, an old version, Quit and the window's close (Save / Don't save / Cancel) | `lib/SaveChangesDialog.svelte`; page `okToDrop`, `onCloseRequested` | **Done** for New, Open, a recent file, Quit and the close button | `files/mod.rs:438–462`, `492–495`; `shell/mod.rs:289`, `740` | — | Old versions and updates don't exist natively yet (rows below and §7). Turning the question off is a setting (§4). |
| Recovery copies of unsaved work, every 10 s / 20 s / 1 min / 5 min or never (default 20 s) | `lib/project.ts` `writeRecovery`; `src-tauri/store.rs` `recovery_write`; settings `autosaveSeconds` | **Partial**: fixed at 2 min after the first unsaved change, then every 2 min | `files/mod.rs:55`, `840–847` | S | A crash can lose up to 2 min of work, against 20 s today. The copy is written off the thread, but measure the pen while it's written. |
| Recovery offered at the next start: every copy listed (thumbnail, size, when), each recovered or discarded | `home` "Unsaved work was found"; `src-tauri/lib.rs` `recovery_list`, `recovery_open`, `recovery_discard` | **Partial**: only the newest copy newer than its file is offered, as a question (Open / Discard / Later). Older copies stay on disk and are never shown. | `files/mod.rs:849–876`, `893–927` | S | Belongs on the start page (§3). |
| Version history: each save keeps the file it replaces (20 by default, 0–100), listed per project (when, size) and opened as unsaved | `lib/VersionsMenu.svelte`; `src-tauri/store.rs` `keep_version`, `versions_list`, `version_open`; page `openOldVersion` | **Missing**. A save writes over the file. | none | M | Copying the old file first must stay off the window thread (the save budget). Using the current app's folder layout carries the history over (checklist). Design question 5. |
| Recent projects: thumbnail, size, layers, when changed, "Moved or deleted", remove from the list; how many show (setting, 12; 30 kept) | `home` Recent; `src-tauri/store.rs` `recent_add`, `recent_list`, `recent_remove` | **Partial**: the last 10 paths, in File › Open…'s submenu. No thumbnails, details or remove. | `header.rs:226–255`; `files/recent.rs:7`, `45–80` | S | A separate list from the current app's (checklist). The details belong on the start page. |
| Open the last project when the app starts (setting) | `home` `reopenLast` | **Missing** | none | S | |
| Projects drawn together listed apart ("With Ana, Ben…"), with Rejoin | `home` `shared`, `rejoin` | **Missing** | none | S | After rejoining from the file (§6). |
| `.ppstudio` both ways with the current app | `crates/ppstudio` | **Done** | `files/load.rs`; `files/save.rs:1–8`; ADR-0014, Phase 2 step 5 | — | The director plugin builds `.ppstudio` files through `crates/ppstudio-cli`, so it works with both. |
| What the native app doesn't edit survives its save (other cuts, sounds, fonts, references, line data, timelapse, layer styles) | `lib/project.ts` (the whole document) | **Partial?** It's kept as the file had it. But a drawing painted in the native app keeps its old editable-stroke data (`lines/<drawing>`), and the native app adds nothing to the stroke log. I didn't check whether the current app then shows the old strokes over the new pixels. | `files/save.rs:1–4`, `673–678` | S | This matters during the beta, when people go back and forth. Check it with drawing.md's line-layer rows. |
| The file dialog works on every desktop | Tauri's dialog plugin | **Partial**: it needs xdg-desktop-portal with a FileChooser backend. Without one, the dialog fails silently (stderr only) and looks like Cancel. | `shell/dialog.rs:70–79` | S | Say so in the app, and make the package depend on a portal (checklist). |
| File › Import… (Ctrl+I) and File › Project settings… | page Import menu (picture sequence, video frames, reference pictures); the Image menu (canvas size, crop) | **Missing**. Both are stubs, and Ctrl+I isn't bound. | `header.rs:30–33`; `files/mod.rs:367–436` (no case for either) | S | The menu only. The imports are in animation.md §12 and drawing.md §16, canvas size in drawing.md §15. Design question 14. |

## 2. Export

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| The export dialog: format, frames, size, transparency, progress, cancel; the window stays smooth while it runs | `lib/ExportDialog.svelte`; page `openAnimExport`; `src-tauri/lib.rs` `export_begin`/`frame`/`end`/`cancel` | **Done** (header button, File › Export…, Ctrl+E) | `export_ui.rs:27–80`, `189–270`; `export/mod.rs:333–420` | — | Measured within the budgets (ADR-0014, Phase 2 step 5). |
| MP4 (H.264) | `lib/exporting.ts` `FORMATS` | **Done** | `export/mod.rs:48–54` | — | |
| WebM (VP9), with transparency | `FORMATS` webm | **Partial**: no transparency (the toggle is for PNG and GIF only) | `export/mod.rs:146–148` | S | `crates/export` already writes alpha WebM (`lib.rs:334–345`). Memory: 4K WebM peaks around 1.5 GB (ADR). |
| GIF (256 colours, transparency, play once) | `FORMATS` gif; `ExportSettings.once` | **Partial**: no "Play once": it always loops | `export/worker.rs:226` | S | |
| PNG sequence (numbered, transparency) | `FORMATS` png; `exporting.ts` `frameFile` | **Done** | `export/mod.rs:48–54` | — | |
| ProRes 4444 (`.mov`, transparency, for editing) | `FORMATS` prores | **Missing** from the dialog. `crates/export` writes it. | `export/mod.rs:48–54`; `crates/export/src/lib.rs:314–333` | S | |
| Animated PNG (full colour; loops or plays once; transparency) | `FORMATS` apng | **Missing** from the dialog. `crates/export` writes it. | `export/mod.rs:48–54`; `crates/export/src/lib.rs:38` | S | |
| PSD per frame (a layered PSD file for each frame) | `FORMATS` psd | **Partial**: one PSD of the frame showing | `export_ui.rs:205–222`; `export/psd.rs` | S | |
| PNG per layer (each layer as its own numbered sequence, to composite) | `FORMATS` layers | **Missing** | none | M | |
| One flat PNG picture (File › Export as › PNG picture) | page `exportAs('png')`; `lib/png-meta.ts` `tagPng` | **Partial**: only as a PNG sequence of the cut or the loop range | `export_ui.rs:226–240` | S | Design question 11. |
| One layered PSD of the picture (layers, groups, masks) | page `exportAs('psd')` | **Done** (PSD: "This frame") | `export_ui.rs:205–222`; `export/psd.rs` | — | |
| Frames: this cut, or the loop range | `ExportRange` cut, loop | **Done** | `export/mod.rs:184–190` | — | |
| Frames: this scene (every cut, with the transitions), or everything | `ExportRange` scene, all; `exporting.ts` `cutsIn`, `plan`, `outputFrames` | **Missing** | `export/mod.rs:184–190` (one cut) | M | After the multi-cut model (animation.md §1, §4). |
| Size: full, 75 %, half, a quarter | `ExportDialog.svelte` scale | **Partial**: Document, Half, 720p, 1080p, 4K. There's no 75 % or quarter. | `export/mod.rs:58–64`, `166–180` | S | The native app adds fixed heights. Sizing by the camera is in animation.md §6. Design question 11. |
| Frame rate out: the project's, or 12 / 15 / 24 / 25 / 30 / 60 | `ExportSettings.fps` | **Missing**: the project's rate only | `export/worker.rs:222` | S | |
| Video quality: Best, Good, Small (setting) | settings `exportQuality` | **Missing**: the default quality only | `export/worker.rs:228`; `crates/export/src/lib.rs:108–115` | S | |
| What the file says about the work: software, author, who drew it together (setting); sRGB tags | `lib/png-meta.ts`; page `exportMeta`; settings `exportCredits` | **Partial**: "Software: PencilPilot" only. The sRGB tags are written. | `export/worker.rs:229`; `crates/export/src/lib.rs:9–11`, `136–142` | S | |
| FFmpeg comes with the app (video works out of the box) | `src-tauri/tauri.bundle.conf.json` `externalBin`; `app.yml` bundles it | **Partial**: it uses the system's FFmpeg (or `PENCILPILOT_FFMPEG`). Without one, MP4 and WebM say to install it. | `export/mod.rs:322–331`, `353–360` | M | A packaging decision (design question 6). The GPL notice must come with a bundled FFmpeg (§7). |
| Export → Animation starts on the format chosen in Settings | settings `exportFormat` | **Missing** | none | S | |

## 3. Start page, menus and the app's own pages

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| A start page: New (templates, Custom), Draw together (host, join with a code), shared projects, Recent, unsaved work found, Open…, Settings, Pen test; Home from the studio | `home`; page home button | **Missing**. The app opens straight into the design's demo project ("Ember", with its demo drawings). | `shell/mod.rs:292`; `state.rs:186–230`; `frames/demo.rs` | M | **A beta blocker:** a first start can't be a demo document. Design question 1. |
| The header's View menu works | (the current app has no View menu: keys, the canvas, the quick menu) | **Partial**: Onion skin works. Zoom in / out, Fit to window, Rotate, Flip, Reset view and Full screen are stubs: they're pushed as `Action::Other`, and only the canvas menu's handler knows them. The keys and the canvas's own menu work. | `header.rs:258–276`; `canvas.rs:225–262` | S | Send the items to `view_item`. `crates/window` has `set_fullscreen` (`lib.rs:486`), but nothing calls it. |
| Pen test: what the pen sends (type, pressure, tilt, twist, buttons, samples a second, lateness) | `routes/pen-test/+page.svelte` (from home) | **Partial**: `pencilpilot --pen-check` measures from the command line. There's no page in the app. | `pencheck.rs`; `main.rs:28–30` | S | Design question 13. |

## 4. Settings

The current app has about 60 preferences in one Settings window (from the studio's header and
from home), kept in `settings.json`. The native app has none: nothing it lets you choose is kept
between runs, and there's no settings surface beyond each tool's own pop-up.

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| A Settings window, kept on disk and applied at once | `lib/SettingsPanel.svelte`; `routes/settings`; `lib/settings.svelte.ts` `loadSettings`, `saveSettings`; `src-tauri/store.rs` `settings_get`/`set` | **Missing** | none | M | The rows below hang on it. Design question 2. |
| Theme: light, dark, or as the system is | `routes/+layout.svelte`; settings `theme` | **Missing**: one light theme (the timeline's dark card is part of it) | `crates/ui/src/theme.rs:48–75` | M | Design question 3. |
| Size of everything (75–150 %) | `lib/appearance.ts` `setScale`; settings `uiScale` | **Partial**: it follows the monitor's scale (fractional scales too). There's no setting of its own. | `crates/ui/src/ui.rs:155`, `185`; `crates/window/src/lib.rs:416` | S | |
| Accent colour, the colour around the canvas, the checkerboard's square size and colour | `lib/appearance.ts` `lookVars` | **Missing** | none | S | |
| Left-handed: tools on the right, panels on the left | settings `leftHanded` | **Missing**. The layout is fixed. | `layout.rs:14–28` | M | Design question 4. |
| Tooltips on or off | `lib/appearance.ts` `tooltips` | **Partial**: tooltips show; they can't be turned off | `header.rs:325`; `canvas.rs:133` | S | |
| Pen and touch: pressure and tilt on or off, what one finger does (auto, draw, move), two- and three-finger taps | settings `penPressure`, `fingers`, `tapGestures` | **Missing** as settings | none | S | The behaviour is in drawing.md §3, §15. |
| Drawing preferences: cursor, brushes (size follows zoom, largest size, a size per brush), canvas and zoom (to the pointer, wheel, speed, smooth, rulers, units, ppi), selection (tint, marching ants) | `lib/settings/Cursor.svelte`, `Brushes.svelte`, `Canvas.svelte`, `Selection.svelte` | **Missing** as settings | none | S | The controls only. Each behaviour has its row in drawing.md. |
| New projects: size, frame rate, brush, grid size; the onion skin's defaults | `lib/settings/Guides.svelte`; settings `newWidth`, `newHeight`, `newFps`, `defaultBrush`, `gridSize`, `onion` | **Missing** | `files/mod.rs:545–548` (fixed 1920 × 1080, 24 fps) | S | |
| History and memory: undo steps, editable strokes, hold to pick, frame memory and its folder | `lib/settings/Performance.svelte` | **Missing** | none | S | Frame memory is in animation.md §12. |
| Files: the projects folder, how often to keep a recovery copy, versions kept, and where the app keeps its files (shown) | `SettingsPanel` Files; `src-tauri/lib.rs` `data_folder` | **Missing** | none | S | The features are in §1. |
| Exports and start-up: default export format, video quality, credits in exports, open the last project, how many recent projects | `lib/settings/Exports.svelte` | **Missing** | none | S | The features are in §1 and §2. |
| Questions and sounds: ask "Save before…?", ask before deleting one of My brushes, a sound snippet when stepping through frames | `lib/settings/Prompts.svelte` | **Missing**: the question is always asked, and the snippet always plays | `files/mod.rs:438–462`; `sound/mod.rs:322` | S | |
| Drawing together: your name (kept), your own relay | `SettingsPanel` Drawing together; settings `shareName`, `shareRelay` | **Partial**: the name is typed in the dialog and forgotten when the app closes; a relay only through `PENCILPILOT_RELAY` | `together_ui.rs:152`; `together/net.rs:173–190` | S | |

## 5. Language and accessibility

Language: both apps are English only, with no right-to-left layout, so there's no gap. The
native text engine already shapes any script (cosmic-text, with the system's locale for fonts:
`crates/ui/src/text.rs:181–187`).

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Screen readers: controls have names and roles the system's screen reader reads (through WebKit) | `aria-label` and roles throughout (21 files) | **Missing**: there's no accessibility tree (AT-SPI) | none | L | Design question 12. |
| The keyboard in dialogs and menus: Tab between controls, Enter, Escape | the HTML dialogs and menus | **Partial**: Escape closes pop-ups and menus. There's no Tab focus between controls. | `files/popup.rs:116`; `crates/ui/src/ui.rs:159` | M | |

## 6. Drawing together (the app's side)

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Host, join with a code, copy the code, who's here, chat, ask for a layer and give or take it back, the others' cursors, leave | `lib/ShareDialog.svelte`, `TogetherChat.svelte`; `src-tauri/lib.rs` `share_*` | **Done** | `together_ui.rs:1–40`; `together/mod.rs:33–52`, `371+` | — | ADR-0014, Phase 6. |
| The invite as a QR code | `src-tauri/lib.rs` `share_qr`; `lib/share.ts` `inviteQr` | **Missing** | none; ADR-0014:539–541 | S | |
| Rejoin a project's session from its file ("This project is shared": `extra.share`) | `ShareDialog` `canRejoin`; `share_rejoin` | **Partial**: it reconnects after losing touch mid-session, but not from a project opened later | `together/mod.rs:437–441`, `970–980`; ADR-0014:539 | S | |
| The same key for this computer as the current app (the others still know you) | `src-tauri/share.rs` `identity` | **Done**: it reads the current app's folder | `together/net.rs:146–171` | — | It helps the switch. |

## 7. Updates, about and what the app calls

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| The app updates itself from pencilpilot.app/updates. It looks 20 s after starting and every 6 h. The download is signed. A corner card offers Update, Later or Skip this version. It saves first. Settings has "Tell me when there's a new version" and Check now. | `lib/updates.svelte.ts`, `UpdatePrompt.svelte` (branch `self-update`); Tauri's updater plugin; `deploy/updates/README.md` | **Missing** | none | L | The channel serves Tauri's `latest.json` and its installers. The native app needs its own format (an AppImage or tarball with a minisign signature) or a channel of its own. Design question 7. |
| About: the version, and credits for the brushes and papers (CC0 packs, and resources by David Revoy and others under CC BY 3.0, attributed) | `SettingsPanel` About | **Missing** | none | S | **Check before the beta:** the native app compiles in six brushes from `apps/web/static/brushes/` (`deevad-*`, `aotz-040-marker`, `pencilpilot-rough-pencil`: `engine/mod.rs:95–116`). If any of them is under CC BY, the attribution is a condition of shipping it. |
| Third-party notices for what's bundled (FFmpeg's GPL text and NOTICE, Rhubarb's licence) | `src-tauri/ffmpeg/NOTICE.md`, `GPL-3.0.txt`; `rhubarb/` (bundle resources) | **Missing** (nothing is bundled yet) | none | S | Needed once FFmpeg or Rhubarb ships with the native app. The full licence page stays deferred, as the owner decided. |

**Not in the current app, so there's no gap:**
- **Onboarding, tutorial or help:** none.
- **Crash or feedback reporting:** none. `crates/feedback` is the practice website's drawing
  tips (ADR-0002), not app feedback.
- **AI generation:** the public app has no AI tools. There's only the guard that keeps AI solo
  (`lib/share.ts` `aiAllowed`, `src-tauri` `ai_allowed`, ADR-0013). The native app needs the
  guard once it has an AI tool.
- **Community and share-to-web:** the app calls no website route except the updates channel
  (`connect-src 'self' ipc:` in `tauri.conf.json`).
- **Accounts and sync:** parked by the owner (#277, #287) until the website is live.
- **`crates/grader`, `crates/feedback`, `crates/ref-filters`, `crates/ref-service`:** the
  website's practice and reference services. The app doesn't use them.
- **`plugins/pencilpilot-director`:** a separate product. It needs only `.ppstudio`
  compatibility (§1).
- **File associations and a single instance:** neither app has them. The Tauri bundle declares
  no file types. They're on the switch checklist below.

## 8. Platform and packaging

| Feature | Current app (file/function) | Native status | Evidence | Size | Notes |
|---|---|---|---|---|---|
| Installers for Linux (`.deb`, `.rpm`, `.AppImage`), Windows (`.msi`, `.exe`) and macOS (`.dmg`), built by CI and published by the Release workflow | `.github/workflows/app.yml` (bundle matrix), `release.yml` | **Missing**: CI tests the native app (fmt, clippy, `cargo test`) but packages nothing | `.github/workflows/ci.yml:23–25` | M | Linux only, for now. |
| A desktop entry and icons (the app in the menu, under Graphics) | the Tauri bundle (`tauri.conf.json` `bundle`, `icons/`) | **Missing**. The Wayland app id is `app.pencilpilot.studio`, but no `.desktop` file or icons are installed. | `crates/window/src/lib.rs:259` | S | The app id is the current app's, so the two clash when both are installed. Design question 8. |
| Windows and macOS | Tauri builds all three | **Missing**: `crates/window` is Wayland only (`#![cfg(target_os = "linux")]`). X11 works only through XWayland, as ADR-0014 decided. | `crates/window/src/lib.rs:29`; ADR-0014 decision 1 | L | Planned after Linux (ADR-0014). Design question 9. |

### Every Tauri command, and where it's covered

| Command(s) | What it does | Covered in |
|---|---|---|
| `project_save`, `project_open` | save and open `.ppstudio` | §1 (Done) |
| `recovery_write`, `recovery_list`, `recovery_open`, `recovery_discard` | recovery copies | §1 (Partial) |
| `versions_list`, `version_open` | version history | §1 (Missing) |
| `recent_list`, `recent_remove` | recent projects | §1 (Partial) |
| `settings_get`, `settings_set`, `data_folder` | settings, and where the app keeps its files | §4 (Missing) |
| `export_begin`, `export_frame`, `export_end`, `export_cancel`, `export_tools` | animation export, which encoders FFmpeg has | §2 |
| `export_write` | a single PNG or PSD | §2 |
| `export_audio` | the sound under a video | animation.md §8 (Done) |
| `video_in_open`, `video_in_frame`, `video_in_close` | video frames in | animation.md §12 (Missing) |
| `lipsync_tools`, `lipsync_audio`, `lipsync_run`, `lipsync_cancel` | lip-sync (Rhubarb) | animation.md §8 |
| `frames_put`, `frames_get`, `frames_swap`, `frames_retain`, `frames_stats` | the frame store, to RAM and disk (`crates/anim`) | animation.md §12 (Partial) |
| `share_host`, `share_join`, `share_rejoin`, `share_status`, `share_edit`, `share_direct`, `share_presence`, `share_leave`, `share_qr` | drawing together | §6 |
| `ai_allowed` | the solo-only AI guard | §7 (no AI tools) |
| `app_info` | version, OS, architecture (the pen test, the benchmark) | §3, §7 |
| `bench_echo`, `bench_report`, `bench_export_dir` | the developer benchmark (`routes/dev/bench`) | not user-facing |
| plugins: dialog (open, save, ask, message); updater and process (branch `self-update`) | the system's dialogs; self-update | §1; §7 |

---

## 9. Every keyboard shortcut

The current app's keys are `lib/shortcuts.ts` `ACTIONS`, plus the keys the page keeps for itself
(`page` `onkey`). Each can be changed in Settings. Rebinding is missing natively (drawing.md §1).
Native keys are bound in `tools.rs:186–191` (tools), `engine/tools/overlay.rs:385–422` (edit and
selection), `canvas.rs:231–246` (view), `strips.rs:264–279` (animation), `header.rs:107–123`
(file), `export_ui.rs:28` (Ctrl+E) and `shell/mod.rs:614–637` (Space, undo, redo).

| Key (current) | Action | Native status | Evidence / notes |
|---|---|---|---|
| B | Brush | Done | `tools.rs:43` |
| E | Eraser | Done | `tools.rs:44` |
| I | Eyedropper | Done | `tools.rs:49` |
| G | Fill | Done | `tools.rs:45` |
| F | Lasso fill | Done | `tools.rs:46` |
| U | Measure | Missing: U is the native **Shape** tool (pressed again, the next shape) | `overlay.rs:416–422`; the Measure tool is missing (drawing.md §8) |
| S | Smudge | Missing (no tool) | drawing.md §8 |
| J | Clone / heal | Missing (no tool) | drawing.md §8 |
| P | Pen path | Missing (no tool) | drawing.md §8 |
| V | Transform | Done | `tools.rs:48` |
| K | Liquify | Missing (no tool) | drawing.md §7 |
| M | Rectangle select | Done (Select) | `tools.rs:47` |
| L | Lasso select | Partial: L picks the native lasso fill | `overlay.rs:413–415` |
| W | Magic wand | Missing (no tool) | drawing.md §6 |
| Shift+G | Gradient | Missing (no tool) | drawing.md §8 |
| T | Text | Done | `tools.rs:50` |
| Shift+V | Select line | Missing (no tool) | drawing.md §12 |
| Ctrl+U | Adjust the layer | Missing | drawing.md §10 |
| [ | Smaller brush | Missing | drawing.md §2 |
| ] | Bigger brush | Missing | drawing.md §2 |
| Ctrl+Z | Undo | Done | `shell/mod.rs:633–638` |
| Ctrl+Shift+Z | Redo | Done | `shell/mod.rs:633–638` |
| Ctrl+C | Copy | Done | `overlay.rs:390–399` |
| Ctrl+X | Cut | Done | `overlay.rs:390–399` |
| Ctrl+J | Copy to a new layer | Missing | none |
| Ctrl+A | Select all | Done | `overlay.rs:390–399` |
| Ctrl+D | Deselect | Done | `overlay.rs:390–399` |
| Ctrl+Shift+I | Invert the selection | Missing (the key). "Invert selection" is a menu item. | `engine/tools/mod.rs:472` |
| Delete | Clear what's selected | Done (Backspace too, while there's a selection) | `overlay.rs:402–404` |
| = | Zoom in | Done (= and +) | `canvas.rs:236–238`. The View menu shows Ctrl++. |
| - | Zoom out | Done | `canvas.rs:239–240`. The View menu shows Ctrl+−. |
| 0 | Fit the canvas | Done | `canvas.rs:233` |
| 4 | Rotate left | Missing | drawing.md §15 |
| 6 | Rotate right | Missing | drawing.md §15 |
| 5 | Straighten the view | Missing | drawing.md §15 |
| H | Mirror the view | Done | `canvas.rs:235` |
| Alt+R | Reference pictures, show or hide | Missing | drawing.md §16 |
| , | Previous frame | Partial: ← instead | `strips.rs:271–273` |
| . | Next frame | Partial: → instead | `strips.rs:274–276` |
| N | New drawing after this one | Done | `strips.rs:264–266` |
| Shift+K | Mark the drawing: key, breakdown, in-between | Missing | animation.md §2 |
| R (hold) | Flip the drawings around this one | Missing. The View menu lists R for Rotate view, unbound. | `header.rs:45`; animation.md §12 |
| Shift+R (hold) | Flip the keys and breakdowns | Missing | animation.md §12 |
| Shift+M | Marker on this frame (tap while playing) | Missing | animation.md §7 |
| Alt+C | Copy frames | Missing (the menu shows it) | animation.md §2 |
| Alt+V | Paste frames | Missing (the menu shows it) | animation.md §2 |
| Shift+D | Duplicate frames as new drawings | Missing | animation.md §2 |
| Shift+Space | Play / stop | Partial: Return instead | `strips.rs:277–279` |
| Alt+I | Loop from this frame | Missing | animation.md §4 |
| Alt+O | Loop to this frame | Missing | animation.md §4 |
| O | Onion skin on / off | Done | `strips.rs:267–269` |
| Shift+O | Pin this frame to the light table | Missing | animation.md §5 |
| Ctrl+S | Save | Done | `header.rs:113–122` |
| Ctrl+Shift+S | Save as… | Done | `header.rs:109–116` |
| Ctrl+O | Open… | Done | `header.rs:113–122` |
| Ctrl+V (kept) | Paste (a picture from the system, or what was copied) | Partial: what was copied in the app; the system's clipboard carries text only | `overlay.rs:390–399`; `crates/window/src/clipboard.rs:1–6`; drawing.md §7 |
| Enter (kept) | Commit the transform; finish the pen path | Done for the transform | `overlay.rs:409–411` |
| Escape (kept) | Cancel the transform or pen path; deselect; close the adjust panel and menus; stop playing | Partial: cancels and closes pop-ups; doesn't stop playback | `overlay.rs:405–407`; `files/popup.rs:116` |
| Backspace (kept) | Take back the pen path's last point | Missing (no pen tool) | drawing.md §8 |
| Arrows (kept) | Nudge the picked lines (Shift: 10 px) | Missing (no line picking) | drawing.md §12 |
| Space (held) | Move the view | Done | `shell/mod.rs:615–617`, `217–219` |

**Keys only the native app has:** Ctrl+N (New), Ctrl+Q (Quit), Ctrl+E (Export), Ctrl+Shift+V
(paste in place), 1 (actual size), U (Shape), Return (play), ← / →.
**Shown in native menus but not bound:** Ctrl+I (Import…), F11 (Full screen), R (Rotate view).

Of the 61 shortcuts: 26 Done, 6 Partial, 29 Missing.

---

## Native-only (not parity: for the record)

- Open a project from the command line (`pencilpilot path.ppstudio`, `main.rs:76`). The current
  app takes no path. That makes file associations easy to add.
- "Show in folder" once an export is done (`export/mod.rs:424–445`).
- A layer-count note when a file has more layers than the painter holds
  (`files/mod.rs:596–605`).
- Full screen (F11) in the View menu, a stub for now (`header.rs:50`).

---

## Release and switch checklist

### Before a Linux beta

1. **Start empty or on a start page, never on the demo.** Today the app opens the design's
   "Ember" project (`shell/mod.rs:292`, `state.rs:186`).
2. **A package:** an AppImage, plus `.deb` and `.rpm` if they're wanted, built in CI on the same
   base as today's installers (`ubuntu-22.04` in `app.yml`, for the glibc it links against).
   - It installs a `.desktop` file (category Graphics, `Exec=pencilpilot %f`) and the icons from
     `apps/studio/src-tauri/icons/`.
   - It declares an `application/x-pencilpilot` MIME type for `*.ppstudio`, so the file manager
     opens projects. The native app already opens a path it's given.
3. **The dependencies say what the app needs:** Vulkan (a Mesa or NVIDIA driver), a Wayland
   compositor with `linux-dmabuf`, `xdg-desktop-portal` with a FileChooser backend, ALSA
   (PipeWire's or PulseAudio's ALSA plugin), and FFmpeg unless it's bundled. When the portal is
   missing, the app says so instead of failing silently (`dialog.rs:70–79`).
4. **The release build's settings:** the workspace's release profile is `opt-level = "s"`
   (`Cargo.toml:12–14`), while the Tauri app builds at `opt-level = 3`. Check which profile the
   ADR-0014 measurements used, and give `pencilpilot-native` its own `opt-level` if needed.
5. **NVIDIA and other desktops:** ADR-0014 checked RADV under mutter and Hyprland. The current
   app needed an NVIDIA fix (`b430eb7`), so test the proprietary NVIDIA driver, and GNOME and
   KDE (explicit and implicit sync).
6. **About, credits and notices:** credit the six compiled-in brushes as their licences require
   (§7), and add FFmpeg's notice if it's bundled.
7. **`--version`** (and `--help`, which prints only): there's no flag yet (`main.rs`), and bug
   reports need the version.
8. **Wire the visible stubs or hide them:** File › Import… and Project settings…, the header's
   View menu, Full screen, and the stubs listed in animation.md and drawing.md.
9. **Recovery every 20 s rather than 2 min**, with every copy offered (§1).
10. **Remember the together name** (one small file in `~/.config/pencilpilot`), so a beta
    tester isn't asked each time.
11. **The beta's updates:** keep the Tauri updater from offering the native build to current
    users (`latest.json` is Tauri's format), and decide how beta testers get new builds (a
    second channel, or download by hand). Design question 7.
12. **The beta next to the current app:** both use the app id `app.pencilpilot.studio` and the
    same together key folder. Decide whether a beta gets its own id (design question 8). Opening
    the same project in both at once isn't guarded in either app.

### Before the switch

1. **Parity:** the rows in app.md, drawing.md and animation.md are Done, or the owner has
   dropped them.
2. **Self-update in the native app** (§7), with the channel at pencilpilot.app/updates serving
   the native build for Linux.
3. **Move the current app's data on the first native start**, once, from
   `~/.local/share/app.pencilpilot.studio/` (Tauri's `app_data_dir` for the identifier; on
   Windows `%APPDATA%\app.pencilpilot.studio`, on macOS
   `~/Library/Application Support/app.pencilpilot.studio`):
   - `settings.json`: theme, keymap (`keys`), folders, the recovery interval, `versionsKept`,
     `shareName`, `shareRelay`, the new-project defaults, the onion skin, the export
     preferences, `reopenLast`, `recentCount`, and the workspaces (if they stay, animation.md
     §10).
   - `recent.json` (up to 30) into the native list (`~/.config/pencilpilot/recent`, which keeps
     10).
   - `recovery/<id>.ppstudio` with `<id>.json` (which file it's for): offer them. The native app
     looks in `~/.local/state/pencilpilot/recovery/` for `<name>.ppstudio` with `<name>.path`.
   - `versions/<key>/` with `path.txt`: keep this layout, so each project's history goes on.
   - `share/`: the native app already reads it (Done).
   - **What's in the webview's own storage** (WebKitGTK's `localstorage/`, an SQLite file):
     My brushes (`pp.myBrushes`), favourite brushes (`pp.favouriteBrushes`), brush textures
     (`pp.brushTextures`, `pp.brushTextureSettings`), compositions (`pp.myCompositions`), recent
     colours and My palette (`pp.recentColors`, `pp.myPalette`), pen firmness
     (`pp.pressureFirmness`), canvas-size presets (`pp.canvasSize`). Either the native app reads
     WebKit's file, or the last Tauri release writes these out as JSON in the data folder.
     Design question 10.
   - The frame cache (`app_cache_dir/frames`) has nothing worth moving.
4. **The last Tauri release hands over.** It offers the native build, and it writes out what's
   in the webview's storage (above).
5. **The packages replace the old ones:** the native `.deb` and `.rpm` declare that they
   replace the Tauri package's name, so upgrading doesn't leave two apps. The AppImage is
   self-contained.
6. **Windows and macOS:** either the native ports exist by then, or those systems stay on the
   Tauri app and only Linux switches (design question 9).
7. **The website's download page and the release workflow** publish the native build
   (`release.yml` gathers the Tauri files today).

---

## Design questions for the owner

1. **The start page:** does the new design have a home screen (New with templates and Custom,
   Recent with thumbnails, unsaved work, Draw together, shared projects)? Or does the app open
   the last project or a new one, with all of that in the File menu? The design's FullApp
   artboards show only the studio.
2. **Settings:** where do they live (a window from the header's mark, a File menu item)? Which
   of the current app's ~60 preferences stay? The design shows only per-tool settings.
3. **A dark theme:** is there one in the new design? The current app has light, dark and "as
   the system".
4. **Left-handed and the size of everything:** keep both? The native layout is fixed.
5. **Version history:** where does it go (File › Versions…, or the title's menu)? Keep 20 by
   default?
6. **FFmpeg on Linux:** bundle it (as today: the GPL notice comes with it, and H.264 has its
   own caveats) or depend on the distribution's package?
7. **Updates for the native app:** the same pencilpilot.app/updates channel in a new format,
   signed with the same minisign key? Or AppImage's own updates? And how do beta testers get
   builds?
8. **The app id:** the native app already uses `app.pencilpilot.studio`, the current app's, so
   the switch replaces it cleanly, but a beta installed next to the current app clashes (the
   dock icon, the `.desktop` file). Should the beta have its own id?
9. **Windows and macOS at the switch:** port first, or switch Linux alone and keep the Tauri
   app on Windows and macOS for a while?
10. **Moving what's in the webview's storage** (My brushes, textures, compositions, colours):
    read WebKit's SQLite file from the native app, or have the last Tauri release write it out?
11. **Export:** keep the native app's 720p / 1080p / 4K and add 75 % and a quarter, or go back to
    percentages? Is a single PNG picture a "This frame" choice under PNG, as PSD has?
12. **Accessibility:** is screen-reader support (AT-SPI) part of Phase 7, or later? It's the
    largest item here.
13. **The pen test:** keep it as a page users can open, or only as `--pen-check`?
14. **File › Project settings…:** what goes in it (canvas size, frame rate, the name)? And
    File › Import…: one item with a submenu (picture sequence, video, reference pictures, a
    picture as a layer)?
15. **Full screen (F11):** it's in the native View menu, but the current app never had it.
    Keep it?

---

## Totals

63 rows in sections 1 to 8. The shortcut table (§9, counted on its own line above) and the
native-only list aren't included.

| Status | Rows |
|---|---|
| Done | 12 |
| Partial | 20 (one of them Partial?) |
| Missing | 31 |

| Size of the gaps (Partial + Missing) | Rows |
|---|---|
| S (under ½ day) | 38 |
| M (1–2 days) | 10 |
| L (more) | 3: self-update, screen readers, Windows and macOS |

At those sizes, that's roughly 19 + 15 + 15 or more, so about 50 working days for this area.
Most of the S rows are settings, which all wait for the Settings window and file (M).

**First, for a Linux beta:** a start page instead of the demo; a package with a desktop entry
and MIME type; the Settings file; recovery every 20 s; About and credits; wiring or hiding the
stubs.
**Then, for the switch:** self-update, version history, moving the current app's data, and the
export formats.
