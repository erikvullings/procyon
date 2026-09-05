//! Search and directory-comparison coordination (task 0119).

use std::sync::Arc;

use fm_comparison::{ComparisonEngine, ComparisonResultsStore, SyncAction, generate_sync_plan};
use fm_domain::{EntryId, EntryKind, EntrySummary, Location, OperationId, ProviderId};
use fm_events::{
    BackendEventPayload, ConflictPolicyPayload, EntryRefPayload, EventAudience, EventBus,
    OperationKindPayload, OperationPayload, OperationProgressDetails, OperationStatePayload,
    SearchExecutionModePayload,
};
use fm_search::{MatchMode, SearchEngine, UnevaluatedPredicate};
use fm_transport_dto::{
    ApplySyncPlanRequestDto, ApplySyncPlanResponseDto, ComparisonPageDto,
    GenerateSyncPlanRequestDto, SearchEntryKindDto, SearchExecutionModeDto, SearchModeDto,
    SearchNameModeDto, SearchPredicateKindDto, SearchProviderLimitationDto, SemanticEvidenceDto,
    SemanticSearchCoverageDto, SemanticSearchResultDto, SemanticSearchScopeDto,
    StartComparisonRequestDto, StartComparisonResponseDto, StartSearchRequestDto,
    StartSearchResponseDto, SyncPlanDto,
};
use uuid::Uuid;

use crate::comparison_mapping::{
    comparison_criteria, comparison_criteria_dto, comparison_entry_dto, sync_action, sync_mode,
    sync_plan_item_dto,
};
use crate::error::ApplicationError;
use crate::operation_requests::{copy_request, delete_request};
use crate::operations_coordinator::OperationsCoordinator;
use crate::semantic::{
    LibraryId, SemanticOperationId, SemanticQuery, SemanticScope, SemanticService, TenantId,
};

pub(crate) struct SearchComparisonCoordinator {
    search: SearchEngine,
    comparison: ComparisonEngine,
    comparison_store: Arc<ComparisonResultsStore>,
    events: EventBus,
    semantic: SemanticService,
}

impl SearchComparisonCoordinator {
    pub(crate) fn new(
        search: SearchEngine,
        comparison: ComparisonEngine,
        comparison_store: Arc<ComparisonResultsStore>,
        events: EventBus,
        semantic: SemanticService,
    ) -> Self {
        Self {
            search,
            comparison,
            comparison_store,
            events,
            semantic,
        }
    }

    pub(crate) fn set_semantic(&mut self, semantic: SemanticService) {
        self.semantic = semantic;
    }

