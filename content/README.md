# Content

Teaching content for PencilPilot. Teaching text in this folder (`feedback/`, `lessons/`) is licensed
**CC BY-SA 4.0** (https://creativecommons.org/licenses/by-sa/4.0/), separately from the MIT-licensed code.

| Folder | What goes here | Milestone |
|---|---|---|
| `feedback/rules.toml` | Diagnosis rules: measurement + threshold → mistake | M4 |
| `feedback/tips.toml` | Tip text for each mistake + praise, 2–4 variants, with `{placeholders}` | M4 |
| `topics.json` | Topics per category + Pixabay search phrases | M5 |
| `lessons/` | Mini-lessons and drills (Markdown) | M6 |
| `references/` | One `manifest.json` per category: source URL, licence, author, keypoints, approval flag. **Image files are never committed.** Keypoints are `{ x, y, label }` with `x`, `y` as fractions (0–1) of the picture, in the order students tap them; mark them in `/dev/keypoints`. | M5 |

See `docs/planning/CONTENT_GUIDE.md` for the writing style and `docs/planning/LICENSING.md` for reference-image rules.
