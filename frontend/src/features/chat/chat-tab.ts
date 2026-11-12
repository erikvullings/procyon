import m, { type FactoryComponent } from 'mithril';
import { FlatButton } from 'mithril-materialized';
import { t } from '../../i18n';
import type { ChatConversationSummary } from '../../models';
import {
  type ChatTabEntry,
  canSendMessage,
  contextForMessage,
  failureNeedsSettings,
} from './chat-tab-state';
import './chat-tab.css';

export interface ChatTabAttrs {
  readonly entry: ChatTabEntry;
  /** Whether the other pane shows a folder whose listing or selection can be attached. */
  readonly folderAvailable: boolean;
  readonly onNew: () => void;
  readonly onSelect: (conversationId: string) => void;
  readonly onRename: (conversationId: string, title: string) => void;
  readonly onDelete: (conversationId: string) => void;
  readonly onSend: (draft: string) => void;
  readonly onStop: () => void;
  readonly onAttachFolder: () => void;
  readonly onAttachSelection: () => void;
  readonly onAttachFileContents: () => void;
  readonly onDetach: (attachmentId: string) => void;
  /** Opens the AI settings, where consent and roles are set; absent when Settings is unavailable. */
  readonly onOpenSettings?: () => void;
}

/**
 * The chat shown in a pane (task 0324): a list of saved conversations, the open conversation, and
 * the read-only context the person attaches. The draft and the rename and delete steps are local UI
 * state only.
 */
