import m from 'mithril';
import type { Messages } from 'translate.js';
import translate from 'translate.js';
import type { EnglishCatalogue } from './en';
import { en } from './en';
import { nl } from './nl';
import type { Entry, Locale, LocalisedCatalogue, Params, Translator } from './types';

type Catalogue = LocalisedCatalogue<EnglishCatalogue>;
type AppTranslator = Translator<EnglishCatalogue>;

/** The initial and fallback locale (task 0098). */
export const DEFAULT_LOCALE: Locale = 'en';

/**
 * Every catalogue, keyed by locale. Adding a language is one import and one
 * entry here; the parity test in `i18n.test.ts` guards key coverage.
 */
export const catalogues: Record<Locale, Catalogue> = { en, nl };

function isDev(): boolean {
  if (typeof import.meta !== 'undefined' && import.meta.env) {
    return import.meta.env.MODE !== 'production';
  }
  if (typeof process !== 'undefined' && process.env) {
    return process.env.NODE_ENV !== 'production';
  }
  return false;
}

function resolvePlural(entry: Entry, count: number): string {
  if (typeof entry === 'string') return entry;
  // entry is a PluralEntry — exact match or fallback
  const exact = entry[count];
  if (typeof exact === 'string') return exact;
  return entry.n.replace('{n}', String(count));
}

/**
 * Builds a translator for one catalogue.
 *
 * Missing keys degrade to the English value, or the key name when English also
 * lacks the key. In development the result is conspicuous (`@@key@@`) and a
 * warning is emitted; production never throws.
 */
export function createTranslatorFor(locale: Locale): AppTranslator {
  const dev = isDev();
  const lib = translate(catalogues[locale] as unknown as Messages, {
    debug: dev,
    useKeyForMissingTranslation: true,
  });
  const catalogue = catalogues[locale];
  const englishCatalogue = en;

  function lookup(group: string, subKey: string): Entry | undefined {
    const groupData = catalogue[group as keyof typeof catalogue];
    if (typeof groupData !== 'object' || groupData === null) return undefined;
    return (groupData as Record<string, Entry>)[subKey];
  }

  function englishLookup(group: string, subKey: string): Entry | undefined {
    const groupData = englishCatalogue[group as keyof typeof englishCatalogue];
    if (typeof groupData !== 'object' || groupData === null) return undefined;
    return (groupData as Record<string, Entry>)[subKey];
  }

  function translateValue(entry: Entry, params?: Params): string {
    if (typeof entry === 'string') {
      let result = entry;
      if (params) {
        for (const [key, value] of Object.entries(params)) {
          result = result.replace(new RegExp(`\\{${key}\\}`, 'g'), String(value));
        }
      }
      return result;
    }
    // Plural entry — entries used as plural always have an implicit {n} from count
    return entry.n;
  }

  function resolve(
    group: keyof EnglishCatalogue,
    second: string | Params,
    third?: Params | number,
  ): string {
    // t('group', params) — params on group level (rare)
    if (typeof second === 'object' && second !== null) {
      const groupData = catalogue[group];
      const starEntry = (groupData as Record<string, Entry>)['*'];
      if (starEntry) return translateValue(starEntry, second);
      return group;
    }
    const subKey = second;

    // t('group', 'subKey', count) — pluralisation
    if (typeof third === 'number') {
      const entry = lookup(group as string, subKey);
      if (entry !== undefined) {
        return resolvePlural(entry, third);
      }
      // Fall back to English.
      const enEntry = englishLookup(group as string, subKey);
      if (enEntry !== undefined) return resolvePlural(enEntry, third);
      return subKey;
    }

    // t('group', 'subKey', { params }) — interpolation
    if (typeof third === 'object' && third !== null) {
      const entry = lookup(group as string, subKey);
      if (entry !== undefined) {
        if (typeof entry === 'string') return translateValue(entry, third);
        return translateValue(entry);
      }
      // Fall back to English.
      const enEntry = englishLookup(group as string, subKey);
      if (enEntry !== undefined) {
        if (typeof enEntry === 'string') return translateValue(enEntry, third);
        return translateValue(enEntry);
      }
      // Dev mode: conspicuous output.
      if (dev) {
        console.warn(`Missing translation for "${group}.${subKey}"`);
        return `@@${group}.${subKey}@@`;
      }
      return subKey;
    }

    // t('group', 'subKey') — simple lookup
    const entry = lookup(group as string, subKey);
    if (entry !== undefined) {
      return typeof entry === 'string' ? entry : entry.n;
    }
    // Fall back to English.
    const enEntry = englishLookup(group as string, subKey);
    if (enEntry !== undefined) {
      return typeof enEntry === 'string' ? enEntry : enEntry.n;
    }
    // Dev mode: conspicuous output.
    if (dev) {
      console.warn(`Missing translation for "${group}.${subKey}"`);
      return `@@${group}.${subKey}@@`;
    }
    return subKey;
  }

  function arrResolve(group: string, second: string, third?: Params | number): unknown[] {
    // For VDOM-safe output, delegate to translate.js for string splitting.
    const args =
      typeof third === 'number'
        ? ([group, second] as [string, string])
        : third
          ? ([group, second, third] as [string, string, Params])
          : ([group, second] as [string, string]);
    return (lib.arr as (...a: unknown[]) => unknown[])(...args);
  }

  const wrapped = resolve as AppTranslator & { arr: AppTranslator['arr'] };
  wrapped.arr = arrResolve as AppTranslator['arr'];
  return wrapped;
}

let currentLocale: Locale = DEFAULT_LOCALE;
let currentTranslator: AppTranslator = createTranslatorFor(currentLocale);

if (import.meta.hot !== undefined) {
  import.meta.hot.accept(['./en.ts', './nl.ts'], ([englishModule, dutchModule]) => {
    if (englishModule !== undefined) catalogues.en = englishModule.en;
    if (dutchModule !== undefined) catalogues.nl = dutchModule.nl;
    currentTranslator = createTranslatorFor(currentLocale);
    m.redraw();
  });
}

/**
 * The stable, process-wide translator. Components import this single object
 * and always get the current locale's catalogue, so `setLocale` updates every
 * surface on the next redraw without re-importing.
 */
export const t: AppTranslator = Object.assign(
  (...args: Parameters<AppTranslator>) => currentTranslator(...args),
  {
    arr: (...args: Parameters<AppTranslator['arr']>) => currentTranslator.arr(...args),
  },
) as unknown as AppTranslator;

/** The current locale. */
export function getLocale(): Locale {
  return currentLocale;
}

/** Maps stable core action ids to frontend-owned copy; plugin titles remain plugin-owned. */
export function actionTitle(actionId: string, fallback: string): string {
  const key = actionId.startsWith('core.') ? actionId.slice('core.'.length) : undefined;
  if (key === undefined || !Object.hasOwn(en.action, key)) return fallback;
  return t('action', key as keyof EnglishCatalogue['action']);
}

/**
 * Switches the process-wide translator to another catalogue at runtime, with
 * no page reload. Callers persist the choice through the settings service and
 * trigger a redraw so all translated surfaces update.
 */
export function setLocale(locale: Locale): Locale {
  currentLocale = locale;
  currentTranslator = createTranslatorFor(locale);
  m.redraw();
  return locale;
}

export type { EnglishCatalogue } from './en';
export type { Entry, Locale, LocalisedCatalogue, Params, PluralEntry, Translator } from './types';
export { LOCALES } from './types';
