import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  ActionDescriptor,
  Connection,
  EntryId,
  PaneId,
  WorkspaceProjection,
} from '../../models';
import {
  constrainSplitRatio,
  pathFromUri,
  WorkspaceLayoutView,
  type WorkspaceLayoutViewAttrs,
} from './workspace-layout';

describe('pathFromUri', () => {
  it('shows an archive as a navigable filesystem path plus inner path', () => {
    expect(pathFromUri('archive:///home/erik/My%20Comic.zip!/chapter')).toBe(
      '/home/erik/My Comic.zip!/chapter',
    );
  });

  it('hides the sftp connection id and returns only the remote path', () => {
    expect(pathFromUri('sftp://11111111-1111-4111-8111-111111111111/home/erik')).toBe('/home/erik');
  });

  it('shows sftp root as slash', () => {
    expect(pathFromUri('sftp://11111111-1111-4111-8111-111111111111/')).toBe('/');
  });

  it('hides FTP connection ids and returns only the remote path', () => {
    expect(pathFromUri('ftp://11111111-1111-4111-8111-111111111111/pub')).toBe('/pub');
    expect(pathFromUri('ftps://11111111-1111-4111-8111-111111111111/secure')).toBe('/secure');
  });

  it.each(['onedrive', 'webdav', 's3'])(
    'hides %s connection ids and returns only the remote path',
    (scheme) => {
      expect(pathFromUri(`${scheme}://11111111-1111-4111-8111-111111111111/team/Documents`)).toBe(
        '/team/Documents',
      );
    },
  );
});

let root: HTMLElement;

/** Simulates the pointer-based tab drag used by `TabStrip` (see tab-strip.ts) — dropping
 * `source` onto `target` via pointerdown/pointermove/pointerup, stubbing
 * `document.elementFromPoint` so the drop-target hit test resolves to `target`. */
function dragTab(
  source: HTMLElement | null | undefined,
  target: HTMLElement | null | undefined,
): void {
  if (source == null || target == null) throw new Error('dragTab: missing source or target');
  Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => target });
  source.dispatchEvent(new MouseEvent('pointerdown', { clientX: 0, clientY: 0, bubbles: true }));
  window.dispatchEvent(new MouseEvent('pointermove', { clientX: 20, clientY: 20 }));
  window.dispatchEvent(new MouseEvent('pointerup', { clientX: 20, clientY: 20 }));
}

const keybindingActions = [
  {
    id: 'core.switchPane',
    title: 'Switch pane',
    defaultShortcuts: [{ key: 'TAB' }, { key: 'TAB', shift: true }],
  },
].map(
  (action): ActionDescriptor => ({
    category: 'test',
    contextRequirements: {},
    source: { kind: 'core' },
    ...action,
  }),
);

function projection(): WorkspaceProjection {
  const emptyView = {
    sort: [],
    columns: [],
    showHidden: false,
    foldersFirst: true,
    quickFilter: null,
  };
  return {
    id: 'workspace-1',
    name: 'Development',
    revision: 7,
    layout: {
      type: 'split',
      axis: 'horizontal',
      ratio: 0.5,
      first: { type: 'pane', paneId: 'left' },
      second: { type: 'pane', paneId: 'right' },
    },
    paneOrder: ['left', 'right'],
    panesById: {
      left: {
        id: 'left',
        tabOrder: ['left-tab'],
        tabsById: {
          'left-tab': {
            id: 'left-tab',
            title: 'Home',
            location: { providerId: 'local', uri: 'file:///home' },
            canNavigateBack: false,
            canNavigateForward: false,
            view: emptyView,
          },
        },
        activeTabId: 'left-tab',
      },
      right: {
        id: 'right',
        tabOrder: ['right-tab'],
        tabsById: {
          'right-tab': {
            id: 'right-tab',
            title: 'Downloads',
            location: { providerId: 'local', uri: 'file:///downloads' },
            canNavigateBack: false,
            canNavigateForward: false,
            view: emptyView,
          },
        },
        activeTabId: 'right-tab',
      },
    },
    activePaneId: 'left',
    operationCentre: { visible: true, height: 180 },
    ephemeral: false,
  };
}

