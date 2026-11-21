import { describe, expect, it } from 'vitest';
import { safeMarkdownHtml } from './markdown-preview';

describe('safeMarkdownHtml', () => {
  it('renders markdown and removes executable markup', () => {
    const html = safeMarkdownHtml('# Safe\n<script>alert(1)</script>');
    expect(html).toContain('<h1>Safe</h1>');
    expect(html).not.toContain('<script');
  });

  it('does not leak the list placeholder of a bullet without text', () => {
    expect(safeMarkdownHtml('before\n\n-\n\nafter')).not.toContain('LISTITEM');
  });

  it('keeps relative links with folders but drops script URLs', () => {
    expect(safeMarkdownHtml('[a](docs/a.md) [b](0336-x.md)')).toContain('href="docs/a.md"');
    expect(safeMarkdownHtml('[b](0336-x.md)')).toContain('href="0336-x.md"');
    expect(safeMarkdownHtml('[c](javascript:alert(1))')).not.toContain('javascript:');
  });
});
