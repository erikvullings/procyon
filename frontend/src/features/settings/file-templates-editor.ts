import m, { type FactoryComponent } from 'mithril';
import { ConfirmButton, FlatButton, Select } from 'mithril-materialized';
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

/**
 * Edits the "New file from template" list one template at a time, picked from a select. Add
 * appends a template, the trash button removes the shown one, and reset restores the bundled list.
 */
export const FileTemplatesEditor: FactoryComponent<FileTemplatesEditorAttrs> = () => {
  /** The template the select shows; falls back to the first one when it is gone. */
  let selectedId: string | undefined;

  return {
    view: ({ attrs }) => {
      const shown =
        attrs.templates.find((template) => template.id === selectedId) ?? attrs.templates[0];
      const replace = (patch: Partial<FileTemplate>): void => {
        if (shown === undefined) return;
        attrs.onChange(
          attrs.templates.map((template) =>
            template.id === shown.id ? { ...template, ...patch } : template,
          ),
        );
      };
      const add = (): void => {
        const id = newTemplateId(attrs.templates);
        selectedId = id;
        attrs.onChange([
          ...attrs.templates,
          {
            id,
            label: t('settings', 'templateNewLabel'),
            fileName: 'new_file.txt',
            body: '',
            enabled: true,
            openInEditor: true,
          },
        ]);
      };
      const remove = (): void => {
        if (shown === undefined) return;
        selectedId = undefined;
        attrs.onChange(attrs.templates.filter((template) => template.id !== shown.id));
      };

      return m('.fm-file-templates', [
        m('label.fm-file-templates__folder', [
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
        m('.fm-file-templates__picker', [
          attrs.templates.length === 0
            ? undefined
            : m(Select<string>, {
                className: 'fm-file-templates__select',
                label: t('settings', 'fileTemplates'),
                options: attrs.templates.map((template) => ({
                  id: template.id,
                  label: template.label,
                })),
                ...(shown === undefined ? {} : { checkedId: shown.id }),
                onchange: ([id]) => {
                  if (id !== undefined) selectedId = id;
                },
              }),
          m(FlatButton, { label: t('settings', 'templateAdd'), onclick: add }),
        ]),
        shown === undefined
          ? m('p.helper-text', t('newFromTemplate', 'empty'))
          : m('.fm-file-template', { key: shown.id }, [
              m('.fm-file-template__head', [
                m('label.fm-file-template__name', [
                  m('span', t('settings', 'templateLabel')),
                  m('input[type=text]', {
                    value: shown.label,
                    oninput: (event: InputEvent) =>
                      replace({ label: (event.currentTarget as HTMLInputElement).value }),
                  }),
                ]),
                m('label.fm-file-template__flag', [
                  m('input[type=checkbox]', {
                    checked: shown.enabled,
                    onchange: (event: Event) =>
                      replace({ enabled: (event.currentTarget as HTMLInputElement).checked }),
                  }),
                  m('span', t('settings', 'templateEnabled')),
                ]),
                m('label.fm-file-template__flag', [
                  m('input[type=checkbox]', {
                    checked: shown.openInEditor,
                    onchange: (event: Event) =>
                      replace({ openInEditor: (event.currentTarget as HTMLInputElement).checked }),
                  }),
                  m('span', t('settings', 'templateOpenInEditor')),
                ]),
              ]),
              m('label', [
                m('span', t('settings', 'templateFileName')),
                m('input[type=text]', {
                  value: shown.fileName,
                  oninput: (event: InputEvent) =>
                    replace({ fileName: (event.currentTarget as HTMLInputElement).value }),
                }),
              ]),
              m('label', [
                m('span', t('settings', 'templateBody')),
                m('textarea.browser-default.fm-file-template__body', {
                  rows: 12,
                  value: shown.body,
                  oninput: (event: InputEvent) =>
                    replace({ body: (event.currentTarget as HTMLTextAreaElement).value }),
                }),
              ]),
              m('.fm-file-template__footer', [
                m(ConfirmButton, {
                  iconName: 'delete',
                  tooltip: t('settings', 'templateRemove'),
                  className: 'fm-file-template__remove',
                  onclick: remove,
                }),
              ]),
            ]),
        m(
          '.fm-file-templates__actions',
          m(FlatButton, {
            label: t('settings', 'templateReset'),
            onclick: () => {
              selectedId = undefined;
              attrs.onChange(DEFAULT_FILE_TEMPLATES.map((template) => ({ ...template })));
            },
          }),
        ),
      ]);
    },
  };
};
