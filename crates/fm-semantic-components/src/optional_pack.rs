//! Independently signed optional semantic pack manifests and lifecycle state.

use std::collections::{BTreeMap, BTreeSet};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const PACK_SCHEMA_VERSION: u32 = 1;
const MINIMUM_RERANKER_NDCG_GAIN: f64 = 0.02;
const MAX_TARGETS: usize = 32;
const MAX_TARGET_BYTES: usize = 128;
const MAX_URL_BYTES: usize = 2_048;
const MAX_MIGRATION_IMPACT_BYTES: usize = 4_096;

/// Independently installable advanced capability family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdvancedPackKind {
    /// OCR and layout conversion.
    Converter,
    /// CPU/GPU embedding acceleration.
    Acceleration,
    /// Local cross-encoder reranking.
    Reranker,
}

/// Capability explicitly declared by a signed pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdvancedCapabilityKind {
    /// Optical character recognition.
    Ocr,
    /// Complex page layout and reading order.
    ComplexLayout,
    /// Structured table extraction.
    Tables,
    /// Optional local image/VLM interpretation.
    ImageInterpretation,
    /// Optimized CPU execution.
    Cpu,
    /// GPU execution.
    Gpu,
    /// Cross-encoder reranking.
    CrossEncoder,
}

/// Required evaluation corpus dimensions for optional capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdvancedFixture {
    /// Multilingual retrieval.
    Multilingual,
    /// Scanned-document OCR.
    Ocr,
    /// Exact-term retrieval.
    ExactTerm,
    /// Source-code retrieval.
    Code,
    /// Structured documents and tables.
    StructuredDocument,
    /// Duplicate and boilerplate handling.
    Duplicate,
    /// End-to-end latency.
    Latency,
    /// Peak resident memory.
    Memory,
    /// Installed and derived storage.
    Storage,
}

const ADVANCED_FIXTURES: [AdvancedFixture; 9] = [
    AdvancedFixture::Multilingual,
    AdvancedFixture::Ocr,
    AdvancedFixture::ExactTerm,
    AdvancedFixture::Code,
    AdvancedFixture::StructuredDocument,
    AdvancedFixture::Duplicate,
    AdvancedFixture::Latency,
    AdvancedFixture::Memory,
    AdvancedFixture::Storage,
];

impl AdvancedFixture {
    /// Returns every required quality and performance fixture.
    #[must_use]
    pub const fn all() -> &'static [Self; 9] {
        &ADVANCED_FIXTURES
    }
}

/// Download, installed-disk, and peak-memory disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedPackResources {
    /// Signed artifact download bytes.
    pub download_bytes: u64,
    /// Estimated installed bytes.
    pub installed_bytes: u64,
    /// Estimated peak runtime memory.
    pub peak_ram_bytes: u64,
}

/// Evaluation evidence embedded in the signed manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedEvaluationReport {
    /// Stable task-0188 baseline fingerprint.
    pub baseline_fingerprint: String,
    /// Candidate capability fingerprint.
    pub candidate_fingerprint: String,
    /// Baseline binary nDCG.
    pub baseline_ndcg: f64,
    /// Candidate binary nDCG.
    pub candidate_ndcg: f64,
    /// Measured p95 latency.
    pub p95_latency_ms: u64,
    /// Measured peak resident memory.
    pub peak_memory_bytes: u64,
    /// Measured installed and derived storage.
    pub storage_bytes: u64,
    /// Comparable measurements for every required fixture dimension.
    pub fixture_results: BTreeMap<AdvancedFixture, AdvancedFixtureResult>,
    /// Explicit index/model-space migration and rollback impact.
    pub migration_impact: String,
}

/// One baseline-versus-candidate measurement covered by the pack signature.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedFixtureResult {
    /// Baseline value under the fixture's documented unit and direction.
    pub baseline: f64,
    /// Candidate value under the same fixture and environment.
    pub candidate: f64,
}

