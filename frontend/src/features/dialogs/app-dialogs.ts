import m from 'mithril';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  Connection,
  ConnectionId,
  Location,
  OperationConflict,
  OperationId,
  PaneId,
  SavedSearch,
  Settings,
  TabId,
} from '../../models';
import { ConnectionsManager } from '../connections/connection-editor';
import {
  acceptSshHostKey as acceptSshHostKeyRequest,
  beginOneDriveAuthorization as beginOneDriveAuthorizationRequest,
  cancelOneDriveAuthorization as cancelOneDriveAuthorizationRequest,
  connectConnection as connectConnectionRequest,
  deleteConnection as deleteConnectionRequest,
  disconnectConnection as disconnectConnectionRequest,
  getOneDriveAuthorizationAttempt as getOneDriveAuthorizationAttemptRequest,
  isBrowsable,
  loadConnections,
  probeSshHostKey as probeSshHostKeyRequest,
  remoteRootLocation,
  saveConnection,
  testConnection as testConnectionRequest,
  upsertConnection,
  withoutConnection,
} from '../connections/connections-model';
import type { DialogUIController } from '../dialogs/dialog-ui-controller';
import type { FinderTagsLoader } from '../directory-table/finder-tags-loader';
import type { EntryFormatSettings } from '../entry-formatting/entry-formatting';
import { FinderTagsDialog } from '../entry-metadata/finder-tags-dialog';
import { SpotlightCommentDialog } from '../entry-metadata/spotlight-comment-dialog';
import { ArchivePasswordDialog } from '../navigation/archive-password-dialog';
import { ApplicationUninstallDialog } from '../operations/application-uninstall-dialog';
import { ArchiveCreateDialog, type ArchiveFormat } from '../operations/archive-create-dialog';
import { ConflictDialog } from '../operations/conflict-dialog';
import { CreateDirectoryDialog } from '../operations/create-directory-dialog';
import { CreateFileDialog } from '../operations/create-file-dialog';
import { MultiRenameDialog } from '../operations/multi-rename-dialog';
import { OperationCentre } from '../operations/operation-centre';
import {
  dismissOperation,
  type OperationCentreState,
  transitionOperationState,
} from '../operations/operation-state';
import type { OperationsController } from '../operations/operations-controller';
import { PermanentDeleteDialog } from '../operations/permanent-delete-dialog';
import { CloseLastTabDialog } from '../panes/close-last-tab-dialog';
import type { TabController } from '../panes/tab-controller';
import { PropertiesDialog } from '../properties/properties-dialog';
import {
  type FindFilesController,
  type SavedSearchOpenTarget,
  searchQueryFromParams,
} from '../search/find-files-controller';
import type { FindFilesSearchParams } from '../search/find-files-dialog';
import { FindFilesDialog } from '../search/find-files-dialog';
import { deleteSavedSearch, saveSearch, toggleSavedSearchPin } from '../search/saved-searches';
import { pathFromUri } from '../workspace/workspace-layout';

