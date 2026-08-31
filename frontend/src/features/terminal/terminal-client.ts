import { Channel, invoke } from '@tauri-apps/api/core';
import type { Location } from '../../models';

export type TerminalEvent =
  | { readonly type: 'output'; readonly data: number[] }
  | { readonly type: 'exited' };

export interface TerminalClient {
  open(
    location: Location,
    columns: number,
    rows: number,
    output: (event: TerminalEvent) => void,
  ): Promise<string>;
  write(sessionId: string, data: Uint8Array): Promise<void>;
  resize(sessionId: string, columns: number, rows: number): Promise<void>;
}

/** Desktop PTY IPC boundary; the browser host deliberately has no PTY implementation. */
export const tauriTerminalClient: TerminalClient = {
  open: async (location, columns, rows, output) => {
    const channel = new Channel<TerminalEvent>();
    channel.onmessage = output;
    return invoke<string>('open_embedded_terminal', { location, columns, rows, channel });
  },
  write: (sessionId, data) => invoke('write_embedded_terminal', { sessionId, data: [...data] }),
  resize: (sessionId, columns, rows) =>
    invoke('resize_embedded_terminal', { sessionId, columns, rows }),
};
