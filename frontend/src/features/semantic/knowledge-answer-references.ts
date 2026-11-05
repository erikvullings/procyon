import type { KnowledgeAnswerCitation, KnowledgeEvidence } from '../../models';
import { decodeEvidenceTitle } from './evidence-title';

export const REFERENCE_ANCHOR = '#fm-knowledge-reference-';

/** Escapes a citation label for use inside a regular expression. */
function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&');
}

/** One numbered source an answer cites: a file, however many of its sections were cited. */
export interface KnowledgeAnswerReference {
  readonly number: number;
  readonly title: string;
  readonly citations: readonly KnowledgeAnswerCitation[];
  /** The first displayed row the answer cited from this file, if any. */
  readonly row: KnowledgeEvidence | undefined;
}

/** A generated answer with its opaque `E#` labels resolved to numbered file references. */
export interface NumberedKnowledgeAnswer {
  /** Markdown in which every cited group is a `⟦1,4⟧` placeholder. */
  readonly markdown: string;
  /** Plain text in which every cited group reads `[1, 4]`. */
  readonly text: string;
  readonly references: readonly KnowledgeAnswerReference[];
}

export const REFERENCE_PLACEHOLDER = /⟦(\d+(?:,\d+)*)⟧/gu;

/**
 * A closing "Sources: ⟦1,4⟧" line, in whatever language the model answered.
 * The reference list below the answer already shows these sources.
 */
export const TRAILING_SOURCES_LINE =
  /\n[ \t]*(?:[*_]{1,2})?[\p{L}][\p{L} ]{0,24}(?:[*_]{1,2})?[ \t]*:[ \t]*(?:[*_]{1,2})?[ \t]*⟦[\d,]+⟧[ \t.]*(?:[*_]{1,2})?\s*$/u;

/**
 * Replaces the opaque evidence labels a model copied - `E1`, `[E1]`,
 * `(E1, E4)`, `E1, E4, E5` - with per-file reference numbers in order of first
 * mention, so several cited sections of one file share one number.
 *
 * Only labels the host resolved to a citation are touched; anything else the
 * model wrote stays as text.
 */
export function numberKnowledgeAnswer(
  text: string,
  citations: readonly KnowledgeAnswerCitation[],
  displayed: ReadonlyMap<string, KnowledgeEvidence>,
): NumberedKnowledgeAnswer {
  const rowFor = (citation: KnowledgeAnswerCitation): KnowledgeEvidence | undefined => {
    const row = displayed.get(citation.recordId);
    return row?.sourceId === citation.sourceId ? row : undefined;
  };
  const documentKey = (citation: KnowledgeAnswerCitation): string =>
    rowFor(citation)?.documentId ?? `source:${citation.sourceId}`;
  const byLabel = new Map(citations.map((citation) => [citation.label, citation]));
  const numbers = new Map<string, number>();
  const numberOf = (citation: KnowledgeAnswerCitation): number => {
    const key = documentKey(citation);
    let number = numbers.get(key);
    if (number === undefined) {
      number = numbers.size + 1;
      numbers.set(key, number);
    }
    return number;
  };
  let markdown = text;
  if (byLabel.size > 0) {
    const label = [...byLabel.keys()]
      .sort((left, right) => right.length - left.length)
      .map(escapeRegExp)
      .join('|');
    const one = `(?<![\\p{L}\\p{N}])(?:${label})(?![\\p{L}\\p{N}])`;
    const group = `${one}(?:\\s*[,;]?\\s*${one})*`;
    const cited = new RegExp(`\\[\\s*${group}\\s*\\]|\\(\\s*${group}\\s*\\)|${group}`, 'gu');
    const labelPattern = new RegExp(one, 'gu');
    markdown = text.replace(cited, (match) => {
      const groupNumbers = [
        ...new Set(
          [...match.matchAll(labelPattern)].flatMap(([found]) => {
            const citation = byLabel.get(found);
            return citation === undefined ? [] : [numberOf(citation)];
          }),
        ),
      ].sort((left, right) => left - right);
      return groupNumbers.length === 0 ? match : `⟦${groupNumbers.join(',')}⟧`;
    });
  }
  markdown = markdown
    .replace(/⟦[\d,]+⟧(?:\s*[,;]\s*⟦[\d,]+⟧)+/gu, (run) => {
      const merged = [...new Set(run.match(/\d+/gu)?.map(Number) ?? [])];
      return `⟦${merged.sort((left, right) => left - right).join(',')}⟧`;
    })
    .replace(TRAILING_SOURCES_LINE, '')
    .trimEnd();
  for (const citation of citations) numberOf(citation);
  const references = [...numbers.entries()].map(([key, number]) => {
    const cited = citations.filter((citation) => documentKey(citation) === key);
    const row = cited.map(rowFor).find((candidate) => candidate !== undefined);
    const title =
      (row === undefined ? undefined : (decodeEvidenceTitle(row.title) ?? row.title)) ??
      cited[0]?.sectionPath.join(' / ') ??
      cited[0]?.label ??
      '';
    return { number, title: title === '' ? (cited[0]?.label ?? '') : title, citations: cited, row };
  });
  return {
    markdown,
    text: markdown.replace(
      REFERENCE_PLACEHOLDER,
      (_match, list: string) => `[${list.split(',').join(', ')}]`,
    ),
    references,
  };
}

/**
 * Turns the reference placeholders in already-sanitised answer HTML into
 * `[1, 4]` groups whose numbers link to openable sources. Only digits are
 * ever inserted, so the sanitised HTML stays safe.
 */
export function linkKnowledgeReferences(html: string, openable: ReadonlySet<number>): string {
  return html.replace(REFERENCE_PLACEHOLDER, (_match, list: string) => {
    const links = list
      .split(',')
      .map((number) =>
        openable.has(Number(number))
          ? `<a href="${REFERENCE_ANCHOR}${number}">${number}</a>`
          : number,
      );
    return `<span class="fm-knowledge-reference">[${links.join(', ')}]</span>`;
  });
}
