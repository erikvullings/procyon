import { describe, expect, it } from 'vitest';
import type { Location } from '../../models';
import { rootLocationFor } from './root-location';

describe('rootLocationFor', () => {
  it('reduces a local file URI to its scheme root', () => {
    const location: Location = { providerId: 'local', uri: 'file:///a/b/c' };
    expect(rootLocationFor(location)).toEqual({ providerId: 'local', uri: 'file:///' });
  });

  it('reduces an sftp URI to its connection root', () => {
    const location: Location = { providerId: 'sftp', uri: 'sftp://connection-1/home/user/docs' };
    expect(rootLocationFor(location)).toEqual({
      providerId: 'sftp',
      uri: 'sftp://connection-1/',
    });
  });

  it('is idempotent once already at the root', () => {
    const location: Location = { providerId: 'local', uri: 'file:///' };
    expect(rootLocationFor(location)).toEqual({ providerId: 'local', uri: 'file:///' });
  });

  it('falls back to a bare slash for a URI with no scheme', () => {
    const location: Location = { providerId: 'local', uri: '/a/b' };
    expect(rootLocationFor(location)).toEqual({ providerId: 'local', uri: '/' });
  });
});
