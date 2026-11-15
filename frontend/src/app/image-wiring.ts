import m from 'mithril';
import { toast } from 'mithril-materialized';
import type { FileManagerClient } from '../api/client/file-manager-client';
import {
  destinationFor,
  emptyImagePane,
  firstImageId,
  forgetGeneration,
  type ImageFailureKey,
  type ImageGenerationRecord,
  type ImagePaneEntry,
  imagePlacementPaneId,
  imageSaveFolderPaneId,
  promptOf,
  promptPatch,
  rememberGeneration,
} from '../features/image-generation/image-pane-state';
import { composeNegativePrompt, composePrompt } from '../features/image-generation/image-presets';
import { dispatchWorkspaceCommand } from '../features/workspace/dispatch-workspace-command';
import { t } from '../i18n';
import type { GeneratedImage, ImagePrompt, Location, PaneId } from '../models';
import { qualityIdOf } from '../models';
import { tabKey } from './shell-helpers';
import type { ShellState } from './shell-state';

export interface ImageWiringCore {
  readonly replaceWorkspace: (next: import('../models').WorkspaceProjection) => void;
}

/** What a save writes: one stored image, a name, a folder, and an explicit agreement to replace. */
export interface ImageSaveInput {
  readonly imageId: string;
  readonly fileName: string;
  readonly folder: Location;
  readonly overwrite: boolean;
}

/**
 * Opens, drives and closes the image generation tabs (task 0325).
 *
 * The backend holds every byte: this wiring asks for a batch, asks for the preview bytes of each
 * image, and only ever writes a file when the person presses Save. A batch that is discarded,
 * regenerated or closed is dropped from the store and its object URLs are revoked.
 */
