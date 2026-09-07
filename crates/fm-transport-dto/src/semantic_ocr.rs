//! Stable desktop transport types for optional OCRmyPDF remediation (task 0197).

use serde::{Deserialize, Serialize};

use crate::LocationDto;

/// Availability of a safely discovered OCRmyPDF executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SemanticOcrAvailabilityDto {
    /// A supported executable is available.
    Available {
        /// Canonical host path selected by backend discovery.
        executable: String,
        /// Resolved semantic version.
        version: String,
    },
    /// OCR cannot currently be enabled or run.
    Unavailable {
        /// Machine-readable rejection detail.
        reason: SemanticOcrUnavailableReasonDto,
        /// Actionable platform-specific installation guidance.
        guidance: String,
    },
}

/// Machine-readable reason OCRmyPDF discovery was rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SemanticOcrUnavailableReasonDto {
    /// This host intentionally has no desktop OCR capability.
    HostUnavailable,
    /// No executable was found in a safe location.
    Missing,
    /// A candidate is not a runnable regular file.
    NonExecutable {
        /// Canonical candidate path.
        path: String,
    },
    /// The version process could not be started.
    CouldNotExecute,
    /// Version output was not a supported semantic-version shape.
    MalformedVersion,
    /// The installed release is outside the audited range.
    UnsupportedVersion {
        /// Installed version.
        version: String,
    },
    /// The bounded version probe timed out.
    TimedOut,
    /// Version output exceeded the capture bound.
    OutputTooLarge {
        /// Maximum combined output bytes.
        limit: u64,
    },
}

/// One backend-reported OCR remediation target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticOcrTargetDto {
    /// Stable enrolled-root identifier.
    pub root_id: String,
    /// Provider-neutral source location.
    pub location: LocationDto,
}

/// Lifecycle state of one durable OCR remediation job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SemanticOcrJobStateDto {
    /// Waiting for the single bounded worker.
    Queued,
    /// One or more target files are being processed.
    Running,
    /// Every target reached a non-failing terminal outcome.
    Completed,
    /// Availability, OCR, or ingestion failed.
    Failed,
    /// The user cancelled the job.
    Cancelled,
}

/// Typed per-file OCR remediation result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SemanticOcrOutcomeDto {
    /// OCR produced searchable text and ingestion completed.
    Succeeded,
    /// OCR completed but deterministic conversion still found no text.
    PostOcrNoText {
        /// Sanitized worker diagnostic.
        detail: String,
    },
    /// The executable or semantic ingestion failed.
    ExecutionFailure {
        /// Sanitized worker diagnostic.
        detail: String,
    },
    /// The file no longer met remediation preconditions.
    Skipped {
        /// Sanitized reason.
        detail: String,
    },
}

/// One file's result within a remediation job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticOcrFileOutcomeDto {
    /// Stable enrolled-root identifier.
    pub root_id: String,
    /// Provider-neutral source location.
    pub location: LocationDto,
    /// Typed result.
    pub outcome: SemanticOcrOutcomeDto,
}

/// Durable public snapshot of one remediation job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticOcrJobDto {
    /// Opaque job identifier.
    pub id: String,
    /// Current lifecycle state.
    pub state: SemanticOcrJobStateDto,
    /// Creation time in Unix milliseconds.
    pub created_at_ms: i64,
    /// Last update time in Unix milliseconds.
    pub updated_at_ms: i64,
    /// Number of concrete target files.
    pub total_files: u64,
    /// Number of recorded file results.
    pub processed_files: u64,
    /// Per-file results.
    pub files: Vec<SemanticOcrFileOutcomeDto>,
    /// Availability failure captured when the job started.
    pub availability_failure: Option<SemanticOcrUnavailableReasonDto>,
}

/// Complete OCR Settings state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticOcrStatusDto {
    /// Whether the user explicitly opted in.
    pub enabled: bool,
    /// Current executable discovery result.
    pub availability: SemanticOcrAvailabilityDto,
    /// Backend-owned files that currently require OCR.
    pub reported_files: Vec<SemanticOcrTargetDto>,
    /// Durable jobs, newest first.
    pub jobs: Vec<SemanticOcrJobDto>,
}

/// Explicit consent update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSemanticOcrConsentRequestDto {
    /// New opt-in value.
    pub enabled: bool,
}

/// One of the four supported remediation scopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "scope",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StartSemanticOcrRemediationRequestDto {
    /// One reported file.
    OneFile {
        /// Requested file.
        file: SemanticOcrTargetDto,
    },
    /// Explicitly selected reported files.
    SelectedFiles {
        /// Requested files.
        files: Vec<SemanticOcrTargetDto>,
    },
    /// Every reported file under one enrolled root.
    EnrolledRoot {
        /// Enrolled-root identifier.
        root_id: String,
    },
    /// Every currently reported OCR-required file.
    AllReported,
}

/// Cancels one queued or running remediation job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelSemanticOcrRemediationRequestDto {
    /// Opaque job identifier.
    pub job_id: String,
}

/// Stable OCR remediation error category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SemanticOcrErrorCodeDto {
    /// Explicit consent is disabled.
    Disabled,
    /// No supported executable is available.
    Unavailable,
    /// The requested job does not exist.
    NotFound,
    /// The scope has no current targets.
    NothingToRemediate,
    /// An explicit target is stale or was never reported.
    UnreportedTarget,
    /// The active queue is full.
    QueueFull,
    /// The request exceeds its file bound.
    TooManyTargets,
    /// The request is malformed.
    InvalidRequest,
    /// Durable state could not be written.
    Persist,
    /// The semantic worker could not be retired.
    WorkerRestart,
}

/// Structured OCR error shared by Rust and the Tauri adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticOcrErrorDto {
    /// Stable category.
    pub code: SemanticOcrErrorCodeDto,
    /// Safe actionable message.
    pub message: String,
    /// Availability detail for an unavailable operation.
    pub availability_reason: Option<SemanticOcrUnavailableReasonDto>,
    /// Relevant configured maximum for a bounded rejection.
    pub maximum: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_and_outcome_discriminators_are_stable() {
        let request = StartSemanticOcrRemediationRequestDto::SelectedFiles {
            files: vec![SemanticOcrTargetDto {
                root_id: "root".to_owned(),
                location: LocationDto {
                    provider_id: "local".to_owned(),
                    uri: "file:///scan.pdf".to_owned(),
                },
            }],
        };
        let request = serde_json::to_value(request).unwrap();
        assert_eq!(request["scope"], "selectedFiles");
        assert_eq!(request["files"][0]["rootId"], "root");

        let outcome = serde_json::to_value(SemanticOcrOutcomeDto::PostOcrNoText {
            detail: "no text".to_owned(),
        })
        .unwrap();
        assert_eq!(outcome["outcome"], "postOcrNoText");
        assert_eq!(outcome["detail"], "no text");
    }
}
