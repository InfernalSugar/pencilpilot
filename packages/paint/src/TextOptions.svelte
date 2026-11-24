<script lang="ts">
	/**
	 * The text tool's options (in the tool options row): what's typed, its font, size,
	 * colour, style, alignment and spacing, and Done / Cancel. Also asks before painting
	 * on a text layer ("Rasterize?"), whatever the tool.
	 */
	import { untrack } from 'svelte';
	import type { CanvasStatus } from './PaintCanvas.svelte';
	import { FONT_TYPES, SIZE_MAX, SIZE_MIN, type TextAlign } from './text';
	import type { TextTool } from './text-tool.svelte';

	let {
		text,
		tool,
		status
	}: {
		text: TextTool;
		tool: string;
		status: CanvasStatus;
	} = $props();

	let area = $state<HTMLTextAreaElement>();
	let picker = $state<HTMLInputElement>();
	const s = $derived(text.spec ?? { ...text.style, text: '', width: 0, rotate: 0 });

	// fonts installed are looked up once the tool is used
	let listed = false;
	$effect(() => {
		if (tool === 'text' && !listed) {
			listed = true;
			untrack(() => text.listFonts());
		}
	});

	// another tool: the text is kept
	$effect(() => {
		if (tool !== 'text') untrack(() => text.done());
	});

	// the canvas changed: typing may have been stopped (undo); a text layer picked with
	// the text tool opens (picked: from layers there already, not a project opened or a
	// canvas made, whose keys would go on into the text)
	let lastActive = 0;
	let lastIds = new Set<number>();
	$effect(() => {
		void status.version;
		const active = status.active;
		const ids = new Set(status.layers.map((l) => l.id));
		untrack(() => {
			text.check();
			const was = lastActive;
			const known = lastIds;
			lastActive = active;
			lastIds = ids;
			if (tool !== 'text' || text.spec || active === was || active === text.last) return;
			if (!known.has(was) || !known.has(active)) return;
			if (status.layers.find((l) => l.id === active)?.text) text.open(active);
		});
	});

	// a colour picked while typing colours the text
	$effect(() => {
		const c = text.colour;
		untrack(() => {
			if (c && text.spec && c !== text.spec.color) text.set({ color: c });
		});
	});

	// a new text: type straight away (once the press on the canvas is over)
	$effect(() => {
		if (!text.layer || !area) return;
		const t = setTimeout(() => area?.focus());
		return () => clearTimeout(t);
	});

	const ALIGNS: { id: TextAlign; label: string; d: string }[] = [
		{ id: 'left', label: 'Align left', d: 'M4 6h16M4 10h10M4 14h16M4 18h10' },
		{ id: 'center', label: 'Centre', d: 'M4 6h16M7 10h10M4 14h16M7 18h10' },
		{ id: 'right', label: 'Align right', d: 'M4 6h16M10 10h10M4 14h16M10 18h10' }
	];
</script>

