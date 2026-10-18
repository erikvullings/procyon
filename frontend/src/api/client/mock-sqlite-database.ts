import type {
  SqliteCell,
  SqliteColumnFilter,
  SqliteObject,
  SqliteQueryResult,
  SqliteQuerySource,
} from '../../models';

/** Rows are plain values; `null` is SQL NULL and `Uint8Array` a BLOB. */
type MockValue = string | number | null | Uint8Array;

interface MockTable {
  readonly object: SqliteObject;
  readonly rows: readonly (readonly MockValue[])[];
}

const MAX_ROWS = 500;

function column(name: string, declaredType: string, primaryKey = false) {
  return { name, declaredType, notNull: primaryKey, primaryKey };
}

function buildTables(): readonly MockTable[] {
  const cities = ['Amsterdam', 'Berlin', 'Copenhagen', 'Dublin', 'Edinburgh'];
  const customers: MockValue[][] = Array.from({ length: 240 }, (_, index) => [
    index + 1,
    `Customer ${index + 1}`,
    cities[index % cities.length] ?? null,
    index % 7 === 0 ? null : `customer${index + 1}@example.com`,
    index % 11 === 0 ? new Uint8Array([0x89, 0x50, 0x4e, 0x47, index & 0xff]) : null,
  ]);
  const orders: MockValue[][] = Array.from({ length: 900 }, (_, index) => [
    index + 1,
    (index % 240) + 1,
    Math.round((10 + ((index * 37) % 990)) * 100) / 100,
    index % 5 === 0 ? 'refunded' : 'paid',
  ]);
  return [
    {
      object: {
        name: 'customers',
        kind: 'table',
        columns: [
          column('id', 'INTEGER', true),
          column('name', 'TEXT'),
          column('city', 'TEXT'),
          column('email', 'TEXT'),
          column('avatar', 'BLOB'),
        ],
        createSql:
          'CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, city TEXT, email TEXT, avatar BLOB)',
      },
      rows: customers,
    },
    {
      object: {
        name: 'orders',
        kind: 'table',
        columns: [
          column('id', 'INTEGER', true),
          column('customer_id', 'INTEGER'),
          column('total', 'REAL'),
          column('status', 'TEXT'),
        ],
        createSql:
          'CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER, total REAL, status TEXT)',
      },
      rows: orders,
    },
  ];
}

function cell(value: MockValue): SqliteCell {
  if (value === null) return { valueType: 'null', truncated: false };
  if (value instanceof Uint8Array) {
    return {
      valueType: 'blob',
      text: [...value].map((byte) => byte.toString(16).padStart(2, '0')).join(' '),
      byteLength: value.length,
      truncated: false,
    };
  }
  if (typeof value === 'number') {
    return {
      valueType: Number.isInteger(value) ? 'integer' : 'real',
      text: String(value),
      truncated: false,
    };
  }
  return { valueType: 'text', text: value, truncated: false };
}

function asText(value: MockValue): string {
  if (value === null) return '';
  if (value instanceof Uint8Array) return '';
  return String(value);
}

function matchesFilter(value: MockValue, filter: SqliteColumnFilter): boolean {
  switch (filter.operator) {
    case 'isNull':
      return value === null;
    case 'isNotNull':
      return value !== null;
    case 'contains':
      return asText(value)
        .toLowerCase()
        .includes((filter.value ?? '').toLowerCase());
    case 'equals':
      return asText(value) === (filter.value ?? '');
    case 'notEquals':
      return asText(value) !== (filter.value ?? '');
    case 'greater':
      return compare(value, filter.value ?? '') > 0;
    case 'less':
      return compare(value, filter.value ?? '') < 0;
  }
}

function compare(left: MockValue, right: MockValue): number {
  if (typeof left === 'number' && typeof right === 'string' && right.trim() !== '') {
    return left - Number(right);
  }
  if (typeof left === 'number' && typeof right === 'number') return left - right;
  return asText(left).localeCompare(asText(right));
}

/**
 * In-memory stand-in for the backend SQLite viewer: two small tables, browse with search,
 * filters, sorting and paging, and a deliberately tiny `SELECT * FROM <table>` SQL subset so the
 * UI can be exercised without a Rust process.
 */
export class MockSqliteDatabase {
  private readonly tables = buildTables();

  get objects(): readonly SqliteObject[] {
    return this.tables.map((table) => table.object);
  }

  query(source: SqliteQuerySource, offset: number, limit: number): SqliteQueryResult {
    const started = performance.now();
    const pageSize = Math.min(Math.max(limit, 1), MAX_ROWS);
    const result =
      source.type === 'browse'
        ? this.browse(source, offset, pageSize)
        : this.sql(source.sql, offset, pageSize);
    return { ...result, elapsedMs: Math.round(performance.now() - started) };
  }

  private table(name: string): MockTable {
    const table = this.tables.find((candidate) => candidate.object.name === name);
    if (table === undefined) throw new Error(`unknown table '${name}'`);
    return table;
  }

  private browse(
    source: Extract<SqliteQuerySource, { type: 'browse' }>,
    offset: number,
    pageSize: number,
  ): Omit<SqliteQueryResult, 'elapsedMs'> {
    const table = this.table(source.object);
    const columns = table.object.columns;
    const indexOf = (name: string): number => {
      const index = columns.findIndex((candidate) => candidate.name === name);
      if (index < 0) throw new Error(`unknown column '${name}'`);
      return index;
    };
    const search = (source.search ?? '').trim().toLowerCase();
    let rows = table.rows.filter((row) => {
      if (search !== '' && !row.some((value) => asText(value).toLowerCase().includes(search))) {
        return false;
      }
      return (source.filters ?? []).every((filter) =>
        matchesFilter(row[indexOf(filter.column)] ?? null, filter),
      );
    });
    if (source.sort != null) {
      const index = indexOf(source.sort.column);
      const direction = source.sort.descending === true ? -1 : 1;
      rows = [...rows].sort(
        (left, right) => direction * compare(left[index] ?? null, right[index] ?? null),
      );
    }
    return this.page(
      columns.map((item) => item.name),
      rows,
      offset,
      pageSize,
      offset === 0 ? rows.length : undefined,
    );
  }

  private sql(sql: string, offset: number, pageSize: number): Omit<SqliteQueryResult, 'elapsedMs'> {
    const match = /^\s*select\s+\*\s+from\s+"?(\w+)"?\s*;?\s*$/i.exec(sql);
    if (match?.[1] === undefined) {
      throw new Error(
        'The mock runtime only understands "SELECT * FROM <table>"; use the Rust backend for full SQL.',
      );
    }
    const table = this.table(match[1]);
    return this.page(
      table.object.columns.map((item) => item.name),
      table.rows,
      offset,
      pageSize,
      undefined,
    );
  }

  private page(
    columns: string[],
    rows: readonly (readonly MockValue[])[],
    offset: number,
    pageSize: number,
    totalRows: number | undefined,
  ): Omit<SqliteQueryResult, 'elapsedMs'> {
    const slice = rows.slice(offset, offset + pageSize);
    return {
      columns,
      rows: slice.map((row) => row.map(cell)),
      offset,
      hasMore: offset + pageSize < rows.length,
      ...(totalRows !== undefined ? { totalRows } : {}),
      truncated: false,
    };
  }
}
