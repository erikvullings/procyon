import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';
import { unexpectedResponseError } from '../api/fetch-mutator';
import { en } from './en';
import {
  apiErrorMessage,
  catalogues,
  DEFAULT_LOCALE,
  errorText,
  geoMessageText,
  notificationText,
  operationEntryErrorText,
  operationFailureText,
  operationResultText,
  operationUndoReasonText,
  setLocale,
  viewerWarningText,
} from './index';
import { LOCALES } from './types';

const root = join(dirname(fileURLToPath(import.meta.url)), '../../..');
const rust = (path: string): string => readFileSync(join(root, path), 'utf8');

/** `pub const NAME: &str = "code";` declarations inside one `pub mod <name> { ... }` block. */
function codesIn(source: string, moduleName: string): string[] {
  const body =
    new RegExp(`pub mod ${moduleName} \\{([\\s\\S]*?)\\n\\}`, 'u').exec(source)?.[1] ?? '';
  return [...body.matchAll(/pub const [A-Z_]+: &str = "(\w+)";/gu)].map((match) => match[1] ?? '');
}

/** Every `pub const NAME: &str = "code";` (any visibility) in a `pub mod <name>`. */
function constsIn(source: string, moduleName: string): string[] {
  const body =
    new RegExp(`pub(?:\\(crate\\))? mod ${moduleName} \\{([\\s\\S]*?)\\n\\}`, 'u').exec(
      source,
    )?.[1] ?? '';
  return [...body.matchAll(/pub(?:\(crate\))? const [A-Z_]+: &str = "(\w+)";/gu)].map(
    (match) => match[1] ?? '',
  );
}

const rejectionSource = rust('crates/fm-application/src/rejection.rs');
const geoSource = rust('crates/fm-geo/src/message.rs');
const operationCodes = rust('crates/fm-operations/src/codes.rs');
const vfsCodes = [
  ...(
    /pub const fn code\(&self\)[\s\S]*?\n {4}\}/u.exec(rust('crates/fm-vfs/src/error.rs'))?.[0] ??
    ''
  ).matchAll(/=> "(\w+)",/gu),
].map((match) => match[1] ?? '');

/** Values of an Orval-generated `as const` enum object, read from source (the boundary test keeps
 * `api/generated` out of runtime imports). */
function generatedEnumValues(file: string): string[] {
  const source = readFileSync(join(root, 'frontend/src/api/generated/models', file), 'utf8');
  return [...source.matchAll(/^ {2}\w+: '(\w+)',?$/gmu)].map((match) => match[1] ?? '');
}

function placeholders(text: string): string[] {
  return [...text.matchAll(/\{(\w+)\}/gu)].map((match) => match[1] ?? '').sort();
}

