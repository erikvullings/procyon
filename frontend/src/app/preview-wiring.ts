import m from 'mithril';
import { toast } from 'mithril-materialized';
import type { FileManagerClient } from '../api/client/file-manager-client';
import { createFileEditorController } from '../features/editor/file-editor-controller';
import type { EditorLinkHandlers } from '../features/editor/link-actions';
import { isSafeExternalUrl, resolveDetectedLink } from '../features/editor/link-actions';
import { parentLocation } from '../features/navigation/navigation';
import type { PrepareMedia } from '../features/player/media-access';
import type { PluginPaneState } from '../features/plugins/plugin-panel-host';
import { createFileViewerController } from '../features/preview/file-viewer-controller';
import { type GalleryPosition, galleryPosition, isGalleryEntry } from '../features/preview/gallery';
import { previewPluginRoutes } from '../features/preview/plugin-preview';
import { openViewerWindow } from '../features/preview/viewer-window';
import { dispatchWorkspaceCommand } from '../features/workspace/dispatch-workspace-command';
import { errorText, t } from '../i18n';
import type {
  EntrySummary,
  Location,
  PaneId,
  PluginDescriptor,
  TabId,
  TabProjection,
  WorkspaceProjection,
} from '../models';
import { tabKey, workspaceErrorMessage } from './shell-helpers';
import type { ShellState } from './shell-state';

/** The narrow cross-cutting surface preview/editor wiring needs from the shell. */
export interface PreviewCore {
  readonly replaceWorkspace: (next: WorkspaceProjection) => void;
  readonly activatePane: (client: FileManagerClient, paneId: PaneId) => Promise<void>;
  readonly prepareMedia: PrepareMedia;
  /** A tab's entries in the order its pane shows them (sorted, quick-filtered). */
  readonly visibleEntries: (paneId: PaneId, tab: TabProjection) => readonly EntrySummary[];
}

