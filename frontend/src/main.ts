import 'mithril-materialized/core.css';
import 'mithril-materialized/forms.css';
import 'mithril-materialized/components.css';
import 'mithril-materialized/utilities.css';
import 'mithril-materialized/presets/compact-minimal.css';
import './themes/theme.css';
import './themes/mithril-materialized-procyon.css';

import m from 'mithril';

import { createFileManagerClient } from './api/client/create-client';
import { AppShell } from './app/app-shell';
import { SessionTokenGate } from './app/session-token-gate';
import { ViewerWindow } from './app/viewer-window';
import { installFrontendDiagnostics } from './features/diagnostics/frontend-diagnostics';
import { decodeViewerRequest } from './features/preview/viewer-window';
import { resolveRuntimeKind } from './utilities/runtime';

const runtime = resolveRuntimeKind(import.meta.env.VITE_RUNTIME);
const client = createFileManagerClient(runtime);
const frontendDiagnostics = installFrontendDiagnostics(client, {
  deferUntilReady: runtime === 'http',
});

const root = document.getElementById('app');
if (root === null) {
  throw new Error('index.html is missing the #app mount point');
}
root.dataset.mmPreset = 'compact-minimal';

// Only the HTTP runtime talks to fm-server's authenticated `/api/v1`
// surface (task 0064); the mock and Tauri runtimes have no session token to
// collect, so they mount `AppShell` directly.
// A `?viewer=` URL is a separate Lister window (viewer-in-own-window setting): it mounts only the
// F3 viewer for that one file instead of the full app shell.
const viewerRequest = decodeViewerRequest(globalThis.location.search);
const mainView = (): m.Children =>
  viewerRequest === undefined
    ? m(AppShell, { runtime, client })
    : m(ViewerWindow, { client, request: viewerRequest });

m.mount(root, {
  view: () =>
    runtime === 'http'
      ? m(SessionTokenGate, {
          children: mainView,
          onReady: frontendDiagnostics.flush,
        })
      : mainView(),
});

if (import.meta.hot !== undefined) {
  import.meta.hot.dispose(() => {
    m.mount(root, null);
    frontendDiagnostics.uninstall();
    client.disconnect();
  });
}