function attrs(overrides: Partial<WorkspaceLayoutViewAttrs> = {}): WorkspaceLayoutViewAttrs {
  return {
    workspace: projection(),
    paneContent: () => ({
      state: { type: 'loaded' },
      entries: [],
      selectedEntryIds: new Set<EntryId>(),
      cutEntryIds: new Set<EntryId>(),
      sortLabel: 'Name ascending',
      sort: [{ columnId: 'core.name', direction: 'ascending' }],
      totalEntryCount: 0,
      hiddenSelectedCount: 0,
      filterOpen: false,
      filterQuery: '',
      platform: 'linux',
      keybindingRuntime: 'desktop',
      actions: keybindingActions,
      keybindingOverrides: {},
      onNavigate: vi.fn(),
      onBack: vi.fn(),
      onForward: vi.fn(),
      onParent: vi.fn(),
      onOpenEntry: vi.fn(),
      onRename: vi.fn(),
      onSelectionAction: vi.fn(),
      onRetry: vi.fn(),
      onLoadNextPage: vi.fn(),
      onSortChange: vi.fn(),
      onFilterQueryChange: vi.fn(),
      onFilterCommit: vi.fn(),
      onFilterClose: vi.fn(),
    }),
    onActivatePane: vi.fn(),
    onUpdateLayout: vi.fn(),
    onSelectTab: vi.fn(),
    onCloseTab: vi.fn(),
    onNewTab: vi.fn(),
    onMoveTab: vi.fn(),
    ...overrides,
  };
}

function mount(viewAttrs: WorkspaceLayoutViewAttrs): void {
  m.mount(root, { view: () => m(WorkspaceLayoutView, viewAttrs) });
}

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  vi.useRealTimers();
  m.mount(root, null);
  root.remove();
});

