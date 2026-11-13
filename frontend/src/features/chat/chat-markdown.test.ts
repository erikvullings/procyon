import { describe, expect, it } from 'vitest';
import { safeMarkdownHtml } from '../editor/markdown-preview';

describe('chat replies as Markdown', () => {
  it('renders emphasis, code and nested lists', () => {
    const html = safeMarkdownHtml(
      '**Key:** `chatris.csr`\n\n1.  **Name:** one\n    *   nested\n    *   nested two',
    );
    expect(html).toContain('<strong>Key:</strong>');
    expect(html).toContain('<code>chatris.csr</code>');
    expect(html).toMatch(/<ol>.*<ul>.*nested two.*<\/ul>.*<\/ol>/s);
  });

  it('never keeps script from a reply', () => {
    expect(safeMarkdownHtml('Hello <script>alert(1)</script>')).not.toContain('<script');
  });
});
