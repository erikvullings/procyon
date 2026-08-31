import type {
  ApplicationUninstallCandidate,
  EntrySummary,
  FinderTag,
  Location,
} from '../../models';

export interface ArchiveCreateRequest {
  readonly sources: readonly Location[];
  readonly destinationDirectory: Location;
  readonly moveSources: boolean;
}

export interface PendingArchiveCredential {
  readonly location: Location;
  readonly invalid: boolean;
  readonly resolve: (supplied: boolean) => void;
}

export interface FinderTagsDialogRequest {
  readonly entry: EntrySummary;
  readonly tags: readonly FinderTag[];
}

export interface SpotlightCommentDialogRequest {
  readonly entry: EntrySummary;
  readonly comment: string;
}

/** Backs the uninstall review checklist (task 0148): the `.app` bundle being uninstalled plus
 * what discovery found for it, held until the user confirms or cancels. */
export interface ApplicationUninstallDialogRequest {
  readonly bundle: EntrySummary;
  readonly productName: string;
  readonly relatedFiles: readonly ApplicationUninstallCandidate[];
}

export interface DialogUIState {
  createDirectoryOpen: boolean;
  createDirectoryLocation: Location | undefined;
  createFileOpen: boolean;
  createFileLocation: Location | undefined;
  archiveCreateRequest: ArchiveCreateRequest | undefined;
  multiRenameOpen: boolean;
  multiRenameEntries: readonly EntrySummary[];
  multiRenameLocation: Location | undefined;
  multiRenameExistingNames: ReadonlySet<string>;
  propertiesOpen: boolean;
  propertiesEntries: readonly EntrySummary[];
  pendingArchiveCredential: PendingArchiveCredential | undefined;
  archiveCredentialError: string | undefined;
  pendingCreatedLocation: string | undefined;
  finderTagsDialog: FinderTagsDialogRequest | undefined;
  spotlightCommentDialog: SpotlightCommentDialogRequest | undefined;
  applicationUninstallDialog: ApplicationUninstallDialogRequest | undefined;
}

export interface DialogUIController {
  getState(): DialogUIState;
  openCreateDirectory(location?: Location): void;
  cancelCreateDirectory(): void;
  confirmCreateDirectory(
    name: string,
    activeLocation: Location | undefined,
    createDirectory: (location: Location, name: string) => Promise<void>,
  ): void;
  openCreateFile(location?: Location): void;
  cancelCreateFile(): void;
  confirmCreateFile(
    name: string,
    activeLocation: Location | undefined,
    createFile: (location: Location, name: string) => Promise<void>,
  ): void;
  openArchiveCreate(request: ArchiveCreateRequest): void;
  cancelArchiveCreate(): void;
  openMultiRename(
    entries: readonly EntrySummary[],
    location: Location,
    existingNames: ReadonlySet<string>,
  ): void;
  cancelMultiRename(): void;
  openProperties(entries: readonly EntrySummary[]): void;
  cancelProperties(): void;
  setPendingArchiveCredential(credential: PendingArchiveCredential | undefined): void;
  setArchiveCredentialError(error: string | undefined): void;
  clearArchiveCredential(): void;
  setPendingCreatedLocation(uri: string | undefined): void;
  openFinderTagsDialog(request: FinderTagsDialogRequest): void;
  cancelFinderTagsDialog(): void;
  openSpotlightCommentDialog(request: SpotlightCommentDialogRequest): void;
  cancelSpotlightCommentDialog(): void;
  openApplicationUninstallDialog(request: ApplicationUninstallDialogRequest): void;
  cancelApplicationUninstallDialog(): void;
}