describe('backend message codes have translations', () => {
  afterEach(() => {
    setLocale(DEFAULT_LOCALE);
  });

  it('finds the Rust code declarations (guards the scanners below)', () => {
    expect(codesIn(operationCodes, 'entry_error').length).toBeGreaterThan(10);
    expect(codesIn(operationCodes, 'result')).toHaveLength(5);
    expect(codesIn(operationCodes, 'undo').length).toBeGreaterThan(10);
    expect(codesIn(operationCodes, 'failure').length).toBeGreaterThan(10);
    expect(constsIn(rejectionSource, 'reason').length).toBeGreaterThan(100);
    expect(constsIn(geoSource, 'codes').length).toBeGreaterThan(70);
    expect(vfsCodes.length).toBeGreaterThan(15);
  });

  it('localises every per-entry operation error code', () => {
    const codes = [...codesIn(operationCodes, 'entry_error'), ...vfsCodes];
    const missing = codes.filter((code) => !Object.hasOwn(en.operationEntryError, code));
    expect(missing).toEqual([]);
  });

  it('localises every operation result summary code', () => {
    const missing = codesIn(operationCodes, 'result').filter(
      (code) => !Object.hasOwn(en.operationResult, code),
    );
    expect(missing).toEqual([]);
  });

  it('localises every undo-unavailable reason code', () => {
    const missing = codesIn(operationCodes, 'undo').filter(
      (code) => !Object.hasOwn(en.operationUndoReason, code),
    );
    expect(missing).toEqual([]);
  });

  it('localises every semantic component and library error code', () => {
    const backendText = new Set(['invalidRequest']);
    const codes = [
      ...generatedEnumValues('semanticComponentErrorCodeDto.ts'),
      ...generatedEnumValues('semanticLibraryErrorCodeDto.ts'),
    ].filter((code) => !backendText.has(code));
    expect(codes.length).toBeGreaterThan(30);
    const missing = codes.filter(
      (code) => !Object.hasOwn(en.semanticError, code) && !Object.hasOwn(en.apiError, code),
    );
    expect(missing).toEqual([]);
  });

  it('localises every operation failure code', () => {
    const missing = codesIn(operationCodes, 'failure').filter(
      (code) => !Object.hasOwn(en.operationFailure, code),
    );
    expect(missing).toEqual([]);
  });

  it('localises every rejection reason', () => {
    const known = (code: string): boolean =>
      Object.hasOwn(en.rejectionReason, code) ||
      Object.hasOwn(en.operationEntryError, code) ||
      Object.hasOwn(en.operationUndoReason, code);
    expect(constsIn(rejectionSource, 'reason').filter((code) => !known(code))).toEqual([]);
    expect(
      constsIn(rejectionSource, 'preview_resource').filter(
        (code) => !Object.hasOwn(en.previewResource, code),
      ),
    ).toEqual([]);
  });

  it('localises every geographic message code', () => {
    expect(
      constsIn(geoSource, 'codes').filter((code) => !Object.hasOwn(en.geoMessage, code)),
    ).toEqual([]);
  });

  it('localises notification, viewer warning, unavailable-reason and OCR codes', () => {
    expect(
      constsIn(rust('crates/fm-events/src/lib.rs'), 'notification_codes').filter(
        (code) => !Object.hasOwn(en.notification, code),
      ),
    ).toEqual([]);
    expect(
      constsIn(rust('crates/fm-application/src/viewer_warning.rs'), 'codes').filter(
        (code) => !Object.hasOwn(en.viewerWarning, code),
      ),
    ).toEqual([]);
    expect(
      generatedEnumValues('semanticUnavailableReasonDto.ts').filter(
        (code) => !Object.hasOwn(en.semanticUnavailable, code),
      ),
    ).toEqual([]);
    const ocr = rust('crates/fm-transport-dto/src/semantic_ocr.rs');
    const body = /pub enum SemanticOcrErrorCodeDto \{([\s\S]*?)\n\}/u.exec(ocr)?.[1] ?? '';
    const ocrCodes = [...body.matchAll(/^ {4}([A-Z][A-Za-z]+),/gmu)].map((match) =>
      (match[1] as string).replace(/^./u, (c) => c.toLowerCase()),
    );
    expect(ocrCodes.length).toBeGreaterThan(5);
    expect(
      ocrCodes.filter(
        (code) =>
          code !== 'invalidRequest' &&
          !Object.hasOwn(en.semanticError, code) &&
          !Object.hasOwn(en.apiError, code),
      ),
    ).toEqual([]);
  });

  it('localises the checksum entry error codes', () => {
    for (const code of ['readFailed', 'noAlgorithmRequested', 'cancelled']) {
      expect(Object.hasOwn(en.operationEntryError, code), code).toBe(true);
    }
  });

  it.each(LOCALES.filter((locale) => locale !== 'en'))(
    'translates every new group in %s with the English placeholders',
    (locale) => {
      const groups = [
        'apiError',
        'semanticError',
        'operationEntryError',
        'operationResult',
        'operationUndoReason',
        'rejectionReason',
        'operationFailure',
        'geoMessage',
        'previewResource',
        'viewerWarning',
        'notification',
        'semanticUnavailable',
        'mapPreset',
      ] as const;
      for (const group of groups) {
        const english: Record<string, string> = en[group];
        const translated: Record<string, string> = catalogues[locale][group];
        for (const [key, source] of Object.entries(english)) {
          const text = translated[key];
          expect(text, `${locale}.${group}.${key}`).toBeTruthy();
          expect(placeholders(text ?? ''), `${locale}.${group}.${key}`).toEqual(
            placeholders(source),
          );
        }
      }
      // Short stock phrases may legitimately match English, but most copy must be translated.
      for (const group of ['rejectionReason', 'operationFailure', 'geoMessage'] as const) {
        const identicalCount = Object.entries(en[group]).filter(
          ([key, text]) => (catalogues[locale][group] as Record<string, string>)[key] === text,
        ).length;
        expect(identicalCount, `${locale}.${group} copies English`).toBeLessThan(4);
      }
      const identical = Object.entries(en.operationEntryError).filter(
        ([key, text]) =>
          (catalogues[locale].operationEntryError as Record<string, string>)[key] === text,
      );
      expect(identical.length).toBeLessThan(5);
    },
  );
});

