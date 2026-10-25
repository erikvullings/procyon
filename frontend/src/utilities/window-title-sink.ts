import { getCurrentWindow } from '@tauri-apps/api/window';

import type { RuntimeKind } from './runtime';

/** Applies a title to the host window; implementations never throw. */
export type WindowTitleSink = (title: string) => void;

/**
 * Desktop windows get the native title (what the macOS Dock menu lists); browser, http and mock
 * runtimes set `document.title` for parity.
 */
export function createWindowTitleSink(kind: RuntimeKind): WindowTitleSink {
  return (title) => {
    document.title = title;
    if (kind !== 'tauri') return;
    try {
      void getCurrentWindow()
        .setTitle(title)
        .catch(() => undefined);
    } catch {
      // Not running inside a Tauri window.
    }
  };
}

/** Wraps a sink so it only fires when the title actually changed. */
export function dedupeWindowTitleSink(sink: WindowTitleSink): WindowTitleSink {
  let last: string | undefined;
  return (title) => {
    if (title === last) return;
    last = title;
    sink(title);
  };
}
