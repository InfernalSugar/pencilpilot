<script lang="ts">
	/**
	 * Character palettes: the project's named colours (a character's skin, hair…), most
	 * often taken from a model sheet shown as the reference. A click paints with a
	 * colour; Edit renames, changes or removes them. Every change is one undo step.
	 */
	import {
		addColour,
		addPalette,
		removeColour,
		removePalette,
		renamePalette,
		setColour,
		MAX_PALETTES,
		type Palette
	} from './palettes';

	let {
		palettes,
		color,
		onchange,
		onuse,
		fromReference
	}: {
		palettes: Palette[];
		/** the colour painted with now */
		color: string;
		/** the palettes changed (one undo step, called `label`) */
		onchange: (next: Palette[], label: string) => void;
		/** paint with this colour */
		onuse: (hex: string) => void;
		/** the reference picture's flat colours, and its name (null: no picture) */
		fromReference?: () => Promise<{ name: string; colours: string[] } | null>;
	} = $props();

	/** the palette being edited (-1: none) */
	let editing = $state(-1);
	let note = $state('');
	let busy = $state(false);

	const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

	async function fromPicture() {
		if (!fromReference) return;
		busy = true;
		note = '';
		try {
			const got = await fromReference();
			if (!got) note = 'Show a model sheet as the reference first.';
			else if (!got.colours.length) note = 'No flat colours found in the reference.';
			else {
				const name = got.name.replace(/\.[a-z0-9]+$/i, '') || 'Model sheet';
				const next = addPalette(
					palettes,
					name,
					got.colours.map((hex, i) => ({ name: `Colour ${i + 1}`, hex }))
				);
				onchange(next, 'Palette from the reference');
				editing = next.length - 1;
				note = `${got.colours.length} colours: name them (Skin, Hair…) so they're easy to find.`;
			}
		} catch (e) {
			note = `Couldn’t read the reference: ${e}`;
		} finally {
			busy = false;
		}
	}
</script>

<section class="palettes" aria-label="Character palettes">
	<div class="head">
		<strong>Character palettes</strong>
		<button
			type="button"
			class="mini"
			disabled={palettes.length >= MAX_PALETTES}
			onclick={() => {
				const next = addPalette(palettes, `Character ${palettes.length + 1}`);
				onchange(next, 'New palette');
				editing = next.length - 1;
			}}
		>
			+ New
		</button>
		{#if fromReference}
			<button
				type="button"
				class="mini"
				disabled={busy || palettes.length >= MAX_PALETTES}
				title="Make a palette of the reference picture’s flat colours (a model sheet: its paper and lines left out)"
				onclick={fromPicture}
			>
				{busy ? 'Reading…' : 'From the reference'}
			</button>
		{/if}
	</div>
	{#if note}<p class="note" role="status">{note}</p>{/if}
	{#if !palettes.length}
		<p class="empty">
			Keep each character’s colours here: show their model sheet as the reference and take its
			colours, or add the colour you’re painting with.
		</p>
	{/if}
	{#each palettes as p, i (i)}
		<div class="palette" role="group" aria-label="Palette {p.name}">
			<div class="row">
				{#if editing === i}
					<input
						class="name"
						value={p.name}
						maxlength="40"
						aria-label="Palette name"
						onchange={(e) =>
							onchange(renamePalette(palettes, i, e.currentTarget.value), 'Rename palette')}
					/>
				{:else}
					<span class="title">{p.name}</span>
				{/if}
				<button
					type="button"
					class="mini"
					title="Add the colour you’re painting with"
					onclick={() => onchange(addColour(palettes, i, color), 'Add palette colour')}
				>
					+ This colour
				</button>
				<button
					type="button"
					class="mini"
					aria-pressed={editing === i}
					onclick={() => (editing = editing === i ? -1 : i)}
				>
					{editing === i ? 'Done' : 'Edit'}
				</button>
			</div>
			{#if editing === i}
				<ul class="edit">
					{#each p.colours as c, j (j)}
						<li>
							<span class="chip" style="background: {c.hex}" aria-hidden="true"></span>
							<input
								value={c.name}
								maxlength="40"
								aria-label="Name of {c.hex}"
								onchange={(e) =>
									onchange(
										setColour(palettes, i, j, { name: e.currentTarget.value }),
										'Rename palette colour'
									)}
							/>
							<button
								type="button"
								class="mini"
								title="Make it the colour you’re painting with"
								disabled={same(c.hex, color)}
								onclick={() =>
									onchange(setColour(palettes, i, j, { hex: color }), 'Change palette colour')}
							>
								Set
							</button>
							<button
								type="button"
								class="mini"
								aria-label="Remove {c.name}"
								onclick={() => onchange(removeColour(palettes, i, j), 'Remove palette colour')}
							>
								×
							</button>
						</li>
					{/each}
				</ul>
				<button
					type="button"
					class="mini quiet"
					onclick={() => {
						onchange(removePalette(palettes, i), 'Delete palette');
						editing = -1;
					}}
				>
					Delete palette
				</button>
			{:else}
				<div class="swatches">
					{#each p.colours as c, j (j)}
						<button
							type="button"
							class="sw"
							class:on={same(c.hex, color)}
							style="background: {c.hex}"
							aria-label="{p.name}: {c.name}"
							title="{c.name} ({c.hex})"
							onclick={() => onuse(c.hex)}
						></button>
					{:else}
						<p class="empty">No colours yet.</p>
					{/each}
				</div>
			{/if}
		</div>
	{/each}
</section>

<style>
	.palettes {
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding-bottom: 12px;
		margin-bottom: 4px;
		border-bottom: 1px solid var(--pp-track, rgba(0, 0, 0, 0.1));
		font: 600 13px var(--pp-font-body);
		color: var(--pp-ink);
	}
	.head,
	.row {
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.head strong,
	.row .title,
	.row .name {
		flex: 1;
		min-width: 0;
	}
	.title {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.palette {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 8px;
		border-radius: 12px;
		background: var(--pp-surface-sunk);
	}
	.mini {
		min-height: 26px;
		padding: 0 8px;
		border: 0;
		border-radius: 8px;
		background: var(--pp-surface);
		color: inherit;
		font: inherit;
		font-size: 12px;
		cursor: pointer;
	}
	.mini[aria-pressed='true'] {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.mini:disabled {
		opacity: 0.45;
		cursor: default;
	}
	.mini.quiet {
		align-self: flex-start;
		background: transparent;
		color: var(--pp-ink-2);
	}
	.swatches {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}
	.sw {
		width: 28px;
		height: 28px;
		padding: 0;
		border: 2px solid rgba(0, 0, 0, 0.12);
		border-radius: 8px;
		cursor: pointer;
	}
	.sw.on {
		outline: 2px solid var(--pp-accent);
		outline-offset: 1px;
	}
	.edit {
		margin: 0;
		padding: 0;
		list-style: none;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.edit li {
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.chip {
		flex: none;
		width: 20px;
		height: 20px;
		border-radius: 6px;
		border: 1px solid rgba(0, 0, 0, 0.12);
	}
	input {
		min-width: 0;
		flex: 1;
		box-sizing: border-box;
		padding: 4px 6px;
		border: 1px solid rgba(0, 0, 0, 0.12);
		border-radius: 8px;
		background: var(--pp-surface);
		color: inherit;
		font: inherit;
	}
	.empty,
	.note {
		margin: 0;
		font-weight: 500;
		color: var(--pp-ink-2);
		line-height: 1.4;
	}
</style>