export function createImageWiring(shellState: ShellState, core: ImageWiringCore) {
  /** Folders recent saves went to, newest first, and the names written to each this session. */
  const recentSaveFolders: Location[] = [];
  const savedNames = new Map<string, Set<string>>();
  function entryOf(key: string): ImagePaneEntry | undefined {
    return shellState.imageByTab.get(key);
  }

  /** Object URL for stored bytes; absent where the host has no URL factory, such as a test run. */
  function objectUrl(bytes: Uint8Array, mediaType: string): string | undefined {
    if (typeof URL.createObjectURL !== 'function') return undefined;
    try {
      return URL.createObjectURL(new Blob([bytes.slice()], { type: mediaType }));
    } catch {
      return undefined;
    }
  }

  function revoke(views: readonly { previewUrl?: string | undefined }[]): void {
    if (typeof URL.revokeObjectURL !== 'function') return;
    for (const view of views) {
      if (view.previewUrl !== undefined) URL.revokeObjectURL(view.previewUrl);
    }
  }

  /** Opens an image pane at the placement rule's pane. */
  async function openImage(): Promise<void> {
    const workspace = shellState.workspace;
    if (workspace === undefined || shellState.imageOpening) return;
    // The pane is entered from a gated command, so the answer may be a moment stale; asking again
    // costs one call and lets a model named in Settings enable it without a restart.
    void refreshRoles(shellState.client);
    const targetPaneId = imagePlacementPaneId(
      workspace.paneOrder,
      workspace.activePaneId,
      shellState.editorByPane.has(workspace.activePaneId),
    );
    const targetPane = workspace.panesById[targetPaneId];
    const activeTab =
      targetPane === undefined ? undefined : targetPane.tabsById[targetPane.activeTabId ?? ''];
    if (activeTab === undefined) return;

    shellState.imageOpening = true;
    try {
      await dispatchWorkspaceCommand(
        shellState.client,
        {
          type: 'addTransientTab',
          workspaceId: workspace.id,
          paneId: targetPaneId,
          location: activeTab.location,
          expectedRevision: workspace.revision,
        },
        (next) => {
          core.replaceWorkspace(next);
          const tabId = next.panesById[targetPaneId]?.activeTabId;
          if (tabId === undefined) return;
          const key = tabKey(targetPaneId, tabId);
          shellState.imageByTab.set(key, emptyImagePane(key, targetPaneId, tabId, workspace.id));
          void loadPrompts(shellState.client);
          m.redraw();
        },
      );
    } catch (error: unknown) {
      toast({ html: error instanceof Error ? error.message : t('image', 'failed') });
    } finally {
      shellState.imageOpening = false;
    }
  }

  /**
   * Refreshes the prompt history every image pane shows. It is one list for all of them, so a prompt
   * saved in one pane is there in the next without reopening it. A failed read leaves the old list.
   */
  async function loadPrompts(client: FileManagerClient): Promise<void> {
    try {
      const prompts = await client.listImagePrompts();
      for (const entry of shellState.imageByTab.values()) entry.prompts = prompts;
    } catch {
      // The history is a convenience; it must never get in the way of generating.
    }
    m.redraw();
  }

  /** Puts a remembered prompt, with all its settings, back in the form. */
  function applyPrompt(key: string, prompt: ImagePrompt): void {
    patch(key, promptPatch(prompt));
  }

  async function deletePrompt(client: FileManagerClient, promptId: string): Promise<void> {
    try {
      await client.deleteImagePrompt(promptId);
    } catch {
      // Already gone is the outcome the person asked for; the reload shows what is really there.
    }
    await loadPrompts(client);
  }

  /** Applies an edit to the prompt side; every control writes straight back to the entry. */
  function patch(key: string, changes: Partial<ImagePaneEntry>): void {
    const entry = entryOf(key);
    if (entry === undefined) return;
    Object.assign(entry, changes);
    m.redraw();
  }

  /**
   * Puts a generation into the crumb bar. The pictures of a generation the bar drops are revoked,
   * since nothing on screen can reach them any more.
   */
  function remember(entry: ImagePaneEntry, record: ImageGenerationRecord): void {
    const before = entry.history;
    const after = rememberGeneration(before, record);
    const kept = new Set(after);
    for (const dropped of before) {
      // Revisiting a generation hands back the very same views, so those stay alive.
      if (kept.has(dropped) || dropped.views === record.views) continue;
      revoke(dropped.views);
    }
    entry.history = after;
  }

  /** Puts the generation on screen into the crumb bar, so it stays reachable. */
  function archive(entry: ImagePaneEntry): void {
    if (entry.batch === undefined) return;
    const generationId = entry.batch.generationId;
    remember(entry, {
      generationId,
      subject: entry.history.find((record) => record.generationId === generationId)?.subject,
      batch: entry.batch,
      views: entry.views,
      selectedImageId: entry.selectedImageId,
    });
  }

  /**
   * Fills the preview side's state for a batch: one loading view per image, first one selected.
   * The pictures it replaces belong to a crumb now, so their URLs are left alone.
   */
  function startViews(client: FileManagerClient, key: string, entry: ImagePaneEntry): void {
    entry.views = (entry.batch?.images ?? []).map((image) => ({
      imageId: image.id,
      loading: true,
    }));
    entry.selectedImageId = firstImageId(entry.batch);
    m.redraw();
    for (const image of entry.batch?.images ?? []) {
      void loadBytes(client, key, image, false);
    }
  }

  /** Fetches one image's bytes and turns them into a preview URL. */
  async function loadBytes(
    client: FileManagerClient,
    key: string,
    image: GeneratedImage,
    original: boolean,
  ): Promise<void> {
    const entry = entryOf(key);
    const generationId = entry?.batch?.generationId;
    if (entry === undefined || generationId === undefined) return;
    const view = entry.views.find((candidate) => candidate.imageId === image.id);
    if (view === undefined) return;
    view.loading = true;
    m.redraw();
    try {
      const bytes = await client.generatedImageBytes(generationId, image.id, original);
      if (entryOf(key) !== entry) return;
      const url = objectUrl(bytes.bytes, bytes.mediaType);
      const target = entry.views.find((candidate) => candidate.imageId === image.id);
      if (target === undefined) return;
      if (url !== undefined && target.previewUrl !== undefined && target.previewUrl !== url) {
        if (typeof URL.revokeObjectURL === 'function') URL.revokeObjectURL(target.previewUrl);
      }
      target.previewUrl = url ?? target.previewUrl;
      target.loading = false;
    } catch {
      const target = entry.views.find((candidate) => candidate.imageId === image.id);
      if (target === undefined) return;
      target.loading = false;
      entry.error = t('image', 'previewFailed');
    }
    m.redraw();
  }

  /** Asks the image role's endpoint for a batch. Nothing is written to disk here. */
  async function generate(client: FileManagerClient, key: string): Promise<void> {
    const entry = entryOf(key);
    if (entry === undefined || entry.pendingRequestId !== undefined) return;
    const prompt = composePrompt(entry.subject, entry.presetIds, entry.lensId, entry.apertureId);
    if (prompt.trim() === '') return;
    const requestId = crypto.randomUUID();
    // The form as it is now is what gets remembered, whatever is edited while the batch is made.
    const remembered = promptOf(entry);
    entry.pendingRequestId = requestId;
    entry.failure = undefined;
    entry.error = undefined;
    m.redraw();
    try {
      const batch = await client.generateImages({
        requestId,
        workspaceId: entry.workspaceId,
        prompt,
        negativePrompt: composeNegativePrompt(entry.negativePrompt, entry.presetIds) || null,
        width: entry.width,
        height: entry.height,
        count: entry.count,
        seed: entry.seed ?? null,
        // A level or a step count nobody chose is left out of the request entirely: an endpoint
        // without those knobs then sees the request it saw before they existed.
        ...(entry.qualityId === '' ? {} : { quality: entry.qualityId }),
        ...(entry.steps === undefined ? {} : { steps: entry.steps }),
        ...(entry.source === undefined ? {} : { source: entry.source }),
      });
      if (entryOf(key) !== entry || entry.pendingRequestId !== requestId) return;
      entry.pendingRequestId = undefined;
      // What was on screen becomes a crumb rather than disappearing.
      archive(entry);
      entry.batch = batch;
      entry.activeGenerationId = batch.generationId;
      // The seed the batch came back with is kept, so "regenerate" can repeat or vary it deliberately.
      if (batch.seed !== undefined && batch.seed !== null) entry.seed = Number(batch.seed);
      startViews(client, key, entry);
      // Only a prompt that produced pictures is worth coming back to.
      void client.saveImagePrompt(remembered).then(
        () => loadPrompts(client),
        () => undefined,
      );
      // The bar holds every generation the pane asked for, the one on screen included.
      remember(entry, {
        generationId: batch.generationId,
        subject: remembered.subject,
        batch,
        views: entry.views,
        selectedImageId: entry.selectedImageId,
      });
    } catch (error: unknown) {
      if (entryOf(key) !== entry) return;
      entry.pendingRequestId = undefined;
      const reason = imageFailureReason(error);
      entry.failure = imageFailureKey(reason);
      entry.error = t('image', entry.failure);
    } finally {
      m.redraw();
    }
  }

  /** Stops the generation in flight; the person keeps the prompt. */
  function stop(client: FileManagerClient, key: string): void {
    const entry = entryOf(key);
    const requestId = entry?.pendingRequestId;
    if (entry === undefined || requestId === undefined) return;
    entry.pendingRequestId = undefined;
    m.redraw();
    void client.cancelImageGeneration(requestId).catch(() => undefined);
  }

  /** Shows one stored image large; its original bytes are fetched if only a thumbnail is held. */
  function select(client: FileManagerClient, key: string, imageId: string): void {
    const entry = entryOf(key);
    if (entry === undefined) return;
    entry.selectedImageId = imageId;
    m.redraw();
    const image = entry.batch?.images.find((candidate) => candidate.id === imageId);
    if (image?.downscaled === true) void loadBytes(client, key, image, true);
  }

  /**
   * Shows a generation the crumb bar holds again. Its prompt comes back with it, and only the
   * pictures the backend has let go of are fetched again.
   */
  function selectGeneration(client: FileManagerClient, key: string, generationId: string): void {
    const entry = entryOf(key);
    if (entry === undefined || entry.activeGenerationId === generationId) return;
    const record = entry.history.find((item) => item.generationId === generationId);
    if (record === undefined) return;
    archive(entry);
    entry.batch = record.batch;
    entry.views = record.views;
    entry.selectedImageId = record.selectedImageId ?? firstImageId(record.batch);
    entry.activeGenerationId = generationId;
    entry.failure = undefined;
    entry.error = undefined;
    // The prompt on screen is what produced these pictures, so it comes back too.
    entry.subject = record.batch.prompt;
    entry.negativePrompt = record.batch.negativePrompt ?? entry.negativePrompt;
    entry.width = record.batch.width;
    entry.height = record.batch.height;
    entry.qualityId = qualityIdOf(record.batch.quality ?? '');
    entry.steps = record.batch.steps ?? undefined;
    entry.source = record.batch.source ?? undefined;
    m.redraw();
    for (const image of record.batch.images) {
      const view = record.views.find((item) => item.imageId === image.id);
      if (view?.previewUrl === undefined) void loadBytes(client, key, image, false);
    }
  }

  /**
   * Clears the prompt for a fresh idea. What has already been generated stays in the crumb bar -
   * the backend still holds those bytes, and dropping them is the person's call, not this one.
   */
  function newPrompt(client: FileManagerClient, key: string): void {
    const entry = entryOf(key);
    if (entry === undefined) return;
    if (entry.pendingRequestId !== undefined) stop(client, key);
    entry.subject = '';
    entry.negativePrompt = '';
    entry.presetIds = [];
    entry.lensId = '';
    entry.apertureId = '';
    entry.seed = undefined;
    entry.qualityId = '';
    entry.steps = undefined;
    entry.source = undefined;
    entry.batch = undefined;
    entry.views = [];
    entry.selectedImageId = undefined;
    entry.activeGenerationId = undefined;
    entry.failure = undefined;
    entry.error = undefined;
    m.redraw();
  }

  /** Writes one stored image to disk. An existing file is refused until the person agrees to replace it. */
  async function save(
    client: FileManagerClient,
    key: string,
    input: ImageSaveInput,
  ): Promise<void> {
    const entry = entryOf(key);
    const generationId = entry?.batch?.generationId;
    if (entry === undefined || generationId === undefined) return;
    entry.savingImageId = input.imageId;
    entry.failure = undefined;
    entry.error = undefined;
    m.redraw();
    try {
      const receipt = await client.saveGeneratedImage({
        generationId,
        imageId: input.imageId,
        destination: destinationFor(input.folder, input.fileName),
        overwrite: input.overwrite,
      });
      if (entryOf(key) !== entry) return;
      const view = entry.views.find((candidate) => candidate.imageId === input.imageId);
      if (view !== undefined) view.savedLocation = receipt.location.uri;
      const folderUri = input.folder.uri.replace(/\/+$/u, '');
      savedNames.set(
        folderUri,
        (savedNames.get(folderUri) ?? new Set()).add(input.fileName.toLowerCase()),
      );
      const at = recentSaveFolders.findIndex((known) => known.uri === input.folder.uri);
      if (at >= 0) recentSaveFolders.splice(at, 1);
      recentSaveFolders.unshift(input.folder);
      recentSaveFolders.length = Math.min(recentSaveFolders.length, 5);
      entry.error = undefined;
      toast({
        html: receipt.promptEmbedded
          ? t('image', 'savedWithPrompt', { location: receipt.location.uri })
          : t('image', 'savedPlain', { location: receipt.location.uri }),
      });
    } catch (error: unknown) {
      if (entryOf(key) !== entry) return;
      const reason = imageFailureReason(error);
      // Only the folder and the name are in the person's hands at this point, so an invalid
      // request here is about the destination - the prompt has already been accepted.
      entry.failure = reason === 'invalidRequest' ? 'destinationInvalid' : imageFailureKey(reason);
      entry.error = t('image', entry.failure);
    } finally {
      if (entryOf(key) === entry) entry.savingImageId = undefined;
      m.redraw();
    }
  }

  /** Drops the batch from the backend's store without saving any of its images. */
  async function discard(client: FileManagerClient, key: string): Promise<void> {
    const entry = entryOf(key);
    const generationId = entry?.batch?.generationId;
    if (entry === undefined || generationId === undefined) return;
    revoke(entry.views);
    entry.history = forgetGeneration(entry.history, generationId);
    entry.batch = undefined;
    entry.views = [];
    entry.selectedImageId = undefined;
    entry.activeGenerationId = undefined;
    entry.failure = undefined;
    entry.error = undefined;
    m.redraw();
    await client.discardImageBatch(generationId).catch(() => undefined);
  }

  /** Generates again from the batch on screen, keeping the prompt that produced it. */
  async function regenerate(client: FileManagerClient, key: string): Promise<void> {
    const entry = entryOf(key);
    const batch = entry?.batch;
    if (entry === undefined || batch === undefined) return;
    // The prompt on the left is what produced this batch; take it back so the person sees why.
    entry.subject = batch.prompt;
    entry.negativePrompt = batch.negativePrompt ?? entry.negativePrompt;
    entry.width = batch.width;
    entry.height = batch.height;
    // Asking again means asking the same thing of the same picture, so the start stays put too.
    entry.qualityId = qualityIdOf(batch.quality ?? '');
    entry.steps = batch.steps ?? undefined;
    entry.source = batch.source ?? entry.source;
    await generate(client, key);
  }

  /** Forgets a closed pane: its pending request is stopped and every batch it held is dropped. */
  function forget(client: FileManagerClient, key: string): void {
    const entry = shellState.imageByTab.get(key);
    shellState.imageByTab.delete(key);
    if (entry === undefined) return;
    const held = [
      ...entry.history.map((record) => record.generationId),
      ...(entry.batch === undefined ? [] : [entry.batch.generationId]),
    ];
    revoke(entry.views);
    for (const record of entry.history) revoke(record.views);
    if (entry.pendingRequestId !== undefined) {
      void client.cancelImageGeneration(entry.pendingRequestId).catch(() => undefined);
    }
    for (const generationId of new Set(held)) {
      void client.discardImageBatch(generationId).catch(() => undefined);
    }
  }

  /**
   * Where a save may go, best first: the other pane's folder, this pane's own, then every folder
   * still open in another pane, then the folders recent saves went to. Choosing one is a click, so
   * nobody has to type a location.
   */
  function saveFolders(key: string): { location: Location; name: string }[] {
    const entry = entryOf(key);
    if (entry === undefined || shellState.workspace === undefined) return [];
    const otherPaneId = imageSaveFolderPaneId(shellState.workspace.paneOrder, entry.paneId);
    const candidates: Location[] = [
      ...(otherPaneId === undefined ? [] : foldersOfPane(otherPaneId)),
      ...foldersOfPane(entry.paneId),
      // Every folder the person still has open somewhere else is a place a picture could go.
      ...shellState.workspace.paneOrder
        .filter((paneId) => paneId !== otherPaneId && paneId !== entry.paneId)
        .flatMap((paneId) => foldersOfPane(paneId)),
      ...recentSaveFolders,
    ];
    const folders: Location[] = [];
    for (const location of candidates) {
      if (location.uri.startsWith('search://')) continue;
      if (!folders.some((known) => known.uri === location.uri)) folders.push(location);
    }
    return folders.map((location) => ({
      location,
      name: decodeURIComponent(location.uri.replace(/\/+$/u, '').split('/').pop() ?? location.uri),
    }));
  }

  /**
   * The names of the files in a folder this app is showing or has just written to, folded to lower
   * case; `undefined` when it knows nothing about that folder, so no guess is made about it.
   */
  function fileNamesIn(folder: Location): ReadonlySet<string> | undefined {
    const uri = folder.uri.replace(/\/+$/u, '');
    let names: Set<string> | undefined;
    for (const view of shellState.directories.values()) {
      if (view.location?.uri.replace(/\/+$/u, '') !== uri) continue;
      names ??= new Set();
      for (const listed of view.entries ?? []) names.add(listed.name.toLowerCase());
    }
    const written = savedNames.get(uri);
    if (written !== undefined) {
      names ??= new Set();
      for (const name of written) names.add(name);
    }
    return names;
  }

  /**
   * Takes the picture the other pane has under its cursor as the one this prompt starts from. The
   * person points at a file they can see rather than typing a path, and the backend reads the bytes
   * through the same provider every other read goes through.
   */
  function useSource(key: string): void {
    const entry = entryOf(key);
    const workspace = shellState.workspace;
    if (entry === undefined || workspace === undefined) return;
    const otherPaneId = imageSaveFolderPaneId(workspace.paneOrder, entry.paneId);
    const otherPane = otherPaneId === undefined ? undefined : workspace.panesById[otherPaneId];
    const otherTabId = otherPane?.activeTabId;
    if (otherPaneId === undefined || otherTabId === undefined) {
      toast({ html: t('image', 'sourceNeedsOtherPane') });
      return;
    }
    const tabKeyOfSource = tabKey(otherPaneId, otherTabId);
    const view = shellState.directories.get(tabKeyOfSource);
    const selection = shellState.selections.get(tabKeyOfSource);
    const entryId = selection?.cursorEntryId ?? selection?.selectedEntryIds[0];
    const file = view?.entries.find((candidate) => candidate.id === entryId);
    if (file === undefined || entryId === undefined) {
      toast({ html: t('image', 'sourceNeedsOtherPane') });
      return;
    }
    if (file.kind !== 'file') {
      toast({ html: t('image', 'sourceNotFile') });
      return;
    }
    patch(key, { source: { location: file.location } });
  }

  /** Stops starting from a picture; the prompt and its settings stay as they were. */
  function clearSource(key: string): void {
    patch(key, { source: undefined });
  }

  /** The folders a pane has open, the one it is showing first. */
  function foldersOfPane(paneId: PaneId): Location[] {
    const pane = shellState.workspace?.panesById[paneId];
    if (pane === undefined) return [];
    const folders: Location[] = [];
    for (const tabId of [pane.activeTabId, ...(pane.tabOrder ?? [])]) {
      const location = shellState.directories.get(tabKey(paneId, tabId))?.location;
      if (location === undefined || location.uri.startsWith('search://')) continue;
      if (!folders.some((known) => known.uri === location.uri)) folders.push(location);
    }
    return folders;
  }

  /** Where a picture goes when no pane is looking at a folder: the OS pictures folder. */
  function picturesFolder(): Location | undefined {
    const home = shellState.homeDirectory;
    if (home === undefined || home === '') return undefined;
    return { providerId: 'local', uri: `file://${home.replace(/\/+$/u, '')}/Pictures/` };
  }

  /**
   * The folder a save goes to. The other pane's folder is the answer the person is already looking
   * at; when that pane shows a tool instead, a folder it has open, then one of this pane's own, then
   * where the last picture went, and failing all of that the OS pictures folder. Typing a path in the
   * form stays the way to anywhere else.
   */
  function saveFolder(key: string): { location: Location; name: string } | undefined {
    const entry = entryOf(key);
    if (entry === undefined || shellState.workspace === undefined) return undefined;
    const otherPaneId = imageSaveFolderPaneId(shellState.workspace.paneOrder, entry.paneId);
    const candidates = [
      ...(otherPaneId === undefined ? [] : foldersOfPane(otherPaneId)),
      ...foldersOfPane(entry.paneId),
      ...recentSaveFolders,
    ];
    const location = candidates[0] ?? picturesFolder();
    if (location === undefined) return undefined;
    const uri = location.uri;
    const name = decodeURIComponent(uri.replace(/\/+$/u, '').split('/').pop() ?? uri);
    return { location, name };
  }

  /**
   * Whether an image model is assigned. Without one the pane can only say so, so its command and its
   * overflow entry stay disabled until the person names a model in Settings (0325).
   */
  async function refreshRoles(client: FileManagerClient): Promise<void> {
    try {
      const roles = await client.listLlmRoles();
      shellState.imageGenerationAvailable = roles.image != null;
    } catch {
      // A host that cannot answer says nothing either way; unavailable keeps the pane's own
      // "No image model is set" explanation the last word.
      shellState.imageGenerationAvailable = false;
    } finally {
      shellState.imageGenerationChecked = true;
      m.redraw();
    }
  }

  function imageRoleAvailable(): boolean {
    return shellState.imageGenerationAvailable;
  }

  return {
    openImage,
    refreshRoles: (client: FileManagerClient) => refreshRoles(client),
    imageRoleAvailable,
    patch,
    generate: (client: FileManagerClient, key: string) => void generate(client, key),
    stop,
    useSource,
    clearSource,
    select: (client: FileManagerClient, key: string, imageId: string) =>
      select(client, key, imageId),
    save: (client: FileManagerClient, key: string, input: ImageSaveInput) =>
      void save(client, key, input),
    discard: (client: FileManagerClient, key: string) => void discard(client, key),
    regenerate: (client: FileManagerClient, key: string) => void regenerate(client, key),
    forget,
    saveFolder,
    saveFolders,
    fileNamesIn,
    selectGeneration: (client: FileManagerClient, key: string, generationId: string) =>
      selectGeneration(client, key, generationId),
    newPrompt,
    applyPrompt,
    deletePrompt: (client: FileManagerClient, promptId: string) =>
      void deletePrompt(client, promptId),
  };
}

