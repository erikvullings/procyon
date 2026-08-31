import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, isAbsolute, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const THIS_DIR = dirname(fileURLToPath(import.meta.url));
const SRC_ROOT = join(THIS_DIR, '..');

function collectTsFiles(dir: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      files.push(...collectTsFiles(full));
    } else if (entry.endsWith('.ts')) {
      files.push(full);
    }
  }
  return files;
}

function importsSpecifierContaining(content: string, needle: string): boolean {
  return new RegExp(`from\\s+['"][^'"]*${needle}[^'"]*['"]`).test(content);
}

/** Separator-agnostic containment, so the check also holds on Windows. */
function isInside(root: string, file: string): boolean {
  const path = relative(root, file);
  return path.length > 0 && !path.startsWith('..') && !isAbsolute(path);
}

describe('module import boundaries', () => {
  const files = collectTsFiles(SRC_ROOT);

  it('imports the concrete FileManagerClient adapters from create-client.ts only (spec §12)', () => {
    const adapters = [
      'http-file-manager-client',
      'mock-file-manager-client',
      'tauri-file-manager-client',
    ];
    const createClientPath = join(THIS_DIR, 'client', 'create-client.ts');

    const offenders = files
      .filter((file) => !file.endsWith('.test.ts'))
      .filter((file) => file !== createClientPath)
      .filter((file) =>
        adapters.some((adapter) =>
          importsSpecifierContaining(readFileSync(file, 'utf-8'), adapter),
        ),
      );

    expect(offenders.map((file) => relative(SRC_ROOT, file))).toEqual([]);
  });

  it('imports api/generated only from src/api and src/models (spec §12)', () => {
    const allowedRoots = [THIS_DIR, join(SRC_ROOT, 'models')];

    const offenders = files
      .filter((file) => !allowedRoots.some((root) => isInside(root, file)))
      .filter((file) => importsSpecifierContaining(readFileSync(file, 'utf-8'), 'generated'));

    expect(offenders.map((file) => relative(SRC_ROOT, file))).toEqual([]);
  });
});
