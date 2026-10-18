import { describe, expect, it } from 'vitest';
import { normalizeTypedPath } from './path-input';

describe('normalizeTypedPath', () => {
  it.each([
    ['  /home/erik  ', '/home/erik'],
    ['`~/.copilot/session-state/files/plan.json`', '~/.copilot/session-state/files/plan.json'],
    ['```/tmp/report.md```', '/tmp/report.md'],
    ['"/Users/erik/My Files"', '/Users/erik/My Files'],
    ["'/Users/erik/My Files'", '/Users/erik/My Files'],
    ['"`~/notes`"', '~/notes'],
    ['` /tmp/padded `', '/tmp/padded'],
    ['`~/notes.md`.', '~/notes.md'],
    ['"C:\\Users\\erik"', 'C:\\Users\\erik'],
    ['\\~', '~'],
    ['\\~/Documents', '~/Documents'],
    ['`\\~/Documents`', '~/Documents'],
    ['~', '~'],
  ])('normalises %j to %j', (input, expected) => {
    expect(normalizeTypedPath(input)).toBe(expected);
  });

  it.each([
    ["/tmp/it's here", "/tmp/it's here"],
    ['`/tmp/unbalanced', '`/tmp/unbalanced'],
    ['"/tmp/mixed\'', '"/tmp/mixed\''],
    ['\\\\server\\share', '\\\\server\\share'],
    ['/tmp/trailing.', '/tmp/trailing.'],
  ])('leaves %j without a matching wrapper untouched', (input, expected) => {
    expect(normalizeTypedPath(input)).toBe(expected);
  });
});
