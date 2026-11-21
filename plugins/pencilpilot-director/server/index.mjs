#!/usr/bin/env node
/**
 * The PencilPilot director's tools (an MCP server Claude Code starts from the plugin):
 * stills from local Qwen-Image (ComfyUI), clips from Seedance (OpenRouter), and the film
 * as a PencilPilot project, step by step. See ../skills/animation-director/SKILL.md.
 */
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { execFile } from "node:child_process";
import { readFile, mkdtemp, rm, access } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import { z } from "zod";

import {
  comfyUp,
  comfyUrl,
  qwenWorkflow,
  runToFile,
  upload,
} from "./lib/comfy.mjs";
import {
  buildProject,
  clipFrames,
  contactSheet,
  initFilm,
  ppstudio,
  projectInfo,
  readFilm,
  renderProject,
  STAGES,
} from "./lib/film.mjs";
import {
  DEFAULT_MODEL,
  PRICES,
  estimate,
  generateVideo,
  logCost,
  openrouterKey,
  spent,
} from "./lib/openrouter.mjs";
import { imagePrompt, videoPrompt } from "./lib/prompts.mjs";

const run = promisify(execFile);
const server = new McpServer({
  name: "pencilpilot-director",
  version: "0.1.0",
});

const text = (v) => ({
  content: [
    {
      type: "text",
      text: typeof v === "string" ? v : JSON.stringify(v, null, 1),
    },
  ],
});
const fail = (e) => ({
  isError: true,
  content: [{ type: "text", text: String(e?.message ?? e) }],
});

/** A small preview of a picture (so the chat can show it), via ffmpeg. */
async function preview(path, width = 360) {
  const dir = await mkdtemp(join(tmpdir(), "pp-preview-"));
  try {
    const out = join(dir, "p.jpg");
    await run(process.env.PENCILPILOT_FFMPEG || "ffmpeg", [
      "-hide_banner",
      "-loglevel",
      "error",
      "-y",
      "-i",
      path,
      "-vf",
      `scale=${width}:-2`,
      "-q:v",
      "4",
      out,
    ]);
    return {
      type: "image",
      mimeType: "image/jpeg",
      data: (await readFile(out)).toString("base64"),
    };
  } catch {
    return null;
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

const has = async (cmd, args = ["-version"]) => {
  try {
    await run(cmd, args);
    return true;
  } catch {
    return false;
  }
};

server.registerTool(
  "setup_check",
  {
    description:
      "What is ready: ComfyUI (local Qwen-Image stills), the OpenRouter key (Seedance clips), ffmpeg, and the ppstudio tool that makes PencilPilot projects. Run it first in a session.",
    inputSchema: {},
  },
  async () => {
    const ff = process.env.PENCILPILOT_FFMPEG || "ffmpeg";
    let pp = null;
    try {
      pp = await ppstudio();
    } catch (e) {
      pp = `couldn't build it: ${e.message}`;
    }
    return text({
      comfyui: { url: comfyUrl(), running: await comfyUp() },
      openrouter: { key: !!openrouterKey(), models: PRICES },
      ffmpeg: await has(ff),
      ppstudio: pp,
    });
  },
);

server.registerTool(
  "film_init",
  {
    description:
      "Start a film: a folder with film.json (the plan: shots, their seconds, transitions, stage) and folders for boards, keys, clips, frames and audio. Safe on an existing film (keeps its plan).",
    inputSchema: {
      folder: z.string().describe("the film folder (absolute)"),
      title: z.string(),
      width: z
        .number()
        .int()
        .optional()
        .describe("default 1080 (vertical 9:16)"),
      height: z.number().int().optional().describe("default 1920"),
      fps: z
        .number()
        .int()
        .optional()
        .describe("default 24 (drawings on twos: 12 a second)"),
    },
  },
  async ({ folder, ...opts }) => {
    try {
      return text(await initFilm(resolve(folder), opts));
    } catch (e) {
      return fail(e);
    }
  },
);

server.registerTool(
  "image_generate",
  {
    description:
      "Make a still with local Qwen-Image (ComfyUI, free): a storyboard sketch, a keyframe in the house style, or a fix of an existing frame (references[0] is the frame; change only what the prompt says). Returns the file and a preview to show the person.",
    inputSchema: {
      out: z
        .string()
        .describe(
          "where to save the PNG (absolute, e.g. <film>/boards/shot1.png)",
        ),
      prompt: z
        .string()
        .describe(
          "what is in the picture: subject, pose, framing, setting, light",
        ),
      look: z
        .enum(["storyboard", "keyframe", "fix", "raw"])
        .default("keyframe")
        .describe(
          "storyboard: rough pencil; keyframe: flat cel house style; fix: edit references[0]; raw: prompt as is",
        ),
      references: z
        .array(z.string())
        .max(3)
        .optional()
        .describe(
          "up to 3 pictures to work from (the first sets the shape): a board, the person’s sketch, a character sheet",
        ),
      character: z
        .string()
        .optional()
        .describe(
          "the character’s fixed description, kept the same in every shot",
        ),
      perspective: z
        .boolean()
        .optional()
        .describe("add the strict perspective rules (rooms, buildings)"),
      light: z
        .string()
        .optional()
        .describe("where the one key light is (adds the light rules)"),
      width: z
        .number()
        .int()
        .optional()
        .describe("default 1088 (Qwen likes multiples of 32)"),
      height: z.number().int().optional().describe("default 1920"),
      seed: z.number().int().optional(),
      steps: z.number().int().optional(),
    },
  },
  async (a) => {
    try {
      const refs = [];
      for (const r of a.references ?? []) refs.push(await upload(resolve(r)));
      const seed = a.seed ?? Math.floor(Math.random() * 2 ** 31);
      const prompt = imagePrompt(a);
      const wf = qwenWorkflow({
        prompt,
        width: a.width ?? 1088,
        height: a.height ?? 1920,
        refs,
        seed,
        steps: a.steps ?? 25,
      });
      const out = resolve(a.out);
      const r = await runToFile(wf, out);
      const img = await preview(out);
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify({ file: out, seed, seconds: r.seconds }),
          },
          ...(img ? [img] : []),
        ],
      };
    } catch (e) {
      return fail(e);
    }
  },
);

