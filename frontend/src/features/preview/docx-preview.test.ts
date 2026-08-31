import { describe, expect, it, vi } from 'vitest';

import { prepareDocxPreviewHtml, searchDocxHtml } from './docx-preview';

describe('prepareDocxPreviewHtml', () => {
  it('preserves semantic content while stripping active content and arbitrary resource loads', async () => {
    const html = await prepareDocxPreviewHtml(
      '<h1>Report</h1><p onclick="evil()">Hello <strong>world</strong></p>' +
        '<script>evil()</script><iframe src="https://evil.example"></iframe>' +
        '<img src="https://tracker.example/pixel.png"><img src="missing.png" alt="Missing">',
      [],
      vi.fn(),
    );

    expect(html).toContain('<h1>Report</h1>');
    expect(html).toContain('<strong>world</strong>');
    expect(html).not.toContain('script');
    expect(html).not.toContain('iframe');
    expect(html).not.toContain('onclick');
    expect(html).not.toContain('tracker.example');
    expect(html).toContain('alt="Missing"');
  });

  describe('searchDocxHtml', () => {
    it('highlights and navigates matches without flattening semantic markup', () => {
      const result = searchDocxHtml(
        '<h1>Report</h1><p>Quarterly report</p>',
        'report',
        false,
        false,
        false,
        1,
      );

      expect(result.matches).toHaveLength(2);
      expect(result.html).toContain('<h1><mark class="fm-docx-search-match">Report</mark></h1>');
      expect(result.html).toContain('<mark class="fm-docx-search-match-active">report</mark>');
    });

    it('supports regex, case-sensitive, and whole-word search options', () => {
      const result = searchDocxHtml('<p>Cat category cat</p>', 'C.t', true, true, true, 0);

      expect(result.matches).toHaveLength(1);
      expect(result.html).toContain('fm-docx-search-match-active');
    });

    it('finds phrases that cross inline formatting boundaries', () => {
      const result = searchDocxHtml(
        '<p>Hello <strong>formatted</strong> world</p>',
        'Hello formatted world',
        false,
        true,
        false,
        0,
      );

      expect(result.matches).toHaveLength(1);
      expect(result.html).toContain(
        '<strong><mark class="fm-docx-search-match-active">formatted</mark>',
      );
    });

    it('caps match highlighting to keep search rendering bounded', () => {
      const result = searchDocxHtml(`<p>${'x '.repeat(5_001)}</p>`, 'x', false, true, false, 0);

      expect(result.matches).toHaveLength(5_000);
      expect(result.truncated).toBe(true);
    });
  });

  it('fetches each declared package image by opaque id and inlines bounded data', async () => {
    const readResource = vi.fn().mockResolvedValue({
      data: [137, 80, 78, 71],
      mediaType: 'image/png',
    });
    const html = await prepareDocxPreviewHtml(
      '<p><img src="media/image1.png"><img src="media/image1.png"></p>',
      [
        {
          resourceId: 'image-1',
          source: 'media/image1.png',
          mediaType: 'image/png',
          byteLength: 4,
        },
      ],
      readResource,
    );

    expect(html.match(/src="data:image\/png;base64,/g)).toHaveLength(2);
    expect(readResource).toHaveBeenCalledOnce();
    expect(readResource).toHaveBeenCalledWith('image-1');
  });

  it('hardens external links and leaves internal bookmarks in-app', async () => {
    const html = await prepareDocxPreviewHtml(
      '<a href="https://example.com">External</a><a href="#section">Internal</a>' +
        '<a href="javascript:evil()">Unsafe</a>',
      [],
      vi.fn(),
    );

    const document = new DOMParser().parseFromString(html, 'text/html');
    const [external, internal, unsafe] = Array.from(document.querySelectorAll('a'));
    expect(external?.getAttribute('target')).toBe('_blank');
    expect(external?.getAttribute('rel')).toBe('noopener noreferrer');
    expect(internal?.hasAttribute('target')).toBe(false);
    expect(unsafe?.hasAttribute('href')).toBe(false);
  });
});
