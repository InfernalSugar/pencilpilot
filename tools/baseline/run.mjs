#!/usr/bin/env node
// The baseline: measures the current app (Tauri + WebKitGTK) on this machine, off-screen.
//
//   node tools/baseline/run.mjs --app <pencilpilot-studio binary> --projects <dir> --out <report.json>
//        [--quick] [--cold] [--display pp-bench-0]
//
// The app runs on a headless Wayland display (mutter --headless, on the real GPU), so no
// window opens on the screen. It is driven over WebDriver (tauri-driver + WebKitWebDriver),
// with its data, settings and caches in a folder of their own. CPU, memory, GPU and power
// are read from /proc and /sys while each scenario runs. See README.md.

import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';

// ---- arguments ----

const argv = process.argv.slice(2);
const opt = (name, dflt) => {
	const i = argv.indexOf(`--${name}`);
	return i < 0 ? dflt : argv[i + 1];
};
const flag = (name) => argv.includes(`--${name}`);
const APP = path.resolve(opt('app', ''));
const PROJECTS = path.resolve(opt('projects', ''));
const OUT = path.resolve(opt('out', 'baseline.json'));
const DISPLAY = opt('display', 'pp-bench-0');
const QUICK = flag('quick');
const COLD = flag('cold');
const PORT = 4471;
const MEM_LIMIT_MB = 12_000; // a scenario that grows past this is stopped (the PC has 31 GB)

if (!fs.existsSync(APP) || !fs.existsSync(PROJECTS)) {
	console.error(
		'usage: run.mjs --app <binary> --projects <dir> --out <report.json> [--quick] [--cold]'
	);
	process.exit(2);
}
const project = (n) => path.join(PROJECTS, `${n}.ppstudio`);
const log = (...a) => console.error(`[baseline ${new Date().toISOString().slice(11, 19)}]`, ...a);

// ---- the headless display ----

const runtime = process.env.XDG_RUNTIME_DIR;
const children = [];
function headless() {
	if (fs.existsSync(path.join(runtime, DISPLAY)))
		return log(`using the headless display ${DISPLAY}`);
	log(`starting a headless display ${DISPLAY} (3840x2160@60, off-screen)`);
	const env = { ...process.env };
	delete env.DISPLAY;
	delete env.WAYLAND_DISPLAY;
	const m = spawn(
		'dbus-run-session',
		[
			'--',
			'mutter',
			'--headless',
			'--no-x11',
			'--wayland-display',
			DISPLAY,
			'--virtual-monitor',
			'3840x2160@60'
		],
		{ env, stdio: 'ignore' }
	);
	children.push(m);
}

// ---- reading the machine ----

const TICK = os.constants?.CLK_TCK ?? 100;
const read = (f) => {
	try {
		return fs.readFileSync(f, 'utf8');
	} catch {
		return null;
	}
};
const gpuDir = (() => {
	for (const c of fs.readdirSync('/sys/class/drm').filter((d) => /^card\d+$/.test(d))) {
		const dev = `/sys/class/drm/${c}/device`;
		if (!fs.existsSync(`${dev}/gpu_busy_percent`)) continue;
		const hw = fs.readdirSync(`${dev}/hwmon`).map((h) => `${dev}/hwmon/${h}`);
		return {
			busy: `${dev}/gpu_busy_percent`,
			power: hw.map((h) => `${h}/power1_average`).find(fs.existsSync)
		};
	}
	return null;
})();
const RAPL = '/sys/class/powercap/intel-rapl:0';
const raplMax = +(read(`${RAPL}/max_energy_range_uj`) ?? 0);

