import m from 'mithril';

/**
 * Wraps `child` in an anchor that shows `label` as a tooltip on hover/focus (native `title`
 * tooltips are unreliable across browsers/webviews).
 *
 * The tooltip itself renders into a single `body`-level portal element, positioned with
 * `getBoundingClientRect` on show, rather than as a `::after` pseudo-element painted on the
 * wrapper in place. Two independent things in this app clip an in-place tooltip before it can
 * ever be seen: an icon button's own `waves-effect` ripple class sets `overflow: hidden` on the
 * button, and several containers a tooltip-bearing control lives inside (e.g. the pane tab
 * strip's horizontally-scrolling `.fm-pane-tabs`) set `overflow-x: hidden` for unrelated reasons
 * - which, per the CSS overflow spec, forces the *other* axis from `visible` to `auto` too,
 * clipping vertically as a side effect. A `position: fixed` portal outside the whole DOM subtree
 * sidesteps both without touching that unrelated layout.
 *
 * The wrapper still carries `label` as `data-tooltip` (unused for rendering) so tests can assert
 * which control owns which tooltip without needing to simulate hover/focus.
 */
export function tooltip(label: string, child: m.Children, extraAttrs?: m.Attributes): m.Vnode {
  return m(
    'span.fm-tooltip',
    {
      ...extraAttrs,
      'data-tooltip': label,
      onmouseenter: (event: MouseEvent) => showTooltipPortal(event.currentTarget, label),
      onmouseleave: hideTooltipPortal,
      onfocusin: (event: FocusEvent) => showTooltipPortal(event.currentTarget, label),
      onfocusout: hideTooltipPortal,
    },
    child,
  );
}

let portal: HTMLDivElement | undefined;

function ensurePortal(): HTMLDivElement {
  if (portal !== undefined) return portal;
  const created = document.createElement('div');
  created.className = 'fm-tooltip-portal';
  created.setAttribute('role', 'tooltip');
  document.body.appendChild(created);
  portal = created;
  return created;
}

function showTooltipPortal(target: EventTarget | null, label: string): void {
  if (!(target instanceof HTMLElement)) return;
  const element = ensurePortal();
  element.textContent = label;
  element.style.display = 'block';
  const anchorRect = target.getBoundingClientRect();
  const portalRect = element.getBoundingClientRect();
  const left = Math.max(
    4,
    Math.min(
      anchorRect.left + anchorRect.width / 2 - portalRect.width / 2,
      window.innerWidth - portalRect.width - 4,
    ),
  );
  element.style.left = `${left}px`;
  element.style.top = `${anchorRect.bottom + 6}px`;
}

function hideTooltipPortal(): void {
  if (portal === undefined) return;
  portal.style.display = 'none';
}
