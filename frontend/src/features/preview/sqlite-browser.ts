import type { FileManagerClient } from '../../api/client/file-manager-client';
import { errorText, viewerWarningText } from '../../i18n';
import type {
  OpenSqliteViewRequest,
  SqliteColumnFilter,
  SqliteObject,
  SqliteQueryResult,
  SqliteQuerySource,
  SqliteView,
} from '../../models';

/** Rows requested per page; the backend caps a page at 500. */
export const SQLITE_PAGE_SIZE = 100;

/** Extensions that may hold a SQLite database. `.db` is ambiguous, so the backend magic-checks. */
const SQLITE_EXTENSIONS: ReadonlySet<string> = new Set([
  'sqlite',
  'sqlite3',
  'db',
  'db3',
  's3db',
  'sl3',
]);

export type SqliteTab = 'data' | 'structure' | 'sql';

export interface SqliteSort {
  readonly column: string;
  readonly descending: boolean;
}

/** One page (or failure) of either the table browser or the SQL console. */
export interface SqliteResultState {
  readonly result: SqliteQueryResult | undefined;
  /** Total matching rows; only the first page carries it, so it is remembered across pages. */
  readonly totalRows: number | undefined;
  readonly offset: number;
  readonly loading: boolean;
  readonly error: string | undefined;
}

export type SqliteClient = Pick<
  FileManagerClient,
  'openSqliteView' | 'querySqliteView' | 'closeSqliteView'
>;

/** F3 content for a SQLite database: table browser, structure and SQL console (task 0251). */
export interface FileViewerSqliteContent {
  readonly kind: 'sqlite';
  readonly sessionId: string;
  readonly sourceBytes: number;
  readonly sqliteVersion: string;
  readonly warning: string | undefined;
  readonly objects: readonly SqliteObject[];
  readonly tab: SqliteTab;
  readonly selected: string | undefined;
  readonly search: string;
  readonly filters: readonly SqliteColumnFilter[];
  readonly sort: SqliteSort | undefined;
  readonly sql: string;
  readonly data: SqliteResultState;
  readonly sqlRun: SqliteResultState;
}

export function isSqliteCandidate(extension: string | undefined): boolean {
  return extension !== undefined && SQLITE_EXTENSIONS.has(extension.toLowerCase());
}

export const EMPTY_SQLITE_RESULT: SqliteResultState = {
  result: undefined,
  totalRows: undefined,
  offset: 0,
  loading: false,
  error: undefined,
};

export function sqliteContentFromOpen(opened: SqliteView): FileViewerSqliteContent {
  const first = opened.objects[0]?.name;
  return {
    kind: 'sqlite',
    sessionId: opened.sessionId,
    sourceBytes: opened.sourceBytes,
    sqliteVersion: opened.sqliteVersion,
    warning: viewerWarningText(opened.warning, opened.warningCode),
    objects: opened.objects,
    tab: 'data',
    selected: first,
    search: '',
    filters: [],
    sort: undefined,
    sql: first === undefined ? 'SELECT 1' : `SELECT * FROM ${quoteSqlIdentifier(first)} LIMIT 100`,
    data: { ...EMPTY_SQLITE_RESULT, loading: first !== undefined },
    sqlRun: EMPTY_SQLITE_RESULT,
  };
}

export function quoteSqlIdentifier(name: string): string {
  return `"${name.replaceAll('"', '""')}"`;
}

export function selectedSqliteObject(content: FileViewerSqliteContent): SqliteObject | undefined {
  return content.objects.find((object) => object.name === content.selected);
}

/** The browse query for the table browser's current table, search, filters and sort. */
export function sqliteBrowseSource(
  content: FileViewerSqliteContent,
): SqliteQuerySource | undefined {
  if (content.selected === undefined) return undefined;
  return {
    type: 'browse',
    object: content.selected,
    search: content.search,
    filters: [...content.filters],
    ...(content.sort === undefined ? {} : { sort: content.sort }),
  };
}

function failureMessage(error: unknown): string {
  return errorText(error);
}

async function fetchPage(
  client: SqliteClient,
  sessionId: string,
  source: SqliteQuerySource,
  offset: number,
  previous: SqliteResultState,
  signal: AbortSignal,
): Promise<SqliteResultState> {
  try {
    const result = await client.querySqliteView(
      { sessionId, source, offset, limit: SQLITE_PAGE_SIZE },
      signal,
    );
    return {
      result,
      totalRows: result.totalRows ?? (offset === 0 ? undefined : previous.totalRows),
      offset,
      loading: false,
      error: undefined,
    };
  } catch (error: unknown) {
    return { ...previous, loading: false, error: failureMessage(error) };
  }
}

/** Loads one page of the table browser. */
export function loadSqliteData(
  client: SqliteClient,
  content: FileViewerSqliteContent,
  offset: number,
  signal: AbortSignal,
): Promise<SqliteResultState> {
  const source = sqliteBrowseSource(content);
  if (source === undefined) return Promise.resolve(EMPTY_SQLITE_RESULT);
  return fetchPage(client, content.sessionId, source, offset, content.data, signal);
}

/** Runs one page of the SQL console's statement. */
export function loadSqliteSql(
  client: SqliteClient,
  content: FileViewerSqliteContent,
  offset: number,
  signal: AbortSignal,
): Promise<SqliteResultState> {
  return fetchPage(
    client,
    content.sessionId,
    { type: 'sql', sql: content.sql },
    offset,
    offset === 0 ? EMPTY_SQLITE_RESULT : content.sqlRun,
    signal,
  );
}

/** Opens the database and returns the initial browser state. */
export async function openSqliteContent(
  client: SqliteClient,
  location: OpenSqliteViewRequest['location'],
  signal: AbortSignal,
): Promise<FileViewerSqliteContent> {
  return sqliteContentFromOpen(await client.openSqliteView({ location }, signal));
}
