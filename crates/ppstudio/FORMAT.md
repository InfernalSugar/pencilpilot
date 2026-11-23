# The `.ppstudio` project file, version 1

A PencilPilot project is a standard **zip** file, so any zip tool can look inside.
This crate (`pencilpilot-ppstudio`) reads and writes it.

| Entry | Compression | What |
|---|---|---|
| `manifest.json` | deflate | everything except pixels (below). Always the first entry |
| `strokes.json` | deflate | the stroke log: a JSON array the app writes and reads back as is |
| `thumbnail.png` | stored | optional; at most 512 px on its long side, for the project browser |
| `tiles/<hash>` | deflate | one tile of pixels (below) |
| `audio/<hash>.<ext>` | stored | a sound file a scene plays, as imported (below) |
| `fonts/<hash>.<ext>` | stored | a font file text layers use, as imported (below) |
| `lines/<drawing>` | deflate | optional; a line layer drawing's strokes, the app's own data (below) |
| `timelapse/<n>.jpg` | stored | optional; the timelapse, a JPEG after each change (`000000.jpg` first) |

## manifest.json

```jsonc
{
  "format": "ppstudio",          // always this
  "version": 1,                  // a reader refuses versions newer than it knows
  "app": "PencilPilot 0.1.0",    // what wrote the file
  "created": "2026-11-21T09:30:00Z",
  "modified": "2026-11-21T10:05:12Z",
  "canvas": { "width": 1920, "height": 1080 },   // 1–8192 a side
  "layers": [                    // bottom → top
    {
      "id": 1, "name": "Sketch", "visible": true, "opacity": 0.6,
      "blend": "normal",         // normal multiply screen overlay darken lighten add
      "locked": false, "alphaLock": false, "clip": false,
      "imported": false,         // holds an imported picture
      "group": 0,                // its group's id (0 = none); a group's layers are next to each other
      "adjust": null,            // an adjustment layer's settings (the app's JSON string)
      "style": {                 // optional: its layer style (each effect keeps its settings while off)
        "stroke": { "on": true, "size": 4, "colour": "#000000", "position": "outside", "opacity": 1 },
        "shadow": { "on": false, "colour": "#000000", "opacity": 0.5, "angle": 120, "distance": 10, "blur": 8 },
        "glow": { "on": false, "colour": "#ffed87", "size": 12, "opacity": 0.75 } },
      "text": null,              // optional: a text layer's text (the app's JSON string)
      "lines": true              // optional (false when left out): a line (vector) layer
    }
  ],
  "groups": [ { "id": 1, "name": "Character", "visible": true, "opacity": 1.0 } ],
  "activeLayer": 1,
  "timeline": {
    "fps": 24,
    "frames": [                  // what shows on each frame, every cut in turn
      { "cels": { "1": "D1" },   // layer id → drawing id; a layer missing here is empty
        "hold": 2 }              // frames it is held for
    ],
    "scenes": [                  // optional: the animation as it is edited
      { "name": "Scene 1",
        "cuts": [
          { "name": "Cut 1",
            "camera": { "x": 0, "y": 0, "width": 1920, "height": 1080 },  // optional
            "action": { "0": "turns to the window" },   // optional notes, frame → text
            "dialogue": { "4": "hel-", "6": "lo" },      // optional: what's said, by frame
            "transition": "flash",                       // optional: into the next cut ("flash" or "dissolve")
            "tracks": {          // layer id → its exposures, in order
              "1": [ { "drawing": "D1", "length": 2 },     // held for 2 frames ("on twos")
                     { "drawing": null, "length": 1 },     // an empty cel
                     { "drawing": "D2", "length": 2 } ] } } ] }
    ],
    "staticLayers": [ 2 ],       // optional: layers that show one drawing all cut long
    "drawingKinds": { "D1": "key", "D3": "breakdown" },  // optional: key / breakdown / inbetween
    "mouthCharts": [             // optional: lip-sync, which drawing is each mouth shape
      { "layer": 3, "shapes": { "A": "D7", "B": "D8", "X": "D9" } } ]
  },
  "drawings": {
    "D1": {
      "tiles": [ { "x": 3, "y": 7, "hash": "9f2c…" } ],
      "mask":  [ { "x": 0, "y": 0, "hash": "51ab…" } ]   // only when the layer has a mask
    }
  },
  "extra": { },                  // the app's own settings (brush, colour…), kept as given
  "timelapse": [ 1795279000000, 1795279004200 ], // optional: when each timelapse picture was taken (ms since 1970)
  "fonts": [ { "family": "My Hand", "file": "5e0c….ttf" } ]  // optional: font files text layers use
}
```

## Tiles

- A drawing covers the whole canvas, cut into **64×64 tiles** from the top-left corner.
  `x` and `y` count tiles, not pixels. Tiles on the right and bottom edges are padded to
  64×64.
- **Colour tiles** are 16 384 bytes: RGBA, 8 bits a channel, **straight (not
  premultiplied) alpha**, sRGB, row by row. Padding is transparent.
