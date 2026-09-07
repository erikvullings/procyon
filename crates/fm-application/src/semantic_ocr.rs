//! Optional production OCRmyPDF discovery, consent, and remediation (task 0197).
//!
//! OCR remediation is a desktop-only capability layered on top of the existing
//! semantic indexing pipeline. This module owns three concerns:
//!
//! * durable, explicit enable/disable **consent** persisted under the injected
//!   settings/app-data root ([`OcrPolicyStore`]);
//! * typed **availability** discovery of a user-installed executable, mapped
//!   from the safe probe in `fm-semantic-docling` ([`OcrAvailability`],
//!   [`OcrExecutableProbe`]);
//! * bounded, cancellable background **remediation** jobs that re-ingest files
//!   already reported as requiring OCR ([`SemanticOcrService`]).
//!
//! The worker itself performs the OCR: enabling consent hands the worker the
//! host-discovered canonical executable (see `fm-semantic-worker`), and
//! remediation simply re-feeds the previously text-less files so the OCR-aware
//! worker reconverts and ingests them. Nothing here ever executes OCRmyPDF; it
//! only decides whether the worker may, and which files to retry.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use fm_settings::{SettingsStore, VersionedDocument};
use serde::{Deserialize, Serialize};

mod remediation;

pub(crate) use coordinator::SemanticOcrCoordinator;
pub(crate) use remediation::{ClaimedOcrJob, classify_ingest_report};
mod coordinator;

pub use remediation::{
    OcrRemediationFileOutcome, OcrRemediationJob, OcrRemediationJobId, OcrRemediationOutcome,
    OcrRemediationScope, OcrRemediationState, OcrRemediationTarget, SemanticOcrError,
    SemanticOcrService,
};

/// Complete observable OCR remediation state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticOcrStatus {
    /// Whether the user has explicitly enabled OCR remediation.
    pub enabled: bool,
    /// Current host discovery result.
    pub availability: OcrAvailability,
    /// Backend-reported files that remain eligible for remediation.
    pub reported_files: Vec<OcrRemediationTarget>,
    /// Durable remediation history, newest first.
    pub jobs: Vec<OcrRemediationJob>,
}

/// Availability of a supported, user-installed OCRmyPDF executable.
///
/// This is the host-neutral projection of `fm-semantic-docling`'s safe probe.
/// A supported installation reports its canonical path and resolved version; a
/// rejected or absent one reports a typed reason plus actionable guidance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrAvailability {
    /// A supported executable was found and safely version-checked.
    Available {
        /// Canonical absolute executable selected by host discovery.
        executable: PathBuf,
        /// Version reported by the resolved executable, e.g. `16.10.4`.
        version: String,
    },
    /// No safe, supported executable is available.
    Unavailable {
        /// Typed reason discovery rejected the installation.
        reason: OcrUnavailableReason,
        /// Cross-platform installation and manual remediation guidance.
        guidance: String,
    },
}

impl OcrAvailability {
    /// Returns the canonical executable when a supported installation exists.
    #[must_use]
    pub fn executable(&self) -> Option<&Path> {
        match self {
            Self::Available { executable, .. } => Some(executable.as_path()),
            Self::Unavailable { .. } => None,
        }
    }

    /// Returns whether a supported executable is available.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
}

/// Typed reason production OCRmyPDF discovery rejected an installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OcrUnavailableReason {
    /// This host deliberately does not expose desktop OCR execution.
    HostUnavailable,
    /// No executable was present in a safe search location.
    Missing,
    /// A candidate exists but is not a regular executable file.
    NonExecutable {
        /// Absolute candidate path.
        path: PathBuf,
    },
    /// The executable could not be launched for its version probe.
    CouldNotExecute,
    /// The executable returned no valid OCRmyPDF semantic version.
    MalformedVersion,
    /// The installed version is outside Procyon's audited range.
    UnsupportedVersion {
        /// Installed version.
        version: String,
    },
    /// The fixed version probe exceeded its deadline.
    TimedOut,
    /// Version output exceeded the fixed capture limit.
    OutputTooLarge {
        /// Maximum combined stdout and stderr bytes.
        limit: u64,
    },
}

impl From<fm_semantic_docling::OcrMyPdfAvailability> for OcrAvailability {
    fn from(availability: fm_semantic_docling::OcrMyPdfAvailability) -> Self {
        match availability {
            fm_semantic_docling::OcrMyPdfAvailability::Available {
                executable,
                version,
            } => Self::Available {
                executable,
                version: version.to_string(),
            },
            fm_semantic_docling::OcrMyPdfAvailability::Unavailable { reason, guidance } => {
                Self::Unavailable {
                    reason: reason.into(),
                    guidance,
                }
            }
        }
    }
}

