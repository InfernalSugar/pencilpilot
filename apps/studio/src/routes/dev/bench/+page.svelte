<script lang="ts">
	/**
	 * The engine benchmark as WebAssembly: the same workload as
	 * `cargo run --release -p pencilpilot-brush --example bench` (native), so the two can
	 * be compared, plus the round trip to the app's Rust side when it runs in the app.
	 * Results show as JSON in the page (and go to the app's log, in the app).
	 */
	import { onMount } from 'svelte';
	import { loadEngine, PaintBrush, PaintCanvas } from '@pencilpilot/paint/brushes';
	import { isTauri } from '@tauri-apps/api/core';

	const W = 1920;
	const H = 1080;
	const BRUSHES: [string, number][] = [
		['deevad-2b-pencil', 1],
		['deevad-airbrush', 3],
		['deevad-liner', 2]
	];
	let result = $state('running…');

	async function load(id: string, scale: number) {
		const b = new PaintBrush(await (await fetch(`/brushes/${id}.myb`)).text());
		b.setSizeScale(scale);
		b.setColor(22, 22, 26);
		return b;
	}

	function stroke(p: PaintCanvas, b: PaintBrush, s: number, flush: boolean) {
		const y0 = 100 + ((s * 13) % 880);
		p.beginStroke();
		for (let i = 0; i < 120; i++) {
			const x = 100 + i * 14;
			const y = y0 + 40 * Math.sin(i / 9 + s);
			const pr = 0.3 + 0.6 * Math.sin(i / 20) ** 2;
			p.strokeTo(b, x, y, pr, 0, 0, 0.008);
			if (flush && i % 4 === 3) p.flush();
		}
		p.endStroke(b);
		if (flush) p.flush();
		return 120;
	}

	async function ipc() {
		if (!isTauri()) return null;
		const { invoke } = await import('@tauri-apps/api/core');
		const small: number[] = [];
		for (let i = 0; i < 300; i++) {
			const t = performance.now();
			await invoke('app_info');
			small.push(performance.now() - t);
		}
		const big = new Uint8Array(256 * 256 * 4); // a dirty rect's pixels
		const bytes: number[] = [];
		for (let i = 0; i < 100; i++) {
			const t = performance.now();
			await invoke('bench_echo', big);
			bytes.push(performance.now() - t);
		}
		const med = (a: number[]) => a.sort((x, y) => x - y)[Math.floor(a.length / 2)];
		const p95 = (a: number[]) => a.sort((x, y) => x - y)[Math.floor(a.length * 0.95)];
		return {
			smallMedianMs: +med(small).toFixed(3),
			smallP95Ms: +p95(small).toFixed(3),
			rect256MedianMs: +med(bytes).toFixed(3),
			rect256P95Ms: +p95(bytes).toFixed(3)
		};
	}

	onMount(async () => {
		await loadEngine();
		const p = new PaintCanvas(W, H);
		p.flush();
		let samples = 0;
		const perBrush: Record<string, number> = {};
		const t0 = performance.now();
		for (const [id, scale] of BRUSHES) {
			const b = await load(id, scale);
			const tb = performance.now();
			let n = 0;
			for (let s = 0; s < 60; s++) n += stroke(p, b, s, true);
			perBrush[id] = +((performance.now() - tb) / n).toFixed(4);
			samples += n;
			b.free();
		}
		const paintMs = performance.now() - t0;
		p.free();

		const c = new PaintCanvas(W, H);
		const air = await load('deevad-airbrush', 3);
		let bottom = 0;
		for (let l = 0; l < 10; l++) {
			if (l > 0) c.addLayer('');
			else bottom = c.activeLayer;
			for (let s = 0; s < 20; s++) stroke(c, air, s * 7 + l, false);
		}
		c.flush();
		const tc = performance.now();
		for (let i = 0; i < 20; i++) {
			c.setLayer(bottom, 'Layer 1', i % 2 === 1, 1, 'normal', false, false, false, false);
			c.flush();
		}
		const compositeMs = (performance.now() - tc) / 20;
		c.free();
		air.free();

		const out = {
			where: isTauri() ? 'app (WebAssembly)' : `browser (WebAssembly): ${navigator.userAgent}`,
			paintMs: +paintMs.toFixed(1),
			samples,
			msPerSample: +(paintMs / samples).toFixed(4),
			perBrush,
			compositeMs: +compositeMs.toFixed(1),
			ipc: await ipc()
		};
		result = JSON.stringify(out, null, 2);
		if (isTauri()) {
			const { invoke } = await import('@tauri-apps/api/core');
			await invoke('bench_report', { report: result }).catch(() => {});
		}
	});
</script>

<main>
	<h1>Engine benchmark</h1>
	<pre data-testid="bench">{result}</pre>
</main>

<style>
	main {
		padding: 24px;
	}
	pre {
		padding: 16px;
		border-radius: 12px;
		background: var(--pp-surface);
	}
</style>
