import type { GradeCategory } from './grader';

/**
 * The grader's weights for a picture category. Cartoons are graded like
 * anime (clean outlines matter most); master studies like real life (copying
 * a painting or drawing is about proportion and tone, as with a photo).
 */
export function gradeCategory(category: string | null | undefined): GradeCategory {
	switch (category) {
		case 'cartoon':
			return 'anime';
		case 'masters':
			return 'real-life';
		case 'anime':
		case 'real-life':
		case 'animals':
		case 'environment':
			return category;
		default:
			return 'fundamentals';
	}
}