/** The rejection reason of a failed request, as the backend reported it. */
export function imageFailureReason(error: unknown): string {
  if (typeof error !== 'object' || error === null) return 'failed';
  const record = error as {
    code?: unknown;
    reason?: unknown;
    status?: unknown;
    details?: unknown;
  };
  // Every rejection shares the `invalidRequest` code, so the reason the person is told about is
  // read from `details` first; the bare code is a last resort, not the primary key.
  const details = record.details;
  if (typeof details === 'object' && details !== null) {
    const reason = (details as { reason?: unknown }).reason;
    if (typeof reason === 'string') return reason;
  }
  if (typeof record.reason === 'string') return record.reason;
  if (typeof record.code === 'string') return record.code;
  if (record.status === 403) return 'cloudImageNotAllowed';
  if (record.status === 404) return 'notFound';
  if (record.status === 409) return 'destinationAlreadyExists';
  if (record.status === 413) return 'imageTooLarge';
  if (record.status === 429 || record.status === 502 || record.status === 503) {
    return 'imageGenerationFailed';
  }
  return 'failed';
}

/** Which message explains a rejection reason. */
export type { ImageFailureKey } from '../features/image-generation/image-pane-state';

/** Maps a rejection reason to its message key; nothing here repeats the endpoint's own words. */
export function imageFailureKey(reason: string): ImageFailureKey {
  switch (reason) {
    case 'roleNotAssigned':
    case 'llmRoleNotAssigned':
      return 'roleNotAssigned';
    case 'cloudImageNotAllowed':
    case 'llmCloudImageNotAllowed':
      return 'cloudImageNotAllowed';
    case 'cloudConsentRequired':
      return 'consentRequired';
    case 'llmProfileInvalid':
      return 'endpointInvalid';
    case 'llmTimeout':
      return 'timedOut';
    case 'permissionDenied':
      return 'hostNotAllowed';
    case 'providerUnavailable':
    case 'operationCancelled':
      // A generation the backend itself cancelled is the same outcome as the person pressing Stop.
      return reason === 'operationCancelled' ? 'cancelled' : 'generationFailed';
    case 'invalidRequest':
      return 'invalidRequest';
    case 'notFound':
      return 'notFound';
    case 'imageBatchExpired':
      return 'batchExpired';
    case 'imageTooLarge':
      return 'imageTooLarge';
    case 'imageSourceUnsupported':
      return 'sourceUnsupported';
    case 'imageSourceUnusable':
      return 'sourceUnusable';
    case 'imageSourceTooLarge':
      return 'sourceTooLarge';
    case 'destinationAlreadyExists':
      return 'destinationExists';
    case 'cancelled':
      return 'cancelled';
    case 'imageGenerationFailed':
      return 'generationFailed';
    default:
      return 'failed';
  }
}
