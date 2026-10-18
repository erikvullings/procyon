import m, { type FactoryComponent } from 'mithril';

import { t } from '../../i18n';
import type { SqliteCell, SqliteFilterOperator, SqliteObject } from '../../models';
import {
  type FileViewerSqliteContent,
  SQLITE_PAGE_SIZE,
  type SqliteResultState,
  type SqliteSort,
  type SqliteTab,
  selectedSqliteObject,
} from './sqlite-browser';

export interface SqliteBrowserActions {
  readonly onTab: (tab: SqliteTab) => void;
  readonly onSelectObject: (name: string) => void;
  readonly onBrowse: (patch: {
    readonly search?: string;
    readonly filters?: FileViewerSqliteContent['filters'];
    readonly sort?: SqliteSort | undefined;
  }) => void;
  readonly onPage: (slot: 'data' | 'sqlRun', offset: number) => void;
  readonly onSql: (sql: string) => void;
  readonly onRunSql: () => void;
}

export interface SqliteBrowserAttrs {
  readonly content: FileViewerSqliteContent;
  readonly actions: SqliteBrowserActions;
}

const FILTER_OPERATORS = [
  { id: 'contains', key: 'sqliteOpContains' },
  { id: 'equals', key: 'sqliteOpEquals' },
  { id: 'notEquals', key: 'sqliteOpNotEquals' },
  { id: 'greater', key: 'sqliteOpGreater' },
  { id: 'less', key: 'sqliteOpLess' },
  { id: 'isNull', key: 'sqliteOpIsNull' },
  { id: 'isNotNull', key: 'sqliteOpIsNotNull' },
] as const satisfies readonly { readonly id: SqliteFilterOperator; readonly key: string }[];

const TABS = [
  { id: 'data', key: 'sqliteTabData' },
  { id: 'structure', key: 'sqliteTabStructure' },
  { id: 'sql', key: 'sqliteTabSql' },
] as const satisfies readonly { readonly id: SqliteTab; readonly key: string }[];

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KiB', 'MiB', 'GiB', 'TiB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

/** NULL, blobs and numbers are visually distinct from text so an empty string never reads as NULL. */
function renderCell(cell: SqliteCell): m.Vnode {
  switch (cell.valueType) {
    case 'null':
      return m('td.fm-sqlite-null', 'NULL');
    case 'integer':
    case 'real':
      return m('td.fm-sqlite-number', cell.text);
    case 'blob':
      return m(
        'td.fm-sqlite-blob',
        { title: t('viewer', 'sqliteBlobBytes', { n: String(cell.byteLength ?? 0) }) },
        `${cell.text ?? ''}${cell.truncated === true ? ' …' : ''}`,
      );
    case 'text':
      return m(
        'td',
        cell.truncated === true
          ? { title: t('viewer', 'sqliteBlobBytes', { n: String(cell.byteLength ?? 0) }) }
          : {},
        `${cell.text ?? ''}${cell.truncated === true ? '…' : ''}`,
      );
  }
}

function renderPager(
  slot: 'data' | 'sqlRun',
  state: SqliteResultState,
  actions: SqliteBrowserActions,
): m.Children {
  const result = state.result;
  if (result === undefined) return undefined;
  const count = result.rows.length;
  const from = count === 0 ? 0 : state.offset + 1;
  const to = state.offset + count;
  const label =
    count === 0
      ? t('viewer', 'sqliteNoRows')
      : state.totalRows !== undefined
        ? t('viewer', 'sqliteRowsRange', {
            from: from.toLocaleString(),
            to: to.toLocaleString(),
            total: state.totalRows.toLocaleString(),
          })
        : t('viewer', 'sqliteRowsRangeOpen', {
            from: from.toLocaleString(),
            to: to.toLocaleString(),
          });
  return m('.fm-sqlite-pager', [
    m('span.fm-sqlite-pager-label', { role: 'status' }, label),
    result.truncated ? m('span.fm-sqlite-pager-note', t('viewer', 'sqliteResultCut')) : undefined,
    m('span.fm-sqlite-pager-time', t('viewer', 'sqliteElapsed', { ms: String(result.elapsedMs) })),
    m(
      'button.fm-sqlite-button',
      {
        type: 'button',
        disabled: state.offset === 0 || state.loading,
        onclick: () => actions.onPage(slot, Math.max(0, state.offset - SQLITE_PAGE_SIZE)),
      },
      t('viewer', 'sqlitePreviousPage'),
    ),
    m(
      'button.fm-sqlite-button',
      {
        type: 'button',
        disabled: !result.hasMore || state.loading,
        // Advance by the rows actually shown: a page cut short by the size budget is shorter
        // than SQLITE_PAGE_SIZE, and skipping ahead a full page would silently drop rows.
        onclick: () => actions.onPage(slot, state.offset + count),
      },
      t('viewer', 'sqliteNextPage'),
    ),
  ]);
}

