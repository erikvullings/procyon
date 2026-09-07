import m, { type FactoryComponent } from 'mithril';
import { NumberInput, Select, Switch, TextInput } from 'mithril-materialized';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  LlmApiCapability,
  LlmProfile,
  LlmProfilePreset,
  OrphanLlmCredentialDisposition,
  SaveLlmProfileRequest,
} from '../../models';

export interface LlmProfileManagementAttrs {
  readonly client: FileManagerClient;
  readonly onSaveHandlerChange?: (handler: (() => Promise<boolean>) | undefined) => void;
}

interface EditorState {
  profileId?: string;
  request: SaveLlmProfileRequest;
  apiKey: string;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof error.message === 'string'
  ) {
    return error.message;
  }
  return t('llmProfiles', 'unknownError');
}

function requestFromPreset(preset: LlmProfilePreset): SaveLlmProfileRequest {
  return {
    name: preset.name,
    preset: preset.preset,
    baseUrl: preset.baseUrl,
    deployment: preset.deployment ?? null,
    apiVersion: preset.apiVersion ?? null,
    model: preset.model,
    credential: null,
    advanced: structuredClone(preset.advanced),
    capabilities: [...preset.capabilities],
    redactFilenames: preset.redactFilenames,
  };
}

function requestFromProfile(profile: LlmProfile): SaveLlmProfileRequest {
  return {
    name: profile.name,
    preset: profile.preset,
    baseUrl: profile.baseUrl,
    deployment: profile.deployment ?? null,
    apiVersion: profile.apiVersion ?? null,
    model: profile.model,
    credential: null,
    advanced: structuredClone(profile.advanced),
    capabilities: [...profile.capabilities],
    redactFilenames: profile.redactFilenames,
  };
}

function withCapability(
  capabilities: readonly LlmApiCapability[],
  capability: LlmApiCapability,
  enabled: boolean,
): LlmApiCapability[] {
  const next = new Set(capabilities);
  if (enabled) next.add(capability);
  else next.delete(capability);
  return [...next];
}