    pub(crate) async fn start_search(
        &self,
        request: StartSearchRequestDto,
        semantic_library: Option<&crate::semantic_library::SemanticLibraryService>,
    ) -> Result<StartSearchResponseDto, ApplicationError> {
        let structured = request.structured_query.as_ref();
        if structured.is_some_and(|query| {
            !matches!(
                query.schema_version,
                1 | fm_transport_dto::search::SEARCH_QUERY_SCHEMA_VERSION
            )
        }) {
            return Err(ApplicationError::InvalidRequest(
                "unsupported search query schema version".to_owned(),
            ));
        }
        let roots: Vec<Location> = structured.map_or_else(
            || request.roots.iter().cloned().map(Into::into).collect(),
            |query| {
                query
                    .scope
                    .locations
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .collect()
            },
        );
        if let Some(query) = structured.filter(|query| query.mode == SearchModeDto::Semantic) {
            return self
                .start_semantic_search(request.workspace_id, query, roots, semantic_library)
                .await;
        }

        let content = structured
            .and_then(|query| query.content.as_ref())
            .map(|predicate| {
                (
                    predicate.query.as_str(),
                    predicate.regex,
                    predicate.case_sensitive,
                    predicate.whole_word,
                )
            })
            .or_else(|| {
                request.content_query.as_deref().map(|query| {
                    (
                        query,
                        request.content_regex,
                        request.content_case_sensitive,
                        request.content_whole_word,
                    )
                })
            });
        let content_query = content
            .as_ref()
            .map(|(query, regex, case_sensitive, whole_word)| {
                fm_vfs::ContentQuery::new(query, *regex, *case_sensitive, *whole_word)
                    .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))
            })
            .transpose()?;
        let search_id = Uuid::new_v4();
        let operation_id = OperationId::from(search_id);
        let audience = EventAudience::Workspace(request.workspace_id.into());
        self.publish_operation(
            audience.clone(),
            operation_id,
            OperationKindPayload::Search,
            roots.clone(),
            None,
        );
        let options = fm_search::SearchOptions {
            filename_query: structured
                .and_then(|query| query.name.as_ref())
                .map_or(request.query, |name| name.pattern.clone()),
            filename_mode: structured
                .and_then(|query| query.name.as_ref())
                .map(|name| match name.mode {
                    SearchNameModeDto::Substring => MatchMode::Substring,
                    SearchNameModeDto::Glob => MatchMode::Glob,
                }),
            filename_case_sensitive: structured
                .and_then(|query| query.name.as_ref())
                .is_some_and(|name| name.case_sensitive),
            content_query,
            recurse: structured.map_or(request.recurse, |query| query.scope.recurse),
            show_hidden: structured.map_or(request.show_hidden, |query| query.scope.show_hidden),
            operation_id: Some(operation_id),
            entry_kinds: structured.map_or_else(
                || vec![EntryKind::File],
                |query| {
                    query
                        .entry_kinds
                        .iter()
                        .map(|kind| match kind {
                            SearchEntryKindDto::File => EntryKind::File,
                            SearchEntryKindDto::Directory => EntryKind::Directory,
                            SearchEntryKindDto::Symlink => EntryKind::Symlink,
                        })
                        .collect()
                },
            ),
            mime_types: structured.map_or_else(Vec::new, |query| query.mime_types.clone()),
            min_size_bytes: structured.and_then(|query| query.min_size_bytes),
            max_size_bytes: structured.and_then(|query| query.max_size_bytes),
            modified_after: structured.and_then(|query| query.modified_after),
            modified_before: structured.and_then(|query| query.modified_before),
            // Git status, tags, and arbitrary metadata are represented in the
            // query but explicitly reported as provider limitations for now.
            git_statuses: Vec::new(),
        };
        let limitations = self
            .search
            .limitations(
                &roots,
                content.is_some(),
                structured.is_some_and(|query| !query.git_statuses.is_empty()),
                structured.is_some_and(|query| !query.tags.is_empty()),
                structured.is_some_and(|query| !query.metadata.is_empty()),
            )
            .into_iter()
            .map(|limitation| SearchProviderLimitationDto {
                provider_id: limitation.provider_id.as_str().to_owned(),
                unevaluated_predicates: limitation
                    .predicates
                    .into_iter()
                    .map(|predicate| match predicate {
                        UnevaluatedPredicate::Content => SearchPredicateKindDto::Content,
                        UnevaluatedPredicate::GitStatus => SearchPredicateKindDto::GitStatus,
                        UnevaluatedPredicate::Tags => SearchPredicateKindDto::Tags,
                        UnevaluatedPredicate::Metadata => SearchPredicateKindDto::Metadata,
                    })
                    .collect(),
            })
            .collect();
        let started = self
            .search
            .start(search_id, roots, options, audience)
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        Ok(StartSearchResponseDto {
            search_id,
            location: started.location.into(),
            limitations,
            execution_mode: match started.execution_mode {
                SearchExecutionModePayload::Indexed => SearchExecutionModeDto::Indexed,
                SearchExecutionModePayload::LiveRecursive => SearchExecutionModeDto::LiveRecursive,
                SearchExecutionModePayload::Mixed => SearchExecutionModeDto::Mixed,
                SearchExecutionModePayload::Semantic => SearchExecutionModeDto::Semantic,
            },
            semantic_results: Vec::new(),
            semantic_coverage: None,
        })
    }

    async fn start_semantic_search(
        &self,
        workspace_id: Uuid,
        query: &fm_transport_dto::SearchQueryDto,
        roots: Vec<Location>,
        semantic_library: Option<&crate::semantic_library::SemanticLibraryService>,
    ) -> Result<StartSearchResponseDto, ApplicationError> {
        let semantic = query.semantic.as_ref().ok_or_else(|| {
            ApplicationError::InvalidRequest(
                "semantic mode requires a semantic predicate".to_owned(),
            )
        })?;
        if semantic.query.trim().is_empty() {
            return Err(ApplicationError::InvalidRequest(
                "semantic query must not be empty".to_owned(),
            ));
        }
        if query.name.is_some() || query.content.is_some() {
            return Err(ApplicationError::InvalidRequest(
                "semantic mode cannot be combined with name or content predicates".to_owned(),
            ));
        }
        let search_id = Uuid::new_v4();
        let results = self
            .semantic
            .query(SemanticQuery {
                scope: SemanticScope::new(
                    TenantId::new(workspace_id.to_string()),
                    LibraryId::new(semantic.library_id.clone()),
                ),
                request_id: SemanticOperationId::new(search_id.to_string()),
                text: semantic.query.clone(),
                maximum_results: 500,
            })
            .await
            .map_err(|error| match error {
                crate::semantic::SemanticError::Unavailable => {
                    ApplicationError::ProviderUnavailable
                }
                _ => ApplicationError::InvalidRequest(error.to_string()),
            })?;
        let mut entries = Vec::new();
        let mut semantic_results = Vec::new();
        let mut coverage = None;
        for mut result in results {
            if !result.metadata.contains_key("uri")
                && let Some(library) = semantic_library
                && let Some(source_id) = semantic_source_ids(&result).into_iter().next()
                && let Ok(Some(occurrence)) = library.resolve_occurrence(
                    &crate::semantic_library::SemanticAccessContext::Host,
                    workspace_id.into(),
                    &source_id,
                )
            {
                result.metadata.insert(
                    "provider_id".to_owned(),
                    occurrence.location.provider_id.as_str().to_owned(),
                );
                result
                    .metadata
                    .insert("uri".to_owned(), occurrence.location.uri.clone());
                result
                    .metadata
                    .insert("entry_id".to_owned(), occurrence.entry_id.to_string());
                result
                    .metadata
                    .insert("available".to_owned(), occurrence.available.to_string());
            }
            if coverage.is_none() {
                coverage = semantic_coverage(&result);
            }

            fn semantic_source_ids(result: &crate::semantic::SemanticSearchResult) -> Vec<String> {
                serde_json::from_str::<Vec<fm_semantic_worker::semantic_search::SemanticEvidence>>(
                    result
                        .metadata
                        .get("semantic.evidence")
                        .map(String::as_str)
                        .unwrap_or("[]"),
                )
                .unwrap_or_default()
                .into_iter()
                .map(|evidence| evidence.source_id)
                .chain(
                    result
                        .metadata
                        .get("semantic.additionalSourceIds")
                        .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
                        .unwrap_or_default(),
                )
                .collect()
            }
            if let Some(entry) = semantic_result_entry(&result, query, &roots) {
                if let Some(evidence) =
                    semantic_result_dto(&result, entry.id, entry.location.clone())
                {
                    semantic_results.push(evidence);
                }
                entries.push(entry);
            }
        }
        let started = self.search.start_materialized(search_id, entries);
        Ok(StartSearchResponseDto {
            search_id,
            location: started.location.into(),
            limitations: Vec::new(),
            execution_mode: SearchExecutionModeDto::Semantic,
            semantic_results,
            semantic_coverage: coverage,
        })
    }

    pub(crate) fn cancel_search(&self, search_id: Uuid) -> Result<(), ApplicationError> {
        self.search
            .cancel(search_id)
            .map_err(|_| ApplicationError::NotFound)
    }

    pub(crate) fn start_comparison(
        &self,
        request: StartComparisonRequestDto,
    ) -> Result<StartComparisonResponseDto, ApplicationError> {
        let left: Location = request.left.into();
        let right: Location = request.right.into();
        let comparison_id = Uuid::new_v4();
        let operation_id = OperationId::from(comparison_id);
        let audience = EventAudience::Workspace(request.workspace_id.into());
        self.publish_operation(
            audience.clone(),
            operation_id,
            OperationKindPayload::Compare,
            vec![left.clone()],
            Some(right.clone()),
        );
        let options = fm_comparison::ComparisonOptions {
            criteria: comparison_criteria(request.criteria),
            show_hidden: request.show_hidden,
            operation_id: Some(operation_id),
        };
        self.comparison
            .start(comparison_id, left, right, options, audience)
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        Ok(StartComparisonResponseDto { comparison_id })
    }

    pub(crate) fn cancel_comparison(&self, comparison_id: Uuid) -> Result<(), ApplicationError> {
        self.comparison
            .cancel(comparison_id)
            .map_err(|_| ApplicationError::NotFound)
    }

    pub(crate) fn comparison_page(
        &self,
        comparison_id: Uuid,
        offset: u64,
        limit: u16,
        differences_only: bool,
    ) -> Result<ComparisonPageDto, ApplicationError> {
        let limit = limit.clamp(1, 500);
        let page = self
            .comparison_store
            .page(
                comparison_id,
                usize::try_from(offset).unwrap_or(usize::MAX),
                usize::from(limit),
                differences_only,
            )
            .ok_or(ApplicationError::NotFound)?;
        Ok(ComparisonPageDto {
            comparison_id,
            left: page.left_root.into(),
            right: page.right_root.into(),
            criteria: comparison_criteria_dto(page.criteria),
            offset,
            limit,
            total: u64::try_from(page.total).unwrap_or(u64::MAX),
            entries: page.entries.iter().map(comparison_entry_dto).collect(),
            is_complete: page.is_complete,
            warnings_count: page.warnings_count,
        })
    }

    pub(crate) fn generate_sync_plan(
        &self,
        comparison_id: Uuid,
        request: GenerateSyncPlanRequestDto,
    ) -> Result<SyncPlanDto, ApplicationError> {
        let entries = self
            .comparison_store
            .all_entries(comparison_id)
            .ok_or(ApplicationError::NotFound)?;
        let items = generate_sync_plan(&entries, sync_mode(request.mode));
        Ok(SyncPlanDto {
            comparison_id,
            items: items.iter().map(sync_plan_item_dto).collect(),
        })
    }

    pub(crate) fn apply_sync_plan(
        &self,
        comparison_id: Uuid,
        request: ApplySyncPlanRequestDto,
        operations: &OperationsCoordinator,
    ) -> Result<ApplySyncPlanResponseDto, ApplicationError> {
        let (left_root, right_root) = self
            .comparison_store
            .roots(comparison_id)
            .ok_or(ApplicationError::NotFound)?;
        let mut operation_ids = Vec::with_capacity(request.items.len());
        for item in request.items {
            let start_request = match sync_action(item.action) {
                SyncAction::Skip => continue,
                SyncAction::CopyLeftToRight => {
                    copy_request(&left_root, &right_root, &item.relative_path)?
                }
                SyncAction::CopyRightToLeft => {
                    copy_request(&right_root, &left_root, &item.relative_path)?
                }
                SyncAction::DeleteLeft => delete_request(&left_root, &item.relative_path)?,
                SyncAction::DeleteRight => delete_request(&right_root, &item.relative_path)?,
            };
            operation_ids.push(operations.start(start_request, None)?.id);
        }
        Ok(ApplySyncPlanResponseDto { operation_ids })
    }

    fn publish_operation(
        &self,
        audience: EventAudience,
        operation_id: OperationId,
        kind: OperationKindPayload,
        sources: Vec<Location>,
        destination: Option<Location>,
    ) {
        self.events.publish(
            audience,
            BackendEventPayload::OperationCreated {
                operation: OperationPayload {
                    id: operation_id,
                    kind,
                    state: OperationStatePayload::Running,
                    sources: sources
                        .into_iter()
                        .map(|location| EntryRefPayload {
                            id: EntryId::from(operation_id.into_inner()),
                            location: location.into(),
                        })
                        .collect(),
                    destination: destination.map(Into::into),
                    progress: OperationProgressDetails {
                        completed_items: 0,
                        total_items: None,
                        completed_bytes: 0,
                        total_bytes: None,
                        current_entry: None,
                        bytes_per_second: None,
                    },
                    conflict_policy: ConflictPolicyPayload::Ask,
                    created_at: chrono::Utc::now(),
                    started_at: None,
                    completed_at: None,
                    undo: None,
                    undo_of: None,
                },
            },
        );
    }
}

