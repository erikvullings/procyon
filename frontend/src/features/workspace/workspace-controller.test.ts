import { describe, expect, it } from 'vitest';

import { locationsMatch } from './workspace-controller';

describe('native drag location matching', () => {
  it('matches equivalent local paths after a native macOS round trip', () => {
    expect(
      locationsMatch(
        [{ providerId: 'local', uri: 'file:///Users/example/caf%C3%A9.txt' }],
        [{ providerId: 'local', uri: 'file:///Users/example/cafe\u0301.txt' }],
      ),
    ).toBe(true);
  });

  it('does not match different providers or native paths', () => {
    expect(
      locationsMatch(
        [{ providerId: 'local', uri: 'file:///Users/example/report.txt' }],
        [{ providerId: 'archive', uri: 'file:///Users/example/report.txt' }],
      ),
    ).toBe(false);
    expect(
      locationsMatch(
        [{ providerId: 'local', uri: 'file:///Users/example/report.txt' }],
        [{ providerId: 'local', uri: 'file:///Users/example/other.txt' }],
      ),
    ).toBe(false);
  });
});