{#if text.rasterizeAsk && text.askingLines}
	<div class="fill-opts" role="alertdialog" aria-label="Convert the line layer to pixels?">
		<span class="msg">
			This is a line layer: it takes the brush, the eraser and fills. Convert it to pixels to use
			this tool on it? (Its lines can’t be selected and changed after that.)
		</span>
		<button type="button" class="tool done" onclick={() => text.rasterize(true)}>
			Convert to pixels
		</button>
		<button type="button" class="tool" onclick={() => text.rasterize(false)}>Cancel</button>
	</div>
{:else if text.rasterizeAsk}
	<div class="fill-opts" role="alertdialog" aria-label="Rasterize the text?">
		<span class="msg">
			This is a text layer. Rasterize it to paint on it? (Its text can’t be changed after that.)
		</span>
		<button type="button" class="tool done" onclick={() => text.rasterize(true)}>Rasterize</button>
		<button type="button" class="tool" onclick={() => text.rasterize(false)}>Cancel</button>
	</div>
{/if}
{#if tool === 'text'}
	<div class="fill-opts" role="group" aria-label="Text">
		{#if text.spec}
			<textarea
				bind:this={area}
				class="typed"
				rows="2"
				placeholder="Type here"
				aria-label="Text"
				value={text.spec.text}
				oninput={(e) => text.set({ text: e.currentTarget.value })}
				onkeydown={(e) => {
					if (e.key === 'Escape') text.cancel();
					else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) text.done();
				}}></textarea>
		{:else}
			<span class="msg plain">Click on the canvas to add text, or on a text to change it.</span>
		{/if}
		<label class="pick">
			<span class="opt-label">Font</span>
			<select
				value={s.font}
				aria-label="Font"
				onchange={(e) => text.set({ font: e.currentTarget.value })}
			>
				{#each text.fonts as f (f)}
					<option value={f} style="font-family: {f}">{f}</option>
				{/each}
			</select>
		</label>
		<button
			type="button"
			class="tool"
			title="Use a font file (TTF, OTF, WOFF2): it's kept in the project, so it opens the same elsewhere. Only fonts you may share."
			onclick={() => picker?.click()}
		>
			Import font…
		</button>
		<input
			bind:this={picker}
			class="file"
			type="file"
			accept={FONT_TYPES}
			tabindex="-1"
			aria-hidden="true"
			onchange={(e) => {
				const f = e.currentTarget.files?.[0];
				e.currentTarget.value = '';
				if (f) void text.importFont(f);
			}}
		/>
		<label class="slider">
			<span>Size <b>{Math.round(s.size)}</b></span>
			<input
				type="range"
				min={SIZE_MIN}
				max={Math.min(SIZE_MAX, 400)}
				step="1"
				value={s.size}
				oninput={(e) => text.set({ size: +e.currentTarget.value })}
			/>
		</label>
		<label class="colour" title="Colour (or pick one in the swatches)">
			<input
				type="color"
				value={s.color}
				aria-label="Text colour"
				oninput={(e) => text.set({ color: e.currentTarget.value })}
			/>
		</label>
		<button
			type="button"
			class="tool"
			aria-pressed={s.bold}
			aria-label="Bold"
			title="Bold"
			onclick={() => text.set({ bold: !s.bold })}
		>
			<b>B</b>
		</button>
		<button
			type="button"
			class="tool"
			aria-pressed={s.italic}
			aria-label="Italic"
			title="Italic"
			onclick={() => text.set({ italic: !s.italic })}
		>
			<i>I</i>
		</button>
		<div class="seg" role="radiogroup" aria-label="Alignment">
			{#each ALIGNS as a (a.id)}
				<button
					type="button"
					class="tool"
					role="radio"
					aria-checked={s.align === a.id}
					aria-label={a.label}
					title={a.label}
					onclick={() => text.set({ align: a.id })}
				>
					<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
						<path d={a.d} />
					</svg>
				</button>
			{/each}
		</div>
		<label class="slider">
			<span>Line spacing <b>{s.lineHeight.toFixed(1)}</b></span>
			<input
				type="range"
				min="0.6"
				max="3"
				step="0.1"
				value={s.lineHeight}
				oninput={(e) => text.set({ lineHeight: +e.currentTarget.value })}
			/>
		</label>
		<label class="slider">
			<span>Letter spacing <b>{s.letterSpacing}</b></span>
			<input
				type="range"
				min="-20"
				max="100"
				step="1"
				value={s.letterSpacing}
				oninput={(e) => text.set({ letterSpacing: +e.currentTarget.value })}
			/>
		</label>
		{#if text.spec}
			<label class="slider">
				<span>Wrap at <b>{s.width ? Math.round(s.width) : 'off'}</b></span>
				<input
					type="range"
					min="0"
					max="4000"
					step="10"
					value={s.width}
					title="Lines wrap at this width (0: only where you press Enter)"
					oninput={(e) => text.set({ width: +e.currentTarget.value })}
				/>
			</label>
			<label class="slider">
				<span>Turn <b>{Math.round(s.rotate)}°</b></span>
				<input
					type="range"
					min="-180"
					max="180"
					step="1"
					value={s.rotate}
					oninput={(e) => text.set({ rotate: +e.currentTarget.value })}
				/>
			</label>
			<button
				type="button"
				class="tool done"
				title="Keep the text (Ctrl+Enter)"
				onclick={() => text.done()}
			>
				Done
			</button>
			<button
				type="button"
				class="tool"
				title="Drop the change (Esc)"
				onclick={() => text.cancel()}
			>
				Cancel
			</button>
		{/if}
		{#if text.note}<span class="msg" role="alert">{text.note}</span>{/if}
	</div>
{/if}

<style>
	.tool {
		min-width: 38px;
		height: 38px;
		padding: 0 12px;
		display: grid;
		place-items: center;
		border: 0;
		border-radius: 12px;
		background: #2a2a30;
		color: var(--pp-on-ink);
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.tool[aria-pressed='true'],
	.tool[role='radio'][aria-checked='true'],
	.tool.done {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.tool svg path {
		fill: none;
		stroke: currentColor;
		stroke-width: 2;
		stroke-linecap: round;
	}
	.seg {
		display: flex;
		gap: 2px;
	}
	.slider {
		display: flex;
		flex-direction: column;
		gap: 4px;
		width: 120px;
		font-size: 13px;
		color: var(--pp-on-ink-muted);
	}
	.slider b {
		color: var(--pp-on-ink);
	}
	.slider input {
		accent-color: var(--pp-yellow);
	}
	.pick {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.pick select {
		height: 38px;
		max-width: 200px;
		padding: 0 10px;
		border: 0;
		border-radius: 12px;
		background: #2a2a30;
		color: var(--pp-on-ink);
		font: 600 14px var(--pp-font-body);
	}
	.typed {
		width: 240px;
		min-height: 38px;
		padding: 6px 10px;
		border: 2px solid var(--pp-accent-strong);
		border-radius: 12px;
		background: #1e1e23;
		color: var(--pp-on-ink);
		font: 500 14px var(--pp-font-body);
		resize: vertical;
	}
	.colour input {
		width: 44px;
		height: 36px;
		padding: 0;
		border: 0;
		border-radius: 10px;
		background: transparent;
		cursor: pointer;
	}
	.file {
		display: none;
	}
	.msg {
		margin: 0;
		color: #ffd6cc;
		font-weight: 600;
		font-size: 14px;
	}
	.msg.plain {
		color: var(--pp-on-ink);
		font-weight: 500;
	}
	.opt-label {
		font-size: 13px;
		font-weight: 600;
		color: rgba(255, 255, 255, 0.75);
	}
	/* always one compact row (scrolls sideways when the window is narrow) */
	.fill-opts {
		display: flex;
		flex-wrap: nowrap;
		align-items: center;
		gap: 8px 12px;
		min-width: 0;
		overflow-x: auto;
		scrollbar-width: thin;
	}
	.fill-opts > * {
		flex: none;
	}
</style>