export interface AppDialogsContext {
  getOperationCentreVisible(): boolean;
  toggleOperationCentre(): void;
  getOperations(): OperationCentreState;
  setOperations(next: OperationCentreState): void;
  getPendingConflict(): OperationConflict | undefined;
  setPendingConflict(conflict: OperationConflict | undefined): void;
  getConnections(): readonly Connection[];
  setConnections(conns: readonly Connection[]): void;
  getConnectionsManagerOpen(): boolean;
  setConnectionsManagerOpen(open: boolean): void;
  getFindFilesOpen(): boolean;
  getFindFilesRoot(): Location | undefined;
  getFindFilesError(): string | undefined;
  getCloseTabConfirmation(): { readonly paneId: PaneId; readonly tabId: TabId } | undefined;
  setCloseTabConfirmation(conf?: { readonly paneId: PaneId; readonly tabId: TabId }): void;
  getDialogs(): DialogUIController;
  getFinderTagsLoader(): FinderTagsLoader | undefined;
  getFormatSettings(): EntryFormatSettings;
  getFindFilesController(): FindFilesController;
  getTabController(): TabController;
  getOpsController(): OperationsController;
  getActiveDirectoryLocation(): Location | undefined;
  getActivePaneId(): PaneId | undefined;
  navigateActiveLocation(location: Location): Promise<void>;
  getFocusPane(): ((paneId: PaneId) => void) | undefined;
  getSettings(): Settings | undefined;
  updateSettings(update: (settings: Settings) => Settings): Promise<void>;
  /** Opens the just-created file (Shift+F4) in the active pane's editor. */
  openEditorForCreatedFile(location: Location, name: string): void;
  cancelAutoDismiss(operationId: OperationId): void;
  rememberDismissedOperation(operationId: OperationId): void;
  hasDismissedOperations(): boolean;
  showAllOperations(): void;
  refetchAffectedPanes(): void;
  redraw(): void;
}

/** Opens a newly created, immediately browsable connection in the active pane. */
export async function openCreatedConnection(
  connection: Connection,
  editingId: ConnectionId | undefined,
  ctx: Pick<AppDialogsContext, 'setConnectionsManagerOpen' | 'navigateActiveLocation' | 'redraw'>,
): Promise<void> {
  if (editingId !== undefined || !isBrowsable(connection)) return;
  ctx.setConnectionsManagerOpen(false);
  ctx.redraw();
  await ctx.navigateActiveLocation(remoteRootLocation(connection));
}

/**
 * Restores keyboard focus to the active pane once the permanent-delete dialog closes (Shift+F8).
 * The dialog's own confirm/cancel buttons hold DOM focus; closing the dialog leaves that focus
 * nowhere useful rather than back in the pane, which broke the app's own Tab-to-other-pane
 * shortcut afterwards (it starts from the pane's `.fm-pane` element carrying focus). Matches the
 * explicit `focusPane` pattern already used after find-files navigation and pane switching.
 *
 * `ModalPanel`'s own generic focus-restore (`restoreFocusToInvoker` in mithril-materialized) also
 * runs on close, one animation frame later, and can silently no-op or refocus the wrong thing: it
 * restores whatever had focus when the dialog *opened* (often the row about to be deleted, whose
 * DOM node is gone by the time the frame fires) rather than the pane container this app's Tab
 * shortcut actually looks for. Deferred two frames so it runs strictly after that restore
 * attempt, rather than racing it - see `restoreFocusToInvoker`'s single-rAF deferral.
 */
function focusActivePaneAfterDeleteDialog(ctx: AppDialogsContext): void {
  const paneId = ctx.getActivePaneId();
  if (paneId === undefined) return;
  requestAnimationFrame(() => {
    requestAnimationFrame(() => ctx.getFocusPane()?.(paneId));
  });
}

/** Mirrors `canUseSystemTrash` in the global keydown handler: uninstalling always goes through
 * the Trash-first delete path, so the confirm button is only offered when every location the
 * dialog would trash is on a local provider. */
function canUseSystemTrash(locations: readonly Location[]): boolean {
  return locations.every(
    (location) => location.providerId === 'file' || location.providerId === 'local',
  );
}

