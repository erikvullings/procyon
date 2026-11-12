import m from 'mithril';
import { toast } from 'mithril-materialized';
import type { FileManagerClient } from '../api/client/file-manager-client';
import {
  type AssistantTabEntry,
  oppositePaneId,
  transcriptForRequest,
} from '../features/assistant/assistant-tab-state';
import { dispatchWorkspaceCommand } from '../features/workspace/dispatch-workspace-command';
import { t } from '../i18n';
import type { LlmProfile, LlmRoles, PaneId, WorkspaceProjection } from '../models';
import { preferredProfile } from '../models/llm-roles';
import { tabKey } from './shell-helpers';
import type { ShellState } from './shell-state';

export interface AssistantCore {
  readonly replaceWorkspace: (next: WorkspaceProjection) => void;
}

/** Opens, drives and closes the file-assistant chat tabs (task 0321). */
export function createAssistantWiring(shellState: ShellState, core: AssistantCore) {
  /** Opens a chat in the pane opposite the editor at `sourcePaneId`. */
  async function openAssistant(sourcePaneId: PaneId): Promise<void> {
    const workspace = shellState.workspace;
    const editor = shellState.editorByPane.get(sourcePaneId);
    if (workspace === undefined || editor === undefined || shellState.assistantOpening) return;
    const targetPaneId = oppositePaneId(workspace.paneOrder, sourcePaneId);
    if (targetPaneId === undefined) {
      toast({ html: t('assistant', 'noOtherPane') });
      return;
    }
    const targetPane = workspace.panesById[targetPaneId];
    const activeTab =
      targetPane === undefined ? undefined : targetPane.tabsById[targetPane.activeTabId ?? ''];
    if (activeTab === undefined) return;

    let profiles: LlmProfile[];
    try {
      profiles = await shellState.client.listLlmProfiles();
    } catch {
      toast({ html: t('assistant', 'loadFailed') });
      return;
    }
    if (profiles.length === 0) {
      toast({ html: t('assistant', 'noProfiles') });
      return;
    }
    // Start on the chat role's endpoint when it is still saved (0322); otherwise the first profile.
    let roles: LlmRoles = { chat: null, embeddings: null };
    try {
      roles = await shellState.client.listLlmRoles();
    } catch {
      // The first saved profile is used when the roles cannot be read.
    }
    const fileName = editor.state.entry.name;
    shellState.assistantOpening = true;
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
          shellState.assistantByTab.set(key, {
            key,
            paneId: targetPaneId,
            tabId,
            sourcePaneId,
            fileName,
            profiles,
            profileId: preferredProfile(profiles, roles.chat)?.id ?? '',
            messages: [],
          });
          m.redraw();
        },
      );
    } catch (error: unknown) {
      toast({ html: error instanceof Error ? error.message : t('assistant', 'failed') });
    } finally {
      shellState.assistantOpening = false;
    }
  }

  /**
   * Sends the person's draft and streams the reply into the chat. Events of a request that was
   * stopped, or superseded, are ignored.
   */
  async function send(client: FileManagerClient, key: string, draft: string): Promise<void> {
    const entry = shellState.assistantByTab.get(key);
    const text = draft.trim();
    if (entry === undefined || text === '' || entry.profileId === '') return;
    if (entry.pendingRequestId !== undefined) return;
    const requestId = crypto.randomUUID();
    const isCurrent = (): boolean => entry.pendingRequestId === requestId;
    entry.messages = [...entry.messages, { role: 'user', content: text }];
    entry.pendingRequestId = requestId;
    entry.streamedText = undefined;
    entry.error = undefined;
    entry.needsConsent = undefined;
    m.redraw();
    const finish = (): void => {
      entry.pendingRequestId = undefined;
      entry.streamedText = undefined;
    };
    try {
      await client.streamFileAssistantReply(
        {
          requestId,
          profileId: entry.profileId,
          fileName: entry.fileName,
          messages: transcriptForRequest(entry.messages),
        },
        (event) => {
          if (!isCurrent()) return;
          if (event.type === 'delta') {
            entry.streamedText = (entry.streamedText ?? '') + event.text;
          } else if (event.type === 'done') {
            entry.messages = [...entry.messages, { role: 'assistant', content: event.text }];
            finish();
          } else if (event.reason === 'consentRequired') {
            entry.needsConsent = true;
            entry.error = t('assistant', 'consentRequired');
            finish();
          } else {
            entry.error = t('assistant', 'failed');
            finish();
          }
          m.redraw();
        },
      );
    } catch {
      if (isCurrent()) entry.error = t('assistant', 'failed');
    } finally {
      if (isCurrent()) finish();
      m.redraw();
    }
  }

  /** Stops the request in flight. Its reply, if it still arrives, is discarded. */
  function stop(client: FileManagerClient, key: string): void {
    const entry = shellState.assistantByTab.get(key);
    const requestId = entry?.pendingRequestId;
    if (entry === undefined || requestId === undefined) return;
    entry.pendingRequestId = undefined;
    entry.streamedText = undefined;
    m.redraw();
    void client.cancelFileAssistantReply(requestId).catch(() => undefined);
  }

  /** Places an assistant reply into the editor it was drafted for. */
  function insert(key: string, index: number): void {
    const entry: AssistantTabEntry | undefined = shellState.assistantByTab.get(key);
    const message = entry?.messages[index];
    if (entry === undefined || message === undefined || message.role !== 'assistant') return;
    const editor = shellState.editorByPane.get(entry.sourcePaneId);
    if (editor === undefined) {
      toast({ html: t('assistant', 'sourceClosed') });
      return;
    }
    editor.controller.setContent(message.content);
    toast({ html: t('assistant', 'inserted') });
  }

  /** Forgets a closed chat tab; a request still in flight is cancelled. */
  function forget(client: FileManagerClient, key: string): void {
    const pending = shellState.assistantByTab.get(key)?.pendingRequestId;
    shellState.assistantByTab.delete(key);
    if (pending !== undefined) void client.cancelFileAssistantReply(pending).catch(() => undefined);
  }

  function setProfile(key: string, profileId: string): void {
    const entry = shellState.assistantByTab.get(key);
    if (entry !== undefined) entry.profileId = profileId;
  }

  return { openAssistant, send, stop, insert, forget, setProfile };
}
