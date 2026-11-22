# ADR-0013: Two kinds of project: solo (private) and shared (live, together)

- Status: proposed (2026-11-22). Milestone S12 in [STUDIO_APP.md](../STUDIO_APP.md).
- Builds on [ADR-0008](0008-private-lobbies.md) (the website's live lobbies) and
  [ADR-0012](0012-where-painting-runs.md) (where the app paints).

## What the owner asked for

The app has two modes:

1. **Solo.** Open the app and draw. The work is entirely private: nothing leaves the
   computer, it belongs to the person, and they can export it.
2. **Shared.** A project that several people draw at the same time. The work belongs to
   everyone and the credits are shared. Drawing is live. Each person has their own layer
   and can ask to draw on someone else's. Layers can be added as in solo. Everyone sees
   the others' cursors.

## Where we start from

- **The app is already private.** Projects are `.ppstudio` files on disk. The window's
  content security policy only allows `self` and Tauri's IPC (`connect-src 'self' ipc:
  http://ipc.localhost`), so the page itself can't reach the internet. The only network
  use is AI jobs, and only when the person sets them up (ADR-0011).
- **The website already does live drawing** (private lobbies, ADR-0008):
  - a WebSocket on the site's Node server, rooms in memory
  - signed join tickets, with accounts or guests
  - strokes streamed as stroke data (`sb` / `sp` / `se`), redrawn by the same engine
  - the engine draws several people's strokes at once (`remote_begin/to/end`), each on
    their own layer and outside the local undo
  - That covers one canvas, one session, nothing kept. A shared project needs layers,
    frames, the timeline, sound, undo, and keeping the work between sessions.
- **The file format suits syncing.** Pictures are 64×64 tiles named by their BLAKE3 hash.
  A change is "these tiles of this drawing now have these hashes". Tiles everyone already
  has are never sent again.
- **Replaying strokes isn't enough for a record.** Live painting runs the same
  WebAssembly engine in the app and on the site (ADR-0012), so replays usually match. But
  members may run different app versions, and fills, transforms, filters and imports
  aren't strokes. So strokes can't be the shared record on their own.

## Decision (proposed)

### Solo stays exactly as it is, and says so

- No account, no server, no connection. The start screen offers **My projects** (solo,
  the default) and **Shared projects**. Solo never touches the network.
- Keep it guaranteed:
  - a test that fails if the window's CSP gains a network origin
  - shared mode's connection lives in the Rust shell, used only for shared projects
  - the shell refuses to open a connection for a solo project
- Making a solo project shared is always an explicit **Share a copy…**. The private
  original stays on disk, untouched and still private.

### Shared projects live on our server

- The website's Node server keeps them, next to the lobbies (one deployment, accounts
  already there).
- It stores:
  - **SQLite:** projects, members, layer access, the change log
  - **a folder of tiles by hash:** the same blobs as in `.ppstudio`, plus sounds and
    imported pictures
- The project stays available when any one person is offline, which fits "belongs to
  everyone".
- Each member's app keeps a local cache, so opening is fast and tiles download once.
- Not chosen: one player's app as the server (peer to peer). It's cheaper, but the project
  disappears when that person is offline and joins break on home routers.

### Each layer has one drawer at a time, so drawings never conflict

- Every layer has an **owner** (who made it) and a list of **people allowed to draw on it**.
  Only they can change its pixels.
- Two people can never change the same pixels at once, so no merge logic is needed for
  drawings. This is the rule that makes the whole design simple and reliable.
- **Asking for access:**
  1. Someone taps "Ask to draw here" on a layer.
  2. The owner gets a prompt: **Let them draw**, **Hand it over**, or **No**.
  3. The owner can take access back at any time.
  4. If the owner is away, the request waits (see the questions below).
- **Adding a layer** works as in solo, and the person who adds it owns it. Deleting a layer
  is the owner's call. Its pixels stay in the history, so it can be brought back.

### Two streams: live strokes for speed, tiles as the record

1. **While a stroke is being drawn,** its points stream to everyone, as in lobbies, and each
   app draws it as a live preview on that layer. Others see the line appear as it's drawn.
2. **When the stroke ends,** the drawer's app sends a **change**:
   - which layer and drawing
   - the new hash of each tile it touched
   - the bytes of any tile the server doesn't have yet
3. The server numbers the change, saves it and passes it on.
4. Every app swaps the tiles in, replacing its preview with the drawer's exact pixels.

The same works for everything that isn't a stroke: fill, transform, filters, paste,
importing a picture. They all end as tile changes, so they sync with no extra code.

### Everything else is a list of small edits, in the server's order

