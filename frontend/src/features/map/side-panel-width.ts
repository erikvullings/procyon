import m, { type Component } from 'mithril';
import { t } from '../../i18n';

/** Width of the geo side panels as a fraction of their container, shared by all geo panels. */
const STORAGE_KEY = 'fm.geoSidePanelFraction';
export const DEFAULT_SIDE_FRACTION = 0.3;
export const MIN_SIDE_FRACTION = 0.12;
export const MAX_SIDE_FRACTION = 0.7;
const KEY_STEP = 0.02;

export const clampFraction = (fraction: number): number =>
  Math.min(MAX_SIDE_FRACTION, Math.max(MIN_SIDE_FRACTION, fraction));

function readFraction(): number {
  try {
    const stored = Number(localStorage.getItem(STORAGE_KEY));
    return Number.isFinite(stored) && stored > 0 ? clampFraction(stored) : DEFAULT_SIDE_FRACTION;
  } catch {
    return DEFAULT_SIDE_FRACTION;
  }
}

let fraction = readFraction();

export const sidePanelFraction = (): number => fraction;

export function setSidePanelFraction(next: number): void {
  fraction = clampFraction(next);
}

function persist(): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(fraction));
  } catch {
    // Storage unavailable: the width then lasts for this session only.
  }
}

/** Inline style giving a side panel its shared width. */
export const sidePanelStyle = (): Record<string, string> => ({
  flexBasis: `${(fraction * 100).toFixed(2)}%`,
});

/**
 * Drag handle between a side panel and the map. Its parent is the flex container whose width
 * the fraction refers to; the panel just before the handle is resized.
 */
export const SidePanelResizer: Component = {
  view: () =>
    m('.fm-side-resizer', {
      role: 'separator',
      'aria-orientation': 'vertical',
      'aria-label': t('geo', 'resizePanel'),
      'aria-valuemin': Math.round(MIN_SIDE_FRACTION * 100),
      'aria-valuemax': Math.round(MAX_SIDE_FRACTION * 100),
      'aria-valuenow': Math.round(fraction * 100),
      tabindex: 0,
      onpointerdown: (event: PointerEvent) => {
        const handle = event.currentTarget as HTMLElement;
        const container = handle.parentElement;
        if (container === null) return;
        handle.setPointerCapture(event.pointerId);
        handle.classList.add('is-dragging');
        const rtl = getComputedStyle(container).direction === 'rtl';
        const move = (e: PointerEvent): void => {
          const rect = container.getBoundingClientRect();
          if (rect.width <= 0) return;
          setSidePanelFraction((rtl ? rect.right - e.clientX : e.clientX - rect.left) / rect.width);
          m.redraw();
        };
        const end = (): void => {
          handle.classList.remove('is-dragging');
          handle.removeEventListener('pointermove', move);
          handle.removeEventListener('pointerup', end);
          handle.removeEventListener('pointercancel', end);
          persist();
        };
        handle.addEventListener('pointermove', move);
        handle.addEventListener('pointerup', end);
        handle.addEventListener('pointercancel', end);
        event.preventDefault();
      },
      onkeydown: (event: KeyboardEvent) => {
        const rtl = getComputedStyle(event.currentTarget as HTMLElement).direction === 'rtl';
        const grow = event.key === (rtl ? 'ArrowLeft' : 'ArrowRight');
        const shrink = event.key === (rtl ? 'ArrowRight' : 'ArrowLeft');
        if (!grow && !shrink) return;
        setSidePanelFraction(fraction + (grow ? KEY_STEP : -KEY_STEP));
        persist();
        event.preventDefault();
      },
    }),
};
