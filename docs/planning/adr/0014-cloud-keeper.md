# ADR-0014: The cloud: an always-on member that keeps shared projects

- Status: accepted (2026-11-23). Milestone S12 in [STUDIO_APP.md](../STUDIO_APP.md).
- Builds on [ADR-0013](0013-solo-and-shared-projects.md) (solo and shared projects, peer to
  peer over iroh, sign-in and passes).

## What the owner asked for

"Add cloud sync so whatever is drawn is sent to the cloud, which solves the whole P2P
problem of someone must be online to draw." For **shared projects only**: solo projects
stay private and open no connection. And: "the cloud saved files should appear on the main
page as their own separate tab for quick and easy access to everyone within the project."

## The problem

Drawing together is peer to peer (ADR-0013). A member coming back to a shared project needs
another member online to catch up from. With no one online, the app hosts its own copy
again, so two members coming back apart each host their own copy, and the two drift (the
known gap "merging two copies that were each hosted apart"). And a project is only on the
computers of the people in it: no way to open it on another computer.

## Decision: an always-on member, the keeper

A server process, the **keeper** (`crates/keeper`, deployed by `deploy/keeper` on the
relay's VPS), joins each shared project's session **as a member who never leaves**, with
the same protocol as people (`pencilpilot/share/2`):

- **It signs in like a person.** The website signs it a long-lived pass for its computer
  key, with the account `pp:cloud` (the site's own accounts are `u_…`, so no person's pass
  can say that). Apps check its pass as they check everyone's; they know it's the cloud by
  that account.
- **It never hosts, draws or takes over.** It's never a candidate in a host hand-over;
  when the host leaves it follows whoever takes over, and when the last person leaves its
  session simply ends. Cursors and live strokes don't go to it. **Live drawing stays
  between the members' apps**: the keeper only adds catch-up and storage.
- **It keeps the project as the last snapshot plus the edits since.** It asks the host
  for the same snapshot a joiner gets (`snap.req`), keeps every numbered edit after it,
  and now and then (every 30 minutes while people draw, or when the edits pass 32 MB)
  asks again: a whole snapshot replaces every edit up to it (**compaction**). An *epoch*
  counts its stays in the session (sessions number edits from 1 each time), so (epoch,
  seq) orders everything it ever kept.
- **On one endpoint for all projects** (`Hub` in crates/share): a joiner reaching it is
  passed on to their session's host, like any member does, so its address works as a
  meeting point for every project.

### The cloud protocol (`pencilpilot/cloud/1`)

Apps talk to the keeper on its own ALPN, from a throwaway key (the computer's own key may
be in a live session): the app shows its pass and a signature, with the computer key the
pass names, over the keeper's id and the connection's. Then: **List** (the projects my
account is in), **Keep** (here's a project's invite: keep it and join its session),
**Touch** (its name and picture changed), **Open** (how to open it: live, someone else is
opening it now, or no one is in it), **CatchUp** (the snapshot, then each edit since).

- **Members only.** An account is a member of a project once it has been in a session the
  keeper was in, or had the keeper keep it (with the invite, which holds the project's
  secret: whoever has it was invited, as before). Others get "You aren't a member".

### Coming back

When an app rejoins a shared project (Rejoin, or a project opened from the Cloud tab):

1. It asks the keeper to **Open** it. Live: it joins through the keeper (who passes it on
   to the host). Someone else is opening it right now: it waits a moment and joins them
   (so two people coming back at once meet in **one** session: the keeper is the meeting
   point that fixes the "hosted apart" gap). No one in it: the keeper notes this app is
   opening it (45 s), and…
2. …the app **catches up** from the keeper's copy, as if a host had sent it, **hosts the
   session again**, and has the keeper **Keep** it (the keeper joins and takes a fresh
   snapshot).
3. **Drawing done while offline:** on the app's own layers, it merges (sent as the app's
   tiles, as rejoining already did). On others' layers, which the app couldn't draw on in
   the session, the changed tiles (found against per-tile checksums the file keeps from the
   session) are **kept aside** (in the project file too) and become an **ask** to the
   owner, with the existing ask/grant flow: yes puts them in over the owner's current
   drawing (other tiles they drew meanwhile stay), no drops them.

Without a cloud set, rejoining works as before (join someone online, or host this copy).

### Home's Cloud tab

A tab next to Recent: every shared project the signed-in account is in, from the keeper
(name, picture, members, who's drawing now, when it last changed, "cloud space full"). One
click opens it: from its file when it's on this computer (and rejoins), else a new canvas
its size that joins or catches up. Signed out: a prompt to sign in. Out of reach: said
plainly, with the last list fetched (kept in the data folder). Nothing connects until the
tab is shown, and never for someone signed out.

### Settings and deployment

- Settings → Drawing together → **Cloud**: the keeper's address (`<id>@host:port`), or
  `off`. Empty: `PENCILPILOT_CLOUD` when the app runs, else `PENCILPILOT_CLOUD_ADDR` when it
  was built (a repository variable, like the site's address), else no cloud.
- `deploy/keeper`: config, a sandboxed systemd service (own user, writes only its folder,
  no privileges, UDP 7843), and an install script that builds it from the checkout and
  prints its id. `apps/web/scripts/keeper-pass.mjs` makes its pass with the site's key.

### Limits

- **Space per account** (default 1 GB): a project counts against the account that first
  had it kept. Past the limit, new edits to its projects aren't kept (the project is
  marked full, and the Cloud tab says so) until a snapshot makes room; no new project is
  taken on. Projects per account: 200.
- Snapshots arrive in pieces (≤ 48 tiles each, as for joiners) and are written to a file
  as they come; files are replaced by renaming, so a catch-up reading the old ones
  finishes reading them whole.

## Not yet (later)

- **End-to-end encryption.** The keeper stores what it receives as it is, and holds each
  project's secret, so whoever runs the server can read the projects. The seam: the keeper
  only reads an edit's author and a snapshot message's `k` and `seq`. Apps would seal each
  edit body and snapshot message with a key derived from the project's secret before
  sending (members already share it), the keeper would keep the sealed bytes unchanged,
  and the project's secret would no longer go to it (Keep would send only the project id
  and membership would rest on passes). Noted in `crates/keeper/src/store.rs`.
- Removing a project (or yourself) from the cloud; an owner moving it to another account.
- Edits made with no cloud and no one online still merge only with the next person to
  join this copy (as before).

## Considered

- **The keeper as the host.** Simpler (it orders every edit), but then all live drawing
  goes through the VPS. Rejected: live drawing stays peer to peer, as decided in ADR-0013.
- **A server that understands edits** (applies them itself to keep a current project and
  compact on its own). It would duplicate the app's rules (`Shared.apply`) in Rust and
  have to change with every new kind of edit, and it couldn't work on sealed bodies later.
  Asking a member for a snapshot keeps the keeper content-agnostic.
- **Sending offline edits as a queue of operations.** The app already merges by state on
  rejoining (its own layers' tiles against the session's); checksums of others' tiles
  find what was drawn on them without keeping a log of every stroke.