function procs() {
	const out = new Map();
	for (const d of fs.readdirSync('/proc')) {
		if (!/^\d+$/.test(d)) continue;
		const s = read(`/proc/${d}/stat`);
		if (!s) continue;
		const r = s.slice(s.lastIndexOf(')') + 2).split(' ');
		out.set(+d, {
			ppid: +r[1],
			ticks: +r[11] + +r[12],
			comm: s.slice(s.indexOf('(') + 1, s.lastIndexOf(')'))
		});
	}
	return out;
}
function tree(root) {
	const all = procs();
	const pids = new Set([root]);
	let grew = true;
	while (grew) {
		grew = false;
		for (const [pid, p] of all)
			if (!pids.has(pid) && pids.has(p.ppid)) (pids.add(pid), (grew = true));
	}
	return [...pids].filter((p) => all.has(p)).map((p) => ({ pid: p, ...all.get(p) }));
}
const pssKB = (pid) => +(/^Pss:\s+(\d+)/m.exec(read(`/proc/${pid}/smaps_rollup`) ?? '')?.[1] ?? 0);

/** samples the app's processes and the machine every 100 ms until stopped */
function sampler(root) {
	const t0 = performance.now();
	const first = new Map(tree(root).map((p) => [p.pid, p.ticks]));
	let e0 = +(read(`${RAPL}/energy_uj`) ?? 0);
	let joules = 0;
	const gpu = [];
	const gpuW = [];
	const pss = [];
	const last = new Map(first);
	let tooBig = false;
	const id = setInterval(() => {
		const t = tree(root);
		for (const p of t) last.set(p.pid, p.ticks);
		const e = +(read(`${RAPL}/energy_uj`) ?? 0);
		joules += (e >= e0 ? e - e0 : e + raplMax - e0) / 1e6;
		e0 = e;
		if (gpuDir) {
			gpu.push(+read(gpuDir.busy));
			if (gpuDir.power) gpuW.push(+read(gpuDir.power) / 1e6);
		}
		const mb = t.reduce((a, p) => a + pssKB(p.pid), 0) / 1024;
		pss.push(mb);
		if (mb > MEM_LIMIT_MB && !tooBig) {
			tooBig = true;
			log(`the app passed ${MEM_LIMIT_MB} MB: stopping it`);
			for (const p of t) {
				try {
					process.kill(p.pid, 'SIGKILL');
				} catch {
					/* gone */
				}
			}
		}
	}, 100);
	return {
		get tooBig() {
			return tooBig;
		},
		stop() {
			clearInterval(id);
			const secs = (performance.now() - t0) / 1000;
			let ticks = 0;
			for (const [pid, v] of last) ticks += v - (first.get(pid) ?? 0);
			const mean = (a) => (a.length ? a.reduce((x, y) => x + y, 0) / a.length : null);
			return {
				seconds: +secs.toFixed(2),
				cpuCorePct: +((100 * ticks) / TICK / secs).toFixed(2), // 100 = one core busy
				pssMeanMB: +mean(pss)?.toFixed(1),
				pssMaxMB: +Math.max(0, ...pss).toFixed(1),
				gpuBusyPct: gpu.length ? +mean(gpu).toFixed(1) : null,
				gpuW: gpuW.length ? +mean(gpuW).toFixed(2) : null,
				cpuPackageW: +(joules / secs).toFixed(2)
			};
		}
	};
}

// ---- WebDriver ----

const WD = `http://127.0.0.1:${PORT}`;
async function wd(method, url, body) {
	const r = await fetch(WD + url, {
		method,
		headers: { 'content-type': 'application/json' },
		body: body === undefined ? undefined : JSON.stringify(body)
	});
	const j = await r.json().catch(() => ({}));
	if (!r.ok) throw new Error(`${method} ${url}: ${JSON.stringify(j.value ?? j).slice(0, 400)}`);
	return j.value;
}

