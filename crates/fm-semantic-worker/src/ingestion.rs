//! Resumable worker-side semantic ingestion pipeline.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use fm_semantic_conversion::{
    Cancellation, CancellationSignal, Chunker, ConversionBudgets, ConversionContext,
    ConversionOutcome, DocumentConverter, DocumentMetadata, SourceContent,
};
use fm_semantic_docling::converter_with_baseline_fallback;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::embedding::{
    EmbeddingCacheKey, EmbeddingError, EmbeddingModelIdentity, LocalEmbeddingRuntime,
};
use crate::semantic_storage::{
    LibraryIndexManifest, Occurrence, SemanticCatalog, StagedGeneration, StagedRecord, StorageError,
};
use crate::{IngestionState, WorkerIngestionBackend, WorkerIngestionInput, WorkerIngestionJob};

const EMBEDDING_CHECKPOINT_INPUTS: usize = 8;

/// Persisted ingestion/reconciliation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestionStage {
    /// Candidate was discovered.
    Discovered,
    /// Source bytes are being hashed.
    Hashing,
    /// Source is being converted.
    Converting,
    /// Structural units are being chunked.
    Chunking,
    /// Cache misses are being embedded.
    Embedding,
    /// New generation is durable but not visible.
    Staging,
    /// Derived records are being written.
    Publishing,
    /// Removed records are being reclaimed.
    Deleting,
    /// Work completed successfully.
    Complete,
    /// Work failed after one bounded attempt.
    Failed,
    /// Work is paused for an actionable resource reason.
    Paused,
    /// Work was cancelled.
    Cancelled,
    /// User explicitly skipped the failed candidate.
    Skipped,
}

/// Bounded exponential delay before a persisted retry attempt.
#[must_use]
pub const fn retry_backoff(previous_attempts: u32) -> std::time::Duration {
    std::time::Duration::from_millis(match previous_attempts {
        0 => 0,
        1 => 100,
        2 => 250,
        _ => 1_000,
    })
}

impl IngestionStage {
    /// Returns the stable persisted/event value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Discovered => "discovered",
            Self::Hashing => "hashing",
            Self::Converting => "converting",
            Self::Chunking => "chunking",
            Self::Embedding => "embedding",
            Self::Staging => "staging",
            Self::Publishing => "publishing",
            Self::Deleting => "deleting",
            Self::Complete => "complete",
            Self::Failed => "failed",
            Self::Paused => "paused",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
        }
    }
}

/// One host-approved, path-free document candidate.
#[derive(Debug, Clone)]
pub struct IngestionDocument {
    /// Stable job identity.
    pub job_id: String,
    /// Tenant boundary.
    pub tenant_id: String,
    /// Enrolled library.
    pub library_id: String,
    /// Logical document identity.
    pub document_id: String,
    /// Source occurrence identity.
    pub occurrence_id: String,
    /// Opaque source locator retained by the host.
    pub source_id: String,
    /// Enrolled root.
    pub root_id: String,
    /// Optional workspace.
    pub workspace_id: Option<String>,
    /// Trusted media type.
    pub media_type: String,
    /// Source modification time in Unix milliseconds.
    pub modified_at_ms: i64,
    /// Bounded source bytes streamed by the host.
    pub bytes: Vec<u8>,
}

/// Derived occurrence record written before generation publication.
#[derive(Debug, Clone)]
pub struct DerivedRecord {
    /// Stable derived primary key.
    pub record_id: String,
    /// Tenant filter field.
    pub tenant_id: String,
    /// Library filter field.
    pub library_id: String,
    /// Root filter field.
    pub root_id: String,
    /// Optional workspace filter field.
    pub workspace_id: Option<String>,
    /// Media-type filter field.
    pub media_type: String,
    /// Source modification time.
    pub modified_at_ms: i64,
    /// Published generation candidate.
    pub generation: u64,
    /// Normalized vector.
    pub embedding: Vec<f32>,
    /// Complete structurally bounded text indexed for lexical retrieval.
    pub content: String,
    /// Bounded display excerpt.
    pub excerpt: String,
}

/// Derived vector index used only for candidate retrieval.
pub trait DerivedIndex: Send + Sync {
    /// Idempotently stages or replaces records.
    fn upsert(&self, records: &[DerivedRecord]) -> Result<(), String>;
    /// Idempotently removes records.
    fn delete(&self, record_ids: &[String]) -> Result<(), String>;
}

/// Embedding surface consumed by the pipeline.
pub trait EmbeddingProvider: Send + Sync {
    /// Exact immutable model identity.
    fn identity(&self) -> &EmbeddingModelIdentity;
    /// Embeds bounded inputs.
    fn embed(
        &self,
        inputs: &[String],
        cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError>;
}

impl EmbeddingProvider for LocalEmbeddingRuntime {
    fn identity(&self) -> &EmbeddingModelIdentity {
        self.identity()
    }

    fn embed(
        &self,
        inputs: &[String],
        cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        self.embed(inputs, cancellation)
    }
}

/// Current host resource observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceState {
    /// Available bytes beneath the semantic data root.
    pub available_disk_bytes: u64,
    /// Whether the device is currently on battery.
    pub on_battery: bool,
    /// Remaining battery percentage, when reported.
    pub battery_percent: Option<u8>,
    /// Whether the host reports serious or critical thermal pressure.
    pub thermal_pressure: bool,
}

/// Injected host resource probe.
pub trait ResourceProbe: Send + Sync {
    /// Reads current resource state.
    fn state(&self) -> ResourceState;
}

/// Sanitized progress event with no excerpts, source bytes, or queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestionProgress {
    /// Stable job identity.
    pub job_id: String,
    /// Current stage.
    pub stage: IngestionStage,
    /// Completed work units.
    pub completed: u64,
    /// Total known work units.
    pub total: u64,
    /// Number of isolated failures.
    pub errors: u64,
}

/// Event sink shared by IPC/HTTP/Tauri projections.
pub trait IngestionEventSink: Send + Sync {
    /// Publishes one sanitized progress update.
    fn publish(&self, event: IngestionProgress);
}

/// Query priority controller. Active interactive queries pause new ingestion.
#[derive(Clone, Default)]
pub struct InteractivePriority {
    active_queries: Arc<AtomicUsize>,
}