function renderGrid(
  state: SqliteResultState,
  sort: SqliteSort | undefined,
  onSort: ((column: string) => void) | undefined,
): m.Children {
  const result = state.result;
  if (state.error !== undefined) {
    return m('.fm-sqlite-error', { role: 'alert' }, state.error);
  }
  if (result === undefined) {
    return state.loading
      ? m('.fm-sqlite-empty', { role: 'status' }, m('span.fm-file-viewer-loading-spinner'))
      : m('.fm-sqlite-empty', t('viewer', 'sqliteNoResultYet'));
  }
  return m(
    '.fm-sqlite-grid-scroll',
    { class: state.loading ? 'is-loading' : '' },
    m('table.fm-sqlite-grid', [
      m(
        'thead',
        m(
          'tr',
          result.columns.map((column) => {
            const active = sort?.column === column;
            return m(
              'th',
              {
                scope: 'col',
                'aria-sort': active ? (sort.descending ? 'descending' : 'ascending') : undefined,
              },
              onSort === undefined
                ? column
                : m(
                    'button.fm-sqlite-sort',
                    {
                      type: 'button',
                      title: t('viewer', 'sqliteSortBy', { column }),
                      onclick: () => onSort(column),
                    },
                    [column, active ? (sort.descending ? ' ▼' : ' ▲') : undefined],
                  ),
            );
          }),
        ),
      ),
      m(
        'tbody',
        result.rows.map((row, index) =>
          m(
            'tr',
            { key: state.offset + index },
            row.map((cell) => renderCell(cell)),
          ),
        ),
      ),
    ]),
  );
}

function nextSort(current: SqliteSort | undefined, column: string): SqliteSort | undefined {
  if (current?.column !== column) return { column, descending: false };
  return current.descending ? undefined : { column, descending: true };
}

function renderDataTab(content: FileViewerSqliteContent, actions: SqliteBrowserActions) {
  const object = selectedSqliteObject(content);
  const columns = object?.columns ?? [];
  return m('.fm-sqlite-pane', [
    m('.fm-sqlite-toolbar', [
      m('input.fm-sqlite-search.browser-default', {
        type: 'search',
        value: content.search,
        placeholder: t('viewer', 'sqliteSearchPlaceholder'),
        'aria-label': t('viewer', 'sqliteSearchPlaceholder'),
        oninput: (event: InputEvent) =>
          actions.onBrowse({ search: (event.currentTarget as HTMLInputElement).value }),
      }),
      m(
        'button.fm-sqlite-button',
        {
          type: 'button',
          disabled: columns.length === 0,
          onclick: () =>
            actions.onBrowse({
              filters: [
                ...content.filters,
                { column: columns[0]?.name ?? '', operator: 'contains', value: '' },
              ],
            }),
        },
        t('viewer', 'sqliteAddFilter'),
      ),
    ]),
    content.filters.length === 0
      ? undefined
      : m(
          '.fm-sqlite-filters',
          content.filters.map((filter, index) => {
            const replace = (next: Partial<typeof filter>) =>
              actions.onBrowse({
                filters: content.filters.map((candidate, position) =>
                  position === index ? { ...candidate, ...next } : candidate,
                ),
              });
            const needsValue = filter.operator !== 'isNull' && filter.operator !== 'isNotNull';
            return m('.fm-sqlite-filter', { key: index }, [
              m(
                'select.fm-sqlite-select.browser-default',
                {
                  'aria-label': t('viewer', 'sqliteFilterColumn'),
                  value: filter.column,
                  onchange: (event: Event) =>
                    replace({ column: (event.currentTarget as HTMLSelectElement).value }),
                },
                columns.map((column) =>
                  m(
                    'option',
                    { value: column.name, selected: column.name === filter.column },
                    column.name,
                  ),
                ),
              ),
              m(
                'select.fm-sqlite-select.browser-default',
                {
                  'aria-label': t('viewer', 'sqliteFilterColumn'),
                  value: filter.operator,
                  onchange: (event: Event) =>
                    replace({
                      operator: (event.currentTarget as HTMLSelectElement)
                        .value as SqliteFilterOperator,
                    }),
                },
                FILTER_OPERATORS.map((operator) =>
                  m(
                    'option',
                    { value: operator.id, selected: operator.id === filter.operator },
                    t('viewer', operator.key),
                  ),
                ),
              ),
              needsValue
                ? m('input.fm-sqlite-filter-value.browser-default', {
                    type: 'text',
                    value: filter.value ?? '',
                    placeholder: t('viewer', 'sqliteFilterValue'),
                    'aria-label': t('viewer', 'sqliteFilterValue'),
                    oninput: (event: InputEvent) =>
                      replace({ value: (event.currentTarget as HTMLInputElement).value }),
                  })
                : undefined,
              m(
                'button.fm-sqlite-button',
                {
                  type: 'button',
                  title: t('viewer', 'sqliteRemoveFilter'),
                  'aria-label': t('viewer', 'sqliteRemoveFilter'),
                  onclick: () =>
                    actions.onBrowse({
                      filters: content.filters.filter((_, position) => position !== index),
                    }),
                },
                '×',
              ),
            ]);
          }),
        ),
    renderGrid(content.data, content.sort, (column) =>
      actions.onBrowse({ sort: nextSort(content.sort, column) }),
    ),
    renderPager('data', content.data, actions),
  ]);
}

