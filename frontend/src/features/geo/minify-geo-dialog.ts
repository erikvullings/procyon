import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  GeoDocument,
  GeoMinify,
  GeoMinifyPreview,
  GeoTransform,
  Location,
  MapSettings,
} from '../../models';
import { resolvedTheme } from '../../utilities/resolved-theme';
import { DEFAULT_MAP_SETTINGS } from '../map/map-settings';
import { MapView } from '../map/map-view';
import {
  cleanMinify,
  lengthChange,
  type MinifyPreset,
  percentSmaller,
  presetOf,
  presetOptions,
} from './minify-presets';
import { formatDistance } from './track-format';

/** The files to minify; the first one is used for the estimate and the map. */
export interface MinifyGeoRequest {
  readonly files: readonly { readonly location: Location; readonly name: string }[];
}

export interface MinifyGeoDialogAttrs {
  readonly request: MinifyGeoRequest | undefined;
  readonly client: Pick<
    FileManagerClient,
    'previewGeoMinify' | 'readGeoFile' | 'getSettings' | 'mapTileUrlTemplate' | 'mediaUrl'
  >;
  readonly onApply: (transform: GeoTransform) => void;
  readonly onCancel: () => void;
}

function bytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MB`;
}

const DEBOUNCE_MS = 300;

/**
 * "Minify geographic files" (task 0282): presets and options, a live size estimate with the effect
 * on points and track length, and a before/after view on the map. Nothing is written until Apply;
 * replacing the original is an explicit choice that keeps a backup unless it is switched off.
 */
export const MinifyGeoDialog: FactoryComponent<MinifyGeoDialogAttrs> = () => {
  let options: GeoMinify = {};
  let replace = false;
  let backup = true;
  let preview: GeoMinifyPreview | undefined;
  let original: GeoDocument | undefined;
  let failure: string | undefined;
  let token = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let previewedKey = '';
  let wasOpen = false;
  let loadedFor = '';
  let mapSettings: MapSettings = DEFAULT_MAP_SETTINGS;
  let settingsRequested = false;
  let settingsLoaded = false;

  function schedule(attrs: MinifyGeoDialogAttrs): void {
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = undefined;
      const first = attrs.request?.files[0];
      if (first === undefined) return;
      const current = ++token;
      preview = undefined;
      failure = undefined;
      attrs.client
        .previewGeoMinify(first.location, cleanMinify(options))
        .then((result) => {
          if (current !== token) return;
          preview = result;
          failure = result.error ?? undefined;
          m.redraw();
        })
        .catch((error: unknown) => {
          if (current !== token) return;
          failure = error instanceof Error ? error.message : String(error);
          m.redraw();
        });
    }, DEBOUNCE_MS);
  }

  /** The preview map uses the configured basemap, not the built-in default. */
  function loadMapSettings(attrs: MinifyGeoDialogAttrs): void {
    if (settingsRequested) return;
    settingsRequested = true;
    void attrs.client
      .getSettings()
      .then((settings) => {
        mapSettings = settings.map ?? DEFAULT_MAP_SETTINGS;
      })
      .catch(() => undefined)
      .finally(() => {
        settingsLoaded = true;
        m.redraw();
      });
  }

  function loadOriginal(attrs: MinifyGeoDialogAttrs): void {
    const first = attrs.request?.files[0];
    if (first === undefined || loadedFor === first.location.uri) return;
    loadedFor = first.location.uri;
    original = undefined;
    void attrs.client
      .readGeoFile(first.location)
      .then((document) => {
        if (loadedFor === first.location.uri) original = document;
        m.redraw();
      })
      .catch(() => undefined);
  }

  function setOptions(patch: GeoMinify): void {
    options = { ...options, ...patch };
  }

  function number(event: Event): number | undefined {
    const value = (event.currentTarget as HTMLInputElement).value.trim();
    if (value === '') return undefined;
    const parsed = Number(value.replace(',', '.'));
    return Number.isFinite(parsed) ? parsed : undefined;
  }

  function check(
    label: string,
    key: 'dropElevation' | 'dropTime' | 'dropExtensions' | 'removeDuplicates',
  ) {
    return m('label.fm-image-edit-check', [
      m('input', {
        type: 'checkbox',
        checked: options[key] === true,
        onchange: (event: Event) =>
          setOptions({ [key]: (event.currentTarget as HTMLInputElement).checked }),
      }),
      m('span', label),
    ]);
  }

  return {
    onupdate: ({ attrs }) => {
      const open = attrs.request !== undefined;
      const key = `${attrs.request?.files[0]?.location.uri}|${JSON.stringify(cleanMinify(options))}`;
      if (open && (!wasOpen || key !== previewedKey)) {
        previewedKey = key;
        loadMapSettings(attrs);
        loadOriginal(attrs);
        schedule(attrs);
      }
      if (!open) {
        token += 1;
        preview = undefined;
        original = undefined;
        loadedFor = '';
        if (timer !== undefined) clearTimeout(timer);
      }
      if (open && !wasOpen) {
        options = {};
        replace = false;
        backup = true;
      }
      wasOpen = open;
    },
    view: ({ attrs }) => {
      const current = presetOf(options);
      const name = attrs.request?.files[0]?.name ?? '';
      const percent =
        preview === undefined
          ? undefined
          : percentSmaller(preview.originalBytes, preview.minifiedBytes);
      const length = preview === undefined ? undefined : lengthChange(preview);
      return m(ModalPanel, {
        title: t('geoMinify', 'title'),
        className: 'fm-dense-modal fm-geo-minify-modal',
        description:
          attrs.request === undefined
            ? undefined
            : m('.fm-geo-minify', [
                m('.fm-geo-minify-options', [
                  m('p', t('geoConvert', 'files', attrs.request.files.length)),
                  m('label.fm-create-directory-field', [
                    m('span', t('geoMinify', 'preset')),
                    m(
                      'select.browser-default',
                      {
                        value: current,
                        onchange: (event: Event) => {
                          const preset = (event.currentTarget as HTMLSelectElement)
                            .value as MinifyPreset;
                          if (preset !== 'custom') options = presetOptions(preset);
                        },
                      },
                      (
                        [
                          ['whitespace', 'presetWhitespace'],
                          ['web', 'presetWeb'],
                          ['share', 'presetShare'],
                          ['custom', 'presetCustom'],
                        ] as const
                      ).map(([id, label]) =>
                        m('option', { value: id, selected: id === current }, t('geoMinify', label)),
                      ),
                    ),
                  ]),
                  m('.fm-geo-minify-pair', [
                    m('label.fm-create-directory-field', [
                      m('span', t('geoMinify', 'precision')),
                      m('input', {
                        type: 'number',
                        min: 0,
                        max: 15,
                        value: options.precision ?? '',
                        oninput: (event: Event) => {
                          const { precision: _unused, ...rest } = options;
                          const value = number(event);
                          options = value === undefined ? rest : { ...rest, precision: value };
                        },
                      }),
                    ]),
                    m('label.fm-create-directory-field', [
                      m('span', t('geoMinify', 'tolerance')),
                      m('input', {
                        type: 'number',
                        min: 0,
                        step: 0.5,
                        value: options.toleranceMeters ?? '',
                        oninput: (event: Event) => {
                          const { toleranceMeters: _unused, ...rest } = options;
                          const value = number(event);
                          options =
                            value === undefined ? rest : { ...rest, toleranceMeters: value };
                        },
                      }),
                    ]),
                  ]),
                  m('.fm-geo-minify-pair', [
                    check(t('geoMinify', 'dropElevation'), 'dropElevation'),
                    check(t('geoMinify', 'dropTime'), 'dropTime'),
                    check(t('geoMinify', 'dropExtensions'), 'dropExtensions'),
                    check(t('geoMinify', 'removeDuplicates'), 'removeDuplicates'),
                  ]),
                  m('fieldset.fm-geo-minify-output', [
                    m('label', [
                      m('input', {
                        type: 'radio',
                        name: 'minify-output',
                        checked: !replace,
                        onchange: () => {
                          replace = false;
                        },
                      }),
                      m('span', ` ${t('geoMinify', 'outputNew')}`),
                    ]),
                    m('label', [
                      m('input', {
                        type: 'radio',
                        name: 'minify-output',
                        checked: replace,
                        onchange: () => {
                          replace = true;
                        },
                      }),
                      m('span', ` ${t('geoMinify', 'outputReplace')}`),
                    ]),
                    replace
                      ? m('label.fm-image-edit-check', [
                          m('input', {
                            type: 'checkbox',
                            checked: backup,
                            onchange: (event: Event) => {
                              backup = (event.currentTarget as HTMLInputElement).checked;
                            },
                          }),
                          m('span', t('geoMinify', 'keepBackup')),
                        ])
                      : undefined,
                    replace && !backup
                      ? m('p.fm-geo-warning', t('geoMinify', 'noBackupWarning'))
                      : undefined,
                  ]),
                ]),
                m('.fm-geo-minify-result', [
                  m('.fm-geo-minify-estimate', { 'aria-live': 'polite' }, [
                    preview === undefined && failure === undefined
                      ? m('p', t('geoMinify', 'estimating'))
                      : undefined,
                    failure === undefined
                      ? undefined
                      : m(
                          'p.fm-field-error',
                          { role: 'alert' },
                          t('geoMinify', 'previewFailed', { message: failure }),
                        ),
                    preview === undefined || failure !== undefined
                      ? undefined
                      : [
                          m(
                            'p',
                            percent === undefined
                              ? t('geoMinify', 'notSmaller', {
                                  before: bytes(preview.originalBytes),
                                  after: bytes(preview.minifiedBytes),
                                })
                              : t('geoMinify', 'estimate', {
                                  before: bytes(preview.originalBytes),
                                  after: bytes(preview.minifiedBytes),
                                  percent: `${percent}%`,
                                }),
                          ),
                          preview.positionsBefore > 0
                            ? m(
                                'p',
                                t('geoMinify', 'positions', {
                                  before: preview.positionsBefore,
                                  after: preview.positionsAfter,
                                }),
                              )
                            : undefined,
                          length === undefined
                            ? undefined
                            : m(
                                'p',
                                t('geoMinify', 'distance', {
                                  before: formatDistance(preview.distanceBeforeMeters),
                                  after: formatDistance(preview.distanceAfterMeters),
                                  percent: length,
                                }),
                              ),
                          preview.notes.length === 0
                            ? undefined
                            : m(
                                'ul.fm-geo-warning',
                                preview.notes.map((note, index) => m('li', { key: index }, note)),
                              ),
                        ],
                  ]),
                  m('p.fm-geo-hint', t('geoMinify', 'previewOf', { name })),
                  // The map is there from the first frame, so the dialog does not resize when the
                  // preview arrives. It waits for the configured basemap rather than flashing (and
                  // requesting tiles for) the built-in default.
                  m('.fm-geo-minify-map', [
                    settingsLoaded
                      ? m(MapView, {
                          client: attrs.client,
                          settings: mapSettings,
                          dark: resolvedTheme() === 'dark',
                          markers: [],
                          ...(preview?.collection == null
                            ? original === undefined
                              ? {}
                              : { features: original.collection }
                            : { features: preview.collection }),
                          ...(original === undefined ? {} : { comparison: original.collection }),
                          ...(original?.bounds == null ? {} : { dataBounds: original.bounds }),
                          fitKey: loadedFor,
                          ariaLabel: t('geoMinify', 'previewOf', { name }),
                        })
                      : m('.fm-geo-minify-map-placeholder'),
                    preview !== undefined && preview.collection == null
                      ? m('p.fm-geo-hint', t('geoMinify', 'noPreview'))
                      : m('p.fm-geo-hint', t('geoMinify', 'overlayHint')),
                  ]),
                ]),
              ]),
        isOpen: attrs.request !== undefined,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) attrs.onCancel();
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: attrs.onCancel },
          {
            label: t('geoMinify', 'apply'),
            disabled: failure !== undefined,
            onclick: () =>
              attrs.onApply({
                minify: cleanMinify(options),
                ...(replace ? { inPlace: true, backupOriginal: backup } : {}),
              }),
          },
        ],
      });
    },
  };
};