impl InteractivePriority {
    /// Marks an interactive query active.
    #[must_use]
    pub fn begin_query(&self) -> InteractiveQueryGuard {
        self.active_queries.fetch_add(1, Ordering::AcqRel);
        InteractiveQueryGuard {
            priority: self.clone(),
        }
    }

    fn query_active(&self) -> bool {
        self.active_queries.load(Ordering::Acquire) != 0
    }
}

/// RAII query-priority lease.
pub struct InteractiveQueryGuard {
    priority: InteractivePriority,
}

impl Drop for InteractiveQueryGuard {
    fn drop(&mut self) {
        self.priority.active_queries.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Worker-side conversion, embedding, and atomic publication coordinator.
pub struct IngestionCoordinator {
    catalog: SemanticCatalog,
    converter: Arc<dyn DocumentConverter>,
    conversion_budgets: ConversionBudgets,
    embedder: Arc<dyn EmbeddingProvider>,
    index: Arc<dyn DerivedIndex>,
    resources: Arc<dyn ResourceProbe>,
    events: Arc<dyn IngestionEventSink>,
    priority: InteractivePriority,
    paused: AtomicBool,
    minimum_free_bytes: u64,
    maximum_attempts: u32,
}

/// Queue adapter connecting the versioned IPC feed to the durable pipeline.
pub struct PipelineIngestionBackend {
    coordinator: Arc<IngestionCoordinator>,
    library_manifest: Option<LibraryIndexManifest>,
}

impl PipelineIngestionBackend {
    /// Creates an IPC-facing queue around one configured pipeline.
    #[must_use]
    pub const fn new(coordinator: Arc<IngestionCoordinator>) -> Self {
        Self {
            coordinator,
            library_manifest: None,
        }
    }

    /// Creates a pipeline that registers each host-approved library against
    /// one immutable manifest before accepting its first document.
    #[must_use]
    pub const fn with_library_manifest(
        coordinator: Arc<IngestionCoordinator>,
        library_manifest: LibraryIndexManifest,
    ) -> Self {
        Self {
            coordinator,
            library_manifest: Some(library_manifest),
        }
    }
}

impl WorkerIngestionBackend for PipelineIngestionBackend {
    fn enqueue(
        &self,
        input: WorkerIngestionInput,
        cancellation: CancellationToken,
    ) -> Result<String, String> {
        fn required(
            metadata: &std::collections::BTreeMap<String, String>,
            key: &'static str,
        ) -> Result<String, String> {
            metadata
                .get(key)
                .filter(|value| !value.is_empty())
                .cloned()
                .ok_or_else(|| format!("ingestion metadata `{key}` is required"))
        }

        if let Some(manifest) = &self.library_manifest {
            self.coordinator
                .catalog
                .register_library(&input.tenant_id, &input.library_id, manifest)
                .map_err(|error| error.to_string())?;
        }
        let occurrence_id = required(&input.metadata, "occurrence_id")?;
        let source_id = required(&input.metadata, "source_id")?;
        let root_id = required(&input.metadata, "root_id")?;
        let modified_at_ms = input
            .metadata
            .get("modified_at_ms")
            .map_or(Ok(0), |value| {
                value
                    .parse::<i64>()
                    .map_err(|_| "ingestion metadata `modified_at_ms` is invalid".to_owned())
            })?;
        let document = IngestionDocument {
            job_id: input.job_id.clone(),
            tenant_id: input.tenant_id,
            library_id: input.library_id,
            document_id: input.document_id,
            occurrence_id,
            source_id,
            root_id,
            workspace_id: input
                .metadata
                .get("workspace_id")
                .filter(|value| !value.is_empty())
                .cloned(),
            media_type: input.media_type,
            modified_at_ms,
            bytes: input.content,
        };
        if let Some(job) = self
            .coordinator
            .catalog
            .job(&document.job_id)
            .map_err(|_| "semantic ingestion catalog unavailable".to_owned())?
        {
            if job.tenant_id.as_deref() != Some(document.tenant_id.as_str())
                || job.library_id.as_deref() != Some(document.library_id.as_str())
                || job.document_id.as_deref() != Some(document.document_id.as_str())
            {
                return Err("ingestion job identity belongs to a different scope".into());
            }
            if !matches!(job.stage.as_str(), "failed" | "paused" | "cancelled") {
                return Ok(document.job_id);
            }
        }
        self.coordinator
            .catalog
            .register_job(
                &document.job_id,
                &document.tenant_id,
                &document.library_id,
                &document.document_id,
            )
            .map_err(|_| "semantic ingestion catalog unavailable".to_owned())?;
        let coordinator = Arc::clone(&self.coordinator);
        let job_id = document.job_id.clone();
        std::thread::spawn(move || {
            let _ = coordinator.ingest(&document, &cancellation);
        });
        Ok(job_id)
    }

    fn job(
        &self,
        tenant_id: &str,
        library_id: &str,
        job_id: &str,
    ) -> Result<Option<WorkerIngestionJob>, String> {
        let Some(job) = self
            .coordinator
            .catalog
            .job(job_id)
            .map_err(|_| "semantic ingestion catalog unavailable".to_owned())?
        else {
            return Ok(None);
        };
        if job.tenant_id.as_deref() != Some(tenant_id)
            || job.library_id.as_deref() != Some(library_id)
        {
            return Ok(None);
        }
        let state = match job.stage.as_str() {
            "discovered" | "paused" => IngestionState::Pending,
            "complete" => IngestionState::Completed,
            "failed" => IngestionState::Failed,
            "cancelled" => IngestionState::Cancelled,
            "skipped" => IngestionState::Skipped,
            _ => IngestionState::Running,
        };
        let error = (state == IngestionState::Failed || state == IngestionState::Skipped)
            .then(|| job.detail.clone())
            .flatten();
        Ok(Some(WorkerIngestionJob {
            document_id: job.document_id.unwrap_or_default(),
            state,
            phase: job.stage,
            completed: u64::from(state == IngestionState::Completed),
            total: 1,
            error,
        }))
    }
}

impl IngestionCoordinator {
    /// Creates a coordinator around deterministic Docling PDF extraction,
    /// baseline fallback, and injected local boundaries.
    #[must_use]
    pub fn new(
        catalog: SemanticCatalog,
        embedder: Arc<dyn EmbeddingProvider>,
        index: Arc<dyn DerivedIndex>,
        resources: Arc<dyn ResourceProbe>,
        events: Arc<dyn IngestionEventSink>,
        priority: InteractivePriority,
    ) -> Self {
        Self::with_converter(
            catalog,
            Arc::new(converter_with_baseline_fallback()),
            embedder,
            index,
            resources,
            events,
            priority,
        )
    }

    /// Creates a coordinator with an explicitly composed converter.
    ///
    /// Hosts use this seam for opt-in capabilities such as OCR without adding
    /// format-specific branches to ingestion.
    #[must_use]
    pub fn with_converter(
        catalog: SemanticCatalog,
        converter: Arc<dyn DocumentConverter>,
        embedder: Arc<dyn EmbeddingProvider>,
        index: Arc<dyn DerivedIndex>,
        resources: Arc<dyn ResourceProbe>,
        events: Arc<dyn IngestionEventSink>,
        priority: InteractivePriority,
    ) -> Self {
        Self {
            catalog,
            converter,
            conversion_budgets: ConversionBudgets::default(),
            embedder,
            index,
            resources,
            events,
            priority,
            paused: AtomicBool::new(false),
            minimum_free_bytes: 512 * 1024 * 1024,
            maximum_attempts: 3,
        }
    }

    /// Replaces per-document conversion limits for explicitly configured
    /// capabilities such as OCR.
    #[must_use]
    pub fn with_conversion_budgets(mut self, budgets: ConversionBudgets) -> Self {
        self.conversion_budgets = budgets;
        self
    }

    /// Pauses new ingestion without affecting query visibility.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Release);
    }

    /// Resumes new ingestion.
    pub fn resume(&self) {
        self.paused.store(false, Ordering::Release);
    }

    /// Marks a failed candidate skipped.
    ///
    /// # Errors
    ///
    /// Returns a durable catalog error.
    pub fn skip(&self, job_id: &str) -> Result<(), IngestionError> {
        let job = self
            .catalog
            .job(job_id)?
            .ok_or(IngestionError::JobNotFound)?;
        if job.stage != IngestionStage::Failed.as_str() {
            return Err(IngestionError::JobNotSkippable { stage: job.stage });
        }
        self.catalog.upsert_job(
            job_id,
            IngestionStage::Skipped.as_str(),
            job.attempts,
            Some("skipped by user"),
        )?;
        self.events.publish(IngestionProgress {
            job_id: job_id.to_owned(),
            stage: IngestionStage::Skipped,
            completed: 0,
            total: 1,
            errors: 0,
        });
        Ok(())
    }

    /// Removes one source occurrence and reclaims its unreferenced derived data.
    ///
    /// Catalog deletion is authoritative: if derived cleanup fails, stale index
    /// candidates are no longer authorized as evidence and can be removed by a
    /// later index repair.
    ///
    /// # Errors
    ///
    /// Returns a durable catalog or derived-index failure.
    pub fn delete_occurrence(
        &self,
        job_id: &str,
        occurrence_id: &str,
    ) -> Result<(), IngestionError> {
        let attempts = self
            .catalog
            .job(job_id)?
            .map_or(1, |job| job.attempts.saturating_add(1));
        self.catalog
            .upsert_job(job_id, IngestionStage::Deleting.as_str(), attempts, None)?;
        self.events.publish(IngestionProgress {
            job_id: job_id.to_owned(),
            stage: IngestionStage::Deleting,
            completed: 0,
            total: 1,
            errors: 0,
        });
        let deleted = self.catalog.delete_occurrence(occurrence_id)?;
        self.index
            .delete(&deleted.record_ids)
            .map_err(IngestionError::DerivedCleanup)?;
        self.catalog
            .upsert_job(job_id, IngestionStage::Complete.as_str(), attempts, None)?;
        self.events.publish(IngestionProgress {
            job_id: job_id.to_owned(),
            stage: IngestionStage::Complete,
            completed: 1,
            total: 1,
            errors: 0,
        });
        Ok(())
    }

    /// Runs or resumes one idempotent ingestion.
    ///
    /// # Errors
    ///
    /// Returns typed cancellation, pause, conversion, embedding, storage, and
    /// derived-index failures. Failures remain isolated to this job.
    pub fn ingest(
        &self,
        document: &IngestionDocument,
        cancellation: &CancellationToken,
    ) -> Result<IngestionReceipt, IngestionError> {
        let previous_attempts = self
            .catalog
            .job(&document.job_id)?
            .map_or(0, |job| job.attempts);
        if previous_attempts >= self.maximum_attempts {
            return Err(IngestionError::RetryExhausted);
        }
        let retry_delay = retry_backoff(previous_attempts);
        let retry_started = std::time::Instant::now();
        while retry_started.elapsed() < retry_delay {
            self.check_cancelled(document, previous_attempts, cancellation)?;
            std::thread::sleep(
                retry_delay
                    .saturating_sub(retry_started.elapsed())
                    .min(std::time::Duration::from_millis(10)),
            );
        }
        let attempts = previous_attempts + 1;
        self.transition(document, IngestionStage::Discovered, attempts, None, 0, 1)?;
        self.check_preconditions(document, attempts, cancellation)?;

        self.transition(document, IngestionStage::Hashing, attempts, None, 0, 1)?;
        let content_hash = sha256_hex(&document.bytes);
        self.check_cancelled(document, attempts, cancellation)?;

        self.transition(document, IngestionStage::Converting, attempts, None, 0, 1)?;
        let metadata = DocumentMetadata::unknown()
            .with_media_type(&document.media_type)
            .with_byte_length(document.bytes.len() as u64);
        let context = ConversionContext::new()
            .with_budgets(self.conversion_budgets.clone())
            .with_cancellation(Cancellation::new(Arc::new(TokenSignal(
                cancellation.clone(),
            ))));
        let outcome = self
            .converter
            .convert(SourceContent::Bytes(&document.bytes), &metadata, &context)
            .map_err(|error| self.fail(document, attempts, error.to_string()))?;
        let converted = match outcome {
            ConversionOutcome::Converted(converted) => converted,
            ConversionOutcome::Cancelled => {
                self.transition(
                    document,
                    IngestionStage::Cancelled,
                    attempts,
                    Some("cancelled"),
                    0,
                    1,
                )?;
                return Err(IngestionError::Cancelled);
            }
            ConversionOutcome::NoTextLayer { detail } => {
                let deleted = self.catalog.delete_occurrence(&document.occurrence_id)?;
                self.index
                    .delete(&deleted.record_ids)
                    .map_err(IngestionError::DerivedCleanup)?;
                self.transition(
                    document,
                    IngestionStage::Skipped,
                    attempts,
                    Some(&detail),
                    0,
                    1,
                )?;
                return Err(IngestionError::Excluded(detail));
            }
            outcome => {
                let detail = format!("{outcome:?}");
                return Err(self.fail(document, attempts, detail));
            }
        };

        self.transition(document, IngestionStage::Chunking, attempts, None, 0, 1)?;
        let chunks = Chunker::default().chunk(&converted);
        self.check_cancelled(document, attempts, cancellation)?;

        self.transition(
            document,
            IngestionStage::Embedding,
            attempts,
            None,
            0,
            chunks.len() as u64,
        )?;
        let identity = self.embedder.identity();
        let keys = chunks
            .iter()
            .map(|chunk| {
                EmbeddingCacheKey::calculate(
                    &chunk.embedding_input,
                    identity,
                    identity.tokenizer.as_str(),
                    &fm_semantic_conversion::STRUCTURAL_CHUNKER_VERSION.to_string(),
                )
            })
            .collect::<Vec<_>>();
        let mut vectors = vec![None; chunks.len()];
        let mut missing_positions = Vec::new();
        for (position, key) in keys.iter().copied().enumerate() {
            if let Some(vector) = self.catalog.cached_vector(key)? {
                vectors[position] = Some(vector);
            } else {
                missing_positions.push(position);
            }
        }
        let embedded_count = missing_positions.len();
        let mut completed = 0_u64;
        for positions in missing_positions.chunks(EMBEDDING_CHECKPOINT_INPUTS) {
            let inputs = positions
                .iter()
                .map(|position| chunks[*position].embedding_input.clone())
                .collect::<Vec<_>>();
            let embedded = self
                .embedder
                .embed(&inputs, cancellation)
                .map_err(|error| self.fail(document, attempts, error.to_string()))?;
            let checkpoint = positions
                .iter()
                .copied()
                .zip(embedded.iter().cloned())
                .map(|(position, vector)| (keys[position], vector))
                .collect::<Vec<_>>();
            self.catalog
                .cache_vectors(&checkpoint, identity.dimensions)?;
            for (position, vector) in positions.iter().copied().zip(embedded) {
                vectors[position] = Some(vector);
            }
            completed = completed.saturating_add(positions.len() as u64);
            self.transition(
                document,
                IngestionStage::Embedding,
                attempts,
                None,
                completed,
                embedded_count as u64,
            )?;
        }
        self.check_cancelled(document, attempts, cancellation)?;

        let generation = self.catalog.resume_or_next_generation(
            &document.tenant_id,
            &document.library_id,
            &document.document_id,
        )?;
        let occurrence = Occurrence {
            occurrence_id: document.occurrence_id.clone(),
            source_id: document.source_id.clone(),
            root_id: document.root_id.clone(),
            workspace_id: document.workspace_id.clone(),
            media_type: document.media_type.clone(),
            modified_at_ms: document.modified_at_ms,
            available: true,
            provenance: "worker-conversion".into(),
        };
        let mut staged_records = Vec::with_capacity(chunks.len());
        let mut derived_records = Vec::with_capacity(chunks.len());
        for (position, ((chunk, key), vector)) in chunks.iter().zip(keys).zip(vectors).enumerate() {
            let vector = vector.ok_or(IngestionError::EmbeddingOutputMissing)?;
            let record_id = record_id(document, generation, position);
            let provenance = serde_json::to_string(&chunk.provenance)
                .map_err(|error| self.fail(document, attempts, error.to_string()))?;
            staged_records.push(StagedRecord {
                record_id: record_id.clone(),
                occurrence_id: document.occurrence_id.clone(),
                cache_key: key,
                vector: vector.clone(),
                record_kind: "chunk".into(),
                excerpt: chunk.display_excerpt.clone(),
                content: chunk.embedding_input.clone(),
                token_count: chunk.estimated_tokens,
                section_path: chunk.section_path.clone(),
                structural_role: structural_role(&chunk.section_path, chunk.source_order).into(),
                provenance,
                source_position: chunk.source_order,
                generated: false,
                concept_id: None,
            });
            derived_records.push(DerivedRecord {
                record_id,
                tenant_id: document.tenant_id.clone(),
                library_id: document.library_id.clone(),
                root_id: document.root_id.clone(),
                workspace_id: document.workspace_id.clone(),
                media_type: document.media_type.clone(),
                modified_at_ms: document.modified_at_ms,
                generation,
                embedding: vector,
                content: chunk.embedding_input.clone(),
                excerpt: chunk.display_excerpt.clone(),
            });
        }

        self.transition(
            document,
            IngestionStage::Staging,
            attempts,
            None,
            0,
            chunks.len() as u64,
        )?;
        self.catalog.stage_generation(&StagedGeneration {
            tenant_id: document.tenant_id.clone(),
            library_id: document.library_id.clone(),
            document_id: document.document_id.clone(),
            content_hash,
            generation,
            occurrences: vec![occurrence],
            records: staged_records,
        })?;

        self.transition(
            document,
            IngestionStage::Publishing,
            attempts,
            None,
            0,
            chunks.len() as u64,
        )?;
        self.index
            .upsert(&derived_records)
            .map_err(|error| self.fail(document, attempts, error))?;
        self.catalog.publish_generation(
            &document.tenant_id,
            &document.library_id,
            &document.document_id,
            generation,
        )?;
        match self.catalog.reclaim_superseded() {
            Ok(reclaimed) => {
                self.index
                    .delete(&reclaimed.record_ids)
                    .map_err(IngestionError::DerivedCleanup)?;
            }
            Err(StorageError::ReadersActive) => {}
            Err(error) => return Err(error.into()),
        }
        self.transition(
            document,
            IngestionStage::Complete,
            attempts,
            None,
            chunks.len() as u64,
            chunks.len() as u64,
        )?;
        Ok(IngestionReceipt {
            generation,
            chunks: chunks.len(),
            embedded: embedded_count,
            reused: chunks.len().saturating_sub(embedded_count),
        })
    }

    fn check_preconditions(
        &self,
        document: &IngestionDocument,
        attempts: u32,
        cancellation: &CancellationToken,
    ) -> Result<(), IngestionError> {
        self.check_cancelled(document, attempts, cancellation)?;
        let reason = if self.paused.load(Ordering::Acquire) {
            Some("paused by user")
        } else if self.priority.query_active() {
            Some("interactive query has priority")
        } else {
            let resources = self.resources.state();
            if resources.available_disk_bytes < self.minimum_free_bytes {
                Some("insufficient free disk space")
            } else if resources.thermal_pressure {
                Some("thermal pressure")
            } else if resources.on_battery
                && resources
                    .battery_percent
                    .is_some_and(|percent| percent < 15)
            {
                Some("low battery")
            } else {
                None
            }
        };
        if let Some(reason) = reason {
            self.transition(
                document,
                IngestionStage::Paused,
                attempts,
                Some(reason),
                0,
                1,
            )?;
            return Err(IngestionError::Paused(reason.to_owned()));
        }
        Ok(())
    }

    fn check_cancelled(
        &self,
        document: &IngestionDocument,
        attempts: u32,
        cancellation: &CancellationToken,
    ) -> Result<(), IngestionError> {
        if cancellation.is_cancelled() {
            self.transition(
                document,
                IngestionStage::Cancelled,
                attempts,
                Some("cancelled"),
                0,
                1,
            )?;
            return Err(IngestionError::Cancelled);
        }
        Ok(())
    }

    fn fail(&self, document: &IngestionDocument, attempts: u32, detail: String) -> IngestionError {
        let sanitized = if detail.chars().count() > 256 {
            format!("{}...", detail.chars().take(253).collect::<String>())
        } else {
            detail
        };
        match self.transition(
            document,
            IngestionStage::Failed,
            attempts,
            Some(&sanitized),
            0,
            1,
        ) {
            Ok(()) => IngestionError::PipelineFailed(sanitized),
            Err(error) => error,
        }
    }

    fn transition(
        &self,
        document: &IngestionDocument,
        stage: IngestionStage,
        attempts: u32,
        detail: Option<&str>,
        completed: u64,
        total: u64,
    ) -> Result<(), IngestionError> {
        self.catalog
            .upsert_job(&document.job_id, stage.as_str(), attempts, detail)?;
        self.events.publish(IngestionProgress {
            job_id: document.job_id.clone(),
            stage,
            completed,
            total,
            errors: u64::from(stage == IngestionStage::Failed),
        });
        Ok(())
    }
}

/// Successful generation publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestionReceipt {
    /// Published generation.
    pub generation: u64,
    /// Total chunks.
    pub chunks: usize,
    /// Cache misses embedded.
    pub embedded: usize,
    /// Cached vectors reused.
    pub reused: usize,
}

