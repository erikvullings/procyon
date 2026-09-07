//! Explicit application/transport mappings for desktop OCR remediation.

use fm_transport_dto::{
    SemanticOcrAvailabilityDto, SemanticOcrErrorCodeDto, SemanticOcrErrorDto,
    SemanticOcrFileOutcomeDto, SemanticOcrJobDto, SemanticOcrJobStateDto, SemanticOcrOutcomeDto,
    SemanticOcrStatusDto, SemanticOcrTargetDto, SemanticOcrUnavailableReasonDto,
    StartSemanticOcrRemediationRequestDto,
};

use crate::semantic_ocr::{
    OcrAvailability, OcrRemediationFileOutcome, OcrRemediationJob, OcrRemediationOutcome,
    OcrRemediationScope, OcrRemediationState, OcrRemediationTarget, OcrUnavailableReason,
    SemanticOcrError, SemanticOcrStatus,
};

/// Maps complete application OCR state to the stable desktop wire contract.
#[must_use]
pub fn semantic_ocr_status_to_dto(status: SemanticOcrStatus) -> SemanticOcrStatusDto {
    SemanticOcrStatusDto {
        enabled: status.enabled,
        availability: availability_to_dto(status.availability),
        reported_files: status
            .reported_files
            .into_iter()
            .map(target_to_dto)
            .collect(),
        jobs: status.jobs.into_iter().map(job_to_dto).collect(),
    }
}

/// Parses one transport scope without trusting any path as an executable.
///
/// The resulting locations are still only requests: `FileManagerService`
/// intersects them with its backend-owned OCR-required report before opening
/// any file.
pub fn remediation_scope_from_dto(
    request: StartSemanticOcrRemediationRequestDto,
) -> Result<OcrRemediationScope, SemanticOcrError> {
    match request {
        StartSemanticOcrRemediationRequestDto::OneFile { file } => {
            Ok(OcrRemediationScope::File(target_from_dto(file)?))
        }
        StartSemanticOcrRemediationRequestDto::SelectedFiles { files } => files
            .into_iter()
            .map(target_from_dto)
            .collect::<Result<Vec<_>, _>>()
            .map(OcrRemediationScope::SelectedFiles),
        StartSemanticOcrRemediationRequestDto::EnrolledRoot { root_id } => root_id
            .parse()
            .map(OcrRemediationScope::Root)
            .map_err(|_| SemanticOcrError::InvalidRequest),
        StartSemanticOcrRemediationRequestDto::AllReported => Ok(OcrRemediationScope::AllRequired),
    }
}

/// Maps one job snapshot to the stable desktop wire contract.
#[must_use]
pub fn semantic_ocr_job_to_dto(job: OcrRemediationJob) -> SemanticOcrJobDto {
    job_to_dto(job)
}

/// Maps one application failure to a structured Tauri error.
#[must_use]
pub fn semantic_ocr_error_to_dto(error: SemanticOcrError) -> SemanticOcrErrorDto {
    let message = error.to_string();
    let (code, availability_reason, maximum) = match error {
        SemanticOcrError::Disabled => (SemanticOcrErrorCodeDto::Disabled, None, None),
        SemanticOcrError::Unavailable(reason) => (
            SemanticOcrErrorCodeDto::Unavailable,
            Some(unavailable_reason_to_dto(reason)),
            None,
        ),
        SemanticOcrError::NotFound => (SemanticOcrErrorCodeDto::NotFound, None, None),
        SemanticOcrError::NothingToRemediate => {
            (SemanticOcrErrorCodeDto::NothingToRemediate, None, None)
        }
        SemanticOcrError::UnreportedTarget => {
            (SemanticOcrErrorCodeDto::UnreportedTarget, None, None)
        }
        SemanticOcrError::QueueFull { maximum } => (
            SemanticOcrErrorCodeDto::QueueFull,
            None,
            Some(u64::try_from(maximum).unwrap_or(u64::MAX)),
        ),
        SemanticOcrError::TooManyTargets { maximum } => (
            SemanticOcrErrorCodeDto::TooManyTargets,
            None,
            Some(u64::try_from(maximum).unwrap_or(u64::MAX)),
        ),
        SemanticOcrError::InvalidRequest => (SemanticOcrErrorCodeDto::InvalidRequest, None, None),
        SemanticOcrError::Persist(_) => (SemanticOcrErrorCodeDto::Persist, None, None),
        SemanticOcrError::WorkerRestart(_) => (SemanticOcrErrorCodeDto::WorkerRestart, None, None),
    };
    SemanticOcrErrorDto {
        code,
        message,
        availability_reason,
        maximum,
    }
}

fn target_from_dto(target: SemanticOcrTargetDto) -> Result<OcrRemediationTarget, SemanticOcrError> {
    Ok(OcrRemediationTarget {
        root_id: target
            .root_id
            .parse()
            .map_err(|_| SemanticOcrError::InvalidRequest)?,
        location: target.location.into(),
    })
}

