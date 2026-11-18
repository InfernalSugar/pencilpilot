import rulesToml from '../../../../content/feedback/rules.toml?raw';
import tipsToml from '../../../../content/feedback/tips.toml?raw';

/** The bits of a feedback rule the UI needs (content/feedback/rules.toml). */
export interface RuleInfo {
	id: string;
	skill: string;
	lesson?: string;
	drill?: string;
	/** tip title, with placeholders like {n} removed */
	title: string;
}

/** Minimal reader for our own flat TOML (`key = "string"` lines only). */
function field(block: string, key: string): string | undefined {
	return block.match(new RegExp(`^${key}\\s*=\\s*"([^"]*)"`, 'm'))?.[1];
}

const tipTitles = new Map(
	tipsToml
		.split(/^\[tip\./m)
		.slice(1)
		.map((b) => [b.slice(0, b.indexOf(']')), field(b, 'title') ?? ''] as const)
);

export const rules: RuleInfo[] = rulesToml
	.split(/^\[\[rule\]\]/m)
	.slice(1)
	.map((b) => {
		const id = field(b, 'id')!;
		return {
			id,
			skill: field(b, 'skill') ?? '',
			lesson: field(b, 'lesson'),
			drill: field(b, 'drill'),
			title: (tipTitles.get(id) ?? id)
				.replace(/\{n\}%?\s*/g, '')
				.replace(/\{dir\}/g, 'off')
				.replace(/\s+/g, ' ')
				.trim()
		};
	});

export const ruleById = (id: string) => rules.find((r) => r.id === id);
