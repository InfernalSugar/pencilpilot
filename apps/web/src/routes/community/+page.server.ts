import { communityActions, loadView } from '#lib/server/community-pages.js';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => loadView('closer', locals);
export const actions: Actions = communityActions;
