import { afterEach, describe, expect, it, vi } from 'vitest';
import { type EditCommand, replayEditCommand } from './edit-command';

afterEach(() => {
  document.body.replaceChildren();
});

describe('replayEditCommand', () => {
  it('replays the command as a Cmd keydown on the focused element', () => {
    const target = document.createElement('section');
    document.body.appendChild(target);
    const seen: string[] = [];
    target.addEventListener('keydown', (event) => {
      if (event.metaKey) seen.push(event.key);
    });
    for (const command of ['cut', 'copy', 'paste', 'selectAll'] as const)
      replayEditCommand(command, target, () => undefined);
    expect(seen).toEqual(['x', 'c', 'v', 'a']);
  });

  it('skips the native fallback when a page handler claims the keystroke', () => {
    document.addEventListener('keydown', (event) => event.preventDefault(), { once: true });
    const fallback = vi.fn<(command: EditCommand) => void>();
    replayEditCommand('copy', null, fallback);
    expect(fallback).not.toHaveBeenCalled();
  });

  it('falls back to the native command when no page handler claims the keystroke', () => {
    const fallback = vi.fn<(command: EditCommand) => void>();
    replayEditCommand('paste', null, fallback);
    expect(fallback).toHaveBeenCalledExactlyOnceWith('paste');
  });
});