- **Mask tiles** are 4 096 bytes: one grey byte a pixel (255 shows the layer, 0 hides it).
  Padding is 255.
- A tile that is fully transparent (colour) or fully 255 (mask) is **left out**. A
  drawing with no tiles is empty.
- The file name is the first 32 hex digits of the **BLAKE3** hash of the tile's bytes.
  Identical tiles are stored once and shared, within a drawing and across drawings and
  frames.

## Timeline

- With `scenes`, the timeline is scenes → cuts → each layer's **exposures**: a drawing
  (or `null`, an empty cel) held for `length` frames. A cut is as long as its longest
  track. `frames` is then the scenes flattened (every cut in turn; runs of identical
  frames joined into holds), so a simple reader can play the file without knowing about
  cuts.
- A **static layer** (`staticLayers`: a background or an overlay) shows its first
  drawing in a cut on every frame of the cut, and its track doesn't count towards the
  cut's length. Its other exposures are kept (made a cel layer again, it animates).
- A cut's **camera** is the part of the canvas it shows, in canvas pixels (none: the
  whole canvas). Players and exports show what's inside it. Apps may mark safe areas
  (action 90%, title 80% of the camera, centred); they aren't stored.
- A scene's **audio** lists sound clips: `{ "name", "file", "start", "gain", "muted" }`. `file`
  is an entry `audio/<file>` in the archive (the sound file as imported, named by the
  BLAKE3 hash of its bytes and its extension); `start` is the scene frame it starts on (the
  scene's cuts one after another; negative: it began before the scene); `gain` 1 is as
  recorded. A scene's **markers** are `{ "frame", "label" }` on scene frames.
- A cut's **action** and **dialogue** are exposure-sheet notes: frame (from 0, in the
  cut) → text. They don't change what shows.
- A **mouth chart** (`mouthCharts`) names, for a layer, the drawing drawn for each mouth
  shape used in lip-sync: `A`–`F` (closed for M B P; slightly open, teeth together; open;
  wide open; rounded; puckered) and the optional `G` (F V), `H` (L) and `X` (rest), as
  Rhubarb Lip Sync names them. Its drawings are stored even when no frame shows them.
- A cut's **transition** is how it goes into the next cut in exports: none
  is a hard cut; `flash` darkens the cut's last 3 frames (to 88%, 76%, 65%), shows one
  white frame and lands the next cut over 2 (white at 55%, 20%); `dissolve` blends the
  two over half a second with an S-curve, centred on the cut. The film keeps its length.
- **Drawing marks** (`drawingKinds`) say what a drawing is in the animation: `key`,
  `breakdown` or `inbetween`. Apps show them on the timeline and the exposure sheet (keys
  circled, breakdowns boxed) and use them for flipping and a keys-only onion skin.
- The **timelapse** is a small picture of the canvas (at most 540 px on its long side)
  taken after each change, across every session: `timelapse/000000.jpg`, `000001.jpg`…
  in order, with `timelapse` in the manifest saying when each was taken. A reader may
  drop pictures that don't line up with their times. Each stroke in `strokes.json` may
  say the `drawing` its layer showed (an animation frame's), so a frame can be drawn
  again stroke by stroke.
- Without `scenes`, `frames` is the whole timeline (a single picture has one frame).
- The app names drawings `D<number>`; a drawing belongs to one layer. Every drawing a
  frame or exposure names must be in `drawings`.

## Text layers

- A layer with `text` is a **text layer**: its drawings hold the text as pixels (so any
  reader shows it), and `text` is the app's description of it (`{ "text", "font",
  "size", "color", "bold", "italic", "align", "lineHeight", "letterSpacing", "x", "y",
  "width", "rotate" }`, see the app's `text.ts`), to set it again. A reader that doesn't
  know text layers treats it as a plain layer.
- `fonts` are the font files the user imported for text layers: `fonts/<file>` (named
  by the BLAKE3 hash of its bytes and its extension), known to `text` by `family`. Only
  fonts the user imported are kept, never the system's.

## Line layers

- A layer with `"lines": true` is a **line (vector) layer**: the app keeps every stroke
  drawn on it, so lines can be moved, changed or deleted later. Its drawings hold the
  strokes' pixels as usual, so a reader that doesn't know line layers (or ignores
  `lines/`) shows it as a plain layer.
- `lines/<drawing>` holds a drawing's strokes (`<drawing>` is its name in `drawings`,
  `D<number>`): the app's own versioned binary, kept as given. A drawing of a line layer
  without an entry has no strokes kept. Each entry is at most 256 MiB; an entry for a
  drawing that isn't in `drawings` is damage.
- Added within version 1: older apps open these files and see plain layers.

## Rules for readers
- Check `format` and `version` first. A newer version means "update the app".
- Every frame's layers and drawings must exist, and every tile a drawing names must be in
  the zip with the right size. Otherwise the file is damaged.
- Keep `extra` and `strokes.json` as you found them if you don't understand them.

## Saving
Write to `<name>.ppstudio.saving`, flush it to disk, then rename it over the old file. A
crash mid-save leaves the previous version intact.
