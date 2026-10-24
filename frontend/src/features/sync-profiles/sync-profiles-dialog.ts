import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type {
  ComparisonCriteria,
  Location,
  SyncMode,
  SyncProfile,
  SyncRunSummary,
  SyncScheduleConfirmation,
} from '../../models';
import {
  MAX_INTERVAL_MINUTES,
  MIN_INTERVAL_MINUTES,
  newSyncProfile,
  type ProfileValidationIssue,
  parsePatternList,
} from './sync-profile';
import type { SyncProfileController } from './sync-profile-controller';
import './sync-profiles-dialog.css';

export interface SyncProfilesDialogAttrs {
  readonly open: boolean;
  readonly controller: SyncProfileController;
  /** Folders currently shown in the left and right panes, offered as profile roots. */
  readonly paneLocations: () => { readonly left?: Location; readonly right?: Location };
  readonly onClose: () => void;
}

const MODES: readonly SyncMode[] = ['twoWayUpdate', 'mirrorLeftToRight', 'mirrorRightToLeft'];
const CRITERIA: readonly ComparisonCriteria[] = [
  'sizeAndTimestamp',
  'nameOnly',
  'contentHash',
  'content',
];
const CONFLICTS: readonly SyncProfile['conflictPolicy'][] = [
  'ask',
  'skip',
  'keepBoth',
  'overwrite',
];
const CONFIRMATIONS: readonly SyncScheduleConfirmation[] = ['required', 'autoApplyAdditions'];

/** A profile under edit: pattern fields are kept as text until saved. */
interface Draft {
  profile: SyncProfile;
  includeText: string;
  excludeText: string;
}

function issueText(issue: ProfileValidationIssue): string {
  return t('syncProfiles', `issue_${issue}` as 'issue_blankName');
}

function lastRunText(run: SyncRunSummary | null): string {
  if (run === null) return t('syncProfiles', 'neverRun');
  const when = new Date(run.at).toLocaleString();
  const base = t('syncProfiles', `outcome_${run.outcome}` as 'outcome_planned', {
    when,
    planned: run.plannedItems,
    applied: run.appliedItems,
  });
  return run.message === null ? base : `${base} ${run.message}`;
}

function selectField<T extends string>(
  label: string,
  value: T,
  options: readonly { readonly id: T; readonly label: string }[],
  onChange: (value: T) => void,
): m.Vnode {
  return m('label.fm-create-directory-field', [
    m('span', label),
    m(
      'select.browser-default',
      {
        value,
        onchange: (event: Event) => onChange((event.currentTarget as HTMLSelectElement).value as T),
      },
      options.map((option) =>
        m('option', { value: option.id, selected: option.id === value }, option.label),
      ),
    ),
  ]);
}

/**
 * "Synchronization profiles…" (task 0161): create, edit, duplicate, delete and manually run named
 * sync pairs. Running a profile never mutates anything; it opens the usual sync-plan review.
 */
