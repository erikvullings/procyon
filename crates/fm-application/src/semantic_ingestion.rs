//! Host-side scheduling and reconciliation decisions for semantic ingestion.
//!
//! Providers and paths remain in the host. This module turns native watcher,
//! delta-API, polling, startup, and manual-refresh observations into bounded
//! path-free work for the semantic worker.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use fm_domain::{EntryId, Location};
use fm_events::{BackendEventPayload, EventAudience, EventBus};
use fm_semantic_worker::ingestion::{IngestionEventSink, IngestionProgress};

/// Default interval that repairs missed/coalesced filesystem events.
pub const DEFAULT_SEMANTIC_RECONCILIATION_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Projects worker progress through the shared HTTP/Tauri event bus.
pub struct SemanticIngestionEventPublisher {
    events: EventBus,
    audience: EventAudience,
}

impl SemanticIngestionEventPublisher {
    /// Creates a publisher for an already-authorized event audience.
    #[must_use]
    pub fn new(events: EventBus, audience: EventAudience) -> Self {
        Self { events, audience }
    }

    /// Publishes aggregate root coverage without source paths or excerpts.
    pub fn publish_coverage(&self, coverage: SemanticCoverage) {
        self.events.publish(
            self.audience.clone(),
            BackendEventPayload::SemanticCoverageChanged {
                library_id: coverage.library_id,
                root_id: coverage.root_id,
                eligible: coverage.eligible,
                indexed: coverage.indexed,
                stale: coverage.stale,
                available: coverage.available,
            },
        );
    }
}

impl IngestionEventSink for SemanticIngestionEventPublisher {
    fn publish(&self, event: IngestionProgress) {
        self.events.publish(
            self.audience.clone(),
            BackendEventPayload::SemanticIngestionProgress {
                job_id: event.job_id,
                stage: event.stage.as_str().to_owned(),
                completed: event.completed,
                total: event.total,
                errors: event.errors,
            },
        );
    }
}

/// Sanitized aggregate coverage for one enrolled root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCoverage {
    /// Enrolled semantic library.
    pub library_id: String,
    /// Enrolled root identity.
    pub root_id: String,
    /// Eligible source count.
    pub eligible: u64,
    /// Sources represented by complete generations.
    pub indexed: u64,
    /// Sources whose visible generation no longer matches current bytes.
    pub stale: u64,
    /// Whether the source root is reachable.
    pub available: bool,
}

/// Cheap provider metadata used only to decide whether content must be hashed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStamp {
    /// Provider-stable entry identity.
    pub entry_id: EntryId,
    /// Current provider location, retained only by the host.
    pub location: Location,
    /// Reported source size.
    pub size: Option<u64>,
    /// Reported modification time in Unix milliseconds.
    pub modified_at_ms: Option<i64>,
}

/// Last successfully catalogued source state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownSource {
    /// Cheap metadata from the last complete observation.
    pub stamp: SourceStamp,
    /// Streamed content digest that established truth.
    pub content_hash: String,
}

/// Why a source should be re-hashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashReason {
    /// New provider entry.
    Discovered,
    /// Size or modification metadata changed.
    MetadataChanged,
    /// A watcher/delta event may indicate a timestamp lie.
    ProviderEvent,
}

/// Freshness returned when a user opens evidence from a published generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceFreshness {
    /// Indexed evidence still represents the current source bytes.
    Current,
    /// Source bytes changed after the visible generation was published.
    Stale {
        /// Safe user-facing warning without source content.
        warning: &'static str,
    },
}

/// Compares authoritative streamed hashes before opening semantic evidence.
#[must_use]
pub fn evidence_freshness(indexed_hash: &str, current_hash: &str) -> EvidenceFreshness {
    if indexed_hash == current_hash {
        EvidenceFreshness::Current
    } else {
        EvidenceFreshness::Stale {
            warning: "This result was indexed from an older version of the source.",
        }
    }
}

/// Reconciliation action for one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconciliationAction {
    /// Stream and hash source bytes before deciding whether to ingest.
    Hash {
        /// Current source stamp.
        source: SourceStamp,
        /// Why hashing is required.
        reason: HashReason,
    },
    /// Stable content moved without requiring re-embedding.
    MoveOccurrence {
        /// Stable provider identity.
        entry_id: EntryId,
        /// Previous location.
        from: Location,
        /// Current location.
        to: Location,
    },
    /// A complete listing proved this source absent.
    DeleteOccurrence {
        /// Stable provider identity.
        entry_id: EntryId,
    },
    /// Root could not be listed; existing evidence remains unavailable.
    MarkRootUnavailable {
        /// Sanitized provider failure.
        reason: String,
    },
}

