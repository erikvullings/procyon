import type { Location } from './location';

/** Copy or move intent retained with in-application file references. */
export type ClipboardMode = 'copy' | 'move';

/** Frontend-owned file references awaiting a paste operation. */
export interface ClipboardState {
  readonly mode?: ClipboardMode;
  readonly locations: readonly Location[];
}