describe('coded backend messages', () => {
  afterEach(() => {
    setLocale(DEFAULT_LOCALE);
  });

  it('interpolates entry-error params and formats byte sizes per locale', () => {
    setLocale('de');
    const text = operationEntryErrorText({
      message: 'English fallback',
      code: 'diskSpaceCheckSkipped',
      params: { required: String(5 * 1_024 * 1_024), available: '2048' },
    });
    expect(text).toBe(
      catalogues.de.operationEntryError.diskSpaceCheckSkipped
        .replace('{required}', '5 MiB')
        .replace('{available}', '2 KiB'),
    );
    setLocale('nl');
    expect(
      operationEntryErrorText({
        message: 'fallback',
        code: 'attributeEntryMissing',
        params: { name: 'a.txt' },
      }),
    ).toBe('a.txt bestaat niet meer.');
  });

  it('keeps the English message for absent or unknown codes', () => {
    setLocale('fr');
    expect(operationEntryErrorText({ message: 'raw' })).toBe('raw');
    expect(operationEntryErrorText({ message: 'raw', code: 'someFutureCode' })).toBe('raw');
    expect(operationResultText({ message: 'Completed 1 items.', code: 'nope' })).toBe(
      'Completed 1 items.',
    );
    expect(operationUndoReasonText('English reason', undefined)).toBe('English reason');
  });

  it('localises result summaries and undo reasons', () => {
    setLocale('es');
    expect(
      operationResultText({
        message: 'Completed 4 items.',
        code: 'completed',
        params: { items: '4' },
      }),
    ).toBe('4 elementos completados.');
    expect(operationUndoReasonText('x', 'alreadyUndone')).toBe('Esta operación ya se deshizo.');
  });

  it('localises semantic errors, passing the backend text as {detail} and formatting bytes', () => {
    setLocale('it');
    expect(apiErrorMessage('consentRequired')).toBe(catalogues.it.semanticError.consentRequired);
    expect(apiErrorMessage('state', undefined, 'state error: disk full')).toContain(
      'state error: disk full',
    );
    const space = apiErrorMessage('insufficientSpace', {
      kind: 'insufficientSpace',
      availableBytes: 1_048_576,
      requiredBytes: 2_147_483_648,
      reserveBytes: 0,
    });
    expect(space).toContain('2 GiB');
    expect(space).toContain('1 MiB');
  });

  it('localises unexpected-response errors and plain rejections without raw strings', () => {
    setLocale('pl');
    expect(unexpectedResponseError('listDirectory', 202).message).toBe(
      catalogues.pl.apiError.unexpectedResponse,
    );
    expect(errorText({ code: 'notFound', message: 'resource not found' })).toBe(
      catalogues.pl.apiError.notFound,
    );
    expect(errorText({ unrelated: true })).toBe(catalogues.pl.apiError.unknownFailure);
    expect(errorText(new Error('plain'))).toBe('plain');
  });

  it('localises failures, geo messages, notifications and viewer warnings by code', () => {
    setLocale('de');
    expect(
      operationFailureText({
        code: 'tooManyParts',
        message: 'x',
        details: { name: 'a', max: '9' },
      }),
    ).toBe(
      catalogues.de.operationFailure.tooManyParts.replace('{name}', 'a').replace('{max}', '9'),
    );
    expect(operationFailureText({ code: 'operationFailed', message: 'raw' })).toBe(
      catalogues.de.operationFailure.operationFailedPlain,
    );
    expect(
      geoMessageText({ code: 'rowsSkipped', message: 'x', params: { count: '2', first: '3' } }),
    ).toBe(catalogues.de.geoMessage.rowsSkipped.replace('{count}', '2').replace('{first}', '3'));
    expect(notificationText({ code: 'clipboardCopied', message: 'Copied' })).toBe(
      catalogues.de.notification.clipboardCopied,
    );
    expect(viewerWarningText('raw', 'walNotShown')).toBe(catalogues.de.viewerWarning.walNotShown);
    expect(viewerWarningText('raw', 'futureCode')).toBe('raw');
    expect(
      apiErrorMessage('invalidRequest', {
        reason: 'previewBudget',
        resource: 'xmlDepth',
        limit: '64',
      }),
    ).toContain('64');
  });
});
