/**
 * The text tool: a press on the canvas places a text (a new text layer) or opens the
 * text under it; typing shows on the layer as it will be; Done keeps it as one undo
 * step. The text's settings show in `TextOptions.svelte`.
 */
import type { ToolHost } from './tool-options.svelte';
import {
	DEFAULT_TEXT,
	GENERIC_FONTS,
	SYSTEM_FONTS,
	boxCorners,
	fontInstalled,
	fontReady,
	importFontFile,
	importedFonts,
	inBox,
	measureText,
	parseText,
	type TextSpec
} from './text';

/** How new texts look (the last text's look). */
export type TextStyle = Omit<TextSpec, 'text' | 'x' | 'y' | 'width' | 'rotate'>;

const styleOf = (s: TextSpec): TextStyle => ({
	font: s.font,
	size: s.size,
	color: s.color,
	bold: s.bold,
	italic: s.italic,
	align: s.align,
	lineHeight: s.lineHeight,
	letterSpacing: s.letterSpacing
});

export class TextTool {
	/** the text being typed (null: none) */
	spec = $state<TextSpec | null>(null);
	/** its layer */
	layer = $state(0);
	/** its box's corners on the canvas, framed there */
	frame = $state<[number, number][] | null>(null);
	/** how the next new text looks */
	style = $state<TextStyle>(styleOf(DEFAULT_TEXT));
	/** the fonts to pick from (generic, installed system fonts, imported) */
	fonts = $state<string[]>([...GENERIC_FONTS]);
	note = $state('');
	/** a text layer something was about to paint on: ask to rasterize it (its id) */
	rasterizeAsk = $state(0);
	/** the layer was made for this text (an empty one goes again) */
	#made = false;
	/** the last text layer typed on (selecting it again doesn't reopen it) */
	last = 0;
	#box = { w: 0, h: 0 };
	#drag: { x: number; y: number; from: TextSpec } | null = null;

	constructor(private host: ToolHost) {}

	/** the current colour (a new pick recolours the text being typed) */
	get colour() {
		return this.host.color();
	}

	/** the fonts to offer, looked up again (an import, a project opened) */
	listFonts() {
		const extra = importedFonts().map((f) => f.family);
		const used = this.spec && !GENERIC_FONTS.includes(this.spec.font) ? [this.spec.font] : [];
		const all = [...GENERIC_FONTS, ...SYSTEM_FONTS.filter(fontInstalled), ...extra, ...used];
		this.fonts = all.filter((f, i) => all.indexOf(f) === i);
	}

	/** for `PaintCanvas`: a press, drag or let-go with the text tool (canvas px) */
	ontext = (x: number, y: number, phase: 'down' | 'move' | 'up') => {
		if (phase === 'move') return this.#dragTo(x, y);
		if (phase === 'up') {
			this.#drag = null;
			return;
		}
		const s = this.spec;
		if (s && inBox(s, this.#box.w, this.#box.h, x, y, 8)) {
			// a press on the text being typed moves it
			this.#drag = { x, y, from: s };
			return;
		}
		if (s) this.done();
		const hit = this.textAt(x, y);
		if (hit) return void this.open(hit);
		const st = this.style;
		void this.#start(0, {
			...DEFAULT_TEXT,
			...st,
			color: this.host.color(),
			x: Math.round(x),
			y: Math.round(y - (st.size * st.lineHeight) / 2)
		});
	};

	#dragTo(x: number, y: number) {
		const d = this.#drag;
		if (!d || !this.spec) return;
		this.set({ x: Math.round(d.from.x + x - d.x), y: Math.round(d.from.y + y - d.y) });
	}

	/** the text layer whose text is at canvas point (x, y), the active one first */
	textAt(x: number, y: number): number {
		const st = this.host.status();
		const layers = [...st.layers].reverse().filter((l) => l.text && l.visible);
		layers.sort((a, b) => Number(b.id === st.active) - Number(a.id === st.active));
		for (const l of layers) {
			const s = parseText(l.text);
			if (!s) continue;
			const box = measureText(s);
			if (inBox(s, box.width, box.height, x, y, 4)) return l.id;
		}
		return 0;
	}

	/** Change the text of a text layer. */
	open(id: number) {
		if (this.spec) this.done();
		const l = this.host.status().layers.find((x) => x.id === id);
		const s = parseText(l?.text);
		if (s) void this.#start(id, s);
	}

	async #start(id: number, s: TextSpec) {
		const c = this.host.canvas();
		if (!c) return;
		const layer = c.textBegin(id);
		if (!layer) {
			this.note = id
				? 'This layer is locked or hidden: unlock or show it to change its text.'
				: 'No room for another layer (32 at most), or finish what you’re doing first.';
			return;
		}
		this.note = '';
		this.layer = layer;
		this.last = layer;
		this.#made = id === 0;
		this.spec = s;
		this.style = styleOf(s);
		if (!this.fonts.includes(s.font)) this.listFonts();
		this.#show();
		// (an imported font may still be loading: shown again once it's there)
		await fontReady(s);
		if (this.spec) this.#show();
	}

	/** Change the text (or, with none, how the next one looks). */
	set(patch: Partial<TextSpec>) {
		if (!this.spec) {
			this.style = { ...this.style, ...patch };
			return;
		}
		this.spec = { ...this.spec, ...patch };
		this.style = styleOf(this.spec);
		this.#show();
		if (patch.font) void fontReady(this.spec).then(() => this.spec && this.#show());
	}

	#show() {
		const c = this.host.canvas();
		if (!this.spec || !c) return;
		const box = c.textPreview(this.spec);
		if (!box) return this.#reset();
		this.#box = { w: box.width, h: box.height };
		this.frame = boxCorners(this.spec, box.width, Math.max(box.height, 1), 4);
	}

	/** Keep the text (one undo step); a new one left empty goes. */
	done() {
		const c = this.host.canvas();
		if (!this.spec || !c) return this.#reset();
		if (c.textLayer() !== this.layer) return this.#reset();
		if (this.#made && !this.spec.text.trim()) c.textCancel();
		else c.textCommit(this.#made ? 'Text' : 'Change text');
		this.#reset();
	}

	cancel() {
		const c = this.host.canvas();
		if (this.spec && c?.textLayer() === this.layer) c.textCancel();
		this.#reset();
	}

	/** typing stopped elsewhere (undo while typing drops it) */
	check() {
		if (this.spec && this.host.canvas()?.textLayer() !== this.layer) this.#reset();
	}

	#reset() {
		this.spec = null;
		this.frame = null;
		this.layer = 0;
		this.#drag = null;
	}

	/** Bring in a font file; texts can use it at once (the one typed switches to it). */
	async importFont(f: File) {
		const family = await importFontFile(f);
		if (!family) {
			this.note = 'That file couldn’t be read as a font (TTF, OTF, WOFF or WOFF2, 32 MB at most).';
			return;
		}
		this.note = '';
		this.listFonts();
		this.set({ font: family });
	}

	/** for `PaintCanvas`: something was about to paint on a text layer */
	onrasterize = (id: number) => {
		this.rasterizeAsk = id;
	};

	/** The answer to "rasterize?": yes turns the layer into a plain one. */
	rasterize(yes: boolean) {
		const id = this.rasterizeAsk;
		this.rasterizeAsk = 0;
		if (yes && id) this.host.canvas()?.rasterizeLayer(id);
	}
}
