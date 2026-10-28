import DOMPurify from 'dompurify';
import { render } from 'slimdown-js';

/** The DOMPurify configuration shared by Markdown, EPUB and plugin previews. */
export const PREVIEW_SANITIZE_OPTIONS = {
  USE_PROFILES: { html: true },
  FORBID_TAGS: ['script', 'style', 'iframe', 'object', 'embed', 'form', 'svg', 'math'],
  FORBID_ATTR: ['style'],
  ALLOWED_URI_REGEXP: /^(?:(?:https?|mailto):|[^a-z]|[a-z+.-]+(?:[^a-z+.-:]|$))/i,
};

/** slimdown leaves its list placeholder behind for a bullet without text. */
const LEAKED_LIST_PLACEHOLDER = /\{\{LISTITEM:[^{}]*\}\}/g;

export function safeMarkdownHtml(markdown: string): string {
  return DOMPurify.sanitize(
    render(markdown).replace(LEAKED_LIST_PLACEHOLDER, ''),
    PREVIEW_SANITIZE_OPTIONS,
  );
}
