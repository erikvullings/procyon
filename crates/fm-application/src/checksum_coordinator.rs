//! Checksum-job and duplicate-scan coordination (task 0077, extracted as
//! part of task 0119's decomposition of the `FileManagerService` facade).
//!
//! Owns the [`ChecksumEngine`] plus its two results stores and publishes the
//! initial `OperationCreated` event both job kinds share, mirroring
//! [`crate::connection_facade::ConnectionFacade`]'s shape: state owned here,
//! a small method-per-capability interface, DTO conversion delegated to
//! `checksum_mapping`'s pure functions. `save_checksum_file` is the one
//! method needing the provider registry (to actually write the destination
//! file); it takes `&ProviderRegistry` as a parameter rather than the
//! coordinator owning a second clone of it, the same choice
//! `content_streaming`'s functions made for the same reason (task 0119,
//! third pass).

use std::sync::Arc;

use fm_checksum::{
    ChecksumEngine, ChecksumJobOptions, ChecksumResultsStore, ChecksumTarget, DuplicateOptions,
    DuplicateResultsStore, DuplicateScanOptions,
};
use fm_domain::{EntryId, Location, OperationId};
use fm_events::{
    BackendEventPayload, ConflictPolicyPayload, EntryRefPayload, EventAudience, EventBus,
    OperationKindPayload, OperationPayload, OperationProgressDetails, OperationStatePayload,
};
use fm_transport_dto::{
    ChecksumFileDto, ChecksumPageDto, DuplicatePageDto, RenderChecksumFileRequestDto,
    SaveChecksumFileRequestDto, SaveChecksumFileResponseDto, StartChecksumRequestDto,
    StartChecksumResponseDto, StartDuplicateScanRequestDto, StartDuplicateScanResponseDto,
    VerificationReportDto, VerifyChecksumFileRequestDto,
};
use fm_vfs::EntryRef;
use uuid::Uuid;

use crate::checksum_mapping::{
    checksum_algorithm, checksum_algorithm_dto, checksum_entry_dto, duplicate_group_dto,
    duplicate_stats_dto, verification_result_dto,
};
use crate::error::ApplicationError;

pub(crate) struct ChecksumCoordinator {
    checksum: ChecksumEngine,
    checksum_store: Arc<ChecksumResultsStore>,
    duplicate_store: Arc<DuplicateResultsStore>,
    events: EventBus,
}

impl ChecksumCoordinator {
    pub(crate) fn new(
        checksum: ChecksumEngine,
        checksum_store: Arc<ChecksumResultsStore>,
        duplicate_store: Arc<DuplicateResultsStore>,
        events: EventBus,
    ) -> Self {
        Self {
            checksum,
            checksum_store,
            duplicate_store,
            events,
        }
    }

    /// Starts a checksum job over the requested entries. The job id doubles
    /// as the operation id (mirrors comparison's `start_comparison`), so the
    /// generic `/operations/{id}/cancel` route and the operation centre can
    /// address a running job without a separate id space.
    pub(crate) fn start_checksums(
        &self,
        request: StartChecksumRequestDto,
    ) -> Result<StartChecksumResponseDto, ApplicationError> {
        let job_id = Uuid::new_v4();
        let operation_id = OperationId::from(job_id);
        let audience = EventAudience::Workspace(request.workspace_id.into());

        let targets: Vec<ChecksumTarget> = request
            .entries
            .into_iter()
            .map(|dto| {
                let location: Location = dto.into();
                // The last path segment is the relative path a checksum file
                // records. A flat selection is the overwhelmingly common
                // case, and it keeps a saved file valid next to its entries.
                let relative_path = location.name().unwrap_or_else(|_| location.uri.clone());
                ChecksumTarget {
                    entry: EntryRef {
                        id: EntryId::new(),
                        location,
                    },
                    relative_path,
                    size: 0,
                }
            })
            .collect();
        let algorithms: Vec<_> = request
            .algorithms
            .into_iter()
            .map(checksum_algorithm)
            .collect();

        let sources: Vec<EntryRefPayload> = targets
            .iter()
            .map(|target| EntryRefPayload {
                id: target.entry.id,
                location: target.entry.location.clone().into(),
            })
            .collect();
        let total_items = u64::try_from(targets.len()).unwrap_or(u64::MAX);

        self.events.publish(
            audience.clone(),
            BackendEventPayload::OperationCreated {
                operation: OperationPayload {
                    id: operation_id,
                    kind: OperationKindPayload::Checksum,
                    state: OperationStatePayload::Running,
                    sources,
                    destination: None,
                    progress: OperationProgressDetails {
                        completed_items: 0,
                        total_items: Some(total_items),
                        completed_bytes: 0,
                        total_bytes: None,
                        current_entry: None,
                        bytes_per_second: None,
                    },
                    conflict_policy: ConflictPolicyPayload::Ask,
                    created_at: chrono::Utc::now(),
                    started_at: None,
                    completed_at: None,
                },
            },
        );

        self.checksum
            .start_checksums(
                job_id,
                targets,
                ChecksumJobOptions {
                    algorithms,
                    operation_id: Some(operation_id),
                },
                audience,
            )
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        Ok(StartChecksumResponseDto { job_id })
    }