fn semantic_result_entry(
    result: &crate::semantic::SemanticSearchResult,
    query: &fm_transport_dto::SearchQueryDto,
    roots: &[Location],
) -> Option<EntrySummary> {
    let provider_id = result.metadata.get("provider_id")?;
    let uri = result.metadata.get("uri")?;
    let location = Location::try_new(ProviderId::new(provider_id), uri.clone()).ok()?;
    if query
        .semantic
        .as_ref()
        .is_some_and(|semantic| semantic.scope == SemanticSearchScopeDto::CurrentFolder)
        && !roots.iter().any(|root| location_is_within(&location, root))
    {
        return None;
    }
    let mime_type = result.metadata.get("media_type").cloned();
    if !query.mime_types.is_empty()
        && !mime_type.as_deref().is_some_and(|actual| {
            query
                .mime_types
                .iter()
                .any(|expected| mime_matches(expected, actual))
        })
    {
        return None;
    }
    let modified_at = result
        .metadata
        .get("modified_at")
        .and_then(|value| value.parse().ok())
        .or_else(|| {
            result
                .metadata
                .get("modified_at_ms")
                .and_then(|value| value.parse::<i64>().ok())
                .and_then(chrono::DateTime::from_timestamp_millis)
        });
    if query
        .modified_after
        .is_some_and(|minimum| modified_at.is_none_or(|actual| actual < minimum))
        || query
            .modified_before
            .is_some_and(|maximum| modified_at.is_none_or(|actual| actual > maximum))
    {
        return None;
    }
    let name = result
        .metadata
        .get("name")
        .cloned()
        .or_else(|| location.name().ok())
        .unwrap_or_else(|| result.document_id.as_str().to_owned());
    Some(EntrySummary {
        id: result
            .metadata
            .get("entry_id")
            .and_then(|value| value.parse().ok())
            .unwrap_or_default(),
        location,
        name,
        kind: EntryKind::File,
        size: result
            .metadata
            .get("size_bytes")
            .and_then(|value| value.parse().ok()),
        modified_at,
        created_at: None,
        hidden: false,
        read_only: false,
        extension: result.metadata.get("extension").cloned(),
        mime_type,
        icon_key: None,
        metadata_revision: 0,
        git_status: None,
    })
}

