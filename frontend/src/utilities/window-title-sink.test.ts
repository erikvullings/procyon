import { describe, expect, it, vi } from 'vitest';

import { createWindowTitleSink, dedupeWindowTitleSink } from './window-title-sink';

describe('window title sink', () => {
  it('routes titles through the client and swallows rejections', async () => {
    const setWindowTitle = vi.fn().mockRejectedValue(new Error('boom'));

    createWindowTitleSink({ setWindowTitle })('A');

    expect(setWindowTitle).toHaveBeenCalledWith('A');
  });

  it('tolerates clients without title support', () => {
    expect(() => createWindowTitleSink({})('A')).not.toThrow();
  });

  it('only fires when the title changes', () => {
    const sink = vi.fn();
    const deduped = dedupeWindowTitleSink(sink);
    deduped('A');
    deduped('A');
    deduped('B');
    expect(sink.mock.calls).toEqual([['A'], ['B']]);
  });
});
