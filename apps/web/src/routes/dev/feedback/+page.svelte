<script lang="ts">
	import { ruleById } from '#lib/rules.js';
	import type { PageProps } from './$types';
	let { data }: PageProps = $props();
</script>

<svelte:head><title>Tip ratings · PencilPilot (dev)</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">Dev only · M8-T03</span>
	<h1>Tip ratings</h1>
	<p class="lede">
		Least helpful first (Wilson lower bound of 👍 share). Rewrite or retune rules near the top once
		they have a few votes.
	</p>
	{#if !data.stats.length}
		<p>No votes yet.</p>
	{:else}
		<table>
			<thead>
				<tr><th>Rule</th><th>Tip</th><th>👍</th><th>👎</th><th>Helpful (low bound)</th></tr>
			</thead>
			<tbody>
				{#each data.stats as s (s.ruleId)}
					<tr class:weak={s.up + s.down >= 5 && s.helpful < 0.4}>
						<td><code>{s.ruleId}</code></td>
						<td>{ruleById(s.ruleId)?.title ?? '—'}</td>
						<td>{s.up}</td>
						<td>{s.down}</td>
						<td>{Math.round(s.helpful * 100)}%</td>
					</tr>
				{/each}
			</tbody>
		</table>
	{/if}
</div>

<style>
	h1 {
		font-size: 40px;
		margin: 4px 0 8px;
	}
	.lede {
		color: var(--pp-ink-2);
	}
	table {
		width: 100%;
		border-collapse: collapse;
		background: var(--pp-surface);
		border-radius: var(--pp-radius-card);
		overflow: hidden;
	}
	th,
	td {
		padding: 10px 14px;
		text-align: left;
		border-bottom: 1px solid var(--pp-track);
	}
	tr.weak {
		background: #fde4df;
	}
</style>
