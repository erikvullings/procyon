import { describe, expect, it } from 'vitest';
import { safeMarkdownHtml } from './markdown-preview';

describe('safeMarkdownHtml', () => {
  it('renders markdown and removes executable markup', () => {
    const html = safeMarkdownHtml('# Safe\n<script>alert(1)</script>');
    expect(html).toContain('<h1>Safe</h1>');
    expect(html).not.toContain('<script');
  });
});
