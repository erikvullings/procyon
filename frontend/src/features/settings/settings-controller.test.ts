import { afterEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { getLocale, setLocale } from '../../i18n';
import { formatEntryModifiedAt, formatEntrySize } from '../entry-formatting/entry-formatting';
import { createSettingsController, type SettingsControllerContext } from './settings-controller';

const timestamp = '2026-07-31T14:05:00.000Z';

afterEach(() => {
  vi.restoreAllMocks();
  setLocale('en');
});

describe('settings appearance entry formatting', () => {
  it.each(['C', 'zz-ZZ'])(
    'falls back to the selected UI language when the OS language is %s',
    async (osLanguage) => {
      vi.spyOn(navigator, 'language', 'get').mockReturnValue(osLanguage);
      const client = new MockFileManagerClient();
      const settings = {
        ...(await client.getSettings()),
        language: 'fr' as const,
        sizeFormat: 'bytes' as const,
      };
      const setLoadedEntryFormatSettings = vi.fn();
      const controller = createSettingsController(context(client, setLoadedEntryFormatSettings));

      controller.applyAppearance(settings);

      const entrySettings = setLoadedEntryFormatSettings.mock.lastCall?.[0];
      expect(formatEntrySize({ kind: 'file', size: 1_536 }, entrySettings)).toBe(
        `${new Intl.NumberFormat('fr').format(1_536)} B`,
      );
      expect(formatEntryModifiedAt(timestamp, entrySettings)).toBe(
        new Intl.DateTimeFormat('fr', { dateStyle: 'medium', timeStyle: 'short' }).format(
          new Date(timestamp),
        ),
      );
      expect(entrySettings.locale).toBe('fr');
      expect(getLocale()).toBe('fr');
    },
  );

  it('retains a valid OS formatting locale independently of the selected UI language', async () => {
    vi.spyOn(navigator, 'language', 'get').mockReturnValue('de-DE');
    const client = new MockFileManagerClient();
    const settings = {
      ...(await client.getSettings()),
      language: 'nl' as const,
      sizeFormat: 'bytes' as const,
    };
    const setLoadedEntryFormatSettings = vi.fn();
    const controller = createSettingsController(context(client, setLoadedEntryFormatSettings));

    controller.applyAppearance(settings);

    const entrySettings = setLoadedEntryFormatSettings.mock.lastCall?.[0];
    expect(entrySettings.locale).toBe('de-DE');
    expect(formatEntrySize({ kind: 'file', size: 1_536 }, entrySettings)).toBe('1.536 B');
    expect(getLocale()).toBe('nl');
  });

  it('does not hide unrelated Intl errors', async () => {
    vi.spyOn(Intl.NumberFormat, 'supportedLocalesOf').mockImplementation(() => {
      throw new TypeError('unexpected Intl failure');
    });
    const client = new MockFileManagerClient();
    const controller = createSettingsController(context(client, vi.fn()));
    const settings = await client.getSettings();

    expect(() => controller.applyAppearance(settings)).toThrow('unexpected Intl failure');
  });
});

function context(
  client: MockFileManagerClient,
  setLoadedEntryFormatSettings: SettingsControllerContext['setLoadedEntryFormatSettings'],
): SettingsControllerContext {
  return {
    setTheme: vi.fn(),
    setLoadedEntryFormatSettings,
    getSettingsDialogOpen: () => false,
    setSettingsDialogOpen: vi.fn(),
    getSettingsDisclosureElement: () => undefined,
    getCurrentSettings: () => undefined,
    setCurrentSettings: vi.fn(),
    getPlugins: () => [],
    getInstalledIconThemeId: () => undefined,
    setInstalledIconThemeId: vi.fn(),
    setNativeIconLoaderEnabled: vi.fn(),
    getRuntimeKind: () => 'mock',
    getWorkspace: () => undefined,
    setWorkspace: vi.fn(),
    getDirectories: () => new Map(),
    getNavigation: () => ({ load: async () => undefined }),
    getClient: () => client,
    redraw: vi.fn(),
  };
}
