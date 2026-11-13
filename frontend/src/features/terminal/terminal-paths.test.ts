import { describe, expect, it } from 'vitest';
import { absolutePathOf, shellQuote } from './terminal-paths';

describe('absolutePathOf', () => {
  it('decodes a local file location to its absolute path', () => {
    expect(absolutePathOf('file:///Users/me/My%20Files')).toBe('/Users/me/My Files');
  });

  it('takes the remote path from an sftp location', () => {
    expect(absolutePathOf('sftp://0d1c2b3a-0000-0000-0000-000000000000/home/me/a%20b')).toBe(
      '/home/me/a b',
    );
  });

  it('has no path for other schemes or malformed escapes', () => {
    expect(absolutePathOf('archive:///x.zip!/docs')).toBeUndefined();
    expect(absolutePathOf('file:///bad%E0%A4%A')).toBeUndefined();
  });
});

describe('shellQuote', () => {
  it('leaves plain paths unquoted', () => {
    expect(shellQuote('/usr/local/bin/git')).toBe('/usr/local/bin/git');
  });

  it('quotes spaces and special characters, and escapes single quotes', () => {
    expect(shellQuote('/Users/me/My File.txt')).toBe("'/Users/me/My File.txt'");
    expect(shellQuote("/tmp/it's here")).toBe("'/tmp/it'\\''s here'");
  });
});
