---
title: Drawing ellipses
skill: line_confidence
minutes: 3
summary: A circle seen at an angle is an ellipse; find its axes and draw it with an even, confident loop.
learn:
  - How a circle turns into a flatter or rounder ellipse
  - Using the major and minor axis to keep it straight
  - Fitting an ellipse inside a square in perspective
drill: ellipse-in-boxes
next: negative-space
---

## Circles seen at an angle

The top of a cup, a wheel, a clock on a wall: almost every circle you see in real life is tilted away from you, so it shows up as an **ellipse**. How round the ellipse looks depends on how much of the circle faces you. A circle right at your eye level is a thin sliver. The further it sits below (or above) your eye level, the rounder it gets.

You can see this on a stack of plates or on a glass: the rim near your eye is flat, the bottom is fuller.

<svg viewBox="0 0 320 220" role="img" aria-label="On the left, three ellipses stacked below a dashed eye-level line, getting rounder the further they are below it. On the right, one ellipse with its long major axis drawn across it in coral and its short minor axis as a dashed line, crossing at right angles. Below it, a pointed lemon shape marked as a mistake." fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
  <path d="M10 22 H150" class="faint" stroke-dasharray="4 6" />
  <ellipse cx="80" cy="40" rx="50" ry="6" />
  <ellipse cx="80" cy="95" rx="50" ry="16" />
  <ellipse cx="80" cy="160" rx="50" ry="28" />
  <ellipse cx="230" cy="90" rx="70" ry="30" />
  <path d="M160 90 H300" class="accent" />
  <path d="M230 56 V124" class="faint" stroke-dasharray="4 5" />
  <path d="M190 180 Q230 154 270 180 Q230 206 190 180" class="faint" />
  <path d="M282 172 l12 12 M294 172 l-12 12" class="accent" />
  <g class="label" stroke="none" fill="currentColor" font-size="12">
    <text x="12" y="16">eye level</text><text x="236" y="52">minor axis</text><text x="200" y="138">major axis (coral)</text><text x="196" y="214">not a lemon</text>
  </g>
</svg>

## Two axes

Every ellipse has a long **major axis** and a short **minor axis**. They always cross at right angles, right in the middle, and the ellipse is symmetrical on both. If your ellipse looks lopsided, draw the two axes through it and you will usually see which side bulges.

For anything round that spins or sits on an axle (a wheel, a cup, a jar) the minor axis lines up with the object’s centre line. That is the trick for drawing cylinders that do not look bent: draw the centre line first, then put each ellipse across it at right angles.

## Fit it in a square

When you know where a square sits in perspective, the circle inside it is easy to place. The ellipse touches each of the four sides exactly once and never crosses them.

Draw the square’s diagonals: where they cross is the true centre of the circle. A line through that point, across the square, shows where the ellipse touches the two slanted sides. Notice that the true centre sits a little *behind* the middle of the ellipse. The near half of the circle looks bigger than the far half, because it is closer to you.

<svg viewBox="0 76 320 124" role="img" aria-label="A square drawn in perspective as a trapezoid, wider at the front. Its two diagonals cross slightly behind the middle. An ellipse sits inside, touching the middle of each side, with its near half visibly larger than its far half." fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
  <path d="M60 170 H260 L220 90 H100 Z" />
  <path d="M60 170 L220 90 M260 170 L100 90" class="faint" stroke-dasharray="4 6" />
  <path d="M85 120 H235 M160 90 V170" class="faint" />
  <ellipse cx="160" cy="130" rx="78" ry="40" class="accent" />
  <circle cx="160" cy="120" r="3" fill="currentColor" />
  <g class="label" stroke="none" fill="currentColor" font-size="12">
    <text x="40" y="192">● true centre, so the near half is bigger</text>
  </g>
</svg>

## Draw it in one loop

Ellipses come out best when drawn fast, from the shoulder, not the wrist. Ghost the motion in the air above the paper two or three times, then let the pencil touch down and go round two or three times lightly. Pick the best of the loops and darken it.

Watch the ends. A common mistake is a **lemon**: ends that come to a point. The ends of a real ellipse are always smoothly rounded, however thin it is. Another is a flat stretch along the sides. The curve should keep turning all the way round, without a straight bit or a corner anywhere.
