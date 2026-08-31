import { linter } from '@codemirror/lint';
import m, { type Component, type FactoryComponent } from 'mithril';
import { FlatButton, IconButton, ModalPanel } from 'mithril-materialized';
import { closeIcon } from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type { PaneId } from '../../models';
import { CodeMirrorEditor } from './code-mirror-editor';
import { jsonParseLinter, languageExtension } from './editor-language';
import type { FileEditorController, FileEditorState } from './file-editor-controller';
import './file-editor.css';

export interface FileEditorAttrs {
  readonly state: FileEditorState;
  readonly controller: FileEditorController;
  readonly paneId?: PaneId;
  readonly onClose: () => void;
}

interface UnsavedChangesDialogAttrs {
  readonly open: boolean;
  readonly saving: boolean;
  readonly onSave: () => void;
  readonly onDiscard: () => void;
  readonly onCancel: () => void;
}

type CloseChoice = 'save' | 'discard' | 'cancel';

const UnsavedChangesDialog: FactoryComponent<UnsavedChangesDialogAttrs> = () => {
  let selected: CloseChoice = 'save';
  let wasOpen = false;
  let keydownHandler: ((event: KeyboardEvent) => void) | undefined;

  const removeFocusTrap = () => {
    if (keydownHandler !== undefined) document.removeEventListener('keydown', keydownHandler);
    keydownHandler = undefined;
  };

  const updateFocusTrap = (dom: Element, open: boolean) => {
    removeFocusTrap();
    const dialog = dom.closest('[role="dialog"]');
    if (!open || dialog === null) {
      wasOpen = false;
      return;
    }
    const choices: readonly CloseChoice[] = ['save', 'discard', 'cancel'];
    const buttons = choices
      .map((choice) => dialog.querySelector<HTMLButtonElement>(`.fm-file-editor-close-${choice}`))
      .filter((button): button is HTMLButtonElement => button !== null && !button.disabled);
    if (!wasOpen) {
      selected = 'save';
      buttons[0]?.focus();
    }
    wasOpen = true;
    keydownHandler = (event: KeyboardEvent) => {
      if (event.key !== 'Tab' || buttons.length === 0) return;
      const currentIndex = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const nextIndex = event.shiftKey
        ? (currentIndex <= 0 ? buttons.length : currentIndex) - 1
        : (currentIndex + 1) % buttons.length;
      event.preventDefault();
      const next = buttons[nextIndex];
      next?.focus();
      selected =
        choices.find((choice) => next?.classList.contains(`fm-file-editor-close-${choice}`)) ??
        'save';
      m.redraw();
    };
    document.addEventListener('keydown', keydownHandler);
  };

  const choiceClass = (choice: CloseChoice) =>
    `fm-file-editor-close-choice fm-file-editor-close-${choice}${selected === choice ? ' is-selected' : ''}`;

  return {
    view: ({ attrs }) =>
      m(ModalPanel, {
        className: 'fm-file-editor-close-modal',
        title: t('editor', 'unsavedChanges'),
        description: m(
          'p',
          {
            oncreate: ({ dom }) => updateFocusTrap(dom, attrs.open),
            onupdate: ({ dom }) => updateFocusTrap(dom, attrs.open),
            onremove: removeFocusTrap,
          },
          t('editor', 'saveChangesBeforeClosing'),
        ),
        isOpen: attrs.open,
        showCloseButton: false,
        closeOnBackdropClick: false,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) attrs.onCancel();
        },
        buttons: [
          {
            label: t('editor', 'yes'),
            className: choiceClass('save'),
            disabled: attrs.saving,
            onclick: attrs.onSave,
          },
          {
            label: t('editor', 'no'),
            className: choiceClass('discard'),
            onclick: attrs.onDiscard,
          },
          {
            label: t('editor', 'cancel'),
            className: choiceClass('cancel'),
            onclick: attrs.onCancel,
          },
        ],
      }),
  };
};

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
    const requestClose = () => {
      if (controller.requestClose()) attrs.onClose();
    };
    return m(
      'section.fm-file-editor',
      {
        'aria-label': t('editor', 'editing', { name: state.entry.name }),
        'data-editor-pane-id': attrs.paneId,
        onkeydown: (event: KeyboardEvent) => {
          if (
            (event.metaKey || event.ctrlKey) &&
            !event.altKey &&
            !event.shiftKey &&
            event.key.toLowerCase() === 'w'
          ) {
            event.preventDefault();
            event.stopPropagation();
            requestClose();
          }
        },
      },
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
                onclick: requestClose,
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
        m(UnsavedChangesDialog, {
          open: state.closePending,
          saving: state.saving,
          onSave: () => {
            void controller.save().then((saved) => {
              if (saved) attrs.onClose();
            });
          },
          onDiscard: attrs.onClose,
          onCancel: () => controller.cancelClose(),
        }),
      ],
    );
  },
};
