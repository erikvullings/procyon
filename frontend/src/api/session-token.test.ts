import { afterEach, describe, expect, it } from 'vitest';

import { getSessionToken, resetSessionTokenForTests, setSessionToken } from './session-token';

afterEach(() => {
  resetSessionTokenForTests();
});

describe('session-token', () => {
  it('returns undefined when no token has been set', () => {
    expect(getSessionToken()).toBeUndefined();
  });

  it('returns a token after it is set', () => {
    setSessionToken('abc123');
    expect(getSessionToken()).toBe('abc123');
  });

  it('persists the token in sessionStorage so a reload can recover it', () => {
    setSessionToken('abc123');
    expect(sessionStorage.getItem('fm.sessionToken')).toBe('abc123');
  });

  it('clears the token from sessionStorage when set to undefined', () => {
    setSessionToken('abc123');
    setSessionToken(undefined);
    expect(getSessionToken()).toBeUndefined();
    expect(sessionStorage.getItem('fm.sessionToken')).toBeNull();
  });

  it('treats an empty string the same as clearing the token', () => {
    setSessionToken('abc123');
    setSessionToken('');
    expect(getSessionToken()).toBeUndefined();
  });
});
