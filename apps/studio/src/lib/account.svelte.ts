/**
 * The PencilPilot account this app is signed in with, for drawing together (ADR-0013).
 * Shared by the Draw together dialog and Settings. Signing in happens in the system
 * browser, through the app's Rust side; the window itself never goes online.
 */
import { accountStatus, cancelSignIn, signIn, signOut, type Account } from './share';

export const account = $state<{
	me: Account | null;
	/** the status has been read once */
	loaded: boolean;
	/** waiting for the browser */
	signingIn: boolean;
	error: string;
}>({ me: null, loaded: false, signingIn: false, error: '' });

export async function loadAccount() {
	try {
		account.me = await accountStatus();
	} catch {
		account.me = null;
	}
	account.loaded = true;
}

export async function startSignIn() {
	account.signingIn = true;
	account.error = '';
	try {
		account.me = await signIn();
	} catch (e) {
		account.error = String(e);
	} finally {
		account.signingIn = false;
	}
}

export async function stopSignIn() {
	await cancelSignIn().catch(() => {});
}

export async function endAccount() {
	account.error = '';
	await signOut().catch(() => {});
	account.me = null;
}
