import topicsJson from '../../../../content/topics.json';

export interface Topic {
	id: string;
	name: string;
	blurb: string;
	tags: string[];
}

const all = topicsJson as unknown as Record<string, Topic[] | string>;

/** Topics for a category, in display order (alphabetical by name). */
export function topicsFor(category: string): Topic[] {
	const list = all[category];
	return Array.isArray(list) ? [...list].sort((a, b) => a.name.localeCompare(b.name)) : [];
}

export const topicName = (category: string, id: string | null | undefined) =>
	topicsFor(category).find((t) => t.id === id)?.name;
