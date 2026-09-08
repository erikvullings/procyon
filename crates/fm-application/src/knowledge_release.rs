//! Release visibility gate for Structured Knowledge Search (task 0208).
//!
//! Retrieval quality is measured per release candidate, so a production build
//! may only expose the feature after a recorded go decision. The decision is
//! owned by the build: the release workflow compiles
//! [`KNOWLEDGE_RELEASE_QUALIFIED_ENV`] into the binary only when the protected
//! repository variable says the candidate was qualified. An unset, false, or
//! malformed value fails closed in a release build.
//!
//! Developer, debug, and test builds stay usable without any qualification so
//! the mock, server, and desktop development workflows are unaffected.

/// Compile-time variable carrying the measured go decision into a build.
///
/// The only value that qualifies a release build is exactly `true`.
pub const KNOWLEDGE_RELEASE_QUALIFIED_ENV: &str = "PROCYON_KNOWLEDGE_SEARCH_RELEASE_QUALIFIED";

/// Whether this build may expose Structured Knowledge Search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeReleaseAccess {
    /// A release build carrying a measured go decision.
    Qualified,
    /// A developer, debug, or test build, usable without qualification.
    Developer,
    /// A release build without a measured go decision: fail closed.
    Unqualified,
}

impl KnowledgeReleaseAccess {
    /// Resolves the access this build was compiled with.
    #[must_use]
    pub fn from_build() -> Self {
        Self::resolve(
            cfg!(debug_assertions),
            option_env!("PROCYON_KNOWLEDGE_SEARCH_RELEASE_QUALIFIED"),
        )
    }

    /// Resolves access from the build kind and the compiled qualification.
    ///
    /// Anything other than an exact `true` leaves a release build unqualified,
    /// so a typo, an empty value, or an absent variable cannot make the feature
    /// production visible.
    #[must_use]
    pub fn resolve(developer_build: bool, qualification: Option<&str>) -> Self {
        match qualification {
            Some("true") => Self::Qualified,
            _ if developer_build => Self::Developer,
            _ => Self::Unqualified,
        }
    }

    /// Reports whether the feature may be offered by this build at all.
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Qualified | Self::Developer)
    }
}

impl Default for KnowledgeReleaseAccess {
    fn default() -> Self {
        Self::from_build()
    }
}

#[cfg(test)]
mod tests {
    use super::KnowledgeReleaseAccess;

    #[test]
    fn a_release_build_fails_closed_without_a_measured_go_decision() {
        for qualification in [None, Some(""), Some("false"), Some("TRUE"), Some("1")] {
            let access = KnowledgeReleaseAccess::resolve(false, qualification);

            assert_eq!(
                access,
                KnowledgeReleaseAccess::Unqualified,
                "release build with {qualification:?} must stay unqualified"
            );
            assert!(!access.is_available());
        }
    }

    #[test]
    fn a_release_build_with_an_exact_go_decision_is_qualified() {
        let access = KnowledgeReleaseAccess::resolve(false, Some("true"));

        assert_eq!(access, KnowledgeReleaseAccess::Qualified);
        assert!(access.is_available());
    }

    #[test]
    fn developer_builds_stay_usable_without_any_qualification() {
        for qualification in [None, Some(""), Some("false")] {
            let access = KnowledgeReleaseAccess::resolve(true, qualification);

            assert_eq!(access, KnowledgeReleaseAccess::Developer);
            assert!(access.is_available());
        }
        assert_eq!(
            KnowledgeReleaseAccess::resolve(true, Some("true")),
            KnowledgeReleaseAccess::Qualified
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    fn this_test_build_is_a_developer_build() {
        assert!(KnowledgeReleaseAccess::from_build().is_available());
        assert_eq!(
            KnowledgeReleaseAccess::default(),
            KnowledgeReleaseAccess::from_build()
        );
    }
}
