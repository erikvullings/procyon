import m from 'mithril';
import { toast } from 'mithril-materialized';
import type { FileManagerClient } from '../api/client/file-manager-client';
import {
  attachedChars,
  fileContentAttachment,
  folderListingAttachment,
  MAX_ATTACHED_CHARS,
  MAX_ATTACHED_FILE_BYTES,
  selectionAttachment,
} from '../features/chat/chat-context';
import {
  CHAT_IMAGE_MAX_COUNT,
  encodePastedImage,
  type PastedImage,
} from '../features/chat/chat-image';
import {
  type ChatAttachment,
  type ChatTabEntry,
  canSendMessage,
  chatFolderPaneId,
  chatPlacementPaneId,
  contextForMessage,
} from '../features/chat/chat-tab-state';
import { dispatchWorkspaceCommand } from '../features/workspace/dispatch-workspace-command';
import { t } from '../i18n';
import type { ChatFailure, PaneId, WorkspaceProjection } from '../models';
import { tabKey } from './shell-helpers';
import type { ShellState } from './shell-state';

export interface ChatCore {
  readonly replaceWorkspace: (next: WorkspaceProjection) => void;
}

/** Opens, drives and closes the chat tabs (task 0324). Replies never prompt for consent. */
export function createChatWiring(shellState: ShellState, core: ChatCore) {
  function entryOf(key: string): ChatTabEntry | undefined {
    return shellState.chatByTab.get(key);
  }

  /** Opens a chat tab at the placement rule's pane and loads the workspace's conversations. */
  async function openChat(): Promise<void> {
    const workspace = shellState.workspace;
    if (workspace === undefined || shellState.chatOpening) return;
    const targetPaneId = chatPlacementPaneId(
      workspace.paneOrder,
      workspace.activePaneId,
      shellState.editorByPane.has(workspace.activePaneId),
    );
    const targetPane = workspace.panesById[targetPaneId];
    const activeTab =
      targetPane === undefined ? undefined : targetPane.tabsById[targetPane.activeTabId ?? ''];
    if (activeTab === undefined) return;

    shellState.chatOpening = true;
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
          shellState.chatByTab.set(key, {
            key,
            paneId: targetPaneId,
            tabId,
            workspaceId: workspace.id,
            conversations: [],
            images: [],
            messages: [],
            attachments: [],
          });
          m.redraw();
          void loadConversations(shellState.client, key);
        },
      );
    } catch (error: unknown) {
      toast({ html: error instanceof Error ? error.message : t('chat', 'failed') });
    } finally {
      shellState.chatOpening = false;
    }
  }

  /** Reloads the list; opens the newest conversation, or a new one, when none is open. */
  async function loadConversations(client: FileManagerClient, key: string): Promise<void> {
    const entry = entryOf(key);
    if (entry === undefined) return;
    try {
      entry.conversations = await client.listChatConversations(entry.workspaceId);
      const open = entry.conversations.some((summary) => summary.id === entry.conversationId);
      if (!open) {
        const first = entry.conversations[0];
        if (first === undefined) await createConversation(client, key);
        else await selectConversation(client, key, first.id);
      }
    } catch {
      entry.error = t('chat', 'loadFailed');
    }
    m.redraw();
  }

  async function refreshList(client: FileManagerClient, key: string): Promise<void> {
    const entry = entryOf(key);
    if (entry === undefined) return;
    try {
      entry.conversations = await client.listChatConversations(entry.workspaceId);
      m.redraw();
    } catch {
      // The list refreshes on the next open; the reply itself already succeeded or failed.
    }
  }

  async function selectConversation(
    client: FileManagerClient,
    key: string,
    conversationId: string,
  ): Promise<void> {
    const entry = entryOf(key);
    if (entry === undefined) return;
    entry.conversationId = conversationId;
    entry.failure = undefined;
    entry.error = undefined;
    entry.streamedText = undefined;
    try {
      const conversation = await client.getChatConversation(entry.workspaceId, conversationId);
      if (entry.conversationId !== conversationId) return;
      entry.messages = [...conversation.messages];
    } catch {
      if (entry.conversationId === conversationId) entry.error = t('chat', 'loadFailed');
    }
    m.redraw();
  }

  /** Starts an empty conversation and makes it the open one. */
  async function createConversation(client: FileManagerClient, key: string): Promise<void> {
    const entry = entryOf(key);
    if (entry === undefined) return;
    try {
      const conversation = await client.createChatConversation(entry.workspaceId);
      entry.conversations = [
        { id: conversation.id, title: '', updatedAt: conversation.updatedAt, messageCount: 0 },
        ...entry.conversations,
      ];
      entry.conversationId = conversation.id;
      entry.messages = [];
      entry.failure = undefined;
      entry.error = undefined;
    } catch {
      entry.error = t('chat', 'loadFailed');
    }
    m.redraw();
  }

  async function renameConversation(
    client: FileManagerClient,
    key: string,
    conversationId: string,
    title: string,
  ): Promise<void> {
    const entry = entryOf(key);
    if (entry === undefined) return;
    try {
      const renamed = await client.renameChatConversation(entry.workspaceId, conversationId, title);
      entry.conversations = entry.conversations.map((summary) =>
        summary.id === conversationId ? { ...summary, title: renamed.title } : summary,
      );
    } catch {
      entry.error = t('chat', 'loadFailed');
    }
    m.redraw();
  }

  async function deleteConversation(
    client: FileManagerClient,
    key: string,
    conversationId: string,
  ): Promise<void> {
    const entry = entryOf(key);
    if (entry === undefined) return;
    try {
      await client.deleteChatConversation(entry.workspaceId, conversationId);
    } catch {
      entry.error = t('chat', 'loadFailed');
      m.redraw();
      return;
    }
    entry.conversations = entry.conversations.filter((summary) => summary.id !== conversationId);
    if (entry.conversationId === conversationId) {
      entry.conversationId = undefined;
      const next = entry.conversations[0];
      if (next === undefined) await createConversation(client, key);
      else await selectConversation(client, key, next.id);
    }
    m.redraw();
  }

  /**
   * Sends the person's message with the attached context, and streams the reply. The message is
   * shown at once; a reply that was stopped, or superseded, is ignored.
   */
  async function send(client: FileManagerClient, key: string, draft: string): Promise<void> {
    const entry = entryOf(key);
    if (entry === undefined || !canSendMessage(entry, draft)) return;
    const content = draft.trim();
    const context = contextForMessage(entry.attachments);
    entry.messages = [...entry.messages, { role: 'user', content, context }];
    await streamReply(client, key, { content, context, retry: false });
  }

  /** Asks again for the reply to the last message, after a failure. The message is not saved twice. */
  async function retry(client: FileManagerClient, key: string): Promise<void> {
    const entry = entryOf(key);
    if (entry === undefined || entry.pendingRequestId !== undefined) return;
    if (entry.conversationId === undefined || !lastIsPersonsMessage(entry)) return;
    await streamReply(client, key, { content: '', context: undefined, retry: true });
  }

  /** Streams one reply. On failure the person's message stays, with the attachments and images
   * restored and any partial reply kept on screen. */
  async function streamReply(
    client: FileManagerClient,
    key: string,
    request: { content: string; context: string | null | undefined; retry: boolean },
  ): Promise<void> {
    const entry = entryOf(key);
    const conversationId = entry?.conversationId;
    if (entry === undefined || conversationId === undefined) return;
    const attached = entry.attachments;
    const images = entry.images;
    const requestId = crypto.randomUUID();
    const isCurrent = (): boolean => entry.pendingRequestId === requestId;
    const finish = (): void => {
      entry.pendingRequestId = undefined;
      entry.streamedText = undefined;
    };
    entry.attachments = [];
    entry.images = [];
    entry.pendingRequestId = requestId;
    entry.streamedText = undefined;
    entry.partialReply = undefined;
    entry.failure = undefined;
    entry.error = undefined;
    m.redraw();
    try {
      await client.streamChatReply(
        {
          requestId,
          workspaceId: entry.workspaceId,
          conversationId,
          content: request.content,
          context: request.context ?? null,
          retry: request.retry,
          ...(images.length > 0 ? { images: images.map(({ data }) => ({ data })) } : {}),
        },
        (event) => {
          if (!isCurrent()) return;
          if (event.type === 'delta') {
            entry.streamedText = (entry.streamedText ?? '') + event.text;
          } else if (event.type === 'done') {
            entry.messages = [
              ...entry.messages,
              { role: 'assistant', content: event.text, context: null },
            ];
            finish();
          } else {
            entry.failure = event.reason;
            entry.partialReply = entry.streamedText;
            // Keep the attached context and images so the person can send again without re-adding.
            entry.attachments = attached;
            entry.images = images;
            entry.error = t('chat', failureKey(event.reason));
            finish();
          }
          m.redraw();
        },
      );
    } catch {
      if (isCurrent()) {
        entry.error = t('chat', 'failed');
        entry.partialReply = entry.streamedText;
        entry.attachments = attached;
        entry.images = images;
      }
    } finally {
      if (isCurrent()) finish();
      m.redraw();
      void refreshList(client, key);
    }
  }

  function lastIsPersonsMessage(entry: ChatTabEntry): boolean {
    return entry.messages[entry.messages.length - 1]?.role === 'user';
  }

  /** Stops the reply in flight. The person's message stays; no reply is saved. */
  function stop(client: FileManagerClient, key: string): void {
    const entry = entryOf(key);
    const requestId = entry?.pendingRequestId;
    if (entry === undefined || requestId === undefined) return;
    entry.pendingRequestId = undefined;
    entry.streamedText = undefined;
    m.redraw();
    void client.cancelChatReply(requestId).catch(() => undefined);
  }

  /** Adds an attachment unless the context of the next message would exceed the limit. */
  function addAttachment(entry: ChatTabEntry, attachment: ChatAttachment): boolean {
    if (attachedChars(entry.attachments) + attachment.text.length > MAX_ATTACHED_CHARS) {
      toast({ html: t('chat', 'contextTooLarge') });
      return false;
    }
    entry.attachments = [...entry.attachments, attachment];
    m.redraw();
    return true;
  }

  /** Text for the draft: the folder pane's current listing. The caller inserts it at the caret. */
  function attachFolder(key: string): ChatAttachment | undefined {
    const found = folderOf(key);
    if (found === undefined) return undefined;
    const { view } = found;
    if (view?.location === undefined) {
      toast({ html: t('chat', 'noFolder') });
      return undefined;
    }
    return textFitting(
      folderListingAttachment(crypto.randomUUID(), view.location.uri, view.entries),
    );
  }

  /** Text for the draft: the selected entries of the folder pane. The caller inserts it. */
  function attachSelection(key: string): ChatAttachment | undefined {
    const found = folderOf(key);
    if (found === undefined) return undefined;
    const { folderKey, view } = found;
    const selected = new Set(shellState.selections.get(folderKey)?.selectedEntryIds ?? []);
    const entries = (view?.entries ?? []).filter((item) => selected.has(item.id));
    if (view?.location === undefined || entries.length === 0) {
      toast({ html: t('chat', 'noSelection') });
      return undefined;
    }
    return textFitting(selectionAttachment(crypto.randomUUID(), view.location.uri, entries));
  }

  /** Inserted text must fit the context limit like any attachment; otherwise the person is told. */
  function textFitting(attachment: ChatAttachment): ChatAttachment | undefined {
    if (attachment.text.length > MAX_ATTACHED_CHARS) {
      toast({ html: t('chat', 'contextTooLarge') });
      return undefined;
    }
    return attachment;
  }

  /** The contents of the one file selected in the folder pane, read only on this request. */
  async function attachFileContents(client: FileManagerClient, key: string): Promise<void> {
    const found = folderOf(key);
    if (found === undefined) return;
    const { entry, folderKey, view } = found;
    const selected = new Set(shellState.selections.get(folderKey)?.selectedEntryIds ?? []);
    const files = (view?.entries ?? []).filter(
      (item) => selected.has(item.id) && item.kind === 'file',
    );
    const target = files.length === 1 ? files[0] : undefined;
    if (target === undefined) {
      toast({ html: t('chat', 'oneFileOnly') });
      return;
    }
    if (target.size !== undefined && target.size > MAX_ATTACHED_FILE_BYTES) {
      toast({ html: t('chat', 'fileTooLarge') });
      return;
    }
    try {
      const file = await client.loadEditableFile({ location: target.location });
      addAttachment(
        entry,
        fileContentAttachment(crypto.randomUUID(), target.name, file.size, file.content),
      );
    } catch {
      toast({ html: t('chat', 'attachFailed') });
    }
    m.redraw();
  }

  /** Whether the chat endpoint is marked as accepting images in Settings. */
  async function chatAcceptsImages(client: FileManagerClient): Promise<boolean> {
    const roles = await client.listLlmRoles();
    const profileId = roles.chat?.profileId;
    if (profileId === undefined) return false;
    const profile = (await client.listLlmProfiles()).find(
      (candidate) => candidate.id === profileId,
    );
    return profile?.capabilities.includes('vision') ?? false;
  }

  /** Adds pasted images to the next message, when the chat endpoint accepts them. */
  async function pasteImages(client: FileManagerClient, key: string, files: Blob[]): Promise<void> {
    const entry = entryOf(key);
    if (entry === undefined || files.length === 0) return;
    if (!(await chatAcceptsImages(client))) {
      toast({ html: t('chat', 'imagesNotSupported') });
      return;
    }
    if (entry.images.length + files.length > CHAT_IMAGE_MAX_COUNT) {
      toast({ html: t('chat', 'imageLimit') });
      return;
    }
    try {
      const encoded: PastedImage[] = [];
      for (const file of files) {
        encoded.push(await encodePastedImage(file, crypto.randomUUID()));
      }
      entry.images = [...entry.images, ...encoded];
    } catch {
      toast({ html: t('chat', 'imageFailed') });
    }
    m.redraw();
  }

  function removeImage(key: string, imageId: string): void {
    const entry = entryOf(key);
    if (entry === undefined) return;
    entry.images = entry.images.filter((image) => image.id !== imageId);
    m.redraw();
  }

  function detach(key: string, attachmentId: string): void {
    const entry = entryOf(key);
    if (entry === undefined) return;
    entry.attachments = entry.attachments.filter((attachment) => attachment.id !== attachmentId);
    m.redraw();
  }

  /** The folder pane is the other pane of the workspace; its active tab supplies the listing. */
  function folderOf(key: string) {
    const entry = entryOf(key);
    const workspace = shellState.workspace;
    if (entry === undefined || workspace === undefined) return undefined;
    const folderPaneId: PaneId | undefined = chatFolderPaneId(workspace.paneOrder, entry.paneId);
    const folderPane = folderPaneId === undefined ? undefined : workspace.panesById[folderPaneId];
    const folderTabId = folderPane?.activeTabId;
    if (folderPaneId === undefined || folderTabId === undefined) return undefined;
    const folderKey = tabKey(folderPaneId, folderTabId);
    return { entry, folderKey, view: shellState.directories.get(folderKey) };
  }

  /** Whether the other pane shows a folder that can be attached. */
  function folderAvailable(key: string): boolean {
    return folderOf(key)?.view?.location !== undefined;
  }

  /**
   * What the folder pane has selected, for the attach commands. `fileTooLarge` is set when the one
   * selected file is over the limit for attaching its contents.
   */
  function folderSelection(key: string): FolderSelection {
    const found = folderOf(key);
    if (found === undefined) return { selected: 0, selectedFiles: 0, fileTooLarge: false };
    const selectedIds = new Set(shellState.selections.get(found.folderKey)?.selectedEntryIds ?? []);
    const chosen = (found.view?.entries ?? []).filter((item) => selectedIds.has(item.id));
    const files = chosen.filter((item) => item.kind === 'file');
    const onlyFile = files.length === 1 ? files[0] : undefined;
    return {
      selected: chosen.length,
      selectedFiles: files.length,
      fileTooLarge: onlyFile?.size !== undefined && onlyFile.size > MAX_ATTACHED_FILE_BYTES,
    };
  }

  /** Forgets a closed chat tab; a reply still in flight is stopped. */
  function forget(client: FileManagerClient, key: string): void {
    const pending = entryOf(key)?.pendingRequestId;
    shellState.chatByTab.delete(key);
    if (pending !== undefined) void client.cancelChatReply(pending).catch(() => undefined);
  }

  return {
    openChat,
    select: (client: FileManagerClient, key: string, id: string) =>
      void selectConversation(client, key, id),
    newConversation: (client: FileManagerClient, key: string) =>
      void createConversation(client, key),
    rename: (client: FileManagerClient, key: string, id: string, title: string) =>
      void renameConversation(client, key, id, title),
    remove: (client: FileManagerClient, key: string, id: string) =>
      void deleteConversation(client, key, id),
    send: (client: FileManagerClient, key: string, draft: string) => void send(client, key, draft),
    retry: (client: FileManagerClient, key: string) => void retry(client, key),
    stop,
    attachFolder,
    attachSelection,
    attachFileContents: (client: FileManagerClient, key: string) =>
      void attachFileContents(client, key),
    detach,
    pasteImages: (client: FileManagerClient, key: string, files: Blob[]) =>
      void pasteImages(client, key, files),
    removeImage,
    folderAvailable,
    folderSelection,
    forget,
  };
}

/** What the other pane has selected, for enabling the attach commands. */
export interface FolderSelection {
  readonly selected: number;
  readonly selectedFiles: number;
  readonly fileTooLarge: boolean;
}

/** Keys of the messages that explain a failed reply. */
type ChatFailureKey =
  | 'consentRequired'
  | 'roleNotAssigned'
  | 'invalidRequest'
  | 'notFound'
  | 'imagesNotSupported'
  | 'noText'
  | 'failed';

/** The i18n key that explains a failed reply. */
function failureKey(reason: ChatFailure): ChatFailureKey {
  switch (reason) {
    case 'consentRequired':
      return 'consentRequired';
    case 'roleNotAssigned':
      return 'roleNotAssigned';
    case 'invalidRequest':
      return 'invalidRequest';
    case 'notFound':
      return 'notFound';
    case 'imagesNotSupported':
      return 'imagesNotSupported';
    case 'empty':
      return 'noText';
    default:
      return 'failed';
  }
}
