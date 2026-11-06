import { describe, expect, it } from 'vitest';

import { MockFileManagerClient } from './mock-file-manager-client';

describe('mock interrupted transfers', () => {
  it('lists nothing by default', async () => {
    const client = new MockFileManagerClient();
    expect((await client.listInterruptedTransfers()).transfers).toEqual([]);
  });

  it('rejects an unknown transfer for both actions', async () => {
    const client = new MockFileManagerClient();
    await expect(client.restartInterruptedTransfer('nope')).rejects.toMatchObject({
      code: 'notFound',
    });
    await expect(client.discardInterruptedTransfer('nope')).rejects.toMatchObject({
      code: 'notFound',
    });
  });
});
