# ADR-0003 — Reference images: licence-clean, SFW, human-approved

**Status:** Accepted (2026-11-18)

## Allowed sources
| Source | Licence | Notes |
|---|---|---|
| Pixabay (API) | Pixabay Content License | Main source for anime/cartoon illustrations; show "via Pixabay"; cache API results 24 h; no mass downloading |
| Unsplash / Pexels (API) | Their own free licences | Real life, animals, environment |
| OpenGameArt | **CC0 or CC-BY only** | No GPL/CC-BY-SA-only art; keep attribution |
| Wikimedia Commons | Per-file; CC0 / CC-BY / CC-BY-SA / PD | Check every file; never "fair use" |
| rawpixel public domain | Public domain | Japanese woodblock prints etc. |

## Banned
Pinterest, Danbooru/Safebooru, Google Images, ArtStation packs, and anything depicting existing franchise characters, logos or brands (fan art). Also **AI-generated images**: they often have broken anatomy, which would teach mistakes.

## Process
1. `ref-importer` pulls candidates with SafeSearch on and records source URL, author, licence and licence URL.
2. Each image goes into an approval queue. A person checks it for NSFW content, AI artefacts, franchise characters and quality before setting `approved: true`.
3. Keypoints are marked in `ref-annotator`.
4. CI fails if any manifest entry lacks source, author, licence or approval.
5. Image files are never committed. The site fetches and caches them, and always shows attribution.
