/**
 * The notes dialogs (task 0337): folder setup with the semantic consent step inline, the wait
 * while a session is turned into a note, and the review of a draft or proposal before it is saved.
 */
import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import './notes.css';
import type { ConnectionClient } from '../../api/client/connection-client';
import type { SemanticClient } from '../../api/client/semantic-client';
import { t } from '../../i18n';
import { safeMarkdownHtml } from '../editor/markdown-preview';
import { SemanticFolderEnrolmentPrompt } from '../settings/semantic-library-management';
import type { NotesController } from './notes-controller';
import { lineDiff, type NoteReview, type NotesState } from './notes-state';

export interface NotesDialogsAttrs {
  readonly state: NotesState;
  readonly controller: NotesController;
  /** Used by the inline semantic consent step. */
  readonly client: ConnectionClient & SemanticClient;
  readonly workspaceId: string | undefined;
}

function setupDescription(
  state: Extract<NotesState, { phase: 'setup' }>,
  controller: NotesController,
): m.Children {
  return m('.fm-notes-setup', [
    m('p.fm-notes-intro', t('notes', 'setupIntro')),
    m('label.fm-notes-field', [
      m('span', t('notes', 'folderLabel')),
      m('input#fm-notes-folder.browser-default', {
        type: 'text',
        value: state.folder,
        spellcheck: false,
        disabled: state.busy,
        'aria-invalid': state.error === undefined ? undefined : 'true',
        'aria-describedby': 'fm-notes-folder-help',
        oninput: (event: Event) =>
          controller.setSetupFolder((event.target as HTMLInputElement).value),
        onkeydown: (event: KeyboardEvent) => {
          if (event.key === 'Enter') {
            event.preventDefault();
            void controller.confirmSetup();
          }
        },
      }),
      m('small#fm-notes-folder-help.fm-notes-help', t('notes', 'folderHelp')),
    ]),
    state.semanticAvailable
      ? m('label.fm-notes-check', [
          m('input', {
            type: 'checkbox',
            checked: state.index,
            disabled: state.busy,
            onchange: (event: Event) =>
              controller.setSetupIndex((event.target as HTMLInputElement).checked),
          }),
          m('span', t('notes', 'indexLabel')),
          m('small.fm-notes-help', t('notes', 'indexHelp')),
        ])
      : m('p.fm-notes-help', t('notes', 'indexUnavailable')),
    state.error === undefined ? undefined : m('.fm-field-error', { role: 'alert' }, state.error),
  ]);
}

function reviewBody(review: NoteReview, controller: NotesController): m.Children {
  const field = (
    id: string,
    label: string,
    value: string,
    onInput: (value: string) => void,
    options: { readonly help?: string; readonly disabled?: boolean } = {},
  ) =>
    m('label.fm-notes-field', [
      m('span', label),
      m(`input#${id}.browser-default`, {
        type: 'text',
        value,
        spellcheck: false,
        disabled: review.busy || options.disabled === true,
        oninput: (event: Event) => onInput((event.target as HTMLInputElement).value),
      }),
      options.help === undefined ? undefined : m('small.fm-notes-help', options.help),
    ]);
  const tab = (view: NoteReview['view'], label: string) =>
    m(
      'button.fm-notes-tab',
      {
        type: 'button',
        role: 'tab',
        'aria-selected': review.view === view ? 'true' : 'false',
        onclick: () => controller.editReview({ view }),
      },
      label,
    );
  return m('.fm-notes-review', [
    review.redactions > 0
      ? m(
          '.fm-notes-warning',
          { role: 'status' },
          t('notes', 'redactions', { count: review.redactions }),
        )
      : undefined,
    m('.fm-notes-fields', [
      field('fm-note-title', t('notes', 'titleLabel'), review.title, (title) =>
        controller.editReview({ title }),
      ),
      field(
        'fm-note-tags',
        t('notes', 'tagsLabel'),
        review.tags,
        (tags) => controller.editReview({ tags }),
        { help: t('notes', 'tagsHelp') },
      ),
      field(
        'fm-note-file',
        t('notes', 'fileNameLabel'),
        review.fileName,
        (fileName) => controller.editReview({ fileName }),
        { disabled: review.replace !== undefined },
      ),
    ]),
    m('.fm-notes-tabs', { role: 'tablist' }, [
      tab('edit', t('notes', 'edit')),
      tab('preview', t('notes', 'preview')),
      review.replace === undefined ? undefined : tab('changes', t('notes', 'changes')),
    ]),
    review.view === 'edit'
      ? m('textarea.fm-notes-content.browser-default', {
          'aria-label': t('notes', 'edit'),
          value: review.content,
          spellcheck: false,
          disabled: review.busy,
          oninput: (event: Event) =>
            controller.editReview({ content: (event.target as HTMLTextAreaElement).value }),
        })
      : review.view === 'preview'
        ? m('.fm-notes-preview.fm-markdown', m.trust(safeMarkdownHtml(review.content)))
        : m(
            'pre.fm-notes-diff',
            lineDiff(review.replace?.original ?? '', review.content).map((line) =>
              m(
                `span.fm-notes-diff-line.fm-notes-diff-line--${line.kind}`,
                `${line.kind === 'added' ? '+' : line.kind === 'removed' ? '-' : ' '} ${line.text}\n`,
              ),
            ),
          ),
    review.error === undefined ? undefined : m('.fm-field-error', { role: 'alert' }, review.error),
  ]);
}

