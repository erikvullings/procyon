import { describe, expect, it } from 'vitest';

import { isTerminalVisible } from './terminal-state';

describe('terminal tab scope', () => {
  it('keeps the drawer visible while the active tab is unchanged', () => {
    expect(isTerminalVisible(new Set(['pane-1:tab-1']), 'pane-1:tab-1')).toBe(true);
  });

  it('hides the drawer after switching to a tab without its own terminal', () => {
    expect(isTerminalVisible(new Set(['pane-1:tab-1']), 'pane-1:tab-2')).toBe(false);
  });

  it('restores a terminal when that tab becomes active again, regardless of folder', () => {
    const openTabKeys = new Set(['pane-1:tab-1', 'pane-1:tab-2']);
    expect(isTerminalVisible(openTabKeys, 'pane-1:tab-1')).toBe(true);
    expect(isTerminalVisible(openTabKeys, 'pane-1:tab-2')).toBe(true);
  });

  it('is hidden when there is no active tab', () => {
    expect(isTerminalVisible(new Set(['pane-1:tab-1']), undefined)).toBe(false);
  });
});
