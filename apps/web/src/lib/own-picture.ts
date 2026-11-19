/**
 * "Practise with your own picture": a picture from the student's device, used
 * as the reference. It is shrunk and kept in this browser tab only; it is never
 * uploaded, shared or stored on the server.
 */
import { loadOwnPicture, type Reference } from './references';

/** how an own picture is graded (the grader's category weights) */
export const OWN_STYLES = [
	{ id: 'real-life', label: 'Photo', blurb: 'A photo of something real' },
	{ id: 'anime', label: 'Anime / manga', blurb: 'Clean outlines matter most' },
	{ id: 'cartoon', label: 'Cartoon', blurb: 'Bold, simple shapes' },
	{ id: 'environment', label: 'Place or scene', blurb: 'Landscapes, rooms, streets' }
] as const;
export type OwnStyle = (typeof OWN_STYLES)[number]['id'];

const KEY = 'pp.ownPicture';

interface Stored {
	id: string;
	url: string;
	width: number;
	height: number;
	name: string;
	style: OwnStyle;
}

/** a short id from the picture's bytes, so Try again finds the same one */
function idOf(url: string): string {
	let h = 2166136261;
	for (let i = 0; i < url.length; i += 7) h = Math.imul(h ^ url.charCodeAt(i), 16777619);
	return `own-${(h >>> 0).toString(36)}`;
}

/** Shrink the picture (at most 1280 px a side) and keep it for this tab. */
export async function keepOwnPicture(file: File, style: OwnStyle): Promise<Reference> {
	const url = await loadOwnPicture(file);
	const img = new Image();
	img.src = url;
	await img.decode();
	const name = file.name.replace(/\.[a-z0-9]+$/i, '').slice(0, 60) || 'Your picture';
	const stored: Stored = {
		id: idOf(url),
		url,
		width: img.naturalWidth,
		height: img.naturalHeight,
		name,
		style
	};
	try {
		sessionStorage.setItem(KEY, JSON.stringify(stored));
	} catch {
		throw new Error('This browser has no room to keep the picture. Try a smaller one.');
	}
	return asReference(stored);
}

/** The picture kept for this tab, as a reference (null if there is none). */
export function ownReference(): Reference | null {
	try {
		const s = JSON.parse(sessionStorage.getItem(KEY) ?? 'null') as Stored | null;
		return s?.url ? asReference(s) : null;
	} catch {
		return null;
	}
}

function asReference(s: Stored): Reference {
	return {
		id: s.id,
		source: 'own',
		sourceId: 0,
		category: 'own',
		pageUrl: '',
		author: 'You',
		authorUrl: '',
		license: 'Your own picture',
		licenseUrl: '',
		tags: [s.name],
		width: s.width,
		height: s.height,
		title: s.name,
		keypoints: [],
		imageUrl: s.url,
		own: true,
		gradeAs: s.style
	};
}
