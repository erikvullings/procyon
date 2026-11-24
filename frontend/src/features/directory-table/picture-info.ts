import type { PictureInfoSettings } from '../../models/settings';
import type { EntryFormatSettings } from '../entry-formatting/entry-formatting';
import { type ImageFieldId, knownFields } from './image-caption';
import type { ImageSummaryLoader } from './image-summary-loader';

/** Everything a grid needs to show picture captions and tooltips (task 0347), bundled so it
 * travels through the pane wiring as one attribute. */
export interface GridPictureInfo {
  readonly summaryLoader: ImageSummaryLoader;
  readonly captionFields: readonly ImageFieldId[];
  readonly tooltip: {
    readonly enabled: boolean;
    readonly delayMs: number;
    readonly fields: readonly ImageFieldId[];
  };
  readonly formatSettings: EntryFormatSettings;
}

/** Combines the persisted settings with the live loader and number/date formatting. */
export function gridPictureInfo(
  settings: PictureInfoSettings,
  summaryLoader: ImageSummaryLoader,
  formatSettings: EntryFormatSettings,
): GridPictureInfo {
  return {
    summaryLoader,
    captionFields: knownFields(settings.captionFields),
    tooltip: {
      enabled: settings.tooltipEnabled,
      delayMs: settings.tooltipDelayMs,
      fields: knownFields(settings.tooltipFields),
    },
    formatSettings,
  };
}
