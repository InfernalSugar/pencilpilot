/**
 * Brushes from other apps into "My brushes": each brush in a file becomes one
 * of My brushes, its pictures kept as its picture tip and grain.
 */
import { importBrushFile, type Decode, type ImportedBrush } from './brush-import';
import { NO_TEXTURE, saveTexture, setBrushTexture } from './brush-textures';
import { NEW_BRUSH_BASE, saveCustomBrush, type CustomBrush } from './custom-brushes';

/** the files the import button offers */
export const BRUSH_FILES = '.myb,.abr,.brush,.brushset,image/png,image/jpeg,image/webp';

/** Keep imported brushes as "My brushes" (with their pictures). Returns them. */
export function keepImported(list: ImportedBrush[]): CustomBrush[] {
	return list.map((b) => {
		const tip = b.tip ? saveTexture({ name: b.name, kind: 'tip', ...b.tip }) : null;
		const grain = b.grain ? saveTexture({ name: b.name, kind: 'grain', ...b.grain }) : null;
		const brush = saveCustomBrush({
			name: b.name,
			base: NEW_BRUSH_BASE,
			// a picture tip stamps its own edges: a hard dab keeps them crisp
			tweaks: b.myb ? {} : { hardness: 1 },
			source: b.myb
		});
		if (tip || grain)
			setBrushTexture(brush.id, {
				...NO_TEXTURE,
				tip: tip?.id ?? null,
				grain: grain?.id ?? null,
				grainStrength: grain ? 0.8 : NO_TEXTURE.grainStrength
			});
		return brush;
	});
}

/** PNG / JPEG bytes → pixels, with the browser's decoder. */
export async function decodeImage(bytes: Uint8Array): Promise<ImageData> {
	const bmp = await createImageBitmap(new Blob([bytes as BlobPart]));
	const c = new OffscreenCanvas(bmp.width, bmp.height);
	const g = c.getContext('2d')!;
	g.drawImage(bmp, 0, 0);
	bmp.close();
	return g.getImageData(0, 0, c.width, c.height);
}

/** Read brush files into My brushes: the brushes added, and a line for each file that failed. */
export async function importBrushFiles(
	files: Iterable<File>,
	decode: Decode = decodeImage
): Promise<{ brushes: CustomBrush[]; errors: string[] }> {
	const brushes: CustomBrush[] = [];
	const errors: string[] = [];
	for (const f of files) {
		try {
			brushes.push(...keepImported(await importBrushFile(f, decode)));
		} catch (e) {
			errors.push(`${f.name}: ${(e as Error).message}`);
		}
	}
	return { brushes, errors };
}

/** One line saying how an import went. */
export function importNote(r: { brushes: CustomBrush[]; errors: string[] }): string {
	const n = r.brushes.length;
	const added = n ? `Added ${n} brush${n === 1 ? '' : 'es'} to My brushes.` : '';
	return [added, ...r.errors].filter(Boolean).join(' ');
}
