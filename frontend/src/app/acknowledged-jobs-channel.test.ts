import { describe, expect, it } from 'vitest';
import { openAcknowledgedJobsChannel } from './acknowledged-jobs-channel';

describe('acknowledged jobs channel', () => {
  it('reaches other windows but not the acknowledging one', async () => {
    const seenByA: string[] = [];
    const seenByB: string[] = [];
    const a = openAcknowledgedJobsChannel((id) => seenByA.push(id));
    const b = openAcknowledgedJobsChannel((id) => seenByB.push(id));
    a.acknowledge('job-1');
    await new Promise((resolve) => setTimeout(resolve, 50));
    a.close();
    b.close();
    expect(seenByB).toEqual(['job-1']);
    expect(seenByA).toEqual([]);
  });
});
