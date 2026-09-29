import type { KeyChord } from '../../models';
import type { SelectionPlatform } from '../selection/keybindings';

/** Formats a chord with the host's modifier names, matching the function-key bar's wording. */
export function formatShortcut(chord: KeyChord, platform: SelectionPlatform = 'unknown'): string {
  const primary = platform === 'macos' ? 'Cmd' : platform === 'unknown' ? 'Ctrl/Cmd' : 'Ctrl';
  return [
    chord.ctrl || chord.meta ? primary : undefined,
    chord.alt ? (platform === 'macos' ? 'Option' : 'Alt') : undefined,
    chord.shift ? 'Shift' : undefined,
    chord.key.length === 1 ? chord.key.toUpperCase() : chord.key,
  ]
    .filter((part): part is string => part !== undefined)
    .join('+');
}
