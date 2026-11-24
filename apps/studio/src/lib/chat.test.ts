import { describe, expect, it } from 'vitest';
import { capLines, chatBody, chatText, KEEP_CHAT, MAX_CHAT_CHARS } from './chat';

describe('chat', () => {
	it('reads only well-formed chat bodies', () => {
		expect(chatText({ k: 'chat', text: ' hi ' })).toBe('hi');
		expect(chatText({ k: 'snap.req' })).toBeNull();
		expect(chatText({ k: 'chat', text: 5 })).toBeNull();
		expect(chatText({ k: 'chat', text: '   ' })).toBeNull();
		expect(chatText(null)).toBeNull();
		expect(chatText('chat')).toBeNull();
	});

	it('cuts long messages and drops control characters', () => {
		expect(chatText({ k: 'chat', text: 'x'.repeat(900) })).toHaveLength(MAX_CHAT_CHARS);
		expect(chatText({ k: 'chat', text: 'a\u0000b\u0007c' })).toBe('abc');
		expect(chatText({ k: 'chat', text: '<b>x</b>' })).toBe('<b>x</b>');
	});

	it('makes what to send from typed text', () => {
		expect(chatBody('  ')).toBeNull();
		expect(chatBody(' hello ')).toEqual({ k: 'chat', text: 'hello' });
		expect(chatBody('y'.repeat(600))!.text).toHaveLength(MAX_CHAT_CHARS);
	});

	it('keeps the newest lines', () => {
		const lines = Array.from({ length: KEEP_CHAT + 5 }, (_, i) => i);
		const kept = capLines(lines);
		expect(kept).toHaveLength(KEEP_CHAT);
		expect(kept[0]).toBe(5);
		expect(capLines([1, 2])).toEqual([1, 2]);
	});
});
