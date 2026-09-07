//! Concrete managed-pack definition for the audited Docling PDF Adapter.

use std::collections::BTreeSet;

use crate::{
    AdvancedCapabilityKind, AdvancedEvaluationReport, AdvancedPackDependency, AdvancedPackKind,
    AdvancedPackManifest, AdvancedPackResources,
};

/// Audited Docling release used by task 0192.
pub const DOCLING_RS_VERSION: &str = "1.36.0";
/// Audited upstream source revision.
pub const DOCLING_RS_REVISION: &str = "660b312780d919a5e29eb9386f2ff8d1a522f8a0";
/// Stable managed-pack identity.
pub const DOCLING_PDF_PACK_ID: &str = "docling-pdf";
/// Targets for which release packaging may produce separate signed artifacts.
pub const DOCLING_PDF_TARGETS: [&str; 5] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
];

/// Measured, target-specific release inputs for one Docling pack artifact.
pub struct DoclingPackRelease {
    /// Target triple whose smoke test produced this artifact.
    pub target: String,
    /// HTTPS location of the immutable assembled pack.
    pub artifact_url: String,
    /// SHA-256 of the complete pack, including its per-file inventory.
    pub artifact_sha256: String,
    /// Native and model artifacts copied into this target-specific pack.
    pub dependencies: Vec<AdvancedPackDependency>,
    /// Worker protocol compatibility.
    pub protocol_min: u32,
    /// Worker protocol compatibility.
    pub protocol_max: u32,
    /// Derived-index schema compatibility.
    pub index_schema_version: u32,
    /// Measured target-specific resource costs.
    pub resources: AdvancedPackResources,
    /// Reproducible target-specific quality and performance report.
    pub evaluation: AdvancedEvaluationReport,
}

impl DoclingPackRelease {
    /// Builds the manifest that is signed only after evaluation and platform
    /// smoke tests have supplied every field.
    #[must_use]
    pub fn into_manifest(self) -> AdvancedPackManifest {
        AdvancedPackManifest {
            schema_version: 2,
            id: DOCLING_PDF_PACK_ID.into(),
            version: DOCLING_RS_VERSION.into(),
            kind: AdvancedPackKind::Converter,
            capabilities: BTreeSet::from([
                AdvancedCapabilityKind::Ocr,
                AdvancedCapabilityKind::ComplexLayout,
                AdvancedCapabilityKind::Tables,
            ]),
            affected_formats: BTreeSet::from(["pdf".into()]),
            dependencies: self.dependencies,
            artifact_url: self.artifact_url,
            artifact_sha256: self.artifact_sha256,
            targets: BTreeSet::from([self.target]),
            protocol_min: self.protocol_min,
            protocol_max: self.protocol_max,
            index_schema_version: self.index_schema_version,
            resources: self.resources,
            evaluation: self.evaluation,
        }
    }
}
