/**
 * Strongly typed identifiers (spec §5), mirroring `fm_domain::ids`.
 *
 * Every identifier is `#[serde(transparent)]` on the Rust side, so each one
 * serializes to a plain JSON string; a type alias is enough here rather than
 * a branded type, and keeps these aligned with what Orval will generate once
 * the owning DTOs exist.
 */

/** Identifies a {@link import('./workspace').Workspace}. */
export type WorkspaceId = string;

/** Identifies a pane within a workspace. */
export type PaneId = string;

/** Identifies a tab within a pane. */
export type TabId = string;

/** Identifies a directory entry (file, directory or symlink). */
export type EntryId = string;

/** Identifies a long-running file operation (copy, move, delete, ...). */
export type OperationId = string;

/** Identifies a location's virtual filesystem provider, e.g. `"file"`. */
export type ProviderId = string;

/** Identifies a plugin manifest. */
export type PluginId = string;

/** Identifies a registered action (built-in or plugin-provided). */
export type ActionId = string;

/** Identifies a {@link import('./connection').Connection}. */
export type ConnectionId = string;
