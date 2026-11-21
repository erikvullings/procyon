import DOMPurify from 'dompurify';
import { render } from 'slimdown-js';
import { metadataTableHtml, splitMetadata } from './markdown-metadata';
import { wikilinksToHtml } from './markdown-wikilinks';
import './markdown-metadata.css';

/** The DOMPurify configuration shared by Markdown, EPUB and plugin previews. */
export const PREVIEW_SANITIZE_OPTIONS = {
  USE_PROFILES: { html: true },
  FORBID_TAGS: ['script', 'style', 'iframe', 'object', 'embed', 'form', 'svg', 'math'],
  FORBID_ATTR: ['style'],
  // DOMPurify's own default shape: a relative URL is anything whose leading run of scheme
  // characters is not followed by `:`. The hyphen is escaped: an unescaped `.-:` is a range
  // that also covers `/` and digits, which stripped relative links such as `docs/a.md`.
  ALLOWED_URI_REGEXP: /^(?:(?:https?|mailto):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i,
};

/** slimdown leaves its list placeholder behind for a bullet without text. */
const LEAKED_LIST_PLACEHOLDER = /\{\{LISTITEM:[^{}]*\}\}/g;

function renderMarkdown(markdown: string): string {
  return render(wikilinksToHtml(markdown)).replace(LEAKED_LIST_PLACEHOLDER, '');
}

/** Markdown as sanitized HTML; front matter or a TASKS-style `Key: value` header becomes a table. */
export function safeMarkdownHtml(markdown: string): string {
  const metadata = splitMetadata(markdown);
  const html =
    metadata === undefined
      ? renderMarkdown(markdown)
      : (metadata.before.trim() === '' ? '' : renderMarkdown(metadata.before)) +
        metadataTableHtml(metadata.fields) +
        renderMarkdown(metadata.after);
  return DOMPurify.sanitize(html, PREVIEW_SANITIZE_OPTIONS);
}
