//! Wire types for transfers interrupted by a crash or restart (task 0300).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{LocationDto, OperationKindDto};

/// What can safely be done with an interrupted transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum TransferRecoveryClassDto {
    /// The provider can prove the partial destination belongs to this job and the source
    /// is unchanged, so the transfer can continue from where it stopped.
    Resumable,
    /// Nothing that matters was published; discarding the private partial and running the
    /// remaining items again is safe.
    SafelyRestartable,
    /// Every remaining item appears to have been published, but the job never confirmed it.
    CompletedUnconfirmed,
    /// Something changed or cannot be checked; a person has to decide.
    NeedsIntervention,
}

/// Why an interrupted transfer was classified the way it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum TransferRecoveryReasonDto {
    /// The job was accepted but no item had started.
    NotStarted,
    /// An item was part-way through; its private partial is discarded before a restart.
    PartialUnfinished,
    /// The destination exists but the job never recorded finishing it.
    PublishedUnconfirmed,
    /// A source changed since the job planned it.
    SourceChanged,
    /// A source no longer exists and its destination was not published.
    SourceMissing,
    /// A destination exists that the job did not publish, or no longer matches its source.
    DestinationReplaced,
    /// A provider could not be reached to check the transfer.
    ProviderUnavailable,
    /// The journal was corrupted in the middle, so how far the job got is unknown.
    JournalDamaged,
}

/// One transfer that was queued or running when the previous process stopped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InterruptedTransferDto {
    /// Identifier of the interrupted job.
    pub operation_id: String,
    /// What the job was doing.
    pub kind: OperationKindDto,
    /// When the job was accepted.
    pub created_at: DateTime<Utc>,
    /// What can safely be done with it.
    pub class: TransferRecoveryClassDto,
    /// Why it was classified this way.
    pub reason: TransferRecoveryReasonDto,
    /// Requested sources.
    pub sources: Vec<LocationDto>,
    /// Requested destination, if any.
    pub destination: Option<LocationDto>,
    /// Items the job had planned.
    pub total_items: u64,
    /// Items confirmed finished.
    pub committed_items: u64,
    /// Items that look published but were never confirmed.
    pub unconfirmed_items: u64,
    /// Private partial destinations that still exist.
    pub partial_items: u64,
    /// Items that still have to run.
    pub remaining_items: u64,
}

/// The interrupted transfers awaiting a decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InterruptedTransfersDto {
    /// Interrupted transfers, oldest first.
    pub transfers: Vec<InterruptedTransferDto>,
}
