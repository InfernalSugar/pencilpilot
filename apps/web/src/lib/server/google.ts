import { Google } from 'arctic';
import { GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET } from '$app/env/private';

export function google(origin: string): Google | null {
	if (!GOOGLE_CLIENT_ID || !GOOGLE_CLIENT_SECRET) return null;
	return new Google(GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET, `${origin}/auth/google/callback`);
}