/// Versioned manifest signed independently from baseline components.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedPackManifest {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Stable pack identity.
    pub id: String,
    /// Immutable semantic version.
    pub version: String,
    /// Optional capability family.
    pub kind: AdvancedPackKind,
    /// Disclosed features.
    pub capabilities: BTreeSet<AdvancedCapabilityKind>,
    /// HTTPS artifact location.
    pub artifact_url: String,
    /// Lowercase SHA-256 artifact digest.
    pub artifact_sha256: String,
    /// Supported Rust-style target triples.
    pub targets: BTreeSet<String>,
    /// Oldest supported worker protocol.
    pub protocol_min: u32,
    /// Newest supported worker protocol.
    pub protocol_max: u32,
    /// Compatible derived-index schema.
    pub index_schema_version: u32,
    /// Resource disclosure.
    pub resources: AdvancedPackResources,
    /// Quality and performance evidence.
    pub evaluation: AdvancedEvaluationReport,
}

impl AdvancedPackManifest {
    /// Returns deterministic RFC 8785 bytes covered by the signature.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, AdvancedPackError> {
        serde_json_canonicalizer::to_vec(self)
            .map_err(|error| AdvancedPackError::Serialization(error.to_string()))
    }

    fn validate(
        &self,
        target: &str,
        protocol: u32,
        index_schema: u32,
    ) -> Result<(), AdvancedPackError> {
        if self.schema_version != PACK_SCHEMA_VERSION {
            return Err(AdvancedPackError::UnsupportedSchema(self.schema_version));
        }
        if !safe_id(&self.id)
            || Version::parse(&self.version).is_err()
            || !is_sha256(&self.artifact_sha256)
            || self.resources.download_bytes == 0
            || self.resources.installed_bytes == 0
            || self.resources.peak_ram_bytes == 0
            || self.protocol_min == 0
            || self.protocol_min > self.protocol_max
            || self.index_schema_version == 0
            || self.targets.is_empty()
            || self.targets.len() > MAX_TARGETS
            || self.targets.iter().any(|target| {
                target.is_empty()
                    || target.len() > MAX_TARGET_BYTES
                    || !target.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            })
        {
            return Err(AdvancedPackError::InvalidManifest);
        }
        if self.artifact_url.len() > MAX_URL_BYTES {
            return Err(AdvancedPackError::InvalidManifest);
        }
        let url =
            url::Url::parse(&self.artifact_url).map_err(|_| AdvancedPackError::InvalidManifest)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(AdvancedPackError::InvalidManifest);
        }
        if !self.targets.contains(target)
            || protocol < self.protocol_min
            || protocol > self.protocol_max
            || self.index_schema_version != index_schema
        {
            return Err(AdvancedPackError::Incompatible);
        }
        validate_capabilities(self.kind, &self.capabilities)?;
        self.evaluation.validate(self.kind)
    }
}

impl AdvancedEvaluationReport {
    fn validate(&self, kind: AdvancedPackKind) -> Result<(), AdvancedPackError> {
        if !safe_id(&self.baseline_fingerprint)
            || !safe_id(&self.candidate_fingerprint)
            || self.baseline_fingerprint == self.candidate_fingerprint
            || self.migration_impact.trim().is_empty()
            || self.migration_impact.len() > MAX_MIGRATION_IMPACT_BYTES
            || !self.baseline_ndcg.is_finite()
            || !self.candidate_ndcg.is_finite()
            || !(0.0..=1.0).contains(&self.baseline_ndcg)
            || !(0.0..=1.0).contains(&self.candidate_ndcg)
            || self.p95_latency_ms == 0
            || self.peak_memory_bytes == 0
            || self.storage_bytes == 0
        {
            return Err(AdvancedPackError::IncompleteEvaluation);
        }
        if self.fixture_results.len() != AdvancedFixture::all().len()
            || !AdvancedFixture::all().iter().all(|fixture| {
                self.fixture_results.get(fixture).is_some_and(|result| {
                    result.baseline.is_finite()
                        && result.baseline >= 0.0
                        && result.candidate.is_finite()
                        && result.candidate >= 0.0
                })
            })
        {
            return Err(AdvancedPackError::IncompleteEvaluation);
        }
        if kind == AdvancedPackKind::Reranker
            && self.candidate_ndcg - self.baseline_ndcg < MINIMUM_RERANKER_NDCG_GAIN
        {
            return Err(AdvancedPackError::InsufficientQualityGain);
        }
        Ok(())
    }
}