    /// Cancels a running checksum job.
    pub(crate) fn cancel_checksums(&self, job_id: Uuid) -> Result<(), ApplicationError> {
        self.checksum
            .cancel_checksums(job_id)
            .map_err(|_| ApplicationError::NotFound)
    }

    /// Returns a bounded page of a checksum job's results.
    pub(crate) fn get_checksum_page(
        &self,
        job_id: Uuid,
        offset: u64,
        limit: u16,
    ) -> Result<ChecksumPageDto, ApplicationError> {
        let limit = limit.clamp(1, 500);
        let page = self
            .checksum_store
            .page(
                job_id,
                usize::try_from(offset).unwrap_or(usize::MAX),
                usize::from(limit),
            )
            .ok_or(ApplicationError::NotFound)?;
        Ok(ChecksumPageDto {
            job_id,
            algorithms: page
                .algorithms
                .iter()
                .map(|algorithm| checksum_algorithm_dto(*algorithm))
                .collect(),
            offset,
            limit,
            total: u64::try_from(page.total).unwrap_or(u64::MAX),
            total_entries: u64::try_from(page.total_entries).unwrap_or(u64::MAX),
            entries: page.entries.iter().map(checksum_entry_dto).collect(),
            is_complete: page.is_complete,
            is_cancelled: page.is_cancelled,
            has_more: page.has_more,
        })
    }

    /// Renders a job's results as coreutils-compatible checksum-file text.
    ///
    /// Returns the text rather than writing a file: saving goes through the
    /// caller's normal write path, so this never becomes a second,
    /// unaudited way to create a file (spec §35).
    pub(crate) fn render_checksum_file(
        &self,
        job_id: Uuid,
        request: RenderChecksumFileRequestDto,
    ) -> Result<ChecksumFileDto, ApplicationError> {
        let algorithm = checksum_algorithm(request.algorithm);
        let entries = self
            .checksum_store
            .all_entries(job_id)
            .ok_or(ApplicationError::NotFound)?;
        let lines: Vec<(String, String)> = entries
            .iter()
            .filter_map(|entry| {
                entry
                    .checksums
                    .get(algorithm)
                    .map(|digest| (entry.relative_path.clone(), digest.to_owned()))
            })
            .collect();
        let mut buffer = Vec::new();
        fm_checksum::write_checksum_file(&lines, algorithm, &mut buffer)
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        let content = String::from_utf8(buffer)
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        Ok(ChecksumFileDto {
            suggested_name: format!("checksums.{algorithm}"),
            content,
        })
    }

    /// Writes a job's results to a checksum file through the provider's
    /// normal `WRITE` path (task 0077).
    ///
    /// Deliberately server-side rather than a host-native save dialog: this
    /// keeps every file this application creates on one audited,
    /// capability-gated path (spec §35), and makes saving behave identically
    /// under the Axum and Tauri hosts.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] for an unknown job, and an
    /// invalid-request error if the destination's provider cannot be
    /// resolved, lacks `WRITE`, or already holds a file and `overwrite` is
    /// false.
    pub(crate) async fn save_checksum_file(
        &self,
        job_id: Uuid,
        request: SaveChecksumFileRequestDto,
        providers: &fm_vfs::ProviderRegistry,
    ) -> Result<SaveChecksumFileResponseDto, ApplicationError> {
        use tokio::io::AsyncWriteExt as _;

        let rendered = self.render_checksum_file(
            job_id,
            RenderChecksumFileRequestDto {
                algorithm: request.algorithm,
            },
        )?;
        let destination: Location = request.destination.into();
        let provider = providers
            .resolve(&destination)
            .map_err(ApplicationError::from)?;
        let capabilities = provider
            .capabilities_for(&destination)
            .map_err(ApplicationError::from)?;
        capabilities
            .require(fm_vfs::ProviderCapabilities::WRITE)
            .map_err(ApplicationError::from)?;

        let cancellation = tokio_util::sync::CancellationToken::new();
        let mut writer = provider
            .open_write(
                &destination,
                fm_vfs::WriteOptions {
                    overwrite: request.overwrite,
                },
                cancellation,
            )
            .await
            .map_err(ApplicationError::from)?;
        let bytes = rendered.content.into_bytes();
        writer
            .write_all(&bytes)
            .await
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        writer
            .shutdown()
            .await
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;

        Ok(SaveChecksumFileResponseDto {
            location: destination.into(),
            bytes_written: bytes.len() as u64,
        })
    }