let driver;
const dataHome = path.join(path.dirname(OUT), 'baseline-profile');
function startDriver() {
	const env = { ...process.env, WAYLAND_DISPLAY: DISPLAY, GDK_BACKEND: 'wayland' };
	delete env.DISPLAY;
	delete env.PP_BENCH;
	// the app's data, settings and caches stay out of the person's own
	for (const [k, d] of [
		['XDG_DATA_HOME', 'data'],
		['XDG_CONFIG_HOME', 'config'],
		['XDG_CACHE_HOME', 'cache']
	]) {
		env[k] = path.join(dataHome, d);
		fs.mkdirSync(env[k], { recursive: true });
	}
	driver = spawn(
		'tauri-driver',
		['--port', String(PORT), '--native-driver', '/usr/bin/WebKitWebDriver'],
		{
			env,
			stdio: ['ignore', 'ignore', 'pipe']
		}
	);
	driver.stderr.on('data', () => {});
	children.push(driver);
}

/** the app's own process: the binary, started under the driver */
async function appPid() {
	const real = fs.realpathSync(APP);
	for (let i = 0; i < 100; i++) {
		for (const p of tree(driver.pid)) {
			try {
				if (fs.readlinkSync(`/proc/${p.pid}/exe`) === real) return p.pid;
			} catch {
				/* gone */
			}
		}
		await sleep(50);
	}
	throw new Error('the app did not start');
}

class App {
	static async launch(recent = []) {
		// a fresh data folder every time: no recent project to reopen, default settings
		// (WebKit's caches, in the cache folder, stay, so later starts are warm)
		const data = path.join(dataHome, 'data');
		fs.rmSync(data, { recursive: true, force: true });
		fs.mkdirSync(path.join(data, 'app.pencilpilot.studio'), { recursive: true });
		// the projects to open are on the home screen's Recent list, opened from there as a
		// person would (a page reload instead would race the app's frame store)
		fs.writeFileSync(
			path.join(data, 'app.pencilpilot.studio', 'recent.json'),
			JSON.stringify(recent)
		);
		const t0 = performance.now();
		const s = await wd('POST', '/session', {
			capabilities: { alwaysMatch: { 'tauri:options': { application: APP } } }
		});
		const app = new App(s.sessionId, t0);
		app.pid = await appPid();
		await wd('POST', `/session/${app.id}/timeouts`, { script: 600_000, pageLoad: 120_000 });
		return app;
	}
	constructor(id, t0) {
		this.id = id;
		this.t0 = t0;
	}
	run(fn, ...args) {
		return wd('POST', `/session/${this.id}/execute/sync`, {
			script: `return (${fn})(...arguments)`,
			args
		});
	}
	/** an async page function; resolves with what it returns */
	runAsync(fn, ...args) {
		const script = `const done = arguments[arguments.length - 1];
			Promise.resolve((${fn})(...Array.prototype.slice.call(arguments, 0, -1)))
				.then((v) => done({ ok: v }), (e) => done({ err: String(e && e.stack || e) }));`;
		return wd('POST', `/session/${this.id}/execute/async`, { script, args }).then((r) => {
			if (r?.err) throw new Error(r.err);
			return r?.ok;
		});
	}
	async until(fn, ms = 120_000, ...args) {
		const t = performance.now();
		while (performance.now() - t < ms) {
			try {
				if (await this.run(fn, ...args)) return performance.now();
			} catch {
				/* mid-navigation */
			}
			await sleep(20);
		}
		throw new Error(`timed out waiting for ${String(fn).slice(0, 120)}`);
	}
	async screenshot(file) {
		const b64 = await wd('GET', `/session/${this.id}/screenshot`);
		fs.writeFileSync(file, Buffer.from(b64, 'base64'));
	}
	async maximise() {
		await wd('POST', `/session/${this.id}/window/rect`, {
			x: 0,
			y: 0,
			width: 3840,
			height: 2160
		}).catch(() => wd('POST', `/session/${this.id}/window/maximize`, {}));
	}
	async quit() {
		const pids = tree(this.pid).map((p) => p.pid);
		await Promise.race([wd('DELETE', `/session/${this.id}`).catch(() => {}), sleep(5000)]);
		await sleep(300);
		for (const sig of ['SIGTERM', 'SIGKILL']) {
			for (const p of pids) {
				try {
					process.kill(p, sig);
				} catch {
					/* gone */
				}
			}
			await sleep(500);
		}
	}
}

