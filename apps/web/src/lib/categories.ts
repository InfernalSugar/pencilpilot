import { inCategory } from './references';

export interface Category {
	id: string;
	name: string;
	blurb: string;
	color: string;
	dark?: boolean;
	/** true when at least one approved reference exists */
	available: boolean;
}

const base: Omit<Category, 'available'>[] = [
	{
		id: 'anime',
		name: 'Anime',
		blurb: 'Characters, action, scenery',
		color: 'var(--pp-cat-anime)'
	},
	{
		id: 'real-life',
		name: 'Real life',
		blurb: 'People, objects, food',
		color: 'var(--pp-cat-real)'
	},
	{
		id: 'animals',
		name: 'Animals',
		blurb: 'Pets, wildlife, birds',
		color: 'var(--pp-cat-animals)'
	},
	{
		id: 'cartoon',
		name: 'Cartoon',
		blurb: 'Characters, critters, things',
		color: 'var(--pp-lilac-ink)'
	},
	{
		id: 'environment',
		name: 'Environment',
		blurb: 'Places, plants, skies',
		color: 'var(--pp-cat-env)'
	},
	{
		id: 'fundamentals',
		name: 'Fundamentals',
		blurb: 'Lines, boxes, gesture',
		color: 'var(--pp-cat-fund)',
		dark: true
	}
];

export const categories: Category[] = base.map((c) => ({
	...c,
	available: inCategory(c.id).length > 0
}));

export const categoryById = (id: string | null | undefined) => categories.find((c) => c.id === id);
