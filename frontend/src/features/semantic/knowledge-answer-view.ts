import m from 'mithril';
import { IconButton, Skeleton } from 'mithril-materialized';
import { technicalTextInputAttrs } from '../../a11y/technical-text-input';
import {
  brainIcon,
  closeIcon,
  cloudIcon,
  cornerDownLeftIcon,
  sparklesIcon,
} from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type { KnowledgeAnswer, KnowledgeEvidence, LlmProfile } from '../../models';
import { safeMarkdownHtml } from '../editor/markdown-preview';
import {
  type KnowledgeAnswerReference,
  linkKnowledgeReferences,
  numberKnowledgeAnswer,
  REFERENCE_ANCHOR,
} from './knowledge-answer-references';

export interface KnowledgeAnswerBodyProps {
  readonly answer: KnowledgeAnswer | undefined;
  readonly generating: boolean;
  readonly notice: string | undefined;
  readonly displayed: ReadonlyMap<string, KnowledgeEvidence>;
  /** Whether the host can open a source at all. */
  readonly canOpen: boolean;
  readonly openRow: (row: KnowledgeEvidence) => void;
}

export interface KnowledgeAskBarProps {
  readonly question: string;
  readonly allowModelKnowledge: boolean;
  readonly generating: boolean;
  readonly profile: LlmProfile | undefined;
  readonly canAsk: boolean;
  readonly onQuestion: (value: string) => void;
  readonly onAllowModelKnowledge: (value: boolean) => void;
  readonly onAsk: () => void;
  readonly onCancel: () => void;
}

/** The generated answer itself, plus how honest it is about its evidence. */
export function knowledgeAnswerBody(props: KnowledgeAnswerBodyProps): m.Children {
  const { answer: current, generating, notice, displayed } = props;
  if (generating) {
    return m('.fm-knowledge-status.fm-knowledge-answer-progress', { role: 'status' }, [
      m('span', t('knowledgeSearch', 'generatingAnswer')),
      m(Skeleton, { count: 3, className: 'fm-knowledge-answer-skeleton' }),
    ]);
  }
  if (current === undefined) {
    return m(
      'p.fm-knowledge-status',
      { role: 'status' },
      notice ?? t('knowledgeSearch', 'answerPlaceholder'),
    );
  }
  const numbered = numberKnowledgeAnswer(current.text, current.citations, displayed);
  const openable = (reference: KnowledgeAnswerReference): boolean =>
    reference.row !== undefined &&
    reference.citations.some((citation) => !citation.unavailable) &&
    props.canOpen;
  const openReference = (reference: KnowledgeAnswerReference): void => {
    if (reference.row !== undefined && openable(reference)) props.openRow(reference.row);
  };
  return [
    m(
      '.fm-knowledge-answer-markdown',
      {
        onclick: (event: MouseEvent) => {
          if (!(event.target instanceof Element)) return;
          const link = event.target.closest<HTMLAnchorElement>(`a[href^="${REFERENCE_ANCHOR}"]`);
          if (link === null) return;
          event.preventDefault();
          const number = Number(link.getAttribute('href')?.slice(REFERENCE_ANCHOR.length));
          const reference = numbered.references.find((item) => item.number === number);
          if (reference !== undefined) openReference(reference);
        },
      },
      m.trust(
        linkKnowledgeReferences(
          safeMarkdownHtml(numbered.markdown),
          new Set(numbered.references.filter(openable).map((reference) => reference.number)),
        ),
      ),
    ),
    current.modelKnowledgeAllowed
      ? m('p.fm-knowledge-warning', t('knowledgeSearch', 'modelKnowledgeUsed'))
      : undefined,
    current.insufficient
      ? m('p.fm-knowledge-warning', t('knowledgeSearch', 'answerInsufficient'))
      : undefined,
    current.staleEvidence === 0
      ? undefined
      : m(
          'p.fm-knowledge-warning',
          t('knowledgeSearch', 'answerStaleEvidence', { count: current.staleEvidence }),
        ),
    current.unavailableEvidence === 0
      ? undefined
      : m(
          'p.fm-knowledge-warning',
          t('knowledgeSearch', 'answerUnavailableEvidence', {
            count: current.unavailableEvidence,
          }),
        ),
    current.withheldUnauthorized === 0
      ? undefined
      : m(
          'p.fm-knowledge-warning',
          t('knowledgeSearch', 'answerWithheldUnauthorized', {
            count: current.withheldUnauthorized,
          }),
        ),
    numbered.references.length === 0
      ? undefined
      : // Keyed so each new answer starts with its sources collapsed again.
        [
          m('details.fm-knowledge-answer-references', { key: current.requestId }, [
            m(
              'summary',
              t('knowledgeSearch', 'answerCitations', { count: numbered.references.length }),
            ),
            m(
              'ol.fm-knowledge-citations',
              numbered.references.map((reference) => {
                const sections = [
                  ...new Set(
                    reference.citations
                      .map((citation) => citation.sectionPath.join(' / '))
                      .filter((section) => section !== '' && section !== reference.title),
                  ),
                ];
                const states = [
                  ...sections,
                  reference.citations.some((citation) => citation.generated)
                    ? t('knowledgeSearch', 'generatedEvidence')
                    : undefined,
                  reference.citations.some((citation) => citation.stale === true)
                    ? t('knowledgeSearch', 'staleEvidence')
                    : undefined,
                  reference.citations.every((citation) => citation.unavailable)
                    ? t('knowledgeSearch', 'unavailableEvidence')
                    : undefined,
                  reference.row === undefined
                    ? t('knowledgeSearch', 'answerCitationNotDisplayed')
                    : undefined,
                ].filter((value): value is string => value !== undefined);
                return m('li', { key: `reference-${reference.number}` }, [
                  m('span.fm-knowledge-reference-number', `[${reference.number}]`),
                  m(
                    'button.fm-knowledge-source-link',
                    {
                      type: 'button',
                      title: reference.title,
                      disabled: !openable(reference),
                      'aria-label': t('knowledgeSearch', 'openCitation', {
                        label: reference.title,
                      }),
                      onclick: () => openReference(reference),
                    },
                    m('span', reference.title),
                  ),
                  states.length === 0
                    ? undefined
                    : m('small.fm-knowledge-hint', states.join(' · ')),
                ]);
              }),
            ),
          ]),
        ],
  ];
}

