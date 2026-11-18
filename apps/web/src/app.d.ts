// See https://svelte.dev/docs/kit/types#app.d.ts
import type { SessionUser } from '#lib/server/auth.js';

declare global {
	namespace App {
		interface Locals {
			/** the signed-in user, or null for guests */
			user: SessionUser | null;
			/** id of the current session (hashed token) */
			sessionId: string | null;
		}
	}
}

export {};
