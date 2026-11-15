import m, { type FactoryComponent } from 'mithril';
import { Select, Switch, TextInput } from 'mithril-materialized';

import type { SemanticClient } from '../../api/client/semantic-client.ts';
import { t } from '../../i18n';
import type {
  LlmProfile,
  LlmProfilePreset,
  LlmRole,
  LlmRoleAssignment,
  LlmRoles,
} from '../../models';
import { canAssignRole, LLM_ROLES, type RoleStatus, roleStatus } from '../../models/llm-roles';
import { connectionLabel, errorMessage } from './llm-profile-management';

export interface LlmRoleManagementAttrs {
  readonly client: SemanticClient;
  /** Changes whenever an endpoint changes; a new value reloads the bindings and profiles. */
  readonly revision: number;
  readonly embeddingModelId?: string | undefined;
  readonly onManageSemantic?: (() => void) | undefined;
  /**
   * Asks the endpoint editor above to open with one provider's preset, so a provider the person has
   * no endpoint for yet is one choice away rather than a trip through the section above.
   */
  readonly onCreateEndpoint?: ((presetId: string) => void) | undefined;
}

/** The editable binding of one role; `profileId` empty means unassigned. */
interface RoleDraft {
  profileId: string;
  model: string;
  allowCloud: boolean;
}

/** Only states the person must act on get a line; "ready" and "unassigned" are shown by the field. */
type NoticeKey = 'statusMissingProfile' | 'statusCloudNotAllowed' | 'statusNeedsConsent';

const NOTICE_KEYS: Partial<Record<RoleStatus, NoticeKey>> = {
  missingProfile: 'statusMissingProfile',
  cloudNotAllowed: 'statusCloudNotAllowed',
  needsConsent: 'statusNeedsConsent',
};

function modelOptions(
  draft: RoleDraft,
  choices: Record<string, string[]>,
): { id: string; label: string }[] {
  const offered = choices[draft.profileId] ?? [];
  // A saved model that the endpoint no longer lists stays selectable, so it can be replaced.
  const ids = [...new Set(draft.model === '' ? offered : [...offered, draft.model])];
  return ids.filter((id) => id.trim() !== '').map((id) => ({ id, label: id }));
}

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
 * Roles (tasks 0322, 0325): which endpoint and model handle chat, image generation and semantic
 * embeddings. Every role is visible and saved independently. Image generation never inherits a
 * chat model. Cloud permission and endpoint consent remain explicit.
 */