function downloadExport(profile: LlmProfile, value: unknown): void {
  const blob = new Blob([JSON.stringify(value, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = `${profile.name.replaceAll(/[^a-z0-9]+/gi, '-').toLowerCase() || 'llm-profile'}.json`;
  anchor.click();
  URL.revokeObjectURL(url);
}

export const LlmProfileManagement: FactoryComponent<LlmProfileManagementAttrs> = () => {
  let presets: LlmProfilePreset[] = [];
  let profiles: LlmProfile[] = [];
  let editor: EditorState | undefined;
  let selectedId: string | undefined;
  let loading = true;
  let busy = false;
  let advanced = false;
  let consent = false;
  let deleteDisposition: OrphanLlmCredentialDisposition = 'delete';
  let message: string | undefined;
  let error: string | undefined;
  let dirty = false;
  let availableModels: string[] = [];
  let modelTestRevision = 0;

  function clearAvailableModels(): void {
    modelTestRevision += 1;
    availableModels = [];
  }

  async function load(client: FileManagerClient): Promise<void> {
    loading = true;
    error = undefined;
    try {
      [presets, profiles] = await Promise.all([
        client.listLlmProfilePresets(),
        client.listLlmProfiles(),
      ]);
      if (editor === undefined) {
        const saved = profiles[0];
        const preset = presets[0];
        if (saved !== undefined) {
          selectProfile(saved);
          discoverModels(client, saved);
        } else if (preset !== undefined) {
          editor = { request: requestFromPreset(preset), apiKey: '' };
          discoverDraftModels(client);
        }
      }
    } catch (reason) {
      error = errorMessage(reason);
    } finally {
      loading = false;
      m.redraw();
    }
  }

  function selectProfile(profile: LlmProfile): void {
    selectedId = profile.id;
    editor = { profileId: profile.id, request: requestFromProfile(profile), apiKey: '' };
    dirty = false;
    clearAvailableModels();
    consent = false;
    message = undefined;
    error = undefined;
  }

  function discoverModels(client: FileManagerClient, profile: LlmProfile): void {
    if (
      profile.locality !== 'loopback' ||
      !profile.capabilities.includes('modelDiscovery') ||
      profile.preset === 'azureOpenAi'
    ) {
      return;
    }
    const revision = modelTestRevision;
    const isCurrent = () => revision === modelTestRevision && selectedId === profile.id && !dirty;
    void run(async () => {
      const models = await client.discoverLlmProfileModels(profile.id);
      if (isCurrent()) availableModels = models;
    }, isCurrent);
  }

  function discoverDraftModels(client: FileManagerClient): void {
    if (
      editor === undefined ||
      !editor.request.capabilities.includes('modelDiscovery') ||
      editor.request.preset === 'azureOpenAi'
    ) {
      return;
    }
    const revision = modelTestRevision;
    const request = {
      ...editor.request,
      credential: editor.apiKey.trim() === '' ? null : { apiKey: editor.apiKey },
    };
    const isCurrent = () => revision === modelTestRevision && selectedId === undefined;
    void run(async () => {
      const models = await client.discoverLlmProfileDraftModels(request);
      if (!isCurrent()) return;
      availableModels = models;
      if (editor !== undefined && editor.request.model.trim() === '' && models[0] !== undefined) {
        editor = {
          ...editor,
          request: { ...editor.request, model: models[0] },
        };
        dirty = true;
      }
    }, isCurrent);
  }

  function choosePreset(client: FileManagerClient, presetId: string): void {
    const preset = presets.find((candidate) => candidate.preset === presetId);
    if (preset === undefined) return;
    selectedId = undefined;
    editor = { request: requestFromPreset(preset), apiKey: '' };
    dirty = true;
    clearAvailableModels();
    consent = false;
    message = undefined;
    discoverDraftModels(client);
  }

  async function run(
    action: () => Promise<void>,
    isCurrent: () => boolean = () => true,
  ): Promise<void> {
    busy = true;
    error = undefined;
    message = undefined;
    try {
      await action();
    } catch (reason) {
      if (isCurrent()) error = errorMessage(reason);
    } finally {
      busy = false;
      m.redraw();
    }
  }

  async function save(client: FileManagerClient): Promise<boolean> {
    if (editor === undefined || !dirty) return true;
    const payload: SaveLlmProfileRequest = {
      ...editor.request,
      credential: editor.apiKey.trim() === '' ? null : { apiKey: editor.apiKey },
    };
    busy = true;
    error = undefined;
    message = undefined;
    try {
      const saved =
        editor?.profileId === undefined
          ? await client.createLlmProfile(payload)
          : await client.updateLlmProfile(editor.profileId, payload);
      const index = profiles.findIndex((profile) => profile.id === saved.id);
      if (index < 0) profiles = [...profiles, saved];
      else profiles = profiles.map((profile) => (profile.id === saved.id ? saved : profile));
      selectProfile(saved);
      discoverModels(client, saved);
      message = t('llmProfiles', 'saved');
      return true;
    } catch (reason) {
      error = errorMessage(reason);
      return false;
    } finally {
      busy = false;
      m.redraw();
    }
  }

  function updateRequest(patch: Partial<SaveLlmProfileRequest>): void {
    if (editor === undefined) return;
    modelTestRevision += 1;
    editor = { ...editor, request: { ...editor.request, ...patch } };
    dirty = true;
  }

  function updateApiKey(apiKey: string): void {
    if (editor === undefined) return;
    modelTestRevision += 1;
    editor = { ...editor, apiKey };
    dirty = true;
  }

  return {
    oninit: ({ attrs }) => {
      attrs.onSaveHandlerChange?.(() => save(attrs.client));
      void load(attrs.client);
    },
    onremove: ({ attrs }) => attrs.onSaveHandlerChange?.(undefined),
    view: ({ attrs }) => {
      if (loading) return m('p', t('llmProfiles', 'loading'));
      if (editor === undefined) return m('p', t('llmProfiles', 'unavailable'));
      const active = profiles.find((profile) => profile.id === selectedId);
      const request = editor.request;
      const customHeaders = request.advanced.customHeaders;
      const modelOptions =
        availableModels.length === 0
          ? []
          : request.model.trim() === '' || availableModels.includes(request.model)
            ? availableModels
            : [request.model, ...availableModels];
      return m('.fm-llm-profiles', { 'aria-label': t('llmProfiles', 'title') }, [
        m('p', t('llmProfiles', 'description')),
        m('.fm-llm-profile-list', [
          profiles.length === 0
            ? m('p', t('llmProfiles', 'none'))
            : profiles.map((profile) =>
                m(
                  'button.fm-semantic-action',
                  {
                    type: 'button',
                    'aria-pressed': selectedId === profile.id,
                    onclick: () => {
                      selectProfile(profile);
                      discoverModels(attrs.client, profile);
                    },
                  },
                  `${profile.name} · ${
                    profile.locality === 'loopback'
                      ? t('llmProfiles', 'local')
                      : t('llmProfiles', 'cloud')
                  }${profile.hasCredential ? ` · ${t('llmProfiles', 'credentialStored')}` : ''}`,
                ),
              ),
          m(
            'button.fm-semantic-action',
            {
              type: 'button',
              onclick: () => {
                const preset = presets[0];
                if (preset !== undefined) choosePreset(attrs.client, preset.preset);
              },
            },
            t('llmProfiles', 'newProfile'),
          ),
        ]),
        m('.row', [
          m(Select<string>, {
            className: 'col s12 m6',
            label: t('llmProfiles', 'preset'),
            options: presets.map((preset) => ({ id: preset.preset, label: preset.name })),
            checkedId: request.preset,
            onchange: ([value]) => value !== undefined && choosePreset(attrs.client, value),
          }),
          m(TextInput, {
            className: 'col s12 m6',
            label: t('llmProfiles', 'name'),
            value: request.name,
            oninput: (value: string) => updateRequest({ name: value }),
          }),
        ]),
        m('.row', [
          m(TextInput, {
            className: 'col s12 m6',
            label: t('llmProfiles', 'baseUrl'),
            value: request.baseUrl,
            oninput: (value: string) => {
              clearAvailableModels();
              updateRequest({ baseUrl: value });
            },
          }),
          modelOptions.length === 0
            ? m(TextInput, {
                className: 'col s12 m6',
                label: t('llmProfiles', 'model'),
                value: request.model,
                oninput: (value: string) => updateRequest({ model: value }),
              })
            : m(Select<string>, {
                className: 'col s12 m6',
                label: t('llmProfiles', 'model'),
                options: modelOptions.map((model) => ({ id: model, label: model })),
                checkedId: request.model,
                onchange: ([value]) => value !== undefined && updateRequest({ model: value }),
              }),
        ]),
        request.preset === 'azureOpenAi'
          ? m('.row', [
              m(TextInput, {
                className: 'col s12 m6',
                label: t('llmProfiles', 'deployment'),
                value: request.deployment ?? '',
                oninput: (value: string) => updateRequest({ deployment: value }),
              }),
              m(TextInput, {
                className: 'col s12 m6',
                label: t('llmProfiles', 'apiVersion'),
                value: request.apiVersion ?? '',
                oninput: (value: string) => updateRequest({ apiVersion: value }),
              }),
            ])
          : undefined,
        m('.row', [
          m(TextInput, {
            className: 'col s12',
            type: 'password',
            label: active?.hasCredential
              ? t('llmProfiles', 'replaceApiKey')
              : t('llmProfiles', 'apiKey'),
            value: editor.apiKey,
            autocomplete: 'off',
            oninput: updateApiKey,
          }),
        ]),
        m(
          'button.fm-semantic-action',
          { type: 'button', onclick: () => (advanced = !advanced), 'aria-expanded': advanced },
          t('llmProfiles', 'advanced'),
        ),
        advanced
          ? m('.fm-llm-advanced-settings', [
              m('.row', [
                m(NumberInput, {
                  className: 'col s6 m3',
                  label: t('llmProfiles', 'contextWindow'),
                  min: 1,
                  value: request.advanced.contextWindow,
                  oninput: (value: number) =>
                    updateRequest({ advanced: { ...request.advanced, contextWindow: value } }),
                }),
                m(NumberInput, {
                  className: 'col s6 m3',
                  label: t('llmProfiles', 'maximumAnswerTokens'),
                  min: 1,
                  value: request.advanced.maximumAnswerTokens,
                  oninput: (value: number) =>
                    updateRequest({
                      advanced: { ...request.advanced, maximumAnswerTokens: value },
                    }),
                }),
                m(NumberInput, {
                  className: 'col s6 m3',
                  label: t('llmProfiles', 'temperature'),
                  min: 0,
                  max: 2,
                  step: 0.1,
                  value: request.advanced.temperature,
                  oninput: (value: number) =>
                    updateRequest({ advanced: { ...request.advanced, temperature: value } }),
                }),
                m(NumberInput, {
                  className: 'col s6 m3',
                  label: t('llmProfiles', 'timeout'),
                  min: 1,
                  max: 600,
                  value: request.advanced.timeoutSeconds,
                  oninput: (value: number) =>
                    updateRequest({ advanced: { ...request.advanced, timeoutSeconds: value } }),
                }),
              ]),
              m(Select<string>, {
                label: t('llmProfiles', 'tlsPolicy'),
                options: [
                  {
                    id: 'requireValidCertificate',
                    label: t('llmProfiles', 'validCertificate'),
                  },
                  { id: 'requireHttps', label: t('llmProfiles', 'requireHttps') },
                ],
                checkedId: request.advanced.tlsPolicy,
                onchange: ([value]) =>
                  value !== undefined &&
                  updateRequest({
                    advanced: {
                      ...request.advanced,
                      tlsPolicy: value as SaveLlmProfileRequest['advanced']['tlsPolicy'],
                    },
                  }),
              }),
              ['openai-organization', 'openai-project', 'x-request-source'].map((header) =>
                m(TextInput, {
                  label: header,
                  value: customHeaders[header] ?? '',
                  oninput: (value: string) => {
                    const next = { ...customHeaders };
                    if (value.trim() === '') delete next[header];
                    else next[header] = value;
                    updateRequest({ advanced: { ...request.advanced, customHeaders: next } });
                  },
                }),
              ),
              m(Switch, {
                label: t('llmProfiles', 'modelDiscovery'),
                checked: request.capabilities.includes('modelDiscovery'),
                onchange: (checked: boolean) => {
                  clearAvailableModels();
                  updateRequest({
                    capabilities: withCapability(request.capabilities, 'modelDiscovery', checked),
                  });
                },
              }),
              m(Switch, {
                label: t('llmProfiles', 'responsesCapability'),
                checked: request.capabilities.includes('responses'),
                onchange: (checked: boolean) =>
                  updateRequest({
                    capabilities: withCapability(request.capabilities, 'responses', checked),
                  }),
              }),
              m(Switch, {
                label: t('llmProfiles', 'redactFilenames'),
                checked: request.redactFilenames,
                onchange: (checked: boolean) => updateRequest({ redactFilenames: checked }),
              }),
            ])
          : undefined,
        active?.locality === 'cloud'
          ? m('.fm-llm-consent', [
              m('p', t('llmProfiles', 'cloudDisclosure', { host: new URL(active.baseUrl).host })),
              m(Switch, {
                label: t('llmProfiles', 'confirmConsent'),
                checked: consent || active.consentedHost != null,
                disabled: active.consentedHost != null,
                onchange: (checked: boolean) => {
                  consent = checked;
                },
              }),
            ])
          : undefined,
        m('.fm-llm-profile-actions', [
          request.capabilities.includes('modelDiscovery') && request.preset !== 'azureOpenAi'
            ? m(
                'button.fm-semantic-action',
                {
                  type: 'button',
                  disabled: busy,
                  onclick: () => {
                    clearAvailableModels();
                    if (active !== undefined && !dirty) discoverModels(attrs.client, active);
                    else discoverDraftModels(attrs.client);
                  },
                },
                t('llmProfiles', 'discoverModels'),
              )
            : undefined,
          active === undefined
            ? undefined
            : m(
                'button.fm-semantic-action',
                {
                  type: 'button',
                  disabled: busy || dirty,
                  onclick: () => {
                    const testedProfileId = active.id;
                    clearAvailableModels();
                    const revision = modelTestRevision;
                    const isCurrent = () =>
                      revision === modelTestRevision && selectedId === testedProfileId;
                    void run(async () => {
                      const result = await attrs.client.testLlmProfile(testedProfileId);
                      if (!isCurrent()) return;
                      availableModels = result.availableModels ?? [];
                      message = result.success
                        ? t('llmProfiles', 'testSucceeded')
                        : t('llmProfiles', 'testFailed', {
                            category: result.category ?? 'transport',
                          });
                    }, isCurrent);
                  },
                },
                t('llmProfiles', 'test'),
              ),
          active === undefined
            ? undefined
            : m(
                'button.fm-semantic-action',
                {
                  type: 'button',
                  disabled:
                    busy ||
                    (active.locality === 'cloud' && active.consentedHost == null && !consent),
                  onclick: () =>
                    void run(async () => {
                      const activated = await attrs.client.activateLlmProfile(active.id, consent);
                      profiles = profiles.map((profile) =>
                        profile.id === activated.id ? activated : profile,
                      );
                      selectProfile(activated);
                      message = t('llmProfiles', 'activated');
                    }),
                },
                t('llmProfiles', 'activate'),
              ),
          active === undefined
            ? undefined
            : m(
                'button.fm-semantic-action',
                {
                  type: 'button',
                  disabled: busy,
                  onclick: () =>
                    void run(async () => {
                      const clone = await attrs.client.cloneLlmProfile(active.id);
                      profiles = [...profiles, clone];
                      selectProfile(clone);
                    }),
                },
                t('llmProfiles', 'clone'),
              ),
          active === undefined
            ? undefined
            : m(
                'button.fm-semantic-action',
                {
                  type: 'button',
                  disabled: busy,
                  onclick: () =>
                    void run(async () =>
                      downloadExport(active, await attrs.client.exportLlmProfile(active.id)),
                    ),
                },
                t('llmProfiles', 'export'),
              ),
          active === undefined
            ? undefined
            : m(Select<OrphanLlmCredentialDisposition>, {
                label: t('llmProfiles', 'credentialOnDelete'),
                options: [
                  { id: 'delete', label: t('llmProfiles', 'deleteCredential') },
                  { id: 'retain', label: t('llmProfiles', 'retainCredential') },
                ],
                checkedId: deleteDisposition,
                onchange: ([value]) => {
                  if (value !== undefined) deleteDisposition = value;
                },
              }),
          active === undefined
            ? undefined
            : m(
                'button.fm-semantic-action',
                {
                  type: 'button',
                  disabled: busy,
                  onclick: () =>
                    void run(async () => {
                      await attrs.client.deleteLlmProfile(active.id, {
                        credentialDisposition: deleteDisposition,
                      });
                      profiles = profiles.filter((profile) => profile.id !== active.id);
                      const preset = presets[0];
                      selectedId = undefined;
                      if (preset !== undefined) {
                        editor = { request: requestFromPreset(preset), apiKey: '' };
                      }
                    }),
                },
                t('button', 'delete'),
              ),
        ]),
        message === undefined ? undefined : m('p.fm-llm-message', { role: 'status' }, message),
        error === undefined ? undefined : m('p.fm-settings-save-error', { role: 'alert' }, error),
      ]);
    },
  };
};
