import m, { type FactoryComponent } from 'mithril';
import type { FileManagerClient, PluginPanelBounds } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { Location, PluginId, TabId } from '../../models';
import './plugin-panel-host.css';

export interface PluginPanelHostAttrs {
  readonly client: FileManagerClient;
  readonly pluginId: PluginId;
  readonly actionId: string;
  readonly location: Location;
  readonly title: string;
  readonly onClose: () => void;
  readonly onError: (error: unknown) => void;
}

export interface PluginPaneState extends PluginPanelHostAttrs {
  readonly panelId: number;
  readonly tabId: TabId;
}

export const PluginPanelHost: FactoryComponent<PluginPaneState> = () => {
  let surface: HTMLElement | undefined;
  let observer: ResizeObserver | undefined;
  let label: string | undefined;
  let opening = false;
  let disposed = false;
  let closeRequested = false;
  let generation = 0;
  let lastBounds: PluginPanelBounds | undefined;
  let attrs: PluginPaneState;

  const reposition = () => {
    if (surface === undefined || disposed) return;
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
      void attrs.client
        .openPluginPanel(attrs.pluginId, attrs.actionId, attrs.location, bounds)
        .then((openedLabel) => {
          if (disposed || currentGeneration !== generation) {
            void attrs.client.closePluginPanel(openedLabel).catch((error: unknown) => {
              console.warn('Could not close replaced plugin panel', error);
            });
          } else {
            label = openedLabel;
            lastBounds = undefined;
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
        lastBounds = undefined;
      }
    },
    view: ({ attrs: current }) => {
      attrs = current;
      return m('.fm-plugin-panel-host', [
        m('.fm-plugin-panel-header', [
          m('strong', current.title),
          m(
            'button.fm-plugin-panel-close',
            {
              type: 'button',
              'aria-label': t('editor', 'closeEditor'),
              onclick: () => {
                closeRequested = true;
                current.onClose();
              },
            },
            t('editor', 'close'),
          ),
        ]),
        m('.fm-plugin-panel-surface', {
          oncreate: ({ dom }) => {
            surface = dom as HTMLElement;
            if (typeof ResizeObserver !== 'undefined') {
              observer = new ResizeObserver(reposition);
              observer.observe(surface);
            }
            window.addEventListener('resize', reposition);
            window.addEventListener('scroll', reposition, true);
            reposition();
          },
          onupdate: reposition,
          onremove: () => {
            disposed = true;
            generation++;
            observer?.disconnect();
            window.removeEventListener('resize', reposition);
            window.removeEventListener('scroll', reposition, true);
            if (label !== undefined) {
              void attrs.client.closePluginPanel(label).catch((error: unknown) => {
                console.warn('Could not close plugin panel', error);
              });
            }
            if (!closeRequested) attrs.onClose();
          },
        }),
      ]);
    },
  };
};
