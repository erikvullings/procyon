import { expect, it } from 'vitest';
import { admitsPath } from './sync-path-filter';

it('admits everything without patterns', () => {
  expect(admitsPath('a/b.txt', [], [])).toBe(true);
  expect(admitsPath('a/b.txt', ['  '], undefined)).toBe(true);
});

it('segment patterns apply at any depth and exclude descendants', () => {
  expect(admitsPath('pkg/node_modules/x/y.js', [], ['node_modules'])).toBe(false);
  expect(admitsPath('deep/scratch.tmp', [], ['*.tmp'])).toBe(false);
  expect(admitsPath('src/main.rs', [], ['node_modules', '*.tmp'])).toBe(true);
});

it('slash patterns anchor at the root and ** crosses directories', () => {
  expect(admitsPath('docs/a/b/readme.md', ['docs/**/*.md'], [])).toBe(true);
  expect(admitsPath('other/docs/readme.md', ['docs/**/*.md'], [])).toBe(false);
  expect(admitsPath('docs/a/readme.md', ['docs/*.md'], [])).toBe(false);
  expect(admitsPath('a/c', ['a?c'], [])).toBe(false);
});

it('exclusions win over inclusions', () => {
  expect(admitsPath('generated/out.rs', ['*.rs'], ['generated'])).toBe(false);
  expect(admitsPath('src/lib.rs', ['*.rs'], ['generated'])).toBe(true);
  expect(admitsPath('src/readme.md', ['*.rs'], ['generated'])).toBe(false);
});
