import { Channel, invoke } from '@tauri-apps/api/core';
import type { Location } from '../../models';
import type {
  TerminalAssistEvent,
  TerminalAssistFiles,
  TerminalAssistReply,
  TerminalAssistRequest,
  TerminalAssistSkill,
} from './terminal-assist';

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
  /**
   * Asks the configured chat endpoint about the terminal (task 0333). Nothing is run. With
   * `onEvent`, each finished line of the answer and each step is reported while the model is
   * still writing, and the reply still carries the whole text.
   */
  assist(
    request: TerminalAssistRequest,
    onEvent?: (event: TerminalAssistEvent) => void,
  ): Promise<TerminalAssistReply>;
  /** Forgets one tab's assistant conversation when the tab closes (task 0336). */
  assistClose(sessionId: string): Promise<void>;
  /** Stops the request the tab's assistant is running (ESC while it works). */
  assistCancel?(sessionId: string): Promise<void>;
  /**
   * Gives the running request a message; the model reads it after its current tool calls
   * (task 0355). `false` when it was not queued.
   */
  assistSteer?(sessionId: string, text: string): Promise<boolean>;
  /** The paths under a session's root, for `@` completion (task 0343). */
  assistFiles?(root: string): Promise<TerminalAssistFiles>;
  /** The skills `/` can invoke in a folder: folder skills and notes (task 0343). */
  assistSkills?(workingDirectory: string): Promise<readonly TerminalAssistSkill[]>;
  /** Replaces a session's conversation with a model-written summary (`/compact`). */
  assistCompact?(sessionId: string): Promise<void>;
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
  assist: (request, onEvent) => {
    if (onEvent === undefined) return invoke<TerminalAssistReply>('terminal_assist', { request });
    const events = new Channel<TerminalAssistEvent>();
    events.onmessage = onEvent;
    return invoke<TerminalAssistReply>('terminal_assist_stream', { request, events });
  },
  assistClose: (sessionId) => invoke<void>('terminal_assist_close', { sessionId }),
  assistCancel: (sessionId) => invoke<void>('terminal_assist_cancel', { sessionId }),
  assistSteer: (sessionId, text) => invoke<boolean>('terminal_assist_steer', { sessionId, text }),
  assistFiles: (root) => invoke<TerminalAssistFiles>('terminal_assist_files', { root }),
  assistSkills: (workingDirectory) =>
    invoke<TerminalAssistSkill[]>('terminal_assist_skills', { workingDirectory }),
  assistCompact: (sessionId) => invoke<void>('terminal_assist_compact', { sessionId }),
};
