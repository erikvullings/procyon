//! Bounded retention of already-displayed Structured Knowledge evidence.
//!
//! Optional answer generation must consume exactly the evidence a user already
//! inspected, and it must never rerun or replan retrieval to obtain it. That
//! requires the host to remember one bounded evidence set per successful
//! search, keyed by the fingerprint the search returned.
//!
//! Retention is deliberately hostile to accidental growth and disclosure:
//!
//! - the cache is bounded by entry count and by entry lifetime;
//! - every entry is bound to the tenant, library, and workspace that produced
//!   it, and a lookup from any other binding is a miss rather than a hit;
//! - entries hold the displayed evidence projection only, which carries opaque
//!   identities, titles, and content — never filesystem paths or locations;
//! - the authorized scope is retained as an opaque payload so authorization can
//!   be re-resolved later without this module interpreting consent.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::knowledge::KnowledgeSearchPlan;
use crate::semantic_library::RagScopeSelection;

/// Maximum evidence sets retained across every binding.
pub(crate) const MAX_CACHED_EVIDENCE_SETS: usize = 16;
/// How long one evidence set stays answerable after it was displayed.
pub(crate) const CACHED_EVIDENCE_TTL: Duration = Duration::from_secs(30 * 60);

/// Tenant, library, and workspace an evidence set belongs to.
///
/// A lookup must present the same binding it was recorded with. Anything else
/// — another tenant, another library, another workspace — is reported as a
/// miss, so a fingerprint can never be replayed across an authorization
/// boundary even if it is guessed or leaked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KnowledgeEvidenceBinding {
    /// Opaque tenant identity that owned the search.
    pub(crate) tenant_id: String,
    /// Opaque library identity that was searched.
    pub(crate) library_id: String,
    /// Workspace through which access was authorized.
    pub(crate) workspace_id: fm_domain::WorkspaceId,
}

/// One retained evidence set exactly as it was displayed.
#[derive(Debug, Clone)]
pub(crate) struct CachedKnowledgeEvidence {
    /// Tenant/library/workspace this set may be answered from.
    pub(crate) binding: KnowledgeEvidenceBinding,
    /// Authorized scope selection, retained only to re-resolve authorization.
    pub(crate) selection: RagScopeSelection,
    /// Deterministic plan that produced the set, used for answer framing.
    pub(crate) plan: KnowledgeSearchPlan,
    /// Displayed evidence in displayed order, with identity and content intact.
    pub(crate) evidence: Vec<fm_transport_dto::KnowledgeEvidenceDto>,
    /// Indexed content fingerprints keyed by record identity.
    ///
    /// Retained because the displayed projection deliberately omits them, and
    /// freshness must be recomputed against a newer snapshot before an answer
    /// is generated.
    pub(crate) indexed_content_hashes: HashMap<String, String>,
}

struct Entry {
    evidence: CachedKnowledgeEvidence,
    recorded_at: Instant,
    sequence: u64,
}

/// Bounded, lifetime-limited store of displayed knowledge evidence sets.
pub(crate) struct KnowledgeEvidenceCache {
    entries: Mutex<HashMap<String, Entry>>,
    next_sequence: Mutex<u64>,
    maximum_entries: usize,
    time_to_live: Duration,
}

impl Default for KnowledgeEvidenceCache {
    fn default() -> Self {
        Self::new(MAX_CACHED_EVIDENCE_SETS, CACHED_EVIDENCE_TTL)
    }
}

