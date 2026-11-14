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
import { errorMessage } from './llm-profile-management';

export interface LlmRoleManagementAttrs {
  readonly client: SemanticClient;
  /** Changes whenever an endpoint changes; a new value reloads the bindings and profiles. */
  readonly revision: number;
  /**
   * Asks the endpoint editor above to open with one provider's preset, so a provider the person has
   * no endpoint for yet is one choice away rather than a trip through the section above.
   */
  readonly onCreateEndpoint?: ((presetId: string) => void) | undefined;
}

/** What picking a provider for the picture role amounts to. */
export type ProviderChoice =
  | { readonly kind: 'bind'; readonly profileId: string; readonly model: string }
  | { readonly kind: 'create'; readonly name: string }
  | { readonly kind: 'none' };

/**
 * Decides what choosing a provider means (0325). A server the person already added is bound as the
 * image endpoint; one they have not added yet is filled into the endpoint editor above, because a
 * role may not be bound to an endpoint that does not exist. A drawing model is typed rather than
 * picked, so only a preset that names one can fill that field.
 */
export function providerChoice(
  presets: readonly LlmProfilePreset[],
  profiles: readonly LlmProfile[],
  presetId: string,
  currentModel: string,
): ProviderChoice {
  if (presetId === '') return { kind: 'none' };
  const preset = presets.find((candidate) => candidate.preset === presetId);
  const existing = profiles.find((candidate) => candidate.preset === presetId);
  if (existing === undefined) return { kind: 'create', name: preset?.name ?? presetId };
  return {
    kind: 'bind',
    profileId: existing.id,
    model: currentModel === '' ? existing.model || preset?.model || '' : currentModel,
  };
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
  return ids.map((id) => ({ id, label: id }));
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
 * embeddings. The simple view binds chat and image generation, because an endpoint that answers chat
 * usually cannot draw: the picture model is its own choice and has to be named. The advanced view
 * adds embeddings, which most people can leave to the chat endpoint. Cloud use for anything but chat
 * is an explicit flag, and consent for a cloud host is given on its endpoint.
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
  /** Why the provider choice did not bind yet, e.g. the endpoint still has to be saved. */
  let providerNotice: string | undefined;
  let loading = true;
  let busy = false;
  let advanced = false;
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
      providerNotice = undefined;
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

  /** Applies a provider choice to the draft; see {@link providerChoice} for what it decides. */
  function chooseProvider(attrs: LlmRoleManagementAttrs, presetId: string): void {
    const choice = providerChoice(presets, profiles, presetId, drafts.image.model);
    if (choice.kind === 'none') return;
    if (choice.kind === 'create') {
      providerNotice = t('llmRoles', 'providerCreateHint', { name: choice.name });
      attrs.onCreateEndpoint?.(presetId);
      m.redraw();
      return;
    }
    providerNotice = undefined;
    drafts = {
      ...drafts,
      image: { ...drafts.image, profileId: choice.profileId, model: choice.model },
    };
    void loadChoices(attrs.client, choice.profileId);
    m.redraw();
  }

  function roleBlock(attrs: LlmRoleManagementAttrs, role: LlmRole) {
    const draft = drafts[role];
    const selected = profiles.find((profile) => profile.id === draft.profileId);
    const status = roleStatus(roles[role], role, profiles);
    const notice = NOTICE_KEYS[status];
    const savedRole = roles[role];
    return m('.fm-llm-role', { 'aria-label': t('llmRoles', role) }, [
      m('h5.fm-llm-role-title', t('llmRoles', role)),
      m('.fm-llm-role-fields', [
        // Only the picture role offers this: an endpoint that answers chat is rarely one that can
        // draw, so the provider of the picture model is a choice of its own.
        role === 'image'
          ? m(Select<string>, {
              label: t('llmRoles', 'provider'),
              disabled: busy || presets.length === 0,
              options: [
                { id: '', label: t('llmRoles', 'providerUnassigned') },
                ...presets.map((preset) => ({ id: preset.preset, label: preset.name })),
              ],
              checkedId: selected?.preset ?? '',
              onchange: ([value]) => {
                if (value === undefined) return;
                chooseProvider(attrs, value);
              },
            })
          : undefined,
        m(Select<string>, {
          label: t('llmRoles', 'endpoint'),
          disabled: busy,
          options: [
            { id: '', label: t('llmRoles', 'unassigned') },
            ...profiles.map((profile) => ({
              id: profile.id,
              label: profile.name,
              disabled: !canAssignRole(profile, role, draft.allowCloud),
            })),
          ],
          checkedId: draft.profileId,
          onchange: ([value]) => {
            if (value === undefined) return;
            // The endpoint's own model is the starting point; the person may pick another.
            const chosen = profiles.find((profile) => profile.id === value);
            drafts = {
              ...drafts,
              [role]: { ...draft, profileId: value, model: chosen?.model ?? '' },
            };
            if (value !== '') void loadChoices(attrs.client, value);
          },
        }),
        // A drawing model is almost never one the endpoint lists for chat, and discovery reports the
        // chat models, so the name is typed rather than picked. It is never taken from the chat role:
        // most models cannot generate images at all.
        role === 'image'
          ? m(TextInput, {
              label: t('llmRoles', 'model'),
              disabled: busy || draft.profileId === '',
              value: draft.model,
              placeholder: selected?.model ?? '',
              oninput: (value: string) => {
                drafts = { ...drafts, [role]: { ...draft, model: value } };
              },
            })
          : m(Select<string>, {
              label: t('llmRoles', 'model'),
              disabled: busy || draft.profileId === '',
              options: modelOptions(draft, choices),
              checkedId: draft.model,
              onchange: ([value]) => {
                if (value === undefined) return;
                drafts = { ...drafts, [role]: { ...draft, model: value } };
              },
            }),
        role === 'image' && draft.profileId !== ''
          ? m('p.fm-llm-role-notice', t('llmRoles', 'imageModelHint'))
          : undefined,
        role === 'image' && providerNotice !== undefined
          ? m('p.fm-llm-role-notice', { role: 'status' }, providerNotice)
          : undefined,
      ]),
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
                // The simple view keeps embeddings on the chat endpoint while that endpoint is local.
                if (role === 'chat' && !advanced && selected?.locality === 'loopback') {
                  roles = await attrs.client.setLlmRole('embeddings', {
                    profileId: draft.profileId,
                    model,
                    allowCloud: false,
                  });
                }
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
      const visibleRoles = LLM_ROLES.filter((role) => advanced || role !== 'embeddings');
      return m('.fm-llm-roles', { 'aria-label': t('llmRoles', 'title') }, [
        m('p.fm-llm-roles-intro', t('llmRoles', 'intro')),
        m(Switch, {
          label: t('llmRoles', 'advanced'),
          checked: advanced,
          disabled: busy,
          onchange: (checked: boolean) => {
            advanced = checked;
          },
        }),
        ...visibleRoles.map((role) => roleBlock(attrs, role)),
        error ? m('p.fm-llm-role-error', { role: 'alert' }, error) : undefined,
        message ? m('p.fm-llm-role-message', { role: 'status' }, message) : undefined,
      ]);
    },
  };
};
