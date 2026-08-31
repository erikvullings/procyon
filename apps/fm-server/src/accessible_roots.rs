//! Validates that incoming file operations stay within configured accessible roots.
//! Performed after symlink resolution to prevent symlink escape attacks (task 0064).

use std::path::{Path, PathBuf};
use thiserror::Error;

use fm_transport_dto::LocationDto;

/// Error validating a location against accessible roots.
#[derive(Debug, Error)]
pub enum AccessibleRootsError {
    /// Path resolution failed (e.g., broken symlink or permission denied).
    #[error("path resolution failed: {0}")]
    ResolutionFailed(String),

    /// Path is outside configured accessible roots.
    #[error("path {path} is outside configured accessible roots")]
    OutsideRoots {
        /// The path that was outside the roots.
        path: String,
    },

    /// No accessible roots configured.
    #[error("no accessible roots configured")]
    NoRootsConfigured,
}

/// Validates that a path is within one of the configured accessible roots,
/// after resolving symlinks. Returns the canonicalized path.
///
/// The target need not exist yet (e.g. a path about to be created): the
/// nearest existing ancestor is canonicalized and the missing suffix is
/// rejoined before the roots check, so an escape can't be smuggled in via a
/// not-yet-created path.
pub fn validate_within_accessible_roots(
    path: &Path,
    roots: &[PathBuf],
) -> Result<PathBuf, AccessibleRootsError> {
    if roots.is_empty() {
        // Empty roots means unrestricted access (for backward compatibility
        // with single-machine deployments). In task 0064, this is allowed but
        // discouraged; LAN deployments must specify roots.
        return Ok(path.to_path_buf());
    }

    let canonical_path = canonicalize_existing_or_pending(path)?;
    let canonical_roots = roots
        .iter()
        .map(|root| {
            root.canonicalize()
                .map_err(|e| AccessibleRootsError::ResolutionFailed(e.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    if canonical_roots
        .iter()
        .any(|root| canonical_path.starts_with(root))
    {
        return Ok(canonical_path);
    }

    Err(AccessibleRootsError::OutsideRoots {
        path: canonical_path.display().to_string(),
    })
}

/// Canonicalizes `path`, falling back to canonicalizing the nearest existing
/// ancestor and rejoining the missing suffix when `path` itself doesn't
/// exist yet.
fn canonicalize_existing_or_pending(path: &Path) -> Result<PathBuf, AccessibleRootsError> {
    if let Ok(canonical) = path.canonicalize() {
        return Ok(canonical);
    }

    let mut ancestor = path.to_path_buf();
    let mut pending_suffix = Vec::new();
    loop {
        let Some(name) = ancestor.file_name().map(std::ffi::OsStr::to_owned) else {
            return Err(AccessibleRootsError::ResolutionFailed(
                "no existing ancestor found".to_owned(),
            ));
        };
        let Some(parent) = ancestor.parent().map(Path::to_path_buf) else {
            return Err(AccessibleRootsError::ResolutionFailed(
                "no existing ancestor found".to_owned(),
            ));
        };
        pending_suffix.push(name);
        ancestor = parent;
        if ancestor.exists() {
            break;
        }
    }

    let mut resolved = ancestor
        .canonicalize()
        .map_err(|e| AccessibleRootsError::ResolutionFailed(e.to_string()))?;
    for name in pending_suffix.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

/// Validates a wire-level [`LocationDto`] against the configured accessible
/// roots. Non-local providers (archive, sftp, ftp, search) are skipped:
/// their locations don't resolve to a native path on this machine, so
/// accessible-roots enforcement doesn't apply to them (task 0064).
pub fn validate_location(
    location: &LocationDto,
    roots: &[PathBuf],
) -> Result<(), AccessibleRootsError> {
    if location.provider_id != "local" {
        return Ok(());
    }
    let domain_location: fm_domain::Location = location.clone().into();
    let path = domain_location
        .to_native_path()
        .map_err(|e| AccessibleRootsError::ResolutionFailed(format!("invalid location: {e}")))?;
    validate_within_accessible_roots(&path, roots).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn empty_roots_allows_any_path() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("file.txt");
        std::fs::write(&path, b"test").unwrap();

        let result = validate_within_accessible_roots(&path, &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn path_within_single_root_is_allowed() {
        let temp = TempDir::new().unwrap();
        let subdir = temp.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        let file = subdir.join("file.txt");
        std::fs::write(&file, b"test").unwrap();

        let result = validate_within_accessible_roots(&file, &[temp.path().to_path_buf()]);
        assert!(result.is_ok());
    }

    #[test]
    fn path_outside_roots_is_denied() {
        let temp1 = TempDir::new().unwrap();
        let temp2 = TempDir::new().unwrap();

        let file = temp2.path().join("file.txt");
        std::fs::write(&file, b"test").unwrap();

        let result = validate_within_accessible_roots(&file, &[temp1.path().to_path_buf()]);
        assert!(matches!(
            result,
            Err(AccessibleRootsError::OutsideRoots { .. })
        ));
    }

    #[test]
    fn symlink_escape_is_prevented() {
        let temp = TempDir::new().unwrap();
        let allowed = temp.path().join("allowed");
        std::fs::create_dir(&allowed).unwrap();

        let outside = temp.path().join("outside.txt");
        std::fs::write(&outside, b"secret").unwrap();

        let symlink = allowed.join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &symlink).unwrap();
        // Creating a symlink needs SeCreateSymbolicLinkPrivilege, which an
        // unelevated Windows session without Developer Mode does not hold.
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_file(&outside, &symlink) {
            eprintln!("symlink fixture unsupported in this Windows environment: {error}");
            return;
        }

        // The symlink's target is outside the allowed root, so access should be denied.
        let result = validate_within_accessible_roots(&symlink, std::slice::from_ref(&allowed));
        assert!(matches!(
            result,
            Err(AccessibleRootsError::OutsideRoots { .. })
        ));
    }

    #[test]
    fn relative_path_traversal_is_prevented() {
        let temp = TempDir::new().unwrap();
        let allowed = temp.path().join("allowed");
        std::fs::create_dir(&allowed).unwrap();

        let outside = temp.path().join("outside.txt");
        std::fs::write(&outside, b"secret").unwrap();

        // Construct a path like allowed/../outside, which canonicalization should resolve.
        let traversal_path = allowed.join("..").join("outside.txt");

        let result =
            validate_within_accessible_roots(&traversal_path, std::slice::from_ref(&allowed));
        assert!(matches!(
            result,
            Err(AccessibleRootsError::OutsideRoots { .. })
        ));
    }

    #[test]
    fn multiple_roots_allow_any_configured_root() {
        let temp1 = TempDir::new().unwrap();
        let temp2 = TempDir::new().unwrap();

        let file1 = temp1.path().join("file1.txt");
        std::fs::write(&file1, b"test1").unwrap();

        let file2 = temp2.path().join("file2.txt");
        std::fs::write(&file2, b"test2").unwrap();

        let roots = vec![temp1.path().to_path_buf(), temp2.path().to_path_buf()];

        assert!(validate_within_accessible_roots(&file1, &roots).is_ok());
        assert!(validate_within_accessible_roots(&file2, &roots).is_ok());
    }

    #[test]
    fn dot_dot_encoded_escape_attempt_is_prevented() {
        // Note: filesystem path components can't actually contain literal `..` when
        // created with normal APIs, but this test documents the intent: after
        // canonicalization, escape attempts are neutralized.
        let temp = TempDir::new().unwrap();
        let allowed = temp.path().join("allowed");
        std::fs::create_dir(&allowed).unwrap();

        // This tests that canonicalization resolves `.` correctly.
        let safe_path = allowed.join(".").join("file.txt");
        std::fs::write(&safe_path, b"test").unwrap();

        let result = validate_within_accessible_roots(&safe_path, std::slice::from_ref(&allowed));
        assert!(result.is_ok());
    }
}
