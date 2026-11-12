import m, { type FactoryComponent } from 'mithril';
import { Switch } from 'mithril-materialized';

import type { SemanticClient } from '../../api/client/semantic-client.ts';
import { t } from '../../i18n';
import type { LlmProfile, LlmRole, LlmRoleAssignment, LlmRoles } from '../../models';
import { canAssignRole, LLM_ROLES, type RoleStatus, roleStatus } from '../../models/llm-roles';
import { errorMessage } from './llm-profile-management';

export interface LlmRoleManagementAttrs {
  readonly client: SemanticClient;
}

/** The editable binding of one role; `profileId` empty means unassigned. */
interface RoleDraft {
  profileId: string;
  model: string;
  allowCloud: boolean;
}

const STATUS_KEYS = {
  unassigned: 'statusUnassigned',
  missingProfile: 'statusMissingProfile',
  cloudNotAllowed: 'statusCloudNotAllowed',
  needsConsent: 'statusNeedsConsent',
  ready: 'statusReady',
} as const satisfies Record<RoleStatus, string>;

function draftFrom(assignment: LlmRoleAssignment | null | undefined): RoleDraft {
  return assignment == null
    ? { profileId: '', model: '', allowCloud: false }
    : {
        profileId: assignment.profileId,
        model: assignment.model,
        allowCloud: assignment.allowCloud,
      };
}

/**
 * Roles (task 0322): which endpoint and model handle chat and semantic embeddings. Cloud use for
 * embeddings is a separate, explicit flag; consent for a cloud host is given on its endpoint.
 */
export const LlmRoleManagement: FactoryComponent<LlmRoleManagementAttrs> = () => {
  let profiles: LlmProfile[] = [];
  let roles: LlmRoles = { chat: null, embeddings: null };
  let drafts: Record<LlmRole, RoleDraft> = {
    chat: draftFrom(null),
    embeddings: draftFrom(null),
  };
  let loading = true;
  let busy = false;
  let message: string | undefined;
  let error: string | undefined;

  async function load(client: SemanticClient): Promise<void> {
    loading = true;
    error = undefined;
    try {
      [profiles, roles] = await Promise.all([client.listLlmProfiles(), client.listLlmRoles()]);
      drafts = { chat: draftFrom(roles.chat), embeddings: draftFrom(roles.embeddings) };
    } catch (reason) {
      error = errorMessage(reason);
    } finally {
      loading = false;
      m.redraw();
    }
  }

  async function run(action: () => Promise<void>): Promise<void> {
    busy = true;
    error = undefined;
    message = undefined;
    try {
      await action();
      message = t('llmRoles', 'saved');
    } catch (reason) {
      error = errorMessage(reason);
    } finally {
      busy = false;
      m.redraw();
    }
  }

  function roleBlock(attrs: LlmRoleManagementAttrs, role: LlmRole) {
    const draft = drafts[role];
    const selected = profiles.find((profile) => profile.id === draft.profileId);
    const status = roleStatus(roles[role], role, profiles);
    const savedRole = roles[role];
    return m('.fm-llm-role', { 'aria-label': t('llmRoles', role) }, [
      m('h5', t('llmRoles', role)),
      m('label.fm-llm-role-endpoint', [
        t('llmRoles', 'endpoint'),
        m(
          'select',
          {
            value: draft.profileId,
            disabled: busy,
            onchange: (event: Event) => {
              drafts = {
                ...drafts,
                [role]: { ...draft, profileId: (event.target as HTMLSelectElement).value },
              };
            },
          },
          [
            m('option', { value: '' }, t('llmRoles', 'unassigned')),
            ...profiles.map((profile) =>
              m(
                'option',
                {
                  value: profile.id,
                  disabled: !canAssignRole(profile, role, draft.allowCloud),
                },
                profile.name,
              ),
            ),
          ],
        ),
      ]),
      m('label.fm-llm-role-model', [
        t('llmRoles', 'model'),
        m('input[type=text]', {
          value: draft.model,
          disabled: busy,
          oninput: (event: Event) => {
            drafts = {
              ...drafts,
              [role]: { ...draft, model: (event.target as HTMLInputElement).value },
            };
          },
        }),
      ]),
      role === 'embeddings'
        ? m(Switch, {
            label: t('llmRoles', 'allowCloud'),
            checked: draft.allowCloud,
            disabled: busy,
            onchange: (checked: boolean) => {
              drafts = { ...drafts, [role]: { ...draft, allowCloud: checked } };
            },
          })
        : undefined,
      role === 'embeddings' && selected !== undefined && selected.locality === 'cloud'
        ? m('p.fm-llm-role-hint', t('llmRoles', 'cloudHint'))
        : undefined,
      m('p.fm-llm-role-status', t('llmRoles', STATUS_KEYS[status])),
      m('.fm-llm-role-actions', [
        m(
          'button.fm-semantic-action',
          {
            type: 'button',
            disabled: busy || draft.profileId === '' || draft.model.trim() === '',
            onclick: () =>
              void run(async () => {
                roles = await attrs.client.setLlmRole(role, {
                  profileId: draft.profileId,
                  model: draft.model.trim(),
                  allowCloud: draft.allowCloud,
                });
                drafts = { ...drafts, [role]: draftFrom(roles[role]) };
              }),
          },
          t('llmRoles', 'save'),
        ),
        m(
          'button.fm-semantic-action',
          {
            type: 'button',
            disabled: busy || savedRole == null,
            onclick: () =>
              void run(async () => {
                roles = await attrs.client.clearLlmRole(role);
                drafts = { ...drafts, [role]: draftFrom(null) };
              }),
          },
          t('llmRoles', 'clear'),
        ),
      ]),
    ]);
  }

  return {
    oninit: ({ attrs }) => {
      void load(attrs.client);
    },
    view: ({ attrs }) => {
      if (loading) return m('p', t('llmProfiles', 'loading'));
      return m('.fm-llm-roles', { 'aria-label': t('llmRoles', 'title') }, [
        m('h4', t('llmRoles', 'title')),
        m('p', t('llmRoles', 'intro')),
        ...LLM_ROLES.map((role) => roleBlock(attrs, role)),
        error ? m('p.fm-llm-role-error', { role: 'alert' }, error) : undefined,
        message ? m('p.fm-llm-role-message', { role: 'status' }, message) : undefined,
      ]);
    },
  };
};
