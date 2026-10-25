import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { catalogues } from './index';
import { ENGLISH_IDENTICAL_ALLOWLIST } from './translation-allowlist';
import type { Locale } from './types';
import { LOCALES } from './types';

const NON_ENGLISH = LOCALES.filter((locale): locale is Exclude<Locale, 'en'> => locale !== 'en');

/** Flattens a catalogue to `group.key[.pluralForm]` -> text. */
function flatten(value: unknown, prefix = '', out: Map<string, string> = new Map()) {
  if (typeof value === 'string') {
    out.set(prefix, value);
  } else if (value !== null && typeof value === 'object') {
    for (const [key, child] of Object.entries(value)) {
      flatten(child, prefix ? `${prefix}.${key}` : key, out);
    }
  }
  return out;
}

function isAllowed(path: string, locale: Locale): boolean {
  const entry = ENGLISH_IDENTICAL_ALLOWLIST[path];
  return entry === 'all' || (entry?.includes(locale) ?? false);
}

const english = flatten(catalogues.en);

describe('translation coverage', () => {
  it.each(NON_ENGLISH)('%s has no English text outside the allowlist', (locale) => {
    const localised = flatten(catalogues[locale]);
    const untranslated = [...english]
      .filter(([path, text]) => localised.get(path) === text && !isAllowed(path, locale))
      .map(([path]) => path);
    expect(untranslated, `${locale} still contains English text`).toEqual([]);
  });

  it.each(NON_ENGLISH)('%s allowlist has no stale entries', (locale) => {
    const localised = flatten(catalogues[locale]);
    const stale = Object.keys(ENGLISH_IDENTICAL_ALLOWLIST).filter(
      (path) => isAllowed(path, locale) && localised.get(path) !== english.get(path),
    );
    expect(stale, `${locale} allowlist entries that differ from English`).toEqual([]);
  });

  it('allowlist only names existing keys and locales', () => {
    for (const [path, entry] of Object.entries(ENGLISH_IDENTICAL_ALLOWLIST)) {
      expect(english.has(path), `unknown key ${path}`).toBe(true);
      if (entry !== 'all') {
        for (const locale of entry) expect(NON_ENGLISH).toContain(locale);
      }
    }
  });

  it('locale catalogues never fall back to English via spreads', () => {
    const dir = dirname(fileURLToPath(import.meta.url));
    for (const locale of NON_ENGLISH) {
      const source = readFileSync(join(dir, `${locale}.ts`), 'utf8');
      expect(source, `${locale}.ts spreads English`).not.toMatch(/\.\.\.en\b/u);
    }
  });
});