/// Result of one provider enumeration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootListing {
    /// Complete authoritative listing.
    Complete(Vec<SourceStamp>),
    /// Partial listing. Its entries may update, but absence proves nothing.
    Partial(Vec<SourceStamp>),
    /// Root is temporarily offline.
    Unavailable {
        /// Sanitized provider failure.
        reason: String,
    },
}

/// Pure watch/reconciliation planner.
#[derive(Debug, Default)]
pub struct ReconciliationPlanner {
    event_candidates: HashMap<EntryId, SourceStamp>,
}

impl ReconciliationPlanner {
    /// Coalesces repeated provider notifications by stable entry identity.
    pub fn observe_provider_event(&mut self, source: SourceStamp) {
        self.event_candidates.insert(source.entry_id, source);
    }

    /// Drains coalesced near-immediate hash candidates.
    #[must_use]
    pub fn drain_provider_events(&mut self) -> Vec<ReconciliationAction> {
        std::mem::take(&mut self.event_candidates)
            .into_values()
            .map(|source| ReconciliationAction::Hash {
                source,
                reason: HashReason::ProviderEvent,
            })
            .collect()
    }

    /// Compares a listing with known state.
    ///
    /// Only [`RootListing::Complete`] can produce deletions. Cheap metadata
    /// avoids unnecessary hashing, but provider events still hash unchanged
    /// metadata so timestamp lies cannot suppress content changes.
    #[must_use]
    pub fn reconcile(
        &self,
        known: &[KnownSource],
        listing: RootListing,
    ) -> Vec<ReconciliationAction> {
        let known = known
            .iter()
            .map(|source| (source.stamp.entry_id, source))
            .collect::<HashMap<_, _>>();
        let (observed, complete) = match listing {
            RootListing::Complete(observed) => (observed, true),
            RootListing::Partial(observed) => (observed, false),
            RootListing::Unavailable { reason } => {
                return vec![ReconciliationAction::MarkRootUnavailable { reason }];
            }
        };
        let observed_ids = observed
            .iter()
            .map(|source| source.entry_id)
            .collect::<HashSet<_>>();
        let mut actions = Vec::new();
        for source in observed {
            match known.get(&source.entry_id) {
                None => actions.push(ReconciliationAction::Hash {
                    source,
                    reason: HashReason::Discovered,
                }),
                Some(previous)
                    if previous.stamp.size != source.size
                        || previous.stamp.modified_at_ms != source.modified_at_ms =>
                {
                    actions.push(ReconciliationAction::Hash {
                        source,
                        reason: HashReason::MetadataChanged,
                    });
                }
                Some(previous) if previous.stamp.location != source.location => {
                    actions.push(ReconciliationAction::MoveOccurrence {
                        entry_id: source.entry_id,
                        from: previous.stamp.location.clone(),
                        to: source.location,
                    });
                }
                Some(_) => {}
            }
        }
        if complete {
            actions.extend(
                known
                    .keys()
                    .filter(|entry_id| !observed_ids.contains(entry_id))
                    .map(|entry_id| ReconciliationAction::DeleteOccurrence {
                        entry_id: *entry_id,
                    }),
            );
        }
        actions
    }

    /// Resolves a streamed hash against catalogued truth.
    #[must_use]
    pub fn content_changed(known: Option<&KnownSource>, streamed_hash: &str) -> bool {
        known.is_none_or(|known| known.content_hash != streamed_hash)
    }
}

/// Startup/periodic/manual reconciliation clock.
#[derive(Debug)]
pub struct ReconciliationSchedule {
    interval: Duration,
    next_due: Instant,
    manual_requested: bool,
}

impl ReconciliationSchedule {
    /// Creates a schedule due immediately on worker startup.
    #[must_use]
    pub fn new(now: Instant, interval: Duration) -> Self {
        Self {
            interval,
            next_due: now,
            manual_requested: false,
        }
    }

    /// Creates the default 30-minute startup schedule.
    #[must_use]
    pub fn default_at(now: Instant) -> Self {
        Self::new(now, DEFAULT_SEMANTIC_RECONCILIATION_INTERVAL)
    }

    /// Requests the same complete reconciliation used by periodic repair.
    pub fn request_manual_refresh(&mut self) {
        self.manual_requested = true;
    }

    /// Takes one due reconciliation and schedules the next interval.
    #[must_use]
    pub fn take_due(&mut self, now: Instant) -> bool {
        if !self.manual_requested && now < self.next_due {
            return false;
        }
        self.manual_requested = false;
        self.next_due = now + self.interval;
        true
    }
}

