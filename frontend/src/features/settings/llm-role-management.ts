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
  ThinkingEffort,
  ThinkingSupport,
} from '../../models';
import { THINKING_EFFORTS } from '../../models/llm-profile';
import { canAssignRole, LLM_ROLES, type RoleStatus, roleStatus } from '../../models/llm-roles';
import { copyText } from '../preview/clipboard';
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
  /** The default thinking effort (chat role, task 0361). */
  thinking: ThinkingEffort;
}

/** What the chat model supports for thinking, in one line (task 0361). */
export function thinkingSupportText(support: ThinkingSupport): string {
  const parts: string[] = [];
  if (!support.thinks) parts.push(t('llmRoles', 'supportNone'));
  else if (!support.canTurnOff) parts.push(t('llmRoles', 'supportAlways'));
  else parts.push(t('llmRoles', support.defaultOn ? 'supportToggleOn' : 'supportToggleOff'));
  if (support.thinks) {
    parts.push(
      support.levels.length > 0
        ? t('llmRoles', 'supportLevels', {
            levels: support.levels.map((level) => thinkingLabel(level)).join(', '),
          })
        : t('llmRoles', support.verified ? 'supportNoLevels' : 'supportLevelsTried'),
    );
  }
  switch (support.origin) {
    case 'template':
      parts.push(
        t('llmRoles', 'supportTemplate', { date: (support.learnedAt ?? '').slice(0, 10) }),
      );
      break;
    case 'card':
      parts.push(t('llmRoles', 'supportSource', { source: support.source }));
      break;
    case 'family':
      parts.push(t('llmRoles', 'supportFamily', { source: support.source }));
      break;
  }
  return parts.join(' ');
}