// ---- in the page ----

/** counts the app's own animation-frame requests (installed after every page load) */
function instrument() {
	if (window.__ppb) return true;
	const orig = window.requestAnimationFrame.bind(window);
	window.__ppb = { orig, requests: 0 };
	window.requestAnimationFrame = (cb) => {
		window.__ppb.requests++;
		return orig(cb);
	};
	return true;
}

const homeReady = () =>
	document.readyState === 'complete' && !!document.querySelector('button.primary');
const studioReady = (frames) => {
	const f = document.querySelector('[data-testid="frame"]');
	if (!f || !document.querySelector('.viewport canvas')) return false;
	const opening = [...document.querySelectorAll('button')].some(
		(b) => b.textContent.trim() === 'Opening…'
	);
	return !opening && (!frames || f.parentElement.textContent.includes(`of ${frames}`));
};

/** pen strokes at 240 Hz across the canvas: when each sample is drawn, and the frame times */
async function penStrokes(cfg) {
	const vp = document.querySelector('.viewport');
	const r = vp.getBoundingClientRect();
	const raf = window.__ppb.orig;
	const dt = 1000 / cfg.hz;
	const frames = [];
	let looping = true;
	raf(function loop(t) {
		frames.push(t);
		if (looping) raf(loop);
	});
	const drawn = [];
	const shown = [];
	const ev = (type, x, y, p) =>
		vp.dispatchEvent(
			new PointerEvent(type, {
				pointerId: 7,
				pointerType: 'pen',
				isPrimary: true,
				bubbles: true,
				cancelable: true,
				composed: true,
				clientX: x,
				clientY: y,
				pressure: p,
				width: 1,
				height: 1,
				button: type === 'pointermove' ? -1 : 0,
				buttons: type === 'pointerup' ? 0 : 1
			})
		);
	const at = (s, i, n) => {
		const u = i / (n - 1);
		return [
			r.left + r.width * (0.1 + 0.8 * u),
			r.top + r.height * (0.15 + 0.7 * ((s * 0.13) % 1)) + r.height * 0.06 * Math.sin(u * 12 + s),
			0.25 + 0.65 * Math.sin(u * Math.PI) ** 2
		];
	};
	const t0 = performance.now() + 200;
	await new Promise((res) => setTimeout(res, 200));
	for (let s = 0; s < cfg.strokes; s++) {
		const n = Math.round(cfg.hz * cfg.strokeSeconds);
		const start = performance.now();
		let i = 0;
		while (i < n) {
			const now = performance.now();
			const due = [];
			while (i < n && start + i * dt <= now) {
				const [x, y, p] = at(s, i, n);
				ev(i === 0 ? 'pointerdown' : 'pointermove', x, y, p);
				due.push(start + i * dt);
				i++;
			}
			if (due.length) {
				// the app's redraw was asked for during the events, so it runs before this
				raf(() => {
					const a = performance.now();
					for (const t of due) drawn.push(a - t);
					raf(() => {
						const b = performance.now();
						for (const t of due) shown.push(b - t);
					});
				});
			}
			await new Promise((res) => setTimeout(res, Math.max(0, start + i * dt - performance.now())));
		}
		const [x, y] = at(s, n - 1, n);
		ev('pointerup', x, y, 0);
		await new Promise((res) => setTimeout(res, 250));
	}
	await new Promise((res) => setTimeout(res, 300));
	looping = false;
	const iv = frames.slice(1).map((t, k) => t - frames[k]);
	return { drawn, shown, intervals: iv, seconds: (performance.now() - t0) / 1000 };
}

