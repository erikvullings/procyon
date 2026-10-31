import type { Location } from '../../models';

/** Raised by an adapter method with no implementation yet for the current milestone; carries the task number that will complete it (spec §12). */
export class NotImplementedError extends Error {
  constructor(methodName: string, taskNumber: string) {
    super(
      `${methodName} is not implemented until task ${taskNumber}; see TASKS/${taskNumber}-*.md`,
    );
    this.name = 'NotImplementedError';
  }
}

export interface OpenWithApplication {
  readonly name: string;
  readonly path: string;
}

/** A native file-reference drop reported by the desktop window. */
export interface NativeFileDrop {
  readonly locations: readonly Location[];
  readonly position: { readonly x: number; readonly y: number };
}

export interface PluginPanelBounds {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

/** Resolved theme colours pushed to the native window frame. */
export interface WindowChrome {
  background: string;
  foreground: string;
  theme: 'light' | 'dark' | null;
}
