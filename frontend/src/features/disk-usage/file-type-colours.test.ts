import { describe, expect, it } from 'vitest';
import type { DiskUsageNode } from '../../models';
import {
  DEFAULT_TREEMAP_COLOURS,
  fileExtension,
  fileTypeGroup,
  hashedExtensionColour,
  nodeColour,
  parseCssColour,
} from './file-type-colours';

function node(name: string, kind: DiskUsageNode['kind'] = 'file'): DiskUsageNode {
  return {
    name,
    kind,
    location: { providerId: 'local', uri: `file:///tmp/${name}` },
    logicalBytes: 1,
    physicalBytes: 1,
    collapsed: false,
    children: [],
  };
}

describe('file type colours', () => {
  it('extracts lowercased extensions and ignores dotfiles', () => {
    expect(fileExtension('Movie.MKV')).toBe('mkv');
    expect(fileExtension('.bashrc')).toBe('');
    expect(fileExtension('README')).toBe('');
    expect(fileExtension('trailing.')).toBe('');
  });

  it('groups common file types', () => {
    expect(fileTypeGroup('clip.mov')).toBe('video');
    expect(fileTypeGroup('photo.HEIC')).toBe('image');
    expect(fileTypeGroup('song.flac')).toBe('audio');
    expect(fileTypeGroup('backup.tar')).toBe('archive');
    expect(fileTypeGroup('libfoo.dylib')).toBe('binary');
    expect(fileTypeGroup('main.rs')).toBe('code');
    expect(fileTypeGroup('report.pdf')).toBe('document');
    expect(fileTypeGroup('index.sqlite')).toBe('database');
    expect(fileTypeGroup('system.log')).toBe('system');
    expect(fileTypeGroup('unknown.qqq')).toBe('other');
  });

  it('gives unknown extensions a stable distinct colour and directories their own colour', () => {
    expect(hashedExtensionColour('qqq')).toEqual(hashedExtensionColour('qqq'));
    expect(hashedExtensionColour('qqq')).not.toEqual(hashedExtensionColour('zzz'));
    expect(nodeColour(node('a.qqq'), DEFAULT_TREEMAP_COLOURS)).toEqual(
      hashedExtensionColour('qqq'),
    );
    expect(nodeColour(node('Makefile'), DEFAULT_TREEMAP_COLOURS)).toEqual(
      DEFAULT_TREEMAP_COLOURS.other,
    );
    expect(nodeColour(node('src.mp4', 'directory'), DEFAULT_TREEMAP_COLOURS)).toEqual(
      DEFAULT_TREEMAP_COLOURS.directory,
    );
  });

  it('parses computed CSS colours', () => {
    expect(parseCssColour('#fff')).toEqual([255, 255, 255]);
    expect(parseCssColour(' #102030 ')).toEqual([16, 32, 48]);
    expect(parseCssColour('rgb(1, 2, 3)')).toEqual([1, 2, 3]);
    expect(parseCssColour('rgba(4 5 6 / 50%)')).toEqual([4, 5, 6]);
    expect(parseCssColour('')).toBeUndefined();
  });
});
