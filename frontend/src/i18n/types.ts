/** A supported locale identifier (task 0098). */
export type Locale = 'en' | 'nl';

/** All supported locales, in the order they appear in the settings selector. */
export const LOCALES: readonly Locale[] = ['en', 'nl'];

/** The default and fallback locale. */
export const DEFAULT_LOCALE: Locale = 'en';

/** A single translation value: plain text, or a plural map with an `n` fallback. */
export type Entry = string | PluralEntry;

/** A count-dependent value: exact counts (0, 1, 2, …) plus an `n` fallback. */
export interface PluralEntry {
  readonly n: string;
  readonly [count: number]: string | undefined;
}

/** Broad constraint used while defining the canonical English catalogue. */
export type CatalogueShape = Readonly<Record<string, Readonly<Record<string, Entry>>>>;

/** Widens translated text while preserving the exact keys and plural forms from English. */
export type LocalisedCatalogue<Canonical extends CatalogueShape> = {
  readonly [Group in keyof Canonical]: {
    readonly [Key in keyof Canonical[Group]]: Canonical[Group][Key] extends string
      ? string
      : Canonical[Group][Key] extends Readonly<Record<PropertyKey, unknown>>
        ? { readonly [Variant in keyof Canonical[Group][Key]]: string }
        : never;
  };
};

/** Placeholder parameters for a single translation call. */
export type Params = Record<string, string | number>;

/**
 * A `translate.js` translator bound to one catalogue.
 *
 * Overloads ordered so the most specific (number/count) is checked first.
 */
export interface Translator<Canonical extends CatalogueShape> {
  /** Lookup: `t('group', 'subKey', { params })` */
  <Group extends keyof Canonical, Key extends keyof Canonical[Group] & string>(
    key: Group,
    subKey: Key,
    params?: Params,
  ): string;
  /** Pluralisation: `t('group', 'pluralKey', count)` */
  <Group extends keyof Canonical, Key extends keyof Canonical[Group] & string>(
    key: Group,
    subKey: Key,
    count: number,
  ): string;
  /** VDOM-safe array output for interpolating vnodes. */
  arr<Group extends keyof Canonical, Key extends keyof Canonical[Group] & string>(
    key: Group,
    subKey: Key,
    params?: Params,
  ): unknown[];
  arr<Group extends keyof Canonical, Key extends keyof Canonical[Group] & string>(
    key: Group,
    subKey: Key,
    count: number,
  ): unknown[];
}
