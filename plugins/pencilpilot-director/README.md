# PencilPilot director (a Claude Code plugin)

Tell Claude what you want to make; it walks you through it the way a studio does, from
the idea to the exported video, one step at a time:

idea → script and shot list → storyboard → animatic → key poses → motion → in-betweens
and clean-up → colour → edit and export

- **Stills** (storyboards, keyframes, fixes) come from your local **Qwen-Image** in
  ComfyUI: free, on your GPU.
- **Motion** comes from **Seedance** through **OpenRouter** (pay per use: about $0.16
  for a 4 s draft at 480p). Claude tells you the cost first and waits for your yes.
- Each shot is **final** (the AI picture is the art) or **traced** (the AI frames are a
  guide and you draw the lines), chosen per shot.
- The film is a **PencilPilot project** (`.ppstudio`): a cut per shot with its timing
  and transitions, layers ready for lines, colour and shadow. Claude can read back what
  you draw.

## Install

From a Claude Code session (once):

```
/plugin marketplace add ~/Drive/Projects/pencilpilot
/plugin install pencilpilot-director@pencilpilot
```

Then start a film with `/new-film a girl waits by a window as the storm ends`, or carry
on one with `/film-next ~/Videos/PencilPilot/Ember`.

## Set up

| What                                      | How                                                                                                                      |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| OpenRouter key (clips)                    | `export OPENROUTER_API_KEY=...` in the shell you start Claude Code from                                                  |
| ComfyUI (stills)                          | running with the Qwen-Image 2.1 models; elsewhere than `127.0.0.1:8188`: `export PENCILPILOT_COMFY_URL=http://host:8188` |
| Other Qwen model files                    | `PENCILPILOT_QWEN_UNET`, `PENCILPILOT_QWEN_CLIP`, `PENCILPILOT_QWEN_VAE`                                                 |
| ffmpeg                                    | on the PATH (or `PENCILPILOT_FFMPEG`)                                                                                    |
| Node.js 22+ and the repo's `pnpm install` | for the plugin's server                                                                                                  |

The `ppstudio` tool that makes projects (`crates/ppstudio-cli`) is built from this
repository the first time it's needed (`cargo build --release -p pencilpilot-ppstudio-cli`).

## Tests

`pnpm --filter @pencilpilot/director-server test` runs the server against a fake
ComfyUI and a fake OpenRouter (no key or GPU needed), with real ffmpeg and the real
`ppstudio` tool.