    /// Verifies a job's computed digests against an existing checksum file,
    /// reporting per-entry match, mismatch or missing.
    pub(crate) fn verify_checksum_file(
        &self,
        job_id: Uuid,
        request: VerifyChecksumFileRequestDto,
    ) -> Result<VerificationReportDto, ApplicationError> {
        let entries = self
            .checksum_store
            .all_entries(job_id)
            .ok_or(ApplicationError::NotFound)?;
        let recorded = fm_checksum::read_checksum_file(std::io::Cursor::new(request.content))
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;

        // A checksum file carries one algorithm and no in-band marker, so the
        // digest width decides which of the job's digests to compare against.
        // 64 hex characters are ambiguous between SHA-256 and BLAKE3; SHA-256
        // is assumed there because that is what `sha256sum` writes.
        let algorithm = recorded
            .first()
            .and_then(|entry| entry.algorithm)
            .unwrap_or(fm_checksum::ChecksumAlgorithm::Sha256);
        let computed: Vec<(String, String)> = entries
            .iter()
            .filter_map(|entry| {
                entry
                    .checksums
                    .get(algorithm)
                    .map(|digest| (entry.relative_path.clone(), digest.to_owned()))
            })
            .collect();
        let results = fm_checksum::verify(
            computed
                .iter()
                .map(|(path, digest)| (path.as_str(), digest.as_str())),
            &recorded,
        );

        let mut matched = 0_u64;
        let mut mismatched = 0_u64;
        let mut missing = 0_u64;
        for result in &results {
            match result.status {
                fm_checksum::VerificationStatus::Match => matched += 1,
                fm_checksum::VerificationStatus::Mismatch { .. } => mismatched += 1,
                fm_checksum::VerificationStatus::Missing => missing += 1,
            }
        }
        Ok(VerificationReportDto {
            job_id,
            results: results.iter().map(verification_result_dto).collect(),
            matched,
            mismatched,
            missing,
        })
    }

    /// Starts a cancellable duplicate scan across one or more roots, using
    /// the staged size -> partial-hash -> full-hash strategy (task 0077).
    pub(crate) fn start_duplicate_scan(
        &self,
        request: StartDuplicateScanRequestDto,
    ) -> Result<StartDuplicateScanResponseDto, ApplicationError> {
        let scan_id = Uuid::new_v4();
        let operation_id = OperationId::from(scan_id);
        let audience = EventAudience::Workspace(request.workspace_id.into());
        let roots: Vec<Location> = request.roots.into_iter().map(Into::into).collect();

        let sources: Vec<EntryRefPayload> = roots
            .iter()
            .map(|root| EntryRefPayload {
                id: EntryId::new(),
                location: root.clone().into(),
            })
            .collect();
        self.events.publish(
            audience.clone(),
            BackendEventPayload::OperationCreated {
                operation: OperationPayload {
                    id: operation_id,
                    kind: OperationKindPayload::FindDuplicates,
                    state: OperationStatePayload::Running,
                    sources,
                    destination: None,
                    progress: OperationProgressDetails {
                        completed_items: 0,
                        total_items: None,
                        completed_bytes: 0,
                        total_bytes: None,
                        current_entry: None,
                        bytes_per_second: None,
                    },
                    conflict_policy: ConflictPolicyPayload::Ask,
                    created_at: chrono::Utc::now(),
                    started_at: None,
                    completed_at: None,
                },
            },
        );

        self.checksum
            .start_duplicate_scan(
                scan_id,
                roots,
                DuplicateScanOptions {
                    detection: DuplicateOptions {
                        include_empty_files: request.include_empty_files,
                        ..DuplicateOptions::default()
                    },
                    show_hidden: request.show_hidden,
                    operation_id: Some(operation_id),
                },
                audience,
            )
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        Ok(StartDuplicateScanResponseDto { scan_id })
    }

    /// Cancels a running duplicate scan.
    pub(crate) fn cancel_duplicate_scan(&self, scan_id: Uuid) -> Result<(), ApplicationError> {
        self.checksum
            .cancel_duplicate_scan(scan_id)
            .map_err(|_| ApplicationError::NotFound)
    }

    /// Returns a bounded page of a duplicate scan's grouped results.
    pub(crate) fn get_duplicate_page(
        &self,
        scan_id: Uuid,
        offset: u64,
        limit: u16,
    ) -> Result<DuplicatePageDto, ApplicationError> {
        let limit = limit.clamp(1, 500);
        let page = self
            .duplicate_store
            .page(
                scan_id,
                usize::try_from(offset).unwrap_or(usize::MAX),
                usize::from(limit),
            )
            .ok_or(ApplicationError::NotFound)?;
        Ok(DuplicatePageDto {
            scan_id,
            roots: page.roots.into_iter().map(Into::into).collect(),
            offset,
            limit,
            total: u64::try_from(page.total).unwrap_or(u64::MAX),
            groups: page.groups.iter().map(duplicate_group_dto).collect(),
            is_complete: page.is_complete,
            is_cancelled: page.is_cancelled,
            has_more: page.has_more,
            stats: duplicate_stats_dto(page.stats),
            warnings_count: u32::try_from(page.warnings.len()).unwrap_or(u32::MAX),
        })
    }
}

