/**
 * How the canvas is shown and handled, as the app's preferences set it: the cursor, the
 * wheel and zoom, the brush size when zooming, units for rulers and measuring, the
 * selection's look and the guides' colours. Everything here only changes how things are
 * shown or steered, never the drawing's pixels. The defaults are how the canvas always
 * worked (the website's studio uses them as they are).
 */

export type CursorStyle = 'outline' | 'crosshair' | 'dot';
export type Unit = 'px' | 'cm' | 'in';

export interface ViewPrefs {
	/** the brush cursor: its outline at its real size, a crosshair, or a dot */
	cursor: CursorStyle;
	/** keep the cursor showing while a stroke is drawn */
	cursorWhilePainting: boolean;
	/** a brush too small to outline shows a crosshair (off: a dot) */
	crosshairWhenSmall: boolean;
	/** zoom towards the pointer (off: towards the middle of the view) */
	zoomToPointer: boolean;
	/** the plain wheel moves the view ('scroll', Ctrl zooms) or zooms ('zoom', Ctrl moves) */
	wheel: 'scroll' | 'zoom';
	/** how far each wheel step zooms (1 = normal) */
	zoomSpeed: number;
	/** zoom glides instead of jumping */
	smoothZoom: boolean;
	/** the brush keeps its size on screen when zooming (Procreate's dynamic brush scaling) */
	brushFollowsZoom: boolean;
	/** the largest brush size (a multiple of the brush's own) */
	maxBrushSize: number;
	rulers: boolean;
	units: Unit;
	/** pixels per inch, for centimetres and inches */
	ppi: number;
	/** how strongly the area outside a selection is tinted (0 = not at all) */
	selectionTint: number;
	/** the selection's outline moves (off: it stays still) */
	marchingAnts: boolean;
	gridColour: string;
	/** perspective, symmetry and the ruler guide */
	guideColour: string;
}

export const VIEW_DEFAULTS: ViewPrefs = {
	cursor: 'outline',
	cursorWhilePainting: true,
	crosshairWhenSmall: false,
	zoomToPointer: true,
	wheel: 'scroll',
	zoomSpeed: 1,
	smoothZoom: true,
	brushFollowsZoom: false,
	maxBrushSize: 6.06,
	rulers: false,
	units: 'px',
	ppi: 300,
	selectionTint: 0,
	marchingAnts: true,
	gridColour: '#3d6bff',
	guideColour: '#e8492f'
};

// ---- units ----

/** Canvas pixels in one unit. */
export function unitPx(unit: Unit, ppi: number): number {
	return unit === 'in' ? ppi : unit === 'cm' ? ppi / 2.54 : 1;
}

/** Pixels → the unit. */
export const toUnits = (px: number, unit: Unit, ppi: number) => px / unitPx(unit, ppi);

/** The unit → whole pixels. */
export const fromUnits = (v: number, unit: Unit, ppi: number) => Math.round(v * unitPx(unit, ppi));

/** A length for people: "1920 px", "16.26 cm", "6.4 in". */
export function showLength(px: number, unit: Unit, ppi: number): string {
	if (unit === 'px') return `${Math.round(px)} px`;
	const v = toUnits(px, unit, ppi);
	return `${+v.toFixed(v < 10 ? 2 : 1)} ${unit}`;
}

// ---- rulers ----

export interface Tick {
	/** where, in screen px along the ruler */
	at: number;
	/** a numbered tick (the others are shorter) */
	label?: string;
}

/**
 * The ticks of a ruler `length` screen px long, where canvas pixel 0 is at `origin` and
 * one canvas pixel is `scale` screen px (negative: the view is mirrored). Numbers are in
 * `unit`, at least `gap` screen px apart, on a 1-2-5 step; five or so plain ticks between.
 */
export function rulerTicks(
	origin: number,
	scale: number,
	length: number,
	unit: Unit,
	ppi: number,
	gap = 60
): Tick[] {
	const per = unitPx(unit, ppi) * Math.abs(scale); // screen px per unit
	if (!(per > 0) || !Number.isFinite(per) || length <= 0) return [];
	// the smallest 1-2-5 step whose numbers are `gap` apart
	let step = 10 ** Math.floor(Math.log10(gap / per));
	for (const m of [1, 2, 5, 10]) {
		if (step * m * per >= gap) {
			step *= m;
			break;
		}
	}
	const sub = String(step).startsWith('2') ? 4 : 5;
	const minor = step / sub;
	const sign = scale < 0 ? -1 : 1;
	// the units at both ends of the ruler
	const a = ((0 - origin) / per) * sign;
	const b = ((length - origin) / per) * sign;
	const [lo, hi] = [Math.min(a, b), Math.max(a, b)];
	const out: Tick[] = [];
	const first = Math.ceil(lo / minor);
	const last = Math.floor(hi / minor);
	if (last - first > 2000) return [];
	const decimals = Math.max(0, -Math.floor(Math.log10(step) + 1e-9));
	for (let i = first; i <= last; i++) {
		const v = i * minor;
		const at = origin + v * per * sign;
		const major = i % sub === 0;
		out.push(major ? { at, label: (Math.round(v / step) * step).toFixed(decimals) } : { at });
	}
	return out;
}

// ---- zoom ----

/**
 * The zoom step for a wheel event, as a change of log(zoom): a mouse wheel's notch is a
 * fixed step, a trackpad's few pixels a small one. `speed` scales both.
 */
export function wheelZoomStep(deltaY: number, deltaMode: number, speed: number): number {
	// lines / pages → pixels; a mouse wheel notch is ~100 px, a trackpad pinch a few px
	const px = deltaY * (deltaMode === 1 ? 16 : deltaMode === 2 ? 400 : 1);
	const notch = Math.abs(px) >= 40 && Number.isInteger(px);
	return (notch ? -Math.sign(px) * Math.log(1.12) : -px * 0.008) * speed;
}

/**
 * The pan after zooming by `k` (new zoom / old zoom) so the point `s` (screen px from
 * the view's middle) stays where it is; with no point, towards the middle.
 */
export function panAfterZoom(pan: number, k: number, s?: number): number {
	return s === undefined ? pan * k : s - k * (s - pan);
}

// ---- brush size ----

/**
 * The size the brush paints at: the chosen size, capped at `max`; when the brush follows
 * the zoom, smaller on the canvas as you zoom in (so it looks the same on screen).
 */
export function paintSize(size: number, zoom: number, followsZoom: boolean, max = Infinity) {
	const s = Math.min(size, max);
	return followsZoom && zoom > 0 ? s / zoom : s;
}