/// Isolated ingestion failure.
#[derive(Debug, thiserror::Error)]
pub enum IngestionError {
    /// Authoritative catalog failure.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// Pipeline failed and may be retried.
    #[error("semantic ingestion failed: {0}")]
    PipelineFailed(String),
    /// Work is safely paused.
    #[error("semantic ingestion paused: {0}")]
    Paused(String),
    /// Work was cancelled.
    #[error("semantic ingestion cancelled")]
    Cancelled,
    /// Source was intentionally excluded with actionable remediation.
    #[error("semantic ingestion excluded the source: {0}")]
    Excluded(String),
    /// Bounded retry count was exhausted.
    #[error("semantic ingestion retry limit exhausted")]
    RetryExhausted,
    /// The requested durable job does not exist.
    #[error("semantic ingestion job was not found")]
    JobNotFound,
    /// Only failed jobs can be explicitly skipped.
    #[error("semantic ingestion job cannot be skipped from stage {stage}")]
    JobNotSkippable {
        /// Current persisted job stage.
        stage: String,
    },
    /// Authoritative deletion succeeded but derived cleanup needs repair.
    #[error("semantic derived-index cleanup failed: {0}")]
    DerivedCleanup(String),
    /// Backend omitted an expected output.
    #[error("embedding backend omitted an output vector")]
    EmbeddingOutputMissing,
}

