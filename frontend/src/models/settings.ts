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

/** Versioned application-wide settings returned by both backend transports. */
export interface Settings {
  readonly schemaVersion: number;
  readonly theme: 'auto' | 'light' | 'dark';
  readonly language: 'en' | 'nl';
  readonly fontSize: number;
  readonly rowHeight: number;
  readonly dateFormat: 'short' | 'medium' | 'iso';
  readonly sizeFormat: 'binary' | 'decimal' | 'bytes';
  readonly showHiddenFiles: boolean;
  readonly confirmPermanentDelete: boolean;
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
  readonly savedSearches: readonly SavedSearch[];
  readonly iconTheme: string;
}
