import type {
  ApplicationUninstallCandidate,
  ArchiveEdit,
  EntrySummary,
  FileTemplateEntry,
  FinderTag,
  Location,
} from '../../models';
import type { CreateLinkDialogRequest } from '../operations/create-link-dialog';

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

/** A nested archive whose edits are waiting for the user to save or discard them. */
export interface PendingArchiveEditPrompt {
  readonly edit: ArchiveEdit;
  /** Why the last save attempt failed, shown inside the dialog. */
  readonly error?: string;
}

export interface FinderTagsDialogRequest {
  readonly entry: EntrySummary;
  readonly tags: readonly FinderTag[];
}

export interface SpotlightCommentDialogRequest {
  readonly entry: EntrySummary;
  readonly comment: string;
}

/** Backs "Open Terminal With..." (task 0127): the directory the chosen terminal opens at. */
export interface TerminalPickerDialogRequest {
  readonly directoryUri: string;
  readonly context: import('../../models').ActionInvocationContext;
}

export interface DocumentSummaryDialogRequest {
  readonly workspaceId: string;
  readonly entry: EntrySummary;
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
  newFromTemplateOpen: boolean;
  newFromTemplateLocation: Location | undefined;
  /** The catalogue loaded for the open picker (task 0312); `undefined` until it arrives. */
  newFromTemplateEntries: readonly FileTemplateEntry[] | undefined;
  createFileLocation: Location | undefined;
  createLinkRequest: CreateLinkDialogRequest | undefined;
  archiveCreateRequest: ArchiveCreateRequest | undefined;
  multiRenameOpen: boolean;
  multiRenameEntries: readonly EntrySummary[];
  multiRenameLocation: Location | undefined;
  multiRenameExistingNames: ReadonlySet<string>;
  propertiesOpen: boolean;
  propertiesEntries: readonly EntrySummary[];
  pendingArchiveCredential: PendingArchiveCredential | undefined;
  archiveCredentialError: string | undefined;
  pendingArchiveEdit: PendingArchiveEditPrompt | undefined;
  pendingCreatedLocation: string | undefined;
  finderTagsDialog: FinderTagsDialogRequest | undefined;
  spotlightCommentDialog: SpotlightCommentDialogRequest | undefined;
  terminalPickerDialog: TerminalPickerDialogRequest | undefined;
  documentSummaryDialog: DocumentSummaryDialogRequest | undefined;
  applicationUninstallDialog: ApplicationUninstallDialogRequest | undefined;
}

export interface DialogUIController {
  getState(): DialogUIState;
  openCreateDirectory(location?: Location): void;
  cancelCreateDirectory(): void;
  confirmCreateDirectory(
    name: string,
    activeLocation: Location | undefined,
    createDirectory: (
      location: Location,
      name: string,
      createIntermediateDirectories: boolean,
    ) => Promise<void>,
    createIntermediateDirectories: boolean,
  ): void;
  openCreateFile(location?: Location): void;
  cancelCreateFile(): void;
  /** Opens the "New file from template" picker for `location` (default: the active directory). */
  openNewFromTemplate(location?: Location): void;
  cancelNewFromTemplate(): void;
  setNewFromTemplateEntries(entries: readonly FileTemplateEntry[]): void;
  confirmCreateFile(
    name: string,
    activeLocation: Location | undefined,
    createFile: (location: Location, name: string) => Promise<void>,
  ): void;
  openCreateLink(request: CreateLinkDialogRequest): void;
  cancelCreateLink(): void;
  openArchiveCreate(request: ArchiveCreateRequest): void;
  cancelArchiveCreate(): void;
  openMultiRename(
    entries: readonly EntrySummary[],
    location: Location | undefined,
    existingNames: ReadonlySet<string>,
  ): void;
  cancelMultiRename(): void;
  openProperties(entries: readonly EntrySummary[]): void;
  cancelProperties(): void;
  setPendingArchiveCredential(credential: PendingArchiveCredential | undefined): void;
  setArchiveCredentialError(error: string | undefined): void;
  clearArchiveCredential(): void;
  setPendingArchiveEdit(prompt: PendingArchiveEditPrompt | undefined): void;
  setPendingCreatedLocation(uri: string | undefined): void;
  openFinderTagsDialog(request: FinderTagsDialogRequest): void;
  cancelFinderTagsDialog(): void;
  openSpotlightCommentDialog(request: SpotlightCommentDialogRequest): void;
  cancelSpotlightCommentDialog(): void;
  openTerminalPickerDialog(request: TerminalPickerDialogRequest): void;
  cancelTerminalPickerDialog(): void;
  openDocumentSummaryDialog(request: DocumentSummaryDialogRequest): void;
  cancelDocumentSummaryDialog(): void;
  openApplicationUninstallDialog(request: ApplicationUninstallDialogRequest): void;
  cancelApplicationUninstallDialog(): void;
}

