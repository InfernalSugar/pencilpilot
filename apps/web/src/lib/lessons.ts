/**
 * Lessons (content/lessons/*.md), parsed at build time, plus the links between
 * feedback rules (content/feedback/rules.toml) and lessons.
 *
 * Lesson format: YAML-like frontmatter (see `parseFrontmatter`), then 3–5 steps,
 * each `## Step title` followed by Markdown. Inline `<svg>` blocks are pulled out
 * of each step as its diagrams. Content is ours and trusted, but scripts, event
 * handlers and `javascript:` URLs are still stripped.
 */
import { marked } from 'marked';
import { ruleById, rules } from '#lib/rules.js';
import type { Attempt } from '#lib/progress.js';

export const SKILLS = [
	{
		id: 'placement',
		label: 'Size & placement',
		blurb: 'Land the drawing at the right size, in the right spot.'
	},
	{
		id: 'proportions',
		label: 'Proportions & angles',
		blurb: 'Measure, compare and get the tilt right.'
	},
	{
		id: 'silhouette',
		label: 'Big shapes',
		blurb: 'Get the overall mass right before the details.'
	},
	{
		id: 'outline',
		label: 'Outlines',
		blurb: 'Every part on the page, every edge where it belongs.'
	},
	{ id: 'tone', label: 'Light & dark', blurb: 'Values that make a drawing look solid.' },
	{
		id: 'line_confidence',
		label: 'Line confidence',
		blurb: 'Smooth, clean lines that read clearly.'
	}
] as const;

export type Skill = (typeof SKILLS)[number]['id'];

export const skillLabel = (skill: string) => SKILLS.find((s) => s.id === skill)?.label ?? skill;

export interface LessonStep {
	title: string;
	/** Rendered HTML of the step text, without its diagrams. */
	html: string;
	/** Inline SVG diagrams that appeared in this step, in order. */
	diagrams: string[];
}

export interface Lesson {
	id: string;
	title: string;
	skill: Skill;
	minutes: number;
	summary: string;
	learn: string[];
	drill: string;
	next: string;
	steps: LessonStep[];
}

// ── Frontmatter ─────────────────────────────────────────────────────

export type FrontValue = string | number | boolean | string[];

/**
 * A tiny frontmatter parser for the subset lessons use:
 * `key: value` (plain, "double" or 'single' quoted; numbers and true/false are
 * converted), `key: [a, "b, c"]` inline lists, and block lists
 * (`key:` followed by `  - item` lines). `#` starts a comment outside quotes.
 * Returns empty data when there is no frontmatter. Throws on malformed lines.
 */
export function parseFrontmatter(src: string): { data: Record<string, FrontValue>; body: string } {
	const text = src.replace(/^\uFEFF/, '').replace(/\r\n?/g, '\n');
	const m = /^---[ \t]*\n([\s\S]*?)\n?---[ \t]*(?:\n|$)/.exec(text);
	if (!m) return { data: {}, body: text };

	const data: Record<string, FrontValue> = {};
	let listKey: string | null = null;
	const lines = m[1].split('\n');
	lines.forEach((raw, i) => {
		const where = `frontmatter line ${i + 2}`;
		const line = stripComment(raw).trimEnd();
		if (!line.trim()) return;
		const item = /^\s*-(?:\s+(.*))?$/.exec(line);
		if (item) {
			if (!listKey) throw new Error(`${where}: list item without a key`);
			(data[listKey] as string[]).push(String(scalar(item[1] ?? '', where)));
			return;
		}
		const kv = /^([A-Za-z_][\w-]*)[ \t]*:(?:[ \t]+(.*))?$/.exec(line);
		if (!kv || /^\s/.test(line)) throw new Error(`${where}: expected "key: value"`);
		const [, key, value = ''] = kv;
		if (key in data) throw new Error(`${where}: duplicate key "${key}"`);
		if (!value.trim()) {
			data[key] = [];
			listKey = key;
		} else {
			data[key] = value.trim().startsWith('[')
				? inlineList(value.trim(), where)
				: scalar(value, where);
			listKey = null;
		}
	});
	return { data, body: text.slice(m[0].length) };
}