export const NotesDialogs: FactoryComponent<NotesDialogsAttrs> = () => ({
  view: ({ attrs }) => {
    const { state, controller } = attrs;
    const close = () => controller.close();
    switch (state.phase) {
      case 'closed':
        return undefined;
      case 'setup':
        return m(ModalPanel, {
          title: t('notes', 'setupTitle'),
          className: 'fm-dense-modal fm-notes-modal',
          isOpen: true,
          closeOnEsc: true,
          onToggle: (open: boolean) => {
            if (!open) close();
          },
          description: setupDescription(state, controller),
          buttons: [
            { label: t('notes', 'cancel'), onclick: close },
            {
              label: t('notes', 'setupConfirm'),
              disabled: state.busy || state.folder.trim() === '',
              onclick: () => void controller.confirmSetup(),
            },
          ],
        });
      case 'enrol':
        return m(ModalPanel, {
          title: t('notes', 'enrolTitle'),
          className: 'fm-dense-modal fm-notes-modal fm-semantic-enrolment-modal',
          isOpen: true,
          closeOnEsc: true,
          onToggle: (open: boolean) => {
            if (!open) void controller.finishEnrolment();
          },
          description: m('.fm-notes-enrol', [
            m('p.fm-notes-intro', t('notes', 'enrolIntro')),
            attrs.workspaceId === undefined
              ? undefined
              : m(SemanticFolderEnrolmentPrompt, {
                  client: attrs.client,
                  workspaceId: attrs.workspaceId,
                  location: state.root,
                  onEnrolled: () => void controller.finishEnrolment(),
                  onCancel: () => void controller.finishEnrolment(),
                }),
          ]),
        });
      case 'drafting':
        return m(ModalPanel, {
          title: t('notes', 'reviewNewTitle'),
          className: 'fm-dense-modal fm-notes-modal',
          isOpen: true,
          closeOnEsc: true,
          onToggle: (open: boolean) => {
            if (!open) close();
          },
          description: m('p.fm-notes-busy', { role: 'status' }, t('notes', 'drafting')),
          buttons: [{ label: t('notes', 'cancel'), onclick: close }],
        });
      case 'review':
        return m(ModalPanel, {
          title:
            state.replace === undefined
              ? t('notes', 'reviewNewTitle')
              : t('notes', 'reviewUpdateTitle'),
          className: 'fm-dense-modal fm-notes-modal fm-notes-review-modal',
          isOpen: true,
          closeOnEsc: true,
          onToggle: (open: boolean) => {
            if (!open) close();
          },
          description: reviewBody(state, controller),
          buttons: [
            { label: t('notes', 'cancel'), onclick: close },
            {
              label: t('notes', 'save'),
              disabled: state.busy,
              onclick: () => void controller.saveReview(),
            },
          ],
        });
    }
  },
});
