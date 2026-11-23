/**
 * A Photoshop file as a new project: its layers, groups and size (psd-read.ts reads it).
 * The project starts unsaved, named after the file; saving makes a .ppstudio.
 */
import type { ReadPsd } from '@pencilpilot/paint/psd-read';
import { AREA_MAX, SIDE_MAX } from '@pencilpilot/paint/canvas-size';
import type { ProjectDoc } from './project';

/** Why a picture this size can't be a canvas ('' when it can). */
export function tooBig(width: number, height: number): string {
	if (width <= SIDE_MAX && height <= SIDE_MAX && width * height <= AREA_MAX) return '';
	return `it’s ${width} × ${height} px, and canvases go up to ${SIDE_MAX} px a side (8K)`;
}

export function psdToDoc(r: ReadPsd, fps: number): ProjectDoc {
	const layers = r.layers.map((l, i) => ({
		id: i + 1,
		name: l.name,
		opacity: l.opacity,
		blend: l.blend,
		visible: l.visible,
		locked: false,
		alphaLock: false,
		// (the bottom layer has nothing under it to clip to)
		clip: i > 0 && l.clip,
		imported: false,
		group: l.group,
		rgba: l.rgba,
		...(l.mask ? { mask: l.mask } : {})
	}));
	// nothing that could be kept (only adjustment layers, say): an empty layer to draw on
	if (!layers.length)
		layers.push({
			id: 1,
			name: 'Layer 1',
			opacity: 1,
			blend: 'normal',
			visible: true,
			locked: false,
			alphaLock: false,
			clip: false,
			imported: false,
			group: 0,
			rgba: new Uint8Array(r.width * r.height * 4)
		});
	return {
		width: r.width,
		height: r.height,
		fps,
		layers,
		groups: r.groups.map((g, i) => ({ id: i + 1, ...g })),
		activeLayer: layers[layers.length - 1].id,
		strokes: [],
		extra: {},
		scenes: []
	};
}

/** The project's name from the file's ("Hero.psd" → "Hero"). */
export const psdStem = (file: string) => file.replace(/\.(psd|psb)$/i, '') || 'Untitled';