server.registerTool(
  "video_estimate",
  {
    description:
      "About what a Seedance clip costs before making it (OpenRouter bills by pixels and seconds). Tell the person this and wait for their yes before video_generate.",
    inputSchema: {
      model: z.string().optional().describe(`default ${DEFAULT_MODEL}`),
      resolution: z.enum(["480p", "720p", "1080p"]).optional(),
      seconds: z.number().optional(),
    },
  },
  async (a) => {
    const cost = estimate(a);
    return text({
      model: a.model ?? DEFAULT_MODEL,
      resolution: a.resolution ?? "480p",
      seconds: a.seconds ?? 4,
      estimate_usd: cost,
      note:
        cost === null
          ? "no known price for that model: check openrouter.ai first"
          : "an estimate from listed prices; the real cost comes back with the clip",
    });
  },
);

server.registerTool(
  "video_generate",
  {
    description:
      "Animate between keyframes with Seedance on OpenRouter (costs money). ONLY after the person agreed to the estimate: pass that figure as approved_usd. One thing moves; the camera stays locked; on twos. Returns the clip and a contact sheet to check it (drift, morphing, timing).",
    inputSchema: {
      folder: z
        .string()
        .describe("the film folder (the cost is noted in its costs.jsonl)"),
      out: z
        .string()
        .describe(
          "where to save the MP4 (absolute, e.g. <film>/clips/shot2.mp4)",
        ),
      first_frame: z.string().describe("the start keyframe (PNG/JPEG/WebP)"),
      last_frame: z
        .string()
        .optional()
        .describe("the end keyframe, for acting shots"),
      moves: z
        .string()
        .describe(
          "the ONE thing that moves, and how, as an end state (not a metaphor)",
        ),
      scene: z.string().optional().describe("what the shot is, briefly"),
      strength: z.enum(["subtle", "gentle", "acting"]).optional(),
      raw_prompt: z
        .string()
        .optional()
        .describe("use this prompt as is instead (rarely)"),
      seconds: z.number().int().min(2).max(12).default(4),
      resolution: z.enum(["480p", "720p", "1080p"]).default("480p"),
      model: z.string().default(DEFAULT_MODEL),
      approved_usd: z.number().describe("the estimate the person said yes to"),
    },
  },
  async (a) => {
    try {
      const est = estimate({
        model: a.model,
        resolution: a.resolution,
        seconds: a.seconds,
      });
      if (est !== null && a.approved_usd + 1e-9 < est)
        return fail(
          `This clip is about $${est}, more than the $${a.approved_usd} agreed. Ask the person again.`,
        );
      if (est === null && !(a.approved_usd > 0))
        return fail(
          "No known price for this model: agree a budget with the person first.",
        );
      const prompt = videoPrompt({
        scene: a.scene ?? "",
        moves: a.moves,
        strength: a.strength,
        end: !!a.last_frame,
        raw: a.raw_prompt,
      });
      const out = resolve(a.out);
      const r = await generateVideo({
        first: resolve(a.first_frame),
        last: a.last_frame ? resolve(a.last_frame) : undefined,
        prompt,
        model: a.model,
        seconds: a.seconds,
        resolution: a.resolution,
        out,
      });
      await logCost(resolve(a.folder), {
        clip: out,
        model: a.model,
        resolution: a.resolution,
        seconds: a.seconds,
        estimate: est,
        cost: r.cost,
      });
      const sheet = out.replace(/\.[^.]+$/, "") + "-sheet.png";
      await contactSheet(out, sheet).catch(() => {});
      const img = await preview(sheet, 720);
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify({
              file: out,
              sheet,
              cost_usd: r.cost,
              estimate_usd: est,
              prompt,
            }),
          },
          ...(img ? [img] : []),
        ],
      };
    } catch (e) {
      return fail(e);
    }
  },
);

