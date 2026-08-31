import m, { type FactoryComponent } from 'mithril';
import { ModalPanel, Select } from 'mithril-materialized';

import { t } from '../../i18n';
import type { ComparisonStatus, SyncAction, SyncPlanItem } from '../../models';

export interface SyncPlanDialogAttrs {
  readonly open: boolean;
  /** The freshly generated proposal; reset into local editable state on open. */
  readonly items: readonly SyncPlanItem[];
  readonly applying?: boolean;
  readonly error?: string;
  readonly onApply: (items: readonly SyncPlanItem[]) => void;
  readonly onCancel: () => void;
}

function statusLabel(): Record<ComparisonStatus, string> {
  return {
    onlyLeft: t('comparison', 'statusOnlyLeft'),
    onlyRight: t('comparison', 'statusOnlyRight'),
    newer: t('comparison', 'statusNewer'),
    older: t('comparison', 'statusOlder'),
    differentSize: t('comparison', 'statusDifferentSize'),
    identical: t('comparison', 'statusIdentical'),
    typeMismatch: t('comparison', 'statusTypeMismatch'),
  };
}

function actionOptions(): { id: SyncAction; label: string }[] {
  return [
    { id: 'skip', label: t('comparison', 'actionSkip') },
    { id: 'copyLeftToRight', label: t('comparison', 'actionCopyLeftToRight') },
    { id: 'copyRightToLeft', label: t('comparison', 'actionCopyRightToLeft') },
    { id: 'deleteLeft', label: t('comparison', 'actionDeleteLeft') },
    { id: 'deleteRight', label: t('comparison', 'actionDeleteRight') },
  ];
}

function sizeLabel(item: SyncPlanItem): string {
  const left = item.left?.size;
  const right = item.right?.size;
  const format = (value: number | undefined): string => (value === undefined ? '—' : `${value} B`);
  return `${format(left)} / ${format(right)}`;
}

/**
 * Moves focus away from the input before the modal closes, so the browser
 * never has to apply aria-hidden to an ancestor of the focused element.
 */
function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

/**
 * Reviews and edits a proposed sync plan before applying it (spec §16 milestone 5, §35: nothing
 * runs without this explicit, reviewed confirmation). Every row is independently editable; the
 * plan is applied exactly as shown, never silently.
 */
export const SyncPlanDialog: FactoryComponent<SyncPlanDialogAttrs> = () => {
  let items: SyncPlanItem[] = [];
  let wasOpen = false;

  function setAction(relativePath: string, action: SyncAction): void {
    items = items.map((item) => (item.relativePath === relativePath ? { ...item, action } : item));
  }

  function cancel(attrs: SyncPlanDialogAttrs): void {
    blurActive();
    attrs.onCancel();
  }

  function apply(attrs: SyncPlanDialogAttrs): void {
    blurActive();
    attrs.onApply(items);
  }

  return {
    view: ({ attrs }) => {
      // ModalPanel keeps this component permanently mounted and only toggles CSS visibility, so
      // reset local editable state on the false->true open transition here, mirroring
      // MultiRenameDialog's rationale for doing the same in `view` rather than `onupdate`.
      if (attrs.open && !wasOpen) items = [...attrs.items];
      wasOpen = attrs.open;

      const actionableCount = items.filter((item) => item.action !== 'skip').length;

      return m(ModalPanel, {
        title: t('comparison', 'title'),
        className: 'fm-dense-modal fm-sync-plan-dialog',
        description: m('.fm-sync-plan-body', [
          items.length === 0
            ? m('p', t('comparison', 'identical'))
            : m('table.fm-sync-plan-table', [
                m(
                  'thead',
                  m('tr', [
                    m('th', t('comparison', 'path')),
                    m('th', t('comparison', 'status')),
                    m('th', t('comparison', 'sizeColumn')),
                    m('th', t('comparison', 'action')),
                  ]),
                ),
                m(
                  'tbody',
                  items.map((item) =>
                    m('tr', { key: item.relativePath }, [
                      m('td.fm-sync-plan-path', item.relativePath),
                      m('td', statusLabel()[item.status]),
                      m('td', sizeLabel(item)),
                      m(
                        'td',
                        m(Select, {
                          id: `sync-plan-action-${item.relativePath}`,
                          label: '',
                          options: actionOptions(),
                          checkedId: item.action,
                          onchange: (value) => setAction(item.relativePath, value[0] as SyncAction),
                        }),
                      ),
                    ]),
                  ),
                ),
              ]),
          attrs.error === undefined ? undefined : m('.fm-field-error', attrs.error),
        ]),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) cancel(attrs);
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: () => cancel(attrs) },
          {
            label:
              (attrs.applying ?? false)
                ? t('comparison', 'applying')
                : t('comparison', 'apply', { count: actionableCount }),
            disabled: (attrs.applying ?? false) || actionableCount === 0,
            onclick: () => apply(attrs),
          },
        ],
      });
    },
  };
};
