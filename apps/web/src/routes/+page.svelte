<script lang="ts">
	import { onMount } from 'svelte';
	import { gradeImages } from '#lib/grader.js';

	let graderStatus = $state('loading…');

	onMount(async () => {
		const blank = new Uint8Array(16).fill(255);
		const g = await gradeImages(4, 4, blank, blank);
		graderStatus = `ready (self-test score ${g.score.toFixed(1)})`;
	});
</script>

<main>
	<h1>✏️ PencilPilot</h1>
	<p>Learn to draw by matching references. Under construction!</p>
	<p data-testid="grader-status">Grader: {graderStatus}</p>
</main>