fn target_to_dto(target: OcrRemediationTarget) -> SemanticOcrTargetDto {
    SemanticOcrTargetDto {
        root_id: target.root_id.to_string(),
        location: target.location.into(),
    }
}

fn availability_to_dto(availability: OcrAvailability) -> SemanticOcrAvailabilityDto {
    match availability {
        OcrAvailability::Available {
            executable,
            version,
        } => SemanticOcrAvailabilityDto::Available {
            executable: executable.to_string_lossy().into_owned(),
            version,
        },
        OcrAvailability::Unavailable { reason, guidance } => {
            SemanticOcrAvailabilityDto::Unavailable {
                reason: unavailable_reason_to_dto(reason),
                guidance,
            }
        }
    }
}

fn unavailable_reason_to_dto(reason: OcrUnavailableReason) -> SemanticOcrUnavailableReasonDto {
    match reason {
        OcrUnavailableReason::HostUnavailable => SemanticOcrUnavailableReasonDto::HostUnavailable,
        OcrUnavailableReason::Missing => SemanticOcrUnavailableReasonDto::Missing,
        OcrUnavailableReason::NonExecutable { path } => {
            SemanticOcrUnavailableReasonDto::NonExecutable {
                path: path.to_string_lossy().into_owned(),
            }
        }
        OcrUnavailableReason::CouldNotExecute => SemanticOcrUnavailableReasonDto::CouldNotExecute,
        OcrUnavailableReason::MalformedVersion => SemanticOcrUnavailableReasonDto::MalformedVersion,
        OcrUnavailableReason::UnsupportedVersion { version } => {
            SemanticOcrUnavailableReasonDto::UnsupportedVersion { version }
        }
        OcrUnavailableReason::TimedOut => SemanticOcrUnavailableReasonDto::TimedOut,
        OcrUnavailableReason::OutputTooLarge { limit } => {
            SemanticOcrUnavailableReasonDto::OutputTooLarge { limit }
        }
    }
}

fn job_to_dto(job: OcrRemediationJob) -> SemanticOcrJobDto {
    SemanticOcrJobDto {
        id: job.id.to_string(),
        state: match job.state {
            OcrRemediationState::Queued => SemanticOcrJobStateDto::Queued,
            OcrRemediationState::Running => SemanticOcrJobStateDto::Running,
            OcrRemediationState::Completed => SemanticOcrJobStateDto::Completed,
            OcrRemediationState::Failed => SemanticOcrJobStateDto::Failed,
            OcrRemediationState::Cancelled => SemanticOcrJobStateDto::Cancelled,
        },
        created_at_ms: job.created_at_ms,
        updated_at_ms: job.updated_at_ms,
        total_files: job.total_files,
        processed_files: job.processed_files(),
        files: job.files.into_iter().map(file_outcome_to_dto).collect(),
        availability_failure: job.availability_failure.map(unavailable_reason_to_dto),
    }
}

fn file_outcome_to_dto(outcome: OcrRemediationFileOutcome) -> SemanticOcrFileOutcomeDto {
    SemanticOcrFileOutcomeDto {
        root_id: outcome.root_id.to_string(),
        location: outcome.location.into(),
        outcome: match outcome.outcome {
            OcrRemediationOutcome::Succeeded => SemanticOcrOutcomeDto::Succeeded,
            OcrRemediationOutcome::PostOcrNoText { detail } => {
                SemanticOcrOutcomeDto::PostOcrNoText { detail }
            }
            OcrRemediationOutcome::ExecutionFailure { detail } => {
                SemanticOcrOutcomeDto::ExecutionFailure { detail }
            }
            OcrRemediationOutcome::Skipped { detail } => SemanticOcrOutcomeDto::Skipped { detail },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_root_identifiers_are_rejected_before_scope_resolution() {
        let error =
            remediation_scope_from_dto(StartSemanticOcrRemediationRequestDto::EnrolledRoot {
                root_id: "not-a-root-id".to_owned(),
            })
            .unwrap_err();
        assert!(matches!(error, SemanticOcrError::InvalidRequest));
    }

    #[test]
    fn unsupported_version_details_survive_error_mapping() {
        let dto = semantic_ocr_error_to_dto(SemanticOcrError::Unavailable(
            OcrUnavailableReason::UnsupportedVersion {
                version: "18.0.0".to_owned(),
            },
        ));
        assert_eq!(dto.code, SemanticOcrErrorCodeDto::Unavailable);
        assert_eq!(
            dto.availability_reason,
            Some(SemanticOcrUnavailableReasonDto::UnsupportedVersion {
                version: "18.0.0".to_owned(),
            })
        );
    }
}
