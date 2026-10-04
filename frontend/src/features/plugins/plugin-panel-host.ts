import m, { type FactoryComponent } from 'mithril';
import type { FileManagerClient, PluginPanelBounds } from '../../api/client/file-manager-client';
import type { Location, PluginId, TabId } from '../../models';
import './plugin-panel-host.css';

export interface PluginPanelHostAttrs {
  readonly client: FileManagerClient;
  readonly pluginId: PluginId;
  readonly actionId: string;
  readonly location: Location;
  readonly title: string;
  readonly active: boolean;
  readonly onError: (error: unknown) => void;
}

export interface PluginPaneState extends PluginPanelHostAttrs {
  readonly panelId: number;
  readonly tabId: TabId;
}

function resolvedTheme(): 'light' | 'dark' {
  const theme = document.documentElement.dataset.theme;
  if (theme === 'light' || theme === 'dark') return theme;
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

export const PluginPanelHost: FactoryComponent<PluginPaneState> = () => {
  let surface: HTMLElement | undefined;
  let observer: ResizeObserver | undefined;
  let label: string | undefined;
  let opening = false;
  let disposed = false;
  let shown = true;
  let visibility = Promise.resolve();
  let themeUpdates = Promise.resolve();
  let lastTheme: 'light' | 'dark' | undefined;
  let themeObserver: MutationObserver | undefined;
  let colorScheme: MediaQueryList | undefined;
  let generation = 0;
  let lastBounds: PluginPanelBounds | undefined;
  let attrs: PluginPaneState;

  const syncTheme = () => {
    const theme = resolvedTheme();
    if (label === undefined || lastTheme === theme) return;
    lastTheme = theme;
    const currentLabel = label;
    themeUpdates = themeUpdates
      .then(() => attrs.client.setPluginPanelTheme(currentLabel, theme))
      .catch((error: unknown) => {
        if (!disposed && label === currentLabel) attrs.onError(error);
      });
  };

  const syncVisibility = () => {
    if (label === undefined || shown === attrs.active) return;
    shown = attrs.active;
    const currentLabel = label;
    const visible = shown;
    visibility = visibility
      .then(() => attrs.client.setPluginPanelVisible(currentLabel, visible))
      .catch((error: unknown) => {
        if (!disposed && label === currentLabel) attrs.onError(error);
      });
  };

  const reposition = () => {
    if (surface === undefined || disposed || !attrs.active) return;
    const rect = surface.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) return;
    const bounds = { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
    if (
      lastBounds?.x === bounds.x &&
      lastBounds.y === bounds.y &&
      lastBounds.width === bounds.width &&
      lastBounds.height === bounds.height
    )
      return;
    lastBounds = bounds;
    if (label !== undefined) {
      void attrs.client.updatePluginPanelBounds(label, bounds).catch(attrs.onError);
    } else if (!opening) {
      opening = true;
      const currentGeneration = generation;
      lastTheme = resolvedTheme();
      void attrs.client
        .openPluginPanel(attrs.pluginId, attrs.actionId, attrs.location, bounds, lastTheme)
        .then((openedLabel) => {
          if (disposed || currentGeneration !== generation) {
            void attrs.client.closePluginPanel(openedLabel).catch((error: unknown) => {
              console.warn('Could not close replaced plugin panel', error);
            });
          } else {
            label = openedLabel;
            lastBounds = undefined;
            syncTheme();
            syncVisibility();
            reposition();
          }
        })
        .catch((error: unknown) => {
          if (!disposed && currentGeneration === generation) attrs.onError(error);
        });
    }
  };

  return {
    onbeforeupdate: ({ attrs: next }, previous) => {
      if (next.panelId !== previous.attrs.panelId) {
        generation++;
        if (label !== undefined) {
          void previous.attrs.client.closePluginPanel(label).catch((error: unknown) => {
            console.warn('Could not close replaced plugin panel', error);
          });
        }
        label = undefined;
        opening = false;
        shown = true;
        visibility = Promise.resolve();
        themeUpdates = Promise.resolve();
        lastTheme = undefined;
        lastBounds = undefined;
      }
    },
    view: ({ attrs: current }) => {
      attrs = current;
      return m('.fm-plugin-panel-host', { 'data-visible': String(current.active) }, [
        m('.fm-plugin-panel-surface', {
          oncreate: ({ dom }) => {
            surface = dom as HTMLElement;
            themeObserver = new MutationObserver(syncTheme);
            themeObserver.observe(document.documentElement, {
              attributes: true,
              attributeFilter: ['data-theme'],
            });
            colorScheme = window.matchMedia?.('(prefers-color-scheme: dark)');
            colorScheme?.addEventListener('change', syncTheme);
            if (typeof ResizeObserver !== 'undefined') {
              observer = new ResizeObserver(reposition);
              observer.observe(surface);
            }
            window.addEventListener('resize', reposition);
            window.addEventListener('scroll', reposition, true);
            reposition();
          },
          onupdate: () => {
            syncTheme();
            syncVisibility();
            reposition();
          },
          onremove: () => {
            disposed = true;
            generation++;
            observer?.disconnect();
            themeObserver?.disconnect();
            colorScheme?.removeEventListener('change', syncTheme);
            window.removeEventListener('resize', reposition);
            window.removeEventListener('scroll', reposition, true);
            if (label !== undefined) {
              void attrs.client.closePluginPanel(label).catch((error: unknown) => {
                console.warn('Could not close plugin panel', error);
              });
            }
          },
        }),
      ]);
    },
  };
};
