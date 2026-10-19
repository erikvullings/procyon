import m, { type FactoryComponent } from 'mithril';
import { ThemeManager } from 'mithril-materialized';

import type { FileManagerClient } from '../api/client/file-manager-client';
import { FileViewer } from '../features/preview/file-viewer';
import { fileViewerAttrs } from '../features/preview/file-viewer-attrs';
import {
  createFileViewerController,
  type FileViewerController,
  type FileViewerState,
} from '../features/preview/file-viewer-controller';
import { previewPluginRoutes } from '../features/preview/plugin-preview';
import type { ViewerWindowRequest } from '../features/preview/viewer-window';
import { setLocale, t } from '../i18n';
import type { Settings } from '../models';
import './viewer-window.css';

export interface ViewerWindowAttrs {
  readonly client: FileManagerClient;
  readonly request: ViewerWindowRequest;
}

function applyAppearance(settings: Settings): void {
  setLocale(settings.language);
  ThemeManager.setTheme(settings.theme);
  document.documentElement.style.setProperty('--fm-font-size', `${settings.fontSize}px`);
  document.documentElement.style.setProperty('--fm-row-height', `${settings.rowHeight}px`);
}

async function closeThisWindow(): Promise<void> {
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow().close();
  } catch {
    globalThis.close();
  }
}

function isEditableTarget(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  );
}

/**
 * Stand-alone Lister window: mounts only the F3 viewer for one file (the "open viewer in a
 * separate window" setting). It uses the same controller and presentational component as the
 * in-pane viewer; only the surrounding chrome - appearance, window closing and the few shortcuts
 * the app shell would otherwise provide - lives here.
 */
export const ViewerWindow: FactoryComponent<ViewerWindowAttrs> = () => {
  let state: FileViewerState | undefined;
  let controller: FileViewerController | undefined;
  let quickLookAvailable = false;

  function disposeController(): void {
    controller?.dispose();
    controller = undefined;
  }

  function onKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape' && !isEditableTarget(event.target)) {
      event.preventDefault();
      void closeThisWindow();
    } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') {
      event.preventDefault();
      document
        .querySelector('.fm-file-viewer')
        ?.dispatchEvent(new CustomEvent('fm-viewer-toggle-search', { bubbles: true }));
    } else if (event.altKey && event.code === 'Space') {
      event.preventDefault();
      controller?.toggleMetadataPanel();
    }
  }

  return {
    oninit: ({ attrs: { client, request } }) => {
      document.title = request.entry.name;
      document.addEventListener('keydown', onKeydown);
      // Closing the OS window or popup never unmounts the component, so release backend
      // sessions (database copies, document conversions) when the page goes away.
      globalThis.addEventListener('pagehide', disposeController);
      void (async () => {
        const [settings, plugins, actions] = await Promise.all([
          client.getSettings().catch(() => undefined),
          client.listPlugins().catch(() => []),
          client.listActions().catch(() => []),
        ]);
        if (settings !== undefined) applyAppearance(settings);
        quickLookAvailable = actions.some(
          (action) =>
            action.id === 'core.quickLook' && action.contextRequirements.featureAvailable !== false,
        );
        controller = createFileViewerController({
          client,
          entry: request.entry,
          ...(request.workspaceId === undefined ? {} : { workspaceId: request.workspaceId }),
          previewPlugins: previewPluginRoutes(plugins),
          ...(request.initialSearch === undefined ? {} : { initialSearch: request.initialSearch }),
          ...(request.initialPage === undefined ? {} : { initialPage: request.initialPage }),
          update: (next) => {
            state = next;
            m.redraw();
          },
        });
        m.redraw();
      })();
    },
    onremove: () => {
      document.removeEventListener('keydown', onKeydown);
      globalThis.removeEventListener('pagehide', disposeController);
      disposeController();
    },
    view: ({ attrs: { client, request } }) => {
      if (state === undefined || controller === undefined) {
        return m(
          '.fm-viewer-window',
          m(
            '.fm-file-viewer-loading',
            { role: 'status', 'aria-label': t('shell', 'loading') },
            m('span.fm-file-viewer-loading-spinner', { 'aria-hidden': 'true' }),
          ),
        );
      }
      const entry = request.entry;
      const invokeOnEntry = (actionId: string) =>
        void client.invokeAction({
          actionId,
          parameters: { uri: entry.location.uri },
          context: { selectedEntryIds: [entry.id], cursorEntryId: entry.id },
        });
      return m(
        '.fm-viewer-window',
        m(
          FileViewer,
          fileViewerAttrs(state, controller, {
            openExternalUrl: (url) => void client.openExternalUrl(url),
            mapClient: client,
            quickLookAvailable,
            onQuickLook: () => invokeOnEntry('core.quickLook'),
            onOpenExternally: () => invokeOnEntry('core.open'),
            onClose: () => void closeThisWindow(),
          }),
        ),
      );
    },
  };
};
