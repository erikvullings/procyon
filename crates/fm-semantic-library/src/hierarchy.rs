use fm_domain::{Location, LocationError};

/// Returns whether `candidate` is the same location as, or a descendant of, `ancestor`.
///
/// Parent traversal uses the owning provider's structural URI rules, never a
/// string prefix.
pub(crate) fn is_same_or_descendant(
    ancestor: &Location,
    candidate: &Location,
) -> Result<bool, LocationError> {
    if ancestor.provider_id != candidate.provider_id {
        return Ok(false);
    }
    let mut current = Some(candidate.clone());
    while let Some(location) = current {
        let parent = checked_parent(&location)?;
        if location == *ancestor {
            return Ok(true);
        }
        current = parent;
    }
    Ok(false)
}

pub(crate) fn depth(location: &Location) -> Result<usize, LocationError> {
    let mut depth = 0;
    let mut current = Some(location.clone());
    while let Some(location) = current {
        depth += 1;
        current = checked_parent(&location)?;
    }
    Ok(depth)
}

/// Rewrites `location` so that it keeps its position relative to a root that
/// moved from `previous_root` to `current_root`.
///
/// Traversal is structural: names are taken apart and rejoined through the
/// owning provider's rules rather than by rewriting a URI prefix.
pub(crate) fn rebase(
    previous_root: &Location,
    current_root: &Location,
    location: &Location,
) -> Result<Location, LocationError> {
    let mut names = Vec::new();
    let mut cursor = location.clone();
    while cursor != *previous_root {
        names.push(cursor.name()?);
        cursor = checked_parent(&cursor)?.ok_or(LocationError::EscapesRoot)?;
    }
    let mut rebased = current_root.clone();
    for name in names.into_iter().rev() {
        rebased = rebased.join(&name)?;
    }
    Ok(rebased)
}

fn checked_parent(location: &Location) -> Result<Option<Location>, LocationError> {
    let parent = location.parent()?;
    if parent.is_some() {
        let name = location.name()?;
        if matches!(name.as_str(), "." | "..") {
            return Err(LocationError::InvalidName(name));
        }
    }
    Ok(parent)
}
