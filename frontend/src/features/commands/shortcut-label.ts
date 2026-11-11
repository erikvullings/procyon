import { usesLiteralControl } from '../../keybindings/dispatcher';
import type { KeyChord } from '../../models';
import type { SelectionPlatform } from '../selection/keybindings';

/** Key names whose display form is not simply the capitalised key. */
const KEY_LABELS: Readonly<Record<string, string>> = {
  ' ': 'Space',
  SPACE: 'Space',
  ARROWUP: 'Up',
  ARROWDOWN: 'Down',
  ARROWLEFT: 'Left',
  ARROWRIGHT: 'Right',
  PAGEUP: 'PageUp',
  PAGEDOWN: 'PageDown',
  ESCAPE: 'Esc',
};

/**
 * The one display form of a key, whatever case the binding was stored in: single characters are
 * upper-cased, function keys keep their name, named keys are capitalised (`ENTER` -> `Enter`).
 */
export function displayKey(key: string): string {
  const upper = key.toUpperCase();
  const label = KEY_LABELS[upper];
  if (label !== undefined) return label;
  if (key.length === 1 || /^F\d{1,2}$/u.test(upper)) return upper;
  return key === upper ? `${upper.charAt(0)}${upper.slice(1).toLowerCase()}` : key;
}

/** The host's name for the primary (Ctrl/Cmd) modifier of a chord ending in `key`. */
function primaryModifier(key: string, platform: SelectionPlatform): string {
  if (usesLiteralControl({ key, ctrl: true })) return 'Ctrl';
  if (platform === 'macos') return 'Cmd';
  return platform === 'unknown' ? 'Ctrl/Cmd' : 'Ctrl';
}

function altModifier(platform: SelectionPlatform): string {
  return platform === 'macos' ? 'Option' : 'Alt';
}

/** Formats a chord with the host's modifier names, matching the function-key bar's wording. */
export function formatShortcut(chord: KeyChord, platform: SelectionPlatform = 'unknown'): string {
  return [
    chord.ctrl || chord.meta ? primaryModifier(chord.key, platform) : undefined,
    chord.alt ? altModifier(platform) : undefined,
    chord.shift ? 'Shift' : undefined,
    displayKey(chord.key),
  ]
    .filter((part): part is string => part !== undefined)
    .join('+');
}

/** Formats a stored binding string (`CTRL+SHIFT+P`) exactly as {@link formatShortcut} would. */
export function formatBindingShortcut(shortcut: string, platform: SelectionPlatform): string {
  const parts = shortcut.split('+');
  const key = parts.at(-1) ?? '';
  return parts
    .map((part, index) => {
      if (index === parts.length - 1) return displayKey(part);
      if (part === 'CTRL') return platform === 'macos' ? primaryModifier(key, platform) : 'Ctrl';
      if (part === 'ALT') return altModifier(platform);
      if (part === 'SHIFT') return 'Shift';
      return displayKey(part);
    })
    .join('+');
}