/** Remove a `#` comment that is outside quotes (and preceded by whitespace or at the start). */
function stripComment(line: string): string {
	let quote = '';
	for (let i = 0; i < line.length; i++) {
		const c = line[i];
		if (quote) {
			if (c === '\\' && quote === '"') i++;
			else if (c === quote) quote = '';
		} else if (c === '"' || c === "'") quote = c;
		else if (c === '#' && (i === 0 || /\s/.test(line[i - 1]))) return line.slice(0, i);
	}
	return line;
}

function scalar(raw: string, where: string): string | number | boolean {
	const v = raw.trim();
	if (v.startsWith('"')) {
		if (!/^"(?:[^"\\]|\\.)*"$/.test(v)) throw new Error(`${where}: unterminated "string"`);
		return v.slice(1, -1).replace(/\\(.)/g, (_, c: string) => (c === 'n' ? '\n' : c));
	}
	if (v.startsWith("'")) {
		if (!/^'(?:[^']|'')*'$/.test(v)) throw new Error(`${where}: unterminated 'string'`);
		return v.slice(1, -1).replace(/''/g, "'");
	}
	if (/^-?\d+(?:\.\d+)?$/.test(v)) return Number(v);
	if (v === 'true' || v === 'false') return v === 'true';
	return v;
}

function inlineList(raw: string, where: string): string[] {
	if (!raw.endsWith(']')) throw new Error(`${where}: unterminated [list]`);
	const inner = raw.slice(1, -1);
	const out: string[] = [];
	let cur = '';
	let quote = '';
	for (let i = 0; i < inner.length; i++) {
		const c = inner[i];
		if (quote) {
			if (c === '\\' && quote === '"') cur += c + (inner[++i] ?? '');
			else {
				if (c === quote) quote = '';
				cur += c;
			}
		} else if (c === '"' || c === "'") {
			quote = c;
			cur += c;
		} else if (c === ',') {
			out.push(cur);
			cur = '';
		} else cur += c;
	}
	if (quote) throw new Error(`${where}: unterminated string in [list]`);
	if (cur.trim() || out.length) out.push(cur);
	return out.map((s) => String(scalar(s, where))).filter((s) => s !== '');
}

// ── Markdown ────────────────────────────────────────────────────────

