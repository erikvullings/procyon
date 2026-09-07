//! Release-gate policies for semantic storage, diagnostics, and backups.

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use fm_semantic_library::DeletionPlanStatus;

const BACKUP_SCHEMA_VERSION: u32 = 1;
const MAX_BACKUP_ENTRIES: usize = 64;
const MAX_BACKUP_BYTES: usize = 64 * 1024 * 1024;
const MAX_BACKUP_DOCUMENT_BYTES: usize = 256 * 1024 * 1024;
const MAX_CAPTURE_DURATION: Duration = Duration::from_secs(15 * 60);
const BACKUP_PLAINTEXT_WARNING: &str =
    "This export contains sensitive semantic settings and retained conversation data in plaintext.";

/// Storage admission result used before desktop enrolment or component installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageAdmission {
    /// Estimated bytes the requested operation will add.
    pub estimated_bytes: u64,
    /// Current free bytes at the semantic data root.
    pub available_bytes: u64,
    /// Configured bytes that must remain free.
    pub reserve_bytes: u64,
}

impl StorageAdmission {
    /// Returns the projected remaining free bytes, or rejects the operation.
    pub fn validate(self) -> Result<u64, HardeningError> {
        let required = self
            .estimated_bytes
            .checked_add(self.reserve_bytes)
            .ok_or(HardeningError::StorageEstimateOverflow)?;
        if required > self.available_bytes {
            return Err(HardeningError::InsufficientFreeSpace {
                required,
                available: self.available_bytes,
            });
        }
        Ok(self.available_bytes - required)
    }
}

/// Authoritative semantic storage measured for one root and source format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticUsageSample {
    /// Stable enrolled-root identity.
    pub root_id: String,
    /// Stable detected-format identity.
    pub format: String,
    /// Authoritatively measured source bytes.
    pub source_bytes: u64,
    /// Authoritatively measured normalized-text bytes.
    pub extracted_bytes: u64,
    /// Authoritatively measured vector bytes.
    pub vector_bytes: u64,
    /// Whether these bytes belong to a pending cleanup.
    pub cleanup_pending: bool,
}

/// Storage totals split between active data and data pending cleanup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SemanticUsageBucket {
    /// Bytes retained by active enrolment.
    pub active_bytes: u64,
    /// Bytes awaiting resumable cleanup.
    pub cleanup_bytes: u64,
}

/// Deterministic storage report suitable for root and format diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticUsageReport {
    /// Usage keyed by stable root identity.
    pub by_root: BTreeMap<String, SemanticUsageBucket>,
    /// Usage keyed by detected format.
    pub by_format: BTreeMap<String, SemanticUsageBucket>,
}

/// Aggregates authoritative measurements without accepting caller-supplied totals.
pub fn semantic_usage_report(
    samples: impl IntoIterator<Item = SemanticUsageSample>,
) -> Result<SemanticUsageReport, HardeningError> {
    let mut by_root = BTreeMap::new();
    let mut by_format = BTreeMap::new();
    for sample in samples {
        if !is_safe_identifier(&sample.root_id) || !is_safe_identifier(&sample.format) {
            return Err(HardeningError::InvalidUsageDimension);
        }
        let bytes = sample
            .source_bytes
            .checked_add(sample.extracted_bytes)
            .and_then(|total| total.checked_add(sample.vector_bytes))
            .ok_or(HardeningError::StorageEstimateOverflow)?;
        add_usage(&mut by_root, sample.root_id, bytes, sample.cleanup_pending)?;
        add_usage(&mut by_format, sample.format, bytes, sample.cleanup_pending)?;
    }
    Ok(SemanticUsageReport { by_root, by_format })
}

fn add_usage(
    buckets: &mut BTreeMap<String, SemanticUsageBucket>,
    key: String,
    bytes: u64,
    cleanup_pending: bool,
) -> Result<(), HardeningError> {
    let bucket = buckets.entry(key).or_default();
    let target = if cleanup_pending {
        &mut bucket.cleanup_bytes
    } else {
        &mut bucket.active_bytes
    };
    *target = target
        .checked_add(bytes)
        .ok_or(HardeningError::StorageEstimateOverflow)?;
    Ok(())
}