export const ChatTab: FactoryComponent<ChatTabAttrs> = () => {
  let draft = '';
  let renaming: { readonly id: string; title: string } | undefined;
  let confirmingDelete: string | undefined;

  const submit = (attrs: ChatTabAttrs): void => {
    if (!canSendMessage(attrs.entry, draft)) return;
    attrs.onSend(draft);
    draft = '';
  };

  const renameForm = (attrs: ChatTabAttrs, current: { id: string; title: string }) =>
    m(
      'form.fm-chat-rename',
      {
        onsubmit: (event: Event) => {
          event.preventDefault();
          attrs.onRename(current.id, current.title);
          renaming = undefined;
        },
      },
      [
        m('input.fm-chat-rename-input', {
          'aria-label': t('chat', 'rename'),
          value: current.title,
          oninput: (event: Event) => {
            current.title = (event.target as HTMLInputElement).value;
          },
        }),
        m(FlatButton, { label: t('chat', 'save'), type: 'submit' }),
        m(FlatButton, {
          label: t('chat', 'cancel'),
          onclick: () => {
            renaming = undefined;
          },
        }),
      ],
    );

  const conversationItem = (attrs: ChatTabAttrs, summary: ChatConversationSummary) => {
    if (renaming?.id === summary.id)
      return m('li.fm-chat-conversation', renameForm(attrs, renaming));
    const selected = summary.id === attrs.entry.conversationId;
    return m(
      'li.fm-chat-conversation',
      { class: selected ? 'fm-chat-conversation-selected' : '' },
      [
        m(
          'button.fm-chat-conversation-open',
          {
            type: 'button',
            'aria-current': selected ? 'true' : undefined,
            onclick: () => attrs.onSelect(summary.id),
          },
          summary.title === '' ? t('chat', 'untitled') : summary.title,
        ),
        m(FlatButton, {
          label: t('chat', 'rename'),
          onclick: () => {
            renaming = { id: summary.id, title: summary.title };
          },
        }),
        confirmingDelete === summary.id
          ? m(FlatButton, {
              label: t('chat', 'confirmDelete'),
              onclick: () => {
                confirmingDelete = undefined;
                attrs.onDelete(summary.id);
              },
            })
          : m(FlatButton, {
              label: t('chat', 'delete'),
              onclick: () => {
                confirmingDelete = summary.id;
              },
            }),
      ],
    );
  };

  return {
    view: ({ attrs }) => {
      const { entry } = attrs;
      const pending = entry.pendingRequestId !== undefined;
      const preview = contextForMessage(entry.attachments);
      return m('section.fm-chat', [
        m('header.fm-chat-header', [
          m('span.fm-chat-title', t('chat', 'title')),
          m(FlatButton, {
            label: t('chat', 'newChat'),
            disabled: pending,
            onclick: () => attrs.onNew(),
          }),
        ]),
        m('.fm-chat-body', [
          m(
            'nav.fm-chat-list',
            { 'aria-label': t('chat', 'conversations') },
            m(
              'ul',
              entry.conversations.map((summary) => conversationItem(attrs, summary)),
            ),
          ),
          m('.fm-chat-main', [
            m(
              '.fm-chat-messages',
              { role: 'log', 'aria-live': 'polite' },
              entry.messages.length === 0 && entry.streamedText === undefined
                ? m('p.fm-chat-empty', t('chat', 'intro'))
                : entry.messages.map((message) =>
                    m(`.fm-chat-message.fm-chat-message-${message.role}`, [
                      m(
                        'span.fm-chat-author',
                        t('chat', message.role === 'user' ? 'you' : 'assistant'),
                      ),
                      m('pre.fm-chat-text', message.content),
                      message.context
                        ? m('details.fm-chat-context', [
                            m('summary', t('chat', 'attachedContext')),
                            m('pre.fm-chat-preview', message.context),
                          ])
                        : undefined,
                    ]),
                  ),
              entry.streamedText
                ? m('.fm-chat-message.fm-chat-message-assistant.fm-chat-streaming', [
                    m('span.fm-chat-author', t('chat', 'assistant')),
                    m('pre.fm-chat-text', entry.streamedText),
                  ])
                : undefined,
              pending && !entry.streamedText
                ? m('p.fm-chat-pending', t('chat', 'thinking'))
                : undefined,
            ),
            entry.error
              ? m('.fm-chat-error', { role: 'alert' }, [
                  m('span', entry.error),
                  failureNeedsSettings(entry.failure) && attrs.onOpenSettings
                    ? m(FlatButton, {
                        label: t('chat', 'openSettings'),
                        onclick: () => attrs.onOpenSettings?.(),
                      })
                    : undefined,
                ])
              : undefined,
            preview === null
              ? undefined
              : m('.fm-chat-attachments', [
                  m('p.fm-chat-attachments-title', t('chat', 'willSend')),
                  m(
                    'ul',
                    entry.attachments.map((attachment) =>
                      m('li', [
                        m('span', attachment.label),
                        m(FlatButton, {
                          label: t('chat', 'remove'),
                          disabled: pending,
                          onclick: () => attrs.onDetach(attachment.id),
                        }),
                      ]),
                    ),
                  ),
                  m('pre.fm-chat-preview', preview),
                ]),
            m('.fm-chat-attach-actions', [
              m(FlatButton, {
                label: t('chat', 'attachFolder'),
                disabled: pending || !attrs.folderAvailable,
                onclick: () => attrs.onAttachFolder(),
              }),
              m(FlatButton, {
                label: t('chat', 'attachSelection'),
                disabled: pending || !attrs.folderAvailable,
                onclick: () => attrs.onAttachSelection(),
              }),
              m(FlatButton, {
                label: t('chat', 'attachFileContents'),
                disabled: pending || !attrs.folderAvailable,
                onclick: () => attrs.onAttachFileContents(),
              }),
            ]),
            m(
              'form.fm-chat-composer',
              {
                onsubmit: (event: Event) => {
                  event.preventDefault();
                  submit(attrs);
                },
              },
              [
                m('textarea.fm-chat-input', {
                  'aria-label': t('chat', 'placeholder'),
                  placeholder: t('chat', 'placeholder'),
                  value: draft,
                  disabled: pending || entry.conversationId === undefined,
                  oninput: (event: Event) => {
                    draft = (event.target as HTMLTextAreaElement).value;
                  },
                  onkeydown: (event: KeyboardEvent) => {
                    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
                      event.preventDefault();
                      submit(attrs);
                    }
                  },
                }),
                pending
                  ? m(FlatButton, { label: t('chat', 'stop'), onclick: () => attrs.onStop() })
                  : m(FlatButton, {
                      label: t('chat', 'send'),
                      type: 'submit',
                      disabled: !canSendMessage(entry, draft),
                    }),
              ],
            ),
          ]),
        ]),
      ]);
    },
  };
};
