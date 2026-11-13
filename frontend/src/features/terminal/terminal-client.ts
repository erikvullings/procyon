import { Channel, invoke } from '@tauri-apps/api/core';
import type { Location } from '../../models';
import type { TerminalAssistReply, TerminalAssistRequest } from './terminal-assist';

export type TerminalEvent =
  | { readonly type: 'output'; readonly data: number[] }
  | { readonly type: 'exited' };

export interface TerminalClient {
  /** Starts (or reuses) the location's shell. `shellIntegration` wraps zsh and bash with the
   * command-block hooks (task 0333); it only applies when the shell starts. */
  open(
    location: Location,
    columns: number,
    rows: number,
    output: (event: TerminalEvent) => void,
    shellIntegration: boolean,
  ): Promise<string>;
  /** Ends one session, so its shell can restart with different startup settings. */
  close(sessionId: string): Promise<boolean>;
  write(sessionId: string, data: Uint8Array): Promise<void>;
  resize(sessionId: string, columns: number, rows: number): Promise<void>;
  /** Asks the configured chat endpoint about the terminal (task 0333). Nothing is run. */
  assist(request: TerminalAssistRequest): Promise<TerminalAssistReply>;
}

/** Desktop PTY IPC boundary; the browser host deliberately has no PTY implementation. */
export const tauriTerminalClient: TerminalClient = {
  open: async (location, columns, rows, output, shellIntegration) => {
    const channel = new Channel<TerminalEvent>();
    channel.onmessage = output;
    return invoke<string>('open_embedded_terminal', {
      location,
      columns,
      rows,
      shellIntegration,
      channel,
    });
  },
  close: (sessionId) => invoke<boolean>('close_embedded_terminal', { sessionId }),
  write: (sessionId, data) => invoke('write_embedded_terminal', { sessionId, data: [...data] }),
  resize: (sessionId, columns, rows) =>
    invoke('resize_embedded_terminal', { sessionId, columns, rows }),
  assist: (request) => invoke<TerminalAssistReply>('terminal_assist', { request }),
};