impl From<fm_semantic_docling::OcrMyPdfRejectionReason> for OcrUnavailableReason {
    fn from(reason: fm_semantic_docling::OcrMyPdfRejectionReason) -> Self {
        use fm_semantic_docling::OcrMyPdfRejectionReason as Reason;
        match reason {
            Reason::Missing => Self::Missing,
            Reason::NonExecutable { path } => Self::NonExecutable { path },
            Reason::CouldNotExecute => Self::CouldNotExecute,
            Reason::MalformedVersion => Self::MalformedVersion,
            Reason::UnsupportedVersion { version } => Self::UnsupportedVersion {
                version: version.to_string(),
            },
            Reason::TimedOut => Self::TimedOut,
            Reason::OutputTooLarge { limit } => Self::OutputTooLarge {
                limit: u64::try_from(limit).unwrap_or(u64::MAX),
            },
        }
    }
}

/// Discovers a supported OCRmyPDF executable for the current host.
///
/// The server host injects an always-unavailable probe so it never executes or
/// discovers OCRmyPDF; the desktop host injects [`DoclingOcrExecutableProbe`].
pub trait OcrExecutableProbe: Send + Sync {
    /// Performs a bounded, side-effect-free availability probe.
    fn probe(&self) -> OcrAvailability;
}

/// Real desktop discovery backed by the safe `fm-semantic-docling` probe.
#[derive(Debug, Default, Clone, Copy)]
pub struct DoclingOcrExecutableProbe;

impl OcrExecutableProbe for DoclingOcrExecutableProbe {
    fn probe(&self) -> OcrAvailability {
        fm_semantic_docling::OcrMyPdfAvailability::discover().into()
    }
}

/// Reports OCR as unavailable without ever launching a subprocess.
///
/// This is the default for hosts (such as the server) that must never gain
/// desktop OCR execution.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableOcrExecutableProbe;

impl OcrExecutableProbe for UnavailableOcrExecutableProbe {
    fn probe(&self) -> OcrAvailability {
        OcrAvailability::Unavailable {
            reason: OcrUnavailableReason::HostUnavailable,
            guidance: "OCR remediation is not available in this host.".to_owned(),
        }
    }
}

const POLICY_FILE_NAME: &str = "policy.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedOcrPolicy {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    enabled: bool,
}

impl PersistedOcrPolicy {
    const fn new(enabled: bool) -> Self {
        Self {
            schema_version: Self::CURRENT_SCHEMA_VERSION,
            enabled,
        }
    }
}

impl VersionedDocument for PersistedOcrPolicy {
    type MigrationError = std::convert::Infallible;

    const FILE_NAME: &'static str = POLICY_FILE_NAME;
    const CURRENT_SCHEMA_VERSION: u32 = 1;

    fn migrate(
        value: serde_json::Value,
        _version: u32,
    ) -> Result<serde_json::Value, Self::MigrationError> {
        Ok(value)
    }

    fn validate(&self) -> Result<(), Self::MigrationError> {
        Ok(())
    }
}

/// Durable, explicit OCR remediation consent.
///
/// Consent defaults to disabled (opt-in) and survives restarts. An unreadable
/// or malformed file is treated as disabled so a corrupt state never silently
/// enables OCR execution. The value is cached in memory so the host worker
/// resolver and the remediation service observe changes without re-reading the
/// file on every launch.
#[derive(Debug)]
pub struct OcrPolicyStore {
    store: SettingsStore,
    enabled: RwLock<bool>,
}

impl OcrPolicyStore {
    /// Loads durable consent from `directory`, defaulting to disabled.
    #[must_use]
    pub fn load(directory: impl Into<PathBuf>) -> Self {
        let store = SettingsStore::new(directory);
        let enabled = store
            .load_document::<PersistedOcrPolicy>()
            .ok()
            .flatten()
            .is_some_and(|policy| policy.enabled);
        Self {
            store,
            enabled: RwLock::new(enabled),
        }
    }

