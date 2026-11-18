# Licensing

| What | Licence |
|---|---|
| Code (`apps/`, `crates/`, `tools/`) | MIT (`/LICENSE`) |
| Teaching content (`content/rules`, `tips`, `lessons`) | CC BY-SA 4.0 |
| Reference images | Their original licence. Never committed; attribution shown on every draw/results screen |

Reference image rules and the source allow-list are in [ADR-0003](adr/0003-reference-images.md).
Each manifest entry needs: `id`, `category`, `source_url`, `author`, `licence`, `licence_url`, `approved`.

Never commit API keys. Use `.env` locally (git-ignored) and GitHub Actions secrets in CI.
