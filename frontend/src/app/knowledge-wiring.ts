import m from 'mithril';
import { toast } from 'mithril-materialized';
import type { FileManagerClient } from '../api/client/file-manager-client';
import { knowledgeEvidencePage } from '../features/semantic/evidence-provenance';
import { provenanceLine } from '../features/semantic/themes-model';
import { dispatchWorkspaceCommand } from '../features/workspace/dispatch-workspace-command';
import { pathFromUri } from '../features/workspace/workspace-layout';
import { t } from '../i18n';
import type {
  EntrySummary,
  KnowledgeEvidence,
  Location,
  PaneId,
  RagScope,
  SemanticComponentStatus,
  ThemePassage,
  WorkspaceProjection,
} from '../models';
import { tabKey } from './shell-helpers';
/** Sets up the knowledgeWiring feature wiring and returns its functions. */
import type { ShellState } from './shell-state';

/** The narrow cross-cutting surface knowledge/themes wiring needs from the shell. */
export interface KnowledgeCore {
  readonly openViewer: (
    client: FileManagerClient,
    paneId: PaneId,
    entry: EntrySummary,
    initialSearch?: {
      readonly query: string;
      readonly regex: boolean;
      readonly caseSensitive: boolean;
      readonly wholeWord: boolean;
    },
    openMetadata?: boolean,
    initialPage?: number,
    knowledgeSourceId?: string,
  ) => void;
  readonly openEditor: (
    client: FileManagerClient,
    paneId: PaneId,
    entry: EntrySummary,
    initialLine?: number,
  ) => void;
  readonly activeDirectory: () =>
    | { readonly paneId: PaneId; readonly location: Location }
    | undefined;
  readonly replaceWorkspace: (next: WorkspaceProjection) => void;
  readonly activatePane: (client: FileManagerClient, paneId: PaneId) => Promise<void>;
}

