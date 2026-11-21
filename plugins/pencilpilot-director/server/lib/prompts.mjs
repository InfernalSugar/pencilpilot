/**
 * Prompts in the house style (from sukkar-studio's RULES.md and server.py, where each
 * rule came from a real shot): flat cel anime, one hard shadow shape, restrained motion,
 * a locked camera, on twos.
 */

export const STYLE =
  "Hand-made 2D TV-anime frame. Flat cel shading: each surface one flat colour plus ONE hard-edged shadow shape; " +
  "no gradients on objects, no airbrushing, no painterly texture, no marble or grime texture, no glow, no bloom, " +
  "no gloss. Hair as a few large flat graphic shapes, never individual strands. Thin, even hand-inked line. " +
  "Desaturated, near-monochrome palette (black, charcoal, cool blue-grey) with crimson accents and only the " +
  "warm light sources named in the shot. Backgrounds as simple flat shapes; keep clutter and debris minimal. " +
  "Correct anatomy: every visible hand has five fingers and connects to a sleeve. Vertical 9:16, full bleed. " +
  "No text, letters or watermark anywhere.";

export const SKETCH =
  "Rough storyboard sketch: loose grey pencil lines on plain white paper, no colour, no shading beyond a few " +
  "quick hatches, simple shapes, clear staging and silhouette, readable at a glance. Vertical 9:16. " +
  "No text, letters, panel borders or watermark.";

export const PERSPECTIVE =
  "PERSPECTIVE (most important): a real camera with a normal 35mm lens, held perfectly level, no tilt, no dutch " +
  "angle. The horizon line is perfectly horizontal. All vertical lines are perfectly vertical and parallel to the " +
  "frame edges. All receding lines converge to ONE vanishing point on the horizon. Every object and person is scaled " +
  "consistently with its distance. No fisheye, no curved lines.";

export const LIGHT =
  "LIGHT LOGIC: there is ONE key light{where}. Surfaces facing it get a thin hard rim of that light; surfaces " +
  "facing away fall into flat cool shadow with one hard shadow shape. Objects and people cast shadows away from " +
  "it, consistently. Do not front-light everything evenly; nothing glows unless it is a light source.";

export const FIX =
  "ATTACHED IMAGE 1 is the CURRENT frame. Keep EVERYTHING in it exactly as it is (composition, camera, perspective, " +
  "crop, character, pose, lighting, objects and their positions) and change ONLY what is described below. " +
  "Further attached images are references for the change.";

export const VIDEO_STYLE =
  "2D hand-drawn anime, Japanese TV-anime cut, flat cel shading exactly like the first frame. " +
  "No 3D, no glow, no bloom, no particles.";

export const VIDEO_STRENGTH = {
  subtle:
    "Almost a still image: the motion is minimal, slow and small, a few centimetres at most. Hair and cloth " +
    "keep their shape; they do not stream or flap.",
  gentle:
    "Slow, gentle, restrained motion. Hair and cloth move only a little, never streaming or flapping.",
  acting:
    "One smooth, weighty movement, eased in and out like a key animator’s, ending exactly as in the last frame. " +
    "Hair moves only as a consequence of the body moving, then settles; no wind.",
};

export const VIDEO_TAIL =
  "Animated on twos like limited TV animation. No morphing, no camera move, no flash, no text, no new objects.";

/**
 * A still's prompt. `look`: "storyboard" (a rough pencil sketch), "keyframe" (the house
 * style), "fix" (change only what's described in reference 1) or "raw" (as written).
 */
export function imagePrompt({
  prompt,
  look = "keyframe",
  perspective = false,
  light,
  character,
}) {
  const parts = [];
  if (look === "fix") parts.push(FIX);
  parts.push(String(prompt ?? "").trim());
  if (look !== "raw" && look !== "fix" && character)
    parts.push(String(character).trim());
  if (perspective) parts.push(PERSPECTIVE);
  if (light !== undefined && light !== null && light !== false)
    parts.push(
      LIGHT.replace("{where}", light ? `: ${String(light).trim()}` : ""),
    );
  if (look === "keyframe" || look === "fix") parts.push("STYLE: " + STYLE);
  if (look === "storyboard") parts.push(SKETCH);
  return parts.filter(Boolean).join("\n\n");
}

/**
 * A motion prompt: the ONE thing that moves; everything else pinned, the camera locked.
 * `strength`: subtle (default with one frame) or acting (default with an end frame).
 */
export function videoPrompt({
  scene = "",
  moves = "",
  strength,
  end = false,
  raw,
}) {
  if (raw) return String(raw);
  const s =
    VIDEO_STRENGTH[strength ?? (end ? "acting" : "subtle")] ??
    VIDEO_STRENGTH.subtle;
  const body = [VIDEO_STYLE];
  if (scene.trim()) body.push(scene.trim());
  body.push(
    "Static camera, locked off, same framing for the whole shot. Everything in the frame stays exactly as " +
      `drawn and does not move, except: ${moves.trim() || "a very small natural movement"}. ${s}`,
  );
  if (end) body.push("It ends exactly on the last frame.");
  body.push(VIDEO_TAIL);
  return body.join("\n\n");
}
