import type { SelectionPlatform } from '../features/selection/keybindings';
import type { ActionDescriptor, ActionId, KeyChord } from '../models';

export type KeybindingScope = 'table' | 'pathInput' | 'modal';
export type KeybindingRuntime = 'browser' | 'desktop';

export interface KeybindingContext {
  readonly scope: KeybindingScope;
  readonly platform: SelectionPlatform;
  readonly runtime: KeybindingRuntime;
}

export interface LiveBinding {
  readonly actionId: ActionId;
  readonly shortcut: string;
  readonly available: boolean;
}

export interface BindingConflict {
  readonly shortcut: string;
  readonly actionIds: readonly ActionId[];
}

// Ctrl+N (new browser window) and Ctrl+U (view source) are, like Ctrl+P/Ctrl+W/Ctrl+T,
// intercepted by Chrome's own UI before a page's keydown listener ever runs - no amount of
// `event.preventDefault()` can reclaim them, so `core.newConnection` (task 0128) and
// `core.swapPanes` fall back to the command palette/menu in browser runtime.
const BROWSER_RESERVED = new Set(['CTRL+P', 'CTRL+W', 'CTRL+T', 'CTRL+N', 'CTRL+U']);

function chordFromText(value: string): KeyChord | undefined {
  const parts = value
    .split('+')
    .map((part) => part.trim())
    .filter(Boolean);
  const key = parts.pop();
  if (key === undefined) return undefined;
  return {
    key,
    ...(parts.some((part) => /^(ctrl|cmd|command)$/iu.test(part)) ? { ctrl: true } : {}),
    ...(parts.some((part) => /^shift$/iu.test(part)) ? { shift: true } : {}),
    ...(parts.some((part) => /^(alt|option)$/iu.test(part)) ? { alt: true } : {}),
  };
}

function effectiveChords(
  action: ActionDescriptor,
  overrides: Readonly<Record<string, string>>,
): readonly KeyChord[] {
  const override = overrides[action.id];
  if (override === undefined) return action.defaultShortcuts;
  const chord = chordFromText(override);
  return chord === undefined ? [] : [chord];
}

/** Formats a `KeyChord` the way live bindings and the shortcuts-help lookup display it. */
export function normalizedShortcut(chord: KeyChord): string {
  return [
    ...(chord.ctrl || chord.meta ? ['CTRL'] : []),
    ...(chord.alt ? ['ALT'] : []),
    ...(chord.shift ? ['SHIFT'] : []),
    chord.key.toUpperCase(),
  ].join('+');
}

/** Returns whether an event uses the host's primary shortcut modifier. */
export function hasPrimaryModifier(event: KeyboardEvent, platform: SelectionPlatform): boolean {
  return platform === 'macos' ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
}

/** A single letter/digit chord key must be matched against `event.code` (the physical key),
 * never `event.key`: macOS's Option modifier composes a dead-key/alternate character for most
 * letters when held (e.g. Option+Q produces `event.key === 'œ'`, not `'q'`), so any Alt+<letter>
 * binding - including user-customized ones like rebinding Quit to Alt+Q - would otherwise never
 * match on macOS even though the physical key is correct. Non-alphanumeric keys (F-keys, Enter,
 * arrows, punctuation) aren't affected by this composition, so they keep comparing `event.key`. */
function keyMatches(event: KeyboardEvent, chordKey: string): boolean {
  if (/^[A-Za-z0-9]$/u.test(chordKey)) {
    // Real keyboard events always carry a `code`; fall back to `event.key` only for
    // hand-built test fixtures/synthetic events that omit it.
    if (event.code) {
      const expectedCode = /[A-Za-z]/u.test(chordKey)
        ? `Key${chordKey.toUpperCase()}`
        : `Digit${chordKey}`;
      return event.code === expectedCode;
    }
  }
  return event.key.toUpperCase() === chordKey.toUpperCase();
}

function matches(event: KeyboardEvent, chord: KeyChord, platform: SelectionPlatform): boolean {
  if (!keyMatches(event, chord.key)) return false;
  const wantsModifier = Boolean(chord.ctrl || chord.meta);
  // Ctrl+Tab/Ctrl+Shift+Tab is a platform-invariant tab-cycling convention (every
  // browser and desktop app honours literal Control here, never Command) because
  // Cmd+Tab is reserved by macOS for the app switcher and never reaches the page.
  // So Tab chords check the literal Control key instead of going through the
  // translated "primary modifier" used for every other shortcut.
  const modifierMatches =
    chord.key.toUpperCase() === 'TAB'
      ? event.ctrlKey === wantsModifier && !event.metaKey
      : wantsModifier
        ? hasPrimaryModifier(event, platform)
        : // A bare chord must reject literal Control on macOS too, not just Command:
          // `hasPrimaryModifier` only recognises Cmd there, so without this check a
          // Ctrl-held keypress would look identical to an unmodified one and silently
          // fall through to whatever bare binding shares the same key (e.g. Ctrl+Backspace
          // on macOS matching the plain-Backspace "parent directory" binding instead of
          // doing nothing, since the real Ctrl-translated shortcut there needs Cmd).
          !event.ctrlKey && !event.metaKey;
  if (!modifierMatches) return false;
  return Boolean(chord.shift) === event.shiftKey && Boolean(chord.alt) === event.altKey;
}

