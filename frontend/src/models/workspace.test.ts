import { describe, expect, it } from 'vitest';

import type { WorkspaceDto } from '../api/generated/models/workspaceDto';
import { workspaceProjectionFromDto } from './workspace';

const fixture: WorkspaceDto = {
  schemaVersion: 1,
  id: '985d4d6e-c37b-4135-90a0-ce0afe165fd9',
  name: 'Development',
  revision: 12,
  layout: {
    type: 'split',
    axis: 'horizontal',
    ratio: 0.52,
    first: { type: 'pane', paneId: 'pane-left' },
    second: { type: 'pane', paneId: 'pane-right' },
  },
  panes: [
    {
      id: 'pane-left',
      title: null,
      activeTabId: 'tab-dev',
      defaultView: {
        sort: [],
        columns: [],
        showHidden: false,
        foldersFirst: true,
        quickFilter: null,
      },
      tabs: [
        {
          id: 'tab-dev',
          titleOverride: null,
          location: { providerId: 'local', uri: 'file:///Users/erik/dev' },
          history: {
            back: [],
            forward: [{ providerId: 'local', uri: 'file:///Users/erik' }],
          },
          view: {
            sort: [{ columnId: 'core.name', direction: 'ascending' }],
            columns: [{ columnId: 'core.name', width: 360, visible: true }],
            showHidden: true,
            foldersFirst: true,
            quickFilter: null,
          },
          pinned: false,
        },
      ],
    },
    {
      id: 'pane-right',
      title: null,
      activeTabId: 'tab-downloads',
      defaultView: {
        sort: [],
        columns: [],
        showHidden: false,
        foldersFirst: true,
        quickFilter: null,
      },
      tabs: [
        {
          id: 'tab-downloads',
          titleOverride: 'Downloads',
          location: { providerId: 'local', uri: 'file:///Users/erik/Downloads' },
          history: {
            back: [{ providerId: 'local', uri: 'file:///Users/erik' }],
            forward: [],
          },
          view: {
            sort: [{ columnId: 'core.modified', direction: 'descending' }],
            columns: [{ columnId: 'core.name', width: 340, visible: true }],
            showHidden: false,
            foldersFirst: true,
            quickFilter: null,
          },
          pinned: false,
        },
      ],
    },
  ],
  activePaneId: 'pane-left',
  operationCentre: { visible: true, height: 180 },
  ephemeral: false,
  createdAt: '2026-07-30T00:00:00Z',
  updatedAt: '2026-07-30T00:00:00Z',
};

const SAMPLE_TAB_VIEW: WorkspaceDto['panes'][number]['tabs'][number]['view'] = {
  sort: [],
  columns: [],
  showHidden: false,
  foldersFirst: true,
  quickFilter: null,
};

/** Builds a minimal single-pane, single-tab workspace for tests that only need to assert on one
 * tab's normalized projection. */
function workspaceWithSingleTab(
  tab: Omit<WorkspaceDto['panes'][number]['tabs'][number], 'view'>,
): WorkspaceDto {
  return {
    schemaVersion: 1,
    id: 'workspace-1',
    name: 'Test',
    revision: 1,
    layout: { type: 'pane', paneId: 'pane-left' },
    panes: [
      {
        id: 'pane-left',
        title: null,
        activeTabId: tab.id,
        defaultView: {
          sort: [],
          columns: [],
          showHidden: false,
          foldersFirst: true,
          quickFilter: null,
        },
        tabs: [{ ...tab, view: SAMPLE_TAB_VIEW }],
      },
    ],
    activePaneId: 'pane-left',
    operationCentre: { visible: true, height: 180 },
    ephemeral: false,
    createdAt: '2026-07-30T00:00:00Z',
    updatedAt: '2026-07-30T00:00:00Z',
  };
}

