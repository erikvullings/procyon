import { describe, expect, it, vi } from 'vitest';

import { preparePptxSlideHtml } from './pptx-preview';

describe('preparePptxSlideHtml', () => {
  it('sanitizes unsafe links and resolves only declared embedded images', async () => {
    const readResource = vi.fn().mockResolvedValue({
      data: [137, 80, 78, 71],
      mediaType: 'image/png',
    });

    const html = await preparePptxSlideHtml(
      '[Safe](https://example.com) [Unsafe](javascript:evil())\n\n' +
        '![Chart](pptx-resource:../media/chart.png)\n\n' +
        '![Missing](pptx-resource:../media/missing.png)',
      [
        {
          resourceId: 'chart',
          source: '../media/chart.png',
          mediaType: 'image/png',
          byteLength: 4,
        },
      ],
      readResource,
    );

    const document = new DOMParser().parseFromString(html, 'text/html');
    const [safe, unsafe] = Array.from(document.querySelectorAll('a'));
    expect(safe?.getAttribute('href')).toBe('https://example.com');
    expect(safe?.getAttribute('rel')).toBe('noopener noreferrer');
    expect(unsafe?.hasAttribute('href')).toBe(false);
    expect(document.querySelector('img[alt="Chart"]')?.getAttribute('src')).toMatch(
      /^data:image\/png;base64,/,
    );
    expect(document.querySelector('img[alt="Missing"]')?.hasAttribute('src')).toBe(false);
    expect(readResource).toHaveBeenCalledOnce();
  });
});
