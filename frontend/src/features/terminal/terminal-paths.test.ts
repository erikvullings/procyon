import { describe, expect, it } from 'vitest';
import { absolutePathOf, cdCommand, fileUriOfPath, shellQuote } from './terminal-paths';

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

describe('cdCommand and fileUriOfPath', () => {
  it('quotes the folder for the shell', () => {
    expect(cdCommand('/Users/me/docs')).toBe('cd -- /Users/me/docs');
    expect(cdCommand("/Users/me/It's here")).toBe("cd -- '/Users/me/It'\\''s here'");
  });

  it('round-trips a path through a file URI', () => {
    const path = '/Users/me/My Docs/ünï#1';
    expect(fileUriOfPath(path)).toBe('file:///Users/me/My%20Docs/%C3%BCn%C3%AF%231');
    expect(absolutePathOf(fileUriOfPath(path))).toBe(path);
  });
});
