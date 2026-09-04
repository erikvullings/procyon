import { describe, expect, it, vi } from 'vitest';
import {
  inlineEpubChapterImages,
  parseEpubContainer,
  parseEpubNavigationEntries,
  parseEpubPackage,
  parseEpubSectionLabels,
  repairEpubChapterOrder,
  resolveEpubPath,
  sanitizeEpubChapterHtml,
  sanitizeEpubSvg,
} from './epub-preview';

describe('parseEpubContainer', () => {
  it('reads the OPF full-path from container.xml', () => {
    const xml = `<?xml version="1.0"?>
      <container>
        <rootfiles>
          <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
        </rootfiles>
      </container>`;
    expect(parseEpubContainer(xml)).toBe('OEBPS/content.opf');
  });

  it('returns undefined when no rootfile is present', () => {
    expect(parseEpubContainer('<container><rootfiles/></container>')).toBeUndefined();
  });
});

describe('resolveEpubPath', () => {
  it('joins a base directory and a relative href', () => {
    expect(resolveEpubPath('OEBPS/', 'text/chapter1.xhtml')).toBe('OEBPS/text/chapter1.xhtml');
  });

  it('collapses .. segments', () => {
    expect(resolveEpubPath('OEBPS/text/', '../images/cover.jpg')).toBe('OEBPS/images/cover.jpg');
  });

  it('resolves against the archive root when the base directory is empty', () => {
    expect(resolveEpubPath('', 'content.opf')).toBe('content.opf');
  });
});

describe('parseEpubPackage', () => {
  const opfXml = `<?xml version="1.0"?>
    <package xmlns="http://www.idpf.org/2007/opf">
      <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
        <dc:title>My Book</dc:title>
      </metadata>
      <manifest>
        <item id="ch1" href="text/chapter1.xhtml" media-type="application/xhtml+xml"/>
        <item id="ch2" href="text/chapter2.xhtml" media-type="application/xhtml+xml"/>
        <item id="cover" href="images/cover.jpg" media-type="image/jpeg"/>
        <item id="css" href="styles/main.css" media-type="text/css"/>
      </manifest>
      <spine>
        <itemref idref="ch1"/>
        <itemref idref="ch2"/>
      </spine>
    </package>`;

  it('extracts the title and resolves the spine to archive-relative chapter paths, in order', () => {
    const book = parseEpubPackage(opfXml, 'OEBPS/content.opf');
    expect(book.title).toBe('My Book');
    expect(book.chapterPaths).toEqual(['OEBPS/text/chapter1.xhtml', 'OEBPS/text/chapter2.xhtml']);
    expect(book.imageResources).toEqual(
      new Map([
        ['OEBPS/images/cover.jpg', { path: 'OEBPS/images/cover.jpg', mediaType: 'image/jpeg' }],
      ]),
    );
  });

  it('excludes non-(X)HTML manifest items (images, stylesheets) from the chapter list', () => {
    const book = parseEpubPackage(opfXml, 'OEBPS/content.opf');
    expect(book.chapterPaths.some((path) => path.includes('cover.jpg'))).toBe(false);
    expect(book.chapterPaths.some((path) => path.includes('main.css'))).toBe(false);
  });

  it('has no title when dc:title is absent', () => {
    const book = parseEpubPackage('<package><manifest/><spine/></package>', 'content.opf');
    expect(book.title).toBeUndefined();
    expect(book.chapterPaths).toEqual([]);
    expect(book.imageResources.size).toBe(0);
  });

  it('maps an EPS resource to its browser-renderable manifest fallback', () => {
    const book = parseEpubPackage(
      '<package><manifest>' +
        '<item id="diagram" href="images/diagram.eps" media-type="application/postscript" fallback="diagram-png"/>' +
        '<item id="diagram-png" href="images/diagram.png" media-type="image/png"/>' +
        '</manifest><spine/></package>',
      'OEBPS/content.opf',
    );

    expect(book.imageResources.get('OEBPS/images/diagram.eps')).toEqual({
      path: 'OEBPS/images/diagram.png',
      mediaType: 'image/png',
    });
  });

  it('locates the EPUB 2 navigation document referenced by the spine', () => {
    const book = parseEpubPackage(
      '<package><manifest>' +
        '<item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>' +
        '<item id="contents" href="toc.ncx" media-type="application/x-dtbncx+xml"/>' +
        '</manifest><spine toc="contents"><itemref idref="chapter"/></spine></package>',
      'OEBPS/content.opf',
    );

    expect(book.navigationPath).toBe('OEBPS/toc.ncx');
  });
});