struct TokenSignal(CancellationToken);

impl CancellationSignal for TokenSignal {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn record_id(document: &IngestionDocument, generation: u64, position: usize) -> String {
    let mut hasher = Sha256::new();
    for part in [
        document.tenant_id.as_str(),
        document.library_id.as_str(),
        document.document_id.as_str(),
        document.occurrence_id.as_str(),
        &generation.to_string(),
        &position.to_string(),
    ] {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }

    sha256_hex(&hasher.finalize())
}

fn structural_role(section_path: &[String], source_position: u32) -> &'static str {
    let headings = section_path
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    if headings
        .iter()
        .any(|value| matches!(value.as_str(), "title" | "document title"))
    {
        "title"
    } else if headings.iter().any(|value| {
        matches!(
            value.as_str(),
            "introduction" | "intro" | "overview" | "background"
        )
    }) || source_position == 0
    {
        "introduction"
    } else if headings
        .iter()
        .any(|value| matches!(value.as_str(), "conclusion" | "conclusions" | "closing"))
    {
        "conclusion"
    } else {
        "body"
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use tempfile::tempdir;

    use super::*;
    use crate::embedding::VectorNormalization;
    use crate::semantic_storage::{
        DistanceMetric, LibraryIndexManifest, QueryFilters, VectorIndexKind, choose_index_kind,
    };
    use lopdf::{Document, Object, Stream, dictionary};

    struct FakeEmbedder {
        identity: EmbeddingModelIdentity,
        calls: Mutex<usize>,
        remaining_before_failure: Mutex<Option<usize>>,
    }

    impl EmbeddingProvider for FakeEmbedder {
        fn identity(&self) -> &EmbeddingModelIdentity {
            &self.identity
        }

        fn embed(
            &self,
            inputs: &[String],
            cancellation: &CancellationToken,
        ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            if cancellation.is_cancelled() {
                return Err(EmbeddingError::Cancelled);
            }
            let mut remaining = self.remaining_before_failure.lock().expect("failure lock");
            if let Some(allowed) = remaining.as_mut() {
                if *allowed < inputs.len() {
                    return Err(EmbeddingError::Backend("injected failure".into()));
                }
                *allowed -= inputs.len();
            }
            *self.calls.lock().expect("calls") += inputs.len();
            Ok(inputs.iter().map(|_| vec![1.0, 0.0, 0.0]).collect())
        }
    }

    #[derive(Default)]
    struct FakeIndex {
        records: Mutex<BTreeMap<String, DerivedRecord>>,
        fail_next: AtomicBool,
    }

    impl DerivedIndex for FakeIndex {
        fn upsert(&self, records: &[DerivedRecord]) -> Result<(), String> {
            if self.fail_next.swap(false, Ordering::AcqRel) {
                return Err("injected index failure".into());
            }
            self.records.lock().expect("records").extend(
                records
                    .iter()
                    .cloned()
                    .map(|record| (record.record_id.clone(), record)),
            );
            Ok(())
        }

        fn delete(&self, record_ids: &[String]) -> Result<(), String> {
            let mut records = self.records.lock().expect("records");
            for record_id in record_ids {
                records.remove(record_id);
            }
            Ok(())
        }
    }

    struct FixedResources(ResourceState);

    impl ResourceProbe for FixedResources {
        fn state(&self) -> ResourceState {
            self.0
        }
    }

    #[derive(Default)]
    struct Events(Mutex<Vec<IngestionProgress>>);

    impl IngestionEventSink for Events {
        fn publish(&self, event: IngestionProgress) {
            self.0.lock().expect("events").push(event);
        }
    }

    struct Fixture {
        _directory: tempfile::TempDir,
        catalog: SemanticCatalog,
        coordinator: Arc<IngestionCoordinator>,
        embedder: Arc<FakeEmbedder>,
        index: Arc<FakeIndex>,
    }

    impl Fixture {
        fn new(resources: ResourceState) -> Self {
            let directory = tempdir().expect("temp directory");
            let catalog =
                SemanticCatalog::open(directory.path().join("catalog.sqlite")).expect("catalog");
            catalog
                .register_library(
                    "tenant-a",
                    "library-a",
                    &LibraryIndexManifest {
                        zvec_schema_version: 1,
                        dimensions: 3,
                        distance_metric: DistanceMetric::Cosine,
                        model_revision: "revision-a".into(),
                        tokenizer: "tokenizer-a".into(),
                        converter_version: "baseline/1".into(),
                        chunker_version: "structural/2".into(),
                        normalization: VectorNormalization::L2,
                    },
                )
                .expect("library");
            let embedder = Arc::new(FakeEmbedder {
                identity: EmbeddingModelIdentity {
                    model_id: "model-a".into(),
                    model_revision: "revision-a".into(),
                    tokenizer: "tokenizer-a".into(),
                    dimensions: 3,
                    max_input_tokens: 512,
                },
                calls: Mutex::new(0),
                remaining_before_failure: Mutex::new(None),
            });
            let index = Arc::new(FakeIndex::default());
            let coordinator = Arc::new(IngestionCoordinator::new(
                catalog.clone(),
                embedder.clone(),
                index.clone(),
                Arc::new(FixedResources(resources)),
                Arc::new(Events::default()),
                InteractivePriority::default(),
            ));
            Self {
                _directory: directory,
                catalog,
                coordinator,
                embedder,
                index,
            }
        }

        fn document(&self, body: &str) -> IngestionDocument {
            IngestionDocument {
                job_id: "job-a".into(),
                tenant_id: "tenant-a".into(),
                library_id: "library-a".into(),
                document_id: "document-a".into(),
                occurrence_id: "occurrence-a".into(),
                source_id: "source-a".into(),
                root_id: "root-a".into(),
                workspace_id: Some("workspace-a".into()),
                media_type: "text/plain".into(),
                modified_at_ms: 1_000,
                bytes: body.as_bytes().to_vec(),
            }
        }

        fn visible_excerpt(&self) -> String {
            let records = self.index.records.lock().expect("records");
            let candidates = records.keys().cloned().collect::<Vec<_>>();
            let evidence = self
                .catalog
                .begin_read()
                .filter_visible_candidates(
                    &candidates,
                    &QueryFilters {
                        tenant_id: "tenant-a".into(),
                        ..QueryFilters::default()
                    },
                )
                .expect("evidence");
            let record = records.get(&evidence[0].record_id).expect("visible record");
            record.excerpt.clone()
        }
    }

    fn healthy_resources() -> ResourceState {
        ResourceState {
            available_disk_bytes: u64::MAX,
            on_battery: false,
            battery_percent: None,
            thermal_pressure: false,
        }
    }

    fn positioned_pdf(content: &str) -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content_id =
            document.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
                "Resources" => resources_id,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save positioned PDF");
        bytes
    }

