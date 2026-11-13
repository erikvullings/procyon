import { describe, expect, it } from 'vitest';
import { applyTypedInput, EMPTY_INPUT, suggestionRemainder } from './typed-input';

describe('applyTypedInput', () => {
  it('builds the line as it is typed and reports it on Enter', () => {
    const typed = applyTypedInput(EMPTY_INPUT, 'ls -l');
    expect(typed.state).toEqual({ text: 'ls -l', known: true });
    const submitted = applyTypedInput(typed.state, '\r');
    expect(submitted.submitted).toBe('ls -l');
    expect(submitted.state).toEqual({ text: '', known: true });
  });

  it('removes characters on backspace, by code point', () => {
    const typed = applyTypedInput(EMPTY_INPUT, 'a🙂\x7f');
    expect(typed.state.text).toBe('a');
  });

  it('clears the line on Ctrl+C and Ctrl+U', () => {
    expect(applyTypedInput(EMPTY_INPUT, 'abc\x03').state.text).toBe('');
    expect(applyTypedInput(EMPTY_INPUT, 'abc\x15').state.text).toBe('');
  });

  it('marks the line unknown after completion or cursor movement until Enter', () => {
    const afterTab = applyTypedInput(EMPTY_INPUT, 'gi\t');
    expect(afterTab.state.known).toBe(false);
    expect(applyTypedInput(afterTab.state, '\r').submitted).toBeUndefined();
    expect(applyTypedInput(EMPTY_INPUT, 'x\x1b[A').state.known).toBe(false);
  });
});

describe('suggestionRemainder', () => {
  const history = ['git status', 'git stash', 'ls'];

  it('suggests the most recent earlier command that extends the typed text', () => {
    expect(suggestionRemainder(history, { text: 'git s', known: true })).toBe('tash');
  });

  it('does not suggest for unknown text, an exact match, or a # question', () => {
    expect(suggestionRemainder(history, { text: 'git s', known: false })).toBeUndefined();
    expect(suggestionRemainder(history, { text: 'ls', known: true })).toBeUndefined();
    expect(suggestionRemainder(history, { text: '#ls', known: true })).toBeUndefined();
  });
});