describe('repairEpubChapterOrder', () => {
  it('repairs a malformed spine when numbered TOC labels confirm natural file order', () => {
    const spine = [
      'titlepage.xhtml',
      'book_11_split_000.htm',
      'book_11_split_001.htm',
      'book_10_split_000.htm',
      'book_10_split_001.htm',
      'book_12_split_000.htm',
      'book_12_split_001.htm',
      'book_05_split_000.htm',
      'book_05_split_001.htm',
    ];
    const toc = `<ncx><navMap>
      <navPoint><navLabel><text>CHAPTER ELEVEN</text></navLabel><content src="book_11_split_001.htm"/></navPoint>
      <navPoint><navLabel><text>CHAPTER TEN</text></navLabel><content src="book_10_split_001.htm"/></navPoint>
      <navPoint><navLabel><text>CHAPTER TWELVE</text></navLabel><content src="book_12_split_001.htm"/></navPoint>
      <navPoint><navLabel><text>CHAPTER FIVE</text></navLabel><content src="book_05_split_001.htm"/></navPoint>
    </navMap></ncx>`;

    expect(repairEpubChapterOrder(spine, toc, 'toc.ncx')).toEqual([
      'titlepage.xhtml',
      'book_05_split_000.htm',
      'book_05_split_001.htm',
      'book_10_split_000.htm',
      'book_10_split_001.htm',
      'book_11_split_000.htm',
      'book_11_split_001.htm',
      'book_12_split_000.htm',
      'book_12_split_001.htm',
    ]);
  });

  it('keeps the explicit spine when TOC labels are not clearly numbered', () => {
    const spine = ['ending.xhtml', 'beginning.xhtml'];
    const toc =
      '<ncx><navMap><navPoint><navLabel><text>Final thoughts</text></navLabel>' +
      '<content src="ending.xhtml"/></navPoint></navMap></ncx>';

    expect(repairEpubChapterOrder(spine, toc, 'toc.ncx')).toEqual(spine);
  });
});

describe('parseEpubSectionLabels', () => {
  it('maps EPUB 2 NCX labels to resolved chapter paths', () => {
    const toc = `<ncx><navMap>
      <navPoint><navLabel><text>Introduction</text></navLabel><content src="text/intro.xhtml#start"/></navPoint>
      <navPoint><navLabel><text>Chapter One</text></navLabel><content src="text/chapter1.xhtml"/></navPoint>
    </navMap></ncx>`;

    expect(parseEpubSectionLabels(toc, 'OEBPS/toc.ncx')).toEqual(
      new Map([
        ['OEBPS/text/intro.xhtml', 'Introduction'],
        ['OEBPS/text/chapter1.xhtml', 'Chapter One'],
      ]),
    );
  });

  it('maps EPUB 3 navigation links and ignores empty labels', () => {
    const navigation = `<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
      <nav epub:type="toc"><ol>
        <li><a href="intro.xhtml">Introduction</a></li>
        <li><a href="chapter1.xhtml"><span>Chapter 1</span></a></li>
        <li><a href="empty.xhtml"> </a></li>
      </ol></nav>
    </body></html>`;

    expect(parseEpubSectionLabels(navigation, 'OEBPS/nav.xhtml')).toEqual(
      new Map([
        ['OEBPS/intro.xhtml', 'Introduction'],
        ['OEBPS/chapter1.xhtml', 'Chapter 1'],
      ]),
    );
  });
});

describe('parseEpubNavigationEntries', () => {
  it('preserves nested labels and fragments that target one spine document', () => {
    const toc = `<ncx><navMap>
      <navPoint><navLabel><text>1 Introduction</text></navLabel><content src="text/part0005.html"/>
        <navPoint><navLabel><text>1.1 Process</text></navLabel><content src="text/part0005.html#ch1.1"/></navPoint>
        <navPoint><navLabel><text>1.4 Summary</text></navLabel><content src="text/part0005.html#ch1.4"/></navPoint>
      </navPoint>
    </navMap></ncx>`;

    expect(parseEpubNavigationEntries(toc, 'toc.ncx')).toEqual([
      { label: '1 Introduction', level: 1, path: 'text/part0005.html' },
      { label: '1.1 Process', level: 2, path: 'text/part0005.html', fragment: 'ch1.1' },
      { label: '1.4 Summary', level: 2, path: 'text/part0005.html', fragment: 'ch1.4' },
    ]);
  });
});