fn validate_capabilities(
    kind: AdvancedPackKind,
    capabilities: &BTreeSet<AdvancedCapabilityKind>,
) -> Result<(), AdvancedPackError> {
    let (required, allowed): (&[AdvancedCapabilityKind], &[AdvancedCapabilityKind]) = match kind {
        AdvancedPackKind::Converter => (
            &[
                AdvancedCapabilityKind::Ocr,
                AdvancedCapabilityKind::ComplexLayout,
                AdvancedCapabilityKind::Tables,
            ],
            &[
                AdvancedCapabilityKind::Ocr,
                AdvancedCapabilityKind::ComplexLayout,
                AdvancedCapabilityKind::Tables,
                AdvancedCapabilityKind::ImageInterpretation,
            ],
        ),
        AdvancedPackKind::Acceleration => (
            &[],
            &[AdvancedCapabilityKind::Cpu, AdvancedCapabilityKind::Gpu],
        ),
        AdvancedPackKind::Reranker => (
            &[AdvancedCapabilityKind::CrossEncoder],
            &[AdvancedCapabilityKind::CrossEncoder],
        ),
    };
    let complete = required
        .iter()
        .all(|capability| capabilities.contains(capability))
        && capabilities
            .iter()
            .all(|capability| allowed.contains(capability))
        && !(kind == AdvancedPackKind::Acceleration && capabilities.is_empty());
    if complete {
        Ok(())
    } else {
        Err(AdvancedPackError::InvalidManifest)
    }
}

/// Manifest plus detached Ed25519 signature.
#[derive(Debug, Clone)]
pub struct SignedAdvancedPackManifest {
    manifest: AdvancedPackManifest,
    signature: [u8; 64],
}

impl SignedAdvancedPackManifest {
    /// Associates one manifest with its detached signature.
    #[must_use]
    pub const fn new(manifest: AdvancedPackManifest, signature: [u8; 64]) -> Self {
        Self {
            manifest,
            signature,
        }
    }
}

/// Signature- and compatibility-verified optional pack.
#[derive(Debug, Clone)]
pub struct TrustedAdvancedPack {
    manifest: AdvancedPackManifest,
}

impl TrustedAdvancedPack {
    /// Verifies signature and compatibility before exposing a pack.
    pub fn verify(
        signed: SignedAdvancedPackManifest,
        key: &VerifyingKey,
        target: &str,
        protocol: u32,
        index_schema: u32,
    ) -> Result<Self, AdvancedPackError> {
        let bytes = signed.manifest.canonical_bytes()?;
        key.verify(&bytes, &Signature::from_bytes(&signed.signature))
            .map_err(|_| AdvancedPackError::InvalidSignature)?;
        signed.manifest.validate(target, protocol, index_schema)?;
        Ok(Self {
            manifest: signed.manifest,
        })
    }

    /// Verifies downloaded bytes before installation.
    pub fn verify_payload(&self, payload: &[u8]) -> Result<(), AdvancedPackError> {
        let digest = hex_sha256(payload);
        if digest == self.manifest.artifact_sha256 {
            Ok(())
        } else {
            Err(AdvancedPackError::PayloadChecksum)
        }
    }

    /// Enforces administrator allow-list policy for server deployment.
    pub fn ensure_server_allowed(
        &self,
        allowed: &BTreeSet<AdvancedPackKind>,
    ) -> Result<(), AdvancedPackError> {
        if allowed.contains(&self.manifest.kind) {
            Ok(())
        } else {
            Err(AdvancedPackError::ServerPolicyDenied)
        }
    }

    /// Returns the verified manifest.
    #[must_use]
    pub const fn manifest(&self) -> &AdvancedPackManifest {
        &self.manifest
    }
}

/// Installed/active/rollback state for one optional capability kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvancedPackStatus {
    /// Installed pack identities.
    pub installed_ids: Vec<String>,
    /// Active pack identity.
    pub active_id: Option<String>,
    /// Single retained rollback identity.
    pub rollback_id: Option<String>,
}

/// In-memory lifecycle state used behind a durable host adapter.
#[derive(Default)]
pub struct AdvancedPackRegistry {
    installed: BTreeMap<String, TrustedAdvancedPack>,
    active: BTreeMap<AdvancedPackKind, String>,
    rollback: BTreeMap<AdvancedPackKind, String>,
}

