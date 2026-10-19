import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { GeoConversionReport, GeoTargetFormat, GeoTransform, Location } from '../../models';

/** The files to convert, with the names shown to the user. */
export interface ConvertGeoRequest {
  readonly files: readonly { readonly location: Location; readonly name: string }[];
}

export interface ConvertGeoDialogAttrs {
  readonly request: ConvertGeoRequest | undefined;
  readonly client: Pick<FileManagerClient, 'previewGeoConversion'>;
  readonly onConvert: (transform: GeoTransform) => void;
  readonly onCancel: () => void;
}

const FORMATS: readonly { readonly id: GeoTargetFormat; readonly label: string }[] = [
  { id: 'geojson', label: 'formatGeojson' },
  { id: 'gpx', label: 'formatGpx' },
  { id: 'kml', label: 'formatKml' },
  { id: 'kmz', label: 'formatKmz' },
  { id: 'csv', label: 'formatCsv' },
];

/** Whether the user may continue: at least one file converts. */
export function canConvert(reports: readonly GeoConversionReport[] | undefined): boolean {
  return reports !== undefined && reports.some((report) => report.error == null);
}

/**
 * "Convert geographic files" (task 0281): choose the format, see per file what would not be
 * represented in the new file, then continue or cancel before anything is written.
 */
export const ConvertGeoDialog: FactoryComponent<ConvertGeoDialogAttrs> = () => {
  let format: GeoTargetFormat = 'geojson';
  let csvWkt = false;
  let reports: readonly GeoConversionReport[] | undefined;
  let failure: string | undefined;
  let token = 0;
  let previewedKey = '';
  let wasOpen = false;

  function preview(attrs: ConvertGeoDialogAttrs): void {
    const request = attrs.request;
    if (request === undefined) return;
    const current = ++token;
    reports = undefined;
    failure = undefined;
    attrs.client
      .previewGeoConversion(
        request.files.map((file) => file.location),
        format,
        csvWkt,
      )
      .then((result) => {
        if (current !== token) return;
        reports = result;
        m.redraw();
      })
      .catch((error: unknown) => {
        if (current !== token) return;
        failure = error instanceof Error ? error.message : String(error);
        m.redraw();
      });
  }

  function nameOf(attrs: ConvertGeoDialogAttrs, report: GeoConversionReport): string {
    return (
      attrs.request?.files.find((file) => file.location.uri === report.location.uri)?.name ??
      report.location.uri
    );
  }

  return {
    onupdate: ({ attrs }) => {
      const open = attrs.request !== undefined;
      const key = `${open}|${format}|${csvWkt}|${attrs.request?.files.map((f) => f.location.uri).join(',')}`;
      if (open && (!wasOpen || key !== previewedKey)) {
        previewedKey = key;
        preview(attrs);
      }
      if (!open) {
        token += 1;
        reports = undefined;
      }
      wasOpen = open;
    },
    view: ({ attrs }) => {
      const loading = attrs.request !== undefined && reports === undefined && failure === undefined;
      return m(ModalPanel, {
        title: t('geoConvert', 'title'),
        className: 'fm-dense-modal',
        description:
          attrs.request === undefined
            ? undefined
            : m('.fm-geo-convert', [
                m('p', t('geoConvert', 'files', { n: attrs.request.files.length })),
                m('label.fm-create-directory-field', [
                  m('span', t('geoConvert', 'targetLabel')),
                  m(
                    'select.browser-default',
                    {
                      value: format,
                      onchange: (event: Event) => {
                        format = (event.currentTarget as HTMLSelectElement)
                          .value as GeoTargetFormat;
                      },
                    },
                    FORMATS.map((option) =>
                      m(
                        'option',
                        { value: option.id, selected: option.id === format },
                        t('geoConvert', option.label as 'formatGeojson'),
                      ),
                    ),
                  ),
                ]),
                format === 'csv'
                  ? m('label.fm-image-edit-check', [
                      m('input', {
                        type: 'checkbox',
                        checked: csvWkt,
                        onchange: (event: Event) => {
                          csvWkt = (event.currentTarget as HTMLInputElement).checked;
                        },
                      }),
                      m('span', t('geoConvert', 'csvWkt')),
                    ])
                  : undefined,
                m('p.fm-geo-hint', t('geoConvert', 'unchanged')),
                loading ? m('p', { role: 'status' }, t('geoConvert', 'checking')) : undefined,
                failure === undefined
                  ? undefined
                  : m(
                      'p.fm-field-error',
                      { role: 'alert' },
                      t('geoConvert', 'previewFailed', { message: failure }),
                    ),
                reports?.map((report) =>
                  m('.fm-geo-convert-report', { key: report.location.uri }, [
                    report.error != null
                      ? m(
                          'p.fm-field-error',
                          t('geoConvert', 'unreadable', {
                            source: nameOf(attrs, report),
                            message: report.error,
                          }),
                        )
                      : [
                          m(
                            'p',
                            m(
                              'strong',
                              t('geoConvert', 'writes', {
                                source: nameOf(attrs, report),
                                output: report.outputName ?? '',
                              }),
                            ),
                          ),
                          report.notes.length === 0
                            ? m('p.fm-geo-hint', t('geoConvert', 'lossless'))
                            : [
                                m('p.fm-geo-warning', t('geoConvert', 'lossHeading')),
                                m(
                                  'ul',
                                  report.notes.map((note, index) => m('li', { key: index }, note)),
                                ),
                              ],
                        ],
                  ]),
                ),
              ]),
        isOpen: attrs.request !== undefined,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) attrs.onCancel();
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: attrs.onCancel },
          {
            label: t('geoConvert', 'convert'),
            disabled: !canConvert(reports),
            onclick: () =>
              attrs.onConvert({ targetFormat: format, csvWkt: format === 'csv' && csvWkt }),
          },
        ],
      });
    },
  };
};
