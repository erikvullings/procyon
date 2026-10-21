import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type { EntrySummary, MapSettings } from '../../models';
import { MapView, type MapViewClient } from '../map/map-view';
import type { MapMarker } from '../map/marker-index';
import { SidePanelResizer, sidePanelStyle } from '../map/side-panel-width';
import { canWriteGps, type GeoImage } from './gps-scan';
import type { ImageMapState } from './image-map-state';
import { thumbnailImage } from './thumbnail-image';

export interface ImageMapPanelAttrs {
  readonly state: ImageMapState;
  readonly client: MapViewClient & Pick<FileManagerClient, 'getThumbnail'>;
  readonly settings: MapSettings;
  readonly dark: boolean;
  /** Entry ids selected in the active pane; their markers are highlighted. */
  readonly selectedIds: ReadonlySet<string>;
  readonly onReveal: (entry: EntrySummary) => void;
  readonly onRecursive: (recursive: boolean) => void;
  readonly onCancel: () => void;
  readonly onBeginPlacing: (entry: EntrySummary) => void;
  readonly onStopPlacing: () => void;
  readonly onPropose: (entry: EntrySummary, latitude: number, longitude: number) => void;
  readonly onConfirm: () => void;
  readonly onCancelPlacement: () => void;
}

function formatDate(takenAt: string | undefined): string | undefined {
  if (takenAt === undefined) return undefined;
  const date = new Date(takenAt);
  return Number.isNaN(date.getTime()) ? takenAt : date.toLocaleString();
}

function coordinateText(latitude: number, longitude: number): string {
  return `${latitude.toFixed(5)}, ${longitude.toFixed(5)}`;
}

/** Markers for the located images; the entry id doubles as the marker id. */
export function markersOf(located: readonly GeoImage[]): MapMarker[] {
  return located.map((image) => ({
    id: image.entry.id,
    latitude: image.latitude,
    longitude: image.longitude,
    title: image.entry.name,
    draggable: canWriteGps(image.entry),
  }));
}

/** Images without a location, one row each like the file panes, in a collapsible left panel. */
function unlocatedPanel(attrs: ImageMapPanelAttrs, open: boolean, toggle: () => void): m.Children {
  const { unlocated } = attrs.state;
  if (unlocated.length === 0) return undefined;
  const label = open ? t('imageMap', 'collapseUnlocated') : t('imageMap', 'expandUnlocated');
  const panel = m(
    '.fm-image-map-side',
    { class: open ? undefined : 'is-collapsed', ...(open ? { style: sidePanelStyle() } : {}) },
    [
      m('.fm-image-map-side-head', [
        open
          ? m(
              'span.fm-image-map-side-title',
              `${t('imageMap', 'noLocation')} (${unlocated.length})`,
            )
          : undefined,
        tooltip(
          label,
          m(
            'button.fm-geo-panel-button',
            { type: 'button', 'aria-label': label, 'aria-expanded': String(open), onclick: toggle },
            open ? '«' : `» ${unlocated.length}`,
          ),
        ),
      ]),
      open
        ? m(
            '.fm-image-map-side-rows',
            unlocated.map((entry) =>
              m('.fm-geo-row.fm-image-map-row', { key: entry.id }, [
                m(
                  'button.fm-image-map-reveal',
                  { type: 'button', title: entry.name, onclick: () => attrs.onReveal(entry) },
                  entry.name,
                ),
                canWriteGps(entry)
                  ? m(
                      'button.fm-image-map-place',
                      { type: 'button', onclick: () => attrs.onBeginPlacing(entry) },
                      t('imageMap', 'place'),
                    )
                  : undefined,
              ]),
            ),
          )
        : undefined,
    ],
  );
  return [panel, open ? m(SidePanelResizer) : undefined];
}

/**
 * "Show on map" panel (task 0276): thumbnail markers with clustering and popups, the images
 * without GPS listed beside the map, and confirmed drag-or-click placement through the metadata
 * editor.
 */