describe('WorkspaceLayoutView pane focus', () => {
  it('uses the saved connection name for a remote root title', () => {
    const workspace = projection();
    const left = workspace.panesById.left;
    const tab = left?.tabsById['left-tab'];
    if (tab === undefined) throw new Error('left tab fixture missing');
    tab.location = {
      providerId: 'ftp',
      uri: 'ftps://11111111-1111-4111-8111-111111111111/',
    };
    tab.title = '11111111-1111-4111-8111-111111111111';
    const base = attrs({ workspace });
    const basePaneContent = base.paneContent;

    mount({
      ...base,
      paneContent: (paneId) => ({
        ...basePaneContent(paneId),
        connections: [
          {
            id: '11111111-1111-4111-8111-111111111111',
            name: 'Rebex demo',
            kind: 'ftps',
            configuration: {
              kind: 'ftps',
              host: 'test.rebex.net',
              port: 21,
              username: 'demo',
              startPath: '/',
            },
            hasCredential: true,
            status: 'connected',
            createdAt: '2026-08-13T00:00:00Z',
            updatedAt: '2026-08-13T00:00:00Z',
          },
        ],
      }),
    });

    expect(root.querySelector('[data-pane-id="left"] .fm-pane-tab-title')?.textContent).toBe(
      'Rebex demo',
    );
  });

  it.each([
    {
      kind: 'ssh',
      providerId: 'sftp',
      rootUri: 'sftp://11111111-1111-4111-8111-111111111111/home/erik',
      childUri: 'sftp://11111111-1111-4111-8111-111111111111/home/erik/Documents',
      configuration: {
        kind: 'ssh',
        host: 'example.test',
        port: 22,
        username: 'erik',
        startPath: '/home/erik',
        authentication: 'password',
        hostKeyPolicy: 'promptOnFirstUse',
      },
    },
    {
      kind: 'oneDrive',
      providerId: 'onedrive',
      rootUri: 'onedrive://11111111-1111-4111-8111-111111111111/',
      childUri: 'onedrive://11111111-1111-4111-8111-111111111111/Documents',
      configuration: { kind: 'oneDrive', accountHint: null },
    },
    {
      kind: 'webDav',
      providerId: 'webdav',
      rootUri: 'webdav://11111111-1111-4111-8111-111111111111/team',
      childUri: 'webdav://11111111-1111-4111-8111-111111111111/team/Documents',
      configuration: {
        kind: 'webDav',
        baseUrl: 'https://example.test/dav',
        username: 'erik',
        authentication: 'basic',
        pathPrefix: '/team',
      },
    },
    {
      kind: 's3',
      providerId: 's3',
      rootUri: 's3://11111111-1111-4111-8111-111111111111/archive',
      childUri: 's3://11111111-1111-4111-8111-111111111111/archive/Documents',
      configuration: {
        kind: 's3',
        accessKeyId: 'AKIAEXAMPLE',
        bucket: 'documents',
        startPath: '/archive',
      },
    },
  ] as const)(
    'uses the saved $kind name at the configured root and keeps its plug icon in subfolders',
    ({ kind, providerId, rootUri, childUri, configuration }) => {
      const workspace = projection();
      const tab = workspace.panesById.left?.tabsById['left-tab'];
      if (tab === undefined) throw new Error('left tab fixture missing');
      tab.location = { providerId, uri: rootUri };
      tab.title = 'opaque root';
      const base = attrs({ workspace });
      const basePaneContent = base.paneContent;
      const connection: Connection = {
        id: '11111111-1111-4111-8111-111111111111',
        name: `${kind} account`,
        kind,
        configuration,
        hasCredential: true,
        status: 'connected',
        rootLocation: kind === 'oneDrive' ? rootUri : null,
        createdAt: '2026-08-13T00:00:00Z',
        updatedAt: '2026-08-13T00:00:00Z',
      };
      const viewAttrs = {
        ...base,
        paneContent: (paneId: PaneId) => ({
          ...basePaneContent(paneId),
          connections: [connection],
        }),
      };

      mount(viewAttrs);
      expect(root.querySelector('[data-pane-id="left"] .fm-pane-tab-title')?.textContent).toBe(
        `${kind} account`,
      );
      expect(
        root.querySelector('[data-pane-id="left"] .fm-pane-tab-connection-icon'),
      ).not.toBeNull();

      tab.location = { providerId, uri: childUri };
      tab.title = 'Documents';
      m.redraw.sync();

      expect(root.querySelector('[data-pane-id="left"] .fm-pane-tab-title')?.textContent).toBe(
        'Documents',
      );
      expect(
        root.querySelector('[data-pane-id="left"] .fm-pane-tab-connection-icon'),
      ).not.toBeNull();
    },
  );

  it('activates a pane when anywhere inside it is clicked', () => {
    const onActivatePane = vi.fn<(paneId: PaneId) => void>();
    mount(attrs({ onActivatePane }));

    root
      .querySelector<HTMLElement>('[data-pane-id="right"] .fm-pane-status')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(onActivatePane).toHaveBeenCalledExactlyOnceWith('right');
    expect(document.activeElement).toBe(root.querySelector('[data-pane-id="right"] > .fm-pane'));
  });

  it('activates the pane that receives keyboard focus', () => {
    const onActivatePane = vi.fn<(paneId: PaneId) => void>();
    mount(attrs({ onActivatePane }));

    root.querySelector<HTMLElement>('[data-pane-id="right"] > .fm-pane')?.focus();

    expect(onActivatePane).toHaveBeenCalledExactlyOnceWith('right');
  });
});