describe('sanitizeEpubChapterHtml', () => {
  it('keeps ordinary content markup', () => {
    const html = sanitizeEpubChapterHtml('<p>Hello <em>world</em></p>');
    expect(html).toContain('Hello');
    expect(html).toContain('<em>world</em>');
  });

  it('strips scripts, embedded styles, and inline style attributes', () => {
    const html = sanitizeEpubChapterHtml(
      '<p style="color:red" onclick="evil()">Text</p><script>evil()</script><style>body{}</style>',
    );
    expect(html).not.toContain('<script');
    expect(html).not.toContain('<style');
    expect(html).not.toContain('style=');
    expect(html).not.toContain('onclick');
    expect(html).toContain('Text');
  });

  it('keeps safe inline SVG and MathML while removing active content', () => {
    const html = sanitizeEpubChapterHtml(
      '<svg viewBox="0 0 10 10" onload="evil()"><circle cx="5" cy="5" r="4"/>' +
        '<script>evil()</script><foreignObject><iframe srcdoc="evil()"></iframe></foreignObject>' +
        '</svg><math><mrow><mi>x</mi><mo>=</mo><mn>1</mn></mrow></math>',
    );

    expect(html).toContain('<svg');
    expect(html).toContain('<circle');
    expect(html).toContain('<math');
    expect(html).toContain('<mi>x</mi>');
    expect(html).not.toContain('onload');
    expect(html).not.toContain('<script');
    expect(html).not.toContain('foreignObject');
    expect(html).not.toContain('<iframe');
  });
});

describe('sanitizeEpubSvg', () => {
  it('keeps safe graphics while removing scripts, handlers, and unsafe links', () => {
    const svg = sanitizeEpubSvg(
      '<svg xmlns="http://www.w3.org/2000/svg" onload="evil()">' +
        '<defs><circle id="dot" cx="5" cy="5" r="4"/></defs><use href="#dot"/>' +
        '<script>evil()</script><a href="javascript:evil()"><circle cx="5" cy="5" r="4"/></a>' +
        '<image href="https://tracker.example/pixel.png"/>' +
        '</svg>',
    );

    expect(svg).toContain('<svg');
    expect(svg).toContain('<circle');
    expect(svg).toContain('href="#dot"');
    expect(svg).not.toContain('<script');
    expect(svg).not.toContain('onload');
    expect(svg).not.toContain('javascript:');
    expect(svg).not.toContain('tracker.example');
  });
});

describe('inlineEpubChapterImages', () => {
  it('inlines relative manifest images and deduplicates repeated reads', async () => {
    const readImage = vi.fn().mockResolvedValue('data:image/png;base64,iVBORw==');
    const html = await inlineEpubChapterImages(
      '<p><img src="../images/cover.png"><img src="../images/cover.png#page"></p>',
      'OEBPS/text/chapter.xhtml',
      new Map([
        ['OEBPS/images/cover.png', { path: 'OEBPS/images/cover.png', mediaType: 'image/png' }],
      ]),
      readImage,
    );

    expect(html.match(/src="data:image\/png;base64,iVBORw=="/g)).toHaveLength(2);
    expect(readImage).toHaveBeenCalledOnce();
    expect(readImage).toHaveBeenCalledWith('OEBPS/images/cover.png', 'image/png');
  });

  it('does not inline EPS resources that browser image elements cannot render', async () => {
    const readImage = vi.fn();
    const html = await inlineEpubChapterImages(
      '<img src="../images/diagram.eps" alt="Diagram">',
      'OEBPS/text/chapter.xhtml',
      new Map(),
      readImage,
    );

    expect(html).toContain('alt="Diagram"');
    expect(html).not.toContain('src=');
    expect(readImage).not.toHaveBeenCalled();
  });
});
