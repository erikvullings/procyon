import m, { type FactoryComponent } from 'mithril';
import { Button, PasswordInput } from 'mithril-materialized';

import {
  fetchMutator,
  setSessionHeaderProvider,
  setUnauthorizedHandler,
} from '../api/fetch-mutator';
import { getSessionToken, setSessionToken } from '../api/session-token';
import { t } from '../i18n';
import './session-token-gate.css';

export interface SessionTokenGateAttrs {
  /** Deferred so the gated subtree (and its `oninit`/network calls) is never
   * created until the token requirement is resolved. */
  readonly children: () => m.Children;
}

/** A cheap, side-effect-free endpoint that requires a session token whenever
 * `fm-server` enforces one, and always succeeds when it doesn't (dev mode). */
const PROBE_PATH = '/api/v1/runtime';

type GateStatus = 'checking' | 'prompting' | 'ready';

/**
 * Blocks the HTTP-runtime app behind a session-token prompt (task 0064
 * backend, frontend follow-up).
 *
 * `fm-server` requires a session token on every `/api/v1` route it serves
 * (except health/docs) *unless* it was started with `--dev-mode-auth-
 * disabled` (local dev's default) — a mode that never prints a token, so
 * this component can't assume one is needed. It probes a harmless
 * authenticated endpoint once: if the server accepts it without a token,
 * auth isn't enforced and the gate steps aside permanently for this session;
 * if it's rejected, the token prompt is shown. This is the one place the
 * token is collected, stored, and wired into every outgoing request (REST
 * via `setSessionHeaderProvider`, SSE via `HttpFileManagerClient`'s
 * `tokenProvider`). A `401` response anywhere later clears the token and
 * returns here, since it means the token was never valid or the server
 * restarted with a new one (tokens don't survive a restart, spec §22).
 */
export const SessionTokenGate: FactoryComponent<SessionTokenGateAttrs> = () => {
  const state = {
    status: (getSessionToken() !== undefined ? 'ready' : 'checking') as GateStatus,
    draft: '',
    error: undefined as string | undefined,
  };

  setSessionHeaderProvider(() => {
    const token = getSessionToken();
    return token === undefined ? undefined : { name: 'Authorization', value: `Bearer ${token}` };
  });
  setUnauthorizedHandler(() => {
    if (state.status !== 'ready' || getSessionToken() === undefined) return;
    setSessionToken(undefined);
    state.status = 'prompting';
    state.error = t('sessionGate', 'rejectedError');
    m.redraw();
  });

  if (state.status === 'checking') {
    fetchMutator(PROBE_PATH)
      .then(() => {
        state.status = 'ready';
      })
      .catch(() => {
        state.status = 'prompting';
      })
      .finally(m.redraw);
  }

  function submit(event: Event): void {
    event.preventDefault();
    const trimmed = state.draft.trim();
    if (trimmed.length === 0) return;
    setSessionToken(trimmed);
    state.draft = '';
    state.error = undefined;
    state.status = 'ready';
  }

  return {
    view: ({ attrs }) => {
      if (state.status === 'ready') return attrs.children();
      if (state.status === 'checking') return null;
      return m('.fm-session-token-gate', [
        m('form.fm-session-token-gate__card', { onsubmit: submit }, [
          m('h1', t('sessionGate', 'heading')),
          m('p', t('sessionGate', 'message')),
          state.error === undefined ? undefined : m('p.fm-session-token-gate__error', state.error),
          m(PasswordInput, {
            label: t('sessionGate', 'accessToken'),
            value: state.draft,
            oninput: (value: string) => {
              state.draft = value;
            },
          }),
          m('.fm-session-token-gate__actions', [
            m(Button, {
              label: t('sessionGate', 'continue'),
              variant: 'submit',
              disabled: state.draft.trim().length === 0,
            }),
          ]),
        ]),
      ]);
    },
  };
};
