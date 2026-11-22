import m, { type FactoryComponent } from 'mithril';

import type { SemanticClient } from '../../api/client/semantic-client.ts';
import { t } from '../../i18n';

export interface ToolCallingStatusAttrs {
  readonly client: SemanticClient;
}

type Status = 'unknown' | 'noChat' | 'pending' | 'yes' | 'no';

/**
 * Read-only: whether the model that answers chat and terminal questions takes native tool calls
 * (task 0339). The app finds this out itself, in the background, whenever the endpoint or model
 * changes; until then, and when the model does not take them, tools use the text protocol.
 */
export const ToolCallingStatus: FactoryComponent<ToolCallingStatusAttrs> = () => {
  let status: Status = 'unknown';

  async function load(client: SemanticClient): Promise<void> {
    try {
      const [profiles, roles] = await Promise.all([
        client.listLlmProfiles(),
        client.listLlmRoles(),
      ]);
      const assignment = roles.chat;
      const profile = profiles.find((candidate) => candidate.id === assignment?.profileId);
      if (assignment == null || profile === undefined) {
        status = 'noChat';
      } else if (profile.toolCalling?.model !== assignment.model) {
        status = 'pending';
      } else {
        status = profile.toolCalling.supported ? 'yes' : 'no';
      }
    } catch {
      status = 'unknown';
    }
    m.redraw();
  }

  return {
    oninit: ({ attrs }) => void load(attrs.client),
    view: () =>
      status === 'unknown'
        ? undefined
        : m('.row', [
            m('.col.s12.fm-settings-tool-calling', { role: 'status' }, [
              m('span', `${t('settings', 'toolCalling')}: `),
              m('strong', t('settings', `toolCalling_${status}`)),
              m('p.helper-text', t('settings', 'toolCallingHelp')),
            ]),
          ]),
  };
};