server.registerTool(
  "clip_frames",
  {
    description:
      'A clip’s frames as pictures, on twos (12 a second at 24 fps), optionally a stretch of it (the best 1.5–2 s). Set the shot’s "frames" in film.json to the folder, then project_build.',
    inputSchema: {
      video: z.string(),
      out_dir: z.string().describe("e.g. <film>/frames/shot2"),
      from: z.number().optional().describe("seconds into the clip"),
      to: z.number().optional(),
      step: z
        .number()
        .int()
        .optional()
        .describe("frames each drawing holds (2: on twos)"),
      fps: z.number().int().optional(),
    },
  },
  async (a) => {
    try {
      return text(await clipFrames(resolve(a.video), resolve(a.out_dir), a));
    } catch (e) {
      return fail(e);
    }
  },
);

server.registerTool(
  "clip_sheet",
  {
    description:
      "A contact sheet of a clip (6 pictures a second) to check it for drift, morphing and timing.",
    inputSchema: { video: z.string(), out: z.string() },
  },
  async (a) => {
    try {
      const r = await contactSheet(resolve(a.video), resolve(a.out));
      const img = await preview(r.sheet, 720);
      return {
        content: [
          { type: "text", text: JSON.stringify(r) },
          ...(img ? [img] : []),
        ],
      };
    } catch (e) {
      return fail(e);
    }
  },
);

server.registerTool(
  "project_build",
  {
    description:
      'Make (or remake) the film’s PencilPilot project from film.json: a cut per shot with its seconds and transition, boards on "Board", AI-final frames on "Picture", frames to trace on "Guide" (half see-through), and Colour / Shadow / Lines layers to draw on. Rebuilding replaces the file: if the person drew in it, look at their work first (project_look) and say so.',
    inputSchema: {
      folder: z.string(),
      out: z.string().optional().describe("default <folder>/<title>.ppstudio"),
    },
  },
  async (a) => {
    try {
      return text(
        await buildProject(
          resolve(a.folder),
          a.out ? resolve(a.out) : undefined,
        ),
      );
    } catch (e) {
      return fail(e);
    }
  },
);

server.registerTool(
  "project_look",
  {
    description:
      'See a PencilPilot project’s frames as PNGs (each drawing change once, or every frame): to look at what the person drew (e.g. layers ["Lines"]) and use it as a reference for the next step.',
    inputSchema: {
      project: z.string(),
      out_dir: z.string(),
      layers: z.array(z.string()).optional(),
      every: z.boolean().optional(),
    },
  },
  async (a) => {
    try {
      return text(
        await renderProject(resolve(a.project), resolve(a.out_dir), a),
      );
    } catch (e) {
      return fail(e);
    }
  },
);

server.registerTool(
  "project_info",
  {
    description:
      "A PencilPilot project’s cuts, their lengths and transitions, layers with drawings, notes.",
    inputSchema: { project: z.string() },
  },
  async (a) => {
    try {
      return text(await projectInfo(resolve(a.project)));
    } catch (e) {
      return fail(e);
    }
  },
);

server.registerTool(
  "film_status",
  {
    description:
      "The film at a glance: each shot’s stage and what it has (board, keys, clip frames), total length, and money spent so far.",
    inputSchema: { folder: z.string() },
  },
  async (a) => {
    try {
      const folder = resolve(a.folder);
      const film = await readFilm(folder);
      const exists = async (p) =>
        p
          ? access(resolve(folder, p)).then(
              () => true,
              () => false,
            )
          : false;
      const shots = [];
      for (const s of film.shots)
        shots.push({
          name: s.name,
          seconds: s.seconds,
          stage: s.stage ?? "script",
          mode: s.mode ?? "final",
          transition: s.transition ?? "cut",
          board: await exists(s.board),
          keys: (s.keys ?? []).length,
          frames: await exists(s.frames),
        });
      return text({
        title: film.title,
        seconds:
          Math.round(
            film.shots.reduce((n, s) => n + (s.seconds ?? 2), 0) * 10,
          ) / 10,
        stages: STAGES,
        shots,
        spent_usd: Math.round((await spent(folder)) * 100) / 100,
      });
    } catch (e) {
      return fail(e);
    }
  },
);

await server.connect(new StdioServerTransport());
