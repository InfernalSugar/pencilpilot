/**
 * Tiles from drawing together inflated and deflated in the paint engine (its WASM), many at a time:
 * much faster than the browser's DecompressionStream a tile at a time. The engine must be
 * loaded (`loadEngine`): a canvas has loaded it.
 */
import { deflate_tiles, inflate_tiles } from 'pencilpilot-brush-wasm';

const TILE_BYTES = 64 * 64 * 4;

/** Each tile's raw deflate → its 64×64 RGBA8 (null: not a tile). */
export function inflateTiles(zs: Uint8Array[]): (Uint8Array | null)[] {
	if (!zs.length) return [];
	const lens = new Uint32Array(zs.map((z) => z.length));
	const blob = new Uint8Array(lens.reduce((n, l) => n + l, 0));
	let at = 0;
	for (const z of zs) {
		blob.set(z, at);
		at += z.length;
	}
	const out = inflate_tiles(blob, lens);
	const ok = out.subarray(zs.length * TILE_BYTES);
	return zs.map((_, i) => (ok[i] ? out.slice(i * TILE_BYTES, (i + 1) * TILE_BYTES) : null));
}

/** Tiles (64×64 RGBA8 each) → each one's raw deflate (the default level), many at once. */
export function deflateTiles(tiles: Uint8Array[]): Uint8Array[] {
	if (!tiles.length) return [];
	const px = new Uint8Array(tiles.length * TILE_BYTES);
	tiles.forEach((t, i) => px.set(t, i * TILE_BYTES));
	const out = deflate_tiles(px);
	const v = new DataView(out.buffer, out.byteOffset, out.byteLength);
	let at = tiles.length * 4;
	return tiles.map((_, i) => {
		const n = v.getUint32(i * 4, true);
		const z = out.slice(at, at + n);
		at += n;
		return z;
	});
}
