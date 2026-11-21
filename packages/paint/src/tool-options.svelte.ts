/**
 * The tools' own settings (fill tolerance, gradient shape, selection mode, guides…) and
 * the Adjust panel's flow, shared by every full studio (the website's and the app's) so
 * a tool behaves the same in both. `ToolOptionsBar.svelte` shows them; the screen
 * passes them on to `PaintCanvas`.
 */
import type PaintCanvas from './PaintCanvas.svelte';
import type { CanvasStatus, ImportResult } from './PaintCanvas.svelte';
import { defaultGuides, type GuideSettings } from './guides';
import { NO_SPEC, parseSpec, specName, type AdjustSpec } from './tone';

type Canvas = ReturnType<typeof PaintCanvas>;

/** What the options need from the screen that holds them. */
export interface ToolHost {
	canvas(): Canvas | undefined;
	status(): CanvasStatus;
	/** the current colour (Recolour paints with it) */
	color(): string;
	setTool(tool: string): void;
	/** the guides changed (to keep them) */
	onguides?(g: GuideSettings): void;
}

export const NO_ADJUST = { hue: 0, saturation: 0, brightness: 0, contrast: 0, blur: 0 };

const IMPORT_TEXT: Record<ImportResult, string> = {
	ok: '',
	'too-many': 'You’ve reached the layer limit (32).',
	unreadable: 'That file couldn’t be opened as a picture.',
	busy: 'Finish what you’re doing first, then import again.',
	'too-big': 'That picture is too big (30 MB at most).'
};

export class ToolOptions {
	gradientKind = $state<'linear' | 'radial'>('linear');
	gradientEnd = $state<'transparent' | 'colour'>('transparent');
	gradientEndColor = $state('#3d6bff');
	liquifySize = $state(60);
	liquifyStrength = $state(0.6);
	selectMode = $state<'replace' | 'add' | 'subtract' | 'intersect'>('replace');
	/** colour fill: selections fill with the colour as they're made (Procreate's Color Fill) */
	selectFill = $state(false);
	/** the rectangle select tool draws rectangles or ellipses */
	selectShape = $state<'rect' | 'ellipse'>('rect');
	/** magic wand: joined-up area only, or every similar colour (Color Range) */
	wandContiguous = $state(true);
	wandTolerance = $state(0.15);
	smudgeStrength = $state(0.6);
	/** the pen tool's path: points and whether it's closed */
	penInfo = $state({ points: 0, closed: false });
	penTaper = $state(true);
	fillTolerance = $state(0.12);
	fillAllLayers = $state(true);
	/** fill: find shapes on this layer (a line-art "reference" layer); 0 = per fillAllLayers */
	fillRefLayer = $state(0);
	/** transform: corners move on their own (distort / perspective) */
	distort = $state(false);
	/** transform: bend with a 4×4 grid (warp) */
	warp = $state(false);
	/** feathering the selection: how soft its edge gets (canvas px) */
	featherPx = $state(16);
	/** hold at the end of a stroke to snap it to a shape */
	quickShape = $state(true);
	guides = $state<GuideSettings>(defaultGuides(800, 600));
	guidesOpen = $state(false);
	/** a note about the last import or paste */
	importNote = $state('');

	// ---- the Adjust panel: live preview on the active layer, Apply or Cancel ----
	adjustOpen = $state(false);
	adjust = $state({ ...NO_ADJUST });
	recolour = $state(false);
	adjustMsg = $state('');
	/** which part of the Adjust panel shows */
	adjustKind = $state<'colour' | 'curves' | 'levels' | 'map'>('colour');
	/** curves, levels and gradient map (and, for adjustment layers, everything) */
	tone = $state<AdjustSpec>({ ...NO_SPEC });
	/** an adjustment layer being made or changed (0: adjusting the layer's pixels) */
	adjustLayer = $state(0);
	/** that layer is new (Cancel removes it) */
	adjustLayerNew = $state(false);
	/** the layer's settings when the panel opened (Cancel goes back to them) */
	#adjustLayerWas: AdjustSpec | null = null;

	constructor(private host: ToolHost) {}

	/** for `PaintCanvas`: the guides were moved on the canvas */
	onguides = (g: GuideSettings) => this.setGuides(g);
	/** for `PaintCanvas`: the pen tool's path changed */
	onpenpath = (p: { points: number; closed: boolean }) => (this.penInfo = p);
	/** for `PaintCanvas`: a picture was dropped or imported */
	onimport = (r: ImportResult) => this.imported(r);

	/** a fill reference layer that was deleted stops counting */
	get fillReference() {
		const id = this.fillRefLayer;
		return this.host.status().layers.some((l) => l.id === id) ? id : 0;
	}

	/** the gradient's end, as `PaintCanvas` takes it */
	get gradientEndValue() {
		return this.gradientEnd === 'transparent' ? 'transparent' : this.gradientEndColor;
	}

