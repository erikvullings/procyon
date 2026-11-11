import m, { type FactoryComponent } from 'mithril';
import { FlatButton } from 'mithril-materialized';
import { t } from '../../i18n';
import { type AssistantTabEntry, canSendDraft } from './assistant-tab-state';
import './assistant-tab.css';

export interface AssistantTabAttrs {
  readonly entry: AssistantTabEntry;
  /** Whether the editor the reply would go into is still open. */
  readonly sourceOpen: boolean;
  readonly onSend: (draft: string) => void;
  readonly onStop: () => void;
  readonly onProfile: (profileId: string) => void;
  readonly onInsert: (index: number) => void;
}

/** The file-assistant chat shown in a pane (task 0321). The draft is local UI state only. */
export const AssistantTab: FactoryComponent<AssistantTabAttrs> = () => {
  let draft = '';

  const submit = (attrs: AssistantTabAttrs): void => {
    if (!canSendDraft(attrs.entry, draft)) return;
    attrs.onSend(draft);
    draft = '';
  };

  return {
    view: ({ attrs }) => {
      const { entry } = attrs;
      const pending = entry.pendingRequestId !== undefined;
      return m('section.fm-assistant', [
        m('header.fm-assistant-header', [
          m('span.fm-assistant-title', t('assistant', 'title')),
          m('span.fm-assistant-file', `${t('assistant', 'file')}: ${entry.fileName}`),
          m('label.fm-assistant-profile', [
            t('assistant', 'profile'),
            m(
              'select',
              {
                value: entry.profileId,
                disabled: pending,
                onchange: (event: Event) =>
                  attrs.onProfile((event.target as HTMLSelectElement).value),
              },
              entry.profiles.map((profile) =>
                m(
                  'option',
                  { value: profile.id, selected: profile.id === entry.profileId },
                  profile.name,
                ),
              ),
            ),
          ]),
        ]),
        m(
          '.fm-assistant-messages',
          { role: 'log', 'aria-live': 'polite' },
          entry.messages.length === 0
            ? m('p.fm-assistant-empty', t('assistant', 'empty'))
            : entry.messages.map((message, index) =>
                m(`.fm-assistant-message.fm-assistant-message-${message.role}`, [
                  m(
                    'span.fm-assistant-author',
                    t('assistant', message.role === 'user' ? 'you' : 'assistant'),
                  ),
                  m('pre.fm-assistant-text', message.content),
                  message.role === 'assistant'
                    ? m(FlatButton, {
                        label: t('assistant', 'insert'),
                        disabled: !attrs.sourceOpen,
                        onclick: () => attrs.onInsert(index),
                      })
                    : undefined,
                ]),
              ),
          pending ? m('p.fm-assistant-pending', t('assistant', 'thinking')) : undefined,
        ),
        entry.error ? m('.fm-assistant-error', { role: 'alert' }, entry.error) : undefined,
        m(
          'form.fm-assistant-composer',
          {
            onsubmit: (event: Event) => {
              event.preventDefault();
              submit(attrs);
            },
          },
          [
            m('textarea.fm-assistant-input', {
              'aria-label': t('assistant', 'placeholder'),
              placeholder: t('assistant', 'placeholder'),
              value: draft,
              disabled: pending,
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
              ? m(FlatButton, { label: t('assistant', 'stop'), onclick: () => attrs.onStop() })
              : m(FlatButton, {
                  label: t('assistant', 'send'),
                  type: 'submit',
                  disabled: !canSendDraft(entry, draft),
                }),
          ],
        ),
      ]);
    },
  };
};