function renderStructureTab(object: SqliteObject | undefined): m.Children {
  if (object === undefined) return m('.fm-sqlite-empty', t('viewer', 'sqliteEmptyDatabase'));
  return m('.fm-sqlite-pane.fm-sqlite-structure', [
    m('.fm-sqlite-grid-scroll', [
      m('table.fm-sqlite-grid', [
        m(
          'thead',
          m('tr', [
            m('th', t('viewer', 'sqliteColName')),
            m('th', t('viewer', 'sqliteColType')),
            m('th', t('viewer', 'sqliteColNotNull')),
            m('th', t('viewer', 'sqliteColPrimaryKey')),
          ]),
        ),
        m(
          'tbody',
          object.columns.map((column) =>
            m('tr', { key: column.name }, [
              m('td', column.name),
              m('td', column.declaredType),
              m('td', column.notNull ? '✓' : ''),
              m('td', column.primaryKey ? '✓' : ''),
            ]),
          ),
        ),
      ]),
    ]),
    object.createSql != null
      ? m('section.fm-sqlite-definition', [
          m('h3', t('viewer', 'sqliteCreateSql')),
          m('pre', object.createSql),
        ])
      : undefined,
  ]);
}

function renderSqlTab(content: FileViewerSqliteContent, actions: SqliteBrowserActions) {
  return m('.fm-sqlite-pane', [
    m('.fm-sqlite-console', [
      m('textarea.fm-sqlite-sql.browser-default', {
        value: content.sql,
        spellcheck: false,
        rows: 4,
        placeholder: t('viewer', 'sqliteSqlPlaceholder'),
        'aria-label': t('viewer', 'sqliteTabSql'),
        oninput: (event: InputEvent) =>
          actions.onSql((event.currentTarget as HTMLTextAreaElement).value),
        onkeydown: (event: KeyboardEvent) => {
          if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
            event.preventDefault();
            actions.onRunSql();
          }
        },
      }),
      m('.fm-sqlite-console-actions', [
        m(
          'button.fm-sqlite-button.fm-sqlite-run',
          { type: 'button', disabled: content.sqlRun.loading, onclick: actions.onRunSql },
          `${t('viewer', 'sqliteRun')} (Ctrl+Enter)`,
        ),
        m('span.fm-sqlite-note', t('viewer', 'sqliteReadOnlyNote')),
      ]),
    ]),
    renderGrid(content.sqlRun, undefined, undefined),
    renderPager('sqlRun', content.sqlRun, actions),
  ]);
}

/** Read-only SQLite database browser: table list, data grid with search/filters/sort, structure
 * and a SQL console (task 0251). Stateless: everything lives in the viewer controller. */
export const SqliteBrowser: FactoryComponent<SqliteBrowserAttrs> = () => ({
  view: ({ attrs: { content, actions } }) => {
    const object = selectedSqliteObject(content);
    return m('.fm-sqlite', [
      content.warning !== undefined
        ? m('.fm-sqlite-warning', { role: 'note' }, content.warning)
        : undefined,
      m('.fm-sqlite-layout', [
        m(
          'nav.fm-sqlite-objects',
          { 'aria-label': t('viewer', 'sqliteTables') },
          content.objects.length === 0
            ? m('.fm-sqlite-empty', t('viewer', 'sqliteEmptyDatabase'))
            : content.objects.map((candidate) =>
                m(
                  'button.fm-sqlite-object',
                  {
                    key: candidate.name,
                    type: 'button',
                    class: candidate.name === content.selected ? 'active' : '',
                    'aria-current': candidate.name === content.selected ? 'true' : undefined,
                    title:
                      candidate.kind === 'view'
                        ? `${candidate.name} (${t('viewer', 'sqliteViewLabel')})`
                        : candidate.name,
                    onclick: () => actions.onSelectObject(candidate.name),
                  },
                  [
                    m('span.fm-sqlite-object-name', candidate.name),
                    candidate.kind === 'view'
                      ? m('span.fm-sqlite-object-kind', t('viewer', 'sqliteViewLabel'))
                      : undefined,
                  ],
                ),
              ),
        ),
        m('.fm-sqlite-main', [
          m('.fm-sqlite-tabs', { role: 'tablist' }, [
            ...TABS.map((tab) =>
              m(
                'button.fm-sqlite-tab',
                {
                  type: 'button',
                  role: 'tab',
                  'aria-selected': content.tab === tab.id ? 'true' : 'false',
                  class: content.tab === tab.id ? 'active' : '',
                  onclick: () => actions.onTab(tab.id),
                },
                t('viewer', tab.key),
              ),
            ),
            m(
              'span.fm-sqlite-info',
              t('viewer', 'sqliteInfo', {
                version: content.sqliteVersion,
                size: formatBytes(content.sourceBytes),
              }),
            ),
          ]),
          content.tab === 'data'
            ? renderDataTab(content, actions)
            : content.tab === 'structure'
              ? renderStructureTab(object)
              : renderSqlTab(content, actions),
        ]),
      ]),
    ]);
  },
});
