<script lang="ts">
	/**
	 * The credit line for a reference picture, worded the way its source asks
	 * (see credit.ts). Inline: put it inside the page's own `<p class="credit">`.
	 */
	import { attributionSentence, creditKind, providerName, type Creditable } from './credit';

	let {
		reference,
		license = true,
		lead = 'By'
	}: {
		reference: Creditable;
		/** also link the licence */
		license?: boolean;
		/** first word for Pixabay credits ("By", "Reference by", "by") */
		lead?: string;
	} = $props();

	const kind = $derived(creditKind(reference));
	const provider = $derived(providerName(reference));
	const sentence = $derived(attributionSentence(reference.attribution));
</script>

{#snippet author()}
	{#if reference.authorUrl}
		<a href={reference.authorUrl} target="_blank" rel="noreferrer">{reference.author}</a>
	{:else}
		{reference.author}
	{/if}
{/snippet}

{#snippet licenseLink()}
	{#if license}
		&nbsp;· <a href={reference.licenseUrl} target="_blank" rel="noreferrer">{reference.license}</a>
	{/if}
{/snippet}

{#if kind === 'museum'}
	Artwork: <a href={reference.pageUrl} target="_blank" rel="noreferrer">
		{reference.title || 'Untitled'}
	</a>, {@render author()}, {provider} (<a
		href={reference.licenseUrl}
		target="_blank"
		rel="noreferrer"
	>
		public domain
	</a>)
{:else if kind === 'pexels'}
	Photo by {@render author()} on
	<a href={reference.pageUrl} target="_blank" rel="noreferrer">Pexels</a>{@render licenseLink()}
{:else if kind === 'openverse'}
	{#if sentence}
		<span title={reference.attribution}>{sentence}</span>
	{:else}
		“{reference.title || 'Untitled'}” by {@render author()}
	{/if}
	· via
	<a href={reference.pageUrl} target="_blank" rel="noreferrer">{provider}</a>{@render licenseLink()}
{:else}
	{lead}
	{@render author()}
	via
	<a href={reference.pageUrl} target="_blank" rel="noreferrer">{provider}</a>{@render licenseLink()}
{/if}
