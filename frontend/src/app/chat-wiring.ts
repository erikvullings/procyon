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
    const conversationId = entry?.conversationId;
    if (entry === undefined || conversationId === undefined || !canSendMessage(entry, draft)) {
      return;
    }
    const content = draft.trim();
    const attached = entry.attachments;
    const context = contextForMessage(attached);
    const requestId = crypto.randomUUID();
    const isCurrent = (): boolean => entry.pendingRequestId === requestId;
    const finish = (): void => {
      entry.pendingRequestId = undefined;
      entry.streamedText = undefined;
    };
    entry.messages = [...entry.messages, { role: 'user', content, context }];
    entry.attachments = [];
    entry.pendingRequestId = requestId;
    entry.streamedText = undefined;
    entry.failure = undefined;
    entry.error = undefined;
    m.redraw();
    try {
      await client.streamChatReply(
        { requestId, workspaceId: entry.workspaceId, conversationId, content, context },
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
            // Keep the attached context so the person can send again without re-attaching.
            entry.attachments = attached;
            entry.error = t('chat', failureKey(event.reason));
            finish();
          }
          m.redraw();
        },
      );
    } catch {
      if (isCurrent()) {
        entry.error = t('chat', 'failed');
        entry.attachments = attached;
      }
    } finally {
      if (isCurrent()) finish();
      m.redraw();
      void refreshList(client, key);
    }
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

  /** The folder pane's current listing, as context for the next message. */
  function attachFolder(key: string): void {
    const found = folderOf(key);
    if (found === undefined) return;
    const { entry, view } = found;
    if (view?.location === undefined) {
      toast({ html: t('chat', 'noFolder') });
      return;
    }
    addAttachment(
      entry,
      folderListingAttachment(crypto.randomUUID(), view.location.uri, view.entries),
    );
  }

  /** The selected entries of the folder pane, as context for the next message. */
  function attachSelection(key: string): void {
    const found = folderOf(key);
    if (found === undefined) return;
    const { entry, folderKey, view } = found;
    const selected = new Set(shellState.selections.get(folderKey)?.selectedEntryIds ?? []);
    const entries = (view?.entries ?? []).filter((item) => selected.has(item.id));
    if (view?.location === undefined || entries.length === 0) {
      toast({ html: t('chat', 'noSelection') });
      return;
    }
    addAttachment(entry, selectionAttachment(crypto.randomUUID(), view.location.uri, entries));
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

  /** How many entries, and how many files, the folder pane has selected; zero without a folder. */
  function folderSelection(key: string): FolderSelection {
    const found = folderOf(key);
    if (found === undefined) return { selected: 0, selectedFiles: 0 };
    const selectedIds = new Set(shellState.selections.get(found.folderKey)?.selectedEntryIds ?? []);
    const chosen = (found.view?.entries ?? []).filter((item) => selectedIds.has(item.id));
    return {
      selected: chosen.length,
      selectedFiles: chosen.filter((item) => item.kind === 'file').length,
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
    stop,
    attachFolder,
    attachSelection,
    attachFileContents: (client: FileManagerClient, key: string) =>
      void attachFileContents(client, key),
    detach,
    folderAvailable,
    folderSelection,
    forget,
  };
}

/** What the other pane has selected, for enabling the attach commands. */
export interface FolderSelection {
  readonly selected: number;
  readonly selectedFiles: number;
}

/** Keys of the messages that explain a failed reply. */
type ChatFailureKey =
  | 'consentRequired'
  | 'roleNotAssigned'
  | 'invalidRequest'
  | 'notFound'
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
    case 'empty':
      return 'noText';
    default:
      return 'failed';
  }
}
