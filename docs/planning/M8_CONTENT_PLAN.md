# M8 content plan: ids shared by lessons, drills and feedback rules

Totals after M8: **30 lessons**, **30 drills**, **75+ rules**. These ids are fixed so the three
streams (lessons, drills, rules/tips) can be written in parallel. Skills: placement, outline,
silhouette, tone, line_confidence, proportions.

## New lessons (18) → `content/lessons/<id>.md`
| id | skill | about | drill |
|---|---|---|---|
| gesture-and-flow | line_confidence | capture the action line and movement before details | gesture-30-second |
| negative-space | silhouette | draw the shapes *between* things to check the subject | negative-space-shapes |
| simplifying-into-shapes | silhouette | see any subject as circles, boxes, cylinders | exaggerate-shapes |
| proportions-of-the-face | proportions | eye line halfway, thirds of the face, ear placement | face-proportion-grid |
| anime-faces | proportions | anime face construction: big eyes, small chin, guide lines | eye-placement |
| drawing-hands-simply | proportions | hands as a mitten + palm box + finger tubes | hand-mitten-shapes |
| animal-construction | silhouette | animals from beans, circles and joints | animal-bean-construction |
| perspective-basics | placement | horizon line, vanishing point, things shrink with distance | cube-in-perspective |
| two-point-perspective | placement | boxes and buildings at an angle | two-point-boxes |
| drawing-ellipses | line_confidence | circles in perspective, minor axis, even curves | ellipse-in-boxes |
| light-and-form-shadow | tone | light side, core shadow, reflected light on a form | form-shading-spheres |
| cast-shadows | tone | shadows cast by objects: direction, shape, softness | cast-shadow-blocks |
| hatching | tone | hatching and cross-hatching to build values | hatching-patches |
| hard-and-soft-edges | tone | where edges are crisp vs blended | soft-edge-blend |
| composition-and-cropping | placement | placing the subject on the page, thumbnails | thumbnail-composition |
| line-weight | line_confidence | thick/thin lines for depth and emphasis | line-weight-pressure |
| cartoon-exaggeration | silhouette | push shapes and proportions on purpose, clearly | exaggerate-shapes |
| atmospheric-perspective | tone | far things lighter, softer, less contrast | atmospheric-layers |

## New drills (17) → `apps/web/src/lib/drills.ts`
| id | kind | idea |
|---|---|---|
| gesture-30-second | reference, 30 s | quick action line of a real figure/animal reference |
| negative-space-shapes | shapes | draw only the gaps around a silhouette |
| face-proportion-grid | figure | oval face with eye line / thirds to copy |
| eye-placement | figure | anime face guide with eyes to place |
| hand-mitten-shapes | shapes | simplified hand (mitten + palm box) to copy |
| animal-bean-construction | shapes | animal built from beans and circles |
| cube-in-perspective | boxes | cube with a given vanishing point |
| two-point-boxes | boxes | box with two vanishing points on a horizon |
| ellipse-in-boxes | ellipses | ellipse fitted inside a perspective square |
| form-shading-spheres | values | sphere with light side, core shadow, cast shadow |
| cast-shadow-blocks | values | simple block + its cast shadow |
| hatching-patches | values | patches of increasing hatching density |
| soft-edge-blend | values | gradient from dark to light without bands |
| thumbnail-composition | values | small 3-value composition to copy |
| line-weight-pressure | lines | one line that goes thick → thin |
| exaggerate-shapes | shapes | a simple shape to redraw with pushed proportions (graded loosely) |
| atmospheric-layers | values | 4 hill layers, near dark → far light |

## Rules
Every new rule links a lesson and drill from the full lists (old + new). Category-specific rules
(anime, cartoon, environment, real-life, animals) may reuse a measurement with category-tuned
thresholds and wording, but at most one rule per (measurement, category, direction) range should
fire.
