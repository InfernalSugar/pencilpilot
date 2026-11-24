/**
 * "My compositions": composition guides the student made or imported, saved in this
 * browser (as "My brushes" are). A project using one keeps its own copy in its guides.
 */
import {
	ID,
	readCompositionDef,
	readCompositionFile,
	type CompositionDef
} from './composition-elements';

const KEY = 'pp.myCompositions';
let cache: CompositionDef[] | null = null;
const listeners = new Set<() => void>();

export function customCompositions(): CompositionDef[] {
	if (cache) return cache;
	try {
		const v = JSON.parse(localStorage.getItem(KEY) ?? '[]');
		cache = Array.isArray(v)
			? v.map(readCompositionDef).filter((d): d is CompositionDef => !!d && !!d.id)
			: [];
	} catch {
		cache = [];
	}
	return cache!;
}

function store(list: CompositionDef[]) {
	cache = list;
	try {
		localStorage.setItem(KEY, JSON.stringify(list));
	} catch {
		/* storage blocked: kept for this visit */
	}
	for (const l of listeners) l();
}

export const customComposition = (id: string) => customCompositions().find((d) => d.id === id);

let made = 0;
/** a new id (several can be made in the same millisecond when importing a file) */
const newId = () => `mc-${Date.now().toString(36)}${(made++ % 1296).toString(36)}`;

/** Save a new guide, or update one with the same id. Returns it. */
export function saveCustomComposition(
	d: Omit<CompositionDef, 'id'> & { id?: string }
): CompositionDef {
	const ids = new Set(customCompositions().map((x) => x.id));
	let id = d.id && ID.test(d.id) ? d.id : newId();
	if (!d.id) while (ids.has(id)) id = newId();
	const def = readCompositionDef({ ...d, id })!;
	const list = customCompositions();
	const at = list.findIndex((x) => x.id === def.id);
	// an edited guide keeps its place in the list
	store(at < 0 ? [...list, def] : list.map((x, i) => (i === at ? def : x)));
	return def;
}

export function deleteCustomComposition(id: string) {
	store(customCompositions().filter((d) => d.id !== id));
}

/** Get told when "My compositions" change (returns an unsubscribe). */
export function onCustomCompositions(f: () => void) {
	listeners.add(f);
	return () => listeners.delete(f);
}

/** the biggest file read as a composition guide */
const MAX_FILE = 1 << 20;

/** Bring guides from .ppcomp.json files into My compositions: the ones saved, and a note. */
export async function importCompositionFiles(
	files: File[]
): Promise<{ saved: CompositionDef[]; note: string }> {
	const saved: CompositionDef[] = [];
	let unread = 0;
	for (const f of files) {
		const defs = f.size <= MAX_FILE ? readCompositionFile(await f.text()) : [];
		if (!defs.length) unread++;
		for (const d of defs) saved.push(saveCustomComposition({ ...d, id: undefined }));
	}
	const n = saved.length;
	const added = n ? `${n} composition${n === 1 ? '' : 's'} added to My compositions.` : '';
	const skipped = unread
		? `${unread === 1 ? 'One file' : `${unread} files`} couldn’t be read as a composition.`
		: '';
	return { saved, note: [added, skipped].filter(Boolean).join(' ') };
}
