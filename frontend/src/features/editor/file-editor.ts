import { linter } from '@codemirror/lint';
import m, { type Component } from 'mithril';
import { FlatButton, IconButton } from 'mithril-materialized';
import { closeIcon } from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import { CodeMirrorEditor } from './code-mirror-editor';
import { jsonParseLinter, languageExtension } from './editor-language';
import type { FileEditorController, FileEditorState } from './file-editor-controller';
import './file-editor.css';

export interface FileEditorAttrs {
  readonly state: FileEditorState;
  readonly controller: FileEditorController;
  readonly onClose: () => void;
}

export const FileEditor: Component<FileEditorAttrs> = {
  view: ({ attrs }) => {
    const { state, controller } = attrs;
    if (state.status === 'loading')
      return m('section.fm-file-editor', m('.fm-file-editor-message', t('editor', 'loading')));
    if (state.status === 'error')
      return m('section.fm-file-editor', [
        m('.fm-file-editor-message', state.message),
        m(FlatButton, { label: t('editor', 'close'), onclick: attrs.onClose }),
      ]);
    return m(
      'section.fm-file-editor',
      { 'aria-label': t('editor', 'editing', { name: state.entry.name }) },
      [
        m('header.fm-file-editor-header', [
          m('strong', state.entry.name),
          state.dirty
            ? m('span.fm-file-editor-dirty', { title: t('editor', 'unsavedChanges') }, '●')
            : undefined,
          m('span.fm-file-editor-spacer'),
          state.language === 'json'
            ? m(FlatButton, {
                label: t('editor', 'formatJson'),
                onclick: () => controller.formatJson(),
              })
            : undefined,
          state.language === 'markdown'
            ? m(FlatButton, {
                label: state.previewVisible ? t('editor', 'edit') : t('editor', 'preview'),
                'aria-pressed': state.previewVisible,
                onclick: () => controller.togglePreview(),
              })
            : undefined,
          m(FlatButton, {
            label: state.saving ? t('editor', 'saving') : t('editor', 'save'),
            disabled: !state.dirty || state.saving,
            onclick: () => void controller.save(),
          }),
          tooltip(
            t('editor', 'closeEditor'),
            m(
              IconButton,
              {
                className: 'fm-file-editor-close',
                'aria-label': t('editor', 'closeEditor'),
                onclick: () => {
                  if (controller.requestClose()) attrs.onClose();
                },
              },
              closeIcon({ size: 13 }),
            ),
          ),
        ]),
        state.error ? m('.fm-file-editor-error', { role: 'alert' }, state.error) : undefined,
        state.conflict
          ? m(
              '.fm-file-editor-conflict',
              { role: 'alertdialog', 'aria-label': t('editor', 'fileChangedOnDisk') },
              [
                m('span', t('editor', 'fileChangedMessage')),
                m(FlatButton, {
                  label: t('editor', 'reload'),
                  onclick: () => void controller.reload(),
                }),
                m(FlatButton, {
                  label: t('editor', 'overwrite'),
                  onclick: () => void controller.save(true),
                }),
                m(FlatButton, {
                  label: t('editor', 'saveAs'),
                  onclick: () => {
                    const uri = window.prompt(t('editor', 'saveAsPrompt'));
                    if (uri !== null && uri.trim() !== '') void controller.save(false, uri.trim());
                  },
                }),
                m(FlatButton, {
                  label: t('editor', 'cancel'),
                  onclick: () => controller.cancelClose(),
                }),
              ],
            )
          : undefined,
        m(
          '.fm-file-editor-content',
          state.language === 'markdown' && state.previewVisible
            ? m('.fm-file-editor-preview', { innerHTML: state.previewHtml ?? '' })
            : m(CodeMirrorEditor, {
                content: state.content,
                language: languageExtension(state.language),
                extensions: state.language === 'json' ? [linter(jsonParseLinter())] : [],
                onChange: (content) => controller.setContent(content),
                onSave: () => void controller.save(),
              }),
        ),
        state.closePending
          ? m(
              '.fm-file-editor-close-dialog',
              { role: 'dialog', 'aria-label': t('editor', 'unsavedChanges') },
              [
                m('span', t('editor', 'saveChangesBeforeClosing')),
                m(FlatButton, {
                  label: t('editor', 'save'),
                  onclick: async () => {
                    if (await controller.save()) attrs.onClose();
                  },
                }),
                m(FlatButton, { label: t('editor', 'discard'), onclick: attrs.onClose }),
                m(FlatButton, {
                  label: t('editor', 'cancel'),
                  onclick: () => controller.cancelClose(),
                }),
              ],
            )
          : undefined,
      ],
    );
  },
};