    #[test]
    fn default_ingestion_uses_geometry_aware_docling_pdf_order() {
        let fixture = Fixture::new(healthy_resources());
        let mut document = fixture.document("");
        document.media_type = "application/pdf".into();
        document.bytes = positioned_pdf(
            "BT /F1 12 Tf\n\
             1 0 0 1 330 720 Tm (Right column starts after the left column.) Tj\n\
             1 0 0 1 72 720 Tm (Left column starts first in reading order.) Tj\n\
             1 0 0 1 330 690 Tm (Right column continues after left finishes.) Tj\n\
             1 0 0 1 72 690 Tm (Left column continues before the right column.) Tj\n\
             ET\n",
        );

        fixture
            .coordinator
            .ingest(&document, &CancellationToken::new())
            .expect("ingest positioned PDF");

        let excerpt = fixture.visible_excerpt();
        assert!(
            excerpt.find("Left column starts").expect("left column")
                < excerpt.find("Right column starts").expect("right column")
        );
    }

    #[test]
    fn ocr_required_pdf_is_skipped_with_actionable_status() {
        let fixture = Fixture::new(healthy_resources());
        let mut document = fixture.document("");
        document.media_type = "application/pdf".into();
        document.bytes = positioned_pdf("");
        fixture
            .catalog
            .register_job(
                &document.job_id,
                &document.tenant_id,
                &document.library_id,
                &document.document_id,
            )
            .expect("register job");

        let result = fixture
            .coordinator
            .ingest(&document, &CancellationToken::new());

        assert!(
            matches!(result, Err(IngestionError::Excluded(ref detail)) if detail.contains("OCRmyPDF"))
        );
        let backend = PipelineIngestionBackend::new(Arc::clone(&fixture.coordinator));
        let status = backend
            .job("tenant-a", "library-a", "job-a")
            .expect("job status")
            .expect("persisted job");
        assert_eq!(status.phase, "skipped");
        assert!(
            status
                .error
                .as_deref()
                .is_some_and(|detail| detail.contains("Homebrew") && detail.contains("WSL"))
        );
    }

