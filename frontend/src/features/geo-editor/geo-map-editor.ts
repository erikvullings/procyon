import type * as MapLibre from 'maplibre-gl';
import m, { type FactoryComponent } from 'mithril';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { errorText, t } from '../../i18n';
import type { GeoTargetFormat, MapSettings } from '../../models';
import { resolvedTheme } from '../../utilities/resolved-theme';
import { DEFAULT_MAP_SETTINGS } from '../map/map-settings';
import { MapView } from '../map/map-view';
import { createDrawBridge, type DrawBridge } from './draw-bridge';
import { splitFeatures } from './draw-features';
import {
  indexOfId,
  type JsonObject,
  type PropertyType,
  propertiesOf,
  typeOfValue,
  valueText,
} from './geo-edit-model';
import { createGeoEditSession, type DrawTool, type GeoEditSession } from './geo-edit-session';
import './geo-editor.css';

export interface GeoMapEditorAttrs {
  readonly client: Pick<
    FileManagerClient,
    'parseGeoText' | 'convertGeoText' | 'getSettings' | 'mapTileUrlTemplate' | 'mediaUrl'
  >;
  /** The text of the file editor; the map editor reads and writes it. */
  readonly content: string;
  readonly onChange: (content: string) => void;
  /** Name of the file being edited, used to suggest the export name. */
  readonly fileName?: string;
  /** Saves an exported copy; the export button is hidden without it (task 0281). */
  readonly onExport?: (name: string, content: string) => Promise<void>;
}

type SideTab = 'features' | 'properties' | 'json';

const TOOLS: readonly { readonly id: DrawTool; readonly label: string }[] = [
  { id: 'select', label: 'toolSelect' },
  { id: 'point', label: 'toolPoint' },
  { id: 'linestring', label: 'toolLine' },
  { id: 'polygon', label: 'toolPolygon' },
  { id: 'rectangle', label: 'toolRectangle' },
  { id: 'circle', label: 'toolCircle' },
];

const PROPERTY_TYPES: readonly PropertyType[] = ['string', 'number', 'boolean', 'null', 'json'];
const TYPE_LABELS = {
  string: 'typeString',
  number: 'typeNumber',
  boolean: 'typeBoolean',
  null: 'typeNull',
  json: 'typeJson',
} as const;

/** One metre is about 1e-5 degrees of latitude. */
const NUDGE_DEGREES = 0.00001;

function featureLabel(feature: JsonObject, index: number): string {
  const geometry = feature.geometry as { type?: string } | null | undefined;
  const name = propertiesOf(feature).name;
  const base = geometry?.type
    ? t('geoEdit', 'featureLabel', { type: geometry.type, n: index + 1 })
    : t('geoEdit', 'noGeometry');
  return typeof name === 'string' && name !== '' ? `${base} - ${name}` : base;
}

/**
 * GeoJSON editing on the map (task 0278), like geojson.io: draw, select and move shapes, edit
 * their properties and the raw JSON side by side. It edits the text of the file editor, so saving,
 * conflict handling and unsaved-change prompts are the editor's own.
 */
