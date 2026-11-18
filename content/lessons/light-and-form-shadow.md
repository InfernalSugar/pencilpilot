---
title: Light and form shadow
skill: tone
minutes: 3
summary: Split a form into a light side and a shadow side, then find the core shadow and the reflected light.
learn:
  - Treating light and shadow as two separate families
  - Finding the core shadow where the form turns away
  - Keeping reflected light darker than anything in the light
drill: form-shading-spheres
next: cast-shadows
---

## One light, two families

Pick one main light and decide where it comes from. Every surface of the form either faces that light or faces away from it. That splits the form into two families: the **light side** and the **shadow side**.

This split matters more than anything else in shading. Squint at a lit object and you will see it as two big shapes, one light and one dark, long before any of the subtle steps in between. Get those two shapes right and the object already looks round.

<svg viewBox="0 0 320 205" role="img" aria-label="A sphere lit from the top left, shown by a coral arrow. The lit side is clean paper with a small highlight. A dark band, the core shadow, curves across the sphere where it turns away from the light. Between the core shadow and the lower right edge is a slightly lighter strip of reflected light. A dark cast shadow stretches along the ground to the right, darkest where the sphere touches the ground." fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
  <path d="M40 172 H310" class="faint" />
  <path d="M130 170 Q170 166 210 163 Q250 166 246 174 Q230 182 160 178 Q136 176 130 170 Z" fill="currentColor" fill-opacity="0.6" stroke="none" />
  <ellipse cx="136" cy="171" rx="12" ry="3" fill="currentColor" stroke="none" />
  <path d="M179.5 50.5 A70 70 0 0 1 80.5 149.5 A70 28 -45 0 0 179.5 50.5 Z" fill="currentColor" fill-opacity="0.35" stroke="none" />
  <path d="M179.5 50.5 A70 28 -45 0 1 80.5 149.5 A70 44 -45 0 0 179.5 50.5 Z" fill="currentColor" fill-opacity="0.75" stroke="none" />
  <circle cx="130" cy="100" r="70" />
  <circle cx="100" cy="68" r="8" class="faint" stroke-dasharray="2 3" />
  <path d="M18 22 L56 50 M46 50 H56 V40" class="accent" />
  <path d="M92 70 L64 82 M158 124 L216 96 M174 142 L216 136" class="faint" stroke-width="1.5" />
  <g class="label" stroke="none" fill="currentColor" font-size="12">
    <text x="16" y="16">light</text><text x="8" y="90">highlight</text><text x="220" y="100">core shadow</text><text x="220" y="140">reflected light</text><text x="220" y="196">cast shadow</text>
  </g>
</svg>

## The core shadow

Where the surface turns from facing the light to facing away, the shading does not stay flat. Just inside the shadow side runs a band that is darker than the rest: the **core shadow**. It is dark because it gets neither direct light nor much light bounced back from the surroundings.

On a sphere the core shadow is a curved band; on a cylinder it is a straight stripe down the length; on a box it falls right on the edge where one plane turns to the next. Finding it, and placing it with care, is what makes a form turn convincingly.

## Reflected light

Past the core shadow, near the edge of the shadow side, the form often gets a little lighter again. Light bounces off the table or the wall and fills the shadow slightly. That is **reflected light**.

The rule that keeps drawings solid: reflected light belongs to the shadow family, so it stays **darker than anything on the light side**. Beginners often make it too bright, almost as light as the lit side, and the object suddenly looks hollow or glowing. Keep it subtle.

## Keep the families apart

Inside the light side, use only light values: paper white for the highlight, gentle greys where the surface tilts away. Inside the shadow side, use only dark values. If a light-side tone ends up as dark as a shadow-side tone, the split breaks and the form flattens.

Shade the shadow shape first, as one even tone. Then darken the core shadow inside it, leave the reflected light alone, and only then model the light side with the lightest touch.
