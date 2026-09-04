import type { SearchInFileMatch } from '../../models';

const MAX_HTML_SEARCH_MATCHES = 5_000;

export interface HtmlSearchResult {
  readonly html: string;
  readonly matches: readonly SearchInFileMatch[];
  readonly truncated: boolean;
}

/** Highlights matches in an already-sanitized HTML fragment without flattening its markup. */
export function searchHtml(
  safeHtml: string,
  query: string,
  regex: boolean,
  caseSensitive: boolean,
  wholeWord: boolean,
  activeIndex: number | undefined,
): HtmlSearchResult {
  if (query === '') return { html: safeHtml, matches: [], truncated: false };
  const source = regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const expression = new RegExp(
    wholeWord ? `\\b(?:${source})\\b` : source,
    caseSensitive ? 'gu' : 'giu',
  );
  const document = new DOMParser().parseFromString(safeHtml, 'text/html');
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  const textNodes: Text[] = [];
  let node = walker.nextNode();
  while (node !== null) {
    textNodes.push(node as Text);
    node = walker.nextNode();
  }
  const fullText = textNodes.map((textNode) => textNode.data).join('');
  const matches: SearchInFileMatch[] = [];
  let truncated = false;
  for (const match of fullText.matchAll(expression)) {
    const value = match[0];
    if (value.length === 0) continue;
    if (matches.length === MAX_HTML_SEARCH_MATCHES) {
      truncated = true;
      break;
    }
    matches.push({ offset: match.index, length: value.length, lineNumber: 1 });
  }

  let nodeOffset = 0;
  for (const textNode of textNodes) {
    const text = textNode.data;
    const fragments: Array<Node | string> = [];
    let cursor = 0;
    const nodeEnd = nodeOffset + text.length;
    for (const [matchIndex, match] of matches.entries()) {
      const matchEnd = match.offset + match.length;
      if (matchEnd <= nodeOffset) continue;
      if (match.offset >= nodeEnd) break;
      const start = Math.max(0, match.offset - nodeOffset);
      const end = Math.min(text.length, matchEnd - nodeOffset);
      fragments.push(text.slice(cursor, start));
      const mark = document.createElement('mark');
      mark.className =
        matchIndex === activeIndex ? 'fm-document-search-match-active' : 'fm-document-search-match';
      mark.textContent = text.slice(start, end);
      fragments.push(mark);
      cursor = end;
    }
    if (fragments.length > 0) {
      fragments.push(text.slice(cursor));
      textNode.replaceWith(...fragments);
    }
    nodeOffset = nodeEnd;
  }
  return { html: document.body.innerHTML, matches, truncated };
}
