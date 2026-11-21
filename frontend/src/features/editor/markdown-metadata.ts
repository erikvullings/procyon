/**
 * Recognises the metadata blocks plain Markdown renders as one run-on paragraph:
 *
 * - YAML front matter (`---` … `---`), as written by Obsidian and Procyon notes;
 * - a TASKS-style header: a paragraph of `Key: value` lines right after the leading `# Title`.
 *
 * Both become a small key/value table. The parser covers flat YAML only (scalars, `[a, b]` flow
 * lists and `- item` block lists); anything else is shown as raw text rather than guessed at.
 */

export interface MetadataField {
  readonly key: string;
  /** One entry for a scalar, several for a list. */
  readonly values: readonly string[];
  readonly list: boolean;
}

export interface MetadataSplit {
  /** Markdown before the metadata block (a leading heading), possibly empty. */
  readonly before: string;
  readonly fields: readonly MetadataField[];
  /** Markdown after the metadata block. */
  readonly after: string;
}

const FRONT_MATTER_OPEN = /^---[ \t]*$/;
const FRONT_MATTER_CLOSE = /^(?:---|\.\.\.)[ \t]*$/;
const MAX_FRONT_MATTER_LINES = 200;
const YAML_KEY = /^([A-Za-z_][\w .-]{0,63}):(?:[ \t]+(.*))?$/;
const YAML_ITEM = /^[ \t]+-[ \t]+(.*)$/;
const HEADER_FIELD = /^([A-Z][A-Za-z0-9 _-]{0,31}):[ \t]+(\S.*)$/;
const MIN_HEADER_FIELDS = 2;

/** Splits off leading YAML front matter, or `undefined` when the document has none. */
export function splitFrontMatter(markdown: string): MetadataSplit | undefined {
  const lines = markdown.split(/\r?\n/);
  if (!FRONT_MATTER_OPEN.test(lines[0] ?? '')) return undefined;
  const limit = Math.min(lines.length, MAX_FRONT_MATTER_LINES + 1);
  let close = -1;
  for (let index = 1; index < limit; index += 1) {
    if (FRONT_MATTER_CLOSE.test(lines[index] ?? '')) {
      close = index;
      break;
    }
  }
  if (close < 0) return undefined;
  const fields = parseYamlFields(lines.slice(1, close));
  if (fields === undefined || fields.length === 0) return undefined;
  return { before: '', fields, after: lines.slice(close + 1).join('\n') };
}

/** Splits off a TASKS-style `Key: value` paragraph that follows the leading `# Title`. */
export function splitHeaderFields(markdown: string): MetadataSplit | undefined {
  const lines = markdown.split(/\r?\n/);
  let index = 0;
  while (index < lines.length && (lines[index] ?? '').trim() === '') index += 1;
  if (!/^#[ \t]+\S/.test(lines[index] ?? '')) return undefined;
  index += 1;
  const headingEnd = index;
  while (index < lines.length && (lines[index] ?? '').trim() === '') index += 1;
  const fields: MetadataField[] = [];
  while (index < lines.length && (lines[index] ?? '').trim() !== '') {
    const match = HEADER_FIELD.exec(lines[index] ?? '');
    if (match === null) return undefined;
    fields.push({ key: match[1] ?? '', values: [(match[2] ?? '').trim()], list: false });
    index += 1;
  }
  if (fields.length < MIN_HEADER_FIELDS) return undefined;
  return {
    before: lines.slice(0, headingEnd).join('\n'),
    fields,
    after: lines.slice(index).join('\n'),
  };
}

/** Front matter first, then a TASKS-style header; `undefined` when neither is present. */
export function splitMetadata(markdown: string): MetadataSplit | undefined {
  return splitFrontMatter(markdown) ?? splitHeaderFields(markdown);
}

/** Parses flat YAML, or `undefined` when a line is something this parser does not understand. */
function parseYamlFields(lines: readonly string[]): MetadataField[] | undefined {
  const fields: MetadataField[] = [];
  let pending: { key: string; items: string[] } | undefined;
  const flush = (): void => {
    if (pending === undefined) return;
    fields.push({ key: pending.key, values: pending.items, list: true });
    pending = undefined;
  };
  for (const line of lines) {
    if (line.trim() === '' || line.trimStart().startsWith('#')) continue;
    const item = YAML_ITEM.exec(line);
    if (item !== null && pending !== undefined) {
      pending.items.push(unquote(item[1] ?? ''));
      continue;
    }
    flush();
    const match = YAML_KEY.exec(line);
    if (match === null) return undefined;
    const key = match[1] ?? '';
    const raw = (match[2] ?? '').trim();
    if (raw === '') {
      pending = { key, items: [] };
    } else if (raw.startsWith('[') && raw.endsWith(']')) {
      fields.push({ key, values: splitFlowList(raw.slice(1, -1)), list: true });
    } else {
      fields.push({ key, values: [unquote(raw)], list: false });
    }
  }
  flush();
  return fields;
}

/** Splits `a, "b, c", d` on the commas outside quotes. */
function splitFlowList(inner: string): string[] {
  const items: string[] = [];
  let current = '';
  let quote: string | undefined;
  for (const char of inner) {
    if (quote !== undefined) {
      current += char;
      if (char === quote) quote = undefined;
    } else if (char === '"' || char === "'") {
      quote = char;
      current += char;
    } else if (char === ',') {
      items.push(current);
      current = '';
    } else {
      current += char;
    }
  }
  items.push(current);
  return items.map((item) => unquote(item.trim())).filter((item) => item !== '');
}

function unquote(value: string): string {
  if (value.length >= 2 && value.startsWith('"') && value.endsWith('"')) {
    return value.slice(1, -1).replace(/\\(["\\])/g, '$1');
  }
  if (value.length >= 2 && value.startsWith("'") && value.endsWith("'")) {
    return value.slice(1, -1).replace(/''/g, "'");
  }
  return value;
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/** A CSS-safe slug of a status value, e.g. `in_progress` → `in-progress`. */
function statusSlug(value: string): string {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '');
}

/** A four-digit task ID, as used by `TASKS/NNNN-title.md` files. */
const TASK_ID = /^\d{4}$/;

/** `Depends on: 0004, 0012` with each task ID as a link the Markdown views resolve on click. */
function dependsOnHtml(value: string): string {
  return value
    .split(/(\s*,\s*|\s+)/)
    .map((part) =>
      TASK_ID.test(part)
        ? `<a href="#" class="fm-md-task-link" data-fm-task="${part}">${part}</a>`
        : escapeHtml(part),
    )
    .join('');
}

function valueHtml(field: MetadataField): string {
  if (field.list) {
    return field.values
      .map((value) => `<span class="fm-md-meta-chip">${escapeHtml(value)}</span>`)
      .join(' ');
  }
  const value = field.values[0] ?? '';
  if (field.key.toLowerCase() === 'status' && value !== '') {
    return `<span class="fm-md-meta-status fm-md-meta-status-${statusSlug(value)}">${escapeHtml(value)}</span>`;
  }
  if (field.key.toLowerCase() === 'depends on') return dependsOnHtml(value);
  return escapeHtml(value);
}

/** The key/value table for a metadata block. Every key and value is HTML-escaped. */
export function metadataTableHtml(fields: readonly MetadataField[]): string {
  const rows = fields
    .map(
      (field) =>
        `<tr><th scope="row">${escapeHtml(field.key)}</th><td>${valueHtml(field)}</td></tr>`,
    )
    .join('');
  return `<table class="fm-md-meta"><tbody>${rows}</tbody></table>`;
}
