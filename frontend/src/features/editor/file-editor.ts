import { linter } from '@codemirror/lint';
import m, { type Component, type FactoryComponent } from 'mithril';
import { FlatButton, IconButton, ModalPanel } from 'mithril-materialized';
import { closeIcon } from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type { PaneId } from '../../models';
import { looksLikeGeoJson } from '../geo/geo-document';
import { GeoMapEditor, type GeoMapEditorAttrs } from '../geo-editor/geo-map-editor';
import { jsonParseLinter, languageExtension } from './editor-language';
import type { FileEditorController, FileEditorState } from './file-editor-controller';
import type { EditorLinkHandlers } from './link-actions';
import { LinkAwareEditor } from './link-aware-editor';
import './file-editor.css';

export interface FileEditorAttrs {
  readonly state: FileEditorState;
  readonly controller: FileEditorController;
  readonly paneId?: PaneId;
  readonly onClose: () => void;
  /** Actions for paths and URLs in the text (task 0271); absent where none are available. */
  readonly links?: EditorLinkHandlers;
  /** Client for the GeoJSON map editor (task 0278); the map toggle is hidden without it. */
  readonly mapClient?: GeoMapEditorAttrs['client'];
  /** Writes an exported copy of the GeoJSON next to the file (task 0281). */
  readonly onExportGeo?: GeoMapEditorAttrs['onExport'];
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
    };
    document.addEventListener('keydown', keydownHandler);
  };

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
            className: 'fm-file-editor-close-choice fm-file-editor-close-save',
            disabled: attrs.saving,
            onclick: attrs.onSave,
          },
          {
            label: t('editor', 'no'),
            className: 'fm-file-editor-close-choice fm-file-editor-close-discard',
            onclick: attrs.onDiscard,
          },
          {
            label: t('editor', 'cancel'),
            className: 'fm-file-editor-close-choice fm-file-editor-close-cancel',
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
    const geoCapable =
      attrs.mapClient !== undefined &&
      state.language === 'json' &&
      (state.entry.name.toLowerCase().endsWith('.geojson') ||
        looksLikeGeoJson(state.content.slice(0, 4096)));
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
          geoCapable
            ? m(FlatButton, {
                label: state.mapMode === true ? t('geoEdit', 'textMode') : t('geoEdit', 'mapMode'),
                'aria-pressed': state.mapMode === true,
                onclick: () => controller.toggleMapMode(),
              })
            : undefined,
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
          state.mapMode === true && attrs.mapClient !== undefined
            ? m(GeoMapEditor, {
                client: attrs.mapClient,
                content: state.content,
                fileName: state.entry.name,
                ...(attrs.onExportGeo === undefined ? {} : { onExport: attrs.onExportGeo }),
                onChange: (content) => controller.setContent(content),
              })
            : state.language === 'markdown' && state.previewVisible
              ? m('.fm-file-editor-preview', { innerHTML: state.previewHtml ?? '' })
              : m(LinkAwareEditor, {
                  content: state.content,
                  language: languageExtension(state.language),
                  extensions: state.language === 'json' ? [linter(jsonParseLinter())] : [],
                  onChange: (content) => controller.setContent(content),
                  onSave: () => void controller.save(),
                  onCursorChange: (offset) => controller.setCursor(offset),
                  ...(state.initialSelection === undefined
                    ? {}
                    : { selection: state.initialSelection }),
                  ...(attrs.links === undefined ? {} : { links: attrs.links }),
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
