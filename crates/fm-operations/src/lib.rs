//! Cancellable, provider-neutral file-operation jobs.
//!
//! The engine owns lifecycle, planning, scheduling, safety checks and event
//! publication. Concrete filesystem mutations are added independently by
//! tasks 0037 through 0044.

mod model;
mod naming;
mod pause;
mod progress;
mod safety;
mod scheduler;

pub use model::{
    ConflictEntry, ConflictPolicy, ConflictResolution, EntryFingerprint, Operation,
    OperationConflict, OperationEntryError, OperationKind, OperationProgress, OperationState,
    OperationUndo, TransitionError, UndoAction, UndoPlan,
};
pub use naming::duplicate_name;
pub use pause::PauseToken;
pub use progress::ProgressPublisher;
pub use safety::{CycleDetector, EntryType, SafetyError, validate_paths, validate_replacement};
pub use scheduler::{
    ExecutionError, ExecutionOutcome, OperationExecutor, OperationPlan, OperationProgressReporter,
    OperationSnapshotObserver, PlanItem, Scheduler, SchedulerError,
};