/** play the cut on a loop for some seconds: when the shown frame changes */
async function playFor(seconds) {
	const raf = window.__ppb.orig;
	const seg = [...document.querySelectorAll('[role="radiogroup"][aria-label="Play"] button')];
	seg.find((b) => b.textContent.trim() === 'Loop')?.click();
	await new Promise((res) => setTimeout(res, 300));
	const readout = document.querySelector('[data-testid="frame"]');
	const length = +/of (\d+)/.exec(readout.parentElement.textContent)[1];
	const play = document.querySelector('button[aria-label="Play"]');
	if (!play) throw new Error('no Play button');
	const changes = [];
	const intervals = [];
	let last = readout.textContent;
	let prevT = 0;
	let started = 0;
	const tClick = performance.now();
	play.click();
	await new Promise((res) => {
		raf(function loop(t) {
			if (prevT && started) intervals.push(t - prevT);
			prevT = t;
			const v = readout.textContent;
			if (v !== last) {
				if (!started) started = performance.now();
				changes.push([performance.now(), +v]);
				last = v;
			}
			if (started && performance.now() - started > seconds * 1000) return res();
			if (!started && performance.now() - tClick > 60_000) return res();
			raf(loop);
		});
	});
	document.querySelector('button[aria-label="Stop"]')?.click();
	return { startMs: started ? started - tClick : null, changes, intervals, length };
}

// ---- statistics ----

const sorted = (a) => [...a].sort((x, y) => x - y);
const pct = (a, p) =>
	a.length ? sorted(a)[Math.min(a.length - 1, Math.floor((a.length * p) / 100))] : null;
const r2 = (x) => (x == null ? null : +x.toFixed(2));
const dist = (a) => ({
	n: a.length,
	p50: r2(pct(a, 50)),
	p95: r2(pct(a, 95)),
	p99: r2(pct(a, 99)),
	max: r2(Math.max(...a))
});

// ---- scenarios ----

const report = {
	when: new Date().toISOString(),
	app: { binary: APP, sizeMB: +(fs.statSync(APP).size / 1e6).toFixed(1) },
	machine: {
		cpu: /model name\s*:\s*(.*)/.exec(read('/proc/cpuinfo'))?.[1],
		threads: os.cpus().length,
		memGB: +(os.totalmem() / 2 ** 30).toFixed(1),
		gpu: (() => {
			try {
				return execFileSync('sh', ['-c', "lspci | grep -iE 'vga|3d' | cut -d: -f3-"])
					.toString()
					.trim();
			} catch {
				return null;
			}
		})(),
		kernel: os.release(),
		display: `headless mutter ${DISPLAY}, 3840x2160@60`
	},
	notes: [],
	results: {}
};

async function systemIdle(label, secs) {
	log(`machine without the app, ${secs} s (${label})`);
	const s = sampler(process.pid);
	await sleep(secs * 1000);
	const r = s.stop();
	return { gpuBusyPct: r.gpuBusyPct, gpuW: r.gpuW, cpuPackageW: r.cpuPackageW };
}

const delta = (m, ref) => ({
	...m,
	cpuPackageWAboveIdle: r2(m.cpuPackageW - ref.cpuPackageW),
	gpuWAboveIdle: m.gpuW == null ? null : r2(m.gpuW - ref.gpuW)
});

async function startup() {
	const out = { warmMs: [] };
	const runs = QUICK ? 2 : 6;
	for (let i = 0; i < runs; i++) {
		const cold = COLD && i === 0;
		if (cold) {
			// sudo asks on this program's input (pipe the password in, or have sudo cached)
			execFileSync(
				'sudo',
				['-S', '-p', '', 'sh', '-c', 'sync; echo 3 > /proc/sys/vm/drop_caches'],
				{
					stdio: ['inherit', 'ignore', 'inherit']
				}
			);
			await sleep(1000);
		}
		const t0 = performance.now();
		const app = await App.launch();
		const t = await app.until(homeReady);
		const ms = r2(t - t0);
		if (cold) out.coldMs = ms;
		else if (i > 0 || !COLD) out.warmMs.push(ms);
		log(`startup ${cold ? 'cold' : 'warm'}: ${ms} ms`);
		if (i === runs - 1) {
			await sleep(3000);
			out.homePssMB = r2(tree(app.pid).reduce((a, p) => a + pssKB(p.pid), 0) / 1024);
			out.processes = tree(app.pid).map((p) => p.comm);
		}
		await app.quit();
	}
	out.warm = dist(out.warmMs);
	return out;
}