export const LlmRoleManagement: FactoryComponent<LlmRoleManagementAttrs> = () => {
  let profiles: LlmProfile[] = [];
  let roles: LlmRoles = { chat: null, embeddings: null, image: null };
  let drafts: Record<LlmRole, RoleDraft> = {
    chat: draftFrom(null),
    embeddings: draftFrom(null),
    image: draftFrom(null),
  };
  /** Models each profile offers, from discovery when the endpoint supports it. */
  let choices: Record<string, string[]> = {};
  /** Provider presets the app knows, so the picture role can name a server it has no endpoint for. */
  let presets: LlmProfilePreset[] = [];
  let loading = true;
  let busy = false;
  let loadedRevision: number | undefined;
  let message: string | undefined;
  let error: string | undefined;

  /** Loads the models a profile offers; a profile that cannot discover offers its own model. */
  async function loadChoices(client: SemanticClient, profileId: string): Promise<void> {
    const profile = profiles.find((candidate) => candidate.id === profileId);
    if (profile === undefined) return;
    let offered = [profile.model];
    if (profile.capabilities.includes('modelDiscovery')) {
      try {
        offered = await client.discoverLlmProfileModels(profileId);
      } catch {
        offered = [profile.model];
      }
    }
    choices = { ...choices, [profileId]: offered };
    m.redraw();
  }

  async function load(client: SemanticClient): Promise<void> {
    loading = true;
    error = undefined;
    try {
      [profiles, roles, presets] = await Promise.all([
        client.listLlmProfiles(),
        client.listLlmRoles(),
        client.listLlmProfilePresets(),
      ]);
      drafts = {
        chat: draftFrom(roles.chat),
        embeddings: draftFrom(roles.embeddings),
        image: draftFrom(roles.image),
      };
      const assigned = LLM_ROLES.map((role) => drafts[role].profileId).filter(
        (profileId) => profileId !== '',
      );
      await Promise.all([...new Set(assigned)].map((id) => loadChoices(client, id)));
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
    const notice = NOTICE_KEYS[status];
    const savedRole = roles[role];
    return m('.fm-llm-role', { 'aria-label': t('llmRoles', role) }, [
      m('header.fm-llm-role-heading', [
        m('h5.fm-llm-role-title', t('llmRoles', role)),
        m(
          'p.fm-llm-role-notice',
          t(
            'llmRoles',
            role === 'image'
              ? 'imagePurpose'
              : role === 'chat'
                ? 'chatPurpose'
                : 'embeddingsPurpose',
          ),
        ),
      ]),
      m('.fm-llm-role-fields', [
        m(Select<string>, {
          className: 'fm-llm-field',
          label: t('llmRoles', 'provider'),
          disabled: busy,
          options: [
            { id: '', label: t('llmRoles', 'unassigned') },
            ...profiles.map((profile) => ({
              id: profile.id,
              label: connectionLabel(profile, profiles),
              disabled: !canAssignRole(profile, role, draft.allowCloud),
            })),
            ...(attrs.onCreateEndpoint === undefined
              ? []
              : [{ id: '__add_connection__', label: t('llmRoles', 'addConnection') }]),
          ],
          checkedId: draft.profileId,
          onchange: ([value]) => {
            if (value === undefined) return;
            if (value === '__add_connection__') {
              attrs.onCreateEndpoint?.(presets[0]?.preset ?? 'openAiCompatible');
              return;
            }
            const chosen = profiles.find((profile) => profile.id === value);
            // A chat model is never an implicit image model.
            const model = role === 'image' ? '' : (chosen?.model ?? '');
            drafts = { ...drafts, [role]: { ...draft, profileId: value, model } };
            if (value !== '') void loadChoices(attrs.client, value);
          },
        }),
        // A drawing model is almost never one the endpoint lists for chat, and discovery reports the
        // chat models, so the name is typed rather than picked. It is never taken from the chat role:
        // most models cannot generate images at all.
        role === 'image' || modelOptions(draft, choices).length === 0
          ? m(TextInput, {
              className: 'fm-llm-field',
              label: t('llmRoles', 'model'),
              disabled: busy || draft.profileId === '',
              value: draft.model,
              placeholder:
                role === 'image' ? t('llmRoles', 'imageModelPlaceholder') : (selected?.model ?? ''),
              oninput: (value: string) => {
                drafts = { ...drafts, [role]: { ...draft, model: value } };
              },
            })
          : m(Select<string>, {
              className: 'fm-llm-field',
              label: t('llmRoles', 'model'),
              disabled: busy || draft.profileId === '',
              options: modelOptions(draft, choices),
              checkedId: draft.model,
              onchange: ([value]) => {
                if (value === undefined) return;
                drafts = { ...drafts, [role]: { ...draft, model: value } };
              },
            }),
      ]),
      role === 'image' && draft.profileId !== ''
        ? m('p.fm-llm-role-notice', t('llmRoles', 'imageModelHint'))
        : undefined,
      // Only chat may reach the cloud without the flag; embeddings and images need it turned on.
      role !== 'chat'
        ? m(Switch, {
            // The embeddings switch speaks of indexing; a picture role sends the words typed into the
            // pane and gets the pictures back, so it says which of the two this permission covers.
            label: t('llmRoles', role === 'image' ? 'allowCloudImage' : 'allowCloud'),
            checked: draft.allowCloud,
            disabled: busy,
            onchange: (checked: boolean) => {
              drafts = { ...drafts, [role]: { ...draft, allowCloud: checked } };
            },
          })
        : undefined,
      role !== 'chat' && selected !== undefined && selected.locality === 'cloud'
        ? m(
            'p.fm-llm-role-notice',
            t('llmRoles', role === 'image' ? 'cloudHintImage' : 'cloudHint'),
          )
        : undefined,
      notice === undefined ? undefined : m('p.fm-llm-role-notice', t('llmRoles', notice)),
      m('.fm-form-actions', [
        m(
          'button.btn-flat.fm-form-actions-leading',
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
        m(
          'button.btn-flat.fm-primary-action',
          {
            type: 'button',
            disabled: busy || draft.profileId === '' || draft.model.trim() === '',
            onclick: () =>
              void run(async () => {
                const model = draft.model.trim();
                roles = await attrs.client.setLlmRole(role, {
                  profileId: draft.profileId,
                  model,
                  allowCloud: draft.allowCloud,
                });
                drafts = { ...drafts, [role]: draftFrom(roles[role]) };
              }),
          },
          t('llmRoles', 'save'),
        ),
      ]),
    ]);
  }

  return {
    oninit: ({ attrs }) => {
      loadedRevision = attrs.revision;
      void load(attrs.client);
    },
    onupdate: ({ attrs }) => {
      if (attrs.revision === loadedRevision) return;
      loadedRevision = attrs.revision;
      void load(attrs.client);
    },
    view: ({ attrs }) => {
      if (loading) return m('p', t('llmProfiles', 'loading'));
      return m('.fm-llm-roles', { 'aria-label': t('llmRoles', 'title') }, [
        m('p.fm-llm-roles-intro', t('llmRoles', 'intro')),
        ...(['chat', 'image'] as const).map((role) => roleBlock(attrs, role)),
        m('section.fm-llm-role', { 'aria-label': t('llmRoles', 'embeddings') }, [
          m('h5.fm-llm-role-title', t('llmRoles', 'embeddings')),
          m('p.fm-llm-role-notice', t('llmRoles', 'embeddingExplanation')),
          m('p.fm-llm-role-notice', [
            t('llmRoles', 'embeddingModelLabel'),
            ': ',
            attrs.embeddingModelId ?? t('llmRoles', 'embeddingModelManaged'),
          ]),
          attrs.onManageSemantic === undefined
            ? undefined
            : m(
                'button.btn-flat',
                {
                  type: 'button',
                  onclick: attrs.onManageSemantic,
                },
                t('llmRoles', 'manageSemanticModel'),
              ),
        ]),
        error ? m('p.fm-llm-role-error', { role: 'alert' }, error) : undefined,
        message ? m('p.fm-llm-role-message', { role: 'status' }, message) : undefined,
      ]);
    },
  };
};
