//! Stable machine-readable codes attached to operation messages.
//!
//! Clients map these to localised text and fall back to the English message when a code is
//! unknown. Per-entry failures from a provider reuse [`fm_vfs::VfsError::code`]; the codes here
//! cover everything else. Every value must have a translation in each frontend locale; a frontend
//! test reads this file to enforce that, so declare each code as a plain `pub const` string.

/// Codes of [`crate::OperationEntryError`] that do not come from a provider error.
pub mod entry_error {
    /// A bulk removal could not process one entry (`detail` carries the reason).
    pub const ENTRY_FAILED: &str = "entryFailed";
    /// A platform (trash) call failed (`detail`).
    pub const PLATFORM_FAILED: &str = "platformFailed";
    /// The planned source disappeared before it was processed.
    pub const SOURCE_MISSING: &str = "sourceMissing";
    /// The user skipped the free-space check (`required`, `available` bytes).
    pub const DISK_SPACE_CHECK_SKIPPED: &str = "diskSpaceCheckSkipped";
    /// Changing attributes was refused (`name`).
    pub const ATTRIBUTE_NOT_PERMITTED: &str = "attributeNotPermitted";
    /// The entry kind does not support attribute changes (`name`).
    pub const ATTRIBUTE_UNSUPPORTED: &str = "attributeUnsupported";
    /// The entry vanished before its attributes changed (`name`).
    pub const ATTRIBUTE_ENTRY_MISSING: &str = "attributeEntryMissing";
    /// Changing attributes failed for another reason (`name`, `detail`).
    pub const ATTRIBUTE_FAILED: &str = "attributeFailed";
    /// Metadata editing is unsupported for the container format.
    pub const IMAGE_FORMAT_UNSUPPORTED: &str = "imageFormatUnsupported";
    /// The format cannot store a requested metadata field (`field`).
    pub const IMAGE_FIELD_UNSUPPORTED: &str = "imageFieldUnsupported";
    /// A geo conversion wrote the file but dropped content (`name`, `notes`).
    pub const GEO_WRITTEN_WITH_LOSS: &str = "geoWrittenWithLoss";
    /// A split part changed while files were being joined (`name`).
    pub const PART_CHANGED_WHILE_JOINING: &str = "partChangedWhileJoining";
    /// A split part does not match the manifest (`name`).
    pub const PART_DAMAGED: &str = "partDamaged";
    /// The joined file does not match the manifest.
    pub const JOINED_FILE_MISMATCH: &str = "joinedFileMismatch";
    /// Summary entry replacing failures dropped from saved history (`count`).
    pub const ERRORS_OMITTED: &str = "errorsOmitted";
}

/// Codes of an operation's terminal result summary.
pub mod result {
    /// Finished without warnings (`items`).
    pub const COMPLETED: &str = "completed";
    /// Finished with per-entry warnings (`count`).
    pub const COMPLETED_WITH_WARNINGS: &str = "completedWithWarnings";
    /// Cancelled by the user (`items`).
    pub const CANCELLED: &str = "cancelled";
    /// Failed.
    pub const FAILED: &str = "failed";
    /// Interrupted by a backend restart (`items`).
    pub const INTERRUPTED: &str = "interrupted";
}

/// Codes explaining why an operation cannot be undone.
pub mod undo {
    /// The operation was already undone.
    pub const ALREADY_UNDONE: &str = "alreadyUndone";
    /// An undo job is already running.
    pub const UNDO_IN_PROGRESS: &str = "undoInProgress";
    /// No safe undo plan exists.
    pub const UNDO_UNSAFE: &str = "undoUnsafe";
}
