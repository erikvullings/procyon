import m, { type FactoryComponent } from 'mithril';
import { t } from '../../i18n';
import type { LinkProblem, LinkReplacementCandidate } from '../../models';
import {
  type LinkDoctorState,
  missingTargetName,
  problemKey,
  sourceName,
} from './link-doctor-state';

export interface LinkDoctorViewAttrs {
  readonly state: LinkDoctorState;
  readonly onFindReplacement: (problem: LinkProblem) => void;
  readonly onRepoint: (problem: LinkProblem, candidate: LinkReplacementCandidate) => void;
  readonly onOpenSource: (problem: LinkProblem) => void;
  readonly onCancel: () => void;
  readonly onClose: () => void;
}

function kindLabel(problem: LinkProblem): string {
  switch (problem.kind) {
    case 'brokenSymlink':
      return t('linkDoctor', 'brokenSymlink');
    case 'brokenShortcut':
      return t('linkDoctor', 'brokenShortcut');
    case 'missingPlaylistEntry':
      return t('linkDoctor', 'missingPlaylistEntry');
    case 'missingDocumentLink':
      return t('linkDoctor', 'missingDocumentLink');
  }
}

function decoded(uri: string): string {
  try {
    return decodeURIComponent(uri);
  } catch {
    return uri;
  }
}

function renderCandidates(attrs: LinkDoctorViewAttrs, problem: LinkProblem) {
  const entry = attrs.state.candidates[problemKey(problem)];
  if (entry === undefined) return undefined;
  if (entry.loading) return m('p.link-doctor__hint', t('linkDoctor', 'searching'));
  if (entry.candidates.length === 0) {
    return m('p.link-doctor__hint', t('linkDoctor', 'noCandidates'));
  }
  return m('ul.link-doctor__candidates', [
    ...entry.candidates.map((candidate) =>
      m('li', { key: candidate.location.uri }, [
        m(
          'span.link-doctor__path',
          { title: candidate.location.uri },
          decoded(candidate.location.uri),
        ),
        m(
          'span.link-doctor__hint',
          ` · ${candidate.sameSize ? t('linkDoctor', 'sameSize') : t('linkDoctor', 'differentSize')}`,
        ),
        problem.kind === 'brokenSymlink'
          ? m(
              'button',
              {
                type: 'button',
                disabled: attrs.state.repairing !== undefined,
                onclick: () => attrs.onRepoint(problem, candidate),
              },
              t('linkDoctor', 'useThis'),
            )
          : undefined,
      ]),
    ),
    entry.truncated ? m('li.link-doctor__hint', t('linkDoctor', 'candidatesTruncated')) : undefined,
  ]);
}

/** Review panel for broken links found by the link doctor (task 0266). */
export const LinkDoctorView: FactoryComponent<LinkDoctorViewAttrs> = () => ({
  view: ({ attrs }) => {
    const { state } = attrs;
    const running = !state.isComplete;
    return m('section.duplicate-review.link-doctor', { 'aria-label': t('linkDoctor', 'title') }, [
      m('header.duplicate-review__header', [
        m('h2', t('linkDoctor', 'title')),
        m(
          'span.duplicate-review__summary',
          state.isCancelled
            ? t('linkDoctor', 'cancelled')
            : running
              ? t('linkDoctor', 'scanning', {
                  entries: state.entriesScanned,
                  links: state.linksChecked,
                })
              : t('linkDoctor', 'summary', {
                  count: state.problems.length,
                  entries: state.entriesScanned,
                }),
        ),
        running
          ? m(
              'button.duplicate-review__cancel',
              { type: 'button', onclick: () => attrs.onCancel() },
              t('linkDoctor', 'cancel'),
            )
          : undefined,
        m(
          'button.duplicate-review__close',
          {
            type: 'button',
            'aria-label': t('linkDoctor', 'close'),
            onclick: () => attrs.onClose(),
          },
          t('button', 'close'),
        ),
      ]),
      state.error === undefined
        ? undefined
        : m('p.duplicate-review__error', { role: 'alert' }, state.error),
      state.isComplete && state.problems.length === 0 && !state.isCancelled
        ? m('p.duplicate-review__empty', t('linkDoctor', 'none'))
        : undefined,
      m(
        'ul.duplicate-review__groups',
        state.problems.map((problem) =>
          m('li.duplicate-review__group', { key: problemKey(problem) }, [
            m('h3.duplicate-review__group-title', [
              m('span.music-duplicates__badge', kindLabel(problem)),
              ` ${sourceName(problem)}`,
              problem.line == null
                ? undefined
                : m(
                    'span.link-doctor__hint',
                    ` · ${t('linkDoctor', 'line', { line: problem.line })}`,
                  ),
            ]),
            m('p.link-doctor__target', [
              `${t('linkDoctor', 'points')} `,
              m('code', problem.target || missingTargetName(problem)),
            ]),
            m('.music-duplicates__actions', [
              m(
                'button',
                {
                  type: 'button',
                  disabled: state.candidates[problemKey(problem)]?.loading === true,
                  onclick: () => attrs.onFindReplacement(problem),
                },
                t('linkDoctor', 'findReplacement'),
              ),
              problem.kind === 'brokenSymlink' || problem.kind === 'brokenShortcut'
                ? undefined
                : m(
                    'button',
                    { type: 'button', onclick: () => attrs.onOpenSource(problem) },
                    t('linkDoctor', 'openSource'),
                  ),
            ]),
            renderCandidates(attrs, problem),
          ]),
        ),
      ),
    ]);
  },
});
