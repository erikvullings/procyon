import m from 'mithril';
import type { Messages } from 'translate.js';
import translate from 'translate.js';
import { setErrorMessageLocaliser } from '../api/error-localiser';
import { de } from './de';
import type { EnglishCatalogue } from './en';
import { en } from './en';
import { es } from './es';
import { fr } from './fr';
import { it } from './it';
import { nl } from './nl';
import { pl } from './pl';
import { pt } from './pt';
import type { Entry, Locale, LocalisedCatalogue, Params, Translator } from './types';

type Catalogue = LocalisedCatalogue<EnglishCatalogue>;
type AppTranslator = Translator<EnglishCatalogue>;

/** The initial and fallback locale (task 0098). */
export const DEFAULT_LOCALE: Locale = 'en';

/**
 * Every catalogue, keyed by locale. Adding a language is one import and one
 * entry here; the parity test in `i18n.test.ts` guards key coverage.
 */
export const catalogues: Record<Locale, Catalogue> = { en, nl, de, fr, es, it, pt, pl };

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
  import.meta.hot.accept(
    ['./en.ts', './nl.ts', './de.ts', './fr.ts', './es.ts', './it.ts', './pt.ts', './pl.ts'],
    ([
      englishModule,
      dutchModule,
      germanModule,
      frenchModule,
      spanishModule,
      italianModule,
      portugueseModule,
      polishModule,
    ]) => {
      if (englishModule !== undefined) catalogues.en = englishModule.en;
      if (dutchModule !== undefined) catalogues.nl = dutchModule.nl;
      if (germanModule !== undefined) catalogues.de = germanModule.de;
      if (frenchModule !== undefined) catalogues.fr = frenchModule.fr;
      if (spanishModule !== undefined) catalogues.es = spanishModule.es;
      if (italianModule !== undefined) catalogues.it = italianModule.it;
      if (portugueseModule !== undefined) catalogues.pt = portugueseModule.pt;
      if (polishModule !== undefined) catalogues.pl = polishModule.pl;
      currentTranslator = createTranslatorFor(currentLocale);
      m.redraw();
    },
  );
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

const IEC_UNITS = ['B', 'KiB', 'MiB', 'GiB', 'TiB'] as const;

/** Formats a byte count carried as a decimal string; non-numeric input is returned unchanged. */
export function byteCount(value: unknown): string {
  const bytes = typeof value === 'number' ? value : Number(value);
  if (!Number.isFinite(bytes)) return typeof value === 'string' ? value : '';
  let scaled = bytes;
  let unit = 0;
  while (scaled >= 1_024 && unit < IEC_UNITS.length - 1) {
    scaled /= 1_024;
    unit += 1;
  }
  const formatted = new Intl.NumberFormat(currentLocale, { maximumFractionDigits: 1 }).format(
    scaled,
  );
  return `${formatted} ${IEC_UNITS[unit]}`;
}

function recordOf(value: unknown): Record<string, unknown> {
  return typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : {};
}

/** Placeholders every coded message may use; they render empty when the backend omits them. */
const KNOWN_PARAMS = [
  'actionId',
  'fingerprint',
  'expectedFingerprint',
  'name',
  'detail',
  'field',
  'notes',
  'count',
  'items',
] as const;

function paramText(value: unknown): string {
  return typeof value === 'string' ? value : typeof value === 'number' ? String(value) : '';
}

/**
 * Interpolation values for coded messages. Every backend parameter is passed through; the ones
 * whose key ends in `Bytes` (or the legacy `required`/`available`) are formatted as sizes, and
 * well-known placeholders that were not sent render empty, never as `{x}`.
 */
function messageParams(params: Readonly<Record<string, unknown>> | null | undefined): Params {
  const source = params ?? {};
  const result: Params = {};
  for (const key of KNOWN_PARAMS) result[key] = paramText(source[key]);
  for (const [key, value] of Object.entries(source)) {
    result[key] = key.endsWith('Bytes') ? byteCount(value) : paramText(value);
  }
  result.required = byteCount(source.required);
  result.available = byteCount(source.available);
  return result;
}

/**
 * Fills a catalogue template. Placeholders without a parameter render empty, and parameter
 * values are never re-scanned, so a file named `{draft}` keeps its braces.
 */
