import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { SqliteCell, SqliteObject } from '../../models';
import { EMPTY_SQLITE_RESULT, type FileViewerSqliteContent } from './sqlite-browser';
import { SqliteBrowser, type SqliteBrowserActions, sqliteTablePicker } from './sqlite-view';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.append(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

const text = (value: string): SqliteCell => ({ valueType: 'text', text: value, truncated: false });

function actions(overrides: Partial<SqliteBrowserActions> = {}): SqliteBrowserActions {
  return {
    onTab: vi.fn(),
    onSelectObject: vi.fn(),
    onBrowse: vi.fn(),
    onPage: vi.fn(),
    onSql: vi.fn(),
    onRunSql: vi.fn(),
    ...overrides,
  };
}

function content(overrides: Partial<FileViewerSqliteContent> = {}): FileViewerSqliteContent {
  return {
    kind: 'sqlite',
    sessionId: 's',
    sourceBytes: 8192,
    sqliteVersion: '3.50.0',
    warning: undefined,
    objects: [
      {
        name: 'people',
        kind: 'table',
        columns: [
          { name: 'id', declaredType: 'INTEGER', notNull: true, primaryKey: true },
          { name: 'name', declaredType: 'TEXT', notNull: false, primaryKey: false },
        ],
        createSql: 'CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT)',
      },
      { name: 'recent', kind: 'view', columns: [] },
    ],
    tab: 'data',
    selected: 'people',
    search: '',
    filters: [],
    sort: undefined,
    sql: 'SELECT 1',
    data: {
      result: {
        columns: ['id', 'name'],
        rows: [
          [{ valueType: 'integer', text: '1', truncated: false }, text('Ada')],
          [
            { valueType: 'integer', text: '2', truncated: false },
            { valueType: 'null', truncated: false },
          ],
          [{ valueType: 'integer', text: '3', truncated: false }, text('')],
          [
            { valueType: 'integer', text: '4', truncated: false },
            { valueType: 'blob', text: '00 ff', byteLength: 80, truncated: true },
          ],
        ],
        offset: 0,
        hasMore: true,
        totalRows: 250,
        truncated: false,
        elapsedMs: 3,
      },
      totalRows: 250,
      offset: 0,
      loading: false,
      error: undefined,
    },
    sqlRun: EMPTY_SQLITE_RESULT,
    ...overrides,
  };
}

function mount(attrs: { content: FileViewerSqliteContent; actions: SqliteBrowserActions }): void {
  m.mount(root, { view: () => m(SqliteBrowser, attrs) });
  m.redraw.sync();
}

describe('SqliteBrowser', () => {
  it('renders objects, rows and shows NULL, empty string and blobs distinctly', () => {
    mount({ content: content(), actions: actions() });
    const cells = [...root.querySelectorAll('tbody tr')].map((row) =>
      [...row.querySelectorAll('td')].map((cell) => ({
        text: cell.textContent,
        className: cell.className,
      })),
    );
    expect(cells[1]?.[1]).toEqual({ text: 'NULL', className: 'fm-sqlite-null' });
    expect(cells[2]?.[1]).toEqual({ text: '', className: '' });
    expect(cells[3]?.[1]?.text).toBe('00 ff …');
    expect(cells[3]?.[1]?.className).toBe('fm-sqlite-blob');
    expect(cells[0]?.[0]?.className).toBe('fm-sqlite-number');
    expect(root.querySelector('.fm-sqlite-pager-label')?.textContent).toBe('Rows 1–4 of 250');
  });

  it('selects objects, switches tabs, and pages', () => {
    const handlers = actions();
    mount({ content: content(), actions: handlers });
    root.querySelectorAll<HTMLButtonElement>('.fm-sqlite-tab')[2]?.click();
    expect(handlers.onTab).toHaveBeenCalledWith('sql');
    const [previous, next] = root.querySelectorAll<HTMLButtonElement>('.fm-sqlite-pager button');
    expect(previous?.disabled).toBe(true);
    next?.click();
    expect(handlers.onPage).toHaveBeenCalledWith('data', 4);
  });

  it('advances by the rows actually shown when a page was cut short', () => {
    const handlers = actions();
    const base = content();
    const result = base.data.result;
    if (result === undefined) throw new Error('fixture has no result');
    mount({
      content: content({
        data: { ...base.data, result: { ...result, truncated: true, hasMore: true } },
      }),
      actions: handlers,
    });
    root.querySelectorAll<HTMLButtonElement>('.fm-sqlite-pager button')[1]?.click();
    expect(handlers.onPage).toHaveBeenCalledWith('data', 4);
  });

  it('forwards search text, filters and sort clicks', () => {
    const handlers = actions();
    mount({ content: content(), actions: handlers });
    const search = root.querySelector<HTMLInputElement>('.fm-sqlite-search');
    if (search === null) throw new Error('missing search input');
    search.value = 'ada';
    search.dispatchEvent(new Event('input'));
    expect(handlers.onBrowse).toHaveBeenCalledWith({ search: 'ada' });

    root.querySelectorAll<HTMLButtonElement>('.fm-sqlite-toolbar button')[0]?.click();
    expect(handlers.onBrowse).toHaveBeenCalledWith({
      filters: [{ column: 'id', operator: 'contains', value: '' }],
    });

    root.querySelectorAll<HTMLButtonElement>('.fm-sqlite-sort')[1]?.click();
    expect(handlers.onBrowse).toHaveBeenCalledWith({
      sort: { column: 'name', descending: false },
    });
  });

  it('renders filter rows and hides the value input for NULL operators', () => {
    const handlers = actions();
    mount({
      content: content({
        filters: [
          { column: 'name', operator: 'contains', value: 'a' },
          { column: 'name', operator: 'isNull', value: '' },
        ],
      }),
      actions: handlers,
    });
    const rows = root.querySelectorAll('.fm-sqlite-filter');
    expect(rows).toHaveLength(2);
    expect(rows[0]?.querySelector('.fm-sqlite-filter-value')).not.toBeNull();
    expect(rows[1]?.querySelector('.fm-sqlite-filter-value')).toBeNull();
    rows[0]?.querySelector<HTMLButtonElement>('button')?.click();
    expect(handlers.onBrowse).toHaveBeenCalledWith({
      filters: [{ column: 'name', operator: 'isNull', value: '' }],
    });
  });

  it('shows the structure tab with columns and the CREATE statement', () => {
    mount({ content: content({ tab: 'structure' }), actions: actions() });
    expect([...root.querySelectorAll('tbody tr')].map((row) => row.textContent)).toEqual([
      'idINTEGER✓✓',
      'nameTEXT',
    ]);
    expect(root.querySelector('.fm-sqlite-definition pre')?.textContent).toContain('CREATE TABLE');
  });

  it('runs SQL with the button or Ctrl+Enter and shows backend errors', () => {
    const handlers = actions();
    mount({
      content: content({
        tab: 'sql',
        sqlRun: { ...EMPTY_SQLITE_RESULT, error: 'only read-only statements are allowed' },
      }),
      actions: handlers,
    });
    expect(root.querySelector('.fm-sqlite-error')?.textContent).toContain('read-only');
    root.querySelector<HTMLButtonElement>('.fm-sqlite-run')?.click();
    const textarea = root.querySelector<HTMLTextAreaElement>('.fm-sqlite-sql');
    textarea?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true }));
    expect(handlers.onRunSql).toHaveBeenCalledTimes(2);
    if (textarea === null) throw new Error('missing textarea');
    textarea.value = 'SELECT 2';
    textarea.dispatchEvent(new Event('input'));
    expect(handlers.onSql).toHaveBeenCalledWith('SELECT 2');
  });

  it('shows a warning banner and an empty-database message', () => {
    mount({
      content: content({
        warning: 'A write-ahead log exists',
        objects: [],
        selected: undefined,
        data: EMPTY_SQLITE_RESULT,
      }),
      actions: actions(),
    });
    expect(root.querySelector('.fm-sqlite-warning')?.textContent).toContain('write-ahead');
    expect(root.querySelector('.fm-sqlite-empty')?.textContent).toContain('no tables');
  });
});

