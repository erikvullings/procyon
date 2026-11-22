/**
 * Recognises the metadata blocks plain Markdown renders as one run-on paragraph:
 *
 * - YAML front matter (`---` … `---`), as written by Obsidian and Procyon notes;
 * - a TASKS-style header: a paragraph of `Key: value` lines right after the leading `# Title`.
 *
 * Both become a small key/value table. The parser covers the YAML front matter actually seen in
 * notes and agent SKILL.md files: scalars (also continued over several lines), `[a, b]` flow
 * lists, `- item` block lists, `|` / `>` block scalars and nested maps. Anything else is shown as
 * raw text rather than guessed at.
 */

export interface MetadataField {
  readonly key: string;
  /** One entry for a scalar, several for a list. */
  readonly values: readonly string[];
  readonly list: boolean;
  /** A literal (`|`) block scalar, whose line breaks are kept. */
  readonly multiline?: boolean;
  /** The fields of a nested map; `values` is then empty. */
  readonly children?: readonly MetadataField[];
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
const YAML_ITEM = /^-(?:[ \t]+(.*))?$/;
const BLOCK_SCALAR = /^([|>])([+-]?)$/;
const MAX_DEPTH = 4;
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

function indentOf(line: string): number {
  return line.length - line.trimStart().length;
}

function isBlank(line: string): boolean {
  return line.trim() === '';
}

/** Removes the common indentation of the non-blank lines. */
function dedent(lines: readonly string[]): string[] {
  const indents = lines.filter((line) => !isBlank(line)).map(indentOf);
  const common = indents.length === 0 ? 0 : Math.min(...indents);
  return lines.map((line) => (isBlank(line) ? '' : line.slice(common)));
}

/** A `|` (literal) or `>` (folded) block scalar; `-` strips and `+` keeps trailing breaks. */
function blockScalar(style: string, chomping: string, lines: readonly string[]): string {
  const body = dedent(lines);
  let text: string;
  if (style === '|') {
    text = body.join('\n');
  } else {
    text = '';
    for (const line of body) {
      if (line === '') text += '\n';
      else text += text === '' || text.endsWith('\n') ? line : ` ${line}`;
    }
  }
  return chomping === '+' ? text : text.replace(/\s+$/, '');
}

/** `- item` lines (at any shared indentation); deeper lines continue the previous item. */
function blockList(lines: readonly string[]): string[] | undefined {
  const items: string[] = [];
  for (const line of dedent(lines)) {
    if (line === '' || line.startsWith('#')) continue;
    const item = YAML_ITEM.exec(line);
    if (item !== null) items.push(item[1] ?? '');
    else if (indentOf(line) > 0 && items.length > 0)
      items.push(`${items.pop() ?? ''} ${line.trim()}`);
    else return undefined;
  }
  return items.map((item) => unquote(item.trim()));
}

/** Parses a YAML mapping, or `undefined` when a line is something this parser does not handle. */
function parseYamlFields(lines: readonly string[], depth = 0): MetadataField[] | undefined {
  if (depth > MAX_DEPTH) return undefined;
  const fields: MetadataField[] = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index] ?? '';
    index += 1;
    if (isBlank(line) || line.trimStart().startsWith('#')) continue;
    if (indentOf(line) > 0) return undefined;
    const match = YAML_KEY.exec(line);
    if (match === null) return undefined;
    const key = match[1] ?? '';
    const raw = (match[2] ?? '').trim();
    // The value's own lines: indented ones, or `- item` lines right under the key.
    const own: string[] = [];
    while (index < lines.length) {
      const next = lines[index] ?? '';
      const continues =
        isBlank(next) ||
        indentOf(next) > 0 ||
        (raw === '' &&
          YAML_ITEM.test(next) &&
          own.every((ownLine) => !YAML_KEY.test(ownLine.trim())));
      if (!continues) break;
      own.push(next);
      index += 1;
    }
    while (own.length > 0 && isBlank(own[own.length - 1] ?? '')) own.pop();
    const block = BLOCK_SCALAR.exec(raw);
    if (block !== null) {
      const style = block[1] ?? '>';
      fields.push({
        key,
        values: [blockScalar(style, block[2] ?? '', own)],
        list: false,
        ...(style === '|' ? { multiline: true } : {}),
      });
    } else if (raw === '') {
      const first = own.find((ownLine) => !isBlank(ownLine))?.trim();
      if (first === undefined) {
        fields.push({ key, values: [''], list: false });
      } else if (YAML_ITEM.test(first)) {
        const items = blockList(own);
        if (items === undefined) return undefined;
        fields.push({ key, values: items, list: true });
      } else if (!YAML_KEY.test(first)) {
        // A scalar that starts on the next, indented line.
        const text = own.map((ownLine) => ownLine.trim()).filter((part) => part !== '');
        fields.push({ key, values: [unquote(text.join(' '))], list: false });
      } else {
        const children = parseYamlFields(dedent(own), depth + 1);
        if (children === undefined) return undefined;
        fields.push({ key, values: [], list: false, children });
      }
    } else if (raw.startsWith('[') && raw.endsWith(']')) {
      if (own.some((ownLine) => !isBlank(ownLine))) return undefined;
      fields.push({ key, values: splitFlowList(raw.slice(1, -1)), list: true });
    } else {
      // A plain or quoted scalar may continue on indented lines, folded into one.
      const text = [raw, ...own.map((ownLine) => ownLine.trim())].filter((part) => part !== '');
      fields.push({ key, values: [unquote(text.join(' '))], list: false });
    }
  }
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
  if (field.children !== undefined) return metadataTableHtml(field.children, true);
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
  return field.multiline === true ? escapeHtml(value).replace(/\n/g, '<br>') : escapeHtml(value);
}

/** The key/value table for a metadata block. Every key and value is HTML-escaped. */
export function metadataTableHtml(fields: readonly MetadataField[], nested = false): string {
  const rows = fields
    .map(
      (field) =>
        `<tr><th scope="row">${escapeHtml(field.key)}</th><td>${valueHtml(field)}</td></tr>`,
    )
    .join('');
  const className = nested ? 'fm-md-meta fm-md-meta-nested' : 'fm-md-meta';
  return `<table class="${className}"><tbody>${rows}</tbody></table>`;
}