function fillTemplate(template: string, params: Params): string {
  return template.replaceAll(/\{(\w+)\}/gu, (_match, key: string) => String(params[key] ?? ''));
}

/** Renders `group.key` from the current catalogue with `params`. */
function render(group: CodedGroup | 'previewResource', key: string, params: Params): string {
  return fillTemplate((t as (group: string, key: string) => string)(group, key), params);
}

type CodedGroup =
  | 'apiError'
  | 'semanticError'
  | 'operationEntryError'
  | 'operationFailure'
  | 'operationUndoReason'
  | 'rejectionReason'
  | 'geoMessage'
  | 'viewerWarning'
  | 'notification';

/** Renders `group.code` with `params`; undefined when the English catalogue lacks the code. */
function codedText(group: CodedGroup, code: string, params: Params): string | undefined {
  if (!Object.hasOwn(en[group], code)) return undefined;
  return render(group, code, params);
}

/** First catalogue (in order) that knows `code`. */
function firstCodedText(
  groups: readonly CodedGroup[],
  code: string,
  params: Params,
): string | undefined {
  for (const group of groups) {
    const text = codedText(group, code, params);
    if (text !== undefined) return text;
  }
  return undefined;
}

/** Budgets counted in items or nesting levels, not bytes or pixels. */
const COUNT_BUDGETS = new Set(['zipEntries', 'imageCount', 'mediaCount', 'slideCount', 'xmlDepth']);

/**
 * Rejection reasons with parameters that need localising before interpolation: the exceeded
 * preview budget names its resource and formats the limit per unit.
 */
function rejectionParams(reason: string, params: Params): Params {
  if (reason !== 'previewBudget') return params;
  const resource = String(params.resource ?? '');
  const limit = Number(params.limit);
  const known = Object.hasOwn(en.previewResource, resource);
  return {
    ...params,
    resource: known ? render('previewResource', resource, {}) : resource,
    limit:
      COUNT_BUDGETS.has(resource) || resource.endsWith('Pixels')
        ? new Intl.NumberFormat(currentLocale).format(limit)
        : byteCount(limit),
  };
}

/** Text for the `reason` of an `invalidRequest`/`platformOperationFailed` rejection. */
function rejectionText(reason: string, record: Record<string, unknown>): string | undefined {
  const params = rejectionParams(reason, messageParams(record));
  return firstCodedText(
    ['rejectionReason', 'operationEntryError', 'operationUndoReason'],
    reason,
    params,
  );
}

/**
 * Localised text for a stable backend error code, or undefined for codes without a dedicated
 * entry. Looks through the `ApplicationErrorCode` catalogue first, then the semantic
 * component/library catalogue. `invalidRequest`/`platformOperationFailed` first try the
 * rejection `reason` carried in `details`, then wrap the backend text. `fallback` is the
 * backend's English text, offered to entries as `{detail}`.
 */
export function apiErrorMessage(
  code: string,
  details?: unknown,
  fallback?: string,
): string | undefined {
  const record = recordOf(details);
  if (code === 'invalidRequest' || code === 'platformOperationFailed') {
    const reason = typeof record.reason === 'string' ? record.reason : undefined;
    const specific = reason === undefined ? undefined : rejectionText(reason, record);
    if (specific !== undefined) return specific;
  }
  if (Object.hasOwn(en.apiError, code)) {
    return render('apiError', code, {
      ...messageParams(record),
      detail: paramText(record.detail) || (fallback ?? ''),
    });
  }
  if (Object.hasOwn(en.semanticError, code)) {
    return render('semanticError', code, {
      ...messageParams(record),
      required: byteCount(record.requiredBytes),
      available: byteCount(record.availableBytes),
      detail: paramText(record.detail) || (fallback ?? ''),
    });
  }
  // Plain reason codes (e.g. a `paneNotFound` failure raised without `invalidRequest`).
  if (Object.hasOwn(en.rejectionReason, code)) return rejectionText(code, record);
  return undefined;
}

setErrorMessageLocaliser(apiErrorMessage);

/** A backend message with an optional stable code and structured parameters. */
export interface CodedMessage {
  readonly code?: string | null | undefined;
  readonly params?: Readonly<Record<string, string>> | null | undefined;
  readonly message: string;
}