impl KnowledgeEvidenceCache {
    /// Creates a cache with explicit bounds, so tests can exercise eviction.
    pub(crate) fn new(maximum_entries: usize, time_to_live: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            next_sequence: Mutex::new(0),
            maximum_entries: maximum_entries.max(1),
            time_to_live,
        }
    }

    /// Records one displayed evidence set, evicting the oldest when full.
    ///
    /// An empty fingerprint is refused rather than stored under a key that
    /// could later collide with a malformed request.
    pub(crate) fn record(&self, fingerprint: &str, evidence: CachedKnowledgeEvidence) {
        if fingerprint.is_empty() {
            return;
        }
        let sequence = {
            let mut next = self.locked_sequence();
            *next = next.wrapping_add(1);
            *next
        };
        let mut entries = self.locked_entries();
        Self::expire(&mut entries, self.time_to_live);
        entries.insert(
            fingerprint.to_owned(),
            Entry {
                evidence,
                recorded_at: Instant::now(),
                sequence,
            },
        );
        while entries.len() > self.maximum_entries {
            let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.sequence)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            entries.remove(&oldest);
        }
    }

    /// Returns the retained set for one fingerprint inside one binding.
    ///
    /// Returns `None` for a miss, an expired entry, or a binding mismatch;
    /// callers translate all three into the same refresh-required answer so
    /// nothing about another binding's cache is observable.
    pub(crate) fn get(
        &self,
        fingerprint: &str,
        binding: &KnowledgeEvidenceBinding,
    ) -> Option<CachedKnowledgeEvidence> {
        let mut entries = self.locked_entries();
        Self::expire(&mut entries, self.time_to_live);
        entries
            .get(fingerprint)
            .filter(|entry| &entry.evidence.binding == binding)
            .map(|entry| entry.evidence.clone())
    }

    /// Drops every retained set, used when a caller invalidates its evidence.
    #[cfg(test)]
    pub(crate) fn clear(&self) {
        self.locked_entries().clear();
    }

    /// Number of retained sets after expiry, exposed for bound assertions.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        let mut entries = self.locked_entries();
        Self::expire(&mut entries, self.time_to_live);
        entries.len()
    }

    fn expire(entries: &mut HashMap<String, Entry>, time_to_live: Duration) {
        let now = Instant::now();
        entries.retain(|_, entry| now.duration_since(entry.recorded_at) < time_to_live);
    }

    fn locked_entries(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn locked_sequence(&self) -> std::sync::MutexGuard<'_, u64> {
        self.next_sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use crate::knowledge::{KnowledgeSearchOptions, RetrievalMode};

    use super::*;

    fn plan() -> KnowledgeSearchPlan {
        KnowledgeSearchPlan {
            version: "structured-knowledge-planner/1".to_owned(),
            subjects: Vec::new(),
            scopes: Vec::new(),
            mode: RetrievalMode::FullText,
            options: KnowledgeSearchOptions::default(),
            searches: Vec::new(),
            omitted_searches: 0,
        }
    }

    fn binding(workspace_id: fm_domain::WorkspaceId) -> KnowledgeEvidenceBinding {
        KnowledgeEvidenceBinding {
            tenant_id: "tenant".to_owned(),
            library_id: "library".to_owned(),
            workspace_id,
        }
    }

    fn cached(binding: KnowledgeEvidenceBinding) -> CachedKnowledgeEvidence {
        CachedKnowledgeEvidence {
            binding,
            selection: RagScopeSelection::EntireLibrary,
            plan: plan(),
            evidence: Vec::new(),
            indexed_content_hashes: HashMap::new(),
        }
    }

    #[test]
    fn an_evidence_set_is_only_answerable_from_the_binding_that_produced_it() {
        let cache = KnowledgeEvidenceCache::default();
        let workspace_id = fm_domain::WorkspaceId::new();
        cache.record("sha256:a", cached(binding(workspace_id)));

        assert!(cache.get("sha256:a", &binding(workspace_id)).is_some());
        assert!(
            cache
                .get("sha256:a", &binding(fm_domain::WorkspaceId::new()))
                .is_none(),
            "another workspace must not observe a retained evidence set"
        );
        let other_tenant = KnowledgeEvidenceBinding {
            tenant_id: "other".to_owned(),
            ..binding(workspace_id)
        };
        assert!(cache.get("sha256:a", &other_tenant).is_none());
        let other_library = KnowledgeEvidenceBinding {
            library_id: "other".to_owned(),
            ..binding(workspace_id)
        };
        assert!(cache.get("sha256:a", &other_library).is_none());
    }

    #[test]
    fn retention_is_bounded_by_entry_count_and_evicts_the_oldest_set() {
        let cache = KnowledgeEvidenceCache::new(2, CACHED_EVIDENCE_TTL);
        let workspace_id = fm_domain::WorkspaceId::new();
        for fingerprint in ["sha256:a", "sha256:b", "sha256:c"] {
            cache.record(fingerprint, cached(binding(workspace_id)));
        }

        assert_eq!(cache.len(), 2);
        assert!(cache.get("sha256:a", &binding(workspace_id)).is_none());
        assert!(cache.get("sha256:c", &binding(workspace_id)).is_some());
    }

    #[test]
    fn retention_is_bounded_by_lifetime_so_a_stale_set_cannot_be_answered() {
        let cache = KnowledgeEvidenceCache::new(8, Duration::from_millis(0));
        let workspace_id = fm_domain::WorkspaceId::new();
        cache.record("sha256:a", cached(binding(workspace_id)));

        assert!(cache.get("sha256:a", &binding(workspace_id)).is_none());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn an_empty_fingerprint_is_never_retained() {
        let cache = KnowledgeEvidenceCache::default();
        let workspace_id = fm_domain::WorkspaceId::new();
        cache.record("", cached(binding(workspace_id)));

        assert_eq!(cache.len(), 0);
        cache.clear();
    }
}
