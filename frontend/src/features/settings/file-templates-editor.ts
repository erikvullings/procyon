import m, { type FactoryComponent } from 'mithril';
import { t } from '../../i18n';
import type { FileTemplate } from '../../models';
import { DEFAULT_FILE_TEMPLATES, newTemplateId } from '../templates/file-templates';

export interface FileTemplatesEditorAttrs {
  readonly templates: readonly FileTemplate[];
  readonly onChange: (templates: readonly FileTemplate[]) => void;
  /** The template folder (task 0312); `null` when the folder source is off. */
  readonly folder: string | null;
  readonly onFolderChange: (folder: string | null) => void;
}

/** Edits the "New file from template" list in place: enable, rename, change content, add, reset. */
export const FileTemplatesEditor: FactoryComponent<FileTemplatesEditorAttrs> = () => ({
  view: ({ attrs }) => {
    const replace = (index: number, patch: Partial<FileTemplate>): void =>
      attrs.onChange(
        attrs.templates.map((template, position) =>
          position === index ? { ...template, ...patch } : template,
        ),
      );
    return m('.fm-file-templates', [
      m('label', [
        m('span', t('settings', 'templateFolder')),
        m('input[type=text]', {
          value: attrs.folder ?? '',
          oninput: (event: InputEvent) => {
            const value = (event.currentTarget as HTMLInputElement).value.trim();
            attrs.onFolderChange(value === '' ? null : value);
          },
        }),
        m('p.helper-text', t('settings', 'templateFolderHelp')),
      ]),
      attrs.templates.map((template, index) =>
        m('fieldset.fm-file-template', { key: template.id }, [
          m('label.fm-file-template__enabled', [
            m('input[type=checkbox]', {
              checked: template.enabled,
              onchange: (event: Event) =>
                replace(index, { enabled: (event.currentTarget as HTMLInputElement).checked }),
            }),
            m('span', t('settings', 'templateEnabled')),
          ]),
          m('label', [
            m('span', t('settings', 'templateLabel')),
            m('input[type=text]', {
              value: template.label,
              oninput: (event: InputEvent) =>
                replace(index, { label: (event.currentTarget as HTMLInputElement).value }),
            }),
          ]),
          m('label', [
            m('span', t('settings', 'templateFileName')),
            m('input[type=text]', {
              value: template.fileName,
              oninput: (event: InputEvent) =>
                replace(index, { fileName: (event.currentTarget as HTMLInputElement).value }),
            }),
          ]),
          m('label', [
            m('span', t('settings', 'templateBody')),
            m('textarea.browser-default.fm-file-template__body', {
              rows: 4,
              value: template.body,
              oninput: (event: InputEvent) =>
                replace(index, { body: (event.currentTarget as HTMLTextAreaElement).value }),
            }),
          ]),
          m(
            'button.btn-flat',
            {
              type: 'button',
              onclick: () => attrs.onChange(attrs.templates.filter((_, i) => i !== index)),
            },
            t('settings', 'templateRemove'),
          ),
        ]),
      ),
      m('.fm-file-templates__actions', [
        m(
          'button.btn-flat',
          {
            type: 'button',
            onclick: () =>
              attrs.onChange([
                ...attrs.templates,
                {
                  id: newTemplateId(attrs.templates),
                  label: t('settings', 'templateNewLabel'),
                  fileName: 'new.txt',
                  body: '',
                  enabled: true,
                },
              ]),
          },
          t('settings', 'templateAdd'),
        ),
        m(
          'button.btn-flat',
          {
            type: 'button',
            onclick: () =>
              attrs.onChange(DEFAULT_FILE_TEMPLATES.map((template) => ({ ...template }))),
          },
          t('settings', 'templateReset'),
        ),
      ]),
    ]);
  },
});
