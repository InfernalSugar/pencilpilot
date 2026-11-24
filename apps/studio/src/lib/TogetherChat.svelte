<script lang="ts">
	/** The chat box while drawing together: a short, collapsible list and a line to type in. */
	import { MAX_CHAT_CHARS, type ChatLine } from './chat';
	import { memberColour } from './share';

	let {
		lines,
		unread,
		open = $bindable(false),
		onsend
	}: {
		lines: ChatLine[];
		unread: number;
		open: boolean;
		onsend: (text: string) => void;
	} = $props();

	let text = $state('');
	let list: HTMLElement | undefined = $state();

	$effect(() => {
		// (new lines scroll to the bottom)
		if (open && list && lines.length) list.scrollTop = list.scrollHeight;
	});

	function send() {
		if (!text.trim()) return;
		onsend(text);
		text = '';
	}
</script>

<section class="chat" aria-label="Chat">
	<button class="head" aria-expanded={open} onclick={() => (open = !open)}>
		Chat
		{#if !open && unread > 0}
			<span class="badge" aria-label="{unread} unread">{unread}</span>
		{/if}
	</button>
	{#if open}
		<ul class="lines" bind:this={list} aria-live="polite">
			{#each lines as l (l.id)}
				{#if l.kind === 'system'}
					<li class="sys">{l.text}</li>
				{:else}
					<li class:mine={l.mine}>
						<b style="color: {memberColour(l.from ?? '')}">{l.mine ? 'You' : l.name}</b>
						<span>{l.text}</span>
					</li>
				{/if}
			{/each}
		</ul>
		<input
			aria-label="Message"
			placeholder="Say something"
			maxlength={MAX_CHAT_CHARS}
			autocomplete="off"
			bind:value={text}
			onkeydown={(e) => {
				e.stopPropagation();
				if (e.key === 'Enter' && !e.isComposing) {
					e.preventDefault();
					send();
				}
			}}
		/>
	{/if}
</section>

<style>
	.chat {
		position: fixed;
		right: 12px;
		bottom: 12px;
		z-index: 30;
		width: 260px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		border: 1px solid var(--pp-track);
		border-radius: var(--pp-radius-card);
		font-size: 13px;
		font-family: var(--pp-font-body);
		box-shadow: 0 2px 10px #0003;
	}
	.head {
		all: unset;
		box-sizing: border-box;
		width: 100%;
		padding: 6px 10px;
		cursor: pointer;
		display: flex;
		justify-content: space-between;
		align-items: center;
		font-weight: 600;
	}
	.head:focus-visible {
		outline: 2px solid var(--pp-accent);
		outline-offset: -2px;
	}
	.badge {
		background: var(--pp-accent-strong);
		color: var(--pp-on-ink);
		border-radius: 10px;
		padding: 0 6px;
		font-size: 11px;
	}
	.lines {
		list-style: none;
		margin: 0;
		padding: 4px 10px;
		max-height: 180px;
		overflow-y: auto;
		display: grid;
		gap: 3px;
		border-top: 1px solid var(--pp-track);
	}
	li {
		overflow-wrap: anywhere;
		white-space: pre-wrap;
	}
	li b {
		margin-right: 4px;
	}
	.sys {
		color: var(--pp-muted);
		font-style: italic;
	}
	input {
		box-sizing: border-box;
		width: calc(100% - 12px);
		margin: 6px;
		padding: 4px 6px;
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
		border: 1px solid var(--pp-track);
		border-radius: var(--pp-radius-btn);
		font: inherit;
	}
</style>