describe('WorkspaceLayoutView keyboard navigation', () => {
  it('moves active pane focus in layout order when Tab is pressed', () => {
    const onActivatePane = vi.fn<(paneId: PaneId) => void>();
    mount(attrs({ onActivatePane }));
    const left = root.querySelector<HTMLElement>('[data-pane-id="left"]');
    left?.focus();
    onActivatePane.mockClear();

    left?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));

    expect(onActivatePane).toHaveBeenCalledExactlyOnceWith('right');
    expect(document.activeElement).toBe(root.querySelector('[data-pane-id="right"] > .fm-pane'));
  });

  it('offers onPaneCycleBoundary a chance to redirect focus when Tab wraps past the last pane', () => {
    const onActivatePane = vi.fn<(paneId: PaneId) => void>();
    const onPaneCycleBoundary = vi.fn(() => true);
    mount(attrs({ onActivatePane, onPaneCycleBoundary }));
    const right = root.querySelector<HTMLElement>('[data-pane-id="right"] > .fm-pane');
    right?.focus();
    onActivatePane.mockClear();

    right?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));

    expect(onPaneCycleBoundary).toHaveBeenCalledOnce();
    // The boundary handler claimed focus (e.g. the directory-tree sidebar), so the normal wrap
    // back to the first pane must not also happen.
    expect(onActivatePane).not.toHaveBeenCalled();
  });

  it('falls back to the normal wrap when onPaneCycleBoundary declines (or is unset)', () => {
    const onActivatePane = vi.fn<(paneId: PaneId) => void>();
    const onPaneCycleBoundary = vi.fn(() => false);
    mount(attrs({ onActivatePane, onPaneCycleBoundary }));
    const right = root.querySelector<HTMLElement>('[data-pane-id="right"] > .fm-pane');
    right?.focus();
    onActivatePane.mockClear();

    right?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));

    expect(onPaneCycleBoundary).toHaveBeenCalledOnce();
    expect(onActivatePane).toHaveBeenCalledExactlyOnceWith('left');
  });

  it('does not consult onPaneCycleBoundary for a Tab that stays within the pane cycle', () => {
    const onActivatePane = vi.fn<(paneId: PaneId) => void>();
    const onPaneCycleBoundary = vi.fn(() => true);
    mount(attrs({ onActivatePane, onPaneCycleBoundary }));
    const left = root.querySelector<HTMLElement>('[data-pane-id="left"] > .fm-pane');
    left?.focus();
    onActivatePane.mockClear();

    left?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));

    expect(onPaneCycleBoundary).not.toHaveBeenCalled();
    expect(onActivatePane).toHaveBeenCalledExactlyOnceWith('right');
  });

  it('moves focus from a folder to an open terminal with Shift+Tab', () => {
    const onFocusTerminal = vi.fn(() => true);
    const onActivatePane = vi.fn<(paneId: PaneId) => void>();
    mount(attrs({ onFocusTerminal, onActivatePane }));
    const left = root.querySelector<HTMLElement>('[data-pane-id="left"] > .fm-pane');

    left?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true }),
    );

    expect(onFocusTerminal).toHaveBeenCalledOnce();
    expect(onActivatePane).not.toHaveBeenCalledWith('right');
  });

  it('moves focus into an open F3 viewer instead of cycling panes when Tab is pressed', () => {
    const onFocusViewer = vi.fn(() => true);
    const onActivatePane = vi.fn<(paneId: PaneId) => void>();
    mount(attrs({ onFocusViewer, onActivatePane }));
    const left = root.querySelector<HTMLElement>('[data-pane-id="left"] > .fm-pane');
    left?.focus();
    onActivatePane.mockClear();

    left?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));

    expect(onFocusViewer).toHaveBeenCalledOnce();
    // The viewer claimed focus, so the normal pane-to-pane cycle must not also run.
    expect(onActivatePane).not.toHaveBeenCalled();
  });

  it('cycles to the other pane when Tab is pressed from inside the F3 viewer', () => {
    const onFocusViewer = vi.fn(() => true);
    const onActivatePane = vi.fn<(paneId: PaneId) => void>();
    mount(attrs({ onFocusViewer, onActivatePane }));
    const leftPane = root.querySelector<HTMLElement>('[data-pane-id="left"] > .fm-pane');
    const viewer = document.createElement('section');
    viewer.className = 'fm-pane-viewer';
    const search = document.createElement('input');
    search.className = 'fm-file-viewer-search-input';
    viewer.append(search);
    leftPane?.append(viewer);
    onActivatePane.mockClear();

    search.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));

    expect(onFocusViewer).not.toHaveBeenCalled();
    expect(onActivatePane).toHaveBeenCalledExactlyOnceWith('right');
  });

  it('falls back to the normal pane cycle when onFocusViewer declines (or is unset)', () => {
    const onFocusViewer = vi.fn(() => false);
    const onActivatePane = vi.fn<(paneId: PaneId) => void>();
    mount(attrs({ onFocusViewer, onActivatePane }));
    const left = root.querySelector<HTMLElement>('[data-pane-id="left"] > .fm-pane');
    left?.focus();
    onActivatePane.mockClear();

    left?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));

    expect(onFocusViewer).toHaveBeenCalledOnce();
    expect(onActivatePane).toHaveBeenCalledExactlyOnceWith('right');
  });

  it('renders and traverses a future three-pane tree in layout order', () => {
    const threePane = projection();
    const right = threePane.panesById.right;
    if (right === undefined) throw new Error('fixture is missing the right pane');
    threePane.panesById.third = { ...right, id: 'third' };
    threePane.paneOrder = ['third', 'right', 'left'];
    threePane.layout = {
      type: 'split',
      axis: 'horizontal',
      ratio: 0.4,
      first: { type: 'pane', paneId: 'left' },
      second: {
        type: 'split',
        axis: 'vertical',
        ratio: 0.5,
        first: { type: 'pane', paneId: 'right' },
        second: { type: 'pane', paneId: 'third' },
      },
    };
    const onActivatePane = vi.fn<(paneId: PaneId) => void>();
    mount(attrs({ workspace: threePane, onActivatePane }));
    const left = root.querySelector<HTMLElement>('[data-pane-id="left"]');

    left?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));

    expect(root.querySelectorAll('.fm-workspace-pane')).toHaveLength(3);
    expect(onActivatePane).toHaveBeenCalledExactlyOnceWith('right');
  });
});