    #[test]
    fn ocr_required_update_removes_previously_indexed_evidence() {
        let fixture = Fixture::new(healthy_resources());
        let document = fixture.document("previously searchable evidence");
        fixture
            .coordinator
            .ingest(&document, &CancellationToken::new())
            .expect("initial ingestion");
        assert!(!fixture.index.records.lock().expect("records").is_empty());

        let mut scanned = document;
        scanned.media_type = "application/pdf".into();
        scanned.bytes = positioned_pdf("");
        assert!(matches!(
            fixture
                .coordinator
                .ingest(&scanned, &CancellationToken::new()),
            Err(IngestionError::Excluded(_))
        ));

        assert!(fixture.index.records.lock().expect("records").is_empty());
    }

    #[test]
    fn changed_document_keeps_old_generation_visible_until_publication() {
        let fixture = Fixture::new(healthy_resources());
        fixture
            .coordinator
            .ingest(
                &fixture.document("first version"),
                &CancellationToken::new(),
            )
            .expect("initial ingestion");
        fixture.index.fail_next.store(true, Ordering::Release);

        let result = fixture.coordinator.ingest(
            &fixture.document("second version"),
            &CancellationToken::new(),
        );

        assert!(matches!(result, Err(IngestionError::PipelineFailed(_))));
        assert_eq!(fixture.visible_excerpt(), "first version");
        fixture
            .coordinator
            .ingest(
                &fixture.document("second version"),
                &CancellationToken::new(),
            )
            .expect("resume");
        assert_eq!(fixture.visible_excerpt(), "second version");
    }

