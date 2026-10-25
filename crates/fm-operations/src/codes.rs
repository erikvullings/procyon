//! Stable machine-readable codes attached to operation messages.
//!
//! Clients map these to localised text and fall back to the English message when a code is
//! unknown. Per-entry failures from a provider reuse [`fm_vfs::VfsError::code`]; the codes here
//! cover everything else. Every value must have a translation in each frontend locale; a frontend
//! test reads this file to enforce that, so declare each code as a plain `pub const` string.

/// Codes of [`crate::OperationEntryError`] that do not come from a provider error.
pub mod entry_error {
    /// A bulk removal could not process one entry (`path`, `detail` carries the platform text).
    pub const ENTRY_FAILED: &str = "entryFailed";
    /// A bulk removal left a read-only entry in place (`path`).
    pub const ENTRY_READ_ONLY: &str = "entryReadOnly";
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

/// Codes explaining why an operation cannot be undone (also the `reason` of the matching
/// `invalidRequest` rejection when an undo is requested anyway).
pub mod undo {
    /// The operation was already undone.
    pub const ALREADY_UNDONE: &str = "alreadyUndone";
    /// An undo job is already running.
    pub const UNDO_IN_PROGRESS: &str = "undoInProgress";
    /// No safe undo plan exists.
    pub const UNDO_UNSAFE: &str = "undoUnsafe";
    /// Undo only becomes available once the operation has completed.
    pub const NOT_YET_AVAILABLE: &str = "notYetAvailable";
    /// The operation did not retain enough evidence to be reversed safely.
    pub const NO_EVIDENCE: &str = "noEvidence";
    /// A previous undo was interrupted, so its remaining effects cannot be reversed safely.
    pub const UNDO_INTERRUPTED: &str = "undoInterrupted";
    /// A previous undo completed only partially.
    pub const UNDO_PARTIAL: &str = "undoPartial";
    /// Existing entries were replaced or merged, which cannot be brought back.
    pub const REPLACED_EXISTING: &str = "replacedExisting";
    /// Undo needs the backup copy, which was not kept.
    pub const BACKUP_REQUIRED: &str = "backupRequired";
    /// The operation changed or created nothing, so there is nothing to undo.
    pub const NOTHING_TO_UNDO: &str = "nothingToUndo";
    /// Undo operations cannot themselves be undone.
    pub const UNDO_OF_UNDO: &str = "undoOfUndo";
    /// The platform did not provide a restorable trash location.
    pub const TRASH_NOT_RESTORABLE: &str = "trashNotRestorable";
    /// A move did not retain its original source fingerprint.
    pub const MOVE_EVIDENCE_MISSING: &str = "moveEvidenceMissing";
    /// A name collision changed the moved entry name, so cross-provider undo is unavailable.
    pub const COLLISION_RENAMED: &str = "collisionRenamed";
    /// The history entry does not exist.
    pub const NOT_FOUND: &str = "historyEntryNotFound";
    /// The inverse plan has no actions.
    pub const NO_EFFECTS: &str = "noEffects";
}

/// Codes of operation-level failures (`operation.failed` events). Provider failures reuse
/// [`fm_vfs::VfsError::code`]; failures without a dedicated code use [`failure::OPERATION_FAILED`].
pub mod failure {
    /// Uncoded internal failure; the English text travels as `detail` in the failure details.
    pub const OPERATION_FAILED: &str = "operationFailed";
    /// The original path is occupied, so undo would overwrite a later change.
    pub const UNDO_PATH_OCCUPIED: &str = "undoPathOccupied";
    /// The output to undo is gone.
    pub const UNDO_OUTPUT_MISSING: &str = "undoOutputMissing";
    /// The output to undo changed after completion.
    pub const UNDO_OUTPUT_CHANGED: &str = "undoOutputChanged";
    /// The backup copy needed for undo is gone.
    pub const UNDO_BACKUP_MISSING: &str = "undoBackupMissing";
    /// The entry to restore is gone.
    pub const UNDO_ENTRY_MISSING: &str = "undoEntryMissing";
    /// The attributes changed after the operation, so they cannot be restored.
    pub const UNDO_ATTRIBUTES_CHANGED: &str = "undoAttributesChanged";
    /// A read-only entry needs an explicit override (`location`).
    pub const READ_ONLY_NEEDS_OVERRIDE: &str = "readOnlyNeedsOverride";
    /// A file and a directory cannot replace one another.
    pub const KIND_MISMATCH: &str = "kindMismatch";
    /// An intermediate path component is not a directory.
    pub const PATH_COMPONENT_NOT_DIRECTORY: &str = "pathComponentNotDirectory";
    /// The source is not a regular file (`name`).
    pub const NOT_REGULAR_FILE: &str = "notRegularFile";
    /// No free name was found among the numbered alternatives.
    pub const NAME_COLLISION_LIMIT: &str = "nameCollisionLimit";
    /// A folder cannot be replaced by a file.
    pub const FOLDER_REPLACE_BLOCKED: &str = "folderReplaceBlocked";
    /// The source changed while it was being split.
    pub const SOURCE_CHANGED_WHILE_SPLITTING: &str = "sourceChangedWhileSplitting";
    /// A split would produce too many parts (`name`, `max`).
    pub const TOO_MANY_PARTS: &str = "tooManyParts";
    /// The image is too large to edit (`size`, `limit` in bytes).
    pub const IMAGE_TOO_LARGE: &str = "imageTooLarge";
    /// A selection expands to too many entries (`limit`).
    pub const TOO_MANY_ENTRIES: &str = "tooManyEntries";
    /// A relative link target is impossible across volumes.
    pub const RELATIVE_LINK_ACROSS_VOLUMES: &str = "relativeLinkAcrossVolumes";
    /// The link target is not valid Unicode.
    pub const LINK_TARGET_NOT_UNICODE: &str = "linkTargetNotUnicode";
    /// The operation has no pending conflict to resolve.
    pub const NO_PENDING_CONFLICT: &str = "noPendingConflict";
}
