<script lang="ts">
	/** Earlier versions of the open project (each save keeps the one it replaces). */
	import { ago, listVersions, type VersionItem } from './project';

	let {
		path,
		disabled = false,
		onopen
	}: {
		path: string | null;
		disabled?: boolean;
		onopen: (id: string) => void;
	} = $props();

	let open = $state(false);
	let items = $state<VersionItem[] | null>(null);
	let error = $state('');

	async function toggle() {
		open = !open;
		if (!open || !path) return;
		items = null;
		error = '';
		try {
			items = await listVersions(path);
		} catch (e) {
			error = String(e);
		}
	}

	const size = (b: number) =>
		b < 1 << 20 ? `${Math.max(1, Math.round(b / 1024))} KB` : `${(b / (1 << 20)).toFixed(1)} MB`;
</script>

<div class="versions">
	<button
		type="button"
		class="act"
		aria-expanded={open}
		disabled={disabled || !path}
		title={path ? 'Earlier versions of this project' : 'Save the project first'}
		onclick={toggle}
	>
		Versions
	</button>
	{#if open}
		<div class="menu" role="group" aria-label="Versions">
			{#if error}
				<p>{error}</p>
			{:else if !items}
				<p>Looking…</p>
			{:else if !items.length}
				<p>No earlier versions yet. Each save keeps the version it replaces (the last 20).</p>
			{:else}
				<ul>
					{#each items as v (v.id)}
						<li>
							{#if v.thumbnail}
								<img src="data:image/png;base64,{v.thumbnail}" alt="" />
							{/if}
							<span class="when">{ago(v.savedAt)}<small>{size(v.bytes)}</small></span>
							<button
								type="button"
								onclick={() => {
									open = false;
									onopen(v.id);
								}}
							>
								Open
							</button>
						</li>
					{/each}
				</ul>
			{/if}
		</div>
	{/if}
</div>

<style>
	.versions {
		position: relative;
	}
	.act {
		min-height: 40px;
		padding: 0 14px;
		border: 0;
		border-radius: 12px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.act:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.menu {
		position: absolute;
		z-index: 30;
		top: calc(100% + 8px);
		right: 0;
		width: 320px;
		max-height: 60vh;
		overflow: auto;
		padding: 12px;
		border-radius: 18px;
		background: var(--pp-surface);
		box-shadow: 0 12px 32px rgba(0, 0, 0, 0.18);
	}
	p {
		margin: 4px;
		color: var(--pp-muted);
		font-size: 14px;
	}
	ul {
		display: grid;
		gap: 8px;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	li {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	img {
		width: 64px;
		height: 40px;
		object-fit: contain;
		border-radius: 6px;
		background: #fff;
		outline: 1px solid var(--pp-surface-sunk);
	}
	.when {
		flex: 1;
		display: grid;
		font-size: 14px;
	}
	small {
		color: var(--pp-muted);
	}
	li button {
		min-height: 36px;
		padding: 0 12px;
		border: 0;
		border-radius: 10px;
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
</style>
