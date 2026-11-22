//! Remembers which workspace each open window is showing, so a restart reopens every window on
//! the workspace it had - not on whichever workspace was last active (or on its stale ephemeral
//! fork). Persisted to `open-windows.json` beside the workspace directory on every change; the
//! file deliberately keeps its entries while the app quits (windows closing as part of a quit
//! must not forget their workspace), and only a genuine single-window close removes one.

use std::path::PathBuf;
use std::sync::Mutex;

use uuid::Uuid;

const FILE_NAME: &str = "open-windows.json";

/// Ordered `(window label, workspace id)` pairs, oldest window first.
type Entries = Vec<(String, Uuid)>;

pub(crate) struct WindowSession {
    path: Option<PathBuf>,
    entries: Mutex<Entries>,
}

impl Default for WindowSession {
    fn default() -> Self {
        let directory = fm_application::workspace::JsonFileWorkspaceRepository::default_directory();
        Self::at(directory.parent().map(|parent| parent.join(FILE_NAME)))
    }
}

impl WindowSession {
    pub(crate) fn at(path: Option<PathBuf>) -> Self {
        Self {
            path,
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Records that `label`'s window now shows `workspace_id` (insert or replace, keeping the
    /// window's position in the order).
    pub(crate) fn record(&self, label: &str, workspace_id: Uuid) {
        let mut entries = self.lock();
        match entries.iter_mut().find(|(existing, _)| existing == label) {
            Some(entry) => entry.1 = workspace_id,
            None => entries.push((label.to_owned(), workspace_id)),
        }
        self.persist(&entries);
    }

    /// Forgets a window that the user closed (not one closed by the app quitting).
    pub(crate) fn forget(&self, label: &str) {
        let mut entries = self.lock();
        let before = entries.len();
        entries.retain(|(existing, _)| existing != label);
        if entries.len() != before {
            self.persist(&entries);
        }
    }

    /// The workspace ids the previous session had open, in window order. Reads the file and
    /// clears the in-memory state: the restored windows have fresh labels and re-record
    /// themselves as they boot.
    pub(crate) fn take_saved(&self) -> Vec<Uuid> {
        self.lock().clear();
        let Some(path) = &self.path else {
            return Vec::new();
        };
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Entries>(&bytes).ok())
            .map(|entries| entries.into_iter().map(|(_, id)| id).collect())
            .unwrap_or_default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Entries> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn persist(&self, entries: &Entries) {
        let Some(path) = &self.path else { return };
        let result = serde_json::to_vec(entries)
            .map_err(std::io::Error::other)
            .and_then(|bytes| std::fs::write(path, bytes));
        if let Err(error) = result {
            tracing::warn!(%error, "could not persist the open-window workspaces");
        }
    }
}

/// Windows whose workspace is worth restoring: the default `"main"` window and
/// `workspace-<id>` windows. Viewer and plugin-panel windows never carry a workspace.
pub(crate) fn is_workspace_window(label: &str) -> bool {
    label == "main" || label.starts_with("workspace-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_each_window_workspace_in_order_across_sessions() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join(FILE_NAME);
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());

        let first = WindowSession::at(Some(path.clone()));
        first.record("main", a);
        first.record("workspace-x_1", b);

        let second = WindowSession::at(Some(path));
        assert_eq!(second.take_saved(), vec![a, b]);
    }

    #[test]
    fn switching_replaces_and_closing_forgets() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join(FILE_NAME);
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());

        let session = WindowSession::at(Some(path.clone()));
        session.record("main", a);
        session.record("w2", b);
        session.record("main", c);
        session.forget("w2");

        assert_eq!(WindowSession::at(Some(path)).take_saved(), vec![c]);
    }

    #[test]
    fn only_workspace_windows_are_tracked() {
        assert!(is_workspace_window("main"));
        assert!(is_workspace_window("workspace-abc_1"));
        assert!(!is_workspace_window("viewer-abc"));
    }
}
