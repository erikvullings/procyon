use fm_domain::Location;

use crate::hierarchy::{depth, is_same_or_descendant};
use crate::{ExclusionId, PolicyError, RootId, SemanticLibraryPolicy};

/// Effective processing consent for a provider-neutral folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentState {
    /// The folder itself is an enrolled root.
    IncludedHere {
        /// Root granting consent.
        root_id: RootId,
    },
    /// Recursive consent is inherited from an ancestor root.
    InheritedFromParent {
        /// Ancestor root granting consent.
        root_id: RootId,
    },
    /// An explicit exclusion revokes consent.
    Excluded {
        /// Root that owns the exclusion.
        root_id: RootId,
        /// Most-specific matching exclusion.
        exclusion_id: ExclusionId,
    },
    /// No root grants consent.
    NotIncluded,
}

pub(crate) fn evaluate(
    policy: &SemanticLibraryPolicy,
    location: &Location,
) -> Result<ConsentState, PolicyError> {
    super::policy::validate_location(location)?;
    let mut excluded = Vec::new();
    let mut included = Vec::new();
    for root in policy.roots().values() {
        for exclusion in root.exclusions() {
            if is_same_or_descendant(exclusion.location(), location)
                .map_err(PolicyError::InvalidLocation)?
            {
                excluded.push((
                    depth(exclusion.location()).map_err(PolicyError::InvalidLocation)?,
                    root.id(),
                    exclusion.id(),
                ));
            }
        }
        let is_exact = root.location() == location;
        if is_exact
            || root.recursive()
                && is_same_or_descendant(root.location(), location)
                    .map_err(PolicyError::InvalidLocation)?
        {
            included.push((
                depth(root.location()).map_err(PolicyError::InvalidLocation)?,
                root,
            ));
        }
    }
    excluded.sort();
    if let Some((_, root_id, exclusion_id)) = excluded.last() {
        return Ok(ConsentState::Excluded {
            root_id: *root_id,
            exclusion_id: *exclusion_id,
        });
    }
    included.sort_by_key(|(root_depth, root)| (*root_depth, root.id()));
    let Some((_, root)) = included.last() else {
        return Ok(ConsentState::NotIncluded);
    };
    if root.location() == location {
        Ok(ConsentState::IncludedHere { root_id: root.id() })
    } else {
        Ok(ConsentState::InheritedFromParent { root_id: root.id() })
    }
}
