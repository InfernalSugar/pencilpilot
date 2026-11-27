// Renders the design's Design Component pages to PNG with headless Chromium (no window).
//
//   node tools/native-qa/render-design.mjs <design-project-dir> <runtime.js> <out-dir>
//
// The pages load `./support.js` (the design tool's runtime); it is served from <runtime.js>.
// Playwright comes from PLAYWRIGHT_DIR (a node_modules folder holding @playwright/test),
// by default the studio app's.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';

const [project, runtime, out] = process.argv.slice(2);
if (!project || !runtime || !out) {
  console.error('usage: render-design.mjs <design-project-dir> <runtime.js> <out-dir>');
  process.exit(2);
}
const here = path.dirname(new URL(import.meta.url).pathname);
const pwDir = process.env.PLAYWRIGHT_DIR
  ?? path.resolve(here, '../../../pencilpilot/apps/studio/node_modules');
const require = createRequire(path.join(pwDir, 'x.js'));
const { chromium } = require('@playwright/test');

const types = { '.html': 'text/html', '.js': 'text/javascript', '.json': 'application/json', '.css': 'text/css' };
const server = http.createServer((req, res) => {
  const p = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const file = p === '/support.js' ? runtime : path.join(project, path.normalize(p).replace(/^(\.\.[/\\])+/, ''));
  fs.readFile(file, (err, data) => {
    if (err) { res.writeHead(404); res.end(); return; }
    res.writeHead(200, { 'content-type': types[path.extname(file)] ?? 'application/octet-stream' });
    res.end(data);
  });
});
await new Promise(r => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}`;

const pages = [
  ['animate', 'FullApp.dc.html'],
  ['draw', 'FullAppDraw.dc.html'],
  ['colour', 'FullAppColour.dc.html'],
  ['review', 'FullAppReview.dc.html'],
  ['timeline-split', 'TimelineSplit.dc.html'],
  ['tray-demo', 'TrayDemo.dc.html'],
  ['timeline-menus', 'TimelineMenus.dc.html'],
  ['other-menus', 'OtherMenus.dc.html'],
  ['timeline-closeup', 'TimelineCloseup.dc.html'],
];
fs.mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ headless: true });
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
let failed = 0;
for (const [name, file] of pages) {
  if (!fs.existsSync(path.join(project, file))) { console.log(`${name}: no ${file}, skipped`); continue; }
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', e => errors.push(String(e)));
  await page.goto(`${base}/${file}`, { waitUntil: 'networkidle', timeout: 30000 }).catch(e => errors.push(String(e)));
  await page.evaluate(() => document.fonts && document.fonts.ready).catch(() => {});
  await page.waitForTimeout(800);
  await page.screenshot({ path: path.join(out, `design-${name}.png`) });
  // the regions' boxes and colours, as the browser laid them out (FullApp pages only)
  const boxes = await page.evaluate(() => {
    const pick = el => {
      const r = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      return {
        x: r.x, y: r.y, w: r.width, h: r.height,
        bg: cs.backgroundColor, radius: parseFloat(cs.borderTopLeftRadius) || 0,
        label: el.getAttribute('aria-label') || (el.textContent || '').trim().slice(0, 40),
      };
    };
    const one = sel => { const el = document.querySelector(sel); return el ? pick(el) : null; };
    const all = sel => [...document.querySelectorAll(sel)].map(pick);
    return {
      regions: {
        header: one('header'),
        tools: one('nav[aria-label="Tools"]'),
        canvas: one('main'),
        panel: one('aside[aria-label="Side panel"]'),
        strip: one('section[aria-label="Timeline"], section[aria-label="Frame strip"], section[aria-label="Drawings to colour"], section[aria-label="Review player"]'),
      },
      strip: one('section[aria-label]')?.label ?? null,
      workspaceTabs: all('[role="tablist"][aria-label="Workspace"] [role="tab"]'),
      panelTabs: all('[role="tablist"][aria-label="Panel"] [role="tab"]'),
      toolButtons: all('nav[aria-label="Tools"] button'),
      headerButtons: all('header button'),
      stripButtons: all('section[aria-label] button'),
      menus: all('[role="menu"]'),
      fontsLoaded: [...new Set([...document.fonts].filter(f => f.status === 'loaded').map(f => f.family))],
    };
  }).catch(e => ({ evalError: String(e) }));
  fs.writeFileSync(path.join(out, `design-${name}.json`), JSON.stringify({ errors, ...boxes }, null, 2));
  console.log(`${name}: ${errors.length ? 'errors: ' + errors.join(' | ') : 'ok'}`);
  if (errors.length) failed++;
  await page.close();
}
await browser.close();
server.close();
process.exit(failed ? 1 : 0);