#[cfg(test)]
mod tests {
    use fm_domain::ProviderId;

    use super::*;

    fn stamp(id: EntryId, uri: &str, size: u64, modified_at_ms: i64) -> SourceStamp {
        SourceStamp {
            entry_id: id,
            location: Location::new(ProviderId::new("local"), uri),
            size: Some(size),
            modified_at_ms: Some(modified_at_ms),
        }
    }

    #[test]
    fn repeated_events_coalesce_but_still_hash_timestamp_lies() {
        let id = EntryId::new();
        let mut planner = ReconciliationPlanner::default();
        planner.observe_provider_event(stamp(id, "file:///a.txt", 4, 10));
        planner.observe_provider_event(stamp(id, "file:///a.txt", 4, 10));

        let actions = planner.drain_provider_events();

        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            ReconciliationAction::Hash {
                reason: HashReason::ProviderEvent,
                ..
            }
        ));
    }

    #[test]
    fn unsorted_complete_listing_finds_missed_changes_moves_and_deletions() {
        let unchanged_id = EntryId::new();
        let moved_id = EntryId::new();
        let changed_id = EntryId::new();
        let deleted_id = EntryId::new();
        let known = vec![
            KnownSource {
                stamp: stamp(moved_id, "file:///old.txt", 3, 10),
                content_hash: "move-hash".into(),
            },
            KnownSource {
                stamp: stamp(deleted_id, "file:///deleted.txt", 2, 5),
                content_hash: "deleted-hash".into(),
            },
            KnownSource {
                stamp: stamp(unchanged_id, "file:///same.txt", 1, 1),
                content_hash: "same-hash".into(),
            },
            KnownSource {
                stamp: stamp(changed_id, "file:///changed.txt", 5, 7),
                content_hash: "old-hash".into(),
            },
        ];
        let listing = RootListing::Complete(vec![
            stamp(changed_id, "file:///changed.txt", 8, 9),
            stamp(unchanged_id, "file:///same.txt", 1, 1),
            stamp(moved_id, "file:///new.txt", 3, 10),
        ]);

        let actions = ReconciliationPlanner::default().reconcile(&known, listing);

        assert_eq!(actions.len(), 3);
        assert!(actions.iter().any(|action| matches!(
            action,
            ReconciliationAction::Hash {
                source,
                reason: HashReason::MetadataChanged
            } if source.entry_id == changed_id
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            ReconciliationAction::MoveOccurrence { entry_id, .. } if *entry_id == moved_id
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            ReconciliationAction::DeleteOccurrence { entry_id } if *entry_id == deleted_id
        )));
    }

    #[test]
    fn partial_and_offline_listings_never_delete_evidence() {
        let id = EntryId::new();
        let known = vec![KnownSource {
            stamp: stamp(id, "file:///retained.txt", 1, 1),
            content_hash: "hash".into(),
        }];
        assert!(
            ReconciliationPlanner::default()
                .reconcile(&known, RootListing::Partial(Vec::new()))
                .is_empty()
        );
        assert!(matches!(
            ReconciliationPlanner::default()
                .reconcile(
                    &known,
                    RootListing::Unavailable {
                        reason: "offline".into()
                    }
                )
                .as_slice(),
            [ReconciliationAction::MarkRootUnavailable { reason }] if reason == "offline"
        ));
    }

    #[test]
    fn streamed_hash_not_timestamp_establishes_content_truth() {
        let known = KnownSource {
            stamp: stamp(EntryId::new(), "file:///a.txt", 4, 10),
            content_hash: "old".into(),
        };
        assert!(!ReconciliationPlanner::content_changed(Some(&known), "old"));
        assert!(ReconciliationPlanner::content_changed(Some(&known), "new"));
        assert_eq!(evidence_freshness("old", "old"), EvidenceFreshness::Current);
        assert!(matches!(
            evidence_freshness("old", "new"),
            EvidenceFreshness::Stale { .. }
        ));
    }

    #[test]
    fn startup_periodic_and_manual_refresh_share_one_schedule() {
        let start = Instant::now();
        let mut schedule = ReconciliationSchedule::default_at(start);
        assert!(schedule.take_due(start));
        assert!(!schedule.take_due(start + Duration::from_secs(1)));
        schedule.request_manual_refresh();
        assert!(schedule.take_due(start + Duration::from_secs(2)));
        assert!(!schedule.take_due(start + Duration::from_secs(29 * 60)));
        assert!(schedule.take_due(start + Duration::from_secs(31 * 60)));
    }
}
