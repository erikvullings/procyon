import { describe, expect, it } from 'vitest';

import { walkthroughForHref, walkthroughs } from './walkthroughs';

describe('bundled walkthroughs', () => {
  it('includes the agreed task guides with meaningful offline content', () => {
    expect(walkthroughs.map((guide) => guide.id)).toEqual([
      'basics',
      'finding-files',
      'ssh-sftp',
      'remote-transfer',
      'semantic-search',
      'collection-basket',
      'workspaces',
    ]);
    expect(walkthroughs.every((guide) => guide.markdown.length > 250)).toBe(true);
  });

  it('resolves only bundled cross-guide links', () => {
    expect(walkthroughForHref('ssh-sftp.md')).toBe('ssh-sftp');
    expect(walkthroughForHref('https://example.com')).toBeUndefined();
  });
});
