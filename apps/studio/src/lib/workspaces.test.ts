import { describe, expect, it } from 'vitest';
import { cleanLayout, cleanSettings, DEFAULT_LAYOUT } from './settings.svelte';
import { BUILT_IN, cleanTools, cleanWorkspaces, freshId, freshName } from './workspaces';

describe('workspaces', () => {
	it('settings from before workspaces: the layout in use becomes Everything’s', () => {
		const s = cleanSettings({ layout: { bottom: false, dockWidth: 400 } });
		expect(s.workspaces.map((w) => w.id)).toEqual(BUILT_IN.map((w) => w.id));
		expect(s.workspace).toBe('everything');
		expect(s.workspaces[0].layout).toEqual(s.layout);
		expect(s.layout).toMatchObject({ bottom: false, dockWidth: 400 });
		// the others as they came
		expect(s.workspaces[1].layout.bottom).toBe(false);
		expect(s.workspaces[2].tab).toBe('xsheet');
	});

	it('clean what was saved: bad ones dropped, unknown tools and tabs out, the brush always', () => {
		const { workspaces, workspace } = cleanWorkspaces(
			[
				{
					id: 'mine-1',
					name: '  Inking  ',
					layout: { dock: false, dockWidth: 9999 },
					tab: 'nope',
					tools: ['eraser', 'laser', 'fill']
				},
				{ id: 'mine-1', name: 'twice' },
				'junk',
				{ name: 'no id' }
			],
			'gone',
			DEFAULT_LAYOUT,
			cleanLayout
		);
		expect(workspaces).toHaveLength(1);
		expect(workspaces[0]).toMatchObject({
			id: 'mine-1',
			name: 'Inking',
			tab: 'layers',
			tools: ['brush', 'eraser', 'fill']
		});
		expect(workspaces[0].layout).toMatchObject({ dock: false, dockWidth: 640 });
		// the one in use is gone: the first
		expect(workspace).toBe('mine-1');
	});

	it('keep their own tool order (the strip’s) and fresh names and ids', () => {
		expect(cleanTools(['lines', 'fill', 'brush'], [])).toEqual(['brush', 'fill', 'lines']);
		expect(cleanTools('x', ['brush', 'eraser'])).toEqual(['brush', 'eraser']);
		const list = [{ ...BUILT_IN[0], id: 'mine-1', name: 'Draw copy' }];
		expect(freshId(list)).toBe('mine-2');
		expect(freshName(list, 'Draw copy')).toBe('Draw copy 2');
		expect(freshName(list, 'Paint')).toBe('Paint');
	});
});
