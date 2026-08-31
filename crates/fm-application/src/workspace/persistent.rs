//! A versioned-JSON-file [`WorkspaceRepository`] (spec §5.3.8).
//!
//! Each workspace is stored as `<workspace_id>.json` inside a single
//! directory, and the last-active workspace id is stored alongside it as
//! `last-active.json`. Saves are atomic (write to a sibling temp file, then
//! rename into place); a workspace file that fails to parse is backed up
//! rather than discarded or silently repaired, and
//! [`WorkspaceNotifier::corrupt_workspace_recovered`] is called so a host can
//! surface it later, rather than the corruption crashing startup.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use fm_domain::{Workspace, WorkspaceId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::WorkspaceError;
use super::migration::migrate_workspace_json;
use super::repository::{LastActiveWorkspaceStore, WorkspaceRepository, WorkspaceSummary};

const LAST_ACTIVE_FILE_NAME: &str = "last-active.json";

/// Notified about workspace-storage recovery events that do not yet have a
/// full event-bus integration (task 0081 wires the real event bus).
pub trait WorkspaceNotifier: Send + Sync {
    /// Called after a workspace file failed to parse and was backed up.
    fn corrupt_workspace_recovered(&self, workspace_file_stem: &str, backup_path: &Path);
}

/// A [`WorkspaceNotifier`] that discards every notification, for hosts and
/// tests that do not need one.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopWorkspaceNotifier;

impl WorkspaceNotifier for NoopWorkspaceNotifier {
    fn corrupt_workspace_recovered(&self, _workspace_file_stem: &str, _backup_path: &Path) {}
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LastActiveFile {
    workspace_id: Option<WorkspaceId>,
}

/// Persists workspaces as versioned JSON files under a single directory
/// (typically the platform config directory), with atomic writes and
/// corrupt-file recovery.
pub struct JsonFileWorkspaceRepository {
    directory: PathBuf,
    notifier: Arc<dyn WorkspaceNotifier>,
}

impl JsonFileWorkspaceRepository {
    /// The default storage location: `workspaces/` under the platform config
    /// directory (via the `dirs` crate, spec §5.3.8's "platform config
    /// directory"), falling back to `.fm-config/workspaces` in the current
    /// directory if the platform cannot report one (for example a container
    /// with no `$HOME`/`$XDG_CONFIG_HOME`).
    #[must_use]
    pub fn default_directory() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".fm-config"))
            .join("fm")
            .join("workspaces")
    }

    /// Creates a repository rooted at `directory`, discarding corruption
    /// notifications.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self::with_notifier(directory, Arc::new(NoopWorkspaceNotifier))
    }

    /// Creates a repository rooted at `directory`, invoking `notifier` when a
    /// corrupt workspace file is recovered.
    pub fn with_notifier(
        directory: impl Into<PathBuf>,
        notifier: Arc<dyn WorkspaceNotifier>,
    ) -> Self {
        Self {
            directory: directory.into(),
            notifier,
        }
    }

    fn workspace_path(&self, id: WorkspaceId) -> PathBuf {
        self.directory.join(format!("{id}.json"))
    }

    fn last_active_path(&self) -> PathBuf {
        self.directory.join(LAST_ACTIVE_FILE_NAME)
    }

    /// Reads and migrates a single workspace file, or `Ok(None)` if it does
    /// not exist. A file that fails to parse is backed up and reported via
    /// `self.notifier` before returning [`WorkspaceError::Corrupt`].
    async fn read_workspace_file(&self, path: &Path) -> Result<Option<Workspace>, WorkspaceError> {
        let bytes = match tokio::fs::read(path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(WorkspaceError::Io(error.to_string())),
        };

        match parse_and_migrate(&bytes) {
            Ok(workspace) => Ok(Some(workspace)),
            Err(_) => {
                let stem = path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("workspace")
                    .to_owned();
                let backup_path = self.backup_corrupt_file(path).await?;
                self.notifier
                    .corrupt_workspace_recovered(&stem, &backup_path);
                Err(WorkspaceError::Corrupt {
                    id: stem,
                    backup_path,
                })
            }
        }
    }

    async fn backup_corrupt_file(&self, path: &Path) -> Result<PathBuf, WorkspaceError> {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace.json");
        let backup_path = self
            .directory
            .join(format!("{file_name}.corrupt-{}", Utc::now().timestamp()));
        tokio::fs::rename(path, &backup_path)
            .await
            .map_err(|error| WorkspaceError::Io(error.to_string()))?;
        Ok(backup_path)
    }
}