	setGuides(g: GuideSettings) {
		this.guides = g;
		this.host.onguides?.(g);
	}

	/** a picture came in: place it right away (move / resize), as Procreate does */
	imported(r: ImportResult) {
		this.importNote = IMPORT_TEXT[r];
		if (r === 'ok') this.host.setTool('transform');
	}

	/** Ctrl+C / Ctrl+X: copy or cut the selection */
	copyCut(cut: boolean) {
		if (this.host.canvas()?.copySelection(cut)) this.importNote = '';
	}

	/** paste what was copied as a new layer, then move it */
	pasteCopied() {
		if (this.host.canvas()?.paste()) this.host.setTool('transform');
	}

	/** everything in the panel as one adjustment */
	#specNow = (): AdjustSpec => ({
		...this.tone,
		hue: this.adjust.hue,
		saturation: this.adjust.saturation,
		brightness: this.adjust.brightness,
		contrast: this.adjust.contrast
	});

	/** The Adjust button: open the panel for the active layer (or close it, cancelling). */
	openAdjust() {
		if (this.adjustOpen) return this.closeAdjust(false);
		this.adjust = { ...NO_ADJUST };
		this.recolour = false;
		this.tone = { ...NO_SPEC };
		this.adjustMsg = '';
		const status = this.host.status();
		// an adjustment layer: change its settings
		const activeLayer = status.layers.find((l) => l.id === status.active);
		if (activeLayer?.adjust) {
			const s = parseSpec(activeLayer.adjust);
			this.tone = s;
			this.adjust = {
				...NO_ADJUST,
				hue: s.hue,
				saturation: s.saturation,
				brightness: s.brightness,
				contrast: s.contrast
			};
			this.adjustLayer = activeLayer.id;
			this.adjustLayerNew = false;
			this.#adjustLayerWas = s;
			this.adjustKind = s.map ? 'map' : s.curves ? 'curves' : s.levels ? 'levels' : 'colour';
			this.host.setTool('brush');
			this.adjustOpen = true;
			return;
		}
		this.adjustLayer = 0;
		if (!this.host.canvas()?.adjustBegin()) {
			this.adjustMsg = 'This layer is locked or hidden, so it can’t be adjusted.';
			return;
		}
		this.host.setTool('brush');
		this.adjustOpen = true;
	}

	previewAdjust() {
		const canvas = this.host.canvas();
		if (this.adjustLayer) canvas?.adjustmentPreview(this.adjustLayer, this.#specNow());
		else
			canvas?.adjustPreview({
				...this.adjust,
				recolour: this.recolour ? this.host.color() : null,
				curves: this.tone.curves,
				levels: this.tone.levels,
				map: this.tone.map
			});
	}

	/** Switch between changing the pixels and a separate adjustment layer. */
	setKeepEditable(on: boolean) {
		const canvas = this.host.canvas();
		if (!canvas || on === !!this.adjustLayer) return;
		if (on) {
			canvas.adjustCancel();
			const id = canvas.addAdjustmentLayer(this.#specNow(), specName(this.#specNow()));
			if (!id) {
				this.adjustMsg = 'There’s no room for another layer.';
				canvas.adjustBegin();
				return this.previewAdjust();
			}
			this.adjustLayer = id;
			this.adjustLayerNew = true;
			this.#adjustLayerWas = null;
		} else {
			if (this.adjustLayerNew) canvas.undo();
			this.adjustLayer = 0;
			if (!canvas.adjustBegin()) {
				this.adjustMsg = 'This layer is locked or hidden, so it can’t be adjusted.';
				this.adjustOpen = false;
				return;
			}
		}
		this.previewAdjust();
	}

	closeAdjust(keep: boolean) {
		const canvas = this.host.canvas();
		this.adjustOpen = false;
		if (this.adjustLayer) {
			const id = this.adjustLayer;
			this.adjustLayer = 0;
			if (this.adjustLayerNew) {
				// one undo step for the finished layer: made again with its final settings
				canvas?.undo();
				if (keep) canvas?.addAdjustmentLayer(this.#specNow(), specName(this.#specNow()));
			} else if (keep) canvas?.adjustmentCommit();
			else if (this.#adjustLayerWas) {
				canvas?.adjustmentPreview(id, this.#adjustLayerWas);
				canvas?.adjustmentCommit();
			}
			return;
		}
		if (keep) canvas?.adjustCommit();
		else canvas?.adjustCancel();
	}

	/** From the Layers panel: change an adjustment layer (its id), or make a new one (0). */
	adjustmentLayer(id: number) {
		if (this.adjustOpen) this.closeAdjust(false);
		if (id) this.host.canvas()?.selectLayer(id);
		// the Adjust panel reads the active layer from the status: let it catch up
		queueMicrotask(() => {
			this.openAdjust();
			if (!id && this.adjustOpen && !this.adjustLayer) this.setKeepEditable(true);
		});
	}
}
