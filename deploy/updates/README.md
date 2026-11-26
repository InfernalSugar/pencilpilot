# App updates (self-update from the website)

The installed app checks `https://pencilpilot.app/updates/latest.json` 20 seconds after it
starts and every 6 hours (Settings → About → "Tell me when there's a new version"). A new
version is offered in a corner card: **Update**, **Later** or **Skip this version**. Update
downloads it, checks its signature, asks "Save before updating?" if a project has changes,
then installs and restarts. Development builds never check.

## How a release gets there

1. Bump the version: `node apps/studio/scripts/set-version.mjs 0.2.0`, commit, merge.
2. The **App** workflow builds the installers, signed for the updater (`.sig` files) because
   the `TAURI_SIGNING_PRIVATE_KEY` secret is set.
3. **Actions → Release → Run workflow** with that App run's id and the tag `v0.2.0`. It
   publishes the GitHub release, then uploads the same files to the website
   (`apps/web/scripts/publish-update.mjs`) and publishes the release there. "What's new" is
   the commits' titles since the last release, unless you write your own.
4. The apps find it within 6 hours (or at their next start).

A bad release: run Release again for the previous good build and tag. Publishing an older
version makes it the offered one again, so the apps stop being offered the bad one. (An
app that already installed the bad one stays on it until a newer version comes out.)

## On the website's server

`node server.js` serves `/updates/…` in front of the site (installers stream straight to
disk, past SvelteKit's 512 KB body limit). Set:

| Variable         | What                                                              |
| ---------------- | ----------------------------------------------------------------- |
| `UPDATES_TOKEN`  | the upload token, the same as the repo's `UPDATES_TOKEN` secret   |
| `UPDATES_DIR`    | where updates live on disk (default `<repo>/data/updates`)        |
| `UPDATES_ORIGIN` | `https://pencilpilot.app`, for download links (or `ORIGIN`)       |
| `UPDATES_KEEP`   | releases kept by the orphan sweep (default 3)                     |

Behind nginx, let large uploads through and don't buffer them:

```nginx
location /updates/ {
    client_max_body_size 1g;
    proxy_request_buffering off;
    proxy_pass http://127.0.0.1:3000;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

### Storage and the orphan sweep

```
UPDATES_DIR/blobs/<sha256>         each file once, named by its content
UPDATES_DIR/incoming/*.part        uploads still arriving
UPDATES_DIR/releases/<version>.json
UPDATES_DIR/current.json           the version offered
```

The sweep runs after every publish and every hour. It keeps the newest `UPDATES_KEEP`
releases plus the current one, and deletes the other releases and every file none of the
kept ones names. It also deletes uploads that stalled for more than an hour, and files
uploaded but never published after 6 hours.

## Keys (keep them safe)

- `~/.tauri/pencilpilot-updater.key` (+ `.password`): the private key every update is
  signed with. Its public half is in `apps/studio/src-tauri/tauri.conf.json`. **Back it up.**
  Without it, installed apps can't be sent updates any more. If it leaks, someone with
  access to the website could ship an update.
- Repo secrets: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`,
  `UPDATES_TOKEN`. Optional repo variable `UPDATES_URL` (default `https://pencilpilot.app`).