/** The label of a thinking effort. */
export function thinkingLabel(effort: ThinkingEffort): string {
  switch (effort) {
    case 'auto':
      return t('llmRoles', 'thinkingAuto');
    case 'off':
      return t('llmRoles', 'thinkingOff');
    case 'low':
      return t('llmRoles', 'thinkingLow');
    case 'medium':
      return t('llmRoles', 'thinkingMedium');
    case 'high':
      return t('llmRoles', 'thinkingHigh');
  }
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

/** Whether the draft differs from what is saved; only then is there anything to save. */
function isDirty(draft: RoleDraft, saved: LlmRoleAssignment | null | undefined): boolean {
  const base = draftFrom(saved);
  return (
    draft.profileId !== base.profileId ||
    draft.model.trim() !== base.model ||
    draft.allowCloud !== base.allowCloud ||
    draft.thinking !== base.thinking
  );
}

function draftFrom(assignment: LlmRoleAssignment | null | undefined): RoleDraft {
  return assignment == null
    ? { profileId: '', model: '', allowCloud: false, thinking: 'auto' }
    : {
        profileId: assignment.profileId,
        model: assignment.model,
        allowCloud: assignment.allowCloud,
        thinking: assignment.thinking ?? 'auto',
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
  /** Roles whose model is being typed rather than picked from what the endpoint reports. */
  const typingModel: Record<LlmRole, boolean> = { chat: false, embeddings: false, image: false };
  /** Provider presets the app knows, so the picture role can name a server it has no endpoint for. */
  let presets: LlmProfilePreset[] = [];
  let loading = true;
  let busy = false;
  let loadedRevision: number | undefined;
  let message: string | undefined;
  /** The role whose save the message is about. */
  let messageRole: LlmRole | undefined;
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
      for (const role of LLM_ROLES) typingModel[role] = false;
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

  async function run(role: LlmRole, action: () => Promise<void>): Promise<void> {
    busy = true;
    error = undefined;
    message = undefined;
    messageRole = role;
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
    const dirty = isDirty(draft, savedRole);
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
        // Both models are picked from what the endpoint reports. A drawing model can also be typed,
        // because what an endpoint lists is usually what it chats with, and an endpoint that reports
        // nothing leaves only the field. Neither model is ever taken from the other role: most
        // models cannot generate images at all.
        (() => {
          const offered = modelOptions(draft, choices);
          const pickable = role !== 'image' ? offered.length > 0 : offered.length > 1;
          if (!pickable || (role === 'image' && typingModel.image)) {
            return [
              m(TextInput, {
                className: 'fm-llm-field',
                label: t('llmRoles', 'model'),
                disabled: draft.profileId === '',
                value: draft.model,
                placeholder:
                  role === 'image'
                    ? t('llmRoles', 'imageModelPlaceholder')
                    : (selected?.model ?? ''),
                oninput: (value: string) => {
                  drafts = { ...drafts, [role]: { ...draft, model: value } };
                },
              }),
              role === 'image' && offered.length > 1
                ? m(
                    'button.btn-flat.fm-llm-model-back',
                    {
                      type: 'button',
                      onclick: () => {
                        typingModel.image = false;
                      },
                    },
                    t('llmRoles', 'modelFromEndpoint'),
                  )
                : undefined,
            ];
          }
          return m(Select<string>, {
            className: 'fm-llm-field',
            label: t('llmRoles', 'model'),
            disabled: draft.profileId === '',
            options:
              role === 'image'
                ? [...offered, { id: '__type__', label: t('llmRoles', 'modelOther') }]
                : offered,
            checkedId: draft.model,
            onchange: ([value]) => {
              if (value === undefined) return;
              if (value === '__type__') {
                typingModel.image = true;
                return;
              }
              drafts = { ...drafts, [role]: { ...draft, model: value } };
            },
          });
        })(),
      ]),
      role === 'image' && draft.profileId !== ''
        ? m('p.fm-llm-role-notice', t('llmRoles', 'imageModelHint'))
        : undefined,
      // The chat role's default thinking effort; a chat or terminal session may choose its own.
      role === 'chat'
        ? m(Select<ThinkingEffort>, {
            className: 'fm-llm-field',
            label: t('llmRoles', 'thinking'),
            helperText: t('llmRoles', 'thinkingHint'),
            disabled: draft.profileId === '',
            options: THINKING_EFFORTS.map((effort) => ({
              id: effort,
              label: thinkingLabel(effort),
            })),
            checkedId: draft.thinking,
            onchange: ([value]) => {
              if (value === undefined) return;
              drafts = { ...drafts, [role]: { ...draft, thinking: value } };
            },
          })
        : undefined,
      role === 'chat' && savedRole?.thinkingSupport != null
        ? m('p.fm-llm-role-notice', thinkingSupportText(savedRole.thinkingSupport))
        : undefined,
      // Read from this server's template: offered as an entry for the curated list (task 0362).
      role === 'chat' && savedRole?.thinkingSupport?.listEntry != null
        ? m(
            'button.btn-flat',
            {
              type: 'button',
              onclick: () => void copyText(savedRole.thinkingSupport?.listEntry ?? ''),
            },
            t('llmRoles', 'copyListEntry'),
          )
        : undefined,
      // Only chat may reach the cloud without the flag; embeddings and images need it turned on.
      role !== 'chat'
        ? m(Switch, {
            // The embeddings switch speaks of indexing; a picture role sends the words typed into the
            // pane and gets the pictures back, so it says which of the two this permission covers.
            label: t('llmRoles', role === 'image' ? 'allowCloudImage' : 'allowCloud'),
            checked: draft.allowCloud,
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
              void run(role, async () => {
                roles = await attrs.client.clearLlmRole(role);
                drafts = { ...drafts, [role]: draftFrom(null) };
              }),
          },
          t('llmRoles', 'clear'),
        ),
        // Always present, so a saved or unsaved note never moves what is below it.
        m(
          'span.fm-llm-role-state',
          { role: 'status' },
          dirty
            ? t('llmRoles', 'unsaved')
            : messageRole === role && message !== undefined
              ? message
              : '',
        ),
        m(
          'button.btn-flat.fm-primary-action',
          {
            type: 'button',
            disabled: busy || !dirty || draft.profileId === '' || draft.model.trim() === '',
            onclick: () =>
              void run(role, async () => {
                const model = draft.model.trim();
                roles = await attrs.client.setLlmRole(role, {
                  profileId: draft.profileId,
                  model,
                  allowCloud: draft.allowCloud,
                  ...(role === 'chat' ? { thinking: draft.thinking } : {}),
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
      ]);
    },
  };
};
