import { afterEach, describe, expect, it } from 'vitest';

import { operationEntryErrorText, setLocale } from './index';

describe('transferRetried entry text', () => {
  afterEach(() => setLocale('en'));

  const entry = {
    code: 'transferRetried',
    message: 'fallback',
    params: { attempt: 2, max: 4, seconds: 1, cause: 'connection reset' },
  };

  it('names the attempt, the wait and the cause', () => {
    setLocale('en');
    expect(operationEntryErrorText(entry)).toBe(
      'Temporary problem (attempt 2 of 4); trying again in 1 s: connection reset',
    );
  });

  it('is translated rather than falling back to English', () => {
    setLocale('nl');
    expect(operationEntryErrorText(entry)).toContain('poging 2 van 4');
  });
});
