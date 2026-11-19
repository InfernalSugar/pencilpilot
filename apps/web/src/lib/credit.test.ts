import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import RefCredit from './RefCredit.svelte';
import { attributionSentence, creditKind, defaultLicense, providerName, safeLink } from './credit';
import { gradeCategory } from './grade-category';

const base = {
	author: 'Ann',
	authorUrl: 'https://pixabay.com/users/ann-1/',
	pageUrl: 'https://pixabay.com/photos/cat-1/',
	license: 'Pixabay Content License',
	licenseUrl: 'https://pixabay.com/service/license-summary/'
};

const text = (html: string) =>
	html
		.replace(/<!--.*?-->/g, '')
		.replace(/<[^>]+>/g, '')
		.replace(/\s+/g, ' ')
		.trim();

describe('reference credits', () => {
	it('names the real source', () => {
		expect(creditKind({ source: 'pixabay' })).toBe('pixabay');
		expect(creditKind({})).toBe('pixabay');
		expect(creditKind({ source: 'openverse' })).toBe('openverse');
		expect(creditKind({ source: 'met' })).toBe('museum');
		expect(creditKind({ source: 'artic' })).toBe('museum');
		expect(providerName({ source: 'artic' })).toBe('Art Institute of Chicago');
		expect(providerName({ source: 'openverse', provider: 'Wikimedia Commons' })).toBe(
			'Wikimedia Commons'
		);
	});

	it('renders Pixabay as before', () => {
		const { body } = render(RefCredit, { props: { reference: { ...base, source: 'pixabay' } } });
		expect(text(body)).toBe('By Ann via Pixabay · Pixabay Content License');
	});

	it('renders museum works as "Artwork: Title, Artist, Museum (public domain)"', () => {
		const { body } = render(RefCredit, {
			props: {
				reference: {
					...base,
					source: 'met',
					title: 'Still Life',
					author: 'Georg Flegel',
					authorUrl: '',
					pageUrl: 'https://www.metmuseum.org/art/collection/search/436305',
					provider: 'The Metropolitan Museum of Art',
					license: 'Public domain (CC0)',
					licenseUrl: 'https://creativecommons.org/publicdomain/zero/1.0/'
				}
			}
		});
		expect(text(body)).toBe(
			'Artwork: Still Life, Georg Flegel, The Metropolitan Museum of Art (public domain)'
		);
		expect(body).toContain('href="https://www.metmuseum.org/art/collection/search/436305"');
	});

	it('shows the attribution Openverse provides, with the provider and licence', () => {
		const attribution =
			'"Tawny owl" by Jane Doe is licensed under CC BY 4.0. To view a copy of this license, visit https://creativecommons.org/licenses/by/4.0/.';
		expect(attributionSentence(attribution)).toBe(
			'"Tawny owl" by Jane Doe is licensed under CC BY 4.0.'
		);
		const { body } = render(RefCredit, {
			props: {
				reference: {
					...base,
					source: 'openverse',
					provider: 'Wikimedia Commons',
					pageUrl: 'https://commons.wikimedia.org/w/index.php?curid=1',
					license: 'CC BY 4.0',
					licenseUrl: 'https://creativecommons.org/licenses/by/4.0/',
					attribution
				}
			}
		});
		expect(text(body)).toBe(
			'"Tawny owl" by Jane Doe is licensed under CC BY 4.0. · via Wikimedia Commons · CC BY 4.0'
		);
	});
});

describe('links from the browser', () => {
	it('allows the picture sources only', () => {
		expect(safeLink('https://pixabay.com/photos/x-1/')).toBe('https://pixabay.com/photos/x-1/');
		expect(safeLink('https://commons.wikimedia.org/wiki/File:x.jpg')).toBeTruthy();
		expect(safeLink('https://www.artic.edu/artworks/1')).toBeTruthy();
		expect(safeLink('https://www.metmuseum.org/art/collection/search/1')).toBeTruthy();
		expect(safeLink('http://pixabay.com/')).toBe('');
		expect(safeLink('https://evil.example/pixabay.com/')).toBe('');
		expect(safeLink('https://pixabay.com.evil.example/')).toBe('');
		expect(safeLink('javascript:alert(1)')).toBe('');
		expect(safeLink(42)).toBe('');
	});

	it('has a licence for every source', () => {
		expect(defaultLicense('met').license).toBe('Public domain (CC0)');
		expect(defaultLicense(undefined).license).toBe('Pixabay Content License');
	});
});

describe('grading categories', () => {
	it('grades master studies like real life and cartoons like anime', () => {
		expect(gradeCategory('masters')).toBe('real-life');
		expect(gradeCategory('cartoon')).toBe('anime');
		expect(gradeCategory('animals')).toBe('animals');
		expect(gradeCategory(undefined)).toBe('fundamentals');
	});
});
