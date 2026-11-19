/**
 * Credits for reference pictures. Every source wants its own wording
 * (ADR-0003): Pixabay "by X via Pixabay", Pexels "Photo by X on Pexels",
 * Openverse the attribution text it provides, museums
 * "Artwork: Title, Artist, Museum (public domain)".
 */

/** The fields a credit needs (a `Reference` or a lobby reference). */
export interface Creditable {
	source?: string;
	author: string;
	authorUrl: string;
	pageUrl: string;
	license: string;
	licenseUrl: string;
	title?: string;
	provider?: string;
	attribution?: string;
}

export type CreditKind = 'pixabay' | 'pexels' | 'openverse' | 'museum';

export function creditKind(r: Pick<Creditable, 'source'>): CreditKind {
	switch (r.source) {
		case 'pexels':
			return 'pexels';
		case 'openverse':
			return 'openverse';
		case 'artic':
		case 'met':
			return 'museum';
		default:
			return 'pixabay';
	}
}

const PROVIDERS: Record<string, string> = {
	pixabay: 'Pixabay',
	pexels: 'Pexels',
	artic: 'Art Institute of Chicago',
	met: 'The Metropolitan Museum of Art'
};

/** Who holds the picture: "Pixabay", "Wikimedia Commons", "The Met", … */
export function providerName(r: Pick<Creditable, 'source' | 'provider'>): string {
	return r.provider || PROVIDERS[r.source ?? 'pixabay'] || 'Pixabay';
}

/**
 * Openverse's attribution without the "To view a copy of this license,
 * visit …" sentence (the licence is linked next to it instead).
 */
export function attributionSentence(text: string | undefined): string {
	if (!text) return '';
	const cut = text.indexOf(' To view a copy');
	return (cut > 0 ? text.slice(0, cut) : text).trim();
}

/**
 * Sites whose pages we link to for a picture's source and author. Links from
 * the browser (lobbies, shared drawings) must point at one of them.
 */
export const LINK_HOSTS = [
	'pixabay.com',
	'pexels.com',
	'openverse.org',
	'wikimedia.org',
	'wikipedia.org',
	'wikidata.org',
	'artic.edu',
	'metmuseum.org',
	'si.edu',
	'nasa.gov',
	'nps.gov',
	'flickr.com',
	'clevelandart.org',
	'rijksmuseum.nl',
	'smk.dk',
	'brooklynmuseum.org',
	'nypl.org',
	'wellcomecollection.org',
	'biodiversitylibrary.org',
	'cooperhewitt.org',
	'getty.edu',
	'creativecommons.org'
];

/** `u` if it is an https link to one of `LINK_HOSTS` (or a subdomain), else ''. */
export function safeLink(u: unknown, max = 300): string {
	if (typeof u !== 'string' || u.length > max || /[\s"<>]/.test(u)) return '';
	let url: URL;
	try {
		url = new URL(u);
	} catch {
		return '';
	}
	if (url.protocol !== 'https:' || url.username || url.password) return '';
	const host = url.hostname.toLowerCase();
	return LINK_HOSTS.some((h) => host === h || host.endsWith(`.${h}`)) ? u : '';
}

/** Default licence for a source, when a lobby message leaves it out. */
export function defaultLicense(source: string | undefined): {
	license: string;
	licenseUrl: string;
} {
	switch (source) {
		case 'pexels':
			return { license: 'Pexels License', licenseUrl: 'https://www.pexels.com/license/' };
		case 'artic':
		case 'met':
			return {
				license: 'Public domain (CC0)',
				licenseUrl: 'https://creativecommons.org/publicdomain/zero/1.0/'
			};
		default:
			return {
				license: 'Pixabay Content License',
				licenseUrl: 'https://pixabay.com/service/license-summary/'
			};
	}
}

export const SOURCES = ['pixabay', 'pexels', 'openverse', 'artic', 'met'] as const;