export const ImageMapPanel: FactoryComponent<ImageMapPanelAttrs> = () => {
  // Memoised so the map only rebuilds its marker index when the located images change.
  let lastLocated: readonly GeoImage[] | undefined;
  let markers: MapMarker[] = [];
  let fitKey = 0;
  let sideOpen = true;

  return {
    view: ({ attrs }) => {
      const { state } = attrs;
      if (!state.open) return undefined;
      if (state.located !== lastLocated) {
        // A re-published identical list (after a rejected drag) still resets the markers.
        lastLocated = state.located;
        markers = markersOf(state.located);
        if (!state.scanning) fitKey += 1;
      }
      const byId = new Map(state.located.map((image) => [image.entry.id, image]));
      return m('.fm-image-map', [
        state.failure === undefined
          ? undefined
          : m(
              'p.fm-field-error',
              { role: 'alert' },
              t('imageMap', 'failed', { message: state.failure }),
            ),
        state.placing === undefined
          ? undefined
          : m('.fm-image-map-placing', [
              m('span', t('imageMap', 'placingHint', { name: state.placing.name })),
              m(
                'button.btn-flat',
                { type: 'button', onclick: attrs.onStopPlacing },
                t('imageMap', 'stopPlacing'),
              ),
            ]),
        !state.scanning && state.total === 0 && state.failure === undefined
          ? m('p', t('imageMap', 'noImages'))
          : m('.fm-image-map-body', [
              unlocatedPanel(attrs, sideOpen, () => (sideOpen = !sideOpen)),
              m('.fm-image-map-canvas', [
                m(MapView, {
                  client: attrs.client,
                  settings: attrs.settings,
                  dark: attrs.dark,
                  markers,
                  selectedIds: attrs.selectedIds,
                  fitKey,
                  ariaLabel: t('imageMap', 'title'),
                  renderMarker: (marker) => {
                    const image = byId.get(marker.id);
                    const element = document.createElement('button');
                    element.type = 'button';
                    element.className = 'fm-map-marker fm-image-map-marker';
                    element.setAttribute('aria-label', marker.title ?? marker.id);
                    if (image !== undefined) {
                      element.append(
                        thumbnailImage(attrs.client, image.entry, 'small', 'fm-image-map-thumb'),
                      );
                    }
                    return element;
                  },
                  renderPopup: (marker) => {
                    const image = byId.get(marker.id);
                    if (image === undefined) return undefined;
                    const root = document.createElement('div');
                    root.className = 'fm-image-map-popup';
                    root.append(
                      thumbnailImage(attrs.client, image.entry, 'large', 'fm-image-map-preview'),
                    );
                    const name = document.createElement('strong');
                    name.textContent = image.entry.name;
                    root.append(name);
                    const date = formatDate(image.takenAt);
                    if (date !== undefined) {
                      const line = document.createElement('div');
                      line.textContent = t('imageMap', 'taken', { date });
                      root.append(line);
                    }
                    const where = document.createElement('div');
                    where.textContent = coordinateText(image.latitude, image.longitude);
                    root.append(where);
                    return root;
                  },
                  onMarkerClick: (marker) => {
                    const image = byId.get(marker.id);
                    if (image !== undefined) attrs.onReveal(image.entry);
                  },
                  onMarkerDrag: (marker, position) => {
                    const image = byId.get(marker.id);
                    if (image !== undefined) {
                      attrs.onPropose(image.entry, position.latitude, position.longitude);
                    }
                  },
                  onMapClick: (position) => {
                    if (state.placing !== undefined) {
                      attrs.onPropose(state.placing, position.latitude, position.longitude);
                    }
                  },
                }),
              ]),
            ]),
        m('.fm-image-map-status', [
          m('strong.fm-image-map-title', t('imageMap', 'title')),
          state.scanning
            ? m(
                'span',
                { role: 'status' },
                t('imageMap', 'scanning', { done: state.done, total: state.total }),
              )
            : m(
                'span',
                t('imageMap', 'summary', {
                  located: state.located.length,
                  unlocated: state.unlocated.length,
                }),
              ),
          state.unreadable.length > 0
            ? m(
                'span.fm-image-map-warning',
                t('imageMap', 'unreadable', { n: state.unreadable.length }),
              )
            : undefined,
          state.truncated
            ? m('span.fm-image-map-warning', t('imageMap', 'truncated', { n: state.total }))
            : undefined,
          state.foldersOffered
            ? m('label.fm-image-map-recursive', [
                m('input', {
                  type: 'checkbox',
                  checked: state.recursive,
                  onchange: (event: Event) =>
                    attrs.onRecursive((event.currentTarget as HTMLInputElement).checked),
                }),
                m('span', t('imageMap', 'includeSubfolders')),
              ])
            : undefined,
          state.scanning
            ? m(
                'button.btn-flat',
                { type: 'button', onclick: attrs.onCancel },
                t('imageMap', 'cancel'),
              )
            : undefined,
        ]),
        m(ModalPanel, {
          title: t('imageMap', 'confirmTitle'),
          className: 'fm-dense-modal',
          description:
            state.pending === undefined
              ? undefined
              : m(
                  'p',
                  t('imageMap', 'confirmText', {
                    name: state.pending.entry.name,
                    latitude: state.pending.latitude.toFixed(5),
                    longitude: state.pending.longitude.toFixed(5),
                  }),
                ),
          isOpen: state.pending !== undefined,
          closeOnEsc: true,
          onToggle: (open: boolean) => {
            if (!open && state.pending !== undefined) attrs.onCancelPlacement();
          },
          buttons: [
            { label: t('button', 'cancel'), onclick: attrs.onCancelPlacement },
            { label: t('imageMap', 'confirm'), onclick: attrs.onConfirm },
          ],
        }),
      ]);
    },
  };
};
