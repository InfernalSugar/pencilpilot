---
title: Hard and soft edges
skill: tone
minutes: 3
summary: Decide where tones meet with a crisp edge and where they melt into each other.
learn:
  - The difference between hard, soft and lost edges
  - Where each kind of edge belongs on a form
  - Blending a smooth gradient without stripes
drill: soft-edge-blend
next: atmospheric-perspective
---

## Every edge is a choice

Wherever two tones meet, there is an **edge**. It can be **hard** (a crisp, sudden change), **soft** (a gradual blend over some distance) or **lost** (the two tones are so close that the edge disappears altogether).

Beginners tend to make every edge hard, usually by outlining everything. The result looks like a cut-out. Mixing edge types is what makes a drawing feel like it has air and light in it.

<svg viewBox="0 0 320 210" role="img" aria-label="Top left, a box with three flat tones, one per face, meeting at hard, crisp edges. Top right, a sphere whose tone turns smoothly from light to dark with no line between them, a soft edge. Below, two bars: on the left a white half meets a dark half with a sharp hard edge; on the right the bar fades smoothly from light to dark." fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <defs>
    <radialGradient id="hse-sphere" cx="0.35" cy="0.3" r="0.8">
      <stop offset="0" stop-color="currentColor" stop-opacity="0" />
      <stop offset="0.4" stop-color="currentColor" stop-opacity="0.12" />
      <stop offset="0.72" stop-color="currentColor" stop-opacity="0.7" />
      <stop offset="1" stop-color="currentColor" stop-opacity="0.5" />
    </radialGradient>
    <linearGradient id="hse-ramp" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="currentColor" stop-opacity="0" />
      <stop offset="1" stop-color="currentColor" stop-opacity="0.75" />
    </linearGradient>
  </defs>
  <path d="M40 70 L64 52 H124 L100 70 Z" />
  <rect x="40" y="70" width="60" height="60" fill="currentColor" fill-opacity="0.25" />
  <path d="M100 70 L124 52 V112 L100 130 Z" fill="currentColor" fill-opacity="0.6" />
  <circle cx="220" cy="92" r="44" fill="url(#hse-sphere)" />
  <rect x="30" y="160" width="55" height="24" stroke="none" />
  <rect x="85" y="160" width="55" height="24" fill="currentColor" fill-opacity="0.75" stroke="none" />
  <rect x="30" y="160" width="110" height="24" class="faint" />
  <rect x="180" y="160" width="110" height="24" fill="url(#hse-ramp)" class="faint" />
  <g class="label" stroke="none" fill="currentColor" font-size="12">
    <text x="30" y="150">hard: planes change</text><text x="172" y="150">soft: the form turns</text>
    <text x="58" y="202">hard edge</text><text x="210" y="202">soft edge</text>
  </g>
</svg>

## Where hard edges go

Use hard edges where something really does change suddenly:

- where one flat plane meets another at a corner, like the faces of a box;
- where a cast shadow starts, right next to the object;
- on the silhouette of the thing you want people to look at first.

Hard edges attract attention. That makes them a tool: put your crispest edge near the centre of interest and the eye goes there.

## Where soft edges go

Use soft edges where the change is gradual:

- where a round form turns away from the light, such as the terminator on a sphere, a cheek or an arm;
- along a cast shadow as it gets further from the object;
- on things in the distance, and on anything you want to sit quietly in the background.

Where a dark form sits against an equally dark background, let the edge be **lost** completely. The viewer’s eye fills in the missing bit, and the drawing feels more natural for it.

## Blending without stripes

A soft edge is really a smooth gradient, and the enemy is **banding**: visible steps where one layer of tone stops and the next starts. To avoid it, build the gradient in several light passes rather than one heavy one. Start each pass a little further along and overlap the previous one, so no stroke ends in the same place twice.

Keep your strokes small and in the same direction, and ease off the pressure as you go towards the light end so the tone fades out rather than stopping. A tissue or blending stump can smooth the last bit, but use it sparingly: it also grinds graphite into the paper and can make the area muddy and hard to lift back out.