    /// Returns whether OCR remediation is currently enabled.
    #[must_use]
    pub fn enabled(&self) -> bool {
        *self
            .enabled
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Durably records explicit consent, returning whether the value changed.
    ///
    /// A changed value is the host's signal to retire and restart any running
    /// worker so its OCR configuration cannot remain stale.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the durable file could not be written; the
    /// in-memory value is only updated after a successful persist.
    pub fn set_enabled(&self, enabled: bool) -> std::io::Result<bool> {
        let mut guard = self
            .enabled
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *guard == enabled {
            return Ok(false);
        }
        self.store
            .save_document(&PersistedOcrPolicy::new(enabled))
            .map_err(std::io::Error::other)?;
        *guard = enabled;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> tempfile::TempDir {
        let parent =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/application-ocr-tests");
        std::fs::create_dir_all(&parent).unwrap();
        tempfile::Builder::new()
            .prefix("ocr-policy-")
            .tempdir_in(std::fs::canonicalize(parent).unwrap())
            .unwrap()
    }

    #[test]
    fn consent_defaults_to_disabled_and_persists_across_reloads() {
        let directory = temp_dir();
        let store = OcrPolicyStore::load(directory.path());
        assert!(!store.enabled());

        assert!(store.set_enabled(true).unwrap());
        assert!(store.enabled());
        // Setting the same value is a no-op that reports no change.
        assert!(!store.set_enabled(true).unwrap());

        // A fresh load observes the durable value.
        let reloaded = OcrPolicyStore::load(directory.path());
        assert!(reloaded.enabled());

        assert!(reloaded.set_enabled(false).unwrap());
        assert!(!OcrPolicyStore::load(directory.path()).enabled());
    }

    #[test]
    fn corrupt_policy_file_is_treated_as_disabled() {
        let directory = temp_dir();
        std::fs::write(directory.path().join(POLICY_FILE_NAME), b"{ not json").unwrap();
        assert!(!OcrPolicyStore::load(directory.path()).enabled());
    }

    #[test]
    fn unknown_policy_version_is_treated_as_disabled() {
        let directory = temp_dir();
        std::fs::write(
            directory.path().join(POLICY_FILE_NAME),
            br#"{"schemaVersion":999,"enabled":true}"#,
        )
        .unwrap();

        assert!(!OcrPolicyStore::load(directory.path()).enabled());
    }

    #[test]
    fn docling_available_maps_to_executable_and_version() {
        let availability = fm_semantic_docling::OcrMyPdfAvailability::Available {
            executable: PathBuf::from("/usr/local/bin/ocrmypdf"),
            version: fm_semantic_docling::OcrMyPdfVersion::new(16, 10, 4),
        };
        let mapped: OcrAvailability = availability.into();
        assert_eq!(
            mapped,
            OcrAvailability::Available {
                executable: PathBuf::from("/usr/local/bin/ocrmypdf"),
                version: "16.10.4".to_owned(),
            }
        );
        assert_eq!(
            mapped.executable(),
            Some(Path::new("/usr/local/bin/ocrmypdf"))
        );
        assert!(mapped.is_available());
    }

    #[test]
    fn docling_rejections_map_to_typed_reasons_with_guidance() {
        let cases = [
            (
                fm_semantic_docling::OcrMyPdfRejectionReason::Missing,
                OcrUnavailableReason::Missing,
            ),
            (
                fm_semantic_docling::OcrMyPdfRejectionReason::CouldNotExecute,
                OcrUnavailableReason::CouldNotExecute,
            ),
            (
                fm_semantic_docling::OcrMyPdfRejectionReason::MalformedVersion,
                OcrUnavailableReason::MalformedVersion,
            ),
            (
                fm_semantic_docling::OcrMyPdfRejectionReason::TimedOut,
                OcrUnavailableReason::TimedOut,
            ),
            (
                fm_semantic_docling::OcrMyPdfRejectionReason::OutputTooLarge { limit: 8192 },
                OcrUnavailableReason::OutputTooLarge { limit: 8192 },
            ),
        ];
        for (reason, expected) in cases {
            let availability = fm_semantic_docling::OcrMyPdfAvailability::Unavailable {
                reason,
                guidance: "install it".to_owned(),
            };
            let mapped: OcrAvailability = availability.into();
            assert_eq!(
                mapped,
                OcrAvailability::Unavailable {
                    reason: expected,
                    guidance: "install it".to_owned(),
                }
            );
            assert!(mapped.executable().is_none());
        }
    }

    #[test]
    fn docling_unsupported_version_carries_the_installed_version() {
        let availability = fm_semantic_docling::OcrMyPdfAvailability::Unavailable {
            reason: fm_semantic_docling::OcrMyPdfRejectionReason::UnsupportedVersion {
                version: fm_semantic_docling::OcrMyPdfVersion::new(15, 0, 0),
            },
            guidance: "upgrade".to_owned(),
        };
        let mapped: OcrAvailability = availability.into();
        assert_eq!(
            mapped,
            OcrAvailability::Unavailable {
                reason: OcrUnavailableReason::UnsupportedVersion {
                    version: "15.0.0".to_owned(),
                },
                guidance: "upgrade".to_owned(),
            }
        );
    }

    #[test]
    fn unavailable_probe_never_reports_an_executable() {
        let probe = UnavailableOcrExecutableProbe;
        assert!(!probe.probe().is_available());
    }
}