fn location_is_within(candidate: &Location, root: &Location) -> bool {
    if candidate.provider_id != root.provider_id {
        return false;
    }
    let mut current = Some(candidate.clone());
    while let Some(location) = current {
        if location == *root {
            return true;
        }
        current = location.parent().ok().flatten();
    }
    false
}

fn mime_matches(expected: &str, actual: &str) -> bool {
    expected == actual
        || expected.strip_suffix("/*").is_some_and(|prefix| {
            actual.starts_with(prefix) && actual.as_bytes().get(prefix.len()) == Some(&b'/')
        })
}

fn semantic_coverage(
    result: &crate::semantic::SemanticSearchResult,
) -> Option<SemanticSearchCoverageDto> {
    let coverage: fm_semantic_worker::semantic_search::SearchCoverage =
        serde_json::from_str(result.metadata.get("semantic.coverage")?).ok()?;
    Some(SemanticSearchCoverageDto {
        eligible: coverage.eligible,
        indexed: coverage.indexed,
        stale: coverage.stale,
        pending: coverage.pending,
        excluded: coverage.excluded,
        skipped: coverage.skipped,
        failed: coverage.failed,
        unavailable: coverage.unavailable,
        partial: coverage.partial(),
    })
}

fn semantic_result_dto(
    result: &crate::semantic::SemanticSearchResult,
    entry_id: EntryId,
    location: Location,
) -> Option<SemanticSearchResultDto> {
    let evidence: Vec<fm_semantic_worker::semantic_search::SemanticEvidence> =
        serde_json::from_str(result.metadata.get("semantic.evidence")?).ok()?;
    let mut evidence = evidence.into_iter();
    let best_evidence = semantic_evidence_dto(evidence.next()?)?;
    Some(SemanticSearchResultDto {
        entry_id: entry_id.into_inner(),
        location: location.into(),
        score: result.score as f32,
        best_evidence,
        additional_evidence: evidence
            .map(semantic_evidence_dto)
            .collect::<Option<Vec<_>>>()?,
        additional_source_ids: result
            .metadata
            .get("semantic.additionalSourceIds")
            .and_then(|value| serde_json::from_str(value).ok())
            .unwrap_or_default(),
    })
}

