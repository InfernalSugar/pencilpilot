export interface Category {
	id: string;
	name: string;
	blurb: string;
	color: string;
	dark?: boolean;
	available: boolean;
}

export const categories: Category[] = [
	{
		id: 'anime',
		name: 'Anime',
		blurb: 'Faces & figures',
		color: 'var(--pp-cat-anime)',
		available: true
	},
	{
		id: 'real-life',
		name: 'Real life',
		blurb: 'People & objects',
		color: 'var(--pp-cat-real)',
		available: true
	},
	{
		id: 'animals',
		name: 'Animals',
		blurb: 'Pets & wildlife',
		color: 'var(--pp-cat-animals)',
		available: true
	},
	{
		id: 'environment',
		name: 'Environment',
		blurb: 'Places & light',
		color: 'var(--pp-cat-env)',
		available: false
	},
	{
		id: 'fundamentals',
		name: 'Fundamentals',
		blurb: 'Lines, boxes, gesture',
		color: 'var(--pp-cat-fund)',
		dark: true,
		available: false
	}
];
