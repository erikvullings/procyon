import type { ActionId, EntryId, OperationId, PaneId, PluginId } from './ids';

/** A keyboard shortcut assigned to an action (spec §18). */
export interface KeyChord {
  key: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  meta?: boolean;
}

/** Left as a plain string until task 0052 defines the concrete category set. */
export type ActionCategory = string;

/** Whether an action is built in or contributed by a plugin (spec §18). */
export type ActionSource = { kind: 'core' } | { kind: 'plugin'; pluginId: PluginId };

/** Left opaque until task 0052 defines the concrete availability rules. */
export interface ActionContextRequirements {
  readonly featureAvailable?: boolean;
  readonly requiresSelection?: boolean;
  readonly requiresSingleSelection?: boolean;
}

/**
 * Typed context supplied with an action invocation: the active pane, the
 * current selection, and the cursor entry (spec §18). The backend
 * re-validates `ActionContextRequirements` against this rather than trusting
 * the frontend's own advisory availability evaluation.
 */
export interface ActionInvocationContext {
  paneId?: PaneId;
  selectedEntryIds?: EntryId[];
  cursorEntryId?: EntryId;
}

/**
 * Describes an invokable action (spec §18). No backend DTO exists yet
 * (actions land in task 0052); fields mirror the domain `ActionDescriptor`
 * struct until then.
 */
export interface ActionDescriptor {
  id: ActionId;
  title: string;
  description?: string;
  category: ActionCategory;
  defaultShortcuts: KeyChord[];
  contextRequirements: ActionContextRequirements;
  parameterSchema?: unknown;
  source: ActionSource;
}

/** Result of invoking one action (spec §18). */
export interface ActionResult {
  actionId: ActionId;
  invoked: boolean;
  operationId?: OperationId;
}