/// Durable proof that catalog cleanup, transient backups, and in-flight work are gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticDeletionProof {
    /// Temporary backup and snapshot records still inside application authority.
    pub backup_snapshot_items: u64,
    /// Jobs still capable of publishing data for the excluded scope.
    pub in_flight_jobs: u64,
}

impl SemanticDeletionProof {
    /// Accepts the proof only after catalog cleanup and external work reach zero.
    pub fn verify(self, catalog_status: DeletionPlanStatus) -> Result<(), HardeningError> {
        if catalog_status != DeletionPlanStatus::Complete
            || self.backup_snapshot_items != 0
            || self.in_flight_jobs != 0
        {
            return Err(HardeningError::IncompleteDeletionProof);
        }
        Ok(())
    }
}

/// Explicitly previewed, short-lived authorization for sensitive diagnostic capture.
#[derive(Debug, Clone)]
pub struct DiagnosticCaptureGrant {
    issued_at: SystemTime,
    expires_at: SystemTime,
    categories: Vec<String>,
}

impl DiagnosticCaptureGrant {
    /// Issues a bounded grant for explicitly previewed safe category identifiers.
    pub fn issue(
        now: SystemTime,
        duration: Duration,
        mut categories: Vec<String>,
    ) -> Result<Self, HardeningError> {
        if duration.is_zero() || duration > MAX_CAPTURE_DURATION || categories.is_empty() {
            return Err(HardeningError::InvalidDiagnosticGrant);
        }
        categories.sort();
        categories.dedup();
        if categories
            .iter()
            .any(|category| !is_safe_identifier(category))
        {
            return Err(HardeningError::InvalidDiagnosticGrant);
        }
        let expires_at = now
            .checked_add(duration)
            .ok_or(HardeningError::InvalidDiagnosticGrant)?;
        Ok(Self {
            issued_at: now,
            expires_at,
            categories,
        })
    }

    /// Returns the exact scope, expiry, and warning to show before capture.
    pub fn preview(&self) -> DiagnosticCapturePreview {
        DiagnosticCapturePreview {
            issued_at: self.issued_at,
            expires_at: self.expires_at,
            categories: self.categories.clone(),
            warning: "Diagnostic capture may contain sensitive queries, excerpts, filenames, prompts, or responses."
                .into(),
        }
    }

    /// Reports whether the grant currently authorizes one category.
    pub fn authorize(&self, now: SystemTime, category: &str) -> bool {
        now >= self.issued_at
            && now < self.expires_at
            && self.categories.iter().any(|allowed| allowed == category)
    }
}

/// User-visible disclosure for a pending sensitive diagnostic capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticCapturePreview {
    /// Time from which the grant is valid.
    pub issued_at: SystemTime,
    /// Exclusive grant expiry.
    pub expires_at: SystemTime,
    /// Explicitly authorized capture categories.
    pub categories: Vec<String>,
    /// Sensitive-data disclosure shown before capture.
    pub warning: String,
}

/// Default-safe semantic diagnostic event. Its type has no free-text content field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticDiagnosticEvent {
    /// Opaque operation identity.
    pub operation_id: String,
    /// Stable processing-stage identity.
    pub stage: String,
    /// Observed stage duration.
    pub duration_ms: u64,
    /// Number of items processed.
    pub item_count: u64,
    /// Optional component identity.
    pub component_id: Option<String>,
    /// Optional model identity.
    pub model_id: Option<String>,
    /// Optional credential-free generation-profile identity.
    pub profile_id: Option<String>,
    /// Optional redacted error category.
    pub error_category: Option<String>,
}

impl SemanticDiagnosticEvent {
    /// Rejects path-like or free-text diagnostic fields.
    pub fn validate(&self) -> Result<(), HardeningError> {
        for value in [
            Some(self.operation_id.as_str()),
            Some(self.stage.as_str()),
            self.component_id.as_deref(),
            self.model_id.as_deref(),
            self.profile_id.as_deref(),
            self.error_category.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !is_safe_identifier(value) {
                return Err(HardeningError::UnsafeDiagnosticField);
            }
        }
        Ok(())
    }
}

