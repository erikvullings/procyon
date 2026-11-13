import { describe, expect, it, vi } from 'vitest';
import { createBlockTracker } from './block-tracker';

describe('createBlockTracker', () => {
  it('finishes a block from prompt, output and end marks', () => {
    let handle = 0;
    const tracker = createBlockTracker<string>({ maxBlocks: 10 });
    const next = () => {
      handle += 1;
      return `m${handle}`;
    };
    tracker.apply({ kind: 'prompt' }, next, () => '');
    tracker.apply({ kind: 'output' }, next, () => 'ls -la');
    const block = tracker.apply({ kind: 'end', exitCode: 0 }, next, () => '');

    expect(block).toEqual({
      id: 1,
      command: 'ls -la',
      promptMark: 'm1',
      outputMark: 'm2',
      endMark: 'm3',
      exitCode: 0,
    });
    expect(tracker.blocks).toHaveLength(1);
  });

  it('produces no block for an empty Enter at the prompt', () => {
    const tracker = createBlockTracker<string>({ maxBlocks: 10 });
    tracker.apply(
      { kind: 'prompt' },
      () => 'p',
      () => '',
    );
    expect(
      tracker.apply(
        { kind: 'end', exitCode: 0 },
        () => 'e',
        () => '',
      ),
    ).toBeUndefined();
    expect(tracker.blocks).toHaveLength(0);
  });

  it('ignores an end mark with no open block', () => {
    const tracker = createBlockTracker<string>({ maxBlocks: 10 });
    expect(
      tracker.apply(
        { kind: 'end', exitCode: 1 },
        () => 'e',
        () => '',
      ),
    ).toBeUndefined();
  });

  it('keeps the newest blocks and reports the evicted ones', () => {
    const onEvict = vi.fn();
    const tracker = createBlockTracker<number>({ maxBlocks: 2, onEvict });
    let mark = 0;
    for (let index = 0; index < 3; index += 1) {
      tracker.apply(
        { kind: 'prompt' },
        () => (mark += 1),
        () => '',
      );
      tracker.apply(
        { kind: 'output' },
        () => (mark += 1),
        () => `cmd ${index}`,
      );
      tracker.apply(
        { kind: 'end', exitCode: 0 },
        () => (mark += 1),
        () => '',
      );
    }
    expect(tracker.blocks.map((block) => block.command)).toEqual(['cmd 1', 'cmd 2']);
    expect(onEvict).toHaveBeenCalledTimes(1);
    expect(onEvict.mock.calls[0]?.[0].command).toBe('cmd 0');
  });
});