impl AdvancedPackRegistry {
    /// Creates an empty optional-pack registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one independently verified payload without activating it.
    pub fn install(
        &mut self,
        pack: TrustedAdvancedPack,
        payload: &[u8],
    ) -> Result<(), AdvancedPackError> {
        pack.verify_payload(payload)?;
        if self.installed.contains_key(&pack.manifest.id) {
            return Err(AdvancedPackError::AlreadyInstalled);
        }
        self.installed.insert(pack.manifest.id.clone(), pack);
        Ok(())
    }

    /// Atomically selects an installed pack while retaining one rollback.
    pub fn activate(&mut self, id: &str) -> Result<(), AdvancedPackError> {
        let kind = self
            .installed
            .get(id)
            .map(|pack| pack.manifest.kind)
            .ok_or(AdvancedPackError::NotInstalled)?;
        if let Some(previous) = self.active.insert(kind, id.to_owned())
            && previous != id
        {
            self.rollback.insert(kind, previous);
        }
        Ok(())
    }

    /// Restores the previously active pack after startup/runtime failure.
    pub fn rollback(&mut self, kind: AdvancedPackKind) -> Result<(), AdvancedPackError> {
        let previous = self
            .rollback
            .remove(&kind)
            .ok_or(AdvancedPackError::NoRollback)?;
        let active = self
            .active
            .insert(kind, previous)
            .ok_or(AdvancedPackError::NoRollback)?;
        self.rollback.insert(kind, active);
        Ok(())
    }

    /// Removes every installed artifact for one optional family.
    pub fn remove(&mut self, kind: AdvancedPackKind) -> Result<(), AdvancedPackError> {
        let ids = self
            .installed
            .iter()
            .filter(|(_, pack)| pack.manifest.kind == kind)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Err(AdvancedPackError::NotInstalled);
        }
        for id in ids {
            self.installed.remove(&id);
        }
        self.active.remove(&kind);
        self.rollback.remove(&kind);
        Ok(())
    }

    /// Returns deterministic diagnostic state without paths or URLs.
    #[must_use]
    pub fn status(&self, kind: AdvancedPackKind) -> AdvancedPackStatus {
        AdvancedPackStatus {
            installed_ids: self
                .installed
                .iter()
                .filter(|(_, pack)| pack.manifest.kind == kind)
                .map(|(id, _)| id.clone())
                .collect(),
            active_id: self.active.get(&kind).cloned(),
            rollback_id: self.rollback.get(&kind).cloned(),
        }
    }
}

fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-._:".contains(character))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Optional-pack verification, policy, or lifecycle failure.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdvancedPackError {
    /// Manifest encoding failed.
    #[error("advanced pack manifest serialization failed: {0}")]
    Serialization(String),
    /// The detached signature is invalid.
    #[error("advanced pack signature is invalid")]
    InvalidSignature,
    /// The manifest has invalid identities, fields, capabilities, or resources.
    #[error("advanced pack manifest is invalid")]
    InvalidManifest,
    /// The manifest schema is unsupported.
    #[error("advanced pack schema version {0} is unsupported")]
    UnsupportedSchema(u32),
    /// Target, protocol, or index schema is incompatible.
    #[error("advanced pack is incompatible with this environment")]
    Incompatible,
    /// Required quality/performance fixtures or impacts are absent.
    #[error("advanced pack evaluation evidence is incomplete")]
    IncompleteEvaluation,
    /// A reranker did not materially improve the baseline.
    #[error("advanced reranker did not meet the minimum nDCG gain")]
    InsufficientQualityGain,
    /// Downloaded bytes did not match the signed digest.
    #[error("advanced pack payload checksum is invalid")]
    PayloadChecksum,
    /// Server policy does not permit this capability family.
    #[error("advanced pack is denied by server administrator policy")]
    ServerPolicyDenied,
    /// The same pack is already installed.
    #[error("advanced pack is already installed")]
    AlreadyInstalled,
    /// The requested pack is not installed.
    #[error("advanced pack is not installed")]
    NotInstalled,
    /// No previous working pack is retained.
    #[error("advanced pack has no rollback candidate")]
    NoRollback,
}
