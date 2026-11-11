import { describe, expect, it } from 'vitest';

import { displayKey, formatBindingShortcut, formatShortcut } from './shortcut-label';

describe('shortcut labels', () => {
  it('shows one display form per key regardless of how the binding was stored', () => {
    expect(displayKey('p')).toBe('P');
    expect(displayKey('f5')).toBe('F5');
    expect(displayKey('ENTER')).toBe('Enter');
    expect(displayKey('Backspace')).toBe('Backspace');
    expect(displayKey('ArrowUp')).toBe('Up');
    expect(displayKey(' ')).toBe('Space');
  });

  it('formats a stored binding exactly like the equivalent chord', () => {
    for (const platform of ['macos', 'windows', 'linux'] as const) {
      expect(formatBindingShortcut('CTRL+SHIFT+P', platform)).toBe(
        formatShortcut({ key: 'p', ctrl: true, shift: true }, platform),
      );
      expect(formatBindingShortcut('ALT+F7', platform)).toBe(
        formatShortcut({ key: 'F7', alt: true }, platform),
      );
    }
  });

  it('uses the host modifier names', () => {
    expect(formatBindingShortcut('CTRL+ENTER', 'macos')).toBe('Cmd+Enter');
    expect(formatBindingShortcut('CTRL+ENTER', 'windows')).toBe('Ctrl+Enter');
    expect(formatBindingShortcut('ALT+Z', 'macos')).toBe('Option+Z');
    expect(formatBindingShortcut('ALT+Z', 'linux')).toBe('Alt+Z');
  });

  it('keeps Ctrl for chords that are literal Control on every host', () => {
    expect(formatBindingShortcut('CTRL+TAB', 'macos')).toBe('Ctrl+Tab');
    expect(formatShortcut({ key: 'Tab', ctrl: true }, 'macos')).toBe('Ctrl+Tab');
  });
});
