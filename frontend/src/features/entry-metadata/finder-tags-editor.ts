import m, { type FactoryComponent } from 'mithril';
import { t } from '../../i18n';
import type { FinderTag, FinderTagColor } from '../../models';
import {
  FINDER_TAG_COLORS,
  finderTagColorLabel,
  finderTagColorSwatch,
} from '../directory-table/finder-tag-colors';
import { renderFinderTagChips } from './finder-tag-chips';

export interface FinderTagsEditorAttrs {
  /** Seeds the working copy on the false→true edge (mirrors Finder's all-at-once replacement). */
  readonly open: boolean;
  readonly initialTags: readonly FinderTag[];
  /** Commits the working set (Save). */
  readonly onConfirm: (tags: readonly FinderTag[]) => void;
  /** Discards the working set (Cancel / Esc). */
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

/**
 * Inline Finder tag editor (task 0136/0145): the chip list, add-row and color picker shared by the
 * standalone {@link FinderTagsDialog} and the Properties dialog's Tags section. It owns only its
 * working copy — the surrounding surface (modal shell, persistence) is the caller's job — so it can
 * render either inside a `ModalPanel` or directly in a properties body without nesting modals.
 */
export const FinderTagsEditor: FactoryComponent<FinderTagsEditorAttrs> = () => {
  let tags: FinderTag[] = [];
  let draftName = '';
  let draftColor: FinderTagColor = 'none';
  let wasOpen = false;

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
    view: ({ attrs }) => [
      tags.length === 0
        ? m('p.fm-finder-tags-empty', t('entryMetadata', 'noTags'))
        : renderFinderTagChips(tags, removeTag),
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
              blurActive();
              attrs.onCancel();
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
      m('.fm-finder-tags-actions.fm-form-actions', [
        m(
          'button.btn-flat.fm-finder-tags-remove-all.fm-form-actions-leading',
          {
            type: 'button',
            disabled: tags.length === 0,
            onclick: () => {
              tags = [];
            },
          },
          t('entryMetadata', 'removeAll'),
        ),
        m(
          'button.btn-flat.fm-finder-tags-cancel',
          {
            type: 'button',
            onclick: () => {
              blurActive();
              attrs.onCancel();
            },
          },
          t('button', 'cancel'),
        ),
        m(
          'button.btn-flat.fm-primary-action.fm-finder-tags-save',
          {
            type: 'button',
            onclick: () => {
              blurActive();
              attrs.onConfirm(tags);
            },
          },
          t('button', 'save'),
        ),
      ]),
    ],
  };
};
