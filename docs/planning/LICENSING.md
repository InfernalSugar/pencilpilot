# Licensing

| What | Licence |
|---|---|
| Code (`apps/`, `crates/`, `tools/`) | MIT (`/LICENSE`) |
| Teaching content (`content/rules`, `tips`, `lessons`) | CC BY-SA 4.0 |
| Reference images | Their original licence. Never committed; attribution shown on every draw/results screen |

Reference image rules and the source allow-list are in [ADR-0003](adr/0003-reference-images.md).
Each manifest entry needs: `id`, `category`, `source_url`, `author`, `licence`, `licence_url`, `approved`.

Never commit API keys. Use `.env` locally (git-ignored) and GitHub Actions secrets in CI.

## Brushes and brush engine

| What | Source | Licence |
|---|---|---|
| Brush engine | [hokusai](https://github.com/reearth/hokusai) 0.3, a pure-Rust port of [libmypaint](https://github.com/mypaint/libmypaint) | MIT OR Apache-2.0 |
| Brush pack (196 `.myb` brushes + preview images in `apps/web/static/brushes/`) | [mypaint-brushes](https://github.com/mypaint/mypaint-brushes) v2.0.2 | CC0-1.0 (per the pack's `Licenses.dep5`). Artists: the MyPaint team, David Revoy, Brien Dieterle, Guillaume Loussarévian, Ramón Miranda, Marcelo “Tanda” Cerviño |

The pack's build scripts are GPL-2+ and are **not** vendored. Regenerate the brush folder with
`node tools/brush-pack/build.mjs <extracted mypaint-brushes v2.0.2>`.
