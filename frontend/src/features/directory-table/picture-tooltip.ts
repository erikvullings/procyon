import m from 'mithril';
import { t } from '../../i18n';
import type { EntrySummary } from '../../models';
import { type ImageFieldId, tooltipRows } from './image-caption';
import { isImageEntry } from './image-summary-loader';
import type { GridPictureInfo } from './picture-info';
import type { ThumbnailLoader } from './thumbnail-loader';
import './picture-tooltip.css';

const TOOLTIP_OFFSET_PX = 8;
const TOOLTIP_WIDTH_PX = 320;
const TOOLTIP_MAX_HEIGHT_PX = 420;

interface ShownTooltip {
  readonly entry: EntrySummary;
  readonly info: GridPictureInfo;
  readonly thumbnails: ThumbnailLoader | undefined;
  readonly left: number;
  readonly top: number;
}

/** The type line under the name: the MIME type when known, else the upper-cased extension. */
function typeLine(entry: EntrySummary): string {
  return entry.mimeType ?? (entry.extension ?? '').toLocaleUpperCase();
}

function placement(anchor: DOMRect): { left: number; top: number } {
  const fitsRight = anchor.right + TOOLTIP_OFFSET_PX + TOOLTIP_WIDTH_PX <= window.innerWidth;
  const left = fitsRight
    ? anchor.right + TOOLTIP_OFFSET_PX
    : Math.max(0, anchor.left - TOOLTIP_OFFSET_PX - TOOLTIP_WIDTH_PX);
  const top = Math.max(0, Math.min(anchor.top, window.innerHeight - TOOLTIP_MAX_HEIGHT_PX));
  return { left, top };
}

function view(shown: ShownTooltip): m.Children {
  const { entry, info, thumbnails } = shown;
  const summary = info.summaryLoader.summaryFor(entry);
  const preview = thumbnails?.thumbnailDataUri(entry, 'large');
  const fields: readonly ImageFieldId[] = info.tooltip.fields;
  const rows = tooltipRows(fields, entry, summary, info.formatSettings);
  const unavailable = summary?.unreadable === true;
  return m(
    '.fm-picture-tooltip',
    { role: 'tooltip', style: { left: `${shown.left}px`, top: `${shown.top}px` } },
    [
      m('.fm-picture-tooltip-name', entry.name),
      m('.fm-picture-tooltip-type', typeLine(entry)),
      preview === undefined
        ? undefined
        : m('img.fm-picture-tooltip-preview', { src: preview, alt: '', 'aria-hidden': 'true' }),
      rows.length > 0
        ? m(
            'table.fm-picture-tooltip-table',
            m(
              'tbody',
              rows.map((row) => m('tr', { key: row.id }, [m('th', row.label), m('td', row.value)])),
            ),
          )
        : undefined,
      unavailable ? m('.fm-picture-tooltip-empty', t('imageMeta', 'unavailable')) : undefined,
    ],
  );
}

/**
 * The hover tooltip of picture tiles (task 0347). One instance serves every grid: hovering a tile
 * starts a timer, the tooltip appears after the configured delay, and it goes away on leaving the
 * tile, scrolling, any pointer press (which may start a drag) or Escape. Hover state is local to
 * this controller; only the settings live in the state tree.
 */
export class PictureTooltipController {
  private timer: ReturnType<typeof setTimeout> | undefined;
  private shown: ShownTooltip | undefined;
  private root: HTMLElement | undefined;
  private readonly dismiss = (): void => this.hide();
  private readonly dismissOnEscape = (event: KeyboardEvent): void => {
    if (event.key === 'Escape') this.hide();
  };

  /** Arms the tooltip for `entry`; ignored for non-pictures and while a button is held. */
  schedule(
    entry: EntrySummary,
    anchor: HTMLElement,
    info: GridPictureInfo | undefined,
    thumbnails: ThumbnailLoader | undefined,
    buttonsPressed: number,
  ): void {
    this.hide();
    if (info === undefined || !info.tooltip.enabled || buttonsPressed !== 0) return;
    if (!isImageEntry(entry)) return;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      this.show({ entry, info, thumbnails, ...placement(anchor.getBoundingClientRect()) });
    }, info.tooltip.delayMs);
  }

  hide(): void {
    if (this.timer !== undefined) {
      clearTimeout(this.timer);
      this.timer = undefined;
    }
    if (this.shown === undefined) return;
    this.shown = undefined;
    document.removeEventListener('keydown', this.dismissOnEscape, true);
    document.removeEventListener('pointerdown', this.dismiss, true);
    document.removeEventListener('scroll', this.dismiss, true);
    document.removeEventListener('dragstart', this.dismiss, true);
    m.redraw();
  }

  private show(tooltip: ShownTooltip): void {
    if (this.root === undefined) {
      this.root = document.createElement('div');
      this.root.className = 'fm-picture-tooltip-root';
      document.body.append(this.root);
      m.mount(this.root, { view: () => (this.shown === undefined ? null : view(this.shown)) });
    }
    this.shown = tooltip;
    document.addEventListener('keydown', this.dismissOnEscape, true);
    document.addEventListener('pointerdown', this.dismiss, true);
    document.addEventListener('scroll', this.dismiss, true);
    document.addEventListener('dragstart', this.dismiss, true);
    m.redraw();
  }
}

/** The shared tooltip instance used by every grid. */
export const pictureTooltip = new PictureTooltipController();
