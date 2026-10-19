import { describe, expect, it } from 'vitest';
import type { EntrySummary, PluginDescriptor } from '../../models';
import { previewPluginFor, previewPluginRoutes, sanitizePluginPreviewHtml } from './plugin-preview';

function file(name: string, extension?: string): EntrySummary {
  return {
    id: name,
    location: { providerId: 'local', uri: `file:///tmp/${name}` },
    name,
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
    ...(extension === undefined ? {} : { extension }),
  };
}

function plugin(id: string, enabled: boolean, extensions?: string[]): PluginDescriptor {
  return {
    id,
    name: id,
    version: '1',
    description: '',
    enabled,
    ...(extensions === undefined ? {} : { preview: { extensions } }),
  };
}

describe('previewPluginRoutes', () => {
  it('lists only enabled plugins that contribute a preview', () => {
    expect(
      previewPluginRoutes([
        plugin('a', true, ['torrent']),
        plugin('b', false, ['eml']),
        plugin('c', true),
      ]),
    ).toEqual([{ pluginId: 'a', extensions: ['torrent'] }]);
  });

  it('treats the backend null preview as no preview', () => {
    const nullPreview = { ...plugin('d', true), preview: null } as unknown as PluginDescriptor;
    expect(previewPluginRoutes([nullPreview])).toEqual([]);
  });
});

describe('previewPluginFor', () => {
  const routes = [{ pluginId: 'a', extensions: ['torrent', 'rs', 'txt', 'eml'] }];

  it('matches a claimed extension case-insensitively, with or without entry.extension', () => {
    expect(previewPluginFor(file('x.TORRENT'), routes)?.pluginId).toBe('a');
    expect(previewPluginFor(file('x.eml', 'eml'), routes)?.pluginId).toBe('a');
  });

  it('never shadows extensions the built-in text viewer already handles', () => {
    expect(previewPluginFor(file('main.rs'), routes)).toBeUndefined();
    expect(previewPluginFor(file('notes.txt'), routes)).toBeUndefined();
  });

  it('ignores unclaimed or extensionless files', () => {
    expect(previewPluginFor(file('x.bin'), routes)).toBeUndefined();
    expect(previewPluginFor(file('README'), routes)).toBeUndefined();
  });
});

describe('sanitizePluginPreviewHtml', () => {
  it('keeps benign structure', () => {
    expect(sanitizePluginPreviewHtml('<h1>T</h1><table><tr><td>x</td></tr></table>')).toContain(
      '<td>x</td>',
    );
  });

  it.each([
    ['script', '<p>a</p><script>alert(1)</script>'],
    ['inline handler', '<p onclick="alert(1)">a</p>'],
    ['javascript url', '<a href="javascript:alert(1)">a</a>'],
    ['iframe', '<iframe src="https://evil.test"></iframe>'],
    ['style attribute', '<p style="background:url(https://evil.test/x)">a</p>'],
    ['remote image', '<img src="https://evil.test/pixel.gif">'],
    ['svg', '<svg onload="alert(1)"><circle/></svg>'],
    ['form', '<form action="https://evil.test"><input></form>'],
    ['image input', '<input type="image" src="https://evil.test/a.png">'],
    [
      'background attribute',
      '<table background="https://evil.test/b.png"><tr><td>x</td></tr></table>',
    ],
    ['live link', '<a href="https://evil.test/track">x</a>'],
  ])('strips %s', (_name, html) => {
    const clean = sanitizePluginPreviewHtml(html);
    expect(clean).not.toMatch(
      /script|onclick|onload|javascript:|iframe|<img|<svg|<form|<input|style=|href=|src=|background=|evil\.test/i,
    );
  });
});
