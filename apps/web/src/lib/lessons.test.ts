import { describe, expect, it } from 'vitest';
import {
	DRILL_IDS,
	LESSONS,
	LESSON_SOURCES,
	SKILLS,
	lessonById,
	lessonForRule,
	lessonsBySkill,
	parseFrontmatter,
	parseLesson,
	recommendedLessons,
	sanitizeHtml
} from './lessons';
import { rules as RULES } from './rules';
import type { Attempt } from './progress';
import m8Plan from '../../../../docs/planning/M8_CONTENT_PLAN.md?raw';

/**
 * Drill ids fixed by the M8 content plan (the `drill` column of its lesson table). Lessons may
 * link to them before the drills and rules that use them land, since the streams merge separately.
 */
const PLANNED_DRILL_IDS = m8Plan
	.split('\n')
	.map(
		(line) =>
			/^\|\s*[a-z0-9-]+\s*\|[^|]*\|[^|]*\|\s*([a-z0-9]+(?:-[a-z0-9]+)*)\s*\|$/.exec(
				line.trim()
			)?.[1]
	)
	.filter((id): id is string => !!id && id !== 'drill');
const KNOWN_DRILL_IDS = [...new Set([...DRILL_IDS, ...PLANNED_DRILL_IDS])];

describe('lesson content', () => {
	it('has a lesson file for every lesson id in rules.toml', () => {
		const missing = [...new Set(RULES.map((r) => r.lesson))].filter((id) => !lessonById(id));
		expect(missing).toEqual([]);
	});

	it('loads all 30 lessons', () => {
		expect(LESSONS).toHaveLength(30);
	});

	it('reads the planned drill ids from the M8 content plan', () => {
		expect(PLANNED_DRILL_IDS).toHaveLength(18);
		expect(PLANNED_DRILL_IDS).toContain('gesture-30-second');
	});

	for (const lesson of LESSONS) {
		describe(lesson.id, () => {
			it('links to a real drill and next lesson', () => {
				expect(KNOWN_DRILL_IDS).toContain(lesson.drill);
				expect(lessonById(lesson.next), `next: ${lesson.next}`).toBeDefined();
				expect(lesson.next).not.toBe(lesson.id);
			});

			it('has complete frontmatter', () => {
				expect(SKILLS.map((s) => s.id)).toContain(lesson.skill);
				expect(lesson.learn).toHaveLength(3);
				expect(lesson.summary.length).toBeGreaterThan(10);
				expect(lesson.minutes).toBeGreaterThanOrEqual(1);
			});

			it('has 3–5 steps, each with text', () => {
				expect(lesson.steps.length).toBeGreaterThanOrEqual(3);
				expect(lesson.steps.length).toBeLessThanOrEqual(5);
				for (const s of lesson.steps) {
					expect(s.title).not.toBe('');
					expect(s.html).toContain('<p>');
				}
			});

			it('has at least one accessible SVG diagram', () => {
				const diagrams = lesson.steps.flatMap((s) => s.diagrams);
				expect(diagrams.length).toBeGreaterThanOrEqual(1);
				for (const d of diagrams) {
					expect(d).toMatch(/role="img"/);
					expect(d).toMatch(/aria-label="[^"]{10,}"/);
				}
			});

			it('contains no scripts or handlers', () => {
				const src = LESSON_SOURCES[lesson.id];
				expect(src).not.toMatch(/<script|\son[a-z]+\s*=|javascript:/i);
			});
		});
	}

	it('has every lesson reachable through `next`', () => {
		const seen = new Set<string>();
		let id = LESSONS[0].id;
		while (!seen.has(id)) {
			seen.add(id);
			id = lessonById(id)!.next;
		}
		expect(seen.size).toBe(LESSONS.length);
	});

	it('groups lessons by skill in SKILLS order', () => {
		const groups = lessonsBySkill();
		expect(groups.flatMap((g) => g.lessons)).toHaveLength(LESSONS.length);
		expect(groups.map((g) => g.id)).toEqual(
			SKILLS.map((s) => s.id).filter((id) => groups.some((g) => g.id === id))
		);
	});
});

