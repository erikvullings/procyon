import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type { FinderTag, FinderTagColor } from '../../models';
import {
  FINDER_TAG_COLORS,
  finderTagColorLabel,
  finderTagColorSwatch,
} from '../directory-table/finder-tag-colors';

export interface FinderTagsDialogAttrs {
  readonly open: boolean;
  readonly entryName: string;
  readonly initialTags: readonly FinderTag[];
  readonly onConfirm: (tags: readonly FinderTag[]) => void;
  readonly onCancel: () => void;
}

function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

function swatch(color: FinderTagColor, selected: boolean, onclick: () => void): m.Children {
  const background = finderTagColorSwatch(color);
  return m('button.fm-finder-tag-color-swatch', {
    type: 'button',
    key: color,
    title: finderTagColorLabel(color),
    'aria-label': finderTagColorLabel(color),
    'aria-pressed': selected ? 'true' : 'false',
    class: selected ? 'fm-finder-tag-color-swatch--selected' : undefined,
    style: background === undefined ? undefined : { background },
    onclick,
  });
}

/** Minimal modal for assigning, removing and creating Finder tags on one entry (task 0136) - a
 * standalone surface until 0140's properties dialog exists to host it instead. Editing is
 * all-at-once (mirrors Finder's own tag editor and `PlatformAdapter::set_finder_tags`): this
 * dialog's local `tags` list is the complete replacement set sent on Save. */
export const FinderTagsDialog: FactoryComponent<FinderTagsDialogAttrs> = () => {
  let tags: FinderTag[] = [];
  let draftName = '';
  let draftColor: FinderTagColor = 'none';
  let wasOpen = false;

  function confirm(attrs: FinderTagsDialogAttrs): void {
    blurActive();
    attrs.onConfirm(tags);
  }

  function cancel(attrs: FinderTagsDialogAttrs): void {
    blurActive();
    attrs.onCancel();
  }

  function addDraftTag(): void {
    const name = draftName.trim();
    if (name.length === 0) return;
    tags = [...tags, { name, color: draftColor }];
    draftName = '';
    draftColor = 'none';
  }

  function removeTag(index: number): void {
    tags = tags.filter((_tag, tagIndex) => tagIndex !== index);
  }

  return {
    onupdate: ({ attrs }) => {
      if (attrs.open && !wasOpen) {
        tags = [...attrs.initialTags];
        draftName = '';
        draftColor = 'none';
        document.getElementById('finder-tags-new-name')?.focus();
      }
      wasOpen = attrs.open;
    },
    view: ({ attrs }) =>
      m(ModalPanel, {
        title: t('entryMetadata', 'tagsTitle', { name: attrs.entryName }),
        className: 'fm-dense-modal',
        description: m('.fm-finder-tags-editor', [
          tags.length === 0
            ? m('p.fm-finder-tags-empty', t('entryMetadata', 'noTags'))
            : m(
                'ul.fm-finder-tags-list',
                tags.map((tag, index) =>
                  m('li.fm-finder-tags-chip', { key: `${tag.name}-${tag.color}-${index}` }, [
                    finderTagColorSwatch(tag.color) === undefined
                      ? undefined
                      : m('span.fm-finder-tag-color-dot', {
                          style: { background: finderTagColorSwatch(tag.color) },
                        }),
                    m('span.fm-finder-tags-chip-name', tag.name),
                    m(
                      'button.fm-finder-tags-chip-remove',
                      {
                        type: 'button',
                        'aria-label': t('entryMetadata', 'removeTagAriaLabel', { name: tag.name }),
                        onclick: () => removeTag(index),
                      },
                      '×',
                    ),
                  ]),
                ),
              ),
          m('.fm-finder-tags-add-row', [
            m('input#finder-tags-new-name', {
              type: 'text',
              placeholder: t('entryMetadata', 'newTagNamePlaceholder'),
              value: draftName,
              oninput: (event: InputEvent) => {
                draftName = (event.currentTarget as HTMLInputElement).value;
              },
              onkeydown: (event: KeyboardEvent) => {
                if (event.key === 'Escape') {
                  event.stopPropagation();
                  cancel(attrs);
                } else if (event.key === 'Enter') {
                  event.preventDefault();
                  event.stopPropagation();
                  addDraftTag();
                }
              },
            }),
            m(
              '.fm-finder-tag-color-picker',
              { role: 'radiogroup', 'aria-label': t('entryMetadata', 'tagColorAriaLabel') },
              FINDER_TAG_COLORS.map((color) =>
                swatch(color, draftColor === color, () => {
                  draftColor = color;
                }),
              ),
            ),
            m(
              'button.fm-finder-tags-add-button',
              { type: 'button', disabled: draftName.trim().length === 0, onclick: addDraftTag },
              t('entryMetadata', 'add'),
            ),
          ]),
        ]),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) cancel(attrs);
        },
        buttons: [
          {
            label: t('entryMetadata', 'removeAll'),
            disabled: tags.length === 0,
            onclick: () => {
              tags = [];
            },
          },
          { label: t('button', 'cancel'), onclick: () => cancel(attrs) },
          { label: t('button', 'save'), onclick: () => confirm(attrs) },
        ],
      }),
  };
};
