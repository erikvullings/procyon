//! `WorkspaceService`: workspace startup lifecycle, CRUD orchestration and
//! semantic mutation commands (spec §5.3.7, §5.3.9, tasks 0079, 0080).

use std::path::PathBuf;

use fm_domain::{Workspace, WorkspaceCommand, WorkspaceId};

use super::command;
use super::default_workspace::{
    build_tab, default_workspace, location_for, resolve_home_directory,
};
use super::error::WorkspaceError;
use super::events;
use super::publisher::{NoopWorkspaceCommandPublisher, WorkspaceCommandPublisher};
use super::repository::{LastActiveWorkspaceStore, WorkspaceRepository, WorkspaceSummary};

/// Orchestrates workspace persistence, the startup lifecycle (spec §5.3.7)
/// and semantic mutation commands (spec §5.3.9) on top of any `R`
/// implementing both [`WorkspaceRepository`] and [`LastActiveWorkspaceStore`].
pub struct WorkspaceService<R> {
    repository: R,
    home_directory: PathBuf,
    secondary_location: Option<PathBuf>,
    publisher: Box<dyn WorkspaceCommandPublisher>,
}

impl<R> WorkspaceService<R>
where
    R: WorkspaceRepository + LastActiveWorkspaceStore,
{
    /// Builds a service backed by `repository`, resolving the home directory
    /// through the platform seam (spec §5.3.7) rather than a hard-coded path.
    pub fn new(repository: R) -> Self {
        Self {
            repository,
            home_directory: resolve_home_directory(),
            secondary_location: None,
            publisher: Box::new(NoopWorkspaceCommandPublisher),
        }
    }

    /// Overrides the second pane's initial location for newly created
    /// default workspaces (spec §5.3.7's "or a configured secondary location
    /// for the right pane").
    #[must_use]
    pub fn with_secondary_location(mut self, secondary_location: PathBuf) -> Self {
        self.secondary_location = Some(secondary_location);
        self
    }

    /// Overrides the publisher notified after every successful
    /// [`WorkspaceCommand`] (spec §5.3.9 step 7). Defaults to a no-op until a
    /// host wires in the real event bus (task 0081).
    #[must_use]
    pub fn with_publisher(mut self, publisher: impl WorkspaceCommandPublisher + 'static) -> Self {
        self.publisher = Box::new(publisher);
        self
    }

    /// Lists every stored workspace as a lightweight summary.
    pub async fn list(&self) -> Result<Vec<WorkspaceSummary>, WorkspaceError> {
        self.repository.list().await
    }

    /// Loads and validates a single workspace by id.
    pub async fn load(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        let workspace = self
            .repository
            .load(id)
            .await?
            .ok_or(WorkspaceError::NotFound { id })?;
        validate_or_error(&workspace)?;
        Ok(workspace)
    }

    /// Creates and persists a fresh workspace shaped like the default
    /// workspace (spec §5.3.7's "Default workspace"), optionally overriding
    /// its name. Does not mark it as the last-active workspace; callers
    /// wanting that should call [`WorkspaceService::open`] afterwards.
    ///
    /// Workspace names must stay unique (case-insensitive, trimmed) so
    /// they're distinguishable in the switcher and Window menu. An
    /// explicitly requested `name` that collides is rejected with
    /// [`WorkspaceError::DuplicateName`]; the default name (used whenever the
    /// frontend's "New Workspace" button asks for `None`, since it never lets
    /// a user type a name up front) is silently deduplicated instead by
    /// appending " 2", " 3", etc., so that button always succeeds.
    pub async fn create(&self, name: Option<String>) -> Result<Workspace, WorkspaceError> {
        let mut workspace =
            default_workspace(&self.home_directory, self.secondary_location.as_deref());
        match name {
            Some(name) => {
                self.ensure_name_available(&name, None).await?;
                workspace.name = name;
            }
            None => {
                workspace.name = self.unique_name(&workspace.name).await?;
            }
        }
        validate_or_error(&workspace)?;
        let persisted = self.repository.save(&workspace, None).await?;
        self.publisher
            .publish(persisted.id, events::workspace_created(&persisted));
        Ok(persisted)
    }

    /// Creates, persists and selects a fresh default workspace (spec §5.3.7's
    /// "Default workspace"), emitting both `workspace.created` and
    /// `workspace.opened` (creating a workspace this way always also selects
    /// it as active).
    pub async fn create_default(&self) -> Result<Workspace, WorkspaceError> {
        let persisted = self.create(None).await?;
        self.repository
            .set_last_active_workspace_id(Some(persisted.id))
            .await?;
        self.publisher
            .publish(persisted.id, events::workspace_opened(&persisted));
        Ok(persisted)
    }

    /// Forks a brand-new ephemeral (per-window) workspace, copying `source_id`'s
    /// current shape (layout, panes, active pane, operation-centre preferences) if
    /// given, or the hardcoded default shape (home directory, two panes) if `None` -
    /// ephemeral per-window workspaces spec follow-up.
    ///
    /// Unlike [`WorkspaceService::create`], the *forked* (ephemeral) workspace itself
    /// never becomes the last-active workspace and its name is never checked for
    /// uniqueness: ephemeral workspaces are excluded from the switcher, so neither
    /// matters. Its named *source*, though, is marked last-active whenever one is
    /// given - a window is "using" that named workspace for as long as it's open,
    /// even before any resync, so a later cold start (`start`, e.g. a fresh window
    /// opened from the Dock with no source of its own) reopens it instead of falling
    /// back to the hardcoded default (ephemeral per-window workspaces spec
    /// follow-up). A `source_id` that no longer exists surfaces the same
    /// [`WorkspaceError::NotFound`] `load` already produces for any other missing
    /// workspace.
    pub async fn fork(&self, source_id: Option<WorkspaceId>) -> Result<Workspace, WorkspaceError> {
        let mut workspace = match source_id {
            Some(id) => self.load(id).await?,
            None => default_workspace(&self.home_directory, self.secondary_location.as_deref()),
        };
        workspace.id = WorkspaceId::new();
        workspace.ephemeral = true;
        workspace.forked_from = source_id;

        let persisted = self.repository.save(&workspace, None).await?;
        if let Some(source_id) = source_id {
            self.repository
                .set_last_active_workspace_id(Some(source_id))
                .await?;
        }
        self.publisher
            .publish(persisted.id, events::workspace_created(&persisted));
        Ok(persisted)
    }

    /// Writes `source_id`'s current shape (layout, panes, active pane, operation-centre
    /// preferences) into a named workspace, keeping that workspace's own id/name/created_at.
    /// Also marks the target as last-active: a workspace that was just synced is the one a
    /// later cold start should reopen (same reasoning as [`WorkspaceService::fork`]'s source).
    ///
    /// `target_id`, when given (the workspace switcher's per-row "Update" button - ephemeral
    /// per-window workspaces spec follow-up), names which named workspace receives the sync -
    /// any saved workspace, not just the one `source_id` was originally forked from, and
    /// `source_id` itself need not be ephemeral (any window's live workspace, including the
    /// main/dock window's own non-ephemeral one, can be the source of an explicit update).
    /// If `source_id` is itself ephemeral, it is relinked to `target_id`, so a later default
    /// resync (the File menu's "Sync Workspace", which always omits `target_id`) keeps
    /// following the same target instead of drifting back to the original source. Errors with
    /// [`WorkspaceError::TargetIsEphemeral`] if `target_id` names an ephemeral workspace.
    ///
    /// With no `target_id`, `source_id` must itself be ephemeral (errors with
    /// [`WorkspaceError::NotEphemeral`] otherwise - a non-ephemeral workspace's shape is already
    /// live, there is nothing to sync it into without an explicit target). Falls back to its own
    /// `forked_from` - or, if it was seeded from the hardcoded default and has no source yet,
    /// creates a brand-new named workspace from it and links it to that new workspace so a
    /// later resync updates it instead of creating another.
    ///
    /// Returns the target (explicitly requested, existing source, or newly created) named
    /// workspace, never `source_id` itself.
    pub async fn resync(
        &self,
        source_id: WorkspaceId,
        target_id: Option<WorkspaceId>,
    ) -> Result<Workspace, WorkspaceError> {
        let source = self.load(source_id).await?;
        if target_id.is_none() && !source.ephemeral {
            return Err(WorkspaceError::NotEphemeral { id: source_id });
        }

        let target = match target_id.or(source.forked_from) {
            Some(target_id) => {
                let mut target = self.load(target_id).await?;
                if target.ephemeral {
                    return Err(WorkspaceError::TargetIsEphemeral { id: target_id });
                }
                target.layout = source.layout.clone();
                target.panes = source.panes.clone();
                target.active_pane_id = source.active_pane_id;
                target.operation_centre = source.operation_centre;
                let expected_revision = target.revision;
                let persisted = self
                    .repository
                    .save(&target, Some(expected_revision))
                    .await?;

                if source.ephemeral && source.forked_from != Some(target_id) {
                    let mut relinked = source.clone();
                    relinked.forked_from = Some(target_id);
                    self.repository
                        .save(&relinked, Some(source.revision))
                        .await?;
                }

                persisted
            }
            None => {
                let mut named = source.clone();
                named.id = WorkspaceId::new();
                named.ephemeral = false;
                named.forked_from = None;
                named.name = self.unique_name("Untitled").await?;
                let persisted = self.repository.save(&named, None).await?;

                let mut relinked = source.clone();
                relinked.forked_from = Some(persisted.id);
                self.repository
                    .save(&relinked, Some(source.revision))
                    .await?;

                persisted
            }
        };

        self.repository
            .set_last_active_workspace_id(Some(target.id))
            .await?;
        self.publisher
            .publish(target.id, events::workspace_created(&target));
        Ok(target)
    }

    /// Deletes a workspace.
    pub async fn delete(
        &self,
        id: WorkspaceId,
        expected_revision: Option<u64>,
    ) -> Result<(), WorkspaceError> {
        // A corrupt or already-vanished workspace can still be deleted; only
        // the event's revision field defaults to 0 in that edge case, not
        // the deletion's own revision-conflict check below.
        let revision = self
            .repository
            .load(id)
            .await
            .ok()
            .flatten()
            .map(|workspace| workspace.revision)
            .unwrap_or_default();

        self.repository.delete(id, expected_revision).await?;
        self.publisher
            .publish(id, events::workspace_deleted(revision));
        Ok(())
    }

    /// Selects an existing workspace as the last-active workspace and
    /// returns its current projection (spec §5.3.12's `openWorkspace`).
    ///
    /// Unlike [`WorkspaceService::start`], a missing or corrupt workspace is
    /// reported rather than silently replaced with a fresh default: that
    /// recovery behaviour is specific to application startup, not to an
    /// explicit request to open a named workspace.
    pub async fn open(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        let previous_active_id = self.repository.last_active_workspace_id().await?;

        let workspace = self.load(id).await?;
        self.repository
            .set_last_active_workspace_id(Some(id))
            .await?;

        if let Some(previous_id) = previous_active_id
            && previous_id != id
            && let Ok(Some(previous)) = self.repository.load(previous_id).await
        {
            self.publisher
                .publish(previous_id, events::workspace_closed(previous.revision));
        }
        self.publisher
            .publish(workspace.id, events::workspace_opened(&workspace));

        Ok(workspace)
    }

    /// Applies a semantic mutation command (spec §5.3.9): verifies the
    /// expected revision, validates and applies the mutation, persists the
    /// result (which increments the revision) and notifies the configured
    /// publisher with the focused event describing the change, returning the
    /// changed projection.
    ///
    /// Runtime-session updates (spec §5.3.9 step 6) are deferred: no
    /// runtime-session concept exists yet.
    pub async fn apply_command(
        &self,
        command: WorkspaceCommand,
    ) -> Result<Workspace, WorkspaceError> {
        let workspace_id = command.workspace_id();
        let expected_revision = command.expected_revision();

        let mut workspace = self.load(workspace_id).await?;
        if workspace.revision != expected_revision {
            return Err(WorkspaceError::RevisionConflict {
                id: workspace_id,
                expected: Some(expected_revision),
                actual: workspace.revision,
            });
        }

        if let WorkspaceCommand::RenameWorkspace { ref name, .. } = command {
            self.ensure_name_available(name, Some(workspace_id)).await?;
        }

        command::apply(&mut workspace, command.clone(), &self.home_directory)?;

        let persisted = self
            .repository
            .save(&workspace, Some(expected_revision))
            .await?;

        let event = events::command_event(&command, &persisted)?;
        self.publisher.publish(persisted.id, event);

        Ok(persisted)
    }

    /// Runs the startup lifecycle (spec §5.3.7 steps 1-4): select an
    /// explicitly requested workspace, otherwise the last-active one,
    /// otherwise the most-recently-updated named workspace already on disk (or the first one
    /// `list()` returns, if several tie), otherwise create a fresh default; a missing or
    /// corrupt selection is recovered from by falling back to the most-recently-updated named
    /// workspace before creating a fresh default. The most-recently-updated fallback matters
    /// because not every named workspace
    /// ever becomes explicitly last-active - one only created via [`WorkspaceService::create`]
    /// or resynced from an ephemeral window (ephemeral per-window workspaces spec follow-up)
    /// never was, until [`WorkspaceService::fork`]/[`WorkspaceService::resync`] started marking
    /// their source/target last-active - without this fallback, a cold start (a fresh window
    /// from the Dock, or the very first launch after upgrading) would keep creating a brand-new
    /// throwaway default instead of reopening whatever the user actually saved. Emits
    /// `workspace.opened` (spec §5.3.7 step 8) exactly once: [`WorkspaceService::create_default`]
    /// already emits it when a fresh default is selected, so this only emits it itself when
    /// reselecting an existing workspace.
    pub async fn start(
        &self,
        requested_workspace_id: Option<WorkspaceId>,
    ) -> Result<Workspace, WorkspaceError> {
        let selected_id = match requested_workspace_id {
            Some(id) => Some(id),
            None => match self.repository.last_active_workspace_id().await? {
                Some(id) => Some(id),
                None => self.most_recently_updated_named_workspace_id(None).await?,
            },
        };
        self.discard_all_transient_tabs().await?;

        let workspace = match selected_id {
            Some(id) => match self.load(id).await {
                Ok(workspace) => {
                    let workspace = self.discard_transient_tabs(workspace).await?;
                    if !workspace.ephemeral {
                        self.repository
                            .set_last_active_workspace_id(Some(workspace.id))
                            .await?;
                    } else if let Some(source_id) = workspace.forked_from {
                        self.repository
                            .set_last_active_workspace_id(Some(source_id))
                            .await?;
                    }
                    self.publisher
                        .publish(workspace.id, events::workspace_opened(&workspace));
                    workspace
                }
                Err(WorkspaceError::NotFound { .. } | WorkspaceError::Corrupt { .. }) => {
                    let fallback_id = if requested_workspace_id.is_none() {
                        self.most_recently_updated_named_workspace_id(Some(id))
                            .await?
                    } else {
                        None
                    };
                    match fallback_id {
                        Some(fallback_id) if fallback_id != id => {
                            let workspace = self.load(fallback_id).await?;
                            let workspace = self.discard_transient_tabs(workspace).await?;
                            self.repository
                                .set_last_active_workspace_id(Some(workspace.id))
                                .await?;
                            self.publisher
                                .publish(workspace.id, events::workspace_opened(&workspace));
                            workspace
                        }
                        _ => self.create_default().await?,
                    }
                }
                Err(error) => return Err(error),
            },
            None => self.create_default().await?,
        };

        Ok(workspace)
    }

    async fn discard_all_transient_tabs(&self) -> Result<(), WorkspaceError> {
        for summary in self.repository.list().await? {
            match self.load(summary.id).await {
                Ok(workspace) => {
                    self.discard_transient_tabs(workspace).await?;
                }
                Err(WorkspaceError::NotFound { .. } | WorkspaceError::Corrupt { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    async fn discard_transient_tabs(
        &self,
        mut workspace: Workspace,
    ) -> Result<Workspace, WorkspaceError> {
        let mut changed = false;
        for pane in &mut workspace.panes {
            let tab_count = pane.tabs.len();
            pane.tabs
                .retain(|tab| !tab.transient && !is_session_only_uri(&tab.location.uri));
            changed |= pane.tabs.len() != tab_count;
            if pane.tabs.is_empty() {
                let mut tab = build_tab(location_for(&self.home_directory));
                tab.view = pane.default_view.clone();
                pane.active_tab_id = tab.id;
                pane.tabs.push(tab);
            } else if !pane.tabs.iter().any(|tab| tab.id == pane.active_tab_id) {
                pane.active_tab_id = pane.tabs[0].id;
            }
        }
        if changed {
            self.repository
                .save(&workspace, Some(workspace.revision))
                .await
        } else {
            Ok(workspace)
        }
    }

    /// The id of the named (non-ephemeral) workspace that was persisted most recently, or the
    /// first one `list()` returns on a tie ("last used, or the first if all are equal") - `None`
    /// if no named workspace exists yet. See [`WorkspaceService::start`]'s fallback chain.
    async fn most_recently_updated_named_workspace_id(
        &self,
        excluding: Option<WorkspaceId>,
    ) -> Result<Option<WorkspaceId>, WorkspaceError> {
        let mut best: Option<WorkspaceSummary> = None;
        for summary in self
            .list()
            .await?
            .into_iter()
            .filter(|s| !s.ephemeral && Some(s.id) != excluding)
        {
            let replace = match &best {
                Some(current) => summary.updated_at > current.updated_at,
                None => true,
            };
            if replace {
                best = Some(summary);
            }
        }
        Ok(best.map(|summary| summary.id))
    }

    /// Rejects `name` (trimmed, case-insensitive) if any other stored
    /// workspace already has it. `excluding` is the workspace being renamed,
    /// if any - renaming a workspace to the name it already has must not
    /// collide with itself.
    async fn ensure_name_available(
        &self,
        name: &str,
        excluding: Option<WorkspaceId>,
    ) -> Result<(), WorkspaceError> {
        let summaries = self.repository.list().await?;
        let collides = summaries
            .iter()
            .any(|summary| Some(summary.id) != excluding && names_match(&summary.name, name));
        if collides {
            return Err(WorkspaceError::DuplicateName {
                name: name.to_owned(),
            });
        }
        Ok(())
    }

    /// Returns `base` unchanged if no stored workspace already has that name
    /// (trimmed, case-insensitive); otherwise appends " 2", " 3", etc. until
    /// finding one that's free.
    async fn unique_name(&self, base: &str) -> Result<String, WorkspaceError> {
        let summaries = self.repository.list().await?;
        let taken = |candidate: &str| {
            summaries
                .iter()
                .any(|summary| names_match(&summary.name, candidate))
        };
        if !taken(base) {
            return Ok(base.to_owned());
        }
        let mut suffix = 2u32;
        loop {
            let candidate = format!("{base} {suffix}");
            if !taken(&candidate) {
                return Ok(candidate);
            }
            suffix += 1;
        }
    }
}

fn names_match(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn is_session_only_uri(uri: &str) -> bool {
    uri.starts_with("search://") || uri.starts_with("archive://")
}

fn validate_or_error(workspace: &Workspace) -> Result<(), WorkspaceError> {
    workspace.validate().map_err(WorkspaceError::Invalid)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use fm_domain::{Location, PaneId, ProviderId};
    use fm_events::BackendEventPayload;

    use super::super::memory::InMemoryWorkspaceRepository;
    use super::*;

    fn service() -> WorkspaceService<InMemoryWorkspaceRepository> {
        WorkspaceService::new(InMemoryWorkspaceRepository::new())
            .with_secondary_location(PathBuf::from("/Users/erik/Downloads"))
    }

    #[tokio::test]
    async fn start_with_no_stored_workspace_creates_a_valid_default() {
        let service = service();

        let workspace = service.start(None).await.expect("start must succeed");

        assert_eq!(workspace.name, "Default");
        assert_eq!(workspace.panes.len(), 2);
        assert!(workspace.validate().is_ok());
        assert_eq!(service.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn start_reselects_the_last_active_workspace_on_a_second_call() {
        let service = service();
        let first_start = service.start(None).await.expect("first start must succeed");

        // A second "restart" with no explicit request must reselect the same
        // workspace rather than creating another default.
        let second_start = service
            .start(None)
            .await
            .expect("second start must succeed");

        assert_eq!(first_start.id, second_start.id);
        assert_eq!(service.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn start_discards_transient_tabs_but_preserves_normal_duplicate_tabs() {
        let service = service();
        let workspace = service.start(None).await.expect("start must succeed");
        let pane_id = workspace.active_pane_id;
        let location = workspace.panes[0].tabs[0].location.clone();
        let duplicate = service
            .apply_command(WorkspaceCommand::AddTab {
                workspace_id: workspace.id,
                pane_id,
                location: location.clone(),
                expected_revision: workspace.revision,
            })
            .await
            .unwrap();
        let with_transient = service
            .apply_command(WorkspaceCommand::AddTransientTab {
                workspace_id: duplicate.id,
                pane_id,
                location,
                expected_revision: duplicate.revision,
            })
            .await
            .unwrap();
        assert_eq!(with_transient.panes[0].tabs.len(), 3);

        let restarted = service.start(Some(workspace.id)).await.unwrap();

        assert_eq!(restarted.panes[0].tabs.len(), 2);
        assert!(restarted.panes[0].tabs.iter().all(|tab| !tab.transient));
        assert_eq!(
            service.load(workspace.id).await.unwrap().panes[0]
                .tabs
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn start_discards_legacy_search_tabs_that_predate_the_transient_flag() {
        let service = service();
        let workspace = service.start(None).await.expect("start must succeed");
        let pane_id = workspace.active_pane_id;
        let with_legacy_search = service
            .apply_command(WorkspaceCommand::AddTab {
                workspace_id: workspace.id,
                pane_id,
                location: Location::new(
                    ProviderId::new("search"),
                    "search://local/11111111-1111-4111-8111-111111111111",
                ),
                expected_revision: workspace.revision,
            })
            .await
            .expect("legacy search tab creation must succeed");
        assert!(
            with_legacy_search.panes[0].tabs.iter().any(|tab| tab
                .location
                .uri
                .starts_with("search://")
                && !tab.transient)
        );

        let restarted = service.start(Some(workspace.id)).await.unwrap();

        assert!(
            restarted.panes[0]
                .tabs
                .iter()
                .all(|tab| !tab.location.uri.starts_with("search://"))
        );
        assert_eq!(restarted.panes[0].tabs.len(), 1);
    }

    #[tokio::test]
    async fn start_discards_transient_tabs_from_non_selected_workspaces() {
        let service = service();
        let selected = service.start(None).await.expect("start must succeed");
        let other = service
            .create(Some("Other".to_owned()))
            .await
            .expect("workspace creation must succeed");
        let pane_id = other.active_pane_id;
        let location = other.panes[0].tabs[0].location.clone();
        let other = service
            .apply_command(WorkspaceCommand::AddTransientTab {
                workspace_id: other.id,
                pane_id,
                location,
                expected_revision: other.revision,
            })
            .await
            .expect("transient tab creation must succeed");
        assert!(other.panes[0].tabs.iter().any(|tab| tab.transient));

        service
            .start(Some(selected.id))
            .await
            .expect("restart must succeed");

        let cleaned = service
            .load(other.id)
            .await
            .expect("other workspace must remain loadable");
        assert!(cleaned.panes[0].tabs.iter().all(|tab| !tab.transient));
    }

    #[tokio::test]
    async fn start_honours_an_explicitly_requested_workspace_id() {
        let service = service();
        let default_workspace = service.start(None).await.expect("start must succeed");

        let explicit = default_workspace_service_second_workspace(&service).await;

        let selected = service
            .start(Some(explicit.id))
            .await
            .expect("start with an explicit id must succeed");
        assert_eq!(selected.id, explicit.id);
        assert_ne!(selected.id, default_workspace.id);
    }

    async fn default_workspace_service_second_workspace(
        service: &WorkspaceService<InMemoryWorkspaceRepository>,
    ) -> Workspace {
        // Build a second, distinct workspace directly through the repository
        // to exercise "explicitly requested" selection.
        let mut workspace = default_workspace(Path::new("/Users/erik"), None);
        workspace.name = "Photos".to_owned();
        service.repository.save(&workspace, None).await.unwrap()
    }

    #[tokio::test]
    async fn start_recovers_from_a_missing_last_active_workspace_by_reopening_the_latest_named_one()
    {
        let service = service();
        let existing = service
            .create(Some("Existing".to_owned()))
            .await
            .expect("workspace creation must succeed");
        service
            .repository
            .set_last_active_workspace_id(Some(WorkspaceId::new()))
            .await
            .unwrap();

        let workspace = service
            .start(None)
            .await
            .expect("start must recover, not fail");
        assert_eq!(workspace.id, existing.id);
        assert_eq!(service.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn explicitly_starting_an_ephemeral_workspace_keeps_its_named_source_last_active() {
        let service = service();
        let named = service
            .create(Some("Existing".to_owned()))
            .await
            .expect("workspace creation must succeed");
        let ephemeral = service
            .fork(Some(named.id))
            .await
            .expect("fork must succeed");

        let started = service
            .start(Some(ephemeral.id))
            .await
            .expect("explicit ephemeral start must succeed");

        assert_eq!(started.id, ephemeral.id);
        assert_eq!(
            service.repository.last_active_workspace_id().await.unwrap(),
            Some(named.id)
        );
    }

    #[tokio::test]
    async fn persisted_restart_recovers_named_tabs_when_the_recorded_ephemeral_is_missing() {
        use super::super::persistent::JsonFileWorkspaceRepository;

        let dir = tempfile::TempDir::new().expect("temp dir");
        let first = WorkspaceService::new(JsonFileWorkspaceRepository::new(dir.path()));
        let named = first
            .create(Some("Existing".to_owned()))
            .await
            .expect("workspace creation must succeed");
        let pane_id = named.active_pane_id;
        let named = first
            .apply_command(WorkspaceCommand::AddTab {
                workspace_id: named.id,
                pane_id,
                location: Location::new(ProviderId::new("local"), "file:///saved-tab"),
                expected_revision: named.revision,
            })
            .await
            .expect("tab creation must succeed");
        let ephemeral = first.fork(Some(named.id)).await.expect("fork must succeed");
        first
            .repository
            .set_last_active_workspace_id(Some(ephemeral.id))
            .await
            .expect("stale last-active state must be writable");
        std::fs::remove_file(dir.path().join(format!("{}.json", ephemeral.id)))
            .expect("ephemeral workspace fixture must be removable");

        let restarted = WorkspaceService::new(JsonFileWorkspaceRepository::new(dir.path()));
        let restored = restarted.start(None).await.expect("restart must recover");

        assert_eq!(restored.id, named.id);
        assert!(
            restored
                .panes
                .iter()
                .flat_map(|pane| &pane.tabs)
                .any(|tab| { tab.location.uri == "file:///saved-tab" })
        );
        assert_eq!(restarted.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn start_falls_back_to_the_most_recently_updated_named_workspace_when_never_marked_last_active()
     {
        let service = service();
        // Neither `create` nor `apply_command` ever marks a workspace last-active on its own -
        // this reproduces a workspace that was saved but never explicitly opened via the
        // switcher, and never forked/resynced from either (which do mark last-active).
        let older = service.create(Some("Older".to_owned())).await.unwrap();
        // `save` stamps `updated_at` with `Utc::now()`; sleep past its resolution so the two
        // saves get strictly ordered timestamps rather than risking a same-millisecond tie.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let newer = service.create(Some("Newer".to_owned())).await.unwrap();
        assert!(newer.updated_at > older.updated_at);

        let started = service.start(None).await.expect("start must succeed");

        assert_eq!(
            started.id, newer.id,
            "must reopen the most recently updated saved workspace, not create another throwaway default"
        );
        assert_eq!(
            service.repository.last_active_workspace_id().await.unwrap(),
            Some(newer.id)
        );
    }

    #[tokio::test]
    async fn start_excludes_ephemeral_workspaces_from_the_most_recently_updated_fallback() {
        let service = service();
        let named = service.create(Some("Photos".to_owned())).await.unwrap();
        // A fork is more recently updated than `named`, but must never be selected - ephemeral
        // workspaces are private per-window sessions, never a startup destination. Forking also
        // marks `named` last-active itself now, so clear that back to force exercising the
        // most-recently-updated fallback specifically, rather than the last-active shortcut.
        service.fork(Some(named.id)).await.unwrap();
        service
            .repository
            .set_last_active_workspace_id(None)
            .await
            .unwrap();

        let started = service.start(None).await.expect("start must succeed");

        assert_eq!(started.id, named.id);
        assert!(!started.ephemeral);
    }

    #[tokio::test]
    async fn load_returns_not_found_for_an_unknown_id() {
        let service = service();
        let error = service.load(WorkspaceId::new()).await.unwrap_err();
        assert!(matches!(error, WorkspaceError::NotFound { .. }));
    }

    #[tokio::test]
    async fn start_recovers_from_a_corrupt_last_active_workspace_file_by_creating_a_default() {
        use super::super::persistent::JsonFileWorkspaceRepository;

        let dir = tempfile::TempDir::new().expect("temp dir");
        let repository = JsonFileWorkspaceRepository::new(dir.path());
        let service = WorkspaceService::new(repository);
        let original = service.create_default().await.expect("create must succeed");

        std::fs::write(
            dir.path().join(format!("{}.json", original.id)),
            b"{ not json",
        )
        .expect("overwrite with corrupt bytes");

        let workspace = service
            .start(None)
            .await
            .expect("start must recover from a corrupt file, not fail");
        assert_eq!(workspace.name, "Default");
        assert_ne!(workspace.id, original.id);
    }

    #[tokio::test]
    async fn delete_removes_a_workspace() {
        let service = service();
        let workspace = service.create_default().await.unwrap();

        service
            .delete(workspace.id, Some(workspace.revision))
            .await
            .expect("delete must succeed");
        assert!(matches!(
            service.load(workspace.id).await.unwrap_err(),
            WorkspaceError::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn create_persists_a_named_workspace_without_marking_it_last_active() {
        let service = service();

        let workspace = service
            .create(Some("Photos".to_owned()))
            .await
            .expect("create must succeed");

        assert_eq!(workspace.name, "Photos");
        assert!(
            service
                .repository
                .last_active_workspace_id()
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn create_with_no_name_uniquifies_the_default_name_instead_of_failing() {
        let service = service();
        let first = service
            .create(None)
            .await
            .expect("first create must succeed");
        let second = service
            .create(None)
            .await
            .expect("second create must succeed");
        let third = service
            .create(None)
            .await
            .expect("third create must succeed");

        assert_eq!(first.name, "Default");
        assert_eq!(second.name, "Default 2");
        assert_eq!(third.name, "Default 3");
    }

    #[tokio::test]
    async fn create_rejects_an_explicit_name_that_collides_with_an_existing_workspace() {
        let service = service();
        service
            .create(Some("Photos".to_owned()))
            .await
            .expect("first create must succeed");

        let error = service.create(Some("photos".to_owned())).await.unwrap_err();

        assert_eq!(
            error,
            WorkspaceError::DuplicateName {
                name: "photos".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn apply_command_rejects_renaming_to_a_name_already_used_by_another_workspace() {
        let service = service();
        let first = service.create_default().await.unwrap();
        let second = service.create(Some("Photos".to_owned())).await.unwrap();

        let error = service
            .apply_command(WorkspaceCommand::RenameWorkspace {
                workspace_id: second.id,
                name: "  default ".to_owned(),
                expected_revision: second.revision,
            })
            .await
            .unwrap_err();

        assert_eq!(
            error,
            WorkspaceError::DuplicateName {
                name: "  default ".to_owned()
            }
        );
        // The rejected rename must not have mutated the first workspace's name.
        assert_eq!(service.load(first.id).await.unwrap().name, "Default");
    }

    #[tokio::test]
    async fn apply_command_allows_renaming_a_workspace_to_its_own_current_name() {
        let service = service();
        let workspace = service.create_default().await.unwrap();

        let renamed = service
            .apply_command(WorkspaceCommand::RenameWorkspace {
                workspace_id: workspace.id,
                name: "Default".to_owned(),
                expected_revision: workspace.revision,
            })
            .await
            .expect("renaming to the same name must succeed");

        assert_eq!(renamed.name, "Default");
    }

    #[tokio::test]
    async fn open_selects_an_existing_workspace_as_last_active() {
        let service = service();
        let workspace = service.create(None).await.unwrap();

        let opened = service.open(workspace.id).await.expect("open must succeed");

        assert_eq!(opened.id, workspace.id);
        assert_eq!(
            service.repository.last_active_workspace_id().await.unwrap(),
            Some(workspace.id)
        );
    }

    #[tokio::test]
    async fn open_reports_not_found_rather_than_substituting_a_default() {
        let service = service();

        let error = service.open(WorkspaceId::new()).await.unwrap_err();

        assert!(matches!(error, WorkspaceError::NotFound { .. }));
    }

    #[tokio::test]
    async fn apply_command_renames_the_workspace_and_increments_the_revision() {
        let service = service();
        let workspace = service.create_default().await.unwrap();

        let renamed = service
            .apply_command(WorkspaceCommand::RenameWorkspace {
                workspace_id: workspace.id,
                name: "Photos".to_owned(),
                expected_revision: workspace.revision,
            })
            .await
            .expect("apply_command must succeed");

        assert_eq!(renamed.name, "Photos");
        assert_eq!(renamed.revision, workspace.revision + 1);
    }

    #[tokio::test]
    async fn apply_command_reports_a_stale_expected_revision_as_a_conflict() {
        let service = service();
        let workspace = service.create_default().await.unwrap();

        let error = service
            .apply_command(WorkspaceCommand::RenameWorkspace {
                workspace_id: workspace.id,
                name: "Photos".to_owned(),
                expected_revision: workspace.revision + 1,
            })
            .await
            .unwrap_err();

        assert_eq!(
            error,
            WorkspaceError::RevisionConflict {
                id: workspace.id,
                expected: Some(workspace.revision + 1),
                actual: workspace.revision,
            }
        );
    }

    #[tokio::test]
    async fn apply_command_closing_a_panes_last_tab_creates_a_replacement() {
        let service = service();
        let workspace = service.create_default().await.unwrap();
        let pane_id = workspace.active_pane_id;
        let tab_id = workspace
            .panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .unwrap()
            .tabs[0]
            .id;

        let mutated = service
            .apply_command(WorkspaceCommand::CloseTab {
                workspace_id: workspace.id,
                pane_id,
                tab_id,
                expected_revision: workspace.revision,
            })
            .await
            .expect("apply_command must succeed");

        let pane = mutated
            .panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .unwrap();
        assert_eq!(pane.tabs.len(), 1);
        assert_ne!(pane.tabs[0].id, tab_id);
    }

    #[tokio::test]
    async fn apply_command_rejects_an_unknown_pane() {
        let service = service();
        let workspace = service.create_default().await.unwrap();

        let error = service
            .apply_command(WorkspaceCommand::SetActivePane {
                workspace_id: workspace.id,
                pane_id: PaneId::new(),
                expected_revision: workspace.revision,
            })
            .await
            .unwrap_err();

        assert!(matches!(error, WorkspaceError::PaneNotFound { .. }));
    }

    #[tokio::test]
    async fn apply_command_notifies_the_configured_publisher() {
        #[derive(Default)]
        struct RecordingPublisher {
            events: Mutex<Vec<(WorkspaceId, BackendEventPayload)>>,
        }

        impl WorkspaceCommandPublisher for Arc<RecordingPublisher> {
            fn publish(&self, workspace_id: WorkspaceId, payload: BackendEventPayload) {
                self.events
                    .lock()
                    .expect("mutex must not be poisoned")
                    .push((workspace_id, payload));
            }
        }

        let publisher = Arc::new(RecordingPublisher::default());
        let service = WorkspaceService::new(InMemoryWorkspaceRepository::new())
            .with_secondary_location(PathBuf::from("/Users/erik/Downloads"))
            .with_publisher(Arc::clone(&publisher));
        let workspace = service.create_default().await.unwrap();

        service
            .apply_command(WorkspaceCommand::RenameWorkspace {
                workspace_id: workspace.id,
                name: "Photos".to_owned(),
                expected_revision: workspace.revision,
            })
            .await
            .expect("apply_command must succeed");

        let events = publisher.events.lock().expect("mutex must not be poisoned");
        let (workspace_id, payload) = events.last().expect("a rename event must be published");
        assert_eq!(*workspace_id, workspace.id);
        assert_eq!(
            *payload,
            BackendEventPayload::WorkspaceRenamed {
                revision: workspace.revision + 1,
                name: "Photos".to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn create_publishes_workspace_created() {
        #[derive(Default)]
        struct RecordingPublisher {
            events: Mutex<Vec<(WorkspaceId, BackendEventPayload)>>,
        }

        impl WorkspaceCommandPublisher for Arc<RecordingPublisher> {
            fn publish(&self, workspace_id: WorkspaceId, payload: BackendEventPayload) {
                self.events
                    .lock()
                    .expect("mutex must not be poisoned")
                    .push((workspace_id, payload));
            }
        }

        let publisher = Arc::new(RecordingPublisher::default());
        let service = WorkspaceService::new(InMemoryWorkspaceRepository::new())
            .with_publisher(Arc::clone(&publisher));

        let workspace = service.create(None).await.expect("create must succeed");

        let events = publisher.events.lock().expect("mutex must not be poisoned");
        assert_eq!(
            *events,
            vec![(
                workspace.id,
                BackendEventPayload::WorkspaceCreated {
                    revision: workspace.revision,
                }
            )]
        );
    }

    #[tokio::test]
    async fn create_default_publishes_created_then_opened() {
        #[derive(Default)]
        struct RecordingPublisher {
            events: Mutex<Vec<(WorkspaceId, BackendEventPayload)>>,
        }

        impl WorkspaceCommandPublisher for Arc<RecordingPublisher> {
            fn publish(&self, workspace_id: WorkspaceId, payload: BackendEventPayload) {
                self.events
                    .lock()
                    .expect("mutex must not be poisoned")
                    .push((workspace_id, payload));
            }
        }

        let publisher = Arc::new(RecordingPublisher::default());
        let service = WorkspaceService::new(InMemoryWorkspaceRepository::new())
            .with_publisher(Arc::clone(&publisher));

        let workspace = service
            .create_default()
            .await
            .expect("create_default must succeed");

        let events = publisher.events.lock().expect("mutex must not be poisoned");
        assert_eq!(
            *events,
            vec![
                (
                    workspace.id,
                    BackendEventPayload::WorkspaceCreated {
                        revision: workspace.revision,
                    }
                ),
                (
                    workspace.id,
                    BackendEventPayload::WorkspaceOpened {
                        revision: workspace.revision,
                    }
                ),
            ]
        );
    }

    #[tokio::test]
    async fn delete_publishes_workspace_deleted_with_the_pre_deletion_revision() {
        #[derive(Default)]
        struct RecordingPublisher {
            events: Mutex<Vec<(WorkspaceId, BackendEventPayload)>>,
        }

        impl WorkspaceCommandPublisher for Arc<RecordingPublisher> {
            fn publish(&self, workspace_id: WorkspaceId, payload: BackendEventPayload) {
                self.events
                    .lock()
                    .expect("mutex must not be poisoned")
                    .push((workspace_id, payload));
            }
        }

        let publisher = Arc::new(RecordingPublisher::default());
        let service = WorkspaceService::new(InMemoryWorkspaceRepository::new())
            .with_publisher(Arc::clone(&publisher));
        let workspace = service.create_default().await.unwrap();

        service
            .delete(workspace.id, Some(workspace.revision))
            .await
            .expect("delete must succeed");

        let events = publisher.events.lock().expect("mutex must not be poisoned");
        assert_eq!(
            events.last(),
            Some(&(
                workspace.id,
                BackendEventPayload::WorkspaceDeleted {
                    revision: workspace.revision,
                }
            ))
        );
    }

    #[tokio::test]
    async fn open_publishes_closed_for_the_previous_workspace_then_opened_for_the_new_one() {
        #[derive(Default)]
        struct RecordingPublisher {
            events: Mutex<Vec<(WorkspaceId, BackendEventPayload)>>,
        }

        impl WorkspaceCommandPublisher for Arc<RecordingPublisher> {
            fn publish(&self, workspace_id: WorkspaceId, payload: BackendEventPayload) {
                self.events
                    .lock()
                    .expect("mutex must not be poisoned")
                    .push((workspace_id, payload));
            }
        }

        let publisher = Arc::new(RecordingPublisher::default());
        let service = WorkspaceService::new(InMemoryWorkspaceRepository::new())
            .with_publisher(Arc::clone(&publisher));
        let first = service.create_default().await.unwrap();
        let second = service.create(None).await.unwrap();

        publisher
            .events
            .lock()
            .expect("mutex must not be poisoned")
            .clear();

        let opened = service.open(second.id).await.expect("open must succeed");

        let events = publisher.events.lock().expect("mutex must not be poisoned");
        assert_eq!(
            *events,
            vec![
                (
                    first.id,
                    BackendEventPayload::WorkspaceClosed {
                        revision: first.revision,
                    }
                ),
                (
                    second.id,
                    BackendEventPayload::WorkspaceOpened {
                        revision: opened.revision,
                    }
                ),
            ]
        );
    }

    #[tokio::test]
    async fn apply_command_add_tab_uses_the_given_location() {
        let service = service();
        let workspace = service.create_default().await.unwrap();
        let pane_id = workspace.active_pane_id;

        let mutated = service
            .apply_command(WorkspaceCommand::AddTab {
                workspace_id: workspace.id,
                pane_id,
                location: Location::new(ProviderId::new("file"), "file:///Users/erik/Music"),
                expected_revision: workspace.revision,
            })
            .await
            .expect("apply_command must succeed");

        let pane = mutated
            .panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .unwrap();
        assert_eq!(pane.tabs.len(), 2);
        assert_eq!(pane.tabs[1].location.uri, "file:///Users/erik/Music");
    }

    #[tokio::test]
    async fn fork_copies_the_sources_shape_and_marks_the_copy_ephemeral() {
        let service = service();
        let source = service.create(Some("Photos".to_owned())).await.unwrap();

        let forked = service.fork(Some(source.id)).await.unwrap();

        assert_ne!(forked.id, source.id);
        assert!(forked.ephemeral);
        assert_eq!(forked.forked_from, Some(source.id));
        assert_eq!(forked.panes, source.panes);
        assert_eq!(forked.layout, source.layout);
    }

    #[tokio::test]
    async fn fork_with_a_source_marks_that_source_last_active() {
        let service = service();
        let source = service.create(Some("Photos".to_owned())).await.unwrap();
        // Some other workspace was active before the fork - the fork must override it.
        service
            .repository
            .set_last_active_workspace_id(Some(WorkspaceId::new()))
            .await
            .unwrap();

        service.fork(Some(source.id)).await.unwrap();

        assert_eq!(
            service.repository.last_active_workspace_id().await.unwrap(),
            Some(source.id),
            "a window forked from a named workspace is using it, so a later cold start (e.g. \
             a fresh Dock window) must reopen that workspace instead of falling back to the \
             hardcoded default"
        );
    }

    #[tokio::test]
    async fn fork_with_no_source_seeds_the_hardcoded_default_and_leaves_last_active_untouched() {
        let service = service();
        let named = service.create(Some("Photos".to_owned())).await.unwrap();
        service
            .repository
            .set_last_active_workspace_id(Some(named.id))
            .await
            .unwrap();

        let forked = service.fork(None).await.unwrap();

        assert!(forked.ephemeral);
        assert_eq!(forked.forked_from, None);
        assert_eq!(
            service.repository.last_active_workspace_id().await.unwrap(),
            Some(named.id),
            "forking with no named source (nothing to mark as last-active) must not clobber \
             whatever named workspace was already last-active"
        );
    }

    #[tokio::test]
    async fn resync_writes_the_ephemerals_shape_back_into_its_source_and_marks_it_last_active() {
        let service = service();
        let source = service.create(Some("Photos".to_owned())).await.unwrap();
        let forked = service.fork(Some(source.id)).await.unwrap();
        let pane_id = forked.active_pane_id;
        let mutated = service
            .apply_command(WorkspaceCommand::AddTab {
                workspace_id: forked.id,
                pane_id,
                location: Location::new(ProviderId::new("file"), "file:///Users/erik/Music"),
                expected_revision: forked.revision,
            })
            .await
            .unwrap();
        service
            .repository
            .set_last_active_workspace_id(Some(WorkspaceId::new()))
            .await
            .unwrap();

        let target = service.resync(mutated.id, None).await.unwrap();

        assert_eq!(
            target.id, source.id,
            "resync must update the source, not the ephemeral"
        );
        let pane = target.panes.iter().find(|pane| pane.id == pane_id).unwrap();
        assert_eq!(pane.tabs.len(), 2, "the synced tab must land on the source");
        assert_eq!(
            service.repository.last_active_workspace_id().await.unwrap(),
            Some(source.id)
        );
    }

    #[tokio::test]
    async fn resync_of_a_from_scratch_default_creates_a_named_workspace_and_relinks_the_ephemeral()
    {
        let service = service();
        let forked = service.fork(None).await.unwrap();

        let target = service.resync(forked.id, None).await.unwrap();

        assert!(!target.ephemeral);
        assert_eq!(target.name, "Untitled");
        assert_eq!(
            service.repository.last_active_workspace_id().await.unwrap(),
            Some(target.id)
        );
        let relinked = service.load(forked.id).await.unwrap();
        assert_eq!(
            relinked.forked_from,
            Some(target.id),
            "a later resync from the same ephemeral window must update the new workspace \
             instead of creating another one"
        );
    }

    #[tokio::test]
    async fn resync_of_a_non_ephemeral_workspace_is_rejected() {
        let service = service();
        let named = service.create(Some("Photos".to_owned())).await.unwrap();

        let error = service.resync(named.id, None).await.unwrap_err();

        assert!(matches!(error, WorkspaceError::NotEphemeral { id } if id == named.id));
    }

    #[tokio::test]
    async fn resync_with_an_explicit_target_replaces_that_workspace_and_keeps_its_own_name() {
        let service = service();
        let source = service.create(Some("Photos".to_owned())).await.unwrap();
        let other = service.create(Some("Documents".to_owned())).await.unwrap();
        let forked = service.fork(Some(source.id)).await.unwrap();
        let pane_id = forked.active_pane_id;
        let mutated = service
            .apply_command(WorkspaceCommand::AddTab {
                workspace_id: forked.id,
                pane_id,
                location: Location::new(ProviderId::new("file"), "file:///Users/erik/Music"),
                expected_revision: forked.revision,
            })
            .await
            .unwrap();

        // Update "Documents" instead of the window's own source, "Photos" - the switcher's
        // per-row "Update" button lets any saved workspace be replaced this way.
        let target = service.resync(mutated.id, Some(other.id)).await.unwrap();

        assert_eq!(target.id, other.id);
        assert_eq!(target.name, "Documents", "the target keeps its own name");
        let pane = target.panes.iter().find(|pane| pane.id == pane_id).unwrap();
        assert_eq!(pane.tabs.len(), 2);
        let untouched_source = service.load(source.id).await.unwrap();
        assert_eq!(
            untouched_source.panes, forked.panes,
            "the workspace the window was originally forked from must be untouched"
        );
        let relinked = service.load(mutated.id).await.unwrap();
        assert_eq!(
            relinked.forked_from,
            Some(other.id),
            "the ephemeral window must now be linked to the workspace it was just synced into"
        );
    }

    #[tokio::test]
    async fn resync_with_an_explicit_target_allows_a_non_ephemeral_source() {
        // The main/dock window's own workspace is never ephemeral, but the switcher's per-row
        // "Update" button must still work from it - it just copies that window's live shape
        // into whichever row was clicked, no fork/ephemeral machinery involved.
        let service = service();
        let current = service.create(Some("Downloads".to_owned())).await.unwrap();
        let pane_id = current.active_pane_id;
        let mutated = service
            .apply_command(WorkspaceCommand::AddTab {
                workspace_id: current.id,
                pane_id,
                location: Location::new(ProviderId::new("file"), "file:///Users/erik/Music"),
                expected_revision: current.revision,
            })
            .await
            .unwrap();
        let other = service.create(Some("Documents".to_owned())).await.unwrap();

        let target = service.resync(mutated.id, Some(other.id)).await.unwrap();

        assert_eq!(target.id, other.id);
        let pane = target.panes.iter().find(|pane| pane.id == pane_id).unwrap();
        assert_eq!(pane.tabs.len(), 2);
    }

    #[tokio::test]
    async fn resync_with_an_explicit_ephemeral_target_is_rejected() {
        let service = service();
        let source = service.create(Some("Photos".to_owned())).await.unwrap();
        let forked = service.fork(Some(source.id)).await.unwrap();
        let other_ephemeral = service.fork(None).await.unwrap();

        let error = service
            .resync(forked.id, Some(other_ephemeral.id))
            .await
            .unwrap_err();

        assert!(
            matches!(error, WorkspaceError::TargetIsEphemeral { id } if id == other_ephemeral.id)
        );
    }
}