/** The question box, model-knowledge toggle and ask/cancel control. */
export function knowledgeAskBar(props: KnowledgeAskBarProps): m.Vnode {
  return m('.fm-knowledge-ask-bar', [
    sparklesIcon({ className: 'fm-knowledge-ask-icon', size: 15 }),
    m(
      'label.fm-visually-hidden',
      { for: 'fm-knowledge-question' },
      t('knowledgeSearch', 'question'),
    ),
    m('textarea#fm-knowledge-question', {
      ...technicalTextInputAttrs,
      name: 'knowledge-answer-question',
      rows: 1,
      maxlength: 8192,
      value: props.question,
      placeholder: t('knowledgeSearch', 'questionPlaceholder'),
      oninput: (event: InputEvent) => {
        // The previous answer stays until the new question is actually asked.
        props.onQuestion((event.currentTarget as HTMLTextAreaElement).value);
      },
      onkeydown: (event: KeyboardEvent) => {
        event.stopPropagation();
        if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
          event.preventDefault();
          props.onAsk();
        }
      },
    }),
    props.profile?.locality === 'cloud'
      ? tooltip(
          t('knowledgeSearch', 'answerCloudEndpoint', { profile: props.profile.name }),
          cloudIcon({ className: 'fm-knowledge-ask-cloud', size: 15 }),
          {
            tabindex: 0,
            role: 'img',
            'aria-label': t('knowledgeSearch', 'answerCloudEndpoint', {
              profile: props.profile.name,
            }),
          },
        )
      : undefined,
    tooltip(
      t('knowledgeSearch', 'allowModelKnowledgeHint'),
      m(
        'label.fm-knowledge-model-knowledge',
        {
          for: 'fm-knowledge-model-knowledge',
          class: props.allowModelKnowledge ? 'is-active' : undefined,
        },
        [
          m('input#fm-knowledge-model-knowledge.fm-visually-hidden', {
            type: 'checkbox',
            checked: props.allowModelKnowledge,
            disabled: props.generating,
            onchange: (event: Event) => {
              props.onAllowModelKnowledge((event.currentTarget as HTMLInputElement).checked);
            },
          }),
          brainIcon({ size: 15 }),
          m('span.fm-visually-hidden', t('knowledgeSearch', 'allowModelKnowledge')),
        ],
      ),
    ),
    props.generating
      ? tooltip(
          t('knowledgeSearch', 'cancelAnswer'),
          m(
            IconButton,
            {
              type: 'button',
              className: 'fm-knowledge-ask-cancel',
              'aria-label': t('knowledgeSearch', 'cancelAnswer'),
              onclick: props.onCancel,
            },
            closeIcon({ size: 13 }),
          ),
        )
      : tooltip(
          t('knowledgeSearch', 'askWith', { profile: props.profile?.name ?? '' }),
          m(
            IconButton,
            {
              type: 'button',
              className: 'fm-knowledge-generate',
              'aria-label': t('knowledgeSearch', 'ask'),
              disabled: !props.canAsk,
              onclick: () => props.onAsk(),
            },
            cornerDownLeftIcon({ size: 16 }),
          ),
        ),
  ]);
}
