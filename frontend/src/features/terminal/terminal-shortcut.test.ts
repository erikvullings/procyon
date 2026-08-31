import { describe, expect, it } from 'vitest';
import { isTerminalToggleShortcut } from '../keybindings/global-keydown-handler';

const key = (value: string, ctrlKey = false, code = '') => ({
  key: value,
  code,
  ctrlKey,
  altKey: false,
  metaKey: false,
  shiftKey: false,
});

describe('embedded terminal shortcut', () => {
  it('toggles with Ctrl+backtick in the desktop host', () => {
    expect(isTerminalToggleShortcut(key('`', true), 'desktop')).toBe(true);
  });

  it('uses the physical Backquote key on dead-key keyboard layouts', () => {
    expect(isTerminalToggleShortcut(key('Dead', true, 'Backquote'), 'desktop')).toBe(true);
  });

  it('reserves F12 for the desktop host', () => {
    expect(isTerminalToggleShortcut(key('F12'), 'desktop')).toBe(true);
    expect(isTerminalToggleShortcut(key('F12'), 'browser')).toBe(false);
  });
});
