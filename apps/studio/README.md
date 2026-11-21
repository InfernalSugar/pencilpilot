# PencilPilot (the app)

The drawing and frame-by-frame animation app: a [Tauri 2](https://tauri.app) shell around
the studio from [`@pencilpilot/paint`](../../packages/paint), with the same look as the
website ([`@pencilpilot/ui`](../../packages/ui)). The plan is in
[docs/planning/STUDIO_APP.md](../../docs/planning/STUDIO_APP.md).

## Run it

From the repo root:

```sh
pnpm install
pnpm app          # builds the paint engine (WASM), then opens the app with live reload
pnpm app:build    # release build + installers (src-tauri/target/release/bundle)
```

Linux needs the WebKitGTK 4.1 and GTK 3 development packages (see Tauri's
[prerequisites](https://tauri.app/start/prerequisites/)). Windows needs WebView2
(included in Windows 10/11). macOS needs the Xcode command-line tools.

## Projects

Ctrl+S saves a `.ppstudio` file (Ctrl+Shift+S: save as), Ctrl+O opens one. The format is
described in [crates/ppstudio/FORMAT.md](../../crates/ppstudio/FORMAT.md).

## Tests

```sh
pnpm --filter @pencilpilot/studio test       # unit tests
pnpm --filter @pencilpilot/studio test:e2e   # the window in a browser, Tauri stubbed
cd apps/studio/src-tauri && cargo test       # the Rust side
```

## Layout

| Path                      | What                                                                                         |
| ------------------------- | -------------------------------------------------------------------------------------------- |
| `src/routes/+page.svelte` | the studio (canvas, tools, layers, brushes, colour, history)                                 |
| `src/routes/pen-test/`    | pen check: pressure levels, tilt, buttons, sample rate, event delay (S0-T05)                 |
| `src-tauri/`              | the Rust shell: window, security policy, commands (the native engine joins in S1)            |
| `vite.config.ts`          | SvelteKit as a single-page app (adapter-static); copies the brush pack into `static/brushes` |

`src-tauri` is its own Cargo project, outside the main workspace, so building the
website and the engines never needs the webview libraries.
