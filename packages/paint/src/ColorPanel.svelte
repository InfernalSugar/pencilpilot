<script lang="ts" module>
	export const STARTER_PALETTES: { name: string; colors: string[] }[] = [
		{
			name: 'Pencil greys',
			colors: ['#16161a', '#3a3a40', '#5c5b62', '#807f86', '#a6a5aa', '#cfcdd0']
		},
		{
			name: 'Skin tones',
			colors: ['#f6d7c3', '#e8b796', '#c98e6a', '#a86b4a', '#7a4a32', '#4a2c1e']
		},
		{ name: 'Earth', colors: ['#5b4632', '#8a6a46', '#b8925e', '#6e7b4a', '#4d5e3a', '#a3543a'] },
		{ name: 'Pastels', colors: ['#f7c5cc', '#fbe1b6', '#fdf5b8', '#c8ecc4', '#bfe0f5', '#d9cdf3'] },
		{ name: 'Vivid', colors: ['#e8492f', '#f2a93b', '#f5d63d', '#1f9d74', '#3d6bff', '#8b5cf6'] }
	];

	const RECENTS = 'pp.recentColors';
	const MINE = 'pp.myPalette';
	const load = (k: string): string[] => {
		try {
			const v = JSON.parse(localStorage.getItem(k) ?? '[]');
			return Array.isArray(v) ? v.filter((c) => /^#[0-9a-f]{6}$/i.test(c)) : [];
		} catch {
			return [];
		}
	};
	const save = (k: string, v: string[]) => {
		try {
			localStorage.setItem(k, JSON.stringify(v));
		} catch {
			/* storage blocked: kept for this page only */
		}
	};

	/** Remember a colour that was just drawn with (most recent first, 12 kept). */
	export function rememberColor(c: string) {
		const c2 = c.toLowerCase();
		save(RECENTS, [c2, ...load(RECENTS).filter((x) => x !== c2)].slice(0, 12));
	}

	export function hexToHsv(hex: string): [number, number, number] {
		const n = parseInt(hex.slice(1), 16) || 0;
		const [r, g, b] = [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
		const max = Math.max(r, g, b);
		const d = max - Math.min(r, g, b);
		let h = 0;
		if (d) {
			if (max === r) h = ((g - b) / d + 6) % 6;
			else if (max === g) h = (b - r) / d + 2;
			else h = (r - g) / d + 4;
		}
		return [h * 60, max ? d / max : 0, max];
	}

	export function hsvToHex(h: number, s: number, v: number): string {
		const f = (n: number) => {
			const k = (n + h / 60) % 6;
			return v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
		};
		return (
			'#' +
			[f(5), f(3), f(1)]
				.map((x) =>
					Math.round(x * 255)
						.toString(16)
						.padStart(2, '0')
				)
				.join('')
		);
	}
</script>

<script lang="ts">
	import { harmony, HARMONIES, type Harmony } from './harmony';

	let { color = $bindable(), onclose }: { color: string; onclose: () => void } = $props();

	let hsv = $state(hexToHsv(color));
	let harmonyKind = $state<Harmony>('complementary');
	let hex = $state(color);
	let recents = $state(load(RECENTS));
	let mine = $state(load(MINE));
	let editing = $state(false);
	let sv: HTMLDivElement;

	// keep the picker in step when the colour changes elsewhere (swatches, eyedropper)
	$effect(() => {
		if (color.toLowerCase() !== hsvToHex(...hsv)) hsv = hexToHsv(color);
		hex = color;
	});

	function set(h: number, s: number, v: number) {
		hsv = [h, Math.max(0, Math.min(1, s)), Math.max(0, Math.min(1, v))];
		color = hsvToHex(...hsv);
	}

	function svAt(e: PointerEvent) {
		const r = sv.getBoundingClientRect();
		set(hsv[0], (e.clientX - r.left) / r.width, 1 - (e.clientY - r.top) / r.height);
	}

	function svKey(e: KeyboardEvent) {
		const d = e.shiftKey ? 0.1 : 0.02;
		const [h, s, v] = hsv;
		if (e.key === 'ArrowLeft') set(h, s - d, v);
		else if (e.key === 'ArrowRight') set(h, s + d, v);
		else if (e.key === 'ArrowUp') set(h, s, v + d);
		else if (e.key === 'ArrowDown') set(h, s, v - d);
		else return;
		e.preventDefault();
	}

	function addMine() {
		const c = color.toLowerCase();
		if (!mine.includes(c)) mine = [...mine, c].slice(-30);
		save(MINE, mine);
	}
	function removeMine(c: string) {
		mine = mine.filter((x) => x !== c);
		save(MINE, mine);
	}
</script>

<div class="panel" role="dialog" aria-label="Colours" tabindex="-1">
	<div class="head">
		<span class="now" style="background: {color}" aria-hidden="true"></span>
		<label class="hex">
			<span class="visually-hidden">Hex colour</span>
			<input
				value={hex}
				maxlength="7"
				spellcheck="false"
				aria-label="Hex colour"
				onchange={(e) => {
					const v = e.currentTarget.value.trim();
					const full = /^#?[0-9a-f]{6}$/i.test(v) ? (v.startsWith('#') ? v : '#' + v) : null;
					if (full) color = full.toLowerCase();
					else e.currentTarget.value = color;
				}}
			/>
		</label>
		<button type="button" class="close" onclick={onclose}>Done</button>
	</div>

	<div
		class="sv"
		bind:this={sv}
		role="slider"
		tabindex="0"
		aria-label="Saturation and brightness"
		aria-valuetext="saturation {Math.round(hsv[1] * 100)}%, brightness {Math.round(hsv[2] * 100)}%"
		aria-valuenow={Math.round(hsv[2] * 100)}
		style="background-color: hsl({hsv[0]} 100% 50%)"
		onpointerdown={(e) => {
			sv.setPointerCapture(e.pointerId);
			svAt(e);
		}}
		onpointermove={(e) => e.buttons && svAt(e)}
		onkeydown={svKey}
	>
		<span class="knob" style="left: {hsv[1] * 100}%; top: {(1 - hsv[2]) * 100}%"></span>
	</div>
	<label class="hue">
		<span class="visually-hidden">Hue</span>
		<input
			type="range"
			min="0"
			max="359"
			value={Math.round(hsv[0])}
			aria-label="Hue"
			oninput={(e) => set(+e.currentTarget.value, hsv[1] || 0.8, hsv[2] || 0.8)}
		/>
	</label>

	<div class="label row">
		<span>Harmony</span>
		<select
			class="mini harmony-kind"
			bind:value={harmonyKind}
			aria-label="Colour harmony"
			title={HARMONIES.find((h) => h.id === harmonyKind)?.hint}
		>
			{#each HARMONIES as h (h.id)}<option value={h.id}>{h.name}</option>{/each}
		</select>
	</div>
	<div data-testid="harmony">{@render swatches(harmony(color, harmonyKind))}</div>

	{#if recents.length}
		<p class="label">Recent</p>
		{@render swatches(recents)}
	{/if}

	<div class="label row">
		<span>My palette</span>
		<button type="button" class="mini" onclick={addMine}>+ Add this colour</button>
		{#if mine.length}
			<button
				type="button"
				class="mini"
				aria-pressed={editing}
				onclick={() => (editing = !editing)}
			>
				{editing ? 'Done editing' : 'Edit'}
			</button>
		{/if}
	</div>
	{#if mine.length}
		{@render swatches(mine, editing)}
	{:else}
		<p class="empty">Add colours you use a lot.</p>
	{/if}

	{#each STARTER_PALETTES as p (p.name)}
		<p class="label">{p.name}</p>
		{@render swatches(p.colors)}
	{/each}
</div>

{#snippet swatches(list: string[], removable = false)}
	<div class="swatches">
		{#each list as c (c)}
			<button
				type="button"
				class="sw"
				class:on={c.toLowerCase() === color.toLowerCase()}
				style="background: {c}"
				aria-label={removable ? `Remove ${c}` : `Use ${c}`}
				onclick={() => (removable ? removeMine(c) : (color = c))}
			>
				{#if removable}<span aria-hidden="true">×</span>{/if}
			</button>
		{/each}
	</div>
{/snippet}

<style>
	.panel {
		display: flex;
		flex-direction: column;
		gap: 8px;
		width: min(300px, calc(100vw - 32px));
		max-height: min(70vh, 640px);
		overflow-y: auto;
		padding: 16px;
		border-radius: 20px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		box-shadow: 0 10px 40px rgba(22, 22, 26, 0.25);
	}
	.head {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.now {
		width: 36px;
		height: 36px;
		border-radius: 10px;
		border: 1px solid var(--pp-track);
	}
	.hex input {
		width: 96px;
		min-height: 36px;
		padding: 4px 8px;
		border: 1px solid var(--pp-track);
		border-radius: 10px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font:
			600 14px ui-monospace,
			monospace;
	}
	.close,
	.mini {
		margin-left: auto;
		min-height: 32px;
		padding: 4px 12px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.mini {
		margin-left: 0;
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
	}
	.mini[aria-pressed='true'] {
		background: var(--pp-ink);
		color: var(--pp-on-ink);
	}
	.sv {
		position: relative;
		height: 150px;
		border-radius: 12px;
		background-image:
			linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, transparent);
		cursor: crosshair;
		touch-action: none;
	}
	.knob {
		position: absolute;
		width: 16px;
		height: 16px;
		margin: -8px 0 0 -8px;
		border: 2px solid #fff;
		border-radius: 999px;
		box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.5);
		pointer-events: none;
	}
	.hue input {
		width: 100%;
		height: 18px;
		-webkit-appearance: none;
		appearance: none;
		border-radius: 999px;
		background: linear-gradient(to right, red, #ff0, lime, cyan, blue, #f0f, red);
	}
	.label {
		margin: 6px 0 0;
		font-size: 12px;
		font-weight: 700;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		color: var(--pp-ink-2);
	}
	.label.row {
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.empty {
		margin: 0;
		font-size: 13px;
		color: var(--pp-muted);
	}
	.swatches {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}
	.sw {
		display: grid;
		place-items: center;
		width: 30px;
		height: 30px;
		padding: 0;
		border: 1px solid var(--pp-track);
		border-radius: 8px;
		color: #fff;
		font-weight: 800;
		text-shadow: 0 0 2px #000;
		cursor: pointer;
	}
	.sw.on {
		outline: 3px solid var(--pp-ink);
		outline-offset: 1px;
	}
	.visually-hidden {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
</style>