/// One authoritative backup member and its checksum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticBackupEntry {
    /// Stable flat member name.
    pub name: String,
    /// Lowercase SHA-256 checksum of the payload.
    pub sha256: String,
    /// Authoritative member payload.
    pub bytes: Vec<u8>,
}

/// Versioned whole-library export. Rebuildable vectors/chunks are intentionally excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticBackup {
    /// Whole-library export schema version.
    pub schema_version: u32,
    /// Required warning that content is not encrypted by this format.
    pub plaintext_warning: String,
    /// Bounded authoritative members; rebuildable data is excluded by callers.
    pub entries: Vec<SemanticBackupEntry>,
}

impl SemanticBackup {
    /// Creates a validated export from authoritative, credential-free members.
    pub fn create(
        authoritative_entries: BTreeMap<String, Vec<u8>>,
    ) -> Result<Self, HardeningError> {
        if authoritative_entries.is_empty() {
            return Err(HardeningError::EmptyBackup);
        }
        if authoritative_entries.len() > MAX_BACKUP_ENTRIES
            || authoritative_entries
                .values()
                .try_fold(0_usize, |total, bytes| total.checked_add(bytes.len()))
                .is_none_or(|total| total > MAX_BACKUP_BYTES)
        {
            return Err(HardeningError::BackupTooLarge);
        }
        let mut entries = Vec::with_capacity(authoritative_entries.len());
        for (name, bytes) in authoritative_entries {
            if !is_backup_name(&name) {
                return Err(HardeningError::InvalidBackupEntry(name));
            }
            entries.push(SemanticBackupEntry {
                name,
                sha256: hex_sha256(&bytes),
                bytes,
            });
        }
        Ok(Self {
            schema_version: BACKUP_SCHEMA_VERSION,
            plaintext_warning: BACKUP_PLAINTEXT_WARNING.into(),
            entries,
        })
    }

    /// Validates version, warning, bounds, names, uniqueness, and checksums.
    pub fn validate(&self) -> Result<(), HardeningError> {
        if self.schema_version != BACKUP_SCHEMA_VERSION {
            return Err(HardeningError::UnsupportedBackupVersion(
                self.schema_version,
            ));
        }
        if self.plaintext_warning != BACKUP_PLAINTEXT_WARNING || self.entries.is_empty() {
            return Err(HardeningError::EmptyBackup);
        }
        if self.entries.len() > MAX_BACKUP_ENTRIES
            || self
                .entries
                .iter()
                .try_fold(0_usize, |total, entry| total.checked_add(entry.bytes.len()))
                .is_none_or(|total| total > MAX_BACKUP_BYTES)
        {
            return Err(HardeningError::BackupTooLarge);
        }
        let mut names = std::collections::BTreeSet::new();
        for entry in &self.entries {
            if !is_backup_name(&entry.name) {
                return Err(HardeningError::InvalidBackupEntry(entry.name.clone()));
            }
            if !names.insert(entry.name.as_str()) {
                return Err(HardeningError::DuplicateBackupEntry(entry.name.clone()));
            }
            if hex_sha256(&entry.bytes) != entry.sha256 {
                return Err(HardeningError::BackupChecksumMismatch(entry.name.clone()));
            }
        }
        Ok(())
    }

    /// Encodes a validated export document for an explicit caller-owned write.
    pub fn encode_json(&self) -> Result<Vec<u8>, HardeningError> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| HardeningError::BackupJson(error.to_string()))?;
        if bytes.len() > MAX_BACKUP_DOCUMENT_BYTES {
            return Err(HardeningError::BackupTooLarge);
        }
        Ok(bytes)
    }

    /// Decodes and fully validates an import before any authoritative state is applied.
    pub fn decode_json(bytes: &[u8]) -> Result<Self, HardeningError> {
        if bytes.len() > MAX_BACKUP_DOCUMENT_BYTES {
            return Err(HardeningError::BackupTooLarge);
        }
        let backup: Self = serde_json::from_slice(bytes)
            .map_err(|error| HardeningError::BackupJson(error.to_string()))?;
        backup.validate()?;
        Ok(backup)
    }
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-._:".contains(character))
}

