import { describe, expect, it } from 'vitest';
import {
	DEFAULT_TEXT,
	boxCorners,
	cssFamily,
	cssFont,
	familyFromFile,
	fontsUsed,
	inBox,
	layoutText,
	parseText,
	textJson,
	type TextSpec
} from './text';

/** every letter 10 px wide */
const mono = (t: string) => [...t].length * 10;
const spec = (o: Partial<TextSpec>): TextSpec => ({
	...DEFAULT_TEXT,
	size: 10,
	lineHeight: 2,
	...o
});

describe('text specs', () => {
	it('round-trip through JSON', () => {
		const s = spec({ text: 'Hi\nthere', font: 'Georgia', bold: true, align: 'center', rotate: 15 });
		expect(parseText(textJson(s))).toEqual(s);
	});

	it('fill in what is missing and keep values in bounds', () => {
		expect(parseText('{}')).toEqual(DEFAULT_TEXT);
		const s = parseText(
			'{"text":5,"size":99999,"color":"red","align":"middle","lineHeight":-1,"font":"  "}'
		)!;
		expect(s.text).toBe('');
		expect(s.size).toBe(1000);
		expect(s.color).toBe(DEFAULT_TEXT.color);
		expect(s.align).toBe('left');
		expect(s.lineHeight).toBe(0.5);
		expect(s.font).toBe('sans-serif');
	});

	it('are null when they are not text', () => {
		expect(parseText(null)).toBeNull();
		expect(parseText('junk')).toBeNull();
		expect(parseText('[1]')).toBeNull();
		expect(parseText('"text"')).toBeNull();
	});
});

describe('fonts', () => {
	it('quote families (generic ones stay bare) with a fallback', () => {
		expect(cssFamily('serif')).toBe('serif');
		expect(cssFamily('Times New Roman')).toBe('"Times New Roman", sans-serif');
		expect(cssFamily('Bad"Name')).toBe('"BadName", sans-serif');
		expect(cssFont(spec({ bold: true, italic: true, size: 30, font: 'serif' }))).toBe(
			'italic bold 30px serif'
		);
	});

	it('take their family from the file name', () => {
		expect(familyFromFile('/fonts/My_Hand.ttf')).toBe('My Hand');
		expect(familyFromFile('C:\\x\\Lobster-Regular.woff2')).toBe('Lobster-Regular');
		expect(familyFromFile('.otf')).toBe('Imported font');
	});

	it('kept with a project are only the imported ones used', () => {
		expect(fontsUsed([spec({ font: 'serif' }), null])).toEqual([]);
	});
});

describe('layout', () => {
	it('keeps lines as typed without a width', () => {
		const l = layoutText(spec({ text: 'ab\nabcd' }), mono);
		expect(l.lines.map((x) => x.text)).toEqual(['ab', 'abcd']);
		expect(l.width).toBe(40);
		expect(l.height).toBe(40);
		expect(l.lines[1].y).toBe(20);
	});

	it('wraps words at the width, dropping the spaces at the break', () => {
		const l = layoutText(spec({ text: 'one two three', width: 75 }), mono);
		expect(l.lines.map((x) => x.text)).toEqual(['one two', 'three']);
		expect(l.width).toBe(75);
	});

	it('cuts a word longer than the width', () => {
		const l = layoutText(spec({ text: 'abcdefghij xy', width: 40 }), mono);
		expect(l.lines.map((x) => x.text)).toEqual(['abcd', 'efgh', 'ij', 'xy']);
	});

	it('aligns lines left, centre or right', () => {
		const at = (align: TextSpec['align']) =>
			layoutText(spec({ text: 'abcd\nab', align }), mono).lines[1].x;
		expect([at('left'), at('center'), at('right')]).toEqual([0, 10, 20]);
	});

	it('counts letter spacing in the widths', () => {
		const l = layoutText(spec({ text: 'abc', letterSpacing: 5 }), mono);
		expect(l.width).toBe(45);
	});

	it('has a line even when empty', () => {
		const l = layoutText(spec({ text: '' }), mono);
		expect(l.lines).toHaveLength(1);
		expect(l.height).toBe(20);
	});
});

describe('the box', () => {
	it('turns around its centre', () => {
		const s = spec({ x: 10, y: 20, rotate: 90 });
		const [tl] = boxCorners(s, 40, 20);
		// centre (30, 30); the top-left corner (-20, -10) from it turns to (10, -20)
		expect(tl[0]).toBeCloseTo(40);
		expect(tl[1]).toBeCloseTo(10);
	});

	it('knows what is inside it, turned or not', () => {
		const s = spec({ x: 0, y: 0 });
		expect(inBox(s, 100, 20, 50, 10)).toBe(true);
		expect(inBox(s, 100, 20, 50, 30)).toBe(false);
		expect(inBox(s, 100, 20, 50, 30, 12)).toBe(true);
		const turned = { ...s, rotate: 90 };
		expect(inBox(turned, 100, 20, 50, 50)).toBe(true);
		expect(inBox(turned, 100, 20, 5, 10)).toBe(false);
	});
});