describe('sqliteTablePicker', () => {
  function mountPicker(objects: SqliteObject[], selected: string | undefined) {
    const handlers = actions();
    const data = content({ objects, selected });
    m.mount(root, { view: () => m('.crumb', sqliteTablePicker(data, handlers)) });
    m.redraw.sync();
    return handlers;
  }

  it('lists every table in a select and reports the chosen one', () => {
    const handlers = mountPicker(
      [
        { name: 'people', kind: 'table', columns: [] },
        { name: 'recent', kind: 'view', columns: [] },
      ],
      'people',
    );
    const select = root.querySelector<HTMLSelectElement>('.fm-file-viewer-table-select');
    expect(select?.value).toBe('people');
    expect(select?.options[1]?.textContent).toBe('recent (view)');
    if (select === null) throw new Error('table select missing');
    select.value = 'recent';
    select.dispatchEvent(new Event('change'));
    expect(handlers.onSelectObject).toHaveBeenCalledWith('recent');
  });

  it('shows the lone table as plain text', () => {
    mountPicker([{ name: 'people', kind: 'table', columns: [] }], 'people');
    expect(root.querySelector('.fm-file-viewer-table-select')).toBeNull();
    expect(root.querySelector('.fm-file-viewer-crumb')?.textContent).toBe('people');
  });
});
