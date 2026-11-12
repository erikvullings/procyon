import m, { type Children, type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type { FileTemplateEntry } from '../../models';
import { validateEntryName } from '../operations/create-directory-dialog';
import { focusAndSelect } from '../operations/focus-select';

export interface NewFromTemplateDialogAttrs {
  readonly open: boolean;
  readonly templates: readonly FileTemplateEntry[];
  readonly onCreate: (template: FileTemplateEntry, fileName: string) => void;
  /** Creates an empty file under `fileName` and opens the assistant chat for it (task 0321). */
  readonly onDraft: (fileName: string) => void;
  readonly onCancel: () => void;
}

/** The small suffix naming a non-app source in the picker: the folder, or the contributing plugin. */
function sourceTag(template: FileTemplateEntry): Children | undefined {
  if (template.source === 'app') return undefined;
  const text =
    template.source === 'folder'
      ? t('newFromTemplate', 'folderSource')
      : (template.sourceName ?? t('newFromTemplate', 'pluginSource'));
  return m('span.fm-terminal-picker__source', text);
}

/** Moves focus out of the modal before it closes, so the browser never applies aria-hidden to an
 * ancestor of the focused element. */
function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

/**
 * "New file from template" (task 0312): pick a template, adjust the suggested file name, create.
 * The name is pre-selected so typing replaces it; the file is created without overwriting.
 */
export const NewFromTemplateDialog: FactoryComponent<NewFromTemplateDialogAttrs> = () => {
  let selectedId: string | undefined;
  let fileName = '';
  let wasOpen = false;
  let focusPending = false;

  function select(template: FileTemplateEntry): void {
    selectedId = template.id;
    fileName = template.fileName;
    focusAndSelect('new-from-template-name', fileName);
  }

  function confirm(attrs: NewFromTemplateDialogAttrs): void {
    const template = attrs.templates.find((candidate) => candidate.id === selectedId);
    if (template === undefined || validateEntryName(fileName) !== undefined) return;
    blurActive();
    attrs.onCreate(template, fileName);
  }

  function draft(attrs: NewFromTemplateDialogAttrs): void {
    if (selectedId === undefined || validateEntryName(fileName) !== undefined) return;
    blurActive();
    attrs.onDraft(fileName);
  }

  function cancel(attrs: NewFromTemplateDialogAttrs): void {
    blurActive();
    attrs.onCancel();
  }

  return {
    onupdate: () => {
      if (focusPending) {
        focusPending = false;
        focusAndSelect('new-from-template-name', fileName);
      }
    },
    view: ({ attrs }) => {
      // Reset while rendering so the first frame already shows the first template's name.
      if (attrs.open && !wasOpen) {
        const first = attrs.templates[0];
        selectedId = first?.id;
        fileName = first?.fileName ?? '';
        focusPending = first !== undefined;
      }
      wasOpen = attrs.open;
      const error = fileName === '' ? undefined : validateEntryName(fileName);
      const canCreate = selectedId !== undefined && fileName !== '' && error === undefined;
      return m(ModalPanel, {
        title: t('newFromTemplate', 'title'),
        className: 'fm-dense-modal',
        description:
          attrs.templates.length === 0
            ? m('p', t('newFromTemplate', 'empty'))
            : m('.fm-new-from-template', [
                m(
                  'ul.fm-terminal-picker',
                  { role: 'radiogroup', 'aria-label': t('newFromTemplate', 'templates') },
                  attrs.templates.map((template) =>
                    m(
                      'li',
                      { key: template.id },
                      m(
                        'button.fm-terminal-picker__item',
                        {
                          type: 'button',
                          role: 'radio',
                          'aria-checked': template.id === selectedId ? 'true' : 'false',
                          title: template.fileName,
                          onclick: () => select(template),
                        },
                        template.label,
                        sourceTag(template),
                      ),
                    ),
                  ),
                ),
                m('label.fm-create-directory-field', [
                  m('span', t('newFromTemplate', 'fileName')),
                  m('input#new-from-template-name', {
                    type: 'text',
                    value: fileName,
                    required: true,
                    'aria-invalid': error === undefined ? undefined : 'true',
                    oninput: (event: InputEvent) => {
                      fileName = (event.currentTarget as HTMLInputElement).value;
                    },
                    onkeydown: (event: KeyboardEvent) => {
                      if (event.key === 'Escape') {
                        event.stopPropagation();
                        cancel(attrs);
                      } else if (event.key === 'Enter') {
                        event.preventDefault();
                        event.stopPropagation();
                        confirm(attrs);
                      }
                    },
                  }),
                  error === undefined ? undefined : m('.fm-field-error', error),
                ]),
              ]),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) cancel(attrs);
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: () => cancel(attrs) },
          {
            label: t('newFromTemplate', 'draftWithAssistant'),
            disabled: !canCreate,
            onclick: () => draft(attrs),
          },
          {
            label: t('button', 'create'),
            disabled: !canCreate,
            onclick: () => confirm(attrs),
          },
        ],
      });
    },
  };
};
