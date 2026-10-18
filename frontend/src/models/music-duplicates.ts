import type { MusicDuplicateFileDto } from '../api/generated/models/musicDuplicateFileDto';
import type { MusicDuplicateGroupDto } from '../api/generated/models/musicDuplicateGroupDto';
import type { MusicDuplicatePageDto } from '../api/generated/models/musicDuplicatePageDto';
import type { Location } from './location';

/** One audio file inside a music duplicate group (task 0265). */
export type MusicDuplicateFile = MusicDuplicateFileDto;
export type MusicDuplicateGroup = MusicDuplicateGroupDto;
export type MusicDuplicatePage = MusicDuplicatePageDto;

export interface StartMusicDuplicateScanRequest {
  readonly roots: readonly Location[];
  readonly showHidden?: boolean;
}

export interface StartMusicDuplicateScanResult {
  readonly scanId: string;
}
