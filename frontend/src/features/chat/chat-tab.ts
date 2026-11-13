import m, { type FactoryComponent } from 'mithril';
import {
  chevronDownIcon,
  chevronRightIcon,
  cornerDownLeftIcon,
  messageCircleIcon,
} from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type { ChatConversationSummary } from '../../models';
import {
  type ChatTabEntry,
  canSendMessage,
  contextForMessage,
  failureNeedsSettings,
} from './chat-tab-state';
import './chat-tab.css';

/** What the other pane has selected; the attach commands follow it (DESIGN.md "Status bar"). */
export interface ChatFolderSelection {
  readonly selected: number;
  readonly selectedFiles: number;
}

export interface ChatTabAttrs {
  readonly entry: ChatTabEntry;
  /** Whether the other pane shows a folder whose listing can be attached. */
  readonly folderAvailable: boolean;
  readonly folderSelection: ChatFolderSelection;
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
  /** The conversation list is folded away until the person opens it. */
  let listOpen = false;
  let renaming: { readonly id: string; title: string } | undefined;
  let confirmingDelete: string | undefined;

  const submit = (attrs: ChatTabAttrs): void => {
    if (!canSendMessage(attrs.entry, draft)) return;
    attrs.onSend(draft);
    draft = '';
  };

  /** A chrome-density flat button (DESIGN.md "button-chrome": 20px, flat). */
  const chromeButton = (
    label: string,
    onclick: () => void,
    options: { readonly type?: 'button' | 'submit'; readonly disabled?: boolean } = {},
  ) =>
    m(
      'button.btn-flat.fm-chat-chrome',
      { type: options.type ?? 'button', disabled: options.disabled ?? false, onclick },
      label,
    );

  /** An icon-only chrome button; the tooltip gives its name, as DESIGN.md requires. */
  const iconButton = (label: string, icon: m.Children, onclick: () => void, disabled = false) =>
    tooltip(
      label,
      m(
        'button.btn-flat.fm-chat-chrome.fm-chat-chrome-icon',
        { type: 'button', 'aria-label': label, disabled, onclick },
        icon,
      ),
    );

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
        chromeButton(t('chat', 'save'), () => undefined, { type: 'submit' }),
        chromeButton(t('chat', 'cancel'), () => {
          renaming = undefined;
        }),
      ],
    );

  /** Rename and delete appear only on the open conversation, so each row stays one line. */
  const rowActions = (attrs: ChatTabAttrs, summary: ChatConversationSummary) =>
    m('.fm-chat-conversation-actions', [
      chromeButton(t('chat', 'rename'), () => {
        renaming = { id: summary.id, title: summary.title };
      }),
      confirmingDelete === summary.id
        ? chromeButton(t('chat', 'confirmDelete'), () => {
            confirmingDelete = undefined;
            attrs.onDelete(summary.id);
          })
        : chromeButton(t('chat', 'delete'), () => {
            confirmingDelete = summary.id;
          }),
    ]);

  const conversationItem = (attrs: ChatTabAttrs, summary: ChatConversationSummary) => {
    if (renaming?.id === summary.id)
      return m('li.fm-chat-conversation', renameForm(attrs, renaming));
    const selected = summary.id === attrs.entry.conversationId;
    return m(
      'li.fm-chat-conversation',
      { class: selected ? 'fm-chat-conversation-selected' : undefined },
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
        selected ? rowActions(attrs, summary) : undefined,
      ],
    );
  };

  const currentTitle = (entry: ChatTabEntry): string | undefined => {
    const summary = entry.conversations.find((item) => item.id === entry.conversationId);
    if (summary === undefined) return undefined;
    return summary.title === '' ? t('chat', 'untitled') : summary.title;
  };

  /**
   * The attach commands sit in the status bar and follow the other pane: the listing needs a
   * folder; the selection needs at least one entry; the file contents need exactly one file.
   */
  const attachCommands = (attrs: ChatTabAttrs) => {
    const pending = attrs.entry.pendingRequestId !== undefined;
    const { selected, selectedFiles } = attrs.folderSelection;
    return [
      chromeButton(t('chat', 'attachFolder'), () => attrs.onAttachFolder(), {
        disabled: pending || !attrs.folderAvailable,
      }),
      chromeButton(t('chat', 'attachSelection'), () => attrs.onAttachSelection(), {
        disabled: pending || selected === 0,
      }),
      chromeButton(t('chat', 'attachFileContents'), () => attrs.onAttachFileContents(), {
        disabled: pending || selectedFiles !== 1,
      }),
    ];
  };

  return {
    view: ({ attrs }) => {
      const { entry } = attrs;
      const pending = entry.pendingRequestId !== undefined;
      const preview = contextForMessage(entry.attachments);
      const current = currentTitle(entry);
      return m('section.fm-chat', [
        m('header.fm-chat-header', [
          m('span.fm-chat-title', t('chat', 'title')),
          m(
            'button.btn-flat.fm-chat-chrome.fm-chat-toggle',
            {
              type: 'button',
              'aria-expanded': String(listOpen),
              'aria-controls': listOpen ? 'fm-chat-list' : undefined,
              onclick: () => {
                listOpen = !listOpen;
              },
            },
            [
              m(
                'span.fm-chat-chevron',
                { 'aria-hidden': 'true' },
                listOpen ? chevronDownIcon({ size: 12 }) : chevronRightIcon({ size: 12 }),
              ),
              t('chat', 'conversations'),
            ],
          ),
          !listOpen && current !== undefined
            ? m('span.fm-chat-current', { title: current }, current)
            : undefined,
          iconButton(
            t('chat', 'newChat'),
            messageCircleIcon({ size: 14 }),
            () => attrs.onNew(),
            pending,
          ),
        ]),
        m('.fm-chat-body', [
          listOpen
            ? m(
                'section.fm-chat-list',
                {
                  id: 'fm-chat-list',
                  role: 'navigation',
                  'aria-label': t('chat', 'conversations'),
                },
                m(
                  'ul',
                  entry.conversations.map((summary) => conversationItem(attrs, summary)),
                ),
              )
            : undefined,
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
                    ? m(
                        'button.btn-flat.fm-chat-chrome',
                        {
                          type: 'button',
                          onclick: () => attrs.onOpenSettings?.(),
                        },
                        t('chat', 'openSettings'),
                      )
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
                        chromeButton(t('chat', 'remove'), () => attrs.onDetach(attachment.id), {
                          disabled: pending,
                        }),
                      ]),
                    ),
                  ),
                  m('pre.fm-chat-preview', preview),
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
                // Trailing controls: the send key is the Enter glyph; a microphone joins here later.
                m('.fm-chat-composer-actions', [
                  pending
                    ? chromeButton(t('chat', 'stop'), () => attrs.onStop())
                    : tooltip(
                        t('chat', 'send'),
                        m(
                          'button.btn-flat.fm-chat-chrome.fm-chat-chrome-icon.fm-chat-send',
                          {
                            type: 'submit',
                            'aria-label': t('chat', 'send'),
                            disabled: !canSendMessage(entry, draft),
                          },
                          cornerDownLeftIcon({ size: 14 }),
                        ),
                      ),
                ]),
              ],
            ),
          ]),
        ]),
        m('footer.fm-chat-status', attachCommands(attrs)),
      ]);
    },
  };
};
