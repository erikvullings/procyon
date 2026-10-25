import { describe, expect, it } from 'vitest';

import type { PaneId, TabId, WorkspaceId, WorkspaceProjection } from '../../models';
import { formatWindowTitle, locationLabel, MAX_WINDOW_TITLE_LENGTH } from './window-title';

function workspaceAt(name: string, uri: string, second?: string): WorkspaceProjection {
  const tab = (id: string, u: string) => ({
    id: id as TabId,
    title: id,
    location: { providerId: 'local', uri: u },
    canNavigateBack: false,
    canNavigateForward: false,
    view: {},
  });
  const left = 'left' as PaneId;
  const right = 'right' as PaneId;
  return {
    id: 'w' as WorkspaceId,
    name,
    revision: 1,
    layout: {},
    paneOrder: [left, right],
    panesById: {
      [left]: {
        id: left,
        tabOrder: ['l' as TabId],
        tabsById: { l: tab('l', uri) },
        activeTabId: 'l' as TabId,
      },
      [right]: {
        id: right,
        tabOrder: ['r' as TabId],
        tabsById: { r: tab('r', second ?? 'file:///other') },
        activeTabId: 'r' as TabId,
      },
    },
    activePaneId: left,
    operationCentre: {},
    ephemeral: false,
  } as unknown as WorkspaceProjection;
}

describe('locationLabel', () => {
  it.each([
    ['file:///Users/me/Documents/', 'Documents'],
    ['file:///', '/'],
    ['file:///C:/', 'C:'],
    ['file:///C:/Work', 'Work'],
    ['file:///Volumes/Backup', 'Backup'],
    ['file:///a/My%20Folder', 'My Folder'],
    ['archive:///tmp/x.zip!/', 'x.zip'],
    ['archive:///tmp/x.zip!/inner/dir', 'dir'],
    ['sftp://conn-1/home/user', 'user'],
    ['sftp://conn-1/', 'sftp'],
  ])('%s -> %s', (uri, label) => {
    expect(locationLabel({ providerId: 'p', uri })).toBe(label);
  });

  it('has no label for search results', () => {
    expect(locationLabel({ providerId: 'p', uri: 'search://abc' })).toBeUndefined();
  });
});

describe('formatWindowTitle', () => {
  it('puts workspace and folder before the app name', () => {
    expect(formatWindowTitle({ workspace: workspaceAt('Work', 'file:///a/Projects') })).toBe(
      'Work · Projects — Procyon',
    );
  });

  it('follows the focused pane', () => {
    const ws = workspaceAt('Work', 'file:///a/Projects', 'file:///b/Photos');
    expect(formatWindowTitle({ workspace: ws, focusedPaneId: 'right' as PaneId })).toBe(
      'Work · Photos — Procyon',
    );
  });

  it('omits a blank workspace name', () => {
    expect(formatWindowTitle({ workspace: workspaceAt('  ', 'file:///a/Projects') })).toBe(
      'Projects — Procyon',
    );
  });

  it('falls back to the app name without a workspace', () => {
    expect(formatWindowTitle({ workspace: undefined })).toBe('Procyon');
  });

  it('shows / for the filesystem root', () => {
    expect(formatWindowTitle({ workspace: workspaceAt('W', 'file:///') })).toBe('W · / — Procyon');
  });

  it('truncates long names to the maximum length with an ellipsis', () => {
    const title = formatWindowTitle({
      workspace: workspaceAt('w'.repeat(80), `file:///${'f'.repeat(80)}`),
    });
    expect(title.length).toBeLessThanOrEqual(MAX_WINDOW_TITLE_LENGTH);
    expect(title).toContain('…');
    expect(title.endsWith(' — Procyon')).toBe(true);
  });

  it('keeps a short workspace intact and shortens a long folder', () => {
    const title = formatWindowTitle({
      workspace: workspaceAt('Work', `file:///${'f'.repeat(80)}`),
    });
    expect(title.startsWith('Work · ')).toBe(true);
    expect(title.length).toBeLessThanOrEqual(MAX_WINDOW_TITLE_LENGTH);
  });
});
