/**
 * The director's pieces against stand-ins: a fake ComfyUI and a fake OpenRouter (local
 * HTTP servers), real ffmpeg and the real ppstudio tool. No key or GPU needed.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { execFile } from "node:child_process";
import { mkdtemp, readFile, writeFile, readdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";

import { imagePrompt, videoPrompt, STYLE, SKETCH } from "../lib/prompts.mjs";
import { estimate } from "../lib/openrouter.mjs";
import { qwenWorkflow } from "../lib/comfy.mjs";

const run = promisify(execFile);

/** a PNG of one colour, made by ffmpeg */
async function png(path, w, h, colour) {
  await run("ffmpeg", [
    "-hide_banner",
    "-loglevel",
    "error",
    "-y",
    "-f",
    "lavfi",
    "-i",
    `color=c=${colour}:s=${w}x${h}`,
    "-frames:v",
    "1",
    path,
  ]);
  return readFile(path);
}

const listen = (handler) =>
  new Promise((res) => {
    const s = createServer(handler);
    s.listen(0, "127.0.0.1", () =>
      res({ s, url: `http://127.0.0.1:${s.address().port}` }),
    );
  });

const body = (req) =>
  new Promise((res) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => res(Buffer.concat(chunks)));
  });

test("prompts follow the house style", () => {
  const k = imagePrompt({
    prompt: "a girl at a window",
    light: "window, left",
  });
  assert.match(k, /a girl at a window/);
  assert.ok(k.includes(STYLE));
  assert.match(k, /ONE key light: window, left/);
  const b = imagePrompt({ prompt: "wide shot", look: "storyboard" });
  assert.ok(b.includes(SKETCH) && !b.includes(STYLE));
  assert.match(
    imagePrompt({ prompt: "make the eye red", look: "fix" }),
    /^ATTACHED IMAGE 1/,
  );
  assert.equal(imagePrompt({ prompt: "as is", look: "raw" }), "as is");
  const v = videoPrompt({ moves: "her hair lifts slightly" });
  assert.match(v, /except: her hair lifts slightly\. Almost a still/);
  assert.match(v, /on twos/);
  assert.match(
    videoPrompt({ moves: "she looks up", end: true }),
    /One smooth, weighty movement[\s\S]*ends exactly on the last frame/,
  );
});

test("costs grow with seconds and resolution", () => {
  assert.equal(estimate({ seconds: 4 }), 0.161);
  assert.equal(
    estimate({ model: "bytedance/seedance-2.5", seconds: 4 }),
    0.411,
  );
  assert.ok(
    estimate({ resolution: "720p", seconds: 4 }) > 2 * estimate({ seconds: 4 }),
  );
  assert.equal(estimate({ model: "someone/else" }), null);
});

test("the Qwen workflow: text only, or edit with references (the first cropped to size)", () => {
  const t = qwenWorkflow({ prompt: "p", width: 1088, height: 1920, seed: 7 });
  assert.equal(t[5].class_type, "EmptyLatentImage");
  assert.deepEqual(t[6].inputs.latent_image, ["5", 0]);
  const e = qwenWorkflow({
    prompt: "p",
    width: 1088,
    height: 1920,
    seed: 7,
    refs: ["a.png", "b.png"],
  });
  assert.equal(e[101].inputs.image, "a.png");
  assert.deepEqual(e[4].inputs["images.image_1"], ["99", 0]);
  assert.deepEqual(e[4].inputs["images.image_2"], ["102", 0]);
  assert.deepEqual(e[6].inputs.latent_image, ["4", 2]);
});

