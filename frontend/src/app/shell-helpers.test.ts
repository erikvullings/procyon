import { describe, expect, it } from 'vitest';
import { sameFileLocation } from './shell-helpers';

describe('sameFileLocation', () => {
  it('matches the same provider and file, whatever the URI encoding', () => {
    const left = { providerId: 'local', uri: 'file:///tmp/My%20Note.md' } as const;
    expect(sameFileLocation(left, { providerId: 'local', uri: 'file:///tmp/My Note.md' })).toBe(
      true,
    );
  });

  it('tells other files and other providers apart', () => {
    const left = { providerId: 'local', uri: 'file:///tmp/a.md' } as const;
    expect(sameFileLocation(left, { providerId: 'local', uri: 'file:///tmp/b.md' })).toBe(false);
    expect(sameFileLocation(left, { providerId: 'sftp', uri: 'file:///tmp/a.md' })).toBe(false);
  });
});