describe('parseFrontmatter', () => {
	it('parses scalars, quotes, numbers and lists', () => {
		const { data, body } = parseFrontmatter(
			[
				'---',
				'title: "Colons: fine here"',
				"quote: 'it''s ok'",
				'minutes: 3',
				'half: 2.5',
				'flag: true',
				'plain: a # comment',
				'hash: a#b',
				'learn:',
				'  - one',
				'  - "two, with comma"',
				'- three',
				'tags: [a, "b, c", \'d\']',
				'empty: []',
				'---',
				'# Body'
			].join('\n')
		);
		expect(data).toEqual({
			title: 'Colons: fine here',
			quote: "it's ok",
			minutes: 3,
			half: 2.5,
			flag: true,
			plain: 'a',
			hash: 'a#b',
			learn: ['one', 'two, with comma', 'three'],
			tags: ['a', 'b, c', 'd'],
			empty: []
		});
		expect(body).toBe('# Body');
	});

	it('handles CRLF line endings and a BOM', () => {
		const { data, body } = parseFrontmatter('\uFEFF---\r\ntitle: Hi\r\n---\r\nText\r\n');
		expect(data).toEqual({ title: 'Hi' });
		expect(body).toBe('Text\n');
	});

	it('returns the whole text as body when there is no frontmatter', () => {
		expect(parseFrontmatter('Just text\n---\n')).toEqual({ data: {}, body: 'Just text\n---\n' });
	});

	it('accepts empty frontmatter and blank lines', () => {
		expect(parseFrontmatter('---\n---\nx').data).toEqual({});
		expect(parseFrontmatter('---\n\na: 1\n\n---\n').data).toEqual({ a: 1 });
	});

	it('keeps --- inside the body', () => {
		expect(parseFrontmatter('---\na: 1\n---\nx\n---\ny').body).toBe('x\n---\ny');
	});

	it('rejects malformed input', () => {
		expect(() => parseFrontmatter('---\n- orphan\n---\n')).toThrow(/list item without a key/);
		expect(() => parseFrontmatter('---\nno colon here\n---\n')).toThrow(/key: value/);
		expect(() => parseFrontmatter('---\na: 1\na: 2\n---\n')).toThrow(/duplicate/);
		expect(() => parseFrontmatter('---\na: "open\n---\n')).toThrow(/unterminated/);
		expect(() => parseFrontmatter('---\na: [x, y\n---\n')).toThrow(/unterminated/);
	});
});

describe('parseLesson', () => {
	const src = (extra = '') =>
		`---\ntitle: T\nskill: tone\nminutes: 2\nsummary: S\nlearn:\n  - a\ndrill: d\nnext: n\n${extra}---\n`;

	it('splits steps and pulls out diagrams', () => {
		const l = parseLesson(
			'x',
			src() +
				'\n## One\n\nHello **there**.\n\n<svg role="img" aria-label="pic"><path d="M0 0" /></svg>\n\nMore.\n\n## Two\n\nBye.\n'
		);
		expect(l.steps.map((s) => s.title)).toEqual(['One', 'Two']);
		expect(l.steps[0].diagrams).toEqual([
			'<svg role="img" aria-label="pic"><path d="M0 0" /></svg>'
		]);
		expect(l.steps[0].html).toContain('<strong>there</strong>');
		expect(l.steps[0].html).not.toContain('<svg');
		expect(l.steps[1].diagrams).toEqual([]);
	});

	it('rejects missing fields and unknown skills', () => {
		expect(() => parseLesson('x', src().replace('title: T\n', ''))).toThrow(/title/);
		expect(() => parseLesson('x', src().replace('skill: tone', 'skill: juggling'))).toThrow(
			/unknown skill/
		);
		expect(() => parseLesson('x', src() + 'Intro text\n## One\nHi\n')).toThrow(/before the first/);
	});
});

describe('sanitizeHtml', () => {
	it('removes scripts, handlers and javascript: links', () => {
		const out = sanitizeHtml(
			'<p onclick="x()">a</p><script>alert(1)</script><a href="javascript:alert(1)">b</a><svg onload=x><foreignObject>y</foreignObject></svg>'
		);
		expect(out).toBe('<p>a</p><a href="#">b</a><svg></svg>');
	});
});

describe('rules → lessons', () => {
	it('maps a rule id to its lesson', () => {
		expect(lessonForRule('too-big')?.id).toBe('measuring-with-units');
		expect(lessonForRule('angles-off')?.id).toBe('finding-angles');
		expect(lessonForRule('nope')).toBeUndefined();
		expect(lessonForRule(undefined)).toBeUndefined();
	});
});

describe('history', () => {
	const at = (fixes?: string[]): Attempt => ({
		category: 'animals',
		topic: null,
		tier: 1,
		score: 5,
		at: 0,
		...(fixes ? { fixes } : {})
	});

	it('recommends the most frequent lessons, ties to the most recent', () => {
		const h = [
			at(['shaky-lines']),
			at(['too-big', 'proportions-off']), // same lesson twice counts once
			at(['too-small', 'flat-shading']),
			at(['angles-off']),
			at(['flat-shading'])
		];
		const recs = recommendedLessons(h);
		expect(recs.map((r) => [r.lesson.id, r.count])).toEqual([
			['value-range', 2],
			['measuring-with-units', 2],
			['finding-angles', 1]
		]);
		expect(recs[0].of).toBe(5);
		expect(recs[0].ruleId).toBe('flat-shading');
		expect(recommendedLessons([at(), at(['unknown-rule'])])).toEqual([]);
	});
});
