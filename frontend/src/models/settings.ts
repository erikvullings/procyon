import type { ComparisonCriteria, SyncMode } from './comparison';
import type { Location } from './location';
import type { SavedSearch } from './search';

/** A user-named provider-neutral favourite location. */
export interface FavouriteLocation {
  readonly label: string;
  readonly location: Location;
}

export type MultiRenameCaseTransform = 'unchanged' | 'upper' | 'lower' | 'title';

export interface MultiRenameSequence {
  readonly start: number;
  readonly step: number;
  readonly padding: number;
}

export interface MultiRenameRules {
  readonly search: string;
  readonly replace: string;
  readonly useRegex: boolean;
  readonly nameMask: string;
  readonly extensionMask: string;
  readonly sequence: MultiRenameSequence;
  readonly caseTransform: MultiRenameCaseTransform;
}

export interface MultiRenamePreset {
  readonly name: string;
  readonly rules: MultiRenameRules;
}

export type FilterKind = 'any' | 'file' | 'directory' | 'symlink';
export type FilterToggle = 'any' | 'only' | 'exclude';

/** Structured pane-filter criteria; applied to the loaded directory listing only (task 0169). */
export interface FilterCriteria {
  /** `;`-separated name masks (substring or `*`/`?` glob). */
  readonly name: string;
  readonly kind: FilterKind;
  readonly minSize: number | null;
  readonly maxSize: number | null;
  readonly modifiedWithinDays: number | null;
  readonly modifiedOlderThanDays: number | null;
  readonly hidden: FilterToggle;
  readonly executable: FilterToggle;
}

export interface FilterPreset {
  readonly name: string;
  readonly criteria: FilterCriteria;
}

/** Whether a scheduled synchronization run needs a human before anything changes (task 0161). */
export type SyncScheduleConfirmation = 'required' | 'autoApplyAdditions';

/** Opt-in schedule; fires only while the app is running. */
export interface SyncSchedule {
  readonly enabled: boolean;
  readonly intervalMinutes: number;
  readonly confirmation: SyncScheduleConfirmation;
}

export type SyncRunTrigger = 'manual' | 'scheduled';
export type SyncRunOutcome = 'planned' | 'applied' | 'upToDate' | 'dismissed' | 'failed';

/** Summary of a profile's most recent run. */
export interface SyncRunSummary {
  /** RFC 3339 timestamp. */
  readonly at: string;
  readonly trigger: SyncRunTrigger;
  readonly outcome: SyncRunOutcome;
  readonly plannedItems: number;
  readonly appliedItems: number;
  readonly message: string | null;
}

/**
 * A named synchronization pair (task 0161): saved input to the comparison and sync-plan
 * services. Locations are provider-neutral; credentials and connection tokens are never stored.
 */
export interface SyncProfile {
  readonly id: string;
  readonly name: string;
  readonly left: Location;
  readonly right: Location;
  readonly mode: SyncMode;
  readonly criteria: ComparisonCriteria;
  readonly showHidden: boolean;
  /** Glob patterns a path must match; empty admits every path. */
  readonly include: readonly string[];
  /** Glob patterns whose matches are never planned. */
  readonly exclude: readonly string[];
  readonly conflictPolicy: 'ask' | 'overwrite' | 'keepBoth' | 'skip';
  readonly schedule: SyncSchedule | null;
  readonly lastRun: SyncRunSummary | null;
}

/** Versioned application-wide settings returned by both backend transports. */
export interface Settings {
  readonly schemaVersion: number;
  readonly theme: 'auto' | 'light' | 'dark';
  readonly language: 'en' | 'nl' | 'de' | 'fr' | 'es' | 'it' | 'pt' | 'pl';
  readonly fontSize: number;
  readonly rowHeight: number;
  readonly dateFormat: 'short' | 'medium' | 'iso';
  readonly sizeFormat: 'binary' | 'decimal' | 'bytes';
  readonly showHiddenFiles: boolean;
  readonly confirmPermanentDelete: boolean;
  readonly confirmFileOperations: boolean;
  readonly defaultConflictPolicy: 'ask' | 'overwrite' | 'keepBoth' | 'skip';
  readonly operationConcurrency: number;
  readonly defaultPaneLayout: 'dual' | 'single';
  readonly defaultColumns: readonly string[];
  /** Column widths, keyed by column id, shared by every tab and pane rather than persisted per
   * tab - resizing a column in one tab is expected to apply everywhere. */
  readonly columnWidths: Readonly<Record<string, number>>;
  readonly keybindings: Readonly<Record<string, string>>;
  readonly enabledPlugins: readonly string[];
  readonly pluginSettings: Readonly<Record<string, unknown>>;
  readonly terminalCommand: string | null;
  readonly editorCommand: string | null;
  readonly defaultStartLocations: readonly string[];
  readonly favouriteLocations: readonly FavouriteLocation[];
  readonly recentLocationsByWorkspace: Readonly<Record<string, readonly Location[]>>;
  readonly multiRenamePresets: readonly MultiRenamePreset[];
  readonly filterPresets: readonly FilterPreset[];
  readonly syncProfiles: readonly SyncProfile[];
  readonly savedSearches: readonly SavedSearch[];
  readonly iconTheme: string;
  readonly checkForUpdatesAutomatically: boolean;
  /** Update checks also offer beta releases not yet approved for the stable channel. */
  readonly checkForBetaUpdates: boolean;
  /** Opens F3 in its own window instead of a tab in the active pane. */
  readonly viewerInSeparateWindow: boolean;
  /** Remembers and restores the view (sort, columns, mode) chosen for each folder (task 0270). */
  readonly rememberFolderViews: boolean;
  /** Basemap of the shared map component (task 0275). */
  readonly map: MapSettings;
}

export type MapBasemapKind = 'raster' | 'style' | 'pmtiles';

/** Where the map component gets its imagery. Only the fields of `basemapKind` are used. */
export interface MapSettings {
  /** Whether requests to remote tile or style hosts are allowed at all. */
  readonly onlineTiles: boolean;
  readonly basemapKind: MapBasemapKind;
  /** Raster tile template with `{z}`, `{x}` and `{y}`; fetched by the backend. */
  readonly urlTemplate: string;
  /** MapLibre style URL, fetched directly by the web view. */
  readonly styleUrl: string;
  /** Local `.pmtiles` raster archive. */
  readonly pmtilesLocation?: Location | null;
  readonly attribution: string;
  readonly maxZoom: number;
  /** Host that receives requests for the selected basemap, derived by the backend. */
  readonly remoteHost?: string | null;
}
