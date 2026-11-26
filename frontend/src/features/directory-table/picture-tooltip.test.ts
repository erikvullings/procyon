import { afterEach, describe, expect, it, vi } from 'vitest';
import type { EntrySummary } from '../../models';
import { DEFAULT_ENTRY_FORMAT_SETTINGS } from '../entry-formatting/entry-formatting';
import type { ImageSummaryLoader } from './image-summary-loader';
import type { GridPictureInfo } from './picture-info';
import { PictureTooltipController } from './picture-tooltip';

function entry(overrides: Partial<EntrySummary> = {}): EntrySummary {
  return {
    id: 'photo.jpg',
    location: { providerId: 'file', uri: 'file:///tmp/photo.jpg' },
    name: 'photo.jpg',
    kind: 'file',
    size: 4_690_000,
    modifiedAt: '2026-08-04T00:00:00.000Z',
    hidden: false,
    readOnly: false,
    extension: 'jpg',
    metadataRevision: 1,
    ...overrides,
  };
}

/** Picture info with no summary loaded: the tooltip still shows the name and the type line. */
function info(overrides: { enabled?: boolean; delayMs?: number } = {}): GridPictureInfo {
  return {
    summaryLoader: { summaryFor: () => undefined } as unknown as ImageSummaryLoader,
    captionFields: [],
    tooltip: {
      enabled: overrides.enabled ?? true,
      delayMs: overrides.delayMs ?? 10,
      fields: [],
    },
    formatSettings: DEFAULT_ENTRY_FORMAT_SETTINGS,
  };
}

const anchor = (): HTMLElement => document.createElement('div');
const shownTooltip = (): Element | null => document.querySelector('.fm-picture-tooltip');

describe('PictureTooltipController', () => {
  let controller: PictureTooltipController | undefined;

  afterEach(() => {
    controller?.hide();
    controller = undefined;
    for (const node of document.querySelectorAll('.fm-picture-tooltip-root')) node.remove();
  });

  it('shows the tooltip for a picture only after the hover delay', async () => {
    controller = new PictureTooltipController();
    controller.schedule(entry(), anchor(), info({ delayMs: 30 }), undefined, 0);
    expect(shownTooltip()).toBeNull();
    await vi.waitFor(() => expect(shownTooltip()?.textContent).toContain('photo.jpg'));
  });

  it('does not arm for a disabled tooltip, a held button, or a non-media file', async () => {
    controller = new PictureTooltipController();
    controller.schedule(entry(), anchor(), info({ enabled: false }), undefined, 0);
    controller.schedule(entry(), anchor(), info(), undefined, 1);
    controller.schedule(
      entry({ name: 'notes.txt', extension: 'txt' }),
      anchor(),
      info(),
      undefined,
      0,
    );
    await new Promise((resolve) => setTimeout(resolve, 40));
    expect(shownTooltip()).toBeNull();
  });

  it('is dismissed by a key press but not by a modifier that only prepares a shortcut', async () => {
    controller = new PictureTooltipController();
    controller.toggle(entry(), anchor(), info(), undefined);
    await vi.waitFor(() => expect(shownTooltip()).not.toBeNull());

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Shift' }));
    expect(controller.toggle(entry(), anchor(), info(), undefined)).toBe(true);
    // The toggle above hid it again: the same entry was showing.
    await vi.waitFor(() => expect(shownTooltip()).toBeNull());

    controller.toggle(entry(), anchor(), info(), undefined);
    await vi.waitFor(() => expect(shownTooltip()).not.toBeNull());
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Control' }));
    expect(shownTooltip()).not.toBeNull();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
    await vi.waitFor(() => expect(shownTooltip()).toBeNull());
  });

  it('is dismissed by a pointer press, so it never sits on a drag', async () => {
    controller = new PictureTooltipController();
    controller.toggle(entry(), anchor(), info(), undefined);
    await vi.waitFor(() => expect(shownTooltip()).not.toBeNull());
    document.dispatchEvent(new Event('pointerdown'));
    await vi.waitFor(() => expect(shownTooltip()).toBeNull());
  });

  it('toggles at once for the cursor entry and ignores files it cannot describe', async () => {
    controller = new PictureTooltipController();
    expect(
      controller.toggle(entry({ name: 'a.txt', extension: 'txt' }), anchor(), info(), undefined),
    ).toBe(false);
    expect(shownTooltip()).toBeNull();
    // Shown at once: no hover delay, and the hover setting does not matter.
    expect(
      controller.toggle(entry(), anchor(), info({ enabled: false, delayMs: 60_000 }), undefined),
    ).toBe(true);
    await vi.waitFor(() => expect(shownTooltip()).not.toBeNull());
  });
});
