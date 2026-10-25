import { describe, expect, it } from 'vitest';

import type { RagScope } from '../../models';
import { mockCorpusThemes } from './mock-corpus-themes';
import { MockFileManagerClient } from './mock-file-manager-client';

const SCOPE = {
  kind: 'entireLibrary',
  workspaceId: 'workspace-1',
  label: 'Library',
  selectedFiles: [],
  semanticSourceIds: [],
  enrolledRootIds: [],
} as unknown as RagScope;

describe('mock corpus themes', () => {
  it('describes every chunk exactly once and never exposes vectors', () => {
    const themes = mockCorpusThemes();
    const assigned = themes.themes.reduce((total, theme) => total + theme.chunkCount, 0);
    expect(assigned + themes.noTheme.chunkCount).toBe(themes.chunkCount);
    expect(themes.themes.map((theme) => theme.id)).toEqual([0, 1, 2]);
    expect(
      themes.themes.every((theme, index, all) => {
        const previous = all[index - 1];
        return previous === undefined || previous.chunkCount >= theme.chunkCount;
      }),
    ).toBe(true);
    expect(JSON.stringify(themes)).not.toMatch(/"(vector|embedding|centroid)s?"\s*:/i);
  });

  it('mixtures reference known themes and sum to one', () => {
    const themes = mockCorpusThemes();
    const ids = new Set(themes.themes.map((theme) => theme.id));
    for (const mixture of themes.documentMixtures) {
      const total = mixture.shares.reduce((sum, share) => sum + share.share, 0);
      expect(total).toBeCloseTo(1, 5);
      for (const share of mixture.shares) {
        expect(share.theme === null || share.theme === undefined || ids.has(share.theme)).toBe(
          true,
        );
      }
    }
  });

  it('serves the same index revision from status and analysis', async () => {
    const client = new MockFileManagerClient();
    const status = await client.getCorpusThemeStatus({ scope: SCOPE });
    const response = await client.analyzeCorpusThemes({ scope: SCOPE });
    expect(response.outcome).toBe('ready');
    expect(response.themes?.indexRevision).toBe(status.indexRevision);
    expect(status.profileTooWeak).toBe(false);
  });
});
