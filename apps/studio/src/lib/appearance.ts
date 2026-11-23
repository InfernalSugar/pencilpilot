/**
 * The interface preferences put to work on every screen: the size of everything, the
 * accent colour, the colour around the canvas, the see-through checkerboard, and the
 * tooltips (the notes a button shows when the pointer rests on it).
 */
import type { Settings } from './settings.svelte';

type Look = Pick<
	Settings,
	'uiScale' | 'accent' | 'tooltips' | 'surround' | 'checkerSize' | 'checkerColour'
>;

/** `hex` darkened by `k` (0–1): towards black, keeping its hue. */
export function darken(hex: string, k: number): string {
	const n = parseInt(hex.slice(1), 16);
	const c = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => Math.round(v * (1 - k)));
	return '#' + c.map((v) => v.toString(16).padStart(2, '0')).join('');
}

/** The theme's accent tokens for a chosen accent: as it is, filled buttons, and text. */
export function accentTokens(hex: string): Record<string, string> {
	return {
		'--pp-accent': hex,
		'--pp-accent-strong': darken(hex, 0.09),
		'--pp-accent-ink': darken(hex, 0.17)
	};
}

/** The CSS custom properties the preferences set on the page ('' = the theme's own). */
export function lookVars(s: Look): Record<string, string> {
	return {
		...(s.accent ? accentTokens(s.accent) : {}),
		'--pp-canvas-surround': s.surround,
		'--pp-checker': s.checkerColour,
		'--pp-checker-size': `${s.checkerSize}px`
	};
}

const SET = ['--pp-accent', '--pp-accent-strong', '--pp-accent-ink'];

/** Put the preferences on the page. */
export function applyLook(s: Look, root = document.documentElement) {
	const vars = lookVars(s);
	for (const k of SET) if (!(k in vars)) root.style.removeProperty(k);
	for (const [k, v] of Object.entries(vars))
		if (v) root.style.setProperty(k, v);
		else root.style.removeProperty(k);
	setScale(s.uiScale, root);
	tooltips(s.tooltips);
}

let scaled = 1;
/** Everything bigger or smaller: the app's window zooms (a plain browser: CSS zoom). */
function setScale(k: number, root: HTMLElement) {
	if (k === scaled) return;
	scaled = k;
	if ('isTauri' in window) {
		void import('@tauri-apps/api/webview')
			.then(({ getCurrentWebview }) => getCurrentWebview().setZoom(k))
			.catch(() => (root.style.zoom = String(k)));
	} else root.style.zoom = k === 1 ? '' : String(k);
}

// ---- tooltips ----
// The browser shows an element's `title` on its own; with tooltips off, a title is moved
// aside as the pointer comes over it (a button named only by its title keeps that name).

let hiding = false;

function hideTitles(e: Event) {
	for (let el = e.target as Element | null; el; el = el.parentElement) {
		const t = el.getAttribute?.('title');
		if (t === null || t === undefined) continue;
		el.setAttribute('data-pp-title', t);
		el.removeAttribute('title');
		if (!el.hasAttribute('aria-label') && !el.textContent?.trim()) {
			el.setAttribute('aria-label', t);
			el.setAttribute('data-pp-named', '');
		}
	}
}

/** Tooltips on or off. */
export function tooltips(on: boolean) {
	if (on === !hiding) return;
	hiding = !on;
	if (hiding) document.addEventListener('pointerover', hideTitles, true);
	else {
		document.removeEventListener('pointerover', hideTitles, true);
		for (const el of document.querySelectorAll('[data-pp-title]')) {
			el.setAttribute('title', el.getAttribute('data-pp-title')!);
			el.removeAttribute('data-pp-title');
			if (el.hasAttribute('data-pp-named')) {
				el.removeAttribute('aria-label');
				el.removeAttribute('data-pp-named');
			}
		}
	}
}
