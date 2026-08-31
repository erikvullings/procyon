//! Relative-path helpers shared by the traversal engine and sync planning.

use fm_domain::{Location, LocationError};

/// Joins a `/`-separated relative path onto `root`, one validated segment at
/// a time, via [`Location::join`].
///
/// An empty `relative_path` resolves to `root` itself. Used to resolve the
/// hypothetical destination location for an entry that exists on only one
/// side: the traversal only ever descends into directory pairs that exist on
/// both sides, so every relative path's parent is guaranteed resolvable on
/// both roots even when the leaf itself is not.
pub fn resolve_relative(root: &Location, relative_path: &str) -> Result<Location, LocationError> {
    let mut current = root.clone();
    if relative_path.is_empty() {
        return Ok(current);
    }
    for segment in relative_path.split('/') {
        current = current.join(segment)?;
    }
    Ok(current)
}

/// Returns the parent of a `/`-separated relative path, or `""` at the root.
#[must_use]
pub fn relative_parent(relative_path: &str) -> &str {
    match relative_path.rsplit_once('/') {
        Some((parent, _name)) => parent,
        None => "",
    }
}

/// Joins a child name onto a `/`-separated relative path.
#[must_use]
pub fn relative_join(relative_path: &str, name: &str) -> String {
    if relative_path.is_empty() {
        name.to_owned()
    } else {
        format!("{relative_path}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_domain::ProviderId;

    fn root() -> Location {
        Location::new(ProviderId::new("local"), "file:///Users/erik/left")
    }

    #[test]
    fn resolve_relative_with_empty_path_returns_root() {
        assert_eq!(resolve_relative(&root(), "").unwrap(), root());
    }

    #[test]
    fn resolve_relative_joins_every_segment() {
        let resolved = resolve_relative(&root(), "sub/dir/file.txt").unwrap();
        assert_eq!(resolved.uri, "file:///Users/erik/left/sub/dir/file.txt");
    }

    #[test]
    fn relative_parent_of_top_level_name_is_empty() {
        assert_eq!(relative_parent("file.txt"), "");
    }

    #[test]
    fn relative_parent_of_nested_name_strips_the_last_segment() {
        assert_eq!(relative_parent("sub/dir/file.txt"), "sub/dir");
    }

    #[test]
    fn relative_join_round_trips_with_relative_parent() {
        assert_eq!(relative_join("sub/dir", "file.txt"), "sub/dir/file.txt");
        assert_eq!(relative_join("", "file.txt"), "file.txt");
    }
}
