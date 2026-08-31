//! Directory comparison and basic synchronization (spec §16 milestone 5,
//! §37, task 0075).
//!
//! Mirrors `fm-search`'s shape: an [`engine::ComparisonEngine`] runs a
//! cancellable traversal in the backend and streams batches into a
//! [`store::ComparisonResultsStore`] that a REST/Tauri layer pages through.
//! [`sync`] turns a materialized comparison into a reviewable, editable plan;
//! applying that plan is left to the caller, which starts ordinary `Copy`/
//! `Trash` operations through the existing operation engine (spec §35: no
//! new mutation path is introduced here).

mod engine;
mod model;
mod path;
mod store;
mod sync;

pub use engine::{ComparisonEngine, ComparisonError, ComparisonOptions};
pub use model::{
    ComparisonCriteria, ComparisonEntry, ComparisonEntrySide, ComparisonStatus, classify,
};
pub use path::{relative_join, relative_parent, resolve_relative};
pub use store::ComparisonResultsStore;
pub use sync::{SyncAction, SyncMode, SyncPlanItem, generate_sync_plan};
