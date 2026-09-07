use fm_domain::Location;

use crate::{FilesystemIdentity, PolicyError, RootId, SemanticLibraryPolicy};

/// Provider observation considered while resolving an enrolled root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedRootIdentity {
    location: Location,
    identity: Option<FilesystemIdentity>,
}

impl ObservedRootIdentity {
    /// Creates a provider-neutral observation.
    #[must_use]
    pub const fn new(location: Location, identity: Option<FilesystemIdentity>) -> Self {
        Self { location, identity }
    }

    /// Returns the observed location.
    #[must_use]
    pub const fn location(&self) -> &Location {
        &self.location
    }

    /// Returns stable identity when the provider supplied one.
    #[must_use]
    pub const fn identity(&self) -> Option<&FilesystemIdentity> {
        self.identity.as_ref()
    }
}

/// Why a root remains at its old location and must be treated as unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootUnavailabilityReason {
    /// The enrolled root or observation had no stable identity.
    UnprovenIdentity,
    /// More than one location reported the same stable identity.
    AmbiguousIdentity,
    /// A matching file id appeared on a different volume.
    CrossVolume,
    /// The old path now names a different filesystem object.
    PathReused,
    /// A matching identity appeared through a different provider.
    CrossProvider,
    /// No candidate matched the enrolled root.
    Missing,
    /// Following the move would leave consent, exclusions, or catalogued
    /// occurrences inconsistent with another enrolled root.
    RelocationConflict,
}

/// Conservative outcome of root move reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootMoveResolution {
    /// The old location still has the enrolled identity.
    Unchanged,
    /// Stable identity proved a same-provider, same-volume move.
    ProvenMove {
        /// Previous persisted location.
        previous: Location,
        /// Newly persisted location.
        current: Location,
    },
    /// The old location is retained and the root becomes unavailable.
    RetainedUnavailable {
        /// Reason automatic following was unsafe.
        reason: RootUnavailabilityReason,
    },
}

pub(crate) fn resolve(
    policy: &mut SemanticLibraryPolicy,
    root_id: RootId,
    observations: &[ObservedRootIdentity],
) -> Result<RootMoveResolution, PolicyError> {
    for observation in observations {
        super::policy::validate_location(observation.location())?;
    }
    let root = policy
        .root(root_id)
        .ok_or(PolicyError::UnknownRoot(root_id))?;
    let old_location = root.location().clone();
    let Some(expected) = root.filesystem_identity().cloned() else {
        return Ok(RootMoveResolution::RetainedUnavailable {
            reason: RootUnavailabilityReason::UnprovenIdentity,
        });
    };

    let exact: Vec<_> = observations
        .iter()
        .filter(|observation| {
            observation.location.provider_id == old_location.provider_id
                && observation.identity.as_ref() == Some(&expected)
        })
        .collect();
    if exact.len() > 1 {
        return Ok(RootMoveResolution::RetainedUnavailable {
            reason: RootUnavailabilityReason::AmbiguousIdentity,
        });
    }
    if let Some(observation) = exact.first() {
        if observation.location == old_location {
            return Ok(RootMoveResolution::Unchanged);
        }
        let current = observation.location.clone();
        let relocated = policy
            .root(root_id)
            .expect("root existence checked above")
            .relocated(&current)?;
        let mut candidate = policy.clone();
        candidate.replace_root(relocated)?;
        candidate.validate_structure()?;
        *policy = candidate;
        return Ok(RootMoveResolution::ProvenMove {
            previous: old_location,
            current,
        });
    }

    let reason = if observations.iter().any(|observation| {
        observation.location.provider_id == old_location.provider_id
            && observation.identity.as_ref().is_some_and(|identity| {
                identity.file_id() == expected.file_id()
                    && identity.volume_id() != expected.volume_id()
            })
    }) {
        RootUnavailabilityReason::CrossVolume
    } else if observations.iter().any(|observation| {
        observation.location == old_location
            && observation
                .identity
                .as_ref()
                .is_some_and(|identity| identity != &expected)
    }) {
        RootUnavailabilityReason::PathReused
    } else if observations.iter().any(|observation| {
        observation.location.provider_id != old_location.provider_id
            && observation.identity.as_ref() == Some(&expected)
    }) {
        RootUnavailabilityReason::CrossProvider
    } else if observations
        .iter()
        .any(|observation| observation.identity.is_none())
    {
        RootUnavailabilityReason::UnprovenIdentity
    } else {
        RootUnavailabilityReason::Missing
    };
    Ok(RootMoveResolution::RetainedUnavailable { reason })
}
