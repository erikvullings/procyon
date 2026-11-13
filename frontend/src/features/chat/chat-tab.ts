import m, { type FactoryComponent, type VnodeDOM } from 'mithril';
import { MaterialBox } from 'mithril-materialized';
import {
  checkIcon,
  chevronDownIcon,
  chevronRightIcon,
  closeIcon,
  copyIcon,
  cornerDownLeftIcon,
  heartIcon,
  messageCircleIcon,
  pencilIcon,
  playerStopIcon,
  plusIcon,
  refreshIcon,
  trashIcon,
} from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type { ChatConversationSummary } from '../../models';
import { safeMarkdownHtml } from '../editor/markdown-preview';
import type { PastedImage } from './chat-image';
import {
  type ChatAttachment,
  type ChatTabEntry,
  canSendMessage,
  expandPastes,
  failureNeedsSettings,
  isLongPaste,
  type PastedText,
  pastePlaceholder,
} from './chat-tab-state';
import './chat-tab.css';

/** What the other pane has selected; the attach commands follow it (DESIGN.md "Status bar"). */
export interface ChatFolderSelection {
  readonly selected: number;
  readonly selectedFiles: number;
  /** The one selected file is over the limit for attaching its contents. */
  readonly fileTooLarge: boolean;
}

export interface ChatTabAttrs {
  readonly entry: ChatTabEntry;
  /** Whether the other pane shows a folder whose listing can be attached. */
  readonly folderAvailable: boolean;
  readonly folderSelection: ChatFolderSelection;
  readonly onNew: () => void;
  /** Opens a new tab in the pane; the header offers it after the new-chat control. */
  readonly onNewTab?: () => void;
  readonly onSelect: (conversationId: string) => void;
  readonly onRename: (conversationId: string, title: string) => void;
  readonly onDelete: (conversationId: string) => void;
  readonly onSend: (draft: string) => void;
  readonly onStop: () => void;
  readonly onRetry: () => void;
  /** Returns the listing as text for the draft; the chat inserts it at the caret. */
  readonly onAttachFolder: () => ChatAttachment | undefined;
  /** Returns the selection as text for the draft; the chat inserts it at the caret. */
  readonly onAttachSelection: () => ChatAttachment | undefined;
  /** Attaches the selected file's contents as a pill; at most one per message. */
  readonly onAttachFileContents: () => void;
  readonly onDetach: (attachmentId: string) => void;
  /** Images pasted for the next message. */
  readonly images: readonly PastedImage[];
  readonly onPasteImages: (files: Blob[]) => void;
  readonly onRemoveImage: (imageId: string) => void;
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
  let textarea: HTMLTextAreaElement | undefined;
  /** Where the caret goes once a redraw has put the inserted text in the field. */
  let caretAfterRedraw: number | undefined;
  /** Long pastes held behind placeholders in the draft; they are restored when the message is sent. */
  let pastes: PastedText[] = [];
  let pasteCount = 0;

  /** Copies a message to the clipboard; a refused copy is not reported. */
  const copyText = (text: string): void => {
    void navigator.clipboard?.writeText(text).catch(() => undefined);
  };

  /** A model reply, rendered as Markdown and sanitized; the copy button still copies the source. */
  const reply = (text: string) => m('.fm-chat-markdown', m.trust(safeMarkdownHtml(text)));

  const lastIsPersons = (entry: ChatTabEntry): boolean =>
    entry.messages[entry.messages.length - 1]?.role === 'user';

  const submit = (attrs: ChatTabAttrs): void => {
    if (!canSendMessage(attrs.entry, draft)) return;
    attrs.onSend(expandPastes(draft, pastes));
    draft = '';
    pastes = [];
  };

