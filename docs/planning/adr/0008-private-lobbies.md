# ADR-0008: Private lobbies (real-time)

- Status: accepted (2026-11-19), M10-T08

## Decision

- **Where it runs.** The live connections are handled inside the website's own
  Node server, on a WebSocket at `/lobby-ws`, not in a separate service. This
  means one deployment, with accounts and sessions already there.
  - Development and preview: a small Vite plugin attaches the socket.
  - Production: `node server.js` wraps the SvelteKit handler and attaches the
    same socket (`pnpm build` bundles it to `build/lobby-socket.js`).
- **State.** Rooms live in memory (`lib/server/lobby/hub.ts`, a plain state
  machine with unit tests) and close 30 minutes after the last person leaves.
  Nothing about a game is stored; players save their own drawing if they want.
- **Joining.**
  - The web server signs a short ticket (HMAC): this browser is player X,
    named Y, signed in as Z (or a guest).
  - The socket checks the ticket on join.
  - Hosts need an account; guests join with a link and a name.
  - A signed-in person's player id is derived from their account and the
    room, so they rejoin as the same player on any device.
  - Removed players can't come back.
- **Strokes travel as stroke data,** not pixels: style plus points
  `[x, y, pressure, dt]`, batched once per frame. Every browser redraws them
  with the same brush engine. The engine can run several strokes at once
  (`remote_begin/to/end`), each on its owner's layer and outside the local
  undo history. When someone undoes or clears, everyone redraws that
  person's layer from their remaining strokes. Late joiners get the whole
  canvas on join.
- **Competition** is driven by the server:
  - Rounds run on the server's clock, and browsers correct for clock skew.
  - At time-up the canvases lock and each browser grades its own drawing
    (like everywhere else on the site) and hands it in. There is a 15 s
    grace period; the round ends early once everyone has handed in.
  - Drawings are revealed one at a time in a shuffled order, with names,
    and players can like or dislike them.
  - The highest grader score wins the round. The game goes to the highest
    total, with ties broken by round wins.
  - "Play again" returns to the same lobby.
- **Pictures.** The host's browser picks them: hand-picked pictures first,
  Endless ones to fill up. The server only accepts our own image paths.

## Limits

- Scores come from the browsers, as with the daily challenge. Fine among
  friends; server-side grading is possible later.
- In-memory rooms mean one server process. Several processes would need a
  shared store (e.g. Redis).
- Up to 8 players. About 3 million stroke numbers per shared canvas.
  120 messages a second per connection.