fn parse_and_migrate(bytes: &[u8]) -> Result<Workspace, WorkspaceError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| WorkspaceError::Serialization(error.to_string()))?;
    let migrated = migrate_workspace_json(value)?;
    serde_json::from_value(migrated)
        .map_err(|error| WorkspaceError::Serialization(error.to_string()))
}

/// Writes `contents` to `path` atomically: a sibling temp file is written
/// first and then renamed into place, so a crash mid-write can never leave a
/// half-written workspace file behind.
async fn write_atomically(path: &Path, contents: &[u8]) -> Result<(), WorkspaceError> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    tokio::fs::create_dir_all(directory)
        .await
        .map_err(|error| WorkspaceError::Io(error.to_string()))?;

    let tmp_path = directory.join(format!(".tmp-{}", Uuid::new_v4()));
    tokio::fs::write(&tmp_path, contents)
        .await
        .map_err(|error| WorkspaceError::Io(error.to_string()))?;
    tokio::fs::rename(&tmp_path, path)
        .await
        .map_err(|error| WorkspaceError::Io(error.to_string()))?;
    Ok(())
}

#[async_trait]
impl WorkspaceRepository for JsonFileWorkspaceRepository {
    async fn list(&self) -> Result<Vec<WorkspaceSummary>, WorkspaceError> {
        let mut entries = match tokio::fs::read_dir(&self.directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(WorkspaceError::Io(error.to_string())),
        };

        let mut summaries = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| WorkspaceError::Io(error.to_string()))?
        {
            let path = entry.path();
            let is_last_active_file =
                path.file_name().and_then(|name| name.to_str()) == Some(LAST_ACTIVE_FILE_NAME);
            let is_workspace_file = path.extension().and_then(|ext| ext.to_str()) == Some("json");
            if is_last_active_file || !is_workspace_file {
                continue;
            }

            match self.read_workspace_file(&path).await {
                Ok(Some(workspace)) => summaries.push(WorkspaceSummary::from(&workspace)),
                Ok(None) => {}
                // Already backed up and notified inside `read_workspace_file`;
                // exclude it from the list rather than failing the whole call.
                Err(WorkspaceError::Corrupt { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(summaries)
    }

    async fn load(&self, id: WorkspaceId) -> Result<Option<Workspace>, WorkspaceError> {
        self.read_workspace_file(&self.workspace_path(id)).await
    }

    async fn save(
        &self,
        workspace: &Workspace,
        expected_revision: Option<u64>,
    ) -> Result<Workspace, WorkspaceError> {
        let path = self.workspace_path(workspace.id);
        // A corrupt existing file has already been backed up and reported;
        // treat it as absent so this save can safely re-create a valid one.
        let existing = match self.read_workspace_file(&path).await {
            Ok(existing) => existing,
            Err(WorkspaceError::Corrupt { .. }) => None,
            Err(error) => return Err(error),
        };

        let mut persisted = workspace.clone();
        match &existing {
            Some(existing) => {
                if let Some(expected) = expected_revision
                    && expected != existing.revision
                {
                    return Err(WorkspaceError::RevisionConflict {
                        id: workspace.id,
                        expected: Some(expected),
                        actual: existing.revision,
                    });
                }
                persisted.created_at = existing.created_at;
                persisted.revision = existing.revision + 1;
            }
            None => {
                if let Some(expected) = expected_revision {
                    return Err(WorkspaceError::RevisionConflict {
                        id: workspace.id,
                        expected: Some(expected),
                        actual: 0,
                    });
                }
                persisted.revision = 1;
            }
        }
        persisted.updated_at = Utc::now();

        let bytes = serde_json::to_vec_pretty(&persisted)
            .map_err(|error| WorkspaceError::Serialization(error.to_string()))?;
        write_atomically(&path, &bytes).await?;
        Ok(persisted)
    }

    async fn delete(
        &self,
        id: WorkspaceId,
        expected_revision: Option<u64>,
    ) -> Result<(), WorkspaceError> {
        let path = self.workspace_path(id);
        let existing = self
            .read_workspace_file(&path)
            .await?
            .ok_or(WorkspaceError::NotFound { id })?;

        if let Some(expected) = expected_revision
            && expected != existing.revision
        {
            return Err(WorkspaceError::RevisionConflict {
                id,
                expected: Some(expected),
                actual: existing.revision,
            });
        }

        tokio::fs::remove_file(&path)
            .await
            .map_err(|error| WorkspaceError::Io(error.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl LastActiveWorkspaceStore for JsonFileWorkspaceRepository {
    async fn last_active_workspace_id(&self) -> Result<Option<WorkspaceId>, WorkspaceError> {
        let bytes = match tokio::fs::read(self.last_active_path()).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(WorkspaceError::Io(error.to_string())),
        };
        // Malformed last-active metadata is not core workspace data: fall
        // back to "none recorded" instead of failing startup over it.
        let parsed: LastActiveFile = serde_json::from_slice(&bytes).unwrap_or_default();
        Ok(parsed.workspace_id)
    }

    async fn set_last_active_workspace_id(
        &self,
        id: Option<WorkspaceId>,
    ) -> Result<(), WorkspaceError> {
        let bytes = serde_json::to_vec_pretty(&LastActiveFile { workspace_id: id })
            .map_err(|error| WorkspaceError::Serialization(error.to_string()))?;
        write_atomically(&self.last_active_path(), &bytes).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use fm_domain::{
        CURRENT_WORKSPACE_SCHEMA_VERSION, ColumnConfiguration, DirectoryViewConfiguration,
        DirectoryViewMode, IconSize, Location, NavigationHistory, OperationCentrePreferences,
        PaneId, PaneState, ProviderId, SortDescriptor, SortDirection, TabId, TabState, Workspace,
        WorkspaceLayout,
    };
    use tempfile::TempDir;

    use super::*;

    fn sample_workspace() -> Workspace {
        let pane_id = PaneId::new();
        let tab_id = TabId::new();
        let view = DirectoryViewConfiguration {
            sort: vec![SortDescriptor {
                column_id: "core.name".to_owned(),
                direction: SortDirection::Ascending,
            }],
            columns: vec![ColumnConfiguration {
                column_id: "core.name".to_owned(),
                width: 300,
                visible: true,
            }],
            show_hidden: false,
            folders_first: true,
            quick_filter: None,
            view_mode: DirectoryViewMode::Table,
            icon_size: IconSize::Medium,
        };
        let now = Utc::now();
        Workspace {
            schema_version: CURRENT_WORKSPACE_SCHEMA_VERSION,
            id: WorkspaceId::new(),
            name: "Default".to_owned(),
            layout: WorkspaceLayout::Pane { pane_id },
            panes: vec![PaneState {
                id: pane_id,
                title: None,
                tabs: vec![TabState {
                    id: tab_id,
                    location: Location::new(ProviderId::new("local"), "file:///home"),
                    title_override: None,
                    history: NavigationHistory {
                        back: vec![],
                        forward: vec![],
                    },
                    view: view.clone(),
                    pinned: false,
                    transient: false,
                }],
                active_tab_id: tab_id,
                default_view: view,
            }],
            active_pane_id: pane_id,
            operation_centre: OperationCentrePreferences {
                visible: false,
                height: 0,
            },
            created_at: now,
            updated_at: now,
            revision: 1,
            ephemeral: false,
            forked_from: None,
        }
    }

    #[test]
    fn default_directory_lives_under_the_platform_config_directory() {
        let expected_suffix = Path::new("fm").join("workspaces");
        assert!(JsonFileWorkspaceRepository::default_directory().ends_with(&expected_suffix));
    }

    #[tokio::test]
    async fn save_writes_a_json_file_that_load_can_read_back() {
        let dir = TempDir::new().expect("temp dir");
        let repository = JsonFileWorkspaceRepository::new(dir.path());
        let workspace = sample_workspace();

        let saved = repository
            .save(&workspace, None)
            .await
            .expect("save must succeed");
        assert!(dir.path().join(format!("{}.json", workspace.id)).exists());

        let loaded = repository
            .load(workspace.id)
            .await
            .expect("load must succeed")
            .expect("workspace must exist");
        assert_eq!(loaded, saved);
    }

    #[tokio::test]
    async fn save_is_atomic_and_leaves_no_temp_files_behind() {
        let dir = TempDir::new().expect("temp dir");
        let repository = JsonFileWorkspaceRepository::new(dir.path());
        repository
            .save(&sample_workspace(), None)
            .await
            .expect("save must succeed");

        let leftover_temp_files = std::fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with(".tmp-"));
        assert!(
            !leftover_temp_files,
            "no .tmp- files should remain after a save"
        );
    }

    #[tokio::test]
    async fn a_stale_expected_revision_is_rejected() {
        let dir = TempDir::new().expect("temp dir");
        let repository = JsonFileWorkspaceRepository::new(dir.path());
        let workspace = sample_workspace();
        let saved = repository
            .save(&workspace, None)
            .await
            .expect("save must succeed");

        let error = repository
            .save(&saved, Some(saved.revision + 1))
            .await
            .expect_err("stale expected_revision must be rejected");
        assert_eq!(
            error,
            WorkspaceError::RevisionConflict {
                id: workspace.id,
                expected: Some(saved.revision + 1),
                actual: saved.revision,
            }
        );
    }

    #[tokio::test]
    async fn list_returns_a_summary_for_every_saved_workspace() {
        let dir = TempDir::new().expect("temp dir");
        let repository = JsonFileWorkspaceRepository::new(dir.path());
        let first = repository.save(&sample_workspace(), None).await.unwrap();
        let second = repository.save(&sample_workspace(), None).await.unwrap();

        let mut summaries = repository.list().await.expect("list must succeed");
        summaries.sort_by_key(|summary| summary.id.to_string());
        let mut expected_ids = [first.id, second.id];
        expected_ids.sort_by_key(WorkspaceId::to_string);

        assert_eq!(
            summaries
                .iter()
                .map(|summary| summary.id)
                .collect::<Vec<_>>(),
            expected_ids
        );
    }

    #[tokio::test]
    async fn corrupt_workspace_file_is_backed_up_and_reported_instead_of_crashing() {
        #[derive(Default)]
        struct RecordingNotifier {
            calls: Mutex<Vec<(String, PathBuf)>>,
        }

        impl WorkspaceNotifier for RecordingNotifier {
            fn corrupt_workspace_recovered(&self, stem: &str, backup_path: &Path) {
                self.calls
                    .lock()
                    .unwrap()
                    .push((stem.to_owned(), backup_path.to_owned()));
            }
        }

        let dir = TempDir::new().expect("temp dir");
        let id = WorkspaceId::new();
        let path = dir.path().join(format!("{id}.json"));
        std::fs::write(&path, b"not valid json").expect("write corrupt file");

        let notifier = Arc::new(RecordingNotifier::default());
        let repository = JsonFileWorkspaceRepository::with_notifier(dir.path(), notifier.clone());

        let error = repository
            .load(id)
            .await
            .expect_err("a corrupt file must not be silently treated as absent");
        let backup_path = match error {
            WorkspaceError::Corrupt { backup_path, .. } => backup_path,
            other => panic!("expected Corrupt, got {other:?}"),
        };

        assert!(
            !path.exists(),
            "the corrupt file must be moved out of the way"
        );
        assert!(backup_path.exists(), "the backup file must exist");
        assert_eq!(notifier.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn last_active_workspace_id_round_trips_through_disk() {
        let dir = TempDir::new().expect("temp dir");
        let repository = JsonFileWorkspaceRepository::new(dir.path());
        assert_eq!(repository.last_active_workspace_id().await.unwrap(), None);

        let id = WorkspaceId::new();
        repository
            .set_last_active_workspace_id(Some(id))
            .await
            .unwrap();
        assert_eq!(
            repository.last_active_workspace_id().await.unwrap(),
            Some(id)
        );
    }

    #[tokio::test]
    async fn a_v0_workspace_file_on_disk_is_migrated_transparently_on_load() {
        let dir = TempDir::new().expect("temp dir");
        let id = "985d4d6e-c37b-4135-90a0-ce0afe165fd9";
        let v0_json = serde_json::json!({
            "id": id,
            "name": "Development",
            "layout": { "type": "pane", "paneId": "11e67e3e-813c-44c5-9426-53be347ad5da" },
            "active_pane_id": "11e67e3e-813c-44c5-9426-53be347ad5da",
            "panes": [{
                "id": "11e67e3e-813c-44c5-9426-53be347ad5da",
                "active_tab_id": "97512c58-9cf8-4f17-a931-94f0be87a1da",
                "tabs": [{
                    "id": "97512c58-9cf8-4f17-a931-94f0be87a1da",
                    "location": { "provider_id": "local", "uri": "file:///Users/erik/dev" },
                    "history": { "back": [], "forward": [] },
                    "view": {
                        "sort": [],
                        "columns": [],
                        "show_hidden": false,
                        "folders_first": false,
                        "quick_filter": null
                    }
                }]
            }]
        });
        std::fs::write(
            dir.path().join(format!("{id}.json")),
            serde_json::to_vec_pretty(&v0_json).unwrap(),
        )
        .expect("write v0 fixture");

        let repository = JsonFileWorkspaceRepository::new(dir.path());
        let workspace_id: WorkspaceId = id.parse().unwrap();
        let loaded = repository
            .load(workspace_id)
            .await
            .expect("migrated v0 file must load successfully")
            .expect("workspace must exist");

        assert_eq!(
            loaded.schema_version,
            fm_domain::CURRENT_WORKSPACE_SCHEMA_VERSION
        );
        assert!(loaded.validate().is_ok());
    }
}
