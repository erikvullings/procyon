/** Signed application update metadata returned by a desktop host. */
export interface AppUpdateInfo {
  readonly currentVersion: string;
  readonly version: string;
  readonly date?: string;
  readonly body?: string;
}

/** Download progress emitted while a signed update is installed. */
export type AppUpdateProgress =
  | { readonly event: 'started'; readonly contentLength?: number }
  | { readonly event: 'progress'; readonly chunkLength: number }
  | { readonly event: 'finished' };