fn is_backup_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.contains('/')
        && !value.contains('\\')
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-._".contains(character))
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Debug, Error, PartialEq, Eq)]
/// Failure while applying semantic release-gate policy.
pub enum HardeningError {
    /// Summing projected or measured storage exceeded `u64`.
    #[error("semantic storage estimate overflowed")]
    StorageEstimateOverflow,
    /// A root or format grouping key was unsafe.
    #[error("semantic usage has an invalid root or format identity")]
    InvalidUsageDimension,
    /// At least one deletion authority still reports retained or publishable data.
    #[error("semantic deletion proof still has catalog, backup, snapshot, or in-flight work")]
    IncompleteDeletionProof,
    /// The operation would consume the configured free-space reserve.
    #[error("semantic storage requires {required} bytes but only {available} are available")]
    InsufficientFreeSpace {
        /// Estimated operation bytes plus the configured reserve.
        required: u64,
        /// Current free bytes at the semantic data root.
        available: u64,
    },
    /// A capture request lacked a valid duration or safe category.
    #[error("diagnostic capture grant is invalid")]
    InvalidDiagnosticGrant,
    /// Default-safe diagnostics contained free text or a path-like value.
    #[error("default semantic diagnostic fields must be opaque identifiers")]
    UnsafeDiagnosticField,
    /// An export contained no data or omitted the required plaintext warning.
    #[error("semantic backup contains no authoritative data")]
    EmptyBackup,
    /// An export exceeded its bounded entry or payload size.
    #[error("semantic backup exceeds its entry or byte limit")]
    BackupTooLarge,
    /// An export member name was unsafe.
    #[error("semantic backup entry `{0}` is invalid")]
    InvalidBackupEntry(String),
    /// An imported export repeated a member name.
    #[error("semantic backup entry `{0}` occurs more than once")]
    DuplicateBackupEntry(String),
    /// An imported export uses an unsupported schema.
    #[error("semantic backup schema version `{0}` is unsupported")]
    UnsupportedBackupVersion(u32),
    /// An imported member did not match its recorded digest.
    #[error("semantic backup checksum failed for `{0}`")]
    BackupChecksumMismatch(String),
    /// Export JSON could not be parsed or encoded.
    #[error("semantic backup JSON is invalid: {0}")]
    BackupJson(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_admission_preserves_the_configured_reserve() {
        assert_eq!(
            StorageAdmission {
                estimated_bytes: 400,
                available_bytes: 1_000,
                reserve_bytes: 500,
            }
            .validate(),
            Ok(100)
        );
        assert!(matches!(
            StorageAdmission {
                estimated_bytes: 600,
                available_bytes: 1_000,
                reserve_bytes: 500,
            }
            .validate(),
            Err(HardeningError::InsufficientFreeSpace { .. })
        ));
    }

    #[test]
    fn usage_report_aggregates_unsorted_roots_formats_and_cleanup() {
        let report = semantic_usage_report([
            SemanticUsageSample {
                root_id: "root-b".into(),
                format: "pdf".into(),
                source_bytes: 10,
                extracted_bytes: 5,
                vector_bytes: 2,
                cleanup_pending: false,
            },
            SemanticUsageSample {
                root_id: "root-a".into(),
                format: "docx".into(),
                source_bytes: 20,
                extracted_bytes: 4,
                vector_bytes: 1,
                cleanup_pending: true,
            },
            SemanticUsageSample {
                root_id: "root-a".into(),
                format: "pdf".into(),
                source_bytes: 30,
                extracted_bytes: 6,
                vector_bytes: 3,
                cleanup_pending: false,
            },
        ])
        .expect("usage report");

        assert_eq!(
            report.by_root["root-a"],
            SemanticUsageBucket {
                active_bytes: 39,
                cleanup_bytes: 25,
            }
        );
        assert_eq!(
            report.by_format["pdf"],
            SemanticUsageBucket {
                active_bytes: 56,
                cleanup_bytes: 0,
            }
        );
    }

    #[test]
    fn deletion_proof_requires_catalog_backups_and_jobs_to_be_complete() {
        SemanticDeletionProof {
            backup_snapshot_items: 0,
            in_flight_jobs: 0,
        }
        .verify(DeletionPlanStatus::Complete)
        .expect("complete proof");
        assert!(matches!(
            SemanticDeletionProof {
                backup_snapshot_items: 1,
                in_flight_jobs: 0,
            }
            .verify(DeletionPlanStatus::Complete),
            Err(HardeningError::IncompleteDeletionProof)
        ));
        assert!(matches!(
            SemanticDeletionProof {
                backup_snapshot_items: 0,
                in_flight_jobs: 0,
            }
            .verify(DeletionPlanStatus::Running),
            Err(HardeningError::IncompleteDeletionProof)
        ));
    }

    #[test]
    fn diagnostic_capture_is_previewed_scoped_and_expires() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let grant = DiagnosticCaptureGrant::issue(
            now,
            Duration::from_secs(60),
            vec!["query".into(), "prompt".into()],
        )
        .expect("grant");
        assert!(grant.preview().warning.contains("sensitive"));
        assert!(grant.authorize(now + Duration::from_secs(59), "query"));
        assert!(!grant.authorize(now + Duration::from_secs(60), "query"));
        assert!(!grant.authorize(now, "headers"));
    }