/** Strip anything executable from trusted-but-careful HTML. */
export function sanitizeHtml(html: string): string {
	return html
		.replace(/<(script|style|iframe|object|embed|foreignObject)\b[\s\S]*?<\/\1\s*>/gi, '')
		.replace(/<\/?(script|style|iframe|object|embed|foreignObject)\b[^>]*>/gi, '')
		.replace(/\s+on[a-z]+\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/gi, '')
		.replace(/(href|src|xlink:href)\s*=\s*(["']?)\s*javascript:[^"'>\s]*\2/gi, '$1="#"');
}

export function renderMarkdown(md: string): string {
	return sanitizeHtml(marked.parse(md, { async: false, gfm: true }) as string);
}

const SVG_RE = /<svg\b[\s\S]*?<\/svg>/gi;

/** Split a lesson body into `## ` steps and pull out each step's inline SVG diagrams. */
export function parseSteps(body: string): LessonStep[] {
	const parts = body.split(/^##[ \t]+/m);
	if (parts[0].trim()) throw new Error('text before the first "## Step" heading');
	return parts.slice(1).map((part) => {
		const nl = part.indexOf('\n');
		const title = (nl < 0 ? part : part.slice(0, nl)).trim();
		const rest = nl < 0 ? '' : part.slice(nl + 1);
		const diagrams = (rest.match(SVG_RE) ?? []).map(sanitizeHtml);
		return { title, html: renderMarkdown(rest.replace(SVG_RE, '')).trim(), diagrams };
	});
}

const req = (data: Record<string, FrontValue>, key: string, id: string) => {
	const v = data[key];
	if (v === undefined || v === '' || (Array.isArray(v) && !v.length))
		throw new Error(`lesson ${id}: missing "${key}"`);
	return v;
};

export function parseLesson(id: string, src: string): Lesson {
	const { data, body } = parseFrontmatter(src);
	const skill = String(req(data, 'skill', id));
	if (!SKILLS.some((s) => s.id === skill))
		throw new Error(`lesson ${id}: unknown skill "${skill}"`);
	const learn = req(data, 'learn', id);
	if (!Array.isArray(learn)) throw new Error(`lesson ${id}: "learn" must be a list`);
	const minutes = Number(req(data, 'minutes', id));
	if (!Number.isFinite(minutes) || minutes <= 0)
		throw new Error(`lesson ${id}: "minutes" must be a number`);
	return {
		id,
		title: String(req(data, 'title', id)),
		skill: skill as Skill,
		minutes,
		summary: String(req(data, 'summary', id)),
		learn,
		drill: String(req(data, 'drill', id)),
		next: String(req(data, 'next', id)),
		steps: parseSteps(body)
	};
}

// ── The lesson library ──────────────────────────────────────────────

const files = import.meta.glob<string>('../../../../content/lessons/*.md', {
	query: '?raw',
	import: 'default',
	eager: true
});

/** Raw lesson sources by id (for tests). */
export const LESSON_SOURCES: Record<string, string> = Object.fromEntries(
	Object.entries(files).map(([path, src]) => [path.split('/').pop()!.replace(/\.md$/, ''), src])
);

export const LESSONS: Lesson[] = Object.entries(LESSON_SOURCES)
	.map(([id, src]) => parseLesson(id, src))
	.sort((a, b) => a.title.localeCompare(b.title));

export const lessonById = (id: string | null | undefined) => LESSONS.find((l) => l.id === id);

/** Lessons grouped by skill, in SKILLS order; empty groups are left out. */
export function lessonsBySkill() {
	return SKILLS.map((s) => ({ ...s, lessons: LESSONS.filter((l) => l.skill === s.id) })).filter(
		(g) => g.lessons.length
	);
}

// ── Rules ↔ lessons ─────────────────────────────────────────────────

/** Every drill id a rule links to. */
export const DRILL_IDS = [...new Set(rules.flatMap((r) => (r.drill ? [r.drill] : [])))];

/** The lesson a feedback rule (fix id) links to. */
export const lessonForRule = (ruleId: string | null | undefined) =>
	ruleId ? lessonById(ruleById(ruleId)?.lesson) : undefined;

// ── History ─────────────────────────────────────────────────────────

export interface Recommendation {
	lesson: Lesson;
	/** The rule that pointed here most often (for the `?from=` link). */
	ruleId: string;
	/** In how many of the last `of` drawings it came up. */
	count: number;
	of: number;
}

/**
 * Lessons to recommend from recent feedback: each lesson scores one point per
 * recent drawing whose fixes point to it (attempts saved without `fixes` are
 * skipped); ties go to the most recent.
 */
export function recommendedLessons(
	history: readonly Attempt[],
	{ window = 10, limit = 3 } = {}
): Recommendation[] {
	const recent = history.flatMap((a) => (Array.isArray(a.fixes) ? [a.fixes] : [])).slice(-window);
	const tally = new Map<string, { count: number; last: number; rules: Map<string, number> }>();
	recent.forEach((fixes, i) => {
		const seen = new Set<string>();
		for (const ruleId of fixes) {
			const lesson = lessonForRule(ruleId);
			if (!lesson) continue;
			const t = tally.get(lesson.id) ?? { count: 0, last: 0, rules: new Map() };
			if (!seen.has(lesson.id)) {
				seen.add(lesson.id);
				t.count++;
				t.last = i;
			}
			t.rules.set(ruleId, (t.rules.get(ruleId) ?? 0) + 1);
			tally.set(lesson.id, t);
		}
	});
	return [...tally.entries()]
		.sort(([, a], [, b]) => b.count - a.count || b.last - a.last)
		.slice(0, limit)
		.map(([id, t]) => ({
			lesson: lessonById(id)!,
			ruleId: [...t.rules.entries()].sort((a, b) => b[1] - a[1])[0][0],
			count: t.count,
			of: recent.length
		}));
}