/** Localised text of a geographic message (note, warning or failure) by its code. */
export function geoMessageText(message: CodedMessage): string {
  const { code } = message;
  if (code == null || !Object.hasOwn(en.geoMessage, code)) return message.message;
  const params = messageParams(message.params);
  let text = render('geoMessage', code, params);
  if (code === 'crsConverted' && Number(message.params?.failed) > 0) {
    text += render('geoMessage', 'crsConvertedFailures', params);
  }
  return text;
}

/** Localises the JSON-encoded geo notes an operation entry error carries in `notes`. */
function geoNotesText(notes: string): string {
  try {
    const parsed: unknown = JSON.parse(notes);
    if (!Array.isArray(parsed)) return notes;
    return parsed
      .map((note) => {
        const record = recordOf(note);
        return geoMessageText({
          code: typeof record.code === 'string' ? record.code : null,
          message: typeof record.message === 'string' ? record.message : '',
          params: recordOf(record.params) as Record<string, string>,
        });
      })
      .join('; ');
  } catch {
    return notes;
  }
}

const ENTRY_ERROR_GROUPS = ['operationEntryError', 'operationFailure', 'geoMessage'] as const;

/** Localised per-entry operation failure; unknown or absent codes keep the English message. */
export function operationEntryErrorText(error: CodedMessage): string {
  const { code } = error;
  if (code == null) return error.message;
  const params = messageParams(error.params);
  if (code === 'geoWrittenWithLoss') params.notes = geoNotesText(String(error.params?.notes ?? ''));
  if (code === 'operationFailed' && params.detail === '') {
    return t('operationFailure', 'operationFailedPlain');
  }
  return firstCodedText(ENTRY_ERROR_GROUPS, code, params) ?? error.message;
}

/** The terminal failure of an operation (`operation.failed`). */
export interface OperationFailureInfo {
  readonly code: string;
  readonly message: string;
  readonly details?: unknown;
}

/** Localised operation-level failure; unknown codes keep the backend's English message. */
export function operationFailureText(failure: OperationFailureInfo): string {
  const record = recordOf(failure.details);
  const params = messageParams(record);
  if (failure.code === 'operationFailed' && params.detail === '') {
    return t('operationFailure', 'operationFailedPlain');
  }
  return (
    firstCodedText(
      ['operationFailure', 'operationEntryError', 'geoMessage'],
      failure.code,
      params,
    ) ??
    apiErrorMessage(failure.code, failure.details, failure.message) ??
    failure.message
  );
}

/** Localised text of a rejection `reason` with its parameters; unknown reasons keep `fallback`. */
export function reasonText(
  reason: string | null | undefined,
  params: Readonly<Record<string, string>> | null | undefined,
  fallback: string,
): string {
  if (reason == null) return fallback;
  return rejectionText(reason, recordOf(params)) ?? fallback;
}

/** Localised operation outcome summary; unknown or absent codes keep the English message. */
export function operationResultText(result: CodedMessage): string {
  const { code } = result;
  if (code == null || !Object.hasOwn(en.operationResult, code)) return result.message;
  return t(
    'operationResult',
    code as keyof EnglishCatalogue['operationResult'],
    messageParams(result.params),
  );
}

/**
 * English reasons stored in operation history written before undo reasons had codes, mapped to
 * the code that now describes them.
 */
