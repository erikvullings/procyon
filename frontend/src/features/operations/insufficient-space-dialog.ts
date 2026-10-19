import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import {
  DEFAULT_ENTRY_FORMAT_SETTINGS,
  formatEntrySize,
} from '../entry-formatting/entry-formatting';
import type { SpaceShortfall } from './insufficient-space';

export interface InsufficientSpaceDialogAttrs {
  readonly shortfall: SpaceShortfall | undefined;
  readonly onProceed: (shortfall: SpaceShortfall) => void;
  readonly onCancel: (shortfall: SpaceShortfall) => void;
}

function size(bytes: number): string {
  return formatEntrySize({ size: bytes, kind: 'file' }, DEFAULT_ENTRY_FORMAT_SETTINGS);
}

/** Shown when a copy or move would not fit; nothing has been written yet (task 0272). */
export const InsufficientSpaceDialog: FactoryComponent<InsufficientSpaceDialogAttrs> = () => ({
  view: ({ attrs }) => {
    const shortfall = attrs.shortfall;
    return m(ModalPanel, {
      title: t('spaceGuard', 'title'),
      description:
        shortfall === undefined
          ? undefined
          : m('.fm-space-guard', [
              m(
                'p',
                t('spaceGuard', 'message', {
                  name: shortfall.operation.kind,
                  required: size(shortfall.required),
                  available: size(shortfall.available),
                  shortfall: size(shortfall.shortfall),
                }),
              ),
              m('p', t('spaceGuard', 'uncertain')),
            ]),
      isOpen: shortfall !== undefined,
      closeOnEsc: true,
      onToggle: (open: boolean) => {
        if (!open && shortfall !== undefined) attrs.onCancel(shortfall);
      },
      buttons: [
        {
          label: t('spaceGuard', 'cancel'),
          onclick: () => shortfall !== undefined && attrs.onCancel(shortfall),
        },
        {
          label: t('spaceGuard', 'proceed'),
          onclick: () => shortfall !== undefined && attrs.onProceed(shortfall),
        },
      ],
    });
  },
});
