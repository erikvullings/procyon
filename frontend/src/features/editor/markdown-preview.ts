import DOMPurify from 'dompurify';
import { render } from 'slimdown-js';

export function safeMarkdownHtml(markdown: string): string {
  return DOMPurify.sanitize(render(markdown), {
    USE_PROFILES: { html: true },
    FORBID_TAGS: ['script', 'style', 'iframe', 'object', 'embed', 'form', 'svg', 'math'],
    FORBID_ATTR: ['style'],
    ALLOWED_URI_REGEXP: /^(?:(?:https?|mailto):|[^a-z]|[a-z+.-]+(?:[^a-z+.-:]|$))/i,
  });
}
