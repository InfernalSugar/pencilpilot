# ADR-0013: Two kinds of project: solo (private) and shared (live, together)

- Status: accepted (2026-11-22): **peer to peer over iroh** for shared projects, with a
  server as the fallback design. Milestone S12 in [STUDIO_APP.md](../STUDIO_APP.md).
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

## Decision

### Solo stays exactly as it is, and says so

- No account, no server, no connection. The start screen offers **My projects** (solo,
  the default) and **Shared projects**. Solo never touches the network.
- Keep it guaranteed:
  - a test that fails if the window's CSP gains a network origin
  - shared mode's connection lives in the Rust shell, used only for shared projects
  - the shell refuses to open a connection for a solo project
- Making a solo project shared is always an explicit **Share a copy…**. The private
  original stays on disk, untouched and still private.

### Shared projects live on every member's computer: peer to peer over iroh

The members' apps connect to each other directly with [iroh](https://iroh.computer): QUIC,
encrypted end to end, with hole punching through home routers. **Nobody forwards ports
or changes router settings.**

- **A relay** introduces members and carries the traffic of those who can't connect
  directly (strict office or school networks). It's encrypted, so the relay sees only
  metadata: IP addresses, member ids, when and how much, never the drawings.
  - Building and testing: iroh's free public relays.
  - A release: our own `iroh-relay` on a small VPS (KVM 1 or 2), so no one else sees
    users' metadata and uptime is ours.
  - Optional later: relay-only mode, so members never learn each other's IP addresses
    (for projects with strangers).
- **Every member's app keeps the whole project** (the `.ppstudio` tiles by hash, plus the
  change log). The work literally belongs to everyone: anyone can open it, draw on their
  own copy offline, and export, any time.
- **The host:** one online app (the one that has been in longest) numbers every edit and
  passes it on, so all apps apply edits in the same order. The others connect to it. When
  it leaves, the next one takes over: **host hand-over**.
- **Cursors** travel as QUIC datagrams: unreliable, newest wins, never stored.
- **Joining needs one member online,** to hand over the project. Opening an old project
  with nobody else online works on your own copy and syncs when someone comes back.
- `crates/share` (pencilpilot-share) does this: identity, invites, the session (host
  ordering, cursors, joins and leaves), tested between apps on one machine and, manually,
  through iroh's public relays.

Fallback, not chosen: projects on our own server (the website's Node server; SQLite plus
tiles by hash on disk). Always available and simpler to make consistent, but the server
stores and passes on everything. It stays the plan B if peer to peer proves unreliable,
and an always-online member running on the VPS is a later middle ground.

### AI generation is a solo act

The owner's rule: AI is for solo work only. Shared projects are drawn by people.

- **No AI in a shared session:** while a session is live, AI tools are hidden in the window
  (`aiAllowed` in `lib/share.ts`) and refused by the shell (`ai_allowed`, which every AI
  feature calls before running).
- **Nothing AI travels through a session:** the session's edits are a closed list:
  strokes, tiles, layers and timeline edits. An edit of any other kind is dropped on
  arrival.
- **Share a copy** of a solo project leaves its AI layers behind (the guides from S6,
  marked with their provenance per ADR-0011) in the private original.

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

### Identity and joining (no accounts)

- **Identity is a key per computer** (`Identity`): made on first use, kept readable only
  by the user, never sent. Its public half is the member's id; every change and credit
  is tied to it. Your name is what you type the first time.
- **Inviting:** any member's app shows an **invite code** (`pp1-…`, about 140 characters)
  to send however you like. It holds the project's secret (the project's id is derived
  from it) and who to ask: the code's maker, then the host, each by id and relay. Home IP
  addresses stay out of codes. Any member passes a joiner on to the host.
- **Edits are signed** by their author's key and numbered by the author, so no one can pass
  one on as someone else's, and none is applied twice (also across a host hand-over). A joiner proves it holds the secret with a keyed hash bound to both
  ids, so the secret itself never crosses the network and a proof can't be reused.
- **Removing a member:** the project's creator makes a new secret (old codes stop
  working) and the apps stop accepting that member's id. Their layers stay, since the work
  belongs to everyone, but they can't draw any more.
- Later, optionally: link an identity to a website account, for a profile and credits
  across devices.

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
- **Joining a project:** fetches the project's list of tile hashes first, then the tiles
  near the playhead, then the rest in the background, from the host.
- **Host upload:** the host passes every edit to everyone (a star), so with 8 people a
  100 KB stroke is about 700 KB of its upload. Fine on home connections. If it isn't, edits
  can later go member to member (iroh-gossip).
- **Each copy:** the change log is compacted into the project file now and then.
- **The relay** stores nothing and costs little. Ours runs on the VPS.

## Risks

| Risk | Plan |
| --- | --- |
| Lag between drawing and others seeing it | Live preview from the points (≈50 ms); the tile change replaces it |
| The preview and the final pixels differ (another app version) | The tiles are the truth; the preview lasts a moment |
| Big imports (a 4K picture, video frames) flood the link | Upload in the background with progress; others see a placeholder until it arrives |
| Someone vandalises a project | Invite only; owners control their layers; the full history lets any change be undone or a member removed |
| Nobody online to join from | Say so plainly; later, an always-online member on the VPS |
| The host leaves mid-session | Host hand-over to the longest-connected member; unsent edits are resent to the new host |
| Members see each other's IP addresses (direct connections) | Fine among friends; relay-only mode for projects with strangers |
| iroh's public relays go down or rate-limit | Our own relay before release |
| Solo privacy slips through a later change | The CSP test and the "no connection for solo" rule in the shell |

## Questions for the owner

1. ~~Where it runs~~: peer to peer over iroh (decided 2026-11-22).
2. ~~Guests~~: no accounts needed; a key per computer (decided with peer to peer).
3. **Owner away:** when a layer's owner is offline and someone asks to draw there, who can
   say yes? The project's creator? An automatic yes after some time? Nobody until they're
   back?
4. **"Credits":** credits as in who made it (authorship), or also AI credits (money)
   spent on the shared project? If money: who pays?
5. **Rights:** "belongs to everyone": may any member use it commercially, or only with
   everyone's agreement? This decides the note people accept when they join.