    #[test]
    fn default_diagnostics_reject_paths_queries_and_headers() {
        let safe = SemanticDiagnosticEvent {
            operation_id: "operation-123".into(),
            stage: "embedding".into(),
            duration_ms: 42,
            item_count: 3,
            component_id: Some("runtime-1".into()),
            model_id: Some("model-v1".into()),
            profile_id: None,
            error_category: Some("resource_limit".into()),
        };
        safe.validate().expect("safe event");
        let serialized = serde_json::to_string(&safe).expect("serialize event");
        for forbidden in [
            "query", "excerpt", "filename", "prompt", "response", "headers", "body",
        ] {
            assert!(!serialized.contains(&format!("\"{forbidden}\"")));
        }
        assert!(matches!(
            SemanticDiagnosticEvent {
                stage: "/Users/alice/private.txt".into(),
                ..safe
            }
            .validate(),
            Err(HardeningError::UnsafeDiagnosticField)
        ));
    }

    #[test]
    fn backup_is_versioned_warns_about_plaintext_and_detects_corruption() {
        let backup = SemanticBackup::create(BTreeMap::from([
            ("library-policy.json".into(), b"policy".to_vec()),
            ("vocabularies.json".into(), b"vocabulary".to_vec()),
            ("profiles.json".into(), b"metadata-without-secrets".to_vec()),
            ("conversations.json".into(), b"pins".to_vec()),
        ]))
        .expect("backup");
        backup.validate().expect("valid checksums");
        assert!(backup.plaintext_warning.contains("plaintext"));
        assert_eq!(
            SemanticBackup::decode_json(&backup.encode_json().expect("encode")).expect("decode"),
            backup
        );
        let mut corrupted = backup;
        corrupted.entries[0].bytes.push(0);
        assert!(matches!(
            corrupted.validate(),
            Err(HardeningError::BackupChecksumMismatch(_))
        ));
    }

    #[test]
    fn backup_rejects_unsafe_duplicate_and_oversized_entries() {
        assert!(matches!(
            SemanticBackup::create(BTreeMap::from([(
                "../settings.json".into(),
                b"settings".to_vec()
            )])),
            Err(HardeningError::InvalidBackupEntry(_))
        ));

        let mut duplicate = SemanticBackup::create(BTreeMap::from([(
            "settings.json".into(),
            b"settings".to_vec(),
        )]))
        .expect("backup");
        duplicate.entries.push(duplicate.entries[0].clone());
        assert!(matches!(
            duplicate.validate(),
            Err(HardeningError::DuplicateBackupEntry(_))
        ));

        assert!(matches!(
            SemanticBackup::create(BTreeMap::from([(
                "oversized.bin".into(),
                vec![0; MAX_BACKUP_BYTES + 1],
            )])),
            Err(HardeningError::BackupTooLarge)
        ));
    }
}
