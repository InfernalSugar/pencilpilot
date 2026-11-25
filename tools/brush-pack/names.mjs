// Clearer names for the MyPaint pack's brushes, by id (the id never changes, so
// projects and "My brushes" that use a brush keep working). build.mjs and
// pencilpilot.mjs both apply them.

/**
 * Brushes whose names clash with another author's get that author's short name (the
 * default 2B pencil and the most used Pen keep theirs).
 */
const BY = {
	'classic-brush': 'Brush (Classic)',
	'deevad-brush': 'Brush (Revoy)',
	'classic-knife': 'Knife (Classic)',
	'ramon-knife': 'Knife (Ramón)',
	'classic-pen': 'Pen (Classic)',
	'ramon-pen': 'Pen (Ramón)',
	'ramon-2b-pencil': '2B pencil (Ramón)',
	'deevad-airbrush': 'Airbrush (Revoy)',
	'kaerhon-v1-airbrush-a': 'Airbrush (Kaerhon)',
	'deevad-fill': 'Fill (Revoy)',
	'kaerhon-v1-fill-c': 'Fill (Kaerhon)',
	'dieterle-round-1': 'Round (Dieterle)',
	'ramon-round': 'Round (Ramón)',
	'kaerhon-v1-smear-sm': 'Smear (Kaerhon)',
	'ramon-smear': 'Smear (Ramón)'
};

/** Spelling, spacing and run-together names. */
const TIDY = {
	'classic-modelling2': 'Modelling 2',
	'classic-puantilism': 'Pointillism',
	'classic-puantilism2': 'Pointillism 2',
	'deevad-spray2': 'Spray 2',
	'dieterle-8b-pencil-1': '8B pencil',
	'dieterle-flat2-1': 'Flat 2',
	'dieterle-halftone-1': 'Halftone',
	'dieterle-halftonecmy-1': 'Halftone CMY',
	'dieterle-tail-feathers2': 'Tail feathers 2',
	'experimental-particules-3': 'Particles 3',
	'experimental-particules-eraser': 'Particles eraser',
	'kaerhon-v1-sketcher2-sk': 'Sketcher 2',
	'kaerhon-v1-airsmudge-a': 'Air smudge',
	'kaerhon-v1-airsmudgeultimate-sk': 'Air smudge ultimate',
	'kaerhon-v1-classicroundblock-static-c': 'Round block (static)',
	'kaerhon-v1-paint-barrr-sm': 'Paint bar',
	'kaerhon-v1-smudge-ink-0-7-sm': 'Smudge ink',
	'ramon-bigairbrush': 'Big airbrush',
	'ramon-round-bl': 'Round block',
	'ramon-wet-direction': 'Wet direction',
	'tanda-charcoal-blur1': 'Charcoal blur',
	'tanda-pencil-2b': 'Pencil 2B',
	'tanda-pencil-8b': 'Pencil 8B'
};

export const RENAMES = { ...BY, ...TIDY };

/** A brush's shown name: its clearer name when it has one. */
export const nameOf = (id, name) => RENAMES[id] ?? name;

/** Categories of PencilPilot's own brushes that the MyPaint pack has none of (shown first). */
export const OWN_CATEGORIES = [['animation', 'Animation']];
