/** Signed application update metadata returned by a desktop host. */
export interface AppUpdateInfo {
  readonly currentVersion: string;
  readonly version: string;
  readonly date?: string;
  readonly body?: string;
}

/** Result of an explicit update check against the build's configured endpoint. */
export interface AppUpdateCheck {
  readonly currentVersion: string;
  /** Version offered online by the configured update endpoint. */
  readonly latestVersion: string;
  /** Present only when the offered version is newer and installable. */
  readonly update?: AppUpdateInfo;
}

/** Download progress emitted while a signed update is installed. */
export type AppUpdateProgress =
  | { readonly event: 'started'; readonly contentLength?: number }
  | { readonly event: 'progress'; readonly chunkLength: number }
  | { readonly event: 'finished' };