export function renderAppDialogs(
  client: FileManagerClient,
  pendingDelete: OperationCentreState['byId'][string],
  ctx: AppDialogsContext,
): m.Children[] {
  const dialogs = ctx.getDialogs();
  const ds = dialogs.getState();

  return [
    ctx.getOperationCentreVisible()
      ? m(OperationCentre, {
          state: ctx.getOperations(),
          formatSettings: ctx.getFormatSettings(),
          onClose: () => ctx.toggleOperationCentre(),
          onCancel: (operationId) => {
            ctx.setOperations(
              transitionOperationState(ctx.getOperations(), operationId, 'cancelling'),
            );
            void client.cancelOperation(operationId).catch(() => undefined);
          },
          onPause: (operationId) => {
            ctx.setOperations(transitionOperationState(ctx.getOperations(), operationId, 'paused'));
            void client.pauseOperation(operationId).catch(() => undefined);
          },
          onResume: (operationId) => {
            ctx.setOperations(
              transitionOperationState(ctx.getOperations(), operationId, 'running'),
            );
            void client.resumeOperation(operationId).catch(() => undefined);
          },
          onUndo: (operationId) => {
            void client
              .undoOperation(operationId)
              .then((undo) => {
                const operations = ctx.getOperations();
                const byId = { ...operations.byId };
                delete byId[operationId];
                byId[undo.id] = byId[undo.id] ?? undo;
                ctx.setOperations({
                  ...operations,
                  byId,
                });
                ctx.redraw();
              })
              .catch(() => undefined);
          },
          onDismiss: (operationId) => {
            ctx.cancelAutoDismiss(operationId);
            ctx.rememberDismissedOperation(operationId);
            ctx.setOperations(dismissOperation(ctx.getOperations(), operationId));
          },
          hasDismissedOperations: ctx.hasDismissedOperations(),
          onShowAll: () => ctx.showAllOperations(),
        })
      : undefined,
    m(CreateDirectoryDialog, {
      open: ds.createDirectoryOpen,
      onCancel: () => dialogs.cancelCreateDirectory(),
      onConfirm: (name: string, createIntermediateDirectories: boolean) =>
        dialogs.confirmCreateDirectory(
          name,
          ctx.getActiveDirectoryLocation(),
          (loc, n, createIntermediates) =>
            ctx
              .getOpsController()
              .createDirectory(loc, n, createIntermediates)
              .then(() => undefined),
          createIntermediateDirectories,
        ),
    }),
    m(CreateFileDialog, {
      open: ds.createFileOpen,
      onCancel: () => dialogs.cancelCreateFile(),
      onConfirm: (name: string) =>
        dialogs.confirmCreateFile(name, ctx.getActiveDirectoryLocation(), (loc, n) =>
          ctx
            .getOpsController()
            .createFile(loc, n)
            .then(() => {
              ctx.openEditorForCreatedFile(loc, n);
            }),
        ),
    }),
    m(ArchiveCreateDialog, {
      open: ds.archiveCreateRequest !== undefined,
      moveSources: ds.archiveCreateRequest?.moveSources ?? false,
      onCancel: () => dialogs.cancelArchiveCreate(),
      onConfirm: (name: string, format: ArchiveFormat, compressionLevel?: number) => {
        const request = ds.archiveCreateRequest;
        if (request === undefined) return;
        dialogs.cancelArchiveCreate();
        void ctx.getOpsController().pack(
          request.sources,
          {
            ...request.destinationDirectory,
            uri: `${request.destinationDirectory.uri.replace(/\/$/u, '')}/${encodeURIComponent(name)}`,
          },
          request.moveSources,
          format,
          compressionLevel,
        );
      },
    }),
    m(MultiRenameDialog, {
      open: ds.multiRenameOpen,
      entries: ds.multiRenameEntries,
      existingSiblingNames: ds.multiRenameExistingNames,
      presets: ctx.getSettings()?.multiRenamePresets ?? [],
      onPresetsChange: (presets) =>
        ctx.updateSettings((settings) => ({ ...settings, multiRenamePresets: presets })),
      onCancel: () => dialogs.cancelMultiRename(),
      onApply: (renamed) => {
        const { multiRenameLocation: location, multiRenameEntries: entries } = ds;
        dialogs.cancelMultiRename();
        if (location === undefined) return;
        const entriesById = new Map(entries.map((entry) => [entry.id, entry]));
        const sources: Location[] = [];
        const destinations: Location[] = [];
        for (const { id, newName } of renamed) {
          const entry = entriesById.get(id);
          if (entry === undefined) continue;
          sources.push(entry.location);
          destinations.push({
            ...entry.location,
            uri: `${location.uri.replace(/\/$/u, '')}/${encodeURIComponent(newName)}`,
          });
        }
        if (sources.length === 0) return;
        void ctx.getOpsController().multiRename(sources, destinations);
      },
    }),
    m(PropertiesDialog, {
      open: ds.propertiesOpen,
      entries: ds.propertiesEntries,
      client,
      formatSettings: ctx.getFormatSettings(),
      onCancel: () => dialogs.cancelProperties(),
    }),
    m(ArchivePasswordDialog, {
      open: ds.pendingArchiveCredential !== undefined,
      invalid: ds.pendingArchiveCredential?.invalid ?? false,
      archiveLabel:
        ds.pendingArchiveCredential === undefined
          ? ''
          : pathFromUri(ds.pendingArchiveCredential.location.uri),
      ...(ds.archiveCredentialError === undefined ? {} : { error: ds.archiveCredentialError }),
      onCancel: () => {
        const pending = ds.pendingArchiveCredential;
        dialogs.clearArchiveCredential();
        pending?.resolve(false);
      },
      onConfirm: (password: string) => {
        const pending = ds.pendingArchiveCredential;
        if (pending === undefined) return;
        void client
          .cacheArchivePassword({ location: pending.location, password })
          .then(() => {
            if (ds.pendingArchiveCredential === pending) {
              dialogs.clearArchiveCredential();
              pending.resolve(true);
              ctx.redraw();
            }
          })
          .catch((error: unknown) => {
            dialogs.setArchiveCredentialError(
              error instanceof Error ? error.message : t('viewer', 'archivePasswordError'),
            );
            ctx.redraw();
          });
      },
    }),
    m(ConnectionsManager, {
      open: ctx.getConnectionsManagerOpen(),
      connections: ctx.getConnections(),
      onRefresh: async () => {
        ctx.setConnections(await loadConnections(client));
      },
      onClose: () => {
        ctx.setConnectionsManagerOpen(false);
        ctx.redraw();
      },
      onSave: async (draft, editingId) => {
        const result = await saveConnection(client, draft, editingId);
        if (result.ok) {
          ctx.setConnections(upsertConnection(ctx.getConnections(), result.connection));
          await openCreatedConnection(result.connection, editingId, ctx);
        }
        return result;
      },
      onDelete: async (id) => {
        await deleteConnectionRequest(client, id);
        ctx.setConnections(withoutConnection(ctx.getConnections(), id));
      },
      onConnect: async (id) => {
        const updated = await connectConnectionRequest(client, id);
        ctx.setConnections(upsertConnection(ctx.getConnections(), updated));
        return updated;
      },
      onDisconnect: async (id) => {
        const updated = await disconnectConnectionRequest(client, id);
        ctx.setConnections(upsertConnection(ctx.getConnections(), updated));
        return updated;
      },
      onTest: async (id) => {
        const updated = await testConnectionRequest(client, id);
        ctx.setConnections(upsertConnection(ctx.getConnections(), updated));
        return updated;
      },
      onProbeHostKey: (id) => probeSshHostKeyRequest(client, id),
      onAcceptHostKey: (id, fingerprint) => acceptSshHostKeyRequest(client, id, fingerprint),
      onBeginOneDriveAuthorization: (id) => beginOneDriveAuthorizationRequest(client, id),
      onGetOneDriveAuthorizationAttempt: (attemptId) =>
        getOneDriveAuthorizationAttemptRequest(client, attemptId),
      onCancelOneDriveAuthorization: (attemptId) =>
        cancelOneDriveAuthorizationRequest(client, attemptId),
      onOneDriveAuthorized: (connection, openAfterAuthorization) => {
        ctx.setConnections(upsertConnection(ctx.getConnections(), connection));
        if (openAfterAuthorization) void openCreatedConnection(connection, undefined, ctx);
        ctx.redraw();
      },
    }),
    m(
      FindFilesDialog,
      (() => {
        const findFilesRoot = ctx.getFindFilesRoot();
        const findFilesError = ctx.getFindFilesError();
        return {
          open: ctx.getFindFilesOpen(),
          scopeLabel: findFilesRoot === undefined ? '' : pathFromUri(findFilesRoot.uri),
          ...(findFilesError === undefined ? {} : { error: findFilesError }),
          savedSearches: ctx.getSettings()?.savedSearches ?? [],
          onSearch: (params: FindFilesSearchParams) =>
            ctx.getFindFilesController().startFindFilesSearch(params),
          onSave: (name: string, params: FindFilesSearchParams, id?: string) => {
            const root = ctx.getFindFilesRoot();
            const paneId = ctx.getActivePaneId();
            if (root === undefined || paneId === undefined) return;
            const existing = ctx.getSettings()?.savedSearches.find((saved) => saved.id === id);
            const saved = {
              id: id ?? crypto.randomUUID(),
              name: name.trim(),
              pinned: existing?.pinned ?? false,
              query: searchQueryFromParams(
                root,
                params,
                ctx.getFindFilesController().activeShowHidden(paneId),
                existing?.query,
              ),
            };
            void ctx.updateSettings((settings) => ({
              ...settings,
              savedSearches: saveSearch(settings.savedSearches, saved),
            }));
          },
          onDeleteSaved: (id: string) =>
            void ctx.updateSettings((settings) => ({
              ...settings,
              savedSearches: deleteSavedSearch(settings.savedSearches, id),
            })),
          onToggleSavedPin: (id: string) =>
            void ctx.updateSettings((settings) => ({
              ...settings,
              savedSearches: toggleSavedSearchPin(settings.savedSearches, id),
            })),
          onOpenSaved: (saved: SavedSearch, target: SavedSearchOpenTarget) =>
            ctx.getFindFilesController().startSavedSearch(saved, target),
          onCancel: () => ctx.getFindFilesController().closeFindFiles(),
        };
      })(),
    ),
    m(PermanentDeleteDialog, {
      open: pendingDelete !== undefined,
      ...(pendingDelete === undefined ? {} : { operationId: pendingDelete.id }),
      itemCount: pendingDelete?.progress.totalItems ?? 0,
      totalBytes: pendingDelete?.progress.totalBytes ?? 0,
      formatSettings: ctx.getFormatSettings(),
      onCancel: () => {
        if (pendingDelete !== undefined) {
          const id = pendingDelete.id;
          ctx.cancelAutoDismiss(id);
          ctx.rememberDismissedOperation(id);
          ctx.setOperations(dismissOperation(ctx.getOperations(), id));
          ctx.redraw();
          void client.cancelOperation(id).catch(() => undefined);
        }
        // The dialog closing leaves DOM focus on document.body (or a now-removed row) rather
        // than back in the pane, which broke Tab-to-other-pane afterwards - restore it
        // explicitly instead of relying on the generic modal's focus-restore, which isn't aware
        // of this app's `.fm-pane` keyboard-target requirement.
        focusActivePaneAfterDeleteDialog(ctx);
      },
      onConfirm: () => {
        if (pendingDelete === undefined) return Promise.resolve();
        const id = pendingDelete.id;
        ctx.setOperations(transitionOperationState(ctx.getOperations(), id, 'running'));
        ctx.redraw();
        focusActivePaneAfterDeleteDialog(ctx);
        return client
          .resolveConflict({
            operationId: id,
            resolution: 'confirm',
            applyToAllSimilar: false,
          })
          .then(() => {
            ctx.refetchAffectedPanes();
            ctx.redraw();
          })
          .catch((error: unknown) => {
            ctx.setOperations(
              transitionOperationState(ctx.getOperations(), id, 'waitingForConflictResolution'),
            );
            ctx.redraw();
            throw error;
          });
      },
    }),
    m(ConflictDialog, {
      conflict: ctx.getPendingConflict(),
      onResolve: (resolution, applyToAllSimilar) => {
        const conflict = ctx.getPendingConflict();
        if (conflict === undefined) return;
        void client
          .resolveConflict({ operationId: conflict.operationId, resolution, applyToAllSimilar })
          .then(() => {
            if (ctx.getPendingConflict()?.conflictId === conflict.conflictId) {
              ctx.setPendingConflict(undefined);
              ctx.refetchAffectedPanes();
              ctx.redraw();
            }
          });
      },
    }),
    m(CloseLastTabDialog, {
      open: ctx.getCloseTabConfirmation() !== undefined,
      onConfirm: () => {
        const confirmation = ctx.getCloseTabConfirmation();
        ctx.setCloseTabConfirmation(undefined);
        if (confirmation !== undefined) {
          ctx.getTabController().performCloseTab(confirmation.paneId, confirmation.tabId);
        }
      },
      onCancel: () => ctx.setCloseTabConfirmation(undefined),
    }),
    m(FinderTagsDialog, {
      open: ds.finderTagsDialog !== undefined,
      entryName: ds.finderTagsDialog?.entry.name ?? '',
      initialTags: ds.finderTagsDialog?.tags ?? [],
      onCancel: () => dialogs.cancelFinderTagsDialog(),
      onConfirm: (tags) => {
        const request = ds.finderTagsDialog;
        dialogs.cancelFinderTagsDialog();
        if (request === undefined) return;
        void client
          .setFinderTags(request.entry.location.uri, { tags: [...tags] })
          .then((persisted) => {
            ctx.getFinderTagsLoader()?.setCached(request.entry.location.uri, persisted);
          })
          .catch(() => undefined);
      },
    }),
    m(SpotlightCommentDialog, {
      open: ds.spotlightCommentDialog !== undefined,
      entryName: ds.spotlightCommentDialog?.entry.name ?? '',
      initialComment: ds.spotlightCommentDialog?.comment ?? '',
      onCancel: () => dialogs.cancelSpotlightCommentDialog(),
      onConfirm: (comment) => {
        const request = ds.spotlightCommentDialog;
        dialogs.cancelSpotlightCommentDialog();
        if (request === undefined) return;
        void client
          .setSpotlightComment(request.entry.location.uri, {
            comment: comment.trim().length === 0 ? null : comment,
          })
          .catch(() => undefined);
      },
    }),
    m(ApplicationUninstallDialog, {
      open: ds.applicationUninstallDialog !== undefined,
      productName: ds.applicationUninstallDialog?.productName ?? '',
      relatedFiles: ds.applicationUninstallDialog?.relatedFiles ?? [],
      canTrash: canUseSystemTrash([
        ...(ds.applicationUninstallDialog === undefined
          ? []
          : [ds.applicationUninstallDialog.bundle.location]),
      ]),
      onCancel: () => dialogs.cancelApplicationUninstallDialog(),
      onConfirm: (checkedRelatedFiles) => {
        const request = ds.applicationUninstallDialog;
        dialogs.cancelApplicationUninstallDialog();
        if (request === undefined) return;
        void ctx
          .getOpsController()
          .trash([request.bundle.location, ...checkedRelatedFiles])
          .then(() => ctx.refetchAffectedPanes())
          .catch(() => undefined);
        // Best-effort, fire-and-forget (task 0148 follow-up): a pinned Dock icon left pointing at
        // the now-trashed bundle is cosmetic, not something that should block or delay the actual
        // uninstall above if it fails or a host doesn't support it.
        void client
          .removeApplicationDockIcon({ location: request.bundle.location })
          .catch(() => undefined);
      },
    }),
  ];
}