test("a still from ComfyUI, a clip from OpenRouter, frames on twos, a PencilPilot project", async () => {
  const dir = await mkdtemp(join(tmpdir(), "pp-director-"));
  const red = await png(join(dir, "red-src.png"), 108, 192, "red");
  // a 2-second 24 fps clip: red, then blue
  const clip = join(dir, "clip-src.mp4");
  await run("ffmpeg", [
    "-hide_banner",
    "-loglevel",
    "error",
    "-y",
    "-f",
    "lavfi",
    "-i",
    "color=c=red:s=108x192:r=24:d=1",
    "-f",
    "lavfi",
    "-i",
    "color=c=blue:s=108x192:r=24:d=1",
    "-filter_complex",
    "[0][1]concat=n=2:v=1",
    "-pix_fmt",
    "yuv420p",
    clip,
  ]);
  const clipBytes = await readFile(clip);

  const seen = { prompt: null, uploads: 0, video: null };
  const comfy = await listen(async (req, res) => {
    const u = new URL(req.url, "http://x");
    if (u.pathname === "/system_stats") return res.end("{}");
    if (u.pathname === "/upload/image") {
      await body(req);
      seen.uploads++;
      return res.end(JSON.stringify({ name: "up.png" }));
    }
    if (u.pathname === "/prompt") {
      seen.prompt = JSON.parse(await body(req)).prompt;
      return res.end(JSON.stringify({ prompt_id: "p1" }));
    }
    if (u.pathname === "/history/p1")
      return res.end(
        JSON.stringify({
          p1: {
            status: { status_str: "success" },
            outputs: {
              8: {
                images: [
                  {
                    filename: "k.png",
                    subfolder: "pencilpilot",
                    type: "output",
                  },
                ],
              },
            },
          },
        }),
      );
    if (u.pathname === "/view") return res.end(red);
    res.statusCode = 404;
    res.end();
  });
  const router = await listen(async (req, res) => {
    const u = new URL(req.url, "http://x");
    if (req.headers.authorization !== "Bearer test-key") {
      res.statusCode = 401;
      return res.end("{}");
    }
    if (req.method === "POST" && u.pathname === "/videos") {
      seen.video = JSON.parse(await body(req));
      return res.end(JSON.stringify({ id: "v1" }));
    }
    if (u.pathname === "/videos/v1")
      return res.end(
        JSON.stringify({ status: "completed", usage: { cost: 0.17 } }),
      );
    if (u.pathname === "/videos/v1/content") return res.end(clipBytes);
    res.statusCode = 404;
    res.end();
  });
  process.env.PENCILPILOT_COMFY_URL = comfy.url;
  process.env.PENCILPILOT_OPENROUTER_URL = router.url;
  process.env.OPENROUTER_API_KEY = "test-key";
  try {
    const { runToFile, upload } = await import("../lib/comfy.mjs");
    const { generateVideo, logCost, spent } =
      await import("../lib/openrouter.mjs");
    const film = await import("../lib/film.mjs");

    const folder = join(dir, "Ember");
    await film.initFilm(folder, { title: "Ember" });
    // a keyframe, made from a sketch
    const ref = await upload(join(dir, "red-src.png"));
    await runToFile(
      qwenWorkflow({
        prompt: imagePrompt({ prompt: "her" }),
        width: 1088,
        height: 1920,
        seed: 1,
        refs: [ref],
      }),
      join(folder, "keys/shot1.png"),
      { poll: 10 },
    );
    assert.equal(seen.uploads, 1);
    assert.match(seen.prompt[4].inputs.prompt, /STYLE:/);
    assert.deepEqual(await readFile(join(folder, "keys/shot1.png")), red);
    // a clip from it, its cost noted
    const r = await generateVideo(
      {
        first: join(folder, "keys/shot1.png"),
        prompt: "p",
        seconds: 2,
        out: join(folder, "clips/shot2.mp4"),
      },
      { poll: 10 },
    );
    assert.equal(r.cost, 0.17);
    assert.equal(seen.video.frame_images[0].frame_type, "first_frame");
    assert.match(
      seen.video.frame_images[0].image_url.url,
      /^data:image\/png;base64,/,
    );
    assert.equal(seen.video.resolution, "480p");
    await logCost(folder, { cost: r.cost });
    assert.equal(await spent(folder), 0.17);
    // its frames on twos: 12 a second for 2 seconds
    const f = await film.clipFrames(
      join(folder, "clips/shot2.mp4"),
      join(folder, "frames/shot2"),
    );
    assert.equal(f.frames.length, 24);
    assert.equal(f.hold, 2);
    await film.contactSheet(
      join(folder, "clips/shot2.mp4"),
      join(folder, "look/sheet.png"),
    );

    // the plan: a still shot with a flash into a traced shot of the clip's frames
    await writeFile(
      join(folder, "film.json"),
      JSON.stringify({
        title: "Ember",
        width: 108,
        height: 192,
        fps: 24,
        step: 2,
        shots: [
          {
            name: "Shot 1",
            seconds: 1.5,
            transition: "flash",
            keys: ["keys/shot1.png"],
            board: "keys/shot1.png",
            beat: "she waits",
          },
          {
            name: "Shot 2",
            seconds: 1,
            mode: "traced",
            frames: "frames/shot2",
            dialogue: "now",
          },
          { name: "Shot 3", seconds: 0.5 },
        ],
      }),
    );
    const built = await film.buildProject(folder);
    assert.deepEqual(
      built.cuts.map((c) => c.frames),
      [36, 24, 12],
    );
    const info = await film.projectInfo(built.project);
    const [s1, s2] = info.scenes[0].cuts;
    assert.equal(s1.transition, "flash");
    assert.deepEqual(s1.action, { 0: "she waits" });
    assert.deepEqual(s2.layers, [{ layer: "Guide", drawings: 12 }]);
    // and looked at again: shot 2's first drawing is red, its last blue
    const look = await film.renderProject(
      built.project,
      join(folder, "look/frames"),
      { layers: ["Guide"] },
    );
    assert.ok(look.frames.length >= 3);
    const files = await readdir(join(folder, "look/frames"));
    assert.ok(files.includes("0001.png"));
  } finally {
    comfy.s.close();
    router.s.close();
  }
});
