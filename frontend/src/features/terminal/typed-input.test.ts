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

  it("prefers this session's commands over older shell history", () => {
    // The drawer puts the shell's history first and this session's commands last.
    const shellHistory = ['git stash list', 'git status --short'];
    const session = ['git stash'];
    expect(suggestionRemainder([...shellHistory, ...session], { text: 'git s', known: true })).toBe(
      'tash',
    );
    expect(suggestionRemainder(shellHistory, { text: 'git s', known: true })).toBe('tatus --short');
  });

  it('does not suggest for unknown text, an exact match, or a # question', () => {
    expect(suggestionRemainder(history, { text: 'git s', known: false })).toBeUndefined();
    expect(suggestionRemainder(history, { text: 'ls', known: true })).toBeUndefined();
    expect(suggestionRemainder(history, { text: '#ls', known: true })).toBeUndefined();
  });
});

describe('bracketed paste', () => {
  it('keeps the line known and takes the pasted text literally', () => {
    const typed = applyTypedInput(EMPTY_INPUT, '# ');
    const pasted = applyTypedInput(
      typed.state,
      '\x1b[200~For each file,\nwrite a summary\x1b[201~',
    );
    expect(pasted.state).toEqual({ text: '# For each file, write a summary', known: true });
    expect(pasted.submitted).toBeUndefined();
  });

  it('follows a paste split over several chunks, and a paste with no leading text', () => {
    let state = applyTypedInput(EMPTY_INPUT, '\x1b[200~#long ').state;
    expect(state.pasting).toBe(true);
    state = applyTypedInput(state, 'question').state;
    state = applyTypedInput(state, '\x1b[201~').state;
    expect(state).toEqual({ text: '#long question', known: true });
    expect(applyTypedInput(state, '\r').submitted).toBe('#long question');
  });

  it('a # with no space after it is still a question line', () => {
    expect(applyTypedInput(EMPTY_INPUT, '#xxx').state.text).toBe('#xxx');
  });
});