export function createKnowledgeWiring(shellState: ShellState, core: KnowledgeCore) {
  function openRagAsk(): void {
    openKnowledgeSearch();
  }

  function openThemes(): void {
    const currentWorkspace = shellState.workspace;
    if (currentWorkspace === undefined || shellState.themesOpening) return;
    const existing = [...shellState.themesByTab.values()].find(
      (entry) => currentWorkspace.panesById[entry.paneId] !== undefined,
    );
    if (existing !== undefined) {
      if (currentWorkspace.panesById[existing.paneId]?.activeTabId !== existing.tabId) {
        shellState.tabController.activateTab(existing.paneId, existing.tabId);
      } else if (currentWorkspace.activePaneId !== existing.paneId) {
        void core.activatePane(shellState.client, existing.paneId).catch((error: unknown) => {
          toast({ html: error instanceof Error ? error.message : t('themes', 'loadFailed') });
        });
      }
      return;
    }
    const base = {
      workspaceId: currentWorkspace.id,
      selectedFiles: [],
      semanticSourceIds: [],
      enrolledRootIds: [],
    };
    const active = core.activeDirectory();
    const scopes: RagScope[] = [
      { ...base, kind: 'entireLibrary', label: t('themes', 'scopeLibrary') },
      ...(active === undefined
        ? []
        : [
            {
              ...base,
              kind: 'currentFolder' as const,
              label: t('themes', 'scopeFolder'),
              folder: active.location,
            },
          ]),
    ];
    const paneId = currentWorkspace.activePaneId;
    const activeTab =
      currentWorkspace.panesById[paneId]?.tabsById[
        currentWorkspace.panesById[paneId]?.activeTabId ?? ''
      ];
    if (activeTab === undefined) return;
    shellState.themesOpening = true;
    void dispatchWorkspaceCommand(
      shellState.client,
      {
        type: 'addTransientTab',
        workspaceId: currentWorkspace.id,
        paneId,
        location: activeTab.location,
        expectedRevision: currentWorkspace.revision,
      },
      (next) => {
        core.replaceWorkspace(next);
        const tabId = next.panesById[paneId]?.activeTabId;
        if (tabId === undefined) return;
        const key = tabKey(paneId, tabId);
        shellState.themesByTab.set(key, { key, paneId, tabId, scopes });
        m.redraw();
      },
    )
      .catch((error: unknown) => {
        toast({ html: error instanceof Error ? error.message : t('themes', 'loadFailed') });
      })
      .finally(() => {
        shellState.themesOpening = false;
      });
  }

  async function openThemePassage(passage: ThemePassage): Promise<void> {
    const currentWorkspace = shellState.workspace;
    if (currentWorkspace === undefined) throw new Error(t('themes', 'openSourceFailed'));
    const targetPaneId =
      currentWorkspace.paneOrder.find((candidate) => candidate !== currentWorkspace.activePaneId) ??
      currentWorkspace.activePaneId;
    const source = await shellState.client.resolveKnowledgeSource({
      workspaceId: currentWorkspace.id,
      sourceId: passage.sourceId,
    });
    if (!source.available) throw new Error(t('themes', 'openSourceFailed'));
    const rawName =
      pathFromUri(source.location.uri).split(/[\\/]/).filter(Boolean).at(-1) ?? passage.title;
    let name = rawName;
    try {
      name = decodeURIComponent(rawName);
    } catch {
      // Keep the provider's original path segment when it is not URI encoded.
    }
    const dot = name.lastIndexOf('.');
    const entry: EntrySummary = {
      id: source.entryId,
      location: source.location,
      name,
      kind: 'file',
      hidden: false,
      readOnly: false,
      ...(dot > 0 ? { extension: name.slice(dot + 1).toLowerCase() } : {}),
      metadataRevision: 0,
    };
    const page = knowledgeEvidencePage(passage.provenance);
    const line = provenanceLine(passage.provenance);
    if (page === undefined && line !== undefined) {
      core.openEditor(shellState.client, targetPaneId, entry, line);
      return;
    }
    core.openViewer(
      shellState.client,
      targetPaneId,
      entry,
      undefined,
      false,
      page,
      passage.sourceId,
    );
  }

  function openKnowledgeSearch(): void {
    openKnowledgeSearchTab();
  }

  function openKnowledgeSearchTab(scoped?: {
    readonly semanticSourceIds: readonly string[];
    readonly initialSubject: string;
  }): void {
    const active = core.activeDirectory();
    const currentWorkspace = shellState.workspace;
    if (currentWorkspace === undefined) return;
    // A theme-scoped search always opens its own tab: an open session keeps the scope it started with.
    const existing =
      scoped === undefined
        ? [...shellState.knowledgeSearchByTab.values()].find(
            (entry) => entry.workspaceId === currentWorkspace.id,
          )
        : undefined;
    if (existing !== undefined) {
      if (currentWorkspace.panesById[existing.paneId]?.activeTabId !== existing.tabId) {
        shellState.tabController.activateTab(existing.paneId, existing.tabId);
      } else if (currentWorkspace.activePaneId !== existing.paneId) {
        void core.activatePane(shellState.client, existing.paneId).catch((error: unknown) => {
          toast({
            html: error instanceof Error ? error.message : t('knowledgeSearch', 'loadFailed'),
          });
        });
      }
      return;
    }
    if (shellState.knowledgeSearchOpening) return;
    const presentation =
      active === undefined
        ? undefined
        : shellState.findFilesPresentationsByLocationUri.get(active.location.uri);
    const semanticSourceIds = scoped?.semanticSourceIds ?? [
      ...new Set(
        (presentation?.semanticResults ?? []).flatMap((result) => [
          result.bestEvidence.sourceId,
          ...result.additionalSourceIds,
        ]),
      ),
    ];
    const paneId = currentWorkspace.activePaneId;
    const pane = currentWorkspace.panesById[paneId];
    const activeTab = pane?.tabsById[pane.activeTabId];
    if (activeTab === undefined) return;
    shellState.knowledgeSearchOpening = true;
    void dispatchWorkspaceCommand(
      shellState.client,
      {
        type: 'addTransientTab',
        workspaceId: currentWorkspace.id,
        paneId,
        location: activeTab.location,
        expectedRevision: currentWorkspace.revision,
      },
      (next) => {
        core.replaceWorkspace(next);
        const tabId = next.panesById[paneId]?.activeTabId;
        if (tabId === undefined) return;
        const key = tabKey(paneId, tabId);
        shellState.knowledgeSearchByTab.set(key, {
          key,
          paneId,
          tabId,
          workspaceId: next.id,
          currentFolder: active?.location,
          semanticSourceIds,
          initialSubject:
            scoped !== undefined
              ? scoped.initialSubject
              : presentation?.kind === 'semantic' || presentation?.kind === 'content'
                ? presentation.term
                : undefined,
        });
        m.redraw();
      },
    )
      .catch((error: unknown) => {
        toast({
          html: error instanceof Error ? error.message : t('knowledgeSearch', 'loadFailed'),
        });
      })
      .finally(() => {
        shellState.knowledgeSearchOpening = false;
      });
  }

  async function openKnowledgeSource(
    sourcePaneId: PaneId,
    evidence: KnowledgeEvidence,
  ): Promise<void> {
    const currentWorkspace = shellState.workspace;
    const targetPaneId = currentWorkspace?.paneOrder.find(
      (candidate) => candidate !== sourcePaneId,
    );
    if (currentWorkspace === undefined || targetPaneId === undefined) {
      throw new Error(t('knowledgeSearch', 'openSourceFailed'));
    }
    const source = await shellState.client.resolveKnowledgeSource({
      workspaceId: currentWorkspace.id,
      sourceId: evidence.sourceId,
    });
    if (!source.available) throw new Error(t('knowledgeSearch', 'openSourceFailed'));
    const path = pathFromUri(source.location.uri);
    const rawName = path.split(/[\\/]/).filter(Boolean).at(-1) ?? evidence.title;
    let name = rawName;
    try {
      name = decodeURIComponent(rawName);
    } catch {
      // Keep the provider's original path segment when it is not URI encoded.
    }
    const dot = name.lastIndexOf('.');
    const entry: EntrySummary = {
      id: source.entryId,
      location: source.location,
      name,
      kind: 'file',
      hidden: false,
      readOnly: false,
      ...(dot > 0 ? { extension: name.slice(dot + 1).toLowerCase() } : {}),
      ...(evidence.mediaType == null ? {} : { mimeType: evidence.mediaType }),
      ...(evidence.modifiedAtMs == null
        ? {}
        : { modifiedAt: new Date(evidence.modifiedAtMs).toISOString() }),
      metadataRevision: evidence.modifiedAtMs ?? 0,
    };
    core.openViewer(
      shellState.client,
      targetPaneId,
      entry,
      undefined,
      false,
      knowledgeEvidencePage(evidence.provenance),
      evidence.sourceId,
    );
  }

  async function refreshKnowledgeSearchAvailability(): Promise<void> {
    try {
      const capabilities = await shellState.client.getKnowledgeCapabilities();
      shellState.knowledgeSearchAvailable = capabilities.fullText || capabilities.semantic;
      shellState.themesAvailable = capabilities.semantic;
    } catch {
      shellState.knowledgeSearchAvailable = false;
      shellState.themesAvailable = false;
    } finally {
      shellState.knowledgeSearchChecked = true;
      m.redraw();
    }
  }

  async function refreshSemanticAssistantAvailability(
    currentComponents?: SemanticComponentStatus,
  ): Promise<void> {
    try {
      const [components, library, profiles] = await Promise.all([
        currentComponents ?? shellState.client.getSemanticComponentStatus(),
        shellState.client.getSemanticLibraryStatus(),
        shellState.client.listLlmProfiles(),
      ]);
      shellState.semanticAssistantAvailable =
        components.lifecycle.state === 'installedEnabled' &&
        components.activeModel != null &&
        library.available &&
        profiles.length > 0;
    } catch {
      shellState.semanticAssistantAvailable = false;
    }
    m.redraw();
  }

  function openSemanticAssistant(): void {
    if (!shellState.semanticAssistantAvailable) return;
    openRagAsk();
  }

  return {
    openRagAsk,
    openThemes,
    openThemePassage,
    openKnowledgeSearch,
    openKnowledgeSearchTab,
    openKnowledgeSource,
    refreshKnowledgeSearchAvailability,
    refreshSemanticAssistantAvailability,
    openSemanticAssistant,
  };
}