export function createDialogUIController(): DialogUIController {
  const state: DialogUIState = {
    createDirectoryOpen: false,
    createDirectoryLocation: undefined,
    createFileOpen: false,
    newFromTemplateOpen: false,
    newFromTemplateLocation: undefined,
    newFromTemplateEntries: undefined,
    createFileLocation: undefined,
    createLinkRequest: undefined,
    archiveCreateRequest: undefined,
    multiRenameOpen: false,
    multiRenameEntries: [],
    multiRenameLocation: undefined,
    multiRenameExistingNames: new Set(),
    propertiesOpen: false,
    propertiesEntries: [],
    pendingArchiveCredential: undefined,
    archiveCredentialError: undefined,
    pendingArchiveEdit: undefined,
    pendingCreatedLocation: undefined,
    finderTagsDialog: undefined,
    spotlightCommentDialog: undefined,
    terminalPickerDialog: undefined,
    documentSummaryDialog: undefined,
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

    confirmCreateDirectory(
      name,
      activeLocation,
      createDirectory,
      createIntermediateDirectories,
    ): void {
      const location = state.createDirectoryLocation ?? activeLocation;
      if (location === undefined) return;
      state.createDirectoryOpen = false;
      state.createDirectoryLocation = undefined;
      const firstDirectory = name.split('/')[0] ?? name;
      const uri = `${location.uri.replace(/\/$/u, '')}/${encodeURIComponent(firstDirectory)}`;
      state.pendingCreatedLocation = uri;
      void createDirectory(location, name, createIntermediateDirectories).catch(() => {
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

    openNewFromTemplate(location?: Location): void {
      state.newFromTemplateLocation = location;
      state.newFromTemplateEntries = undefined;
      state.newFromTemplateOpen = true;
    },

    setNewFromTemplateEntries(entries: readonly FileTemplateEntry[]): void {
      state.newFromTemplateEntries = entries;
    },

    cancelNewFromTemplate(): void {
      state.newFromTemplateOpen = false;
      state.newFromTemplateLocation = undefined;
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

    openCreateLink(request: CreateLinkDialogRequest): void {
      state.createLinkRequest = request;
    },

    cancelCreateLink(): void {
      state.createLinkRequest = undefined;
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

    setPendingArchiveEdit(prompt): void {
      state.pendingArchiveEdit = prompt;
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

    openTerminalPickerDialog(request): void {
      state.terminalPickerDialog = request;
    },

    cancelTerminalPickerDialog(): void {
      state.terminalPickerDialog = undefined;
    },

    openDocumentSummaryDialog(request): void {
      state.documentSummaryDialog = request;
    },

    cancelDocumentSummaryDialog(): void {
      state.documentSummaryDialog = undefined;
    },

    openApplicationUninstallDialog(request): void {
      state.applicationUninstallDialog = request;
    },

    cancelApplicationUninstallDialog(): void {
      state.applicationUninstallDialog = undefined;
    },
  };
}
