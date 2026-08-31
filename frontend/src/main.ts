import 'mithril-materialized/core.css';
import 'mithril-materialized/forms.css';
import 'mithril-materialized/components.css';
import 'mithril-materialized/utilities.css';
import './themes/theme.css';
import './themes/mithril-materialized-procyon.css';

import m from 'mithril';

import { createFileManagerClient } from './api/client/create-client';
import { AppShell } from './app/app-shell';
import { SessionTokenGate } from './app/session-token-gate';
import { resolveRuntimeKind } from './utilities/runtime';

const runtime = resolveRuntimeKind(import.meta.env.VITE_RUNTIME);
const client = createFileManagerClient(runtime);

const root = document.getElementById('app');
if (root === null) {
  throw new Error('index.html is missing the #app mount point');
}

// Only the HTTP runtime talks to fm-server's authenticated `/api/v1`
// surface (task 0064); the mock and Tauri runtimes have no session token to
// collect, so they mount `AppShell` directly.
m.mount(root, {
  view: () =>
    runtime === 'http'
      ? m(SessionTokenGate, { children: () => m(AppShell, { runtime, client }) })
      : m(AppShell, { runtime, client }),
});

if (import.meta.hot !== undefined) {
  import.meta.hot.dispose(() => {
    m.mount(root, null);
    client.disconnect();
  });
}