export const SyncProfilesDialog: FactoryComponent<SyncProfilesDialogAttrs> = () => {
  let draft: Draft | undefined;
  let issues: readonly ProfileValidationIssue[] = [];
  let confirmingDelete: string | undefined;

  function startNew(attrs: SyncProfilesDialogAttrs): void {
    const panes = attrs.paneLocations();
    const empty: Location = { providerId: 'local', uri: '' };
    draft = {
      profile: newSyncProfile(crypto.randomUUID(), '', panes.left ?? empty, panes.right ?? empty),
      includeText: '',
      excludeText: '',
    };
    issues = [];
  }

  function startEdit(profile: SyncProfile): void {
    draft = {
      profile,
      includeText: profile.include.join('\n'),
      excludeText: profile.exclude.join('\n'),
    };
    issues = [];
  }

  function update(change: Partial<SyncProfile>): void {
    // Mutate in place: input handlers can fire again before the next render replaces `current`.
    if (draft !== undefined) draft.profile = { ...draft.profile, ...change };
  }

  async function save(attrs: SyncProfilesDialogAttrs): Promise<void> {
    if (draft === undefined) return;
    const profile: SyncProfile = {
      ...draft.profile,
      include: parsePatternList(draft.includeText),
      exclude: parsePatternList(draft.excludeText),
    };
    issues = await attrs.controller.save(profile);
    if (issues.length === 0) draft = undefined;
    m.redraw();
  }

  function locationRow(
    attrs: SyncProfilesDialogAttrs,
    label: string,
    location: Location,
    set: (location: Location) => void,
  ): m.Vnode {
    const panes = attrs.paneLocations();
    return m('.fm-sync-profile-location', [
      m('span.fm-sync-profile-label', label),
      m(
        'code.fm-sync-profile-uri',
        location.uri === '' ? t('syncProfiles', 'noLocation') : location.uri,
      ),
      m('.fm-sync-profile-buttons', [
        m(
          'button.btn-flat',
          {
            type: 'button',
            disabled: panes.left === undefined,
            onclick: () => panes.left && set(panes.left),
          },
          t('syncProfiles', 'useLeftPane'),
        ),
        m(
          'button.btn-flat',
          {
            type: 'button',
            disabled: panes.right === undefined,
            onclick: () => panes.right && set(panes.right),
          },
          t('syncProfiles', 'useRightPane'),
        ),
      ]),
    ]);
  }

  function editor(attrs: SyncProfilesDialogAttrs, current: Draft): m.Vnode {
    const { profile } = current;
    const schedule = profile.schedule;
    return m('.fm-sync-profile-editor', [
      m('label.fm-create-directory-field', [
        m('span', t('syncProfiles', 'name')),
        m('input', {
          type: 'text',
          value: profile.name,
          oninput: (event: InputEvent) =>
            update({ name: (event.currentTarget as HTMLInputElement).value }),
        }),
      ]),
      locationRow(attrs, t('syncProfiles', 'leftRoot'), profile.left, (left) => update({ left })),
      locationRow(attrs, t('syncProfiles', 'rightRoot'), profile.right, (right) =>
        update({ right }),
      ),
      selectField(
        t('syncProfiles', 'direction'),
        profile.mode,
        MODES.map((id) => ({ id, label: t('syncProfiles', `mode_${id}` as 'mode_twoWayUpdate') })),
        (mode) => update({ mode }),
      ),
      selectField(
        t('syncProfiles', 'compareBy'),
        profile.criteria,
        CRITERIA.map((id) => ({
          id,
          label: t('syncProfiles', `criteria_${id}` as 'criteria_nameOnly'),
        })),
        (criteria) => update({ criteria }),
      ),
      selectField(
        t('syncProfiles', 'conflicts'),
        profile.conflictPolicy,
        CONFLICTS.map((id) => ({
          id,
          label: t('syncProfiles', `conflict_${id}` as 'conflict_ask'),
        })),
        (conflictPolicy) => update({ conflictPolicy }),
      ),
      m('label.fm-create-directory-field', [
        m('span', t('syncProfiles', 'include')),
        m('textarea', {
          rows: 2,
          value: current.includeText,
          placeholder: '*.jpg',
          oninput: (event: InputEvent) => {
            if (draft !== undefined)
              draft.includeText = (event.currentTarget as HTMLTextAreaElement).value;
          },
        }),
      ]),
      m('label.fm-create-directory-field', [
        m('span', t('syncProfiles', 'exclude')),
        m('textarea', {
          rows: 2,
          value: current.excludeText,
          placeholder: 'node_modules\n*.tmp',
          oninput: (event: InputEvent) => {
            if (draft !== undefined)
              draft.excludeText = (event.currentTarget as HTMLTextAreaElement).value;
          },
        }),
      ]),
      m('p.fm-sync-profile-hint', t('syncProfiles', 'patternHint')),
      m('label.fm-sync-profile-check', [
        m('input', {
          type: 'checkbox',
          checked: profile.showHidden,
          onchange: (event: Event) =>
            update({ showHidden: (event.currentTarget as HTMLInputElement).checked }),
        }),
        m('span', t('syncProfiles', 'showHidden')),
      ]),
      m('fieldset.fm-sync-profile-schedule', [
        m('legend', t('syncProfiles', 'schedule')),
        m('label.fm-sync-profile-check', [
          m('input', {
            type: 'checkbox',
            checked: schedule?.enabled === true,
            onchange: (event: Event) => {
              const enabled = (event.currentTarget as HTMLInputElement).checked;
              update({
                schedule: { intervalMinutes: 60, confirmation: 'required', ...schedule, enabled },
              });
            },
          }),
          m('span', t('syncProfiles', 'scheduleEnabled')),
        ]),
        schedule === null
          ? undefined
          : [
              m('label.fm-create-directory-field', [
                m('span', t('syncProfiles', 'intervalMinutes')),
                m('input', {
                  type: 'number',
                  min: MIN_INTERVAL_MINUTES,
                  max: MAX_INTERVAL_MINUTES,
                  value: schedule.intervalMinutes,
                  oninput: (event: InputEvent) =>
                    update({
                      schedule: {
                        ...schedule,
                        intervalMinutes: Number((event.currentTarget as HTMLInputElement).value),
                      },
                    }),
                }),
              ]),
              selectField(
                t('syncProfiles', 'confirmation'),
                schedule.confirmation,
                CONFIRMATIONS.map((id) => ({
                  id,
                  label: t('syncProfiles', `confirmation_${id}` as 'confirmation_required'),
                })),
                (confirmation) => update({ schedule: { ...schedule, confirmation } }),
              ),
              m('p.fm-sync-profile-hint', t('syncProfiles', 'scheduleHint')),
            ],
      ]),
      issues.length === 0
        ? undefined
        : m(
            'ul.fm-field-error',
            issues.map((issue) => m('li', issueText(issue))),
          ),
    ]);
  }

  function list(attrs: SyncProfilesDialogAttrs): m.Vnode {
    const profiles = attrs.controller.profiles();
    const pending = attrs.controller.pendingReview();
    return m('.fm-sync-profiles', [
      pending === undefined || pending.open
        ? undefined
        : m('.fm-sync-profile-pending', [
            m('span', t('syncProfiles', 'reviewPending', { name: pending.profile.name })),
            m(
              'button.btn-flat',
              { type: 'button', onclick: () => attrs.controller.openReview() },
              t('syncProfiles', 'review'),
            ),
          ]),
      profiles.length === 0
        ? m('p', t('syncProfiles', 'empty'))
        : m(
            'ul.fm-sync-profile-list',
            profiles.map((profile) =>
              m('li.fm-sync-profile-row', { key: profile.id }, [
                m('.fm-sync-profile-summary', [
                  m('strong', profile.name),
                  m(
                    'small',
                    `${profile.left.uri} ${t('syncProfiles', `modeArrow_${profile.mode}` as 'modeArrow_twoWayUpdate')} ${profile.right.uri}`,
                  ),
                  m('small', lastRunText(profile.lastRun)),
                  profile.schedule?.enabled === true
                    ? m(
                        'small',
                        t('syncProfiles', 'scheduledEvery', {
                          minutes: profile.schedule.intervalMinutes,
                        }),
                      )
                    : undefined,
                ]),
                m('.fm-sync-profile-buttons', [
                  m(
                    'button.btn-flat',
                    {
                      type: 'button',
                      disabled: attrs.controller.isRunning(profile.id) || pending !== undefined,
                      onclick: () => {
                        void attrs.controller.run(profile.id);
                      },
                    },
                    attrs.controller.isRunning(profile.id)
                      ? t('syncProfiles', 'running')
                      : t('syncProfiles', 'run'),
                  ),
                  m(
                    'button.btn-flat',
                    { type: 'button', onclick: () => startEdit(profile) },
                    t('syncProfiles', 'edit'),
                  ),
                  m(
                    'button.btn-flat',
                    {
                      type: 'button',
                      onclick: () => {
                        void attrs.controller.duplicate(profile.id).then(() => m.redraw());
                      },
                    },
                    t('syncProfiles', 'duplicate'),
                  ),
                  m(
                    'button.btn-flat',
                    {
                      type: 'button',
                      onclick: () => {
                        if (confirmingDelete === profile.id) {
                          confirmingDelete = undefined;
                          void attrs.controller.remove(profile.id).then(() => m.redraw());
                        } else {
                          confirmingDelete = profile.id;
                        }
                      },
                    },
                    confirmingDelete === profile.id
                      ? t('syncProfiles', 'confirmDelete')
                      : t('syncProfiles', 'delete'),
                  ),
                ]),
              ]),
            ),
          ),
    ]);
  }

  return {
    view: ({ attrs }) => {
      if (!attrs.open && draft !== undefined) draft = undefined;
      return m(ModalPanel, {
        title: t('syncProfiles', 'title'),
        className: 'fm-dense-modal fm-sync-profiles-dialog',
        description: draft === undefined ? list(attrs) : editor(attrs, draft),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) attrs.onClose();
        },
        buttons:
          draft === undefined
            ? [
                { label: t('syncProfiles', 'new'), onclick: () => startNew(attrs) },
                { label: t('button', 'close'), onclick: () => attrs.onClose() },
              ]
            : [
                {
                  label: t('button', 'cancel'),
                  onclick: () => {
                    draft = undefined;
                    issues = [];
                  },
                },
                {
                  label: t('syncProfiles', 'save'),
                  onclick: () => {
                    void save(attrs);
                  },
                },
              ],
      });
    },
  };
};
