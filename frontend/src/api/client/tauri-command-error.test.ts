import { describe, expect, it } from 'vitest';
import { TauriCommandError, toCommandError } from './tauri-command-error';

describe('toCommandError', () => {
  it('keeps the message and structured fields of an application error object', () => {
    const error = toCommandError('invoke_action', {
      code: 'ACTION_UNAVAILABLE',
      message: 'The action is not available here.',
      requestId: 'r1',
      details: { a: 1 },
    });
    expect(error).toBeInstanceOf(TauriCommandError);
    expect(error.message).toBe('The action is not available here.');
    expect(error).toMatchObject({ code: 'ACTION_UNAVAILABLE', requestId: 'r1', details: { a: 1 } });
  });

  it('wraps string rejections and passes real errors through', () => {
    expect(toCommandError('x', 'boom').message).toBe('boom');
    const original = new Error('already');
    expect(toCommandError('x', original)).toBe(original);
  });

  it('never yields an empty message', () => {
    expect(toCommandError('open_it', {}).message).toBe('open_it failed');
    expect(toCommandError('open_it', undefined).message).toBe('open_it failed');
  });
});