describe('tab strip wiring', () => {
  it('forwards tab select/close/new callbacks scoped to the owning pane', () => {
    const onSelectTab = vi.fn();
    const onCloseTab = vi.fn();
    const onNewTab = vi.fn();
    // A pane's only tab has no close button (tab-strip.ts) - a second tab is needed here so
    // the close button this test clicks actually exists.
    const twoTabs = projection();
    const rightPaneState = twoTabs.panesById.right;
    if (rightPaneState === undefined) throw new Error('right pane missing');
    twoTabs.panesById.right = {
      ...rightPaneState,
      tabOrder: ['right-tab', 'right-tab-2'],
      tabsById: {
        ...rightPaneState.tabsById,
        'right-tab-2': {
          id: 'right-tab-2',
          title: 'Second',
          location: { providerId: 'local', uri: 'file:///second' },
          canNavigateBack: false,
          canNavigateForward: false,
          view: rightPaneState.tabsById['right-tab']?.view as never,
        },
      },
    };
    mount(attrs({ workspace: twoTabs, onSelectTab, onCloseTab, onNewTab }));

    const rightPane = root.querySelector<HTMLElement>('[data-pane-id="right"]');
    rightPane?.querySelector<HTMLElement>('[role="tab"]')?.click();
    rightPane?.querySelector<HTMLElement>('.fm-pane-tab-close')?.click();
    rightPane?.querySelector<HTMLElement>('.fm-pane-tab-new')?.click();

    expect(onSelectTab).toHaveBeenCalledExactlyOnceWith('right', 'right-tab');
    expect(onCloseTab).toHaveBeenCalledExactlyOnceWith('right', 'right-tab');
    expect(onNewTab).toHaveBeenCalledExactlyOnceWith('right');
  });

  it('does not also dispatch pane activation when a tab header handles the click', () => {
    const onSelectTab = vi.fn();
    const onActivatePane = vi.fn();
    mount(attrs({ onSelectTab, onActivatePane }));

    root.querySelector<HTMLElement>('[data-pane-id="right"] [role="tab"]')?.click();

    expect(onSelectTab).toHaveBeenCalledExactlyOnceWith('right', 'right-tab');
    expect(onActivatePane).not.toHaveBeenCalled();
  });

  it('requests a persisted tab reorder within the owning pane', () => {
    const twoTabs = projection();
    const leftPane = twoTabs.panesById.left;
    if (leftPane === undefined) throw new Error('left pane missing');
    twoTabs.panesById.left = {
      ...leftPane,
      tabOrder: ['left-tab', 'left-tab-2'],
      tabsById: {
        ...leftPane.tabsById,
        'left-tab-2': {
          id: 'left-tab-2',
          title: 'Second',
          location: { providerId: 'local', uri: 'file:///second' },
          canNavigateBack: false,
          canNavigateForward: false,
          view: leftPane.tabsById['left-tab']?.view as never,
        },
      },
    };
    const onMoveTab = vi.fn();
    mount(attrs({ workspace: twoTabs, onMoveTab }));

    const leftPaneElement = root.querySelector<HTMLElement>('[data-pane-id="left"]');
    const tabTitles = (): (string | undefined)[] =>
      [...(leftPaneElement?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [])].map(
        (element) => element.querySelector('.fm-pane-tab-title')?.textContent ?? undefined,
      );
    expect(tabTitles()).toEqual(['Home', 'Second']);

    const tabs = leftPaneElement?.querySelectorAll<HTMLElement>('[role="tab"]');
    dragTab(tabs?.[1], tabs?.[0]);
    expect(onMoveTab).toHaveBeenCalledExactlyOnceWith('left', 'left-tab-2', 'left', 0);
  });

  it('moves a dragged tab to another pane', () => {
    const onMoveTab = vi.fn();
    mount(attrs({ onMoveTab }));
    const leftTab = root.querySelector<HTMLElement>('[data-pane-id="left"] [role="tab"]');
    const rightTab = root.querySelector<HTMLElement>('[data-pane-id="right"] [role="tab"]');

    dragTab(leftTab, rightTab);

    expect(onMoveTab).toHaveBeenCalledExactlyOnceWith('left', 'left-tab', 'right', 0);
  });
});

