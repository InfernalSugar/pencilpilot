/** Only allow same-site relative redirects (no open redirects). */
export function safeNext(next: string | null | undefined, fallback = '/progress'): string {
	return next && next.startsWith('/') && !next.startsWith('//') && !next.startsWith('/\\')
		? next
		: fallback;
}
