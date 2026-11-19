import { createHash } from 'node:crypto';

/** A signed-in person's player id in a lobby: the same on every device. */
export const memberPlayerId = (userId: string, code: string) =>
	'm-' + createHash('sha256').update(`${userId}:${code}`).digest('base64url').slice(0, 16);

/** Guests pick a random id in the browser; never one that looks like a member's. */
export const isGuestId = (id: unknown): id is string =>
	typeof id === 'string' && /^[A-Za-z0-9_-]{12,24}$/.test(id) && !id.startsWith('m-');
