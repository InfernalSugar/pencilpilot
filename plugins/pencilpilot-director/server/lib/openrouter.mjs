/**
 * Seedance through OpenRouter's video API: an async job with a first frame (and an
 * optional last frame), polled until done, then downloaded. Costs are estimated before
 * (OpenRouter bills video by its pixels, so they grow with resolution) and the actual
 * cost is read back.
 */
import { readFile, writeFile, mkdir, appendFile } from "node:fs/promises";
import { dirname, extname } from "node:path";

export const openrouterBase = () =>
  (
    process.env.PENCILPILOT_OPENROUTER_URL || "https://openrouter.ai/api/v1"
  ).replace(/\/$/, "");
export const openrouterKey = () => process.env.OPENROUTER_API_KEY || "";

/** US dollars a second at 480p (OpenRouter's listed prices, November 2026; check them). */
export const PRICES = {
  "bytedance/seedance-2.0-fast": 0.04035,
  "bytedance/seedance-2.0": 0.06726,
  "bytedance/seedance-2.5": 0.1028,
};
export const DEFAULT_MODEL = "bytedance/seedance-2.0-fast";
/** how many times 480p's pixels each resolution has */
const PIXELS = {
  "480p": 1,
  "720p": (1280 * 720) / (854 * 480),
  "1080p": (1920 * 1080) / (854 * 480),
};

/** About what a clip costs (dollars), or null for a model without a known price. */
export function estimate({
  model = DEFAULT_MODEL,
  resolution = "480p",
  seconds = 4,
}) {
  const per = PRICES[model];
  const k = PIXELS[resolution];
  if (per === undefined || k === undefined) return null;
  return Math.round(per * k * seconds * 1000) / 1000;
}

const dataUri = async (path) => {
  const type = {
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".webp": "image/webp",
  }[extname(path).toLowerCase()];
  if (!type) throw new Error(`${path}: frames must be PNG, JPEG or WebP`);
  return `data:${type};base64,${(await readFile(path)).toString("base64")}`;
};

/** Make a clip; returns { cost, model, seconds }. `onStatus` hears the job's progress. */
export async function generateVideo(
  {
    first,
    last,
    prompt,
    model = DEFAULT_MODEL,
    seconds = 4,
    resolution = "480p",
    aspect = "9:16",
    seed,
    out,
  },
  { onStatus = () => {}, poll = 5000, timeoutMs = 1_800_000 } = {},
) {
  const key = openrouterKey();
  if (!key)
    throw new Error(
      "No OpenRouter key: set OPENROUTER_API_KEY where Claude Code starts.",
    );
  const frames = [
    {
      type: "image_url",
      image_url: { url: await dataUri(first) },
      frame_type: "first_frame",
    },
  ];
  if (last)
    frames.push({
      type: "image_url",
      image_url: { url: await dataUri(last) },
      frame_type: "last_frame",
    });
  const body = {
    model,
    prompt,
    frame_images: frames,
    duration: Math.round(seconds),
    resolution,
    aspect_ratio: aspect,
    generate_audio: false,
    ...(seed ? { seed } : {}),
  };
  const auth = { authorization: `Bearer ${key}` };
  const sub = await fetch(openrouterBase() + "/videos", {
    method: "POST",
    headers: { ...auth, "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!sub.ok)
    throw new Error(
      `OpenRouter refused the clip: ${sub.status} ${(await sub.text()).slice(0, 800)}`,
    );
  const job = await sub.json();
  const pollUrl = job.polling_url || `${openrouterBase()}/videos/${job.id}`;
  const t0 = Date.now();
  let st;
  for (;;) {
    st = await (await fetch(pollUrl, { headers: auth })).json();
    onStatus(st.status, (Date.now() - t0) / 1000);
    if (st.status === "completed") break;
    if (st.status === "failed")
      throw new Error(
        "OpenRouter: the clip failed: " + JSON.stringify(st).slice(0, 800),
      );
    if (Date.now() - t0 > timeoutMs)
      throw new Error("OpenRouter: timed out (it may still bill the clip)");
    await new Promise((r) => setTimeout(r, poll));
  }
  const src =
    st.unsigned_urls?.[0] ||
    `${openrouterBase()}/videos/${job.id}/content?index=0`;
  const r = await fetch(src, { headers: auth });
  if (!r.ok)
    throw new Error(`OpenRouter: couldn't download the clip: ${r.status}`);
  await mkdir(dirname(out), { recursive: true });
  await writeFile(out, Buffer.from(await r.arrayBuffer()));
  return { model, seconds, cost: st.usage?.cost ?? null };
}

/** Note what was spent, in the film's folder (costs.jsonl), for a running total. */
export async function logCost(folder, entry) {
  await appendFile(
    `${folder}/costs.jsonl`,
    JSON.stringify({ at: new Date().toISOString(), ...entry }) + "\n",
  );
}

export async function spent(folder) {
  try {
    const lines = (await readFile(`${folder}/costs.jsonl`, "utf8"))
      .split("\n")
      .filter(Boolean);
    return lines
      .map((l) => JSON.parse(l))
      .reduce((n, e) => n + (e.cost ?? e.estimate ?? 0), 0);
  } catch {
    return 0;
  }
}
