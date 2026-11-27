import { expect, type Page } from '@playwright/test';

// The window's surroundings for the tests: Tauri's IPC stubbed (the files, the frame store,
// the dialogs and the settings), and a few ways to draw and look at the canvas.
//
// The settings answer with the Classic layout unless a test asks for another (the tests
// written for the classic layout drive its controls; layout.e2e.ts drives the new one).

export interface Stub {
	/** what recovery_list / versions_list / recent_list answer */
	recovery?: unknown[];
	versions?: unknown[];
	recent?: unknown[];
	/** the answer to "lose unsaved changes?" */
	askAnswer?: boolean;
	/** what settings_get answers (default: the Classic layout) */
	settings?: Record<string, unknown>;
}

export async function stubTauri(page: Page, stub: Stub = {}) {
	let kept: Buffer | null = null;
	await page.route('**/__keep', async (r) => {
		kept = r.request().postDataBuffer();
		await r.fulfill({ body: 'ok' });
	});
	await page.route('**/__packet', (r) =>
		kept ? r.fulfill({ body: kept }) : r.fulfill({ status: 404 })
	);
	// the frame store lives here, so it outlasts a page (as the app's Rust side does)
	const frameStore = new Map<string, Buffer>();
	await page.route('**/__frame/*', async (r) => {
		const id = r.request().url().split('/').pop()!;
		if (r.request().method() === 'POST') {
			frameStore.set(id, r.request().postDataBuffer() ?? Buffer.alloc(0));
			return r.fulfill({ body: 'ok' });
		}
		if (r.request().method() === 'DELETE') {
			const keep = new URL(r.request().url()).searchParams.get('keep');
			if (keep !== null) {
				const ids = new Set(keep.split(',').filter(Boolean));
				for (const k of [...frameStore.keys()]) if (!ids.has(k)) frameStore.delete(k);
			} else frameStore.delete(id);
			return r.fulfill({ body: 'ok' });
		}
		const b = frameStore.get(id);
		return b ? r.fulfill({ body: b }) : r.fulfill({ status: 404 });
	});
	await page.addInitScript((stub: Stub) => {
		const w = window as unknown as Record<string, unknown>;
		const calls: {
			cmd: string;
			path?: string;
			bytes?: number;
			head?: number[];
			json?: string;
			repeat?: string;
			/** export frames: how many pixels are dark */
			ink?: number;
		}[] = [];
		w.__calls = calls;
		w.isTauri = true;
		const packet = async () => (await fetch('/__packet')).arrayBuffer();
		// the frame store: drawing id → its tiles as the engine packs them
		const frames = new Map<number, Uint8Array>();
		w.__frames = frames;
		const TILE = 64 * 64 * 4;
		// events the window listens to (plugin:event|listen): tests send them with __emit
		const handlers: ((e: unknown) => void)[] = [];
		const listening: { event: string; id: number }[] = [];
		w.__emit = (event: string, payload: unknown) => {
			for (const l of listening) if (l.event === event) handlers[l.id]?.({ event, payload });
		};
		w.__TAURI_INTERNALS__ = {
			transformCallback: (f: (e: unknown) => void) => {
				handlers.push(f);
				return handlers.length - 1;
			},
			unregisterCallback: () => {},
			metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
			invoke: async (
				cmd: string,
				args: unknown,
				options?: { headers?: Record<string, string> }
			) => {
				const path = decodeURIComponent(options?.headers?.path ?? '');
				const bytes = args instanceof Uint8Array ? args : null;
				calls.push({
					cmd,
					path,
					bytes: bytes?.length,
					head: bytes ? [...bytes.subarray(0, 4)] : undefined,
					json: bytes ? undefined : JSON.stringify(args ?? null),
					repeat: options?.headers?.repeat,
					ink:
						cmd === 'export_frame' && bytes
							? bytes.filter((v, i) => i % 4 === 0 && v < 230).length
							: undefined
				});
				switch (cmd) {
					case 'plugin:dialog|save': {
						const ext = JSON.stringify(args).match(/"extensions":\["(\w+)"/)?.[1] ?? 'ppstudio';
						return `/home/me/Walk cycle.${ext}`;
					}
					case 'plugin:dialog|open':
						return JSON.stringify(args).includes('"mp4"')
							? '/home/me/dance take 2.mp4'
							: '/home/me/Walk cycle.ppstudio';
					case 'video_in_open': {
						// a made-up video: a red frame, then a blue one
						const a = args as { width: number; height: number };
						const colours = [
							[255, 0, 0],
							[0, 0, 255]
						];
						w.__video = colours.map((c) => {
							const f = new Uint8Array(a.width * a.height * 4);
							for (let i = 0; i < f.length; i += 4) f.set([...c, 255], i);
							return f.buffer;
						});
						return { seconds: 0.08, width: 64, height: 36 };
					}
					case 'video_in_frame':
						return (w.__video as ArrayBuffer[]).shift() ?? new ArrayBuffer(0);
					case 'plugin:dialog|message':
						// ask(): the label of the button pressed
						return (stub.askAnswer ?? true) ? 'Lose changes' : 'Keep working';
					case 'project_save':
						await fetch('/__keep', { method: 'POST', body: bytes as BlobPart });
						return path;
					case 'project_open':
					case 'recovery_open':
					case 'version_open':
						return packet();
					case 'settings_get':
						return stub.settings ?? { uiLayout: 'classic' };
					case 'recovery_list':
						return stub.recovery ?? [];
					case 'versions_list':
						return stub.versions ?? [];
					case 'recent_list':
						return stub.recent ?? [];
					case 'export_write':
						return path;
					case 'export_tools':
						return { ffmpeg: '/usr/bin/ffmpeg', video: ['mp4', 'prores', 'webm'] };
					case 'export_end':
						return '/home/me/Walk cycle.gif';
					case 'lipsync_tools':
						return '/opt/rhubarb/rhubarb';
					case 'lipsync_run': {
						// Rhubarb's answer for half a second (a frame at 24 fps is 1/24 s): rest,
						// wide open, half a frame of closed lips, open
						const f = 1 / 24;
						return [
							{ start: 0, end: 2 * f, value: 'X' },
							{ start: 2 * f, end: 4 * f, value: 'D' },
							{ start: 4 * f, end: 4.5 * f, value: 'A' },
							{ start: 4.5 * f, end: 0.5, value: 'C' }
						];
					}
					case 'frames_put': {
						const b = bytes!;
						const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
						let at = 4;
						for (let i = 0, n = v.getUint32(0, true); i < n; i++) {
							const id = v.getUint32(at, true);
							const end = at + 8 + v.getUint32(at + 4, true) * (8 + TILE);
							frames.set(id, b.slice(at + 4, end));
							await fetch(`/__frame/${id}`, { method: 'POST', body: b.slice(at + 4, end) });
							at = end;
						}
						return null;
					}
					case 'frames_get': {
						for (const id of (args as { ids: number[] }).ids)
							if (!frames.has(id)) {
								const r = await fetch(`/__frame/${id}`);
								if (r.ok) frames.set(id, new Uint8Array(await r.arrayBuffer()));
							}
						const ids = (args as { ids: number[] }).ids.filter((id) => frames.has(id));
						const size = ids.reduce((n, id) => n + 4 + frames.get(id)!.length, 4);
						const out = new Uint8Array(size);
						const v = new DataView(out.buffer);
						v.setUint32(0, ids.length, true);
						let at = 4;
						for (const id of ids) {
							v.setUint32(at, id, true);
							out.set(frames.get(id)!, at + 4);
							at += 4 + frames.get(id)!.length;
						}
						return out.buffer;
					}
					case 'plugin:event|listen': {
						const a = args as { event: string; handler: number };
						listening.push({ event: a.event, id: a.handler });
						return listening.length;
					}
					case 'share_host':
					case 'share_rejoin':
						return {
							invite: 'pp1-abc',
							me: { id: 'me1', name: 'Me' },
							members: [
								{ id: 'me1', name: 'Me' },
								{ id: 'sam1', name: 'Sam' }
							],
							host: true
						};
					case 'frames_swap': {
						const ids = (args as { pairs: number[] }).pairs;
						for (let i = 0; i + 1 < ids.length; i += 2) {
							const [a, b] = [ids[i], ids[i + 1]];
							const [x, y] = [frames.get(a), frames.get(b)];
							for (const [id, v] of [
								[a, y],
								[b, x]
							] as const) {
								if (v) frames.set(id, v);
								else frames.delete(id);
								await fetch(
									`/__frame/${id}`,
									v ? { method: 'POST', body: v } : { method: 'DELETE' }
								);
							}
						}
						return null;
					}
					case 'frames_retain': {
						const keep = (args as { keep: number[] }).keep;
						for (const id of [...frames.keys()]) if (!keep.includes(id)) frames.delete(id);
						// (opening a project: the copies kept for the next page go too. A new canvas
						// keeps them: the app's store would read them from the file again on opening)
						if (keep.length)
							await fetch(`/__frame/retain?keep=${keep.join(',')}`, { method: 'DELETE' });
						return null;
					}
					default:
						// recovery writes, events (window close) and anything else
						return null;
				}
			}
		};
	}, stub);
}

export const canvas = (page: Page) => page.getByLabel(/Drawing canvas/);
export type Call = {
	cmd: string;
	path?: string;
	bytes?: number;
	head?: number[];
	json?: string;
	repeat?: string;
	ink?: number;
};
export const calls = (page: Page) =>
	page.evaluate(() => (window as unknown as { __calls: Call[] }).__calls);

/** dark pixels in rows y0–y1 (fractions of the canvas) */
export async function ink(page: Page, y0 = 0, y1 = 1) {
	return canvas(page).evaluate(
		(el: HTMLCanvasElement, [y0, y1]) => {
			const top = Math.round(el.height * y0);
			const h = Math.max(1, Math.round(el.height * (y1 - y0)));
			const d = el.getContext('2d')!.getImageData(0, top, el.width, h).data;
			let n = 0;
			for (let i = 0; i < d.length; i += 4) if (d[i] < 200) n++;
			return n;
		},
		[y0, y1]
	);
}

export async function ready(page: Page, url = '/studio') {
	await page.setViewportSize({ width: 1440, height: 960 });
	await page.goto(url);
	await expect(canvas(page)).toHaveAttribute('data-ready', 'true', { timeout: 15_000 });
}

export async function scribble(page: Page, y: number) {
	const b = (await canvas(page).boundingBox())!;
	await page.mouse.move(b.x + b.width * 0.2, b.y + b.height * y);
	await page.mouse.down();
	await page.mouse.move(b.x + b.width * 0.8, b.y + b.height * (y + 0.05), { steps: 20 });
	await page.mouse.up();
}
