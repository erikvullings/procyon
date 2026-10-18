import { describe, expect, it } from 'vitest';
import { type DetectedLink, detectLinkAt, detectLinkInLine } from './link-detection';

function at(line: string, needle: string): DetectedLink | undefined {
  return detectLinkInLine(line, line.indexOf(needle) + 1);
}

describe('detectLinkInLine: URLs', () => {
  it('finds http(s) URLs and drops sentence punctuation and unbalanced brackets', () => {
    expect(at('See https://example.com/a?b=1#c.', 'example')).toMatchObject({
      kind: 'url',
      url: 'https://example.com/a?b=1#c',
    });
    expect(at('(docs: https://example.com/page)', 'example')).toMatchObject({
      url: 'https://example.com/page',
    });
    expect(at('wiki https://en.wikipedia.org/wiki/Foo_(bar) ok', 'wikipedia')).toMatchObject({
      url: 'https://en.wikipedia.org/wiki/Foo_(bar)',
    });
  });

  it('does not treat other schemes as URLs to open', () => {
    expect(at('run javascript:alert(1) now', 'alert')).toBeUndefined();
  });
});

describe('detectLinkInLine: paths', () => {
  it('recognises absolute, relative, home, drive and file: paths', () => {
    expect(at('open /var/log/system.log now', 'var')).toMatchObject({
      kind: 'path',
      path: '/var/log/system.log',
    });
    expect(at('see ./src/main.rs', 'src')).toMatchObject({ path: './src/main.rs' });
    expect(at('cd ../up', 'up')).toMatchObject({ path: '../up' });
    expect(at('config ~/dotfiles/zshrc', 'dotfiles')).toMatchObject({ path: '~/dotfiles/zshrc' });
    expect(at(String.raw`C:\Users\me\a.txt failed`, 'Users')).toMatchObject({
      path: String.raw`C:\Users\me\a.txt`,
    });
    expect(at('file:///Users/me/My%20Doc.txt', 'Users')).toMatchObject({
      path: '/Users/me/My Doc.txt',
    });
  });

  it('reads path:line and path:line:col suffixes', () => {
    expect(at('error at src/app.ts:42:7: boom', 'app')).toMatchObject({
      path: 'src/app.ts',
      line: 42,
      column: 7,
    });
    expect(at('src/app.ts:42', 'app')).toMatchObject({ line: 42, column: undefined });
    expect(at(String.raw`C:\a\b.rs:10`, 'a')).toMatchObject({
      path: String.raw`C:\a\b.rs`,
      line: 10,
    });
  });

  it('finds bare relative paths only when they look like files', () => {
    expect(at('edit docs/readme.md please', 'docs')).toMatchObject({ path: 'docs/readme.md' });
    expect(at('use and/or here', 'and')).toBeUndefined();
    expect(at('ratio 1/2 and 3.5/4', '1/2')).toBeUndefined();
    expect(at('plain words only', 'words')).toBeUndefined();
    expect(at('// a comment', '//')).toBeUndefined();
  });

  it('strips delimiters such as quotes and Markdown link brackets', () => {
    expect(at('[doc](docs/a.md)', 'docs')).toMatchObject({ path: 'docs/a.md' });
    expect(at('path = "/etc/hosts";', 'etc')).toMatchObject({ path: '/etc/hosts' });
  });

  it('only matches when the cursor is on the token', () => {
    expect(detectLinkInLine('open /etc/hosts now', 2)).toBeUndefined();
    expect(detectLinkInLine('open /etc/hosts now', 5)).toMatchObject({ path: '/etc/hosts' });
  });
});

describe('detectLinkAt', () => {
  it('works across lines and reports document offsets', () => {
    const text = 'first line\nsee /tmp/x.txt:3 here\nlast';
    const position = text.indexOf('tmp');
    expect(detectLinkAt(text, position)).toMatchObject({
      path: '/tmp/x.txt',
      line: 3,
      from: text.indexOf('/tmp'),
    });
    expect(detectLinkAt(text, 2)).toBeUndefined();
  });
});
