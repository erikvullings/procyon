//! Workspace persistence, validation and lifecycle (spec §5.3, tasks 0079,
//! 0080, 0081).
//!
//! [`WorkspaceService`] is the entry point: it wraps a [`WorkspaceRepository`]
//! (an in-memory implementation for tests, a JSON-file-backed one for real
//! persistence) and owns the startup lifecycle, default-workspace creation,
//! the create/load/list/delete surface and semantic mutation commands
//! (`WorkspaceCommand`/`apply_command`, spec §5.3.9). Every lifecycle
//! transition and applied command is turned into a focused `workspace.*`
//! event (`events`, spec §5.3.11) and handed to the injectable
//! [`WorkspaceCommandPublisher`] seam; wiring that seam onto a real shared
//! event bus (task 0031) is left to whichever host constructs the service.

mod command;
mod default_workspace;
mod error;
mod events;
mod memory;
mod migration;
mod persistent;
mod publisher;
mod repository;
mod service;

pub use default_workspace::{default_workspace, resolve_home_directory};
pub use error::WorkspaceError;
pub use memory::InMemoryWorkspaceRepository;
pub use persistent::{JsonFileWorkspaceRepository, NoopWorkspaceNotifier, WorkspaceNotifier};
pub use publisher::{NoopWorkspaceCommandPublisher, WorkspaceCommandPublisher};
pub use repository::{LastActiveWorkspaceStore, WorkspaceRepository, WorkspaceSummary};
pub use service::WorkspaceService;