- **Layers:** add, remove, reorder, name, opacity, blend, groups, masks.
- **The timeline:** scenes, cuts, frames, exposures, key/breakdown marks, transitions,
  dialogue, sound clips.
- **Project settings:** canvas size, fps.

How they sync:
- The server puts every edit in one order, so everyone ends up with the same project.
- Two edits to the same thing (two people renaming one cut): the later one wins.
- Edits that would wreck someone else's work are refused, so the app shows why:
  - changing the canvas size
  - deleting a scene with other people's drawings in it
  - deleting someone else's layer

### Undo is your own

- Ctrl+Z undoes **your** last change, never someone else's. In tiles, undo is a new change
  that puts back the old hashes.
- If someone else has since drawn over those tiles (on a layer you gave them), the undo
  says so instead of wiping their work.
- In shared mode, the engine's undo history is kept per layer you can draw on (it is one
  stack today, see `Edit::Tiles` in `crates/brush`).

### Cursors, presence and following

- Each app sends its pointer about 20 times a second:
  - position, tool, colour and size
  - the layer and frame it's on
- Only the newest position counts and nothing is stored.
- Others see a named, coloured cursor. When they're on another frame, a small badge on
  the timeline shows where they are.
- The members panel shows who is here, what they're drawing on, and **Follow** (your view
  follows theirs).

### Credits are shared and kept automatically

- Every change is saved with who made it. The project knows who drew which layers and
  drawings, how many strokes, and roughly how long each person drew.
- **Credits panel:** every member, with what they worked on.
- **Exports:** carry the credits in the file's metadata, and can add an end card.
- **Saving a copy** to a solo file keeps the credits with it.
- **Any member can export** the film or save a copy.
- **Joining** shows a short note: the work is shared, every member may use and export it,
  and credits travel with it. (This wording needs a legal check, see the questions.)

### Accounts and joining

- A shared project needs an account. Credits need a name that stays with the person, and
  the project needs to know who may draw where.
- **Signing in from the app:** it opens the website's sign-in in the browser. The site
  hands the app a token through a `pencilpilot://` link or a code on localhost. The token
  is kept in the OS keychain, as AI keys are.
- **Inviting:** by a link or a code, as with lobbies. The project's creator can remove a
  member. Their layers stay, since the work belongs to everyone, but they can't draw any
  more.

### Going offline and coming back

- Changes you make while offline wait in a queue and go out when you reconnect.
- Your layers can't conflict, since only you draw on them.
- If your access to a layer was taken away meanwhile, your offline drawing on it becomes
  a new layer of yours ("Sam's changes, offline"), so nothing is lost.

## Sizes and limits (first version)

- **People:** up to 8 at once, as in lobbies.
- **Data per stroke:**
  - Tiles are 16 KB raw. Compressed they're usually 1–4 KB, and ink lines are mostly
    empty space.
  - A typical stroke touches 5–30 tiles, so about 20–100 KB once it ends. That's fine
    for home connections.
  - The live points stream is tiny: 120 messages a second per person is already the
    lobby limit.
- **Opening:** fetches the project's list of tile hashes first, then the tiles near the
  playhead, then the rest in the background.
- **Server:** the change log is compacted into snapshots now and then, and tiles nothing
  points to are cleared out after 30 days.
- **One server process** to begin with. More would need a shared store, as noted in
  ADR-0008.

## Risks

| Risk | Plan |
| --- | --- |
| Lag between drawing and others seeing it | Live preview from the points (≈50 ms); the tile change replaces it |
| The preview and the final pixels differ (another app version) | The tiles are the truth; the preview lasts a moment |
| Big imports (a 4K picture, video frames) flood the link | Upload in the background with progress; others see a placeholder until it arrives |
| Someone vandalises a project | Invite only; owners control their layers; the full history lets any change be undone or a member removed |
| Server costs grow with stored tiles | Shared tiles stored once, compaction, a size limit per project |
| Solo privacy slips through a later change | The CSP test and the "no connection for solo" rule in the shell |

## Questions for the owner

1. **Where it runs:** on the website's server (recommended), or should one player's app
   host?
2. **Guests:** account only (recommended, credits need a lasting name), or can guests
   join and draw without credits?
3. **Owner away:** when a layer's owner is offline and someone asks to draw there, who can
   say yes? The project's creator? An automatic yes after some time? Nobody until they're
   back?
4. **"Credits":** credits as in who made it (authorship), or also AI credits (money)
   spent on the shared project? If money: who pays?
5. **Rights:** "belongs to everyone": may any member use it commercially, or only with
   everyone's agreement? This decides the note people accept when they join.
