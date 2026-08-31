//! VFS-independent contract for native local-search indexes.
//!
//! Search engines use this narrow interface as an optional optimization. It
//! deliberately carries local paths and normalized search semantics rather
//! than VFS providers or platform query languages.

use std::path::{Component, Path, PathBuf};

use tokio_util::sync::CancellationToken;

/// A local path returned by a native index.
///
/// The path is absolute and lexically normalized, but deliberately not
/// canonicalized: canonicalizing would follow symlinks before the search
/// engine has checked its containment policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPathReference(PathBuf);

impl LocalPathReference {
    /// Creates a normalized reference to an absolute local path.
    ///
    /// # Errors
    ///
    /// Returns [`SearchAccelerationError::InvalidPath`] for relative paths.
    pub fn new(path: &Path) -> Result<Self, SearchAccelerationError> {
        if !path.is_absolute() {
            return Err(SearchAccelerationError::InvalidPath(
                "native index result is not an absolute path".to_owned(),
            ));
        }

        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    normalized.pop();
                }
                Component::Normal(component) => normalized.push(component),
                Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
            }
        }
        Ok(Self(normalized))
    }

    /// Returns this reference as a native path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// Name-predicate forms understood by a native index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchAccelerationPredicate {
    /// Case-insensitive literal filename substring.
    NameSubstring,
    /// Case-sensitive literal filename substring.
    CaseSensitiveNameSubstring,
}

/// Local-root scopes understood by a native index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchAccelerationScope {
    /// A local directory and all of its descendants.
    RecursiveDirectory,
}

/// The semantics advertised by a native index implementation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchAccelerationCapabilities {
    /// Predicates evaluated by the index without weakening their semantics.
    pub supported_predicates: Vec<SearchAccelerationPredicate>,
    /// Scopes evaluated by the index without weakening their semantics.
    pub supported_scopes: Vec<SearchAccelerationScope>,
}

impl SearchAccelerationCapabilities {
    /// Whether this implementation can exactly evaluate a predicate/scope pair.
    #[must_use]
    pub fn supports(
        &self,
        predicate: SearchAccelerationPredicate,
        scope: SearchAccelerationScope,
    ) -> bool {
        self.supported_predicates.contains(&predicate) && self.supported_scopes.contains(&scope)
    }
}

/// A native indexed filename search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchAccelerationRequest {
    /// Root that constrains every result.
    pub root: LocalPathReference,
    /// Literal filename text. Wildcards are not part of this contract.
    pub name: String,
    /// Whether filename matching is case-sensitive.
    pub case_sensitive: bool,
    /// Requested local scope.
    pub scope: SearchAccelerationScope,
}

/// A normalized local result from a native index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchAccelerationResult {
    /// Location returned by the native index.
    pub path: LocalPathReference,
    /// Whether the platform identified this result as an alias rather than
    /// the real filesystem entry.
    pub is_alias: bool,
}

/// Failures at the native-index boundary.
#[derive(Debug, thiserror::Error)]
pub enum SearchAccelerationError {
    /// This host has no usable native index.
    #[error("native indexed search is unsupported")]
    Unsupported,
    /// The platform index is temporarily unavailable.
    #[error("native indexed search is unavailable: {0}")]
    Unavailable(String),
    /// The index query failed.
    #[error("native indexed search failed: {0}")]
    Failed(String),
    /// The caller cancelled the native query.
    #[error("native indexed search was cancelled")]
    Cancelled,
    /// A native result did not identify an absolute local path.
    #[error("invalid native indexed path: {0}")]
    InvalidPath(String),
}

/// Optional acceleration over a platform-maintained local filesystem index.
pub trait SearchAcceleration: Send + Sync {
    /// Reports precisely the predicates and scopes this implementation can preserve.
    fn capabilities(&self) -> SearchAccelerationCapabilities;

    /// Queries the index without exposing platform query syntax to callers.
    ///
    /// # Errors
    ///
    /// Implementations return [`SearchAccelerationError::Unsupported`] or
    /// [`SearchAccelerationError::Unavailable`] when the caller should use a
    /// recursive fallback, and [`SearchAccelerationError::Cancelled`] promptly
    /// after cancellation.
    fn search(
        &self,
        request: &SearchAccelerationRequest,
        cancellation: &CancellationToken,
    ) -> Result<Vec<SearchAccelerationResult>, SearchAccelerationError>;
}

/// Fallback used by unsupported hosts and isolated tests.
#[derive(Debug, Default)]
pub struct UnsupportedSearchAccelerator;

impl SearchAcceleration for UnsupportedSearchAccelerator {
    fn capabilities(&self) -> SearchAccelerationCapabilities {
        SearchAccelerationCapabilities::default()
    }

    fn search(
        &self,
        _request: &SearchAccelerationRequest,
        _cancellation: &CancellationToken,
    ) -> Result<Vec<SearchAccelerationResult>, SearchAccelerationError> {
        Err(SearchAccelerationError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalPathReference, SearchAcceleration, UnsupportedSearchAccelerator};
    use tokio_util::sync::CancellationToken;

    #[test]
    fn unsupported_accelerator_advertises_no_native_search_capabilities() {
        let accelerator = UnsupportedSearchAccelerator;

        assert!(accelerator.capabilities().supported_predicates.is_empty());
        assert!(accelerator.capabilities().supported_scopes.is_empty());
    }

    #[test]
    fn local_path_references_are_absolute_and_lexically_normalized() {
        let root = std::env::current_dir().unwrap();
        let reference = LocalPathReference::new(&root.join("one").join("..").join("two")).unwrap();

        assert_eq!(reference.as_path(), root.join("two"));
    }

    #[test]
    fn unsupported_accelerator_returns_a_safe_error() {
        let accelerator = UnsupportedSearchAccelerator;
        let root = std::env::current_dir().unwrap();
        let error = accelerator
            .search(
                &super::SearchAccelerationRequest {
                    root: LocalPathReference::new(&root).unwrap(),
                    name: "report".to_owned(),
                    case_sensitive: false,
                    scope: super::SearchAccelerationScope::RecursiveDirectory,
                },
                &CancellationToken::new(),
            )
            .unwrap_err();

        assert!(matches!(error, super::SearchAccelerationError::Unsupported));
    }
}