fn semantic_evidence_dto(
    evidence: fm_semantic_worker::semantic_search::SemanticEvidence,
) -> Option<SemanticEvidenceDto> {
    Some(SemanticEvidenceDto {
        record_id: evidence.record_id,
        source_id: evidence.source_id,
        score: evidence.score,
        chunk_kind: evidence.chunk_kind,
        excerpt: evidence.excerpt,
        provenance_json: serde_json::to_string(&evidence.provenance).ok()?,
        indexed_content_hash: evidence.indexed_content_hash,
        generation: evidence.generation,
        available: !evidence.unavailable,
        stale: evidence.stale,
        generated: evidence.generated,
        source_position: evidence.source_position,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use fm_comparison::{ComparisonEngine, ComparisonResultsStore};
    use fm_events::EventBus;
    use fm_search::{SearchEngine, SearchResultsStore};
    use fm_transport_dto::{
        SearchExecutionModeDto, SearchModeDto, SearchQueryDto, SearchScopeDto,
        SearchSemanticPredicateDto, SemanticSearchScopeDto, StartSearchRequestDto,
    };
    use fm_vfs::ProviderRegistry;
    use uuid::Uuid;

    use super::SearchComparisonCoordinator;
    use crate::error::ApplicationError;
    use crate::semantic::{
        DocumentId, DocumentIngestion, FakeSemanticCapability, LibraryId, SemanticCapability,
        SemanticOperationId, SemanticScope, SemanticService, TenantId,
    };

    fn coordinator() -> SearchComparisonCoordinator {
        let providers = ProviderRegistry::new();
        let events = EventBus::default();
        let search_store = Arc::new(SearchResultsStore::new());
        let comparison_store = Arc::new(ComparisonResultsStore::new());
        SearchComparisonCoordinator::new(
            SearchEngine::new(search_store, events.clone(), providers.clone()),
            ComparisonEngine::new(Arc::clone(&comparison_store), events.clone(), providers),
            comparison_store,
            events,
            crate::semantic::SemanticService::unavailable(),
        )
    }

    #[tokio::test]
    async fn semantic_mode_uses_the_existing_paged_virtual_location() {
        let providers = ProviderRegistry::new();
        let events = EventBus::default();
        let search_store = Arc::new(SearchResultsStore::new());
        let comparison_store = Arc::new(ComparisonResultsStore::new());
        let capability = Arc::new(FakeSemanticCapability::new());
        let workspace_id = Uuid::new_v4();
        let library_id = "library-1";
        capability
            .ingest(DocumentIngestion {
                scope: SemanticScope::new(
                    TenantId::new(workspace_id.to_string()),
                    LibraryId::new(library_id),
                ),
                operation_id: SemanticOperationId::new("ingest-1"),
                document_id: DocumentId::new("document-1"),
                metadata: BTreeMap::from([
                    ("provider_id".to_owned(), "local".to_owned()),
                    ("uri".to_owned(), "file:///library/report.pdf".to_owned()),
                    ("name".to_owned(), "report.pdf".to_owned()),
                    ("media_type".to_owned(), "application/pdf".to_owned()),
                ]),
                media_type: "application/pdf".to_owned(),
                content: b"multilingual semantic retrieval".to_vec(),
            })
            .await
            .expect("ingest fixture");
        let coordinator = SearchComparisonCoordinator::new(
            SearchEngine::new(Arc::clone(&search_store), events.clone(), providers.clone()),
            ComparisonEngine::new(Arc::clone(&comparison_store), events.clone(), providers),
            comparison_store,
            events,
            SemanticService::new(capability),
        );
        let response = coordinator
            .start_search(
                StartSearchRequestDto {
                    workspace_id,
                    roots: vec![fm_transport_dto::LocationDto {
                        provider_id: "local".to_owned(),
                        uri: "file:///library".to_owned(),
                    }],
                    query: String::new(),
                    content_query: None,
                    content_regex: false,
                    content_case_sensitive: false,
                    content_whole_word: false,
                    recurse: true,
                    show_hidden: false,
                    structured_query: Some(SearchQueryDto {
                        schema_version: 2,
                        mode: SearchModeDto::Semantic,
                        scope: SearchScopeDto {
                            locations: vec![fm_transport_dto::LocationDto {
                                provider_id: "local".to_owned(),
                                uri: "file:///library".to_owned(),
                            }],
                            recurse: true,
                            show_hidden: false,
                        },
                        name: None,
                        entry_kinds: Vec::new(),
                        mime_types: vec!["application/pdf".to_owned()],
                        min_size_bytes: None,
                        max_size_bytes: None,
                        modified_after: None,
                        modified_before: None,
                        content: None,
                        semantic: Some(SearchSemanticPredicateDto {
                            query: "semantic retrieval".to_owned(),
                            library_id: library_id.to_owned(),
                            scope: SemanticSearchScopeDto::CurrentFolder,
                            enrolled_root_ids: Vec::new(),
                        }),
                        git_statuses: Vec::new(),
                        tags: Vec::new(),
                        metadata: BTreeMap::new(),
                    }),
                },
                None,
            )
            .await
            .expect("start semantic search");

        assert_eq!(response.execution_mode, SearchExecutionModeDto::Semantic);
        let (entries, more) = search_store
            .page(response.search_id, 0, 10)
            .expect("registered search");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "report.pdf");
        assert!(!more);
    }

    #[test]
    fn comparison_page_reports_an_unknown_comparison() {
        let error = coordinator()
            .comparison_page(uuid::Uuid::new_v4(), 0, 50, false)
            .expect_err("unknown comparisons must not return an empty page");

        assert_eq!(error, ApplicationError::NotFound);
    }

    #[tokio::test]
    async fn start_search_rejects_an_invalid_content_regex_before_starting() {
        let error = coordinator()
            .start_search(
                StartSearchRequestDto {
                    workspace_id: uuid::Uuid::new_v4(),
                    roots: Vec::new(),
                    query: String::new(),
                    content_query: Some("(".to_owned()),
                    content_regex: true,
                    content_case_sensitive: false,
                    content_whole_word: false,
                    recurse: true,
                    show_hidden: true,
                    structured_query: None,
                },
                None,
            )
            .await
            .expect_err("invalid regex must be rejected");

        assert!(matches!(error, ApplicationError::InvalidRequest(_)));
    }
}