#[cfg(test)]
mod tests {
    use fm_transport_dto::ChecksumAlgorithmDto;
    use fm_vfs::ProviderRegistry;
    use fm_vfs_local::LocalFileSystemProvider;

    use super::*;

    fn coordinator() -> ChecksumCoordinator {
        let checksum_store = Arc::new(ChecksumResultsStore::new());
        let duplicate_store = Arc::new(DuplicateResultsStore::new());
        let events = EventBus::default();
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(LocalFileSystemProvider));
        let checksum = ChecksumEngine::new(
            Arc::clone(&checksum_store),
            Arc::clone(&duplicate_store),
            events.clone(),
            providers,
        );
        ChecksumCoordinator::new(checksum, checksum_store, duplicate_store, events)
    }

    fn workspace_id() -> Uuid {
        Uuid::new_v4()
    }

    #[test]
    fn get_checksum_page_reports_not_found_for_an_unknown_job() {
        let coordinator = coordinator();
        assert_eq!(
            coordinator.get_checksum_page(Uuid::new_v4(), 0, 50),
            Err(ApplicationError::NotFound)
        );
    }

    #[test]
    fn render_checksum_file_reports_not_found_for_an_unknown_job() {
        let coordinator = coordinator();
        assert_eq!(
            coordinator.render_checksum_file(
                Uuid::new_v4(),
                RenderChecksumFileRequestDto {
                    algorithm: ChecksumAlgorithmDto::Sha256,
                },
            ),
            Err(ApplicationError::NotFound)
        );
    }

    #[test]
    fn verify_checksum_file_reports_not_found_for_an_unknown_job() {
        let coordinator = coordinator();
        assert_eq!(
            coordinator.verify_checksum_file(
                Uuid::new_v4(),
                VerifyChecksumFileRequestDto {
                    content: "deadbeef  a.txt\n".to_owned(),
                },
            ),
            Err(ApplicationError::NotFound)
        );
    }

    #[test]
    fn get_duplicate_page_reports_not_found_for_an_unknown_scan() {
        let coordinator = coordinator();
        assert_eq!(
            coordinator.get_duplicate_page(Uuid::new_v4(), 0, 50),
            Err(ApplicationError::NotFound)
        );
    }

    #[test]
    fn cancel_checksums_reports_not_found_for_an_unknown_job() {
        let coordinator = coordinator();
        assert_eq!(
            coordinator.cancel_checksums(Uuid::new_v4()),
            Err(ApplicationError::NotFound)
        );
    }

    #[test]
    fn cancel_duplicate_scan_reports_not_found_for_an_unknown_scan() {
        let coordinator = coordinator();
        assert_eq!(
            coordinator.cancel_duplicate_scan(Uuid::new_v4()),
            Err(ApplicationError::NotFound)
        );
    }

    #[tokio::test]
    async fn start_checksums_publishes_an_operation_created_event_and_returns_a_job_id() {
        let coordinator = coordinator();
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"hello").expect("must write file");
        let location =
            Location::from_native_path(&file).expect("native path must convert to a location");

        let response = coordinator
            .start_checksums(StartChecksumRequestDto {
                workspace_id: workspace_id(),
                entries: vec![location.into()],
                algorithms: vec![ChecksumAlgorithmDto::Sha256],
            })
            .expect("start_checksums must succeed");

        // The job id doubles as the operation id: a page becomes available
        // (possibly still in-progress) for that same id without a second
        // lookup.
        let page = coordinator
            .get_checksum_page(response.job_id, 0, 50)
            .expect("job must be known immediately after starting");
        assert_eq!(page.job_id, response.job_id);
    }

    #[tokio::test]
    async fn start_duplicate_scan_returns_a_scan_id_whose_page_is_immediately_queryable() {
        let coordinator = coordinator();
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let location =
            Location::from_native_path(dir.path()).expect("native path must convert to a location");

        let response = coordinator
            .start_duplicate_scan(StartDuplicateScanRequestDto {
                workspace_id: workspace_id(),
                roots: vec![location.into()],
                include_empty_files: false,
                show_hidden: false,
            })
            .expect("start_duplicate_scan must succeed");

        let page = coordinator
            .get_duplicate_page(response.scan_id, 0, 50)
            .expect("scan must be known immediately after starting");
        assert_eq!(page.scan_id, response.scan_id);
    }
}
