/**
 * Renders markdown HTML into a container and, when a search match is active, wraps it in a
 * `<mark>` and scrolls it into view. This owns the container's DOM entirely and imperatively
 * (like `code-mirror-editor.ts`/`pdf-preview.ts` own theirs) - `MarkdownPreview` in
 * `file-viewer.ts` never gives Mithril an `innerHTML` attribute to diff, so nothing here ever
 * fights a Mithril redraw.
 *
 * (An earlier version of this used the CSS Custom Highlight API - `CSS.highlights`/
 * `::highlight()` - to avoid a DOM-mutation approach entirely. That worked in Chromium but,
 * per manual testing, did not visibly render inside Tauri's WKWebView, so this reverts to plain
 * `<mark>` injection, which every engine renders reliably.)
 *
 * The backend reports a match as a byte offset into the raw markdown *source*
 * (`FileViewerTextContent.highlightOffset`/`highlightLength`), which has no fixed position in the
 * *rendered* HTML - markdown syntax (`#`, `**`, `[...] (...)`, ...) is stripped/transformed away.
 * Rather than trying to map the offset through that transform, this locates the match by its
 * literal text instead: it counts which occurrence (in reading order) `highlightOffset` is among
 * all occurrences of that exact substring in the source, then finds the same-numbered occurrence
 * in the rendered DOM - order is preserved by markdown rendering even though character positions
 * aren't.
 *
 * This is a best-effort location, not a guaranteed one: a match split across an inline formatting
 * boundary (e.g. half inside `**bold**`) exists as more than one DOM text node and won't be
 * found. Highlighting is then simply skipped for that match - the search match count and
 * next/previous navigation all come from search state alone and keep working regardless.
 */

const HIGHLIGHT_CLASS = 'fm-file-viewer-highlight';

/** Counts occurrences of `needle` in `haystack` strictly before `beforeIndex` - i.e. the 0-based
 * index, in reading order, that the occurrence starting at `beforeIndex` is. */
export function occurrenceIndexOf(haystack: string, needle: string, beforeIndex: number): number {
  if (needle.length === 0) return 0;
  let count = 0;
  let index = haystack.indexOf(needle);
  while (index !== -1 && index < beforeIndex) {
    count += 1;
    index = haystack.indexOf(needle, index + 1);
  }
  return count;
}

/** Finds the `occurrenceIndex`-th (0-based) occurrence of `needle` within `container`'s text
 * nodes, in document order. Only matches text within a single text node. */
export function findNthTextRange(
  container: Node,
  needle: string,
  occurrenceIndex: number,
): Range | undefined {
  if (needle.length === 0) return undefined;
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
  let remaining = occurrenceIndex;
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    const text = node.nodeValue ?? '';
    let index = text.indexOf(needle);
    while (index !== -1) {
      if (remaining === 0) {
        const range = new Range();
        range.setStart(node, index);
        range.setEnd(node, index + needle.length);
        return range;
      }
      remaining -= 1;
      index = text.indexOf(needle, index + 1);
    }
  }
  return undefined;
}

/** Scrolls `element` into view (centered) within its nearest scrollable ancestor. Measurement can
 * throw in test environments without full layout support; that's caught and treated as "nothing
 * to scroll". */
function scrollElementIntoView(element: HTMLElement): void {
  let rect: DOMRect;
  try {
    rect = element.getBoundingClientRect();
  } catch {
    return;
  }
  for (
    let ancestor: HTMLElement | null = element.parentElement;
    ancestor !== null;
    ancestor = ancestor.parentElement
  ) {
    if (ancestor.scrollHeight <= ancestor.clientHeight) continue;
    if (!/(auto|scroll)/.test(getComputedStyle(ancestor).overflowY)) continue;
    const containerRect = ancestor.getBoundingClientRect();
    const target = rect.top - containerRect.top + ancestor.scrollTop - ancestor.clientHeight / 2;
    ancestor.scrollTo({ top: Math.max(0, target) });
    return;
  }
}

/**
 * Renders `html` into `container`, then - if `highlightOffset`/`highlightLength` describe an
 * active match - wraps the located occurrence of `text.slice(highlightOffset, ...)` in a
 * `<mark class="fm-file-viewer-highlight">` and scrolls it into view. Always resets `innerHTML`
 * first, so a previous render's mark (or stale content from a different file/window) never
 * lingers.
 */
export function renderMarkdownWithHighlight(
  container: HTMLElement,
  html: string,
  text: string,
  highlightOffset: number | undefined,
  highlightLength: number | undefined,
): void {
  container.innerHTML = html;
  if (highlightOffset === undefined || highlightLength === undefined || highlightLength === 0) {
    return;
  }
  const needle = text.slice(highlightOffset, highlightOffset + highlightLength);
  const occurrenceIndex = occurrenceIndexOf(text, needle, highlightOffset);
  const range = findNthTextRange(container, needle, occurrenceIndex);
  if (range === undefined) return;
  const mark = document.createElement('mark');
  mark.className = HIGHLIGHT_CLASS;
  try {
    range.surroundContents(mark);
  } catch {
    // `findNthTextRange` only ever returns a range within a single text node, so this shouldn't
    // throw in practice - stays defensive rather than leaving the match entirely unhighlighted.
    return;
  }
  scrollElementIntoView(mark);
}
