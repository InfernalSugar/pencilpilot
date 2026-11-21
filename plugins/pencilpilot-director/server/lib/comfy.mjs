/**
 * ComfyUI (local Qwen-Image): queue a workflow, wait for it, fetch the picture it saved
 * over ComfyUI's own API (so ComfyUI can run on another machine).
 */
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { basename, dirname } from "node:path";
import { randomBytes } from "node:crypto";

export const comfyUrl = () =>
  (process.env.PENCILPILOT_COMFY_URL || "http://127.0.0.1:8188").replace(
    /\/$/,
    "",
  );

/** The Qwen-Image model files (sukkar-studio's; set PENCILPILOT_QWEN_* to others). */
export const models = () => ({
  unet:
    process.env.PENCILPILOT_QWEN_UNET ||
    "qwen_image_2.1_int8_convrot.safetensors",
  clip: process.env.PENCILPILOT_QWEN_CLIP || "qwen3vl_8b_w4a8.safetensors",
  vae:
    process.env.PENCILPILOT_QWEN_VAE || "qwen_image_2.1_vae_bf16.safetensors",
});

export async function comfyUp() {
  try {
    const r = await fetch(comfyUrl() + "/system_stats", {
      signal: AbortSignal.timeout(2500),
    });
    return r.ok;
  } catch {
    return false;
  }
}

/** Send a picture to ComfyUI's input folder; its name there. */
export async function upload(path) {
  const name = `pp_${randomBytes(4).toString("hex")}_${basename(path)}`;
  const form = new FormData();
  form.append("image", new Blob([await readFile(path)]), name);
  form.append("overwrite", "true");
  const r = await fetch(comfyUrl() + "/upload/image", {
    method: "POST",
    body: form,
  });
  if (!r.ok) throw new Error(`ComfyUI upload: ${r.status} ${await r.text()}`);
  return (await r.json()).name;
}

/**
 * The Qwen-Image workflow: text to image, or with references an edit/compose. The first
 * reference sets the shape: it's cropped to the size asked for.
 */
export function qwenWorkflow({
  prompt,
  width,
  height,
  refs = [],
  seed,
  steps = 25,
}) {
  const m = models();
  const wf = {
    1: {
      class_type: "UNETLoader",
      inputs: { unet_name: m.unet, weight_dtype: "default" },
    },
    2: {
      class_type: "CLIPLoader",
      inputs: { clip_name: m.clip, type: "qwen_image" },
    },
    3: { class_type: "VAELoader", inputs: { vae_name: m.vae } },
    4: {
      class_type: "TextEncodeQwenImage21",
      inputs: {
        clip: ["2", 0],
        prompt,
        negative_prompt: "",
        resolution: Math.max(width, height),
      },
    },
    6: {
      class_type: "KSampler",
      inputs: {
        model: ["1", 0],
        positive: ["4", 0],
        negative: ["4", 1],
        seed,
        steps,
        cfg: 1.0,
        sampler_name: "euler",
        scheduler: "simple",
        denoise: 1.0,
      },
    },
    7: {
      class_type: "VAEDecode",
      inputs: { samples: ["6", 0], vae: ["3", 0] },
    },
    8: {
      class_type: "SaveImage",
      inputs: { images: ["7", 0], filename_prefix: "pencilpilot/still" },
    },
  };
  if (refs.length) {
    wf[4].inputs.vae = ["3", 0];
    wf[4].inputs.resolution = Math.round(Math.sqrt(width * height) / 32) * 32;
    refs.slice(0, 3).forEach((name, i) => {
      const id = String(101 + i);
      wf[id] = { class_type: "LoadImage", inputs: { image: name } };
      let src = id;
      if (i === 0) {
        wf[99] = {
          class_type: "ImageScale",
          inputs: {
            image: [id, 0],
            upscale_method: "lanczos",
            width,
            height,
            crop: "center",
          },
        };
        src = "99";
      }
      wf[4].inputs[`images.image_${i + 1}`] = [src, 0];
    });
    wf[6].inputs.latent_image = ["4", 2];
  } else {
    wf[5] = {
      class_type: "EmptyLatentImage",
      inputs: { width, height, batch_size: 1 },
    };
    wf[6].inputs.latent_image = ["5", 0];
  }
  return wf;
}

/** Queue a workflow, wait, save its first picture to `out`. */
export async function runToFile(
  wf,
  out,
  { timeoutMs = 3_600_000, poll = 2000 } = {},
) {
  if (!(await comfyUp()))
    throw new Error(
      `ComfyUI isn't answering at ${comfyUrl()}. Start it (or set PENCILPILOT_COMFY_URL to where it runs).`,
    );
  const q = await fetch(comfyUrl() + "/prompt", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ prompt: wf }),
  });
  if (!q.ok)
    throw new Error(
      `ComfyUI refused the workflow: ${q.status} ${(await q.text()).slice(0, 800)}`,
    );
  const { prompt_id: id } = await q.json();
  const t0 = Date.now();
  for (;;) {
    const h = await (await fetch(`${comfyUrl()}/history/${id}`)).json();
    const job = h[id];
    if (job) {
      if (job.status?.status_str === "error")
        throw new Error(
          "ComfyUI error: " +
            JSON.stringify(job.status.messages ?? []).slice(-1200),
        );
      const images = Object.values(job.outputs ?? {}).flatMap(
        (o) => o.images ?? [],
      );
      if (!images.length) throw new Error("ComfyUI made no picture");
      const im = images[0];
      const url = `${comfyUrl()}/view?${new URLSearchParams({ filename: im.filename, subfolder: im.subfolder ?? "", type: im.type ?? "output" })}`;
      const r = await fetch(url);
      if (!r.ok) throw new Error(`ComfyUI view: ${r.status}`);
      await mkdir(dirname(out), { recursive: true });
      await writeFile(out, Buffer.from(await r.arrayBuffer()));
      return { seconds: (Date.now() - t0) / 1000 };
    }
    if (Date.now() - t0 > timeoutMs) throw new Error("ComfyUI timed out");
    await new Promise((r) => setTimeout(r, poll));
  }
}