const LEGACY_UNDO_REASONS: Readonly<Record<string, keyof EnglishCatalogue['operationUndoReason']>> =
  {
    'This operation has already been undone.': 'alreadyUndone',
    'Undo is already in progress for this operation.': 'undoInProgress',
    'This operation cannot currently be undone safely.': 'undoUnsafe',
    'This operation cannot be undone safely.': 'undoUnsafe',
    'Operation history entry was not found.': 'historyEntryNotFound',
    'This operation has no effects to undo.': 'noEffects',
    'Undo is available only after the operation completes.': 'notYetAvailable',
    'This operation does not retain enough evidence to be undone safely.': 'noEvidence',
    'The previous undo was interrupted, so its remaining effects cannot be reversed safely.':
      'undoInterrupted',
    'The previous undo completed only partially, so it cannot be retried safely.': 'undoPartial',
    'An existing file was replaced, which cannot be brought back.': 'replacedExisting',
    'Existing files were replaced, which cannot be brought back.': 'replacedExisting',
    'The link replaced an existing entry, which cannot be restored.': 'replacedExisting',
    'Undo is unavailable because the copy replaced or merged existing entries.': 'replacedExisting',
    'Undo needs the backup copy; run the reduction with "keep original" enabled.': 'backupRequired',
    'Undo needs the backup copy; run the edit with "keep original" enabled.': 'backupRequired',
    'No file was changed.': 'nothingToUndo',
    'No entry was changed.': 'nothingToUndo',
    'No file was created.': 'nothingToUndo',
    'No link was created, so there is nothing to undo.': 'nothingToUndo',
    'The copy created no entries to undo.': 'nothingToUndo',
    'The operation created no entries to undo.': 'nothingToUndo',
    'The move destination was not completed, so there is nothing safe to undo.': 'nothingToUndo',
    'The renamed entry was not completed, so there is nothing safe to undo.': 'nothingToUndo',
    'Undo operations cannot themselves be undone.': 'undoOfUndo',
    'The platform did not provide a restorable trash location.': 'trashNotRestorable',
    'The move did not retain its original source fingerprint.': 'moveEvidenceMissing',
    'A collision changed the moved entry name, so cross-provider undo is unavailable.':
      'collisionRenamed',
  };

/**
 * Localised reason an undo is unavailable. History written before codes existed carries only the
 * English reason; known reasons are mapped to their code, anything else keeps the English text.
 */
export function operationUndoReasonText(reason: string, code: string | null | undefined): string {
  const resolved = code ?? LEGACY_UNDO_REASONS[reason];
  if (resolved == null || !Object.hasOwn(en.operationUndoReason, resolved)) return reason;
  return t('operationUndoReason', resolved as keyof EnglishCatalogue['operationUndoReason']);
}

/** Localised text of a viewer caveat (`warningCode`); unknown codes keep the English text. */
export function viewerWarningText(
  warning: string | null | undefined,
  code: string | null | undefined,
  detail?: string | null,
): string | undefined {
  if (warning == null) return undefined;
  if (code == null) return warning;
  return codedText('viewerWarning', code, messageParams({ detail })) ?? warning;
}

/** Localised backend notification; unknown or absent codes keep the English message. */
export function notificationText(notification: CodedMessage): string {
  const { code } = notification;
  if (code == null) return notification.message;
  return (
    codedText('notification', code, messageParams(notification.params)) ?? notification.message
  );
}

/** Localised reason a folder or estimate is unavailable. */
export function semanticUnavailableText(
  code: string | null | undefined,
  fallback: string | null | undefined,
): string | undefined {
  if (code == null || !Object.hasOwn(en.semanticUnavailable, code)) return fallback ?? undefined;
  return t('semanticUnavailable', code as keyof EnglishCatalogue['semanticUnavailable']);
}

/**
 * Text for any caught value. Errors (including `ApiError`/`TauriCommandError`, whose `message`
 * is already localised) keep their message; plain `{ code, message }` rejections are localised
 * by code; anything else gets a generic localised line instead of `[object Object]`.
 */
export function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string' && error.length > 0) return error;
  const record = recordOf(error);
  const message =
    typeof record.message === 'string' && record.message.length > 0 ? record.message : undefined;
  const code = typeof record.code === 'string' ? record.code : undefined;
  if (code !== undefined) {
    const localised = apiErrorMessage(code, record.details, message);
    if (localised !== undefined) return localised;
  }
  return message ?? t('apiError', 'unknownFailure');
}

/** Labels core command categories; plugin categories are humanised from their camelCase id. */
export function actionCategoryLabel(category: string): string {
  if (Object.hasOwn(en.commandCategory, category)) {
    return t('commandCategory', category as keyof EnglishCatalogue['commandCategory']);
  }
  const words = category
    .replaceAll(/([a-z0-9])([A-Z])/gu, '$1 $2')
    .replaceAll(/[._-]+/gu, ' ')
    .trim()
    .toLowerCase();
  return words.length === 0 ? category : words[0]?.toUpperCase() + words.slice(1);
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