    #[test]
    fn identical_chunk_reuses_cached_vector_after_a_move() {
        let fixture = Fixture::new(healthy_resources());
        let first = fixture.document("same content");
        fixture
            .coordinator
            .ingest(&first, &CancellationToken::new())
            .expect("first");
        let mut moved = fixture.document("same content");
        moved.job_id = "job-b".into();
        moved.document_id = "document-b".into();
        moved.occurrence_id = "occurrence-b".into();
        moved.source_id = "source-moved".into();
        let receipt = fixture
            .coordinator
            .ingest(&moved, &CancellationToken::new())
            .expect("moved");

        assert_eq!(receipt.embedded, 0);
        assert_eq!(receipt.reused, receipt.chunks);
        assert_eq!(*fixture.embedder.calls.lock().expect("calls"), 1);
    }

    #[test]
    fn localized_edit_reuses_unchanged_chunk_vectors() {
        let fixture = Fixture::new(healthy_resources());
        let stable = "stable ".repeat(500);
        let changed = "before ".repeat(500);
        let first = format!("# Stable\n\n{stable}\n\n# Changed\n\n{changed}");
        let first_receipt = fixture
            .coordinator
            .ingest(&fixture.document(&first), &CancellationToken::new())
            .expect("first");
        assert!(first_receipt.chunks > 1);

        let second = format!(
            "# Stable\n\n{stable}\n\n# Changed\n\n{}",
            "after ".repeat(500)
        );
        let mut document = fixture.document(&second);
        document.job_id = "job-local-edit".into();
        let receipt = fixture
            .coordinator
            .ingest(&document, &CancellationToken::new())
            .expect("localized edit");

        assert!(receipt.reused > 0);
        assert!(receipt.embedded > 0);
        assert!(receipt.embedded < receipt.chunks);
    }

    #[test]
    fn interrupted_embedding_reuses_durable_batch_checkpoints() {
        let fixture = Fixture::new(healthy_resources());
        let document = fixture.document(&"semantic evidence ".repeat(10_000));
        *fixture
            .embedder
            .remaining_before_failure
            .lock()
            .expect("failure lock") = Some(EMBEDDING_CHECKPOINT_INPUTS);

        assert!(matches!(
            fixture
                .coordinator
                .ingest(&document, &CancellationToken::new()),
            Err(IngestionError::PipelineFailed(_))
        ));
        assert_eq!(
            *fixture.embedder.calls.lock().expect("calls"),
            EMBEDDING_CHECKPOINT_INPUTS
        );

        *fixture
            .embedder
            .remaining_before_failure
            .lock()
            .expect("failure lock") = None;
        let receipt = fixture
            .coordinator
            .ingest(&document, &CancellationToken::new())
            .expect("resumed ingestion");

        assert!(receipt.chunks > EMBEDDING_CHECKPOINT_INPUTS);
        assert!(receipt.reused >= EMBEDDING_CHECKPOINT_INPUTS);
        assert_eq!(
            *fixture.embedder.calls.lock().expect("calls"),
            EMBEDDING_CHECKPOINT_INPUTS + receipt.embedded
        );
    }