async function openStudio(app, query, frames) {
	await app.run(instrument);
	const t0 = performance.now();
	await app.run((q) => location.assign(`/studio?${q}`), query);
	await sleep(50);
	const t = await app.until(studioReady, 180_000, frames);
	await app.run(instrument);
	return r2(t - t0);
}

/** open a project from the home screen's Recent list, as a person does; ms to ready */
async function openRecent(app, file, frames) {
	await app.run(instrument);
	const t0 = performance.now();
	const ok = await app.run((f) => {
		const b = [...document.querySelectorAll('button.card')].find((x) => x.title === f);
		b?.click();
		return !!b;
	}, file);
	if (!ok) throw new Error(`${file} is not on the Recent list`);
	await sleep(50);
	const t = await app.until(studioReady, 180_000, frames);
	await app.run(instrument);
	return r2(t - t0);
}

/** the native frame store's counts (how many drawings it holds) */
const frameStats = (app) => app.runAsync(() => window.__TAURI_INTERNALS__.invoke('frames_stats'));

async function idle(app, ref, secs) {
	await sleep(5000); // let it settle
	await app.run(() => ((window.__ppb.requests = 0), true));
	const s = sampler(app.pid);
	await sleep(secs * 1000);
	const m = s.stop();
	const requests = await app.run(() => window.__ppb.requests);
	return {
		...delta(m, ref),
		appFrameRequests: requests,
		appFramesPerSecond: r2(requests / m.seconds)
	};
}

async function draw(app, ref, cfg, shot) {
	const s = sampler(app.pid);
	const r = await app.runAsync(penStrokes, cfg);
	const m = s.stop();
	await app
		.screenshot(path.join(path.dirname(OUT), shot))
		.catch((e) => log('no screenshot:', e.message));
	const vsync = pct(r.intervals, 50);
	return {
		...delta(m, ref),
		samples: r.drawn.length,
		penToDrawnMs: dist(r.drawn),
		penToScreenMs: dist(r.shown),
		frameMs: dist(r.intervals),
		framesOver1_5Vsync: r.intervals.filter((x) => x > vsync * 1.5).length,
		frames: r.intervals.length
	};
}

async function playback(app, ref, secs) {
	const s = sampler(app.pid);
	const r = await app.runAsync(playFor, secs);
	const m = s.stop();
	if (!r.startMs) return { error: 'playback never started', ...delta(m, ref) };
	const ch = r.changes;
	let skipped = 0;
	for (let k = 1; k < ch.length; k++) {
		const step = (ch[k][1] - ch[k - 1][1] + r.length) % r.length;
		if (step > 1) skipped += step - 1;
	}
	const span = (ch.at(-1)[0] - ch[0][0]) / 1000;
	const shownGaps = ch.slice(1).map((c, k) => c[0] - ch[k][0]);
	return {
		...delta(m, ref),
		startMs: r2(r.startMs),
		framesShown: ch.length,
		expectedAt24: Math.round(span * 24) + 1,
		framesSkipped: skipped,
		shownFps: r2((ch.length - 1) / span),
		frameChangeGapMs: dist(shownGaps),
		vsyncIntervalMs: dist(r.intervals)
	};
}

