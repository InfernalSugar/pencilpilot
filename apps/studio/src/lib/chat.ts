/**
 * Chat while drawing together. Messages go to each other member as a direct message
 * (reliable, unlike presence; not an edit, so they never enter the shared project).
 * Nothing is kept: the log lives in the window until the session ends.
 */

export const MAX_CHAT_CHARS = 500;
export const KEEP_CHAT = 200;

export interface ChatLine {
	id: number;
	/** `system`: someone joined or left (no sender) */
	kind: 'message' | 'system';
	from?: string;
	name?: string;
	mine?: boolean;
	text: string;
}

/** The text of an incoming chat body, or null when it isn't one. Never HTML: show it as text. */
export function chatText(body: unknown): string | null {
	const b = body as { k?: unknown; text?: unknown } | null;
	if (!b || typeof b !== 'object' || b.k !== 'chat' || typeof b.text !== 'string') return null;
	// control characters (but not tabs and line breaks) are dropped
	// eslint-disable-next-line no-control-regex
	const text = b.text.replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, '').trim();
	return text ? text.slice(0, MAX_CHAT_CHARS) : null;
}

/** What to send for typed text: the body, or null when there's nothing to say. */
export function chatBody(typed: string): { k: 'chat'; text: string } | null {
	const text = typed.trim().slice(0, MAX_CHAT_CHARS);
	return text ? { k: 'chat', text } : null;
}

/** The newest `KEEP_CHAT` lines. */
export function capLines<T>(lines: T[], keep = KEEP_CHAT): T[] {
	return lines.length > keep ? lines.slice(lines.length - keep) : lines;
}
