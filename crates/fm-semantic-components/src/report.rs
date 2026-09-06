use std::fs;
use std::path::{Path, PathBuf};

use semver::Version;
use thiserror::Error;

use crate::{
    ArtifactId, ArtifactKind, ComponentId, DataCategory, InstalledComponent, SemanticState,
    SemanticStateError,
};

/// Whether an installed component is active or retained for rollback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ComponentLifecycleStatus {
    /// Component currently selected by durable state.
    Active,
    /// Inactive component retained for rollback.
    Rollback,
}

/// Status and actual installed size of one component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatusEntry {
    artifact_id: ArtifactId,
    component_id: ComponentId,
    kind: ArtifactKind,
    version: Version,
    status: ComponentLifecycleStatus,
    installed_bytes: u64,
}

impl ComponentStatusEntry {
    /// Returns the exact installed artifact.
    #[must_use]
    pub const fn artifact_id(&self) -> &ArtifactId {
        &self.artifact_id
    }

    /// Returns the logical component identifier.
    #[must_use]
    pub const fn component_id(&self) -> &ComponentId {
        &self.component_id
    }

    /// Returns the component role.
    #[must_use]
    pub const fn kind(&self) -> &ArtifactKind {
        &self.kind
    }

    /// Returns the exact installed version.
    #[must_use]
    pub const fn version(&self) -> &Version {
        &self.version
    }

    /// Returns whether this installation is active or retained for rollback.
    #[must_use]
    pub const fn status(&self) -> ComponentLifecycleStatus {
        self.status
    }

    /// Returns actual bytes occupied by the component payload.
    #[must_use]
    pub const fn installed_bytes(&self) -> u64 {
        self.installed_bytes
    }
}

/// Actual disk usage of one known data category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CategoryDiskUse {
    category: DataCategory,
    bytes: u64,
}

impl CategoryDiskUse {
    /// Returns the semantic data category.
    #[must_use]
    pub const fn category(self) -> DataCategory {
        self.category
    }

    /// Returns actual bytes occupied by regular files.
    #[must_use]
    pub const fn bytes(self) -> u64 {
        self.bytes
    }
}

/// Actual per-category and aggregate semantic disk use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticDiskUse {
    categories: Vec<CategoryDiskUse>,
    total_bytes: u64,
}

impl SemanticDiskUse {
    /// Returns every known category in stable order, including zero-byte categories.
    #[must_use]
    pub fn categories(&self) -> &[CategoryDiskUse] {
        &self.categories
    }

    /// Returns actual bytes for one category.
    #[must_use]
    pub fn bytes_for(&self, category: DataCategory) -> Option<u64> {
        self.categories
            .iter()
            .find(|usage| usage.category == category)
            .map(|usage| usage.bytes)
    }

    /// Returns aggregate bytes across known categories.
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
}

/// Component lifecycle and disk-use report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticStatusReport {
    components: Vec<ComponentStatusEntry>,
    disk_use: SemanticDiskUse,
}

impl SemanticStatusReport {
    /// Returns active and retained rollback components.
    #[must_use]
    pub fn components(&self) -> &[ComponentStatusEntry] {
        &self.components
    }

    /// Returns actual per-category disk use.
    #[must_use]
    pub const fn disk_use(&self) -> &SemanticDiskUse {
        &self.disk_use
    }
}

pub(crate) fn build_status(
    state: &SemanticState,
) -> Result<SemanticStatusReport, SemanticStatusError> {
    let mut components = Vec::new();
    let active_model = state.active_model().map(|selection| selection.identity());
    for component in state.installed_components() {
        let status = match component.kind() {
            ArtifactKind::Model(identity) if Some(identity) != active_model => {
                ComponentLifecycleStatus::Rollback
            }
            _ => ComponentLifecycleStatus::Active,
        };
        components.push(component_status(component, status)?);
    }
    for component in state
        .rollback_workers()
        .iter()
        .chain(state.retained_models())
    {
        components.push(component_status(
            component,
            ComponentLifecycleStatus::Rollback,
        )?);
    }
    components.sort_by(|left, right| {
        left.component_id
            .cmp(&right.component_id)
            .then_with(|| left.status.cmp(&right.status))
            .then_with(|| left.version.cmp(&right.version))
    });

    let mut categories = Vec::with_capacity(DataCategory::all().len());
    let mut total_bytes = 0_u64;
    for category in DataCategory::all() {
        let bytes = path_bytes(&state.data_root().category_path(*category))?;
        total_bytes = total_bytes
            .checked_add(bytes)
            .ok_or(SemanticStatusError::SizeOverflow)?;
        categories.push(CategoryDiskUse {
            category: *category,
            bytes,
        });
    }
    Ok(SemanticStatusReport {
        components,
        disk_use: SemanticDiskUse {
            categories,
            total_bytes,
        },
    })
}

fn component_status(
    component: &InstalledComponent,
    status: ComponentLifecycleStatus,
) -> Result<ComponentStatusEntry, SemanticStatusError> {
    Ok(ComponentStatusEntry {
        artifact_id: component.artifact_id().clone(),
        component_id: component.component_id().clone(),
        kind: component.kind().clone(),
        version: component.version().clone(),
        status,
        installed_bytes: path_bytes(component.installed_path())?,
    })
}

fn path_bytes(path: &Path) -> Result<u64, SemanticStatusError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        return Err(SemanticStatusError::UnsafeEntry {
            path: path.to_owned(),
        });
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Err(SemanticStatusError::UnsafeEntry {
            path: path.to_owned(),
        });
    }
    let mut total = 0_u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        total = total
            .checked_add(path_bytes(&entry.path())?)
            .ok_or(SemanticStatusError::SizeOverflow)?;
    }
    Ok(total)
}

/// Component status reporting failure.
#[derive(Debug, Error)]
pub enum SemanticStatusError {
    /// A symlink or non-file/non-directory entry made reporting unsafe.
    #[error("semantic status encountered an unsafe entry: {}", path.display())]
    UnsafeEntry {
        /// Unsafe path.
        path: PathBuf,
    },
    /// Aggregate byte count overflowed.
    #[error("semantic disk usage overflowed")]
    SizeOverflow,
    /// Durable state could not be loaded.
    #[error(transparent)]
    State(#[from] SemanticStateError),
    /// Filesystem scanning failed.
    #[error("semantic status filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}
