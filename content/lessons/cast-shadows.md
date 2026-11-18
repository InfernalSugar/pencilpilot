---
title: Cast shadows
skill: tone
minutes: 3
summary: A cast shadow points away from the light, takes the shape of the object, and is darkest and sharpest where it touches.
learn:
  - Finding a shadow’s direction from the light
  - Plotting its shape from the object’s corners
  - Why shadows are dark and crisp near the object and softer further away
drill: cast-shadow-blocks
next: hatching
---

## A shadow is a shape

A **cast shadow** is the patch where an object blocks the light from reaching another surface: the ground, a table, a wall. It is different from the form shadow on the object itself. The form shadow is about which way the surface faces; the cast shadow is about what is in the way.

Cast shadows do a lot of work. They tell the viewer where the light is, they anchor the object to the ground, and they describe the surface they land on. Without one, objects often look like they are floating.

<svg viewBox="0 0 320 200" role="img" aria-label="A white block standing on the ground, lit by a yellow sun at the top left. Dashed light rays pass over the block’s top corners and land on the ground to the right, marking the far end of a grey cast shadow. The side of the block facing away from the light is shaded, and the cast shadow is darkest right where it meets the base of the block." fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
  <path d="M10 170 H310" class="faint" />
  <circle cx="35" cy="32" r="12" class="fill-yellow" />
  <path d="M80 170 L140 170 L210 162 L234 144 L174 144 L104 152 Z" fill="currentColor" fill-opacity="0.45" stroke="none" />
  <path d="M140 170 L164 152 L176 152 L150 170 Z" fill="currentColor" fill-opacity="0.9" stroke="none" />
  <path d="M80 110 L104 92 H164 L140 110 Z" class="fill-paper" />
  <path d="M140 110 L164 92 V152 L140 170 Z" class="fill-paper" />
  <path d="M140 110 L164 92 V152 L140 170 Z" fill="currentColor" fill-opacity="0.3" />
  <rect x="80" y="110" width="60" height="60" class="fill-paper" />
  <path d="M50 43 L210 162 M75 26 L234 144" class="faint" stroke-dasharray="3 5" />
  <g class="accent">
    <circle cx="210" cy="162" r="3" /><circle cx="234" cy="144" r="3" />
  </g>
  <g class="label" stroke="none" fill="currentColor" font-size="12">
    <text x="20" y="62">light</text><text x="180" y="66">rays over the corners</text><text x="244" y="160">shadow ends</text><text x="96" y="192">darkest at the contact</text>
  </g>
</svg>

## Direction comes from the light

A shadow always falls on the side of the object **away from the light**. Draw an imaginary line from the light, over the top of the object, down to the ground: the shadow stretches along that line.

The height of the light sets the length. A low light, like an evening sun, throws long shadows; a light high overhead throws short ones that tuck in close to the base. In one drawing, with one light, every shadow should point the same way and follow the same logic, so check them against each other.

## Plot the shape from the corners

For a simple block, you can find the shadow exactly. Take each top corner of the block and send a ray from the light past it down to the ground. Where each ray lands is a corner of the shadow. Join those points back to the base of the block, and you have the shadow’s outline.

For rounder objects, do the same with a few key points (the top of a head, the end of a nose, the rim of a cup) and join them with a smooth line. The shadow of a ball on the ground is an oval, because the ray hits the ground at an angle.

## Dark and sharp near, soft far

Right where the object meets the ground, almost no light gets in. That thin line of contact (sometimes called the **occlusion shadow**) is the darkest dark in the whole area. Put it in firmly; it is what really sits the object down.

As the shadow travels away from the object, its edge gets **softer** and its tone usually a little **lighter**, because light from the surroundings starts to creep in. A sharp-edged, evenly black shadow from end to end looks like a hole cut in the ground.
