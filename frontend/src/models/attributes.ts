import type { EntryKind } from './entry';
import type { Location } from './location';

/** Which visited entries a (recursive) attribute change touches. */
export type AttributeScope = 'both' | 'filesOnly' | 'foldersOnly';

/**
 * Fields to change; a field left out is untouched. `mode` holds Unix permission bits (rwx for user,
 * group and other, setuid, setgid and sticky: 0 through 0o7777) and `modeMask` selects which of its
 * bits apply, so a mixed selection only changes the bits the user touched.
 */
export interface AttributeChange {
  mode?: number;
  modeMask?: number;
  uid?: number;
  gid?: number;
  readOnly?: boolean;
  hidden?: boolean;
  archive?: boolean;
}

/** The change carried by a `changeAttributes` operation (task 0258). */
export interface ChangeAttributes {
  change: AttributeChange;
  recursive: boolean;
  scope: AttributeScope;
}

/** Editable state of one entry; absent fields are not exposed by its provider. */
export interface AttributeSnapshot {
  mode?: number | null;
  uid?: number | null;
  gid?: number | null;
  readOnly?: boolean | null;
  hidden?: boolean | null;
  archive?: boolean | null;
}

/** Which fields every selected entry's provider lets the user edit. */
export interface AttributeSupport {
  mode: boolean;
  ownership: boolean;
  windowsAttributes: boolean;
}

/** Why nothing can be edited for a selection. */
export type AttributeUnsupportedReason = 'providerUnsupported' | 'mixedProviders' | 'symbolicLink';

export interface AttributeEntry {
  location: Location;
  kind: EntryKind;
  attributes?: AttributeSnapshot | null;
}

export interface InspectAttributesResult {
  support: AttributeSupport;
  unsupportedReason?: AttributeUnsupportedReason | null;
  entries: AttributeEntry[];
}

/** How many entries a change would touch, shown before it is applied. */
export interface AttributeChangePreview {
  total: number;
  files: number;
  folders: number;
  skippedSymlinks: number;
  truncated: boolean;
}
