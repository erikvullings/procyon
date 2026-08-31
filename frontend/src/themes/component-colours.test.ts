import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const HEX_COLOUR = /#[\da-f]{3,8}\b/i;

describe('component theme boundaries', () => {
  it('does not hard-code hex colours outside theme and generated sources', () => {
    const sourceDirectory = join(process.cwd(), 'src');
    const sourceFiles = readdirSync(sourceDirectory, { recursive: true })
      .map(String)
      // `readdirSync` yields the platform separator, so compare in POSIX form.
      .map((path) => path.replaceAll('\\', '/'))
      .filter((path) => /\.(?:css|ts)$/.test(path))
      .filter((path) => !path.startsWith('themes/'))
      .filter((path) => !path.startsWith('api/generated/'))
      .filter((path) => !path.endsWith('.test.ts'));

    const offenders = sourceFiles.filter((path) =>
      HEX_COLOUR.test(readFileSync(join(sourceDirectory, path), 'utf8')),
    );

    expect(offenders).toEqual([]);
  });
});
