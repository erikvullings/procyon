import type { KnowledgeQueryDraft, KnowledgeScopeSelector } from '../../models';

/** Canonical DSL text for one scope selector, matching the backend serialiser. */
export function scopeSelectorText(selector: KnowledgeScopeSelector): string {
  switch (selector.kind) {
    case 'wholeLibrary':
      return 'library';
    case 'root':
      return `root:${selector.id ?? ''}`;
    case 'workspace':
      return `workspace:${selector.id ?? ''}`;
  }
}

/** Quotes a value exactly like the DSL serialiser, so a round-trip is lossless. */
export function quoteValue(value: string): string {
  return /[\s,:;"]/u.test(value) ? `"${value.replace(/(["\\])/gu, '\\$1')}"` : value;
}

/** Serialises a value list into the comma-separated DSL form. */
export function joinValues(values: readonly string[]): string {
  return values.map(quoteValue).join(', ');
}

/**
 * Splits a comma-separated DSL value list, honouring quotes and escapes, so a
 * value that itself contains a comma - `"ACME, Inc."` - stays one value.
 */
export function splitValues(raw: string): string[] {
  const values: string[] = [];
  let current = '';
  let quoted = false;
  for (let index = 0; index < raw.length; index += 1) {
    const character = raw[index];
    if (character === '\\' && quoted && index + 1 < raw.length) {
      current += raw[index + 1] ?? '';
      index += 1;
      continue;
    }
    if (character === '"') {
      quoted = !quoted;
      continue;
    }
    if (character === ',' && !quoted) {
      values.push(current.trim());
      current = '';
      continue;
    }
    current += character;
  }
  values.push(current.trim());
  return values.filter((value) => value.length > 0);
}

/**
 * Reads the subject editor: one subject per line or comma-separated, using the
 * same quoting rules as the DSL, so a subject like `"ACME, Inc."` survives
 * editing as the single value the parser returned.
 */
export function splitSubjects(value: string): string[] {
  return value.split('\n').flatMap((line) => splitValues(line));
}

/** Writes the subject editor: one quoted-as-needed subject per line. */
export function joinSubjects(values: readonly string[]): string {
  return values.map(quoteValue).join('\n');
}

/** An emptied canonical draft; every field is explicit so nothing is inferred. */
export function emptyDraft(): KnowledgeQueryDraft {
  return {
    about: [],
    needs: [],
    related: [],
    scopes: [],
    action: null,
    context: null,
    constraints: [],
    format: null,
    depth: null,
  };
}

/** Why one DSL scope selector cannot be applied to the visual scope control. */
export interface KnowledgeScopeIssue {
  readonly selector: KnowledgeScopeSelector;
  readonly reason: 'unknownRoot' | 'unsupported';
}

/**
 * Splits DSL scope selectors into the ones the visual scope control can
 * represent and the ones it cannot.
 *
 * `library` maps to the entire-library scope and `root:<id>` to a selected
 * indexed root; a root the host never reported, and any `workspace:<id>`
 * selector (which the backend resolves against a different authority), are
 * returned as issues so the dialog can say so instead of dropping them.
 */
export function classifyKnowledgeScopeSelectors(
  selectors: readonly KnowledgeScopeSelector[],
  knownRootIds: ReadonlySet<string>,
): {
  readonly wholeLibrary: boolean;
  readonly rootIds: readonly string[];
  readonly issues: readonly KnowledgeScopeIssue[];
} {
  const rootIds: string[] = [];
  const issues: KnowledgeScopeIssue[] = [];
  let wholeLibrary = false;
  for (const selector of selectors) {
    switch (selector.kind) {
      case 'wholeLibrary':
        wholeLibrary = true;
        break;
      case 'root': {
        const id = selector.id ?? '';
        if (knownRootIds.has(id)) rootIds.push(id);
        else issues.push({ selector, reason: 'unknownRoot' });
        break;
      }
      case 'workspace':
        issues.push({ selector, reason: 'unsupported' });
        break;
    }
  }
  return { wholeLibrary, rootIds, issues };
}
