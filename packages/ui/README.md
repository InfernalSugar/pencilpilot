# @pencilpilot/ui

The PencilPilot design system ("Studio Pop, Bento"), shared by the website (`apps/web`)
and the app (`apps/studio`, planned):

- `@pencilpilot/ui/theme.css`: colour, type and shape tokens (`--pp-*`) and the fonts.
  Import it once at the app's root.
- `Button.svelte`, `Logo.svelte`, `Tile.svelte`.

Components ship as Svelte source; each app compiles them. Website-only parts (the
site header, sign-in cards) stay in `apps/web`.
