import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type { CsvGeoInfo, CsvGeoOptions } from '../../models';

export interface CsvGeoDialogAttrs {
  readonly open: boolean;
  readonly info: CsvGeoInfo;
  readonly onApply: (options: CsvGeoOptions) => void;
  readonly onCancel: () => void;
}

/** Coordinate systems offered as suggestions; any other EPSG code can be typed. */
const CRS_SUGGESTIONS: readonly (readonly [string, string])[] = [
  ['EPSG:4326', 'WGS 84'],
  ['EPSG:3857', 'Web Mercator'],
  ['EPSG:4258', 'ETRS89'],
  ['EPSG:25832', 'ETRS89 / UTM 32N'],
  ['EPSG:28992', 'Amersfoort / RD New'],
  ['EPSG:27700', 'OSGB36 / British National Grid'],
  ['EPSG:2154', 'RGF93 / Lambert-93'],
  ['EPSG:32618', 'WGS 84 / UTM 18N'],
];

/**
 * "Columns and coordinate system" of a CSV file on the map: which columns hold the position (or a
 * WKT geometry) and which coordinate system they are in. Any EPSG code works.
 */
export const CsvGeoDialog: FactoryComponent<CsvGeoDialogAttrs> = () => {
  let xColumn = '';
  let yColumn = '';
  let wktColumn = '';
  let crs = '';
  let wasOpen = false;

  function column(
    label: string,
    value: string,
    onChange: (next: string) => void,
    headers: readonly string[],
  ): m.Vnode {
    return m('label.fm-create-directory-field', [
      m('span', label),
      m(
        'select.browser-default',
        {
          value,
          onchange: (event: Event) => onChange((event.currentTarget as HTMLSelectElement).value),
        },
        [
          m('option', { value: '', selected: value === '' }, t('geoCsv', 'none')),
          ...headers.map((header) =>
            m('option', { value: header, selected: header === value }, header),
          ),
        ],
      ),
    ]);
  }

  return {
    view: ({ attrs }) => {
      const { info } = attrs;
      if (attrs.open && !wasOpen) {
        xColumn = info.xColumn ?? '';
        yColumn = info.yColumn ?? '';
        wktColumn = info.wktColumn ?? '';
        crs = info.crs ?? '';
      }
      wasOpen = attrs.open;
      const usesWkt = wktColumn !== '';
      return m(ModalPanel, {
        title: t('geoCsv', 'title'),
        className: 'fm-dense-modal',
        description: m('.fm-geo-csv-dialog', [
          column(t('geoCsv', 'xColumn'), xColumn, (next) => (xColumn = next), info.headers),
          column(t('geoCsv', 'yColumn'), yColumn, (next) => (yColumn = next), info.headers),
          column(t('geoCsv', 'wktColumn'), wktColumn, (next) => (wktColumn = next), info.headers),
          usesWkt ? m('p.fm-geo-hint', t('geoCsv', 'wktHint')) : undefined,
          m('label.fm-create-directory-field', [
            m('span', t('geoCsv', 'crs')),
            m('input', {
              type: 'text',
              list: 'fm-csv-crs-suggestions',
              placeholder: 'EPSG:4326',
              value: crs,
              oninput: (event: Event) => {
                crs = (event.currentTarget as HTMLInputElement).value;
              },
            }),
            m(
              'datalist#fm-csv-crs-suggestions',
              CRS_SUGGESTIONS.map(([code, name]) => m('option', { value: code }, name)),
            ),
          ]),
          m('p.fm-geo-hint', t('geoCsv', 'crsHint')),
        ]),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) attrs.onCancel();
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: attrs.onCancel },
          {
            label: t('geoCsv', 'apply'),
            onclick: () =>
              attrs.onApply({
                ...(xColumn === '' ? {} : { xColumn }),
                ...(yColumn === '' ? {} : { yColumn }),
                ...(wktColumn === '' ? {} : { wktColumn }),
                ...(crs.trim() === '' ? {} : { crs: crs.trim() }),
              }),
          },
        ],
      });
    },
  };
};