export const GeoMapEditor: FactoryComponent<GeoMapEditorAttrs> = (initial) => {
  let latest: GeoMapEditorAttrs = initial.attrs;
  let mapSettings: MapSettings = DEFAULT_MAP_SETTINGS;
  const session: GeoEditSession = createGeoEditSession({
    getContent: () => latest.content,
    setContent: (text) => {
      // Keep the cached text current, so a re-render before the host updates does not look like
      // an outside change.
      latest = { ...latest, content: text };
      latest.onChange(text);
    },
    client: initial.attrs.client,
    changed: () => m.redraw(),
  });
  let bridge: DrawBridge | undefined;
  let bridgeStarting = false;
  let loadedRevision = -1;
  let tab: SideTab = 'features';
  let newKey = '';
  let exportOpen = false;
  let exportFormat: GeoTargetFormat = 'gpx';
  let exportWkt = false;
  let exportName = '';
  let exportNotes: readonly string[] = [];
  let exportContent: string | undefined;
  let exportError: string | undefined;
  let exportToken = 0;
  let importOpen = false;
  let importText = '';
  let message: string | undefined;
  const propertyErrors = new Map<string, string>();

  function attachBridge(map: MapLibre.Map): void {
    if (bridgeStarting) return;
    bridgeStarting = true;
    void createDrawBridge(map, {
      onDrawn: (geometry) => {
        session.drawn(geometry);
        tab = 'properties';
        m.redraw();
      },
      onChanged: (id, geometry) => {
        session.geometryChanged(id, geometry);
        m.redraw();
      },
      onDeleted: (ids) => {
        for (const id of ids) session.remove(id);
        m.redraw();
      },
      onSelect: (id) => {
        session.select(id);
        if (id !== undefined) tab = 'properties';
        m.redraw();
      },
    })
      .then((created) => {
        bridge = created;
        loadedRevision = -1;
        created.setTool(session.state().tool);
        m.redraw();
      })
      .catch(() => {
        message = t('geoEdit', 'mapUnavailable');
        m.redraw();
      });
  }

  function syncBridge(): void {
    const state = session.state();
    if (bridge === undefined || state.document === undefined || loadedRevision === state.revision) {
      return;
    }
    loadedRevision = state.revision;
    bridge.load(splitFeatures(state.document).editable, state.selectedId);
  }

  function choose(id: string | undefined): void {
    session.select(id);
    bridge?.select(id);
  }

  function toolButton(tool: (typeof TOOLS)[number]): m.Children {
    const active = session.state().tool === tool.id;
    return m(
      'button.fm-geo-editor-tool',
      {
        type: 'button',
        key: tool.id,
        'aria-pressed': String(active),
        class: active ? 'is-active' : undefined,
        onclick: () => {
          session.setTool(tool.id);
          bridge?.setTool(tool.id);
        },
      },
      t('geoEdit', tool.label as 'toolSelect'),
    );
  }

  function featuresTab(): m.Children {
    const { document, selectedId } = session.state();
    if (document === undefined) return undefined;
    if (document.features.length === 0) return m('p.fm-geo-hint', t('geoEdit', 'noFeatures'));
    return m(
      'ul.fm-geo-editor-features',
      {
        role: 'listbox',
        'aria-label': t('geoEdit', 'tabFeatures'),
        onkeydown: (event: KeyboardEvent) => {
          if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
          event.preventDefault();
          const current = indexOfId(document, selectedId);
          const next = Math.min(
            document.features.length - 1,
            Math.max(0, current + (event.key === 'ArrowDown' ? 1 : -1)),
          );
          choose(document.ids[next]);
        },
      },
      document.features.map((feature, index) => {
        const id = document.ids[index] as string;
        return m(
          'li',
          {
            key: id,
            role: 'option',
            tabindex: id === selectedId || (selectedId === undefined && index === 0) ? 0 : -1,
            'aria-selected': String(id === selectedId),
            class: id === selectedId ? 'is-selected' : undefined,
            onclick: () => choose(id),
          },
          featureLabel(feature, index),
        );
      }),
    );
  }

  function propertiesTab(): m.Children {
    const { document, selectedId } = session.state();
    const index = document === undefined ? -1 : indexOfId(document, selectedId);
    if (document === undefined || selectedId === undefined || index < 0) {
      return m('p.fm-geo-hint', t('geoEdit', 'selectFeatureFirst'));
    }
    const properties = propertiesOf(document.features[index]);
    return m('.fm-geo-editor-properties', [
      m(
        'table',
        m(
          'thead',
          m('tr', [
            m('th', t('geoEdit', 'propertyKey')),
            m('th', t('geoEdit', 'propertyType')),
            m('th', t('geoEdit', 'propertyValue')),
            m('th'),
          ]),
        ),
        m(
          'tbody',
          Object.entries(properties).map(([key, value]) => {
            const type = typeOfValue(value);
            const problem = propertyErrors.get(`${selectedId}:${key}`);
            return m('tr', { key: `${selectedId}:${key}` }, [
              m(
                'td',
                m('input', {
                  type: 'text',
                  value: key,
                  'aria-label': t('geoEdit', 'propertyKey'),
                  onchange: (event: Event) =>
                    session.renameProperty(
                      selectedId,
                      key,
                      (event.currentTarget as HTMLInputElement).value.trim(),
                    ),
                }),
              ),
              m(
                'td',
                m(
                  'select.browser-default',
                  {
                    'aria-label': t('geoEdit', 'propertyType'),
                    onchange: (event: Event) => {
                      const next = (event.currentTarget as HTMLSelectElement).value as PropertyType;
                      const text =
                        next === 'boolean' ? 'false' : next === 'number' ? '0' : valueText(value);
                      const error = session.setProperty(selectedId, key, text, next);
                      if (error !== undefined) propertyErrors.set(`${selectedId}:${key}`, error);
                    },
                  },
                  PROPERTY_TYPES.map((option) =>
                    m(
                      'option',
                      { value: option, selected: option === type },
                      t('geoEdit', TYPE_LABELS[option]),
                    ),
                  ),
                ),
              ),
              m('td', [
                m('input', {
                  type: 'text',
                  value: valueText(value),
                  disabled: type === 'null',
                  'aria-label': t('geoEdit', 'propertyValue'),
                  'aria-invalid': problem === undefined ? undefined : 'true',
                  onchange: (event: Event) => {
                    const error = session.setProperty(
                      selectedId,
                      key,
                      (event.currentTarget as HTMLInputElement).value,
                      type,
                    );
                    if (error === undefined) propertyErrors.delete(`${selectedId}:${key}`);
                    else propertyErrors.set(`${selectedId}:${key}`, error);
                  },
                }),
                problem === undefined ? undefined : m('.fm-field-error', problem),
              ]),
              m(
                'td',
                m(
                  'button.btn-flat',
                  {
                    type: 'button',
                    'aria-label': t('geoEdit', 'deleteProperty', { key }),
                    onclick: () => session.deleteProperty(selectedId, key),
                  },
                  '×',
                ),
              ),
            ]);
          }),
        ),
      ),
      m('.fm-geo-editor-add', [
        m('input', {
          type: 'text',
          value: newKey,
          placeholder: t('geoEdit', 'newPropertyName'),
          'aria-label': t('geoEdit', 'newPropertyName'),
          oninput: (event: Event) => {
            newKey = (event.currentTarget as HTMLInputElement).value;
          },
        }),
        m(
          'button.btn-flat',
          {
            type: 'button',
            disabled: newKey.trim() === '' || newKey.trim() in properties,
            onclick: () => {
              session.addProperty(selectedId, newKey.trim());
              newKey = '';
            },
          },
          t('geoEdit', 'addProperty'),
        ),
      ]),
    ]);
  }

  function jsonTab(): m.Children {
    const state = session.state();
    const error = state.rawError;
    return m('.fm-geo-editor-json', [
      m('textarea', {
        spellcheck: false,
        'aria-label': t('geoEdit', 'tabJson'),
        'aria-invalid': error === undefined ? undefined : 'true',
        value: state.rawText,
        oninput: (event: Event) =>
          session.setRaw((event.currentTarget as HTMLTextAreaElement).value),
      }),
      error === undefined
        ? m('p.fm-geo-hint', t('geoEdit', 'jsonValid'))
        : m(
            'p.fm-field-error',
            { role: 'alert' },
            error.line != null && error.column != null
              ? t('geoEdit', 'jsonErrorAt', {
                  line: error.line,
                  column: error.column,
                  message: error.message,
                })
              : error.path != null && error.path !== ''
                ? t('geoEdit', 'jsonErrorPath', { path: error.path, message: error.message })
                : error.message,
          ),
      state.warnings.length === 0
        ? undefined
        : m(
            'ul.fm-geo-warning',
            state.warnings.map((warning, index) => m('li', { key: index }, warning.message)),
          ),
    ]);
  }

  function exportExtension(): string {
    return exportFormat;
  }

  function suggestedExportName(): string {
    const base = (latest.fileName ?? 'features').replace(/\.[^.]+$/, '');
    return `${base}.${exportExtension()}`;
  }

  /** Converts the current text with the chosen format, to show the report before saving. */
  function refreshExport(): void {
    const token = ++exportToken;
    exportContent = undefined;
    exportError = undefined;
    exportNotes = [];
    void latest.client
      .convertGeoText(latest.content, exportFormat, exportWkt)
      .then((result) => {
        if (token !== exportToken) return;
        exportContent = result.content;
        exportNotes = result.notes;
        m.redraw();
      })
      .catch((error: unknown) => {
        if (token !== exportToken) return;
        exportError = errorText(error);
        m.redraw();
      });
  }

  function exportPanel(): m.Children {
    return m('.fm-geo-editor-export', [
      m('label', [
        m('span', t('geoEdit', 'exportFormat')),
        m(
          'select.browser-default',
          {
            'aria-label': t('geoEdit', 'exportFormat'),
            value: exportFormat,
            onchange: (event: Event) => {
              exportFormat = (event.currentTarget as HTMLSelectElement).value as GeoTargetFormat;
              exportName = suggestedExportName();
              refreshExport();
            },
          },
          (['geojson', 'gpx', 'kml', 'csv'] as const).map((id) =>
            m('option', { value: id, selected: id === exportFormat }, id.toUpperCase()),
          ),
        ),
      ]),
      exportFormat === 'csv'
        ? m('label', [
            m('input', {
              type: 'checkbox',
              checked: exportWkt,
              onchange: (event: Event) => {
                exportWkt = (event.currentTarget as HTMLInputElement).checked;
                refreshExport();
              },
            }),
            m('span', t('geoConvert', 'csvWkt')),
          ])
        : undefined,
      m('label', [
        m('span', t('geoConvert', 'exportName')),
        m('input', {
          type: 'text',
          value: exportName,
          oninput: (event: Event) => {
            exportName = (event.currentTarget as HTMLInputElement).value;
          },
        }),
      ]),
      exportError === undefined
        ? exportNotes.length === 0
          ? m(
              'p.fm-geo-hint',
              exportContent === undefined
                ? t('geoConvert', 'checking')
                : t('geoConvert', 'lossless'),
            )
          : [
              m('p.fm-geo-warning', t('geoConvert', 'lossHeading')),
              m(
                'ul',
                exportNotes.map((note, index) => m('li', { key: index }, note)),
              ),
            ]
        : m('p.fm-field-error', { role: 'alert' }, exportError),
      m(
        'button.btn-flat',
        {
          type: 'button',
          disabled: exportContent === undefined || exportName.trim() === '',
          onclick: () => {
            const content = exportContent;
            const name = exportName.trim();
            if (content === undefined || latest.onExport === undefined) return;
            void latest
              .onExport(name, content)
              .then(() => {
                message = undefined;
                exportOpen = false;
                m.redraw();
              })
              .catch((error: unknown) => {
                exportError = t('geoConvert', 'exportFailed', {
                  message: errorText(error),
                });
                m.redraw();
              });
          },
        },
        t('geoConvert', 'exportSave'),
      ),
    ]);
  }

  async function dropped(event: DragEvent): Promise<void> {
    event.preventDefault();
    const file = event.dataTransfer?.files[0];
    const text = file === undefined ? event.dataTransfer?.getData('text/plain') : await file.text();
    if (text === undefined || text === '') return;
    const failure = await session.importText(text);
    message = failure === undefined ? undefined : t('geoEdit', 'dropFailed', { message: failure });
    m.redraw();
  }

  return {
    oncreate: ({ attrs }) => {
      void attrs.client
        .getSettings()
        .then((settings) => {
          mapSettings = settings.map ?? DEFAULT_MAP_SETTINGS;
          m.redraw();
        })
        .catch(() => undefined);
    },
    onupdate: () => {
      session.sync();
      syncBridge();
    },
    onremove: () => {
      bridge?.destroy();
      session.dispose();
    },
    view: ({ attrs }) => {
      latest = attrs;
      session.sync();
      const state = session.state();
      if (state.status !== 'ready' || state.document === undefined) {
        return m(
          '.fm-geo-editor.is-unavailable',
          { role: 'alert' },
          state.status === 'tooLarge'
            ? t('geoEdit', 'tooLarge')
            : t('geoEdit', 'invalid', { reason: state.reason ?? '' }),
        );
      }
      const { readOnly } = splitFeatures(state.document);
      const hasReadOnly = readOnly.length > 0;
      return m(
        '.fm-geo-editor',
        {
          onkeydown: (event: KeyboardEvent) => {
            if (!event.altKey || state.selectedId === undefined) return;
            const step = (event.shiftKey ? 10 : 1) * NUDGE_DEGREES;
            const delta: Record<string, [number, number]> = {
              ArrowLeft: [-step, 0],
              ArrowRight: [step, 0],
              ArrowUp: [0, step],
              ArrowDown: [0, -step],
            };
            const move = delta[event.key];
            if (move === undefined) return;
            event.preventDefault();
            session.nudge(state.selectedId, move[0], move[1]);
          },
        },
        [
          m('.fm-geo-editor-toolbar', { role: 'toolbar', 'aria-label': t('geoEdit', 'mapMode') }, [
            TOOLS.map(toolButton),
            m(
              'button.btn-flat',
              { type: 'button', disabled: !state.canUndo, onclick: () => session.undo() },
              t('geoEdit', 'undo'),
            ),
            m(
              'button.btn-flat',
              { type: 'button', disabled: !state.canRedo, onclick: () => session.redo() },
              t('geoEdit', 'redo'),
            ),
            m(
              'button.btn-flat',
              {
                type: 'button',
                disabled: state.selectedId === undefined,
                onclick: () =>
                  state.selectedId !== undefined && session.duplicate(state.selectedId),
              },
              t('geoEdit', 'duplicate'),
            ),
            m(
              'button.btn-flat',
              {
                type: 'button',
                disabled: state.selectedId === undefined,
                onclick: () => state.selectedId !== undefined && session.remove(state.selectedId),
              },
              t('geoEdit', 'delete'),
            ),
            m(
              'button.btn-flat',
              {
                type: 'button',
                onclick: () => {
                  importOpen = !importOpen;
                },
              },
              t('geoEdit', 'importButton'),
            ),
            m(
              'button.btn-flat',
              { type: 'button', onclick: () => session.reformat(true) },
              t('geoMinify', 'compact'),
            ),
            m(
              'button.btn-flat',
              { type: 'button', onclick: () => session.reformat(false) },
              t('geoMinify', 'pretty'),
            ),
            latest.onExport === undefined
              ? undefined
              : m(
                  'button.btn-flat',
                  {
                    type: 'button',
                    'aria-expanded': String(exportOpen),
                    onclick: () => {
                      exportOpen = !exportOpen;
                      if (exportOpen) {
                        exportName = suggestedExportName();
                        refreshExport();
                      }
                    },
                  },
                  t('geoConvert', 'exportButton'),
                ),
          ]),
          exportOpen ? exportPanel() : undefined,
          importOpen
            ? m('.fm-geo-editor-import', [
                m('p.fm-geo-hint', t('geoEdit', 'importHint')),
                m('textarea', {
                  spellcheck: false,
                  'aria-label': t('geoEdit', 'importTitle'),
                  value: importText,
                  oninput: (event: Event) => {
                    importText = (event.currentTarget as HTMLTextAreaElement).value;
                  },
                }),
                m(
                  'button.btn-flat',
                  {
                    type: 'button',
                    disabled: importText.trim() === '',
                    onclick: () => {
                      void session.importText(importText).then((failure) => {
                        message =
                          failure === undefined
                            ? undefined
                            : t('geoEdit', 'dropFailed', { message: failure });
                        if (failure === undefined) {
                          importText = '';
                          importOpen = false;
                        }
                        m.redraw();
                      });
                    },
                  },
                  t('geoEdit', 'importApply'),
                ),
              ])
            : undefined,
          message === undefined ? undefined : m('p.fm-field-error', { role: 'alert' }, message),
          state.wrapped ? m('p.fm-geo-hint', t('geoEdit', 'wrapped')) : undefined,
          m('.fm-geo-editor-body', [
            m(
              '.fm-geo-editor-map',
              {
                ondragover: (event: DragEvent) => event.preventDefault(),
                ondrop: (event: DragEvent) => void dropped(event),
              },
              m(MapView, {
                client: attrs.client,
                settings: mapSettings,
                dark: resolvedTheme() === 'dark',
                markers: [],
                features: { type: 'FeatureCollection', features: readOnly as never },
                fitKey: state.document.features.length === 0 ? 0 : 1,
                ariaLabel: t('geoEdit', 'mapMode'),
                onMapReady: attachBridge,
                onMapGone: () => {
                  bridge?.destroy();
                  bridge = undefined;
                  bridgeStarting = false;
                },
              }),
            ),
            m('aside.fm-geo-editor-side', [
              m(
                '.fm-geo-editor-tabs',
                { role: 'tablist' },
                (['features', 'properties', 'json'] as const).map((id) =>
                  m(
                    'button.btn-flat',
                    {
                      key: id,
                      type: 'button',
                      role: 'tab',
                      'aria-selected': String(tab === id),
                      onclick: () => {
                        tab = id;
                      },
                    },
                    t(
                      'geoEdit',
                      id === 'features'
                        ? 'tabFeatures'
                        : id === 'properties'
                          ? 'tabProperties'
                          : 'tabJson',
                    ),
                  ),
                ),
              ),
              tab === 'features'
                ? featuresTab()
                : tab === 'properties'
                  ? propertiesTab()
                  : jsonTab(),
              hasReadOnly ? m('p.fm-geo-hint', t('geoEdit', 'readOnlyShapes')) : undefined,
              m('p.fm-geo-hint', t('geoEdit', 'mapHelp')),
            ]),
          ]),
        ],
      );
    },
  };
};