export function createPreviewWiring(shellState: ShellState, core: PreviewCore) {
  function openViewer(
    client: FileManagerClient,
    paneId: PaneId,
    entry: EntrySummary,
    initialSearch?: {
      readonly query: string;
      readonly regex: boolean;
      readonly caseSensitive: boolean;
      readonly wholeWord: boolean;
    },
    openMetadata?: boolean,
    initialPage?: number,
    knowledgeSourceId?: string,
  ): void {
    if (entry.kind === 'symlink') {
      // View what the link points at: the target's kind and extension pick the viewer.
      const unavailable = () => {
        const message = document.createElement('span');
        message.textContent = t('viewer', 'previewUnavailable', { name: entry.name });
        toast({ html: message.outerHTML });
      };
      client
        .resolveLinkTarget(entry.location)
        .then((resolved) => {
          if (resolved.kind !== 'file') {
            // A link to a folder opens with the OS default application, as the folder itself would.
            const osOpen = shellState.registeredActions.find((action) => action.id === 'core.open');
            const parameters = shellState.actionCommandController.platformActionParameters(
              'core.view',
              [entry],
              undefined,
            );
            if (
              osOpen?.contextRequirements.featureAvailable === false ||
              parameters === undefined
            ) {
              unavailable();
              return;
            }
            shellState.actionCommandController.invokeActionById(
              'core.view',
              parameters,
              shellState.actionCommandController.actionContext(),
            );
            return;
          }
          openViewer(
            client,
            paneId,
            resolved,
            initialSearch,
            openMetadata,
            initialPage,
            knowledgeSourceId,
          );
        })
        .catch(unavailable);
      return;
    }
    // "Open viewer in a separate window": the Lister opens in its own window. Opens that rely on
    // in-pane state (knowledge sources, the Alt+Space panel preset) stay in a pane tab, and so does
    // the fallback when no window could be opened (e.g. a blocked browser popup).
    if (
      shellState.currentSettings?.viewerInSeparateWindow === true &&
      knowledgeSourceId === undefined &&
      openMetadata !== true
    ) {
      void openViewerWindow(client, {
        entry,
        ...(shellState.workspace ? { workspaceId: shellState.workspace.id } : {}),
        ...(initialSearch ? { initialSearch } : {}),
        ...(initialPage ? { initialPage } : {}),
      }).then((opened) => {
        if (!opened)
          openViewerInPane(client, paneId, entry, initialSearch, openMetadata, initialPage);
      });
      return;
    }
    openViewerInPane(
      client,
      paneId,
      entry,
      initialSearch,
      openMetadata,
      initialPage,
      knowledgeSourceId,
    );
  }

  function openViewerInPane(
    client: FileManagerClient,
    paneId: PaneId,
    entry: EntrySummary,
    initialSearch?: {
      readonly query: string;
      readonly regex: boolean;
      readonly caseSensitive: boolean;
      readonly wholeWord: boolean;
    },
    openMetadata?: boolean,
    initialPage?: number,
    knowledgeSourceId?: string,
  ): void {
    const existingViewer = [...shellState.viewerByTab.entries()][0];
    if (existingViewer !== undefined) {
      const [key, viewer] = existingViewer;
      if (viewer.state.entry.location.uri === entry.location.uri) {
        if (initialPage === undefined) {
          closeViewer(viewer.paneId, viewer.tabId);
          return;
        }
        shellState.tabController.activateTab(viewer.paneId, viewer.tabId);
        viewer.controller.goToPdfPage(initialPage);
        return;
      }
      viewer.controller.dispose();
      shellState.viewerByTab.delete(key);
      const controller = createFileViewerController({
        client,
        entry,
        ...(shellState.workspace ? { workspaceId: shellState.workspace.id } : {}),
        previewPlugins: previewPluginRoutes(shellState.plugins),
        ...(initialSearch ? { initialSearch } : {}),
        ...(initialPage ? { initialPage } : {}),
        initialMetadataPanelOpen: openMetadata === true,
        prepareMedia: core.prepareMedia,
        update: (state) => {
          const current = shellState.viewerByTab.get(key);
          if (current === undefined) return;
          current.state = state;
          m.redraw();
        },
      });
      shellState.viewerByTab.set(key, {
        paneId: viewer.paneId,
        tabId: viewer.tabId,
        controller,
        ...(knowledgeSourceId === undefined ? {} : { knowledgeSourceId }),
        state: { status: 'loading', entry },
      });
      shellState.tabController.activateTab(viewer.paneId, viewer.tabId);
      m.redraw();
      return;
    }
    const currentWorkspace = shellState.workspace;
    const pane = currentWorkspace?.panesById[paneId];
    const activeTab = pane?.tabsById[pane.activeTabId];
    if (currentWorkspace === undefined || activeTab === undefined) return;
    void dispatchWorkspaceCommand(
      client,
      {
        type: 'addTransientTab',
        workspaceId: currentWorkspace.id,
        paneId,
        location: activeTab.location,
        expectedRevision: currentWorkspace.revision,
      },
      (next) => {
        core.replaceWorkspace(next);
        const tabId = next.panesById[paneId]?.activeTabId;
        if (tabId === undefined) return;
        const key = tabKey(paneId, tabId);
        const controller = createFileViewerController({
          client,
          entry,
          workspaceId: currentWorkspace.id,
          previewPlugins: previewPluginRoutes(shellState.plugins),
          ...(initialSearch ? { initialSearch } : {}),
          ...(initialPage ? { initialPage } : {}),
          initialMetadataPanelOpen: openMetadata === true,
          prepareMedia: core.prepareMedia,
          update: (state) => {
            const existing = shellState.viewerByTab.get(key);
            if (existing === undefined) return;
            existing.state = state;
            m.redraw();
          },
        });
        shellState.viewerByTab.set(key, {
          paneId,
          tabId,
          controller,
          ...(knowledgeSourceId === undefined ? {} : { knowledgeSourceId }),
          state: { status: 'loading', entry },
        });
        m.redraw();
      },
    ).catch((error: unknown) => {
      console.error('openViewerInPane failed', error);
      toast({ html: errorText(error) });
    });
  }

  /**
   * Where the pane's open media viewer sits among the folder's images/videos/audio, in the order
   * the folder is displayed. The viewer tab only borrows its folder from the tab it was opened
   * from, so that sibling tab provides the listing.
   */
  function galleryOf(paneId: PaneId): GalleryPosition | undefined {
    const pane = shellState.workspace?.panesById[paneId];
    const viewer =
      pane === undefined ? undefined : shellState.viewerByTab.get(tabKey(paneId, pane.activeTabId));
    const viewerTab = pane?.tabsById[pane.activeTabId];
    if (pane === undefined || viewer === undefined || viewerTab === undefined) return undefined;
    if (!isGalleryEntry(viewer.state.entry)) return undefined;
    const source = Object.values(pane.tabsById).find(
      (tab) =>
        tab.id !== viewerTab.id &&
        !shellState.viewerByTab.has(tabKey(paneId, tab.id)) &&
        tab.location.providerId === viewerTab.location.providerId &&
        tab.location.uri === viewerTab.location.uri,
    );
    if (source === undefined) return undefined;
    return galleryPosition(core.visibleEntries(paneId, source), viewer.state.entry.location);
  }

  /** Flips the pane's media viewer to the previous (-1) or next (1) item; false at either end. */
  function stepGallery(client: FileManagerClient, paneId: PaneId, delta: -1 | 1): boolean {
    const position = galleryOf(paneId);
    const target = delta < 0 ? position?.previous : position?.next;
    if (target === undefined) return false;
    openViewerInPane(client, paneId, target);
    return true;
  }

  function closeViewer(paneId: PaneId, tabId?: TabId): void {
    const resolvedTabId = tabId ?? shellState.workspace?.panesById[paneId]?.activeTabId;
    if (resolvedTabId === undefined) return;
    const key = tabKey(paneId, resolvedTabId);
    const viewer = shellState.viewerByTab.get(key);
    if (viewer === undefined) return;
    viewer.controller.dispose();
    shellState.viewerByTab.delete(key);
    shellState.tabController.performCloseTab(paneId, resolvedTabId);
  }

  function openEditor(
    client: FileManagerClient,
    paneId: PaneId,
    entry: EntrySummary,
    initialLine?: number,
  ): void {
    shellState.pluginByPane.delete(paneId);
    closeViewer(paneId);
    shellState.editorByPane.get(paneId)?.controller.dispose();
    const controller = createFileEditorController({
      client,
      entry,
      ...(initialLine === undefined ? {} : { initialLine }),
      update: (state) => {
        const existing = shellState.editorByPane.get(paneId);
        if (existing === undefined) return;
        if (state.status === 'error') {
          existing.controller.dispose();
          shellState.editorByPane.delete(paneId);
          toast({ html: state.message });
        } else {
          existing.state = state;
        }
        m.redraw();
      },
    });
    shellState.editorByPane.set(paneId, { controller, state: { status: 'loading', entry } });
    m.redraw();
  }

  function editorIsDirty(paneId: PaneId): boolean {
    const state = shellState.editorByPane.get(paneId)?.state;
    return state?.status === 'ready' && state.dirty;
  }

  function editorLinkHandlers(paneId: PaneId, base: Location): EditorLinkHandlers {
    /** Browsing goes to the other pane when there is one, so the editor stays untouched. */
    const browsePane = (): PaneId | undefined => {
      const other = shellState.workspace?.paneOrder.find((candidate) => candidate !== paneId);
      if (other !== undefined) return other;
      if (editorIsDirty(paneId)) {
        toast({ html: t('editorLinks', 'saveFirst') });
        return undefined;
      }
      const viewer = [...shellState.viewerByTab.values()].find(
        (candidate) => candidate.paneId === paneId,
      );
      if (viewer !== undefined) closeViewer(viewer.paneId, viewer.tabId);
      closeEditor(paneId);
      return paneId;
    };
    return {
      base,
      resolve: (link) => resolveDetectedLink(link, base, shellState.client),
      openUrl: (url) => {
        if (isSafeExternalUrl(url)) void shellState.client.openExternalUrl(url);
      },
      openInPane: (link) => {
        const target = browsePane();
        if (target === undefined) return;
        if (link.entry.kind === 'directory')
          void shellState.navigation.navigate(target, link.location);
        else
          void shellState.navigation.navigate(
            target,
            parentLocation(link.location),
            link.entry.name,
          );
      },
      reveal: (link) => {
        void shellState.client
          .invokeAction({
            actionId: 'core.revealInSystemFileManager',
            parameters: { uri: link.location.uri },
            context: { paneId, selectedEntryIds: [link.entry.id] },
          })
          .catch((error: unknown) => {
            toast({ html: errorText(error) });
          });
      },
      editAt: (link) => {
        if (editorIsDirty(paneId)) {
          toast({ html: t('editorLinks', 'saveFirst') });
          return;
        }
        openEditor(shellState.client, paneId, link.entry, link.line);
      },
    };
  }

  function actOnLinkAtCursor(mode: 'open' | 'reveal'): void {
    const paneId = shellState.workspace?.activePaneId;
    const editor = paneId === undefined ? undefined : shellState.editorByPane.get(paneId);
    const link = editor?.controller.linkAtCursor();
    if (paneId === undefined || editor === undefined || link === undefined) {
      toast({ html: t('editorLinks', 'nothingAtCursor') });
      return;
    }
    const base = editor.state.entry.location;
    const handlers = editorLinkHandlers(paneId, base);
    void handlers.resolve(link).then((resolved) => {
      if (resolved === undefined) {
        toast({ html: t('editorLinks', link.kind === 'url' ? 'unsafeUrl' : 'notFound') });
      } else if (resolved.kind === 'url') {
        if (mode === 'open') handlers.openUrl(resolved.url);
      } else if (mode === 'reveal') {
        handlers.reveal(resolved);
      } else {
        handlers.openInPane(resolved);
      }
    });
  }

  function closeEditor(paneId: PaneId): void {
    shellState.editorByPane.get(paneId)?.controller.dispose();
    shellState.editorByPane.delete(paneId);
    m.redraw();
  }

  function openPluginPane(
    sourcePaneId: PaneId,
    plugin: PluginDescriptor,
    actionId: string,
    entry: EntrySummary,
  ): 'opened' | 'blocked' | 'unavailable' {
    if (shellState.runtimeKind !== 'tauri') return 'unavailable';
    const target = shellState.workspace?.paneOrder.find((paneId) => paneId !== sourcePaneId);
    if (target === undefined) return 'unavailable';
    if (shellState.openingPluginPanes.has(target)) return 'blocked';
    const existing = shellState.pluginByPane.get(target);
    if (
      existing?.pluginId === plugin.id &&
      existing.actionId === actionId &&
      existing.location.providerId === entry.location.providerId &&
      existing.location.uri === entry.location.uri
    ) {
      if (shellState.workspace?.panesById[target]?.activeTabId === existing.tabId) {
        void core.activatePane(shellState.client, target).catch((error: unknown) => {
          toast({ html: workspaceErrorMessage(error, t('action', 'unableToRun')) });
        });
      } else {
        shellState.tabController.activateTab(target, existing.tabId);
      }
      return 'opened';
    }
    if (shellState.editorByPane.has(target)) {
      requestCloseEditor(target);
      if (shellState.editorByPane.has(target)) return 'blocked';
    }
    const current = shellState.workspace;
    const activeTab = current?.panesById[target]?.tabsById[current.panesById[target].activeTabId];
    if (current === undefined || activeTab === undefined) return 'unavailable';
    const showPanel = (tabId: TabId) => {
      const panel: PluginPaneState = {
        panelId: ++shellState.nextPluginPanelId,
        tabId,
        client: shellState.client,
        pluginId: plugin.id,
        actionId,
        location: entry.location,
        title: `${plugin.name}: ${entry.name}`,
        active: true,
        onCloseRequest: () => {
          if (
            shellState.pluginByPane.get(target) === panel &&
            shellState.workspace?.panesById[target]?.activeTabId === tabId
          )
            shellState.tabController.requestCloseTab(target, tabId);
        },
        onError: (error) => {
          if (shellState.pluginByPane.get(target) !== panel) return;
          const text = document.createElement('span');
          text.textContent = workspaceErrorMessage(error, t('action', 'unableToRun'));
          toast({ html: text.innerHTML });
          shellState.tabController.performCloseTab(target, tabId);
        },
      };
      shellState.pluginByPane.set(target, panel);
      m.redraw();
    };
    if (existing !== undefined) {
      showPanel(existing.tabId);
      if (current.panesById[target]?.activeTabId === existing.tabId) {
        void core.activatePane(shellState.client, target).catch((error: unknown) => {
          toast({ html: workspaceErrorMessage(error, t('action', 'unableToRun')) });
        });
      } else {
        shellState.tabController.activateTab(target, existing.tabId);
      }
      return 'opened';
    }
    shellState.openingPluginPanes.add(target);
    void dispatchWorkspaceCommand(
      shellState.client,
      {
        type: 'addTransientTab',
        workspaceId: current.id,
        paneId: target,
        location: activeTab.location,
        expectedRevision: current.revision,
      },
      (next) => {
        core.replaceWorkspace(next);
        const tabId = next.panesById[target]?.activeTabId;
        if (tabId !== undefined) {
          showPanel(tabId);
          void core.activatePane(shellState.client, target).catch((error: unknown) => {
            toast({ html: workspaceErrorMessage(error, t('action', 'unableToRun')) });
          });
        }
      },
    )
      .catch((error: unknown) => {
        toast({ html: workspaceErrorMessage(error, t('action', 'unableToRun')) });
      })
      .finally(() => shellState.openingPluginPanes.delete(target));
    return 'opened';
  }

  function requestCloseEditor(paneId: PaneId): boolean {
    const editor = shellState.editorByPane.get(paneId);
    if (editor === undefined) return false;
    if (editor.controller.requestClose()) closeEditor(paneId);
    return true;
  }

  return {
    openViewer,
    openViewerInPane,
    closeViewer,
    galleryOf,
    stepGallery,
    openEditor,
    editorIsDirty,
    editorLinkHandlers,
    actOnLinkAtCursor,
    closeEditor,
    openPluginPane,
    requestCloseEditor,
  };
}
