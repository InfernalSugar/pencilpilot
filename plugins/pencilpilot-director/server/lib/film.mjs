/**
 * The film plan (`film.json` in the film's folder) and what's made from it: a PencilPilot
 * project, frames from a clip, contact sheets. Paths in the plan are relative to its
 * folder.
 *
 * {
 *   "title": "Ember", "width": 1080, "height": 1920, "fps": 24, "step": 2,
 *   "music": [{ "file": "audio/score.mp3", "start": 0 }],
 *   "shots": [{
 *     "name": "Shot 1", "seconds": 1.7, "transition": "flash",
 *     "beat": "she turns to the window", "dialogue": "…",
 *     "mode": "final" | "traced", "stage": "board",
 *     "board": "boards/shot1.png", "keys": ["keys/shot1-a.png"], "frames": "frames/shot1"
 *   }]
 * }
 */
import { execFile } from "node:child_process";
import { readFile, readdir, writeFile, mkdir, access } from "node:fs/promises";
import { dirname, join, resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const run = promisify(execFile);
const here = dirname(fileURLToPath(import.meta.url));
/** the PencilPilot repository this plugin lives in */
export const repo = resolve(here, "../../../..");

export const STAGES = [
  "script",
  "board",
  "animatic",
  "key",
  "motion",
  "clean",
  "colour",
  "done",
];
export const LAYERS = [
  "Board",
  "Picture",
  "Guide",
  "Colour",
  "Shadow",
  "Lines",
];

export async function readFilm(folder) {
  const film = JSON.parse(await readFile(join(folder, "film.json"), "utf8"));
  return {
    width: 1080,
    height: 1920,
    fps: 24,
    step: 2,
    music: [],
    shots: [],
    ...film,
  };
}

const pictureFiles = async (dir) =>
  (await readdir(dir))
    .filter((f) => /\.(png|jpe?g|webp)$/i.test(f))
    .sort((a, b) => a.localeCompare(b, undefined, { numeric: true }))
    .map((f) => join(dir, f));

/**
 * The project spec (for `ppstudio build`) a film plan makes. Each shot is a cut, its
 * length its seconds. Its frames (a folder of pictures, each held `step` frames) go on
 * "Picture" (AI-final shots) or "Guide" (shots to trace, half see-through), else its
 * keys do (held); its board is underneath on "Board". Colour, Shadow (clipped) and Lines
 * are ready to draw on.
 */
export async function filmSpec(film, folder) {
  const at = (p) => resolve(folder, p);
  const cuts = [];
  for (const s of film.shots) {
    const len = Math.max(1, Math.round((s.seconds ?? 2) * film.fps));
    const tracks = {};
    const layer = s.mode === "traced" ? "Guide" : "Picture";
    if (s.board) tracks.Board = [{ image: at(s.board), length: len }];
    const frames = s.frames ? await pictureFiles(at(s.frames)) : [];
    if (frames.length) {
      const step = Math.max(1, s.step ?? film.step);
      const t = [];
      let n = 0;
      for (const [i, f] of frames.entries()) {
        if (n >= len) break;
        const l = Math.min(step, len - n);
        t.push({ image: f, length: l, ...(i === 0 ? { kind: "key" } : {}) });
        n += l;
      }
      if (n < len) t.at(-1).length += len - n;
      tracks[layer] = t;
    } else if (s.keys?.length) {
      const keys = s.keys.map(at);
      const each = Math.floor(len / keys.length);
      tracks[layer] = keys.map((k, i) => ({
        image: k,
        length: i === keys.length - 1 ? len - each * (keys.length - 1) : each,
        kind: "key",
      }));
    }
    // (a shot with nothing yet still lasts its seconds)
    if (!Object.keys(tracks).length) tracks.Board = [{ length: len }];
    cuts.push({
      name: s.name,
      ...(s.transition === "flash" || s.transition === "dissolve"
        ? { transition: s.transition }
        : {}),
      ...(s.beat ? { action: { 0: s.beat } } : {}),
      ...(s.dialogue ? { dialogue: { 0: s.dialogue } } : {}),
      tracks,
    });
  }
  return {
    width: film.width,
    height: film.height,
    fps: film.fps,
    layers: [
      { name: "Board", opacity: 1 },
      { name: "Picture" },
      { name: "Guide", opacity: 0.45 },
      { name: "Colour" },
      { name: "Shadow", clip: true, blend: "multiply" },
      { name: "Lines" },
    ],
    scenes: [
      {
        name: film.title || "Scene 1",
        cuts,
        audio: (film.music ?? []).map((m) => ({
          file: at(m.file),
          start: Math.round((m.start ?? 0) * film.fps),
          gain: m.gain ?? 1,
        })),
      },
    ],
  };
}

/** The `ppstudio` command (built from this repository the first time). */
export async function ppstudio() {
  const env = process.env.PENCILPILOT_PPSTUDIO;
  if (env) return env;
  const bin = join(repo, "target/release/ppstudio");
  try {
    await access(bin);
  } catch {
    await run(
      "cargo",
      ["build", "--release", "-p", "pencilpilot-ppstudio-cli"],
      {
        cwd: repo,
        maxBuffer: 1 << 26,
      },
    );
  }
  return bin;
}

async function cli(args) {
  const { stdout } = await run(await ppstudio(), args, {
    maxBuffer: 1 << 26,
  }).catch((e) => {
    throw new Error(String(e.stderr || e.message).trim());
  });
  return JSON.parse(stdout);
}

/** Make the film's PencilPilot project (`<title>.ppstudio` in its folder, or `out`). */
export async function buildProject(folder, out) {
  const film = await readFilm(folder);
  const spec = await filmSpec(film, folder);
  const specPath = join(folder, ".project-spec.json");
  await writeFile(specPath, JSON.stringify(spec, null, 1));
  const target =
    out ??
    join(
      folder,
      `${(film.title || "film").replace(/[\\/:*?"<>|]/g, "-")}.ppstudio`,
    );
  return cli(["build", specPath, target]);
}

export const renderProject = (
  project,
  outDir,
  { layers, every = false } = {},
) =>
  cli([
    "render",
    project,
    outDir,
    ...(layers?.length ? ["--layers", layers.join(",")] : []),
    ...(every ? ["--every"] : []),
  ]);

export const projectInfo = (project) => cli(["info", project]);

const ffmpeg = () => process.env.PENCILPILOT_FFMPEG || "ffmpeg";

/** A clip's frames as pictures, `fps / step` a second (on twos: 12 at 24 fps). */
export async function clipFrames(
  video,
  outDir,
  { fps = 24, step = 2, from = 0, to } = {},
) {
  await mkdir(outDir, { recursive: true });
  const args = [
    "-hide_banner",
    "-loglevel",
    "error",
    "-y",
    "-ss",
    String(from),
  ];
  if (to !== undefined) args.push("-t", String(Math.max(0.05, to - from)));
  args.push("-i", video, "-vf", `fps=${fps / step}`, join(outDir, "%04d.png"));
  await run(ffmpeg(), args);
  const files = await pictureFiles(outDir);
  return {
    frames: files.map((f) => relative(outDir, f)),
    folder: outDir,
    hold: step,
  };
}

/** A contact sheet of a clip (6 pictures a second, 4 across), to check it at a glance. */
export async function contactSheet(
  video,
  out,
  { perSecond = 6, columns = 4, rows = 6 } = {},
) {
  await mkdir(dirname(out), { recursive: true });
  await run(ffmpeg(), [
    "-hide_banner",
    "-loglevel",
    "error",
    "-y",
    "-i",
    video,
    "-vf",
    `fps=${perSecond},scale=240:-2,tile=${columns}x${rows}`,
    "-frames:v",
    "1",
    out,
  ]);
  return { sheet: out };
}

/** A new film's folder: the plan and a folder for each kind of picture. */
export async function initFilm(
  folder,
  { title, width = 1080, height = 1920, fps = 24 },
) {
  for (const d of ["boards", "keys", "clips", "frames", "audio", "look"])
    await mkdir(join(folder, d), { recursive: true });
  const path = join(folder, "film.json");
  try {
    await access(path);
    return { film: path, existed: true };
  } catch {
    await writeFile(
      path,
      JSON.stringify(
        { title, width, height, fps, step: 2, music: [], shots: [] },
        null,
        2,
      ) + "\n",
    );
    return { film: path, existed: false };
  }
}