describe('workspaceProjectionFromDto', () => {
  it('normalizes the persisted-workspace example into ordered id maps', () => {
    const projection = workspaceProjectionFromDto(fixture);

    expect(projection).toEqual({
      id: fixture.id,
      name: 'Development',
      revision: 12,
      layout: fixture.layout,
      paneOrder: ['pane-left', 'pane-right'],
      panesById: {
        'pane-left': {
          id: 'pane-left',
          tabOrder: ['tab-dev'],
          tabsById: {
            'tab-dev': {
              id: 'tab-dev',
              title: 'dev',
              location: { providerId: 'local', uri: 'file:///Users/erik/dev' },
              canNavigateBack: false,
              canNavigateForward: true,
              view: fixture.panes[0]?.tabs[0]?.view,
            },
          },
          activeTabId: 'tab-dev',
        },
        'pane-right': {
          id: 'pane-right',
          tabOrder: ['tab-downloads'],
          tabsById: {
            'tab-downloads': {
              id: 'tab-downloads',
              title: 'Downloads',
              location: { providerId: 'local', uri: 'file:///Users/erik/Downloads' },
              canNavigateBack: true,
              canNavigateForward: false,
              view: fixture.panes[1]?.tabs[0]?.view,
            },
          },
          activeTabId: 'tab-downloads',
        },
      },
      activePaneId: 'pane-left',
      operationCentre: { visible: true, height: 180 },
      ephemeral: false,
    });
  });

  it('carries ephemeral and forkedFrom through onto the projection', () => {
    const forked: WorkspaceDto = {
      ...fixture,
      ephemeral: true,
      forkedFrom: 'source-workspace-id',
    };

    const projection = workspaceProjectionFromDto(forked);

    expect(projection.ephemeral).toBe(true);
    expect(projection.forkedFrom).toBe('source-workspace-id');
  });

  it('omits forkedFrom from the projection when the DTO has none', () => {
    const projection = workspaceProjectionFromDto(fixture);

    expect(projection.forkedFrom).toBeUndefined();
  });

  it('titles a filesystem root tab "/" instead of the bare "file:" scheme', () => {
    const withRootTab = workspaceWithSingleTab({
      id: 'tab-root',
      titleOverride: null,
      location: { providerId: 'local', uri: 'file:///' },
      history: { back: [], forward: [] },
      pinned: false,
    });

    const projection = workspaceProjectionFromDto(withRootTab);

    expect(projection.panesById['pane-left']?.tabsById['tab-root']?.title).toBe('/');
  });

  it('redirects a persisted search:// tab to the folder it was originally searched from', () => {
    const withSearchTab = workspaceWithSingleTab({
      id: 'tab-search',
      titleOverride: null,
      location: { providerId: 'search', uri: 'search://local/some-search-id' },
      history: {
        back: [
          { providerId: 'local', uri: 'file:///Users/erik/dev' },
          { providerId: 'local', uri: 'file:///Users/erik/dev/src' },
        ],
        forward: [],
      },
      pinned: false,
    });

    const projection = workspaceProjectionFromDto(withSearchTab, { redirectSessionOnlyTabs: true });
    const tab = projection.panesById['pane-left']?.tabsById['tab-search'];

    expect(tab?.location).toEqual({ providerId: 'local', uri: 'file:///Users/erik/dev/src' });
    expect(tab?.title).toBe('src');
    expect(tab?.canNavigateBack).toBe(false);
    expect(tab?.canNavigateForward).toBe(false);
  });

  it('leaves a search:// tab unchanged when not hydrating (live command response)', () => {
    const withSearchTab = workspaceWithSingleTab({
      id: 'tab-search',
      titleOverride: null,
      location: { providerId: 'search', uri: 'search://local/some-search-id' },
      history: {
        back: [{ providerId: 'local', uri: 'file:///Users/erik/dev' }],
        forward: [],
      },
      pinned: false,
    });

    const projection = workspaceProjectionFromDto(withSearchTab);
    const tab = projection.panesById['pane-left']?.tabsById['tab-search'];

    expect(tab?.location).toEqual({ providerId: 'search', uri: 'search://local/some-search-id' });
  });

  it('leaves a search:// tab unchanged when its history has no usable folder', () => {
    const withSearchTab = workspaceWithSingleTab({
      id: 'tab-search',
      titleOverride: null,
      location: { providerId: 'search', uri: 'search://local/some-search-id' },
      history: { back: [], forward: [] },
      pinned: false,
    });

    const projection = workspaceProjectionFromDto(withSearchTab, { redirectSessionOnlyTabs: true });
    const tab = projection.panesById['pane-left']?.tabsById['tab-search'];

    expect(tab?.location).toEqual({ providerId: 'search', uri: 'search://local/some-search-id' });
  });

  it('redirects a persisted archive:// tab to the folder containing the archive file', () => {
    const withArchiveTab = workspaceWithSingleTab({
      id: 'tab-archive',
      titleOverride: null,
      location: {
        providerId: 'archive',
        uri: 'archive:///Users/erik/Downloads/photos.zip!/chapter1',
      },
      history: { back: [], forward: [] },
      pinned: false,
    });

    const projection = workspaceProjectionFromDto(withArchiveTab, {
      redirectSessionOnlyTabs: true,
    });
    const tab = projection.panesById['pane-left']?.tabsById['tab-archive'];

    expect(tab?.location).toEqual({ providerId: 'local', uri: 'file:///Users/erik/Downloads' });
    expect(tab?.title).toBe('Downloads');
    expect(tab?.canNavigateBack).toBe(false);
    expect(tab?.canNavigateForward).toBe(false);
  });
});
