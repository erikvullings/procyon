import type { FileManagerClient } from '../api/client/file-manager-client';

/** Applies a title to the host window; implementations never throw. */
export type WindowTitleSink = (title: string) => void;

/** Routes titles through the runtime client (native title on desktop, `document.title` elsewhere). */
export function createWindowTitleSink(
  client: Pick<FileManagerClient, 'setWindowTitle'>,
): WindowTitleSink {
  return (title) => {
    void client.setWindowTitle?.(title).catch(() => undefined);
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
