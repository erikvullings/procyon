import { describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../api/client/mock-file-manager-client';
import { createKnowledgeWiring } from './knowledge-wiring';
import { createShellState } from './shell-state';

describe('knowledge wiring: corpus themes availability', () => {
  async function setup(roots: number) {
    const client = new MockFileManagerClient();
    const workspace = await client.startWorkspace();
    const shellState = createShellState(client);
    shellState.workspace = workspace;
    vi.spyOn(client, 'getKnowledgeCapabilities').mockResolvedValue({
      fullText: true,
      semantic: true,
    } as Awaited<ReturnType<typeof client.getKnowledgeCapabilities>>);
    vi.spyOn(client, 'listKnowledgeRoots').mockResolvedValue(
      Array.from({ length: roots }, (_, index) => ({
        rootId: `root-${index}`,
      })) as unknown as Awaited<ReturnType<typeof client.listKnowledgeRoots>>,
    );
    const wiring = createKnowledgeWiring(shellState, {} as never);
    await wiring.refreshKnowledgeSearchAvailability();
    return shellState;
  }

  it('offers themes only when the workspace has an indexed folder', async () => {
    const withIndex = await setup(1);
    expect(withIndex.themesAvailable).toBe(true);
    expect(withIndex.themesWorkspaceIndexed).toBe(true);

    const withoutIndex = await setup(0);
    expect(withoutIndex.themesAvailable).toBe(true);
    expect(withoutIndex.themesWorkspaceIndexed).toBe(false);
  });
});