export function createDialogUIController(): DialogUIController {
  const state: DialogUIState = {
    createDirectoryOpen: false,
    createDirectoryLocation: undefined,
    createFileOpen: false,
    createFileLocation: undefined,
    archiveCreateRequest: undefined,
    multiRenameOpen: false,
    multiRenameEntries: [],
    multiRenameLocation: undefined,
    multiRenameExistingNames: new Set(),
    propertiesOpen: false,
    propertiesEntries: [],
    pendingArchiveCredential: undefined,
    archiveCredentialError: undefined,
    pendingCreatedLocation: undefined,
    finderTagsDialog: undefined,
    spotlightCommentDialog: undefined,
    applicationUninstallDialog: undefined,
  };

  return {
    getState: () => state,

    openCreateDirectory(location?: Location): void {
      state.createDirectoryLocation = location;
      state.createDirectoryOpen = true;
    },

    cancelCreateDirectory(): void {
      state.createDirectoryOpen = false;
      state.createDirectoryLocation = undefined;
    },

    confirmCreateDirectory(name, activeLocation, createDirectory): void {
      const location = state.createDirectoryLocation ?? activeLocation;
      if (location === undefined) return;
      state.createDirectoryOpen = false;
      state.createDirectoryLocation = undefined;
      const uri = `${location.uri.replace(/\/$/u, '')}/${encodeURIComponent(name)}`;
      state.pendingCreatedLocation = uri;
      void createDirectory(location, name).catch(() => {
        state.pendingCreatedLocation = undefined;
      });
    },

    openCreateFile(location?: Location): void {
      state.createFileLocation = location;
      state.createFileOpen = true;
    },

    cancelCreateFile(): void {
      state.createFileOpen = false;
      state.createFileLocation = undefined;
    },

    confirmCreateFile(name, activeLocation, createFile): void {
      const location = state.createFileLocation ?? activeLocation;
      if (location === undefined) return;
      state.createFileOpen = false;
      state.createFileLocation = undefined;
      const uri = `${location.uri.replace(/\/$/u, '')}/${encodeURIComponent(name)}`;
      state.pendingCreatedLocation = uri;
      void createFile(location, name).catch(() => {
        state.pendingCreatedLocation = undefined;
      });
    },

    openArchiveCreate(request: ArchiveCreateRequest): void {
      state.archiveCreateRequest = request;
    },

    cancelArchiveCreate(): void {
      state.archiveCreateRequest = undefined;
    },

    openMultiRename(entries, location, existingNames): void {
      state.multiRenameOpen = true;
      state.multiRenameEntries = entries;
      state.multiRenameLocation = location;
      state.multiRenameExistingNames = existingNames;
    },

    cancelMultiRename(): void {
      state.multiRenameOpen = false;
      state.multiRenameEntries = [];
      state.multiRenameLocation = undefined;
      state.multiRenameExistingNames = new Set();
    },

    openProperties(entries): void {
      state.propertiesOpen = true;
      state.propertiesEntries = entries;
    },

    cancelProperties(): void {
      state.propertiesOpen = false;
      state.propertiesEntries = [];
    },

    setPendingArchiveCredential(credential): void {
      state.pendingArchiveCredential = credential;
    },

    setArchiveCredentialError(error): void {
      state.archiveCredentialError = error;
    },

    clearArchiveCredential(): void {
      state.pendingArchiveCredential = undefined;
      state.archiveCredentialError = undefined;
    },

    setPendingCreatedLocation(uri): void {
      state.pendingCreatedLocation = uri;
    },

    openFinderTagsDialog(request): void {
      state.finderTagsDialog = request;
    },

    cancelFinderTagsDialog(): void {
      state.finderTagsDialog = undefined;
    },

    openSpotlightCommentDialog(request): void {
      state.spotlightCommentDialog = request;
    },

    cancelSpotlightCommentDialog(): void {
      state.spotlightCommentDialog = undefined;
    },

    openApplicationUninstallDialog(request): void {
      state.applicationUninstallDialog = request;
    },

    cancelApplicationUninstallDialog(): void {
      state.applicationUninstallDialog = undefined;
    },
  };
}