async function main() {
	headless();
	await sleep(1500);
	startDriver();
	await sleep(1000);
	const idleSecs = QUICK ? 10 : 30;
	const ref = await systemIdle('before', QUICK ? 10 : 20);
	report.results.machineIdle = ref;

	report.results.startup = await startup();

	// an empty canvas: idle, drawing
	{
		const app = await App.launch();
		await app.until(homeReady);
		await app.maximise();
		const openMs = await openStudio(app, `new=1920x1080&fps=24`);
		const mem = tree(app.pid).reduce((a, p) => a + pssKB(p.pid), 0) / 1024;
		log(`empty canvas: ${r2(mem)} MB, idling ${idleSecs} s`);
		report.results.empty = { openMs, pssMB: r2(mem), idle: await idle(app, ref, idleSecs) };
		log('drawing on the empty canvas');
		report.results.empty.draw = await draw(
			app,
			ref,
			{ hz: 240, strokes: QUICK ? 3 : 10, strokeSeconds: 1 },
			'draw-empty.png'
		);
		await app.quit();
	}

	// 20 layers at 4K: open, idle, draw, play
	{
		const app = await App.launch([project('layers20-4k')]);
		await app.until(homeReady);
		await app.maximise();
		log('opening 20 layers at 4K');
		const openMs = await openRecent(app, project('layers20-4k'), 24);
		await sleep(3000);
		const mem = tree(app.pid).reduce((a, p) => a + pssKB(p.pid), 0) / 1024;
		await app.screenshot(path.join(path.dirname(OUT), 'open-4k.png'));
		report.results.layers20_4k = { openMs, pssMB: r2(mem), store: await frameStats(app) };
		log(`opened in ${openMs} ms, ${r2(mem)} MB; idling`);
		report.results.layers20_4k.idle = await idle(app, ref, idleSecs);
		log('drawing on 20 layers at 4K');
		report.results.layers20_4k.draw = await draw(
			app,
			ref,
			{ hz: 240, strokes: QUICK ? 3 : 10, strokeSeconds: 1 },
			'draw-4k.png'
		);
		log('playing 20 layers at 4K');
		report.results.layers20_4k.play = await playback(app, ref, QUICK ? 5 : 15);
		report.results.layers20_4k.pssAfterMB = r2(
			tree(app.pid).reduce((a, p) => a + pssKB(p.pid), 0) / 1024
		);
		await app.quit();
	}

	// 500 drawings: open, memory, play
	{
		const app = await App.launch([project('drawings500')]);
		await app.until(homeReady);
		await app.maximise();
		log('opening 500 drawings');
		const s = sampler(app.pid);
		const openMs = await openRecent(app, project('drawings500'), 100);
		const m = s.stop();
		await sleep(3000);
		const mem = tree(app.pid).reduce((a, p) => a + pssKB(p.pid), 0) / 1024;
		await app.screenshot(path.join(path.dirname(OUT), 'open-500.png'));
		report.results.drawings500 = {
			openMs,
			pssMB: r2(mem),
			openPeakPssMB: m.pssMaxMB,
			store: await frameStats(app)
		};
		log(`opened in ${openMs} ms, ${r2(mem)} MB; playing`);
		report.results.drawings500.play = await playback(app, ref, QUICK ? 5 : 10);
		report.results.drawings500.pssAfterMB = r2(
			tree(app.pid).reduce((a, p) => a + pssKB(p.pid), 0) / 1024
		);
		await app.quit();
	}

	report.results.machineIdleAfter = await systemIdle('after', QUICK ? 10 : 20);
}

const stopAll = () => {
	for (const c of children) {
		try {
			c.kill('SIGTERM');
		} catch {
			/* gone */
		}
	}
};
process.on('SIGINT', () => (stopAll(), process.exit(130)));

main()
	.then(() => {
		fs.writeFileSync(OUT, JSON.stringify(report, null, 2));
		log(`written ${OUT}`);
	})
	.catch((e) => {
		report.error = String(e.stack ?? e);
		fs.writeFileSync(OUT, JSON.stringify(report, null, 2));
		log('failed:', e);
		process.exitCode = 1;
	})
	.finally(stopAll);