function available(chord: KeyChord, context: KeybindingContext): boolean {
  return context.runtime !== 'browser' || !BROWSER_RESERVED.has(normalizedShortcut(chord));
}

/** Resolves a keyboard event to one action id without reading from the DOM. */
export function dispatchKeybinding(
  event: KeyboardEvent,
  context: KeybindingContext,
  actions: readonly ActionDescriptor[],
  overrides: Readonly<Record<string, string>>,
): ActionId | undefined {
  if (context.scope !== 'table') return undefined;
  for (const action of actions) {
    for (const chord of effectiveChords(action, overrides)) {
      if (available(chord, context) && matches(event, chord, context.platform)) return action.id;
    }
  }
  return undefined;
}

/** Lists effective bindings for the function-key bar and settings editor. */
export function getLiveBindings(
  actions: readonly ActionDescriptor[],
  overrides: Readonly<Record<string, string>>,
  context: KeybindingContext,
): readonly LiveBinding[] {
  return actions.flatMap((action) => {
    return effectiveChords(action, overrides).map((chord) => ({
      actionId: action.id,
      shortcut: normalizedShortcut(chord),
      available: available(chord, context),
    }));
  });
}

const FOOTER_SHORTCUT_PATTERN = /^F(?:2|3|4|5|6|7|8)$/u;

/** One entry in the footer function-key hint bar. */
export interface FunctionKeyBinding {
  readonly actionId: ActionId;
  readonly shortcut: string;
  readonly key: string;
  readonly title: string;
  readonly actionAvailable: boolean;
}

export interface FunctionKeyModifiers {
  readonly primary?: boolean;
  readonly alt?: boolean;
  readonly shift?: boolean;
}

/**
 * Lists the footer's function-key hints (F2-F8) for the active modifier
 * layer, sorted in ascending F-key order; `actionAvailable` says whether
 * the bound action can run right
 * now (see `evaluateActionAvailability`). Actions that are permanently
 * unavailable in this runtime (`contextRequirements.featureAvailable ===
 * false`) are omitted entirely rather than shown disabled, since they can
 * never become available without a full session restart. `core.view` and
 * `core.edit` are exceptions because their in-app implementations work in
 * browser/server mode even when the platform fallback does not.
 */
export function footerFunctionKeyBindings(
  actions: readonly ActionDescriptor[],
  overrides: Readonly<Record<string, string>>,
  context: KeybindingContext,
  isActionAvailable: (action: ActionDescriptor) => boolean,
  modifiers: FunctionKeyModifiers = {},
): readonly FunctionKeyBinding[] {
  return getLiveBindings(actions, overrides, context)
    .flatMap((binding) => {
      const key = binding.shortcut.split('+').at(-1);
      if (key === undefined || !FOOTER_SHORTCUT_PATTERN.test(key)) return [];
      const parts = new Set(binding.shortcut.split('+').slice(0, -1));
      if (
        parts.has('CTRL') !== Boolean(modifiers.primary) ||
        parts.has('ALT') !== Boolean(modifiers.alt) ||
        parts.has('SHIFT') !== Boolean(modifiers.shift)
      )
        return [];
      return [{ binding, key }];
    })
    .flatMap((binding) => {
      const action = actions.find((candidate) => candidate.id === binding.binding.actionId);
      // View and Edit have browser-capable in-app implementations. Their backend descriptors
      // only describe whether the OS fallback is available, so keep their footer hints visible.
      if (
        action === undefined ||
        (action.contextRequirements.featureAvailable === false &&
          (binding.binding.shortcut !== binding.key ||
            (action.id !== 'core.view' && action.id !== 'core.edit')))
      )
        return [];
      return [
        {
          actionId: action.id,
          shortcut: binding.binding.shortcut,
          key: binding.key,
          title: action.title,
          actionAvailable: isActionAvailable(action),
        },
      ];
    })
    .sort((a, b) => Number.parseInt(a.key.slice(1), 10) - Number.parseInt(b.key.slice(1), 10));
}

/** Reports effective-shortcut collisions for the settings editor. */
export function detectBindingConflicts(
  actions: readonly ActionDescriptor[],
  overrides: Readonly<Record<string, string>>,
  context: KeybindingContext,
): readonly BindingConflict[] {
  const byShortcut = new Map<string, ActionId[]>();
  for (const binding of getLiveBindings(actions, overrides, context)) {
    if (!binding.available) continue;
    const actionIds = byShortcut.get(binding.shortcut) ?? [];
    actionIds.push(binding.actionId);
    byShortcut.set(binding.shortcut, actionIds);
  }
  return [...byShortcut.entries()]
    .filter(([, actionIds]) => actionIds.length > 1)
    .map(([shortcut, actionIds]) => ({ shortcut, actionIds }));
}