describe('splitter constraints', () => {
  it('keeps both sides above their minimum width', () => {
    expect(constrainSplitRatio(10, 1_000, 240)).toBeCloseTo(0.24);
    expect(constrainSplitRatio(990, 1_000, 240)).toBeCloseTo(0.76);
    expect(constrainSplitRatio(500, 1_000, 240)).toBeCloseTo(0.5);
  });

  it('debounces a dragged ratio before emitting the updated layout', () => {
    vi.useFakeTimers();
    const onUpdateLayout = vi.fn<(layout: WorkspaceProjection['layout']) => void>();
    mount(attrs({ onUpdateLayout }));
    const split = root.querySelector<HTMLElement>('.fm-workspace-split');
    const splitter = root.querySelector<HTMLElement>('.fm-workspace-splitter');
    vi.spyOn(split as HTMLElement, 'getBoundingClientRect').mockReturnValue({
      x: 100,
      y: 0,
      top: 0,
      right: 1_100,
      bottom: 600,
      left: 100,
      width: 1_000,
      height: 600,
      toJSON: () => ({}),
    });

    splitter?.dispatchEvent(new MouseEvent('pointerdown', { clientX: 600, bubbles: true }));
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: 800 }));

    expect(onUpdateLayout).not.toHaveBeenCalled();
    vi.advanceTimersByTime(499);
    expect(onUpdateLayout).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);

    expect(onUpdateLayout).toHaveBeenCalledExactlyOnceWith({
      ...projection().layout,
      ratio: 0.7,
    });
  });

  it('flushes a pending debounced layout update immediately via registerFlush', () => {
    vi.useFakeTimers();
    const onUpdateLayout = vi.fn<(layout: WorkspaceProjection['layout']) => void>();
    let flush: (() => void) | undefined;
    mount(
      attrs({
        onUpdateLayout,
        registerFlush: (registered) => {
          flush = registered;
        },
      }),
    );
    const split = root.querySelector<HTMLElement>('.fm-workspace-split');
    const splitter = root.querySelector<HTMLElement>('.fm-workspace-splitter');
    vi.spyOn(split as HTMLElement, 'getBoundingClientRect').mockReturnValue({
      x: 100,
      y: 0,
      top: 0,
      right: 1_100,
      bottom: 600,
      left: 100,
      width: 1_000,
      height: 600,
      toJSON: () => ({}),
    });

    splitter?.dispatchEvent(new MouseEvent('pointerdown', { clientX: 600, bubbles: true }));
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: 800 }));

    expect(onUpdateLayout).not.toHaveBeenCalled();
    flush?.();

    expect(onUpdateLayout).toHaveBeenCalledExactlyOnceWith({
      ...projection().layout,
      ratio: 0.7,
    });

    vi.advanceTimersByTime(500);
    expect(onUpdateLayout).toHaveBeenCalledOnce();
  });

  it('does nothing when flushed with no pending layout update', () => {
    let flush: (() => void) | undefined;
    const onUpdateLayout = vi.fn<(layout: WorkspaceProjection['layout']) => void>();
    mount(
      attrs({
        onUpdateLayout,
        registerFlush: (registered) => {
          flush = registered;
        },
      }),
    );

    flush?.();

    expect(onUpdateLayout).not.toHaveBeenCalled();
  });
});