  /** Inserts text where the caret is, replacing any selected text in the draft. */
  const insertAtCaret = (text: string): void => {
    const start = textarea?.selectionStart ?? draft.length;
    const end = textarea?.selectionEnd ?? start;
    draft = draft.slice(0, start) + text + draft.slice(end);
    caretAfterRedraw = start + text.length;
    textarea?.focus();
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
      iconButton(t('chat', 'rename'), pencilIcon({ size: 14 }), () => {
        renaming = { id: summary.id, title: summary.title };
      }),
      confirmingDelete === summary.id
        ? iconButton(t('chat', 'confirmDelete'), checkIcon({ size: 14 }), () => {
            confirmingDelete = undefined;
            attrs.onDelete(summary.id);
          })
        : iconButton(t('chat', 'delete'), trashIcon({ size: 14 }), () => {
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
   * folder; the selection needs at least one entry; the file contents need exactly one file that is
   * small enough, and only once per message.
   */
  const attachCommands = (attrs: ChatTabAttrs) => {
    const pending = attrs.entry.pendingRequestId !== undefined;
    const { selected, selectedFiles, fileTooLarge } = attrs.folderSelection;
    const fileAttached = attrs.entry.attachments.some((attachment) => attachment.kind === 'file');
    return [
      chromeButton(
        t('chat', 'attachFolder'),
        () => {
          const attachment = attrs.onAttachFolder();
          if (attachment !== undefined) insertAtCaret(attachment.text);
        },
        { disabled: pending || !attrs.folderAvailable },
      ),
      chromeButton(
        t('chat', 'attachSelection'),
        () => {
          const attachment = attrs.onAttachSelection();
          if (attachment !== undefined) insertAtCaret(attachment.text);
        },
        { disabled: pending || selected === 0 },
      ),
      chromeButton(t('chat', 'attachFileContents'), () => attrs.onAttachFileContents(), {
        disabled: pending || selectedFiles !== 1 || fileTooLarge || fileAttached,
      }),
    ];
  };

  return {
    view: ({ attrs }) => {
      const { entry } = attrs;
      const pending = entry.pendingRequestId !== undefined;
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
          // The pane's trailing actions, in the same order as the breadcrumb bar: chat, new tab, favourites.
          iconButton(
            t('chat', 'newChat'),
            messageCircleIcon({ size: 16 }),
            () => attrs.onNew(),
            pending,
          ),
          attrs.onNewTab === undefined
            ? undefined
            : iconButton(t('pane', 'newTab'), plusIcon({ size: 16 }), () => attrs.onNewTab?.()),
          iconButton(t('pane', 'favourites'), heartIcon({ size: 16 }), () => undefined),
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
              entry.messages.map((message) =>
                m(`.fm-chat-message.fm-chat-message-${message.role}`, [
                  m('.fm-chat-message-head', [
                    m(
                      'span.fm-chat-author',
                      t('chat', message.role === 'user' ? 'you' : 'assistant'),
                    ),
                    iconButton(t('chat', 'copy'), copyIcon({ size: 14 }), () =>
                      copyText(message.content),
                    ),
                  ]),
                  message.role === 'assistant'
                    ? reply(message.content)
                    : m('pre.fm-chat-text', message.content),
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
                    reply(entry.streamedText),
                  ])
                : undefined,
              // A reply cut short by a failure stays visible, so nothing that streamed is lost.
              !pending && entry.partialReply
                ? m('.fm-chat-message.fm-chat-message-assistant.fm-chat-partial', [
                    m('span.fm-chat-author', t('chat', 'assistant')),
                    reply(entry.partialReply),
                  ])
                : undefined,
              pending && !entry.streamedText
                ? m('p.fm-chat-pending', t('chat', 'waitingForModel'))
                : undefined,
            ),
            entry.error
              ? m('.fm-chat-error', { role: 'alert' }, [
                  m('span', entry.error),
                  !pending && lastIsPersons(entry)
                    ? m(
                        'button.btn-flat.fm-chat-chrome.fm-chat-retry',
                        { type: 'button', onclick: () => attrs.onRetry() },
                        [refreshIcon({ size: 14 }), m('span', t('chat', 'retry'))],
                      )
                    : undefined,
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
            // One box holds the attached pills above the text field, so the context travels with the
            // question it belongs to.
            m(
              'form.fm-chat-composer',
              {
                onsubmit: (event: Event) => {
                  event.preventDefault();
                  submit(attrs);
                },
              },
              [
                entry.attachments.length === 0 && attrs.images.length === 0
                  ? undefined
                  : m('.fm-chat-pills', { role: 'list', 'aria-label': t('chat', 'willSend') }, [
                      ...attrs.images.map((image) =>
                        m('.fm-chat-pill.fm-chat-pill-image', { role: 'listitem' }, [
                          // The thumbnail opens the full image in MaterialBox.
                          m(MaterialBox, {
                            src: image.previewUrl,
                            alt: t('chat', 'imageAlt'),
                            width: 20,
                            height: 20,
                          }),
                          m('span.fm-chat-pill-label', t('chat', 'imageLabel')),
                          iconButton(
                            t('chat', 'remove'),
                            closeIcon({ size: 12 }),
                            () => attrs.onRemoveImage(image.id),
                            pending,
                          ),
                        ]),
                      ),
                      ...entry.attachments.map((attachment) =>
                        m('.fm-chat-pill', { role: 'listitem' }, [
                          m('span.fm-chat-pill-label', attachment.label),
                          iconButton(
                            t('chat', 'remove'),
                            closeIcon({ size: 12 }),
                            () => attrs.onDetach(attachment.id),
                            pending,
                          ),
                        ]),
                      ),
                    ]),
                m('.fm-chat-composer-row', [
                  m('textarea.fm-chat-input', {
                    'aria-label': t('chat', 'placeholder'),
                    placeholder: t('chat', 'placeholder'),
                    value: draft,
                    disabled: pending || entry.conversationId === undefined,
                    oncreate: ({ dom }: VnodeDOM) => {
                      textarea = dom as HTMLTextAreaElement;
                    },
                    onupdate: ({ dom }: VnodeDOM) => {
                      if (caretAfterRedraw === undefined) return;
                      const caret = caretAfterRedraw;
                      caretAfterRedraw = undefined;
                      (dom as HTMLTextAreaElement).setSelectionRange(caret, caret);
                    },
                    oninput: (event: Event) => {
                      draft = (event.target as HTMLTextAreaElement).value;
                    },
                    onkeydown: (event: KeyboardEvent) => {
                      if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
                        event.preventDefault();
                        submit(attrs);
                      }
                    },
                    // Short pastes go in as typed. A long one shows a placeholder; its text is sent
                    // in place of the placeholder. Pasted images and other data keep the default.
                    onpaste: (event: ClipboardEvent) => {
                      // Pasted images go to the next message, when the chat endpoint accepts them.
                      const images = Array.from(event.clipboardData?.files ?? []).filter((file) =>
                        file.type.startsWith('image/'),
                      );
                      if (images.length > 0) {
                        event.preventDefault();
                        attrs.onPasteImages(images);
                        return;
                      }
                      const text = event.clipboardData?.getData('text/plain') ?? '';
                      if (text.trim() === '' || !isLongPaste(text)) return;
                      event.preventDefault();
                      pasteCount += 1;
                      const placeholder = pastePlaceholder(text, pasteCount);
                      pastes = [...pastes, { placeholder, text }];
                      insertAtCaret(placeholder);
                    },
                  }),
                  // Trailing controls: the send key is the Enter glyph; a microphone joins here later.
                  m('.fm-chat-composer-actions', [
                    pending
                      ? tooltip(
                          t('chat', 'stop'),
                          m(
                            'button.btn-flat.fm-chat-chrome.fm-chat-chrome-icon.fm-chat-stop',
                            {
                              type: 'button',
                              'aria-label': t('chat', 'stop'),
                              onclick: () => attrs.onStop(),
                            },
                            playerStopIcon({ size: 14 }),
                          ),
                        )
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
