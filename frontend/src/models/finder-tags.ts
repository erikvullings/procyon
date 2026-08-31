/**
 * Finder tags and the Spotlight comment already have real backend DTOs
 * (task 0136); re-export them as-is so feature code depends on `models/`,
 * never on `api/generated/` directly (spec §12).
 */
export type { FinderTagColorDto as FinderTagColor } from '../api/generated/models/finderTagColorDto';
export type { FinderTagDto as FinderTag } from '../api/generated/models/finderTagDto';
export type { FinderTagsDto as FinderTags } from '../api/generated/models/finderTagsDto';
export type { SpotlightCommentDto as SpotlightComment } from '../api/generated/models/spotlightCommentDto';