    #[test]
    fn low_disk_and_interactive_queries_pause_before_source_processing() {
        let low_disk = Fixture::new(ResourceState {
            available_disk_bytes: 1,
            ..healthy_resources()
        });
        assert!(matches!(
            low_disk.coordinator.ingest(
                &low_disk.document("content"),
                &CancellationToken::new()
            ),
            Err(IngestionError::Paused(reason)) if reason.contains("disk")
        ));

        let fixture = Fixture::new(healthy_resources());
        let guard = fixture.coordinator.priority.begin_query();
        assert!(matches!(
            fixture.coordinator.ingest(
                &fixture.document("content"),
                &CancellationToken::new()
            ),
            Err(IngestionError::Paused(reason)) if reason.contains("query")
        ));
        drop(guard);

        for resources in [
            ResourceState {
                on_battery: true,
                battery_percent: Some(10),
                ..healthy_resources()
            },
            ResourceState {
                thermal_pressure: true,
                ..healthy_resources()
            },
        ] {
            let constrained = Fixture::new(resources);
            assert!(matches!(
                constrained
                    .coordinator
                    .ingest(&constrained.document("content"), &CancellationToken::new()),
                Err(IngestionError::Paused(_))
            ));
        }
    }

    #[test]
    fn cancelled_job_is_durable_and_emits_no_content() {
        let fixture = Fixture::new(healthy_resources());
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(matches!(
            fixture
                .coordinator
                .ingest(&fixture.document("secret content"), &cancellation),
            Err(IngestionError::Cancelled)
        ));
        assert_eq!(
            fixture
                .catalog
                .job("job-a")
                .expect("job")
                .expect("persisted")
                .stage,
            "cancelled"
        );
    }

    #[test]
    fn index_selection_remains_measurement_driven() {
        assert_eq!(choose_index_kind(100, 100), VectorIndexKind::Flat);
        assert_eq!(choose_index_kind(101, 100), VectorIndexKind::Hnsw);
    }

    #[test]
    fn failed_jobs_have_bounded_retries_and_explicit_skip() {
        let fixture = Fixture::new(healthy_resources());
        let document = fixture.document("content");
        fixture.index.fail_next.store(true, Ordering::Release);
        assert!(matches!(
            fixture
                .coordinator
                .ingest(&document, &CancellationToken::new()),
            Err(IngestionError::PipelineFailed(_))
        ));
        fixture.index.fail_next.store(true, Ordering::Release);
        assert!(matches!(
            fixture
                .coordinator
                .ingest(&document, &CancellationToken::new()),
            Err(IngestionError::PipelineFailed(_))
        ));
        fixture.index.fail_next.store(true, Ordering::Release);
        assert!(matches!(
            fixture
                .coordinator
                .ingest(&document, &CancellationToken::new()),
            Err(IngestionError::PipelineFailed(_))
        ));
        assert!(matches!(
            fixture
                .coordinator
                .ingest(&document, &CancellationToken::new()),
            Err(IngestionError::RetryExhausted)
        ));

        fixture.coordinator.skip("job-a").expect("skip failed job");
        let job = fixture
            .catalog
            .job("job-a")
            .expect("job read")
            .expect("persisted job");
        assert_eq!(job.stage, "skipped");
        assert_eq!(job.attempts, 3);
        assert!(matches!(
            fixture.coordinator.skip("job-a"),
            Err(IngestionError::JobNotSkippable { .. })
        ));
    }

    #[test]
    fn deleting_an_occurrence_removes_authoritative_and_derived_records() {
        let fixture = Fixture::new(healthy_resources());
        fixture
            .coordinator
            .ingest(
                &fixture.document("deleted content"),
                &CancellationToken::new(),
            )
            .expect("ingestion");

        fixture
            .coordinator
            .delete_occurrence("delete-job", "occurrence-a")
            .expect("deletion");

        assert!(fixture.index.records.lock().expect("records").is_empty());
        assert_eq!(
            fixture
                .catalog
                .job("delete-job")
                .expect("job")
                .expect("persisted")
                .stage,
            "complete"
        );
    }

    #[test]
    fn protocol_backend_queues_durable_scoped_jobs() {
        let fixture = Fixture::new(healthy_resources());
        let backend = PipelineIngestionBackend::new(Arc::clone(&fixture.coordinator));
        let job_id = backend
            .enqueue(
                WorkerIngestionInput {
                    job_id: "protocol-job".into(),
                    tenant_id: "tenant-a".into(),
                    library_id: "library-a".into(),
                    document_id: "protocol-document".into(),
                    media_type: "text/plain".into(),
                    metadata: [
                        ("occurrence_id".into(), "protocol-occurrence".into()),
                        ("source_id".into(), "opaque-source".into()),
                        ("root_id".into(), "root-a".into()),
                    ]
                    .into_iter()
                    .collect(),
                    content: b"protocol content".to_vec(),
                },
                CancellationToken::new(),
            )
            .expect("queue");
        assert_eq!(job_id, "protocol-job");

        let mut completed = None;
        for _ in 0..100 {
            let job = backend
                .job("tenant-a", "library-a", &job_id)
                .expect("job read")
                .expect("scoped job");
            if job.state == IngestionState::Completed {
                completed = Some(job);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(completed.expect("job completion").phase, "complete");
        assert!(
            backend
                .job("tenant-b", "library-a", &job_id)
                .expect("cross-tenant read")
                .is_none()
        );
    }

    #[test]
    fn every_interrupted_stage_replays_to_one_complete_generation() {
        for stage in [
            IngestionStage::Discovered,
            IngestionStage::Hashing,
            IngestionStage::Converting,
            IngestionStage::Chunking,
            IngestionStage::Embedding,
            IngestionStage::Staging,
            IngestionStage::Publishing,
            IngestionStage::Deleting,
            IngestionStage::Paused,
            IngestionStage::Cancelled,
            IngestionStage::Failed,
        ] {
            let fixture = Fixture::new(healthy_resources());
            fixture
                .catalog
                .upsert_job("job-a", stage.as_str(), 0, Some("interrupted"))
                .expect("interrupted stage");

            let receipt = fixture
                .coordinator
                .ingest(&fixture.document("replayed"), &CancellationToken::new())
                .expect("replay");

            assert_eq!(receipt.generation, 1, "stage {}", stage.as_str());
            assert_eq!(
                fixture
                    .catalog
                    .job("job-a")
                    .expect("job")
                    .expect("persisted")
                    .stage,
                "complete"
            );
        }
    }
}
