import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { EnglishCatalogue } from './en';
import { actionTitle, catalogues, DEFAULT_LOCALE, getLocale, LOCALES, setLocale, t } from './index';
import { nl } from './nl';
import type { Locale, LocalisedCatalogue } from './types';

describe('i18n', () => {
  it('rejects unknown translation groups and keys at compile time', () => {
    const assertTypeSafety = () => {
      // @ts-expect-error unknown groups must not compile
      t('unknown', 'save');
      // @ts-expect-error unknown keys within a known group must not compile
      t('button', 'unknown');
    };
    expect(assertTypeSafety).toBeTypeOf('function');
  });

  beforeEach(() => {
    setLocale(DEFAULT_LOCALE);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('initial locale selection', () => {
    it('defaults to English', () => {
      expect(getLocale()).toBe('en');
    });

    it('serves English copy by default', () => {
      expect(t('settings', 'language')).toBe('Language');
    });
  });

  describe('runtime switching', () => {
    it('switches to Dutch without a page reload', () => {
      setLocale('nl');
      expect(getLocale()).toBe('nl');
      expect(t('settings', 'language')).toBe('Taal');
    });

    it('switches back to English', () => {
      setLocale('nl');
      setLocale('en');
      expect(t('settings', 'language')).toBe('Language');
    });

    it('updates the same translator object on every call', () => {
      const before = t('settings', 'language');
      setLocale('nl');
      const after = t('settings', 'language');
      expect(before).not.toBe(after);
      expect(before).toBe('Language');
      expect(after).toBe('Taal');
    });

    it('localises stable core action ids while preserving plugin-owned titles', () => {
      setLocale('nl');
      expect(actionTitle('core.copy', 'Copy')).toBe('Kopiëren');
      expect(actionTitle('core.compareDirectories', 'Compare Directories')).toBe(
        'Mappen vergelijken',
      );
      expect(actionTitle('core.sortByExtension', 'Sort by Extension')).toBe('Op extensie sorteren');
      expect(t('action', 'openExternally')).toBe('Extern openen');
      expect(t('action', 'externalEdit')).toBe('Extern bewerken');
      expect(actionTitle('plugin.example', 'Example action')).toBe('Example action');
    });
  });

  describe('interpolation', () => {
    it('substitutes named placeholders', () => {
      expect(t('shell', 'connectionName', { name: 'Home' })).toBe('Connection: Home');
    });

    it('substitutes in the second locale too', () => {
      setLocale('nl');
      expect(t('shell', 'connectionName', { name: 'Home' })).toBe('Verbinding: Home');
    });

    it('leaves unknown placeholders intact', () => {
      expect(t('shell', 'connectionName', {})).toBe('Connection: {name}');
    });
  });

  describe('pluralisation', () => {
    it('uses the exact count when present', () => {
      expect(t('shell', 'operationsCount', 1)).toBe('1 operation');
      expect(t('shell', 'operationsCount', 0)).toBe('No operations');
    });

    it('falls back to the n form', () => {
      expect(t('shell', 'operationsCount', 5)).toBe('5 operations');
    });
  });

  describe('missing-key behaviour', () => {
    it('degrades to the key name when both locales lack the key', () => {
      const translator = t as unknown as (group: string, key: string) => string;
      const result = translator('settings', 'doesNotExist');
      // In dev mode: conspicuous @@key@@, in production: bare key.
      expect(['doesNotExist', '@@settings.doesNotExist@@']).toContain(result);
    });

    it('falls back to the English value when the active locale lacks the key', () => {
      const full = JSON.parse(
        JSON.stringify(catalogues.nl),
      ) as LocalisedCatalogue<EnglishCatalogue>;
      delete (full.settings as Record<string, unknown>).language;
      (catalogues as Record<Locale, unknown>).nl = full;
      setLocale('nl');
      try {
        expect(t('settings', 'language')).toBe('Language');
      } finally {
        (catalogues as Record<Locale, unknown>).nl = nl;
        setLocale('en');
      }
    });

    it('is conspicuous in development', () => {
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
      const translator = t as unknown as (group: string, key: string) => string;
      const result = translator('settings', 'totallyMissing');
      expect(['totallyMissing', '@@settings.totallyMissing@@', '@@totallyMissing@@']).toContain(
        result,
      );
      expect(result.length).toBeGreaterThan(0);
      expect(warn).toHaveBeenCalled();
    });
  });

  describe('VDOM-safe array output', () => {
    it('returns an array when using .arr', () => {
      const result = t.arr('shell', 'connectionName', { name: 'X' });
      expect(Array.isArray(result)).toBe(true);
      expect(result.some((part: unknown) => part === 'X')).toBe(true);
    });
  });

  describe('catalogue parity', () => {
    function keysOf(value: unknown, prefix = ''): string[] {
      if (typeof value === 'string') return [prefix];
      if (value === null || typeof value !== 'object') return [];
      return Object.entries(value).flatMap(([k, v]) => keysOf(v, prefix ? `${prefix}.${k}` : k));
    }

    it('every catalogue has the same key set as English', () => {
      const englishKeys = new Set(keysOf(catalogues.en));
      for (const [locale, catalogue] of Object.entries(catalogues)) {
        if (locale === 'en') continue;
        const localeKeys = new Set(keysOf(catalogue));
        const missing = [...englishKeys].filter((k) => !localeKeys.has(k));
        const extra = [...localeKeys].filter((k) => !englishKeys.has(k));
        expect(missing, `${locale} is missing keys: ${missing.join(', ')}`).toEqual([]);
        expect(extra, `${locale} has extra keys: ${extra.join(', ')}`).toEqual([]);
      }
    });

    it('LOCALES covers every catalogue entry', () => {
      expect(LOCALES).toEqual(['en', 'nl']);
    });
  });

  describe('component redraws on locale change', () => {
    let root: HTMLElement;

    beforeEach(() => {
      root = document.createElement('div');
      document.body.appendChild(root);
    });

    afterEach(() => {
      m.mount(root, null);
      root.remove();
    });

    it('redear renders translated labels in Dutch after setLocale', () => {
      const Comp = () => ({
        view: () => m('div', { 'data-test': 'translated' }, t('button', 'save')),
      });
      m.mount(root, Comp);
      m.redraw.sync();
      expect(root.querySelector('[data-test="translated"]')?.textContent).toBe('Save');

      setLocale('nl');
      m.redraw.sync();
      expect(root.querySelector('[data-test="translated"]')?.textContent).toBe('Opslaan');

      setLocale('en');
      m.redraw.sync();
      expect(root.querySelector('[data-test="translated"]')?.textContent).toBe('Save');
    });

    it('redraws interpolated strings on locale change', () => {
      const Comp = () => ({
        view: () =>
          m('div', { 'data-test': 'interpolated' }, t('shell', 'connectionName', { name: 'Home' })),
      });
      m.mount(root, Comp);
      m.redraw.sync();
      expect(root.querySelector('[data-test="interpolated"]')?.textContent).toBe(
        'Connection: Home',
      );

      setLocale('nl');
      m.redraw.sync();
      expect(root.querySelector('[data-test="interpolated"]')?.textContent).toBe(
        'Verbinding: Home',
      );

      setLocale('en');
    });
  });
});
