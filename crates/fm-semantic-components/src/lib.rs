//! Managed semantic component catalog, installation, and data lifecycle engine.

mod catalog;
mod data_root;
mod deletion;
mod installer;
mod model_migration;
mod model_pack;
mod optional_pack;
mod report;
mod state;

pub use catalog::{
    ArtifactCompatibility, ArtifactId, ArtifactKind, ArtifactLocation, CatalogArtifact,
    CatalogError, CatalogManifest, ComponentDisclosure, ComponentId, ComponentResources,
    EmbeddingNormalization, InstallationOffer, LicenseInfo, LocalOnlyDisclosure, ManifestRevision,
    ModelField, ModelId, ModelIdentity, ModelManifest, ModelMetadata, ModelRevision, ProtocolRange,
    RuntimeCompatibility, Sha256Digest, SignedCatalogManifest, TargetTriple, TokenizerId,
    TrustedCatalog,
};
pub use data_root::{DataCategory, DataRootError, SemanticDataRoot};
pub use data_root::{
    DataMigrationCancellation, DataRootMigrationError, DataRootMigrationReceipt,
    IndexingController, IndexingPauseGuard, PauseError,
};
pub use deletion::{
    DeletionTarget, EnrolmentDeletionCounts, EnrolmentDeletionError, EnrolmentDeletionPlan,
    EnrolmentDeletionResult, EnrolmentId,
};
pub use installer::{
    ActivationError, ActivationProbe, ArtifactChunk, ArtifactRequest, ArtifactSource,
    ArtifactSourceError, ComponentCleanupIssue, ComponentManager, ComponentQuiescer,
    FreeSpaceError, FreeSpaceProbe, InstallEnvironment, InstallError, InstallReceipt,
    InstallationConsent, QuiesceError, UninstallError, UninstallIndexDecision, UninstallReceipt,
    WorkerPatchUpdate,
};
pub use model_migration::{
    LocalModelImport, LocalModelImportRequest, ModelImportError, ModelImportField,
};
pub use model_pack::{
    MODEL_PACK_MAGIC, ModelPack, ModelPackError, ModelPackFile, ModelPackIndex, ModelPackKind,
    ModelPackSpec, write_model_pack,
};
pub use optional_pack::{
    AdvancedCapabilityKind, AdvancedEvaluationReport, AdvancedFixture, AdvancedFixtureResult,
    AdvancedPackError, AdvancedPackKind, AdvancedPackManifest, AdvancedPackRegistry,
    AdvancedPackResources, AdvancedPackStatus, SignedAdvancedPackManifest, TrustedAdvancedPack,
};
pub use report::{
    CategoryDiskUse, ComponentLifecycleStatus, ComponentStatusEntry, SemanticDiskUse,
    SemanticStatusError, SemanticStatusReport,
};
pub use state::{
    ConfirmedModelMigration, FilesystemDurability, InstalledComponent, MigrationId,
    ModelMigrationPlan, PendingModelMigration, ReindexEstimate, ReindexReason,
    ResolvedModelSelection, SemanticState, SemanticStateError, SemanticStateStore,
};

use serde::{Deserialize, Serialize};

/// An abstract semantic quality/coverage choice independent of any concrete model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticProfile {
    /// Recommended small multilingual profile.
    CompactMultilingual,
    /// Small English-only profile.
    CompactEnglish,
    /// Larger multilingual profile prioritising retrieval quality.
    MultilingualQuality,
}

impl SemanticProfile {
    /// Returns the profile recommended by setup without selecting a model.
    #[must_use]
    pub const fn recommended() -> Self {
        Self::CompactMultilingual
    }

    /// Returns every supported abstract profile in setup order.
    #[must_use]
    pub const fn all() -> &'static [Self; 3] {
        &[
            Self::CompactMultilingual,
            Self::CompactEnglish,
            Self::MultilingualQuality,
        ]
    }

    /// Explains the profile without naming a concrete model.
    #[must_use]
    pub const fn explanation(self) -> &'static str {
        match self {
            Self::CompactMultilingual => {
                "Recommended balance of multilingual coverage, disk use, and memory use."
            }
            Self::CompactEnglish => {
                "Lower resource use for libraries whose searchable content is English."
            }
            Self::MultilingualQuality => {
                "Higher multilingual retrieval quality with greater disk and memory use."
            }
        }
    }
}
