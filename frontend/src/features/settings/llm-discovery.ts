import m, { type FactoryComponent } from 'mithril';

import type { SemanticClient } from '../../api/client/semantic-client';
import { t } from '../../i18n';
import type {
  DetectedLlmServer,
  LlmDiscoveryChoice,
  LlmDiscoveryStatus,
  LlmPreset,
} from '../../models';

export interface LlmDiscoveredEndpoint {
  readonly preset: LlmPreset;
  readonly baseUrl: string;
  readonly model: string;
}

interface LlmDiscoveryAttrs {
  readonly client: SemanticClient;
  readonly firstRun?: boolean;
  readonly settingsOpen?: boolean;
  readonly endpointRevision?: number;
  readonly onConnectCloud: () => void;
  readonly onConfigured: (endpoint: LlmDiscoveredEndpoint) => void;
}

export const LlmDiscovery: FactoryComponent<LlmDiscoveryAttrs> = () => {
  const lifetime = new AbortController();
  let status: LlmDiscoveryStatus | undefined;
  let open = false;
  let statusRequest = 0;
  let probing = false;
  let saving = false;
  let error = false;
  let servers: DetectedLlmServer[] = [];
  const models = new Map<LlmPreset, string>();

  async function detect(client: SemanticClient): Promise<void> {
    open = true;
    probing = true;
    error = false;
    servers = [];
    models.clear();
    try {
      servers = await client.detectLocalLlmServers(lifetime.signal);
      for (const server of servers) {
        const model = server.models[0];
        if (model !== undefined) models.set(server.preset, model);
      }
    } catch {
      error = true;
    } finally {
      probing = false;
      if (!lifetime.signal.aborted) m.redraw();
    }
  }

  async function choose(
    attrs: LlmDiscoveryAttrs,
    choice: LlmDiscoveryChoice,
    after?: () => void,
  ): Promise<void> {
    if (saving) return;
    saving = true;
    error = false;
    try {
      await attrs.client.chooseLlmDiscovery(choice, lifetime.signal);
      if (lifetime.signal.aborted) return;
      open = false;
      if (status !== undefined) status = { ...status, showBanner: false };
      after?.();
      if (!attrs.firstRun && !lifetime.signal.aborted) {
        await refreshStatus(attrs);
      }
    } catch {
      error = true;
    } finally {
      saving = false;
      if (!lifetime.signal.aborted) m.redraw();
    }
  }

  function button(label: string, onclick: () => void, disabled = saving): m.Vnode {
    return m('button.btn-flat', { type: 'button', disabled, onclick }, label);
  }

  function panel(attrs: LlmDiscoveryAttrs): m.Children {
    return [
      probing ? m('p', { role: 'status' }, t('llmDiscovery', 'checking')) : null,
      !probing && servers.length === 0 && !error
        ? m(
            'p',
            t('llmDiscovery', status?.loopbackAvailable === false ? 'loopbackDisabled' : 'none'),
          )
        : null,
      servers.map((server) =>
        m('.fm-llm-discovery-server', { key: server.preset }, [
          m('strong', server.name),
          m('div', server.baseUrl),
          server.models.length > 0
            ? [
                m('label.fm-semantic-field', [
                  t('llmDiscovery', 'model'),
                  m(
                    'select.browser-default',
                    {
                      disabled: saving,
                      value: models.get(server.preset),
                      onchange: (event: Event) => {
                        models.set(server.preset, (event.currentTarget as HTMLSelectElement).value);
                      },
                    },
                    server.models.map((model) => m('option', { key: model, value: model }, model)),
                  ),
                ]),
                button(t('llmDiscovery', 'use'), () => {
                  const model = models.get(server.preset);
                  if (model !== undefined) {
                    void choose(attrs, { kind: 'use', preset: server.preset, model }, () =>
                      attrs.onConfigured({ preset: server.preset, baseUrl: server.baseUrl, model }),
                    );
                  }
                }),
              ]
            : m('p', t('llmDiscovery', 'noModels')),
        ]),
      ),
      m('.fm-llm-discovery-actions', [
        button(
          t('llmDiscovery', 'cloud'),
          () => void choose(attrs, { kind: 'complete' }, attrs.onConnectCloud),
        ),
        button(t('llmDiscovery', 'later'), () => void choose(attrs, { kind: 'later' })),
      ]),
    ];
  }

  async function refreshStatus(attrs: LlmDiscoveryAttrs, automatic = false): Promise<void> {
    const request = ++statusRequest;
    try {
      const result = await attrs.client.getLlmDiscoveryStatus(lifetime.signal);
      if (lifetime.signal.aborted || request !== statusRequest) return;
      status = result;
      if (attrs.firstRun && !result.firstRunPending) open = false;
      if (automatic && result.firstRunPending) void detect(attrs.client);
    } catch {
      if (!attrs.firstRun && request === statusRequest) error = true;
    } finally {
      if (!lifetime.signal.aborted) m.redraw();
    }
  }

  return {
    oninit: ({ attrs }) => void refreshStatus(attrs, attrs.firstRun),
    onbeforeupdate: ({ attrs }, { attrs: previous }) => {
      if (
        attrs.endpointRevision !== previous.endpointRevision ||
        (previous.settingsOpen && !attrs.settingsOpen)
      ) {
        void refreshStatus(attrs);
      }
    },
    onremove: () => lifetime.abort(),
    view: ({ attrs }) => {
      const errorMessage = error ? m('p', { role: 'alert' }, t('llmDiscovery', 'failed')) : null;
      if (attrs.firstRun) {
        return open && !attrs.settingsOpen
          ? m('aside.fm-first-run-llm', { 'aria-label': t('llmDiscovery', 'title') }, [
              m('strong', t('llmDiscovery', 'title')),
              errorMessage,
              panel(attrs),
            ])
          : null;
      }
      return m('section.fm-llm-discovery', [
        status?.showBanner
          ? m('p', [
              t('llmDiscovery', 'reminder'),
              button(t('llmDiscovery', 'dismiss'), () => void choose(attrs, { kind: 'complete' })),
            ])
          : null,
        errorMessage,
        m('.fm-llm-discovery-actions', [
          button(t('llmDiscovery', 'detect'), () => void detect(attrs.client), probing || saving),
          button(
            t('llmDiscovery', 'reset'),
            () => void choose(attrs, { kind: 'reset' }),
            probing || saving,
          ),
        ]),
        open ? panel(attrs) : null,
      ]);
    },
  };
};
