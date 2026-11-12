import m, { type Vnode } from 'mithril';

import { t } from '../i18n';
import { checkIcon, closeIcon } from './tabler-icons';
import { tooltip } from './tooltip';

export interface InlineEditActionsAttrs {
  /** Extra class on the confirm (submit) button, for feature-level hooks. */
  readonly confirmClass?: string;
  /** Extra class on the cancel button, for feature-level hooks. */
  readonly cancelClass?: string;
  /** Accessible name and tooltip of the confirm button; defaults to "Save". */
  readonly confirmLabel?: string;
  readonly confirmDisabled?: boolean;
  readonly onCancel: () => void;
}

/**
 * The one confirm/cancel pair for an edit that happens inline in a row (DESIGN.md "Decision
 * actions"): a check icon that submits the surrounding form (Enter) and an x icon that abandons it
 * (Esc). Dialog and settings footers use text buttons instead.
 */
export function inlineEditActions(attrs: InlineEditActionsAttrs): Vnode[] {
  const confirmLabel = attrs.confirmLabel ?? t('button', 'save');
  const cancelLabel = t('button', 'cancel');
  return [
    tooltip(
      confirmLabel,
      m(
        'button.fm-inline-action.fm-inline-action-confirm',
        {
          type: 'submit',
          class: attrs.confirmClass,
          'aria-label': confirmLabel,
          disabled: attrs.confirmDisabled === true,
        },
        checkIcon({ size: 16 }),
      ),
    ),
    tooltip(
      cancelLabel,
      m(
        'button.fm-inline-action.fm-inline-action-cancel',
        {
          type: 'button',
          class: attrs.cancelClass,
          'aria-label': cancelLabel,
          onclick: attrs.onCancel,
        },
        closeIcon({ size: 16 }),
      ),
    ),
  ];
}
