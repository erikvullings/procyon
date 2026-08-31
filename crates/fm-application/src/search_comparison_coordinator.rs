//! Search and directory-comparison coordination (task 0119).

use std::sync::Arc;

use fm_comparison::{ComparisonEngine, ComparisonResultsStore, SyncAction, generate_sync_plan};
use fm_domain::{EntryId, EntryKind, Location, OperationId};
use fm_events::{
    BackendEventPayload, ConflictPolicyPayload, EntryRefPayload, EventAudience, EventBus,
    OperationKindPayload, OperationPayload, OperationProgressDetails, OperationStatePayload,
    SearchExecutionModePayload,
};
use fm_search::{MatchMode, SearchEngine, UnevaluatedPredicate};
use fm_transport_dto::{
    ApplySyncPlanRequestDto, ApplySyncPlanResponseDto, ComparisonPageDto,
    GenerateSyncPlanRequestDto, SearchEntryKindDto, SearchExecutionModeDto, SearchNameModeDto,
    SearchPredicateKindDto, SearchProviderLimitationDto, StartComparisonRequestDto,
    StartComparisonResponseDto, StartSearchRequestDto, StartSearchResponseDto, SyncPlanDto,
};
use uuid::Uuid;

use crate::comparison_mapping::{
    comparison_criteria, comparison_criteria_dto, comparison_entry_dto, sync_action, sync_mode,
    sync_plan_item_dto,
};
use crate::error::ApplicationError;
use crate::operation_requests::{copy_request, delete_request};
use crate::operations_coordinator::OperationsCoordinator;

pub(crate) struct SearchComparisonCoordinator {
    search: SearchEngine,
    comparison: ComparisonEngine,
    comparison_store: Arc<ComparisonResultsStore>,
    events: EventBus,
}

impl SearchComparisonCoordinator {
    pub(crate) fn new(
        search: SearchEngine,
        comparison: ComparisonEngine,
        comparison_store: Arc<ComparisonResultsStore>,
        events: EventBus,
    ) -> Self {
        Self {
            search,
            comparison,
            comparison_store,
            events,
        }
    }

    pub(crate) fn start_search(
        &self,
        request: StartSearchRequestDto,
    ) -> Result<StartSearchResponseDto, ApplicationError> {
        let structured = request.structured_query.as_ref();
        if structured.is_some_and(|query| {
            query.schema_version != fm_transport_dto::search::SEARCH_QUERY_SCHEMA_VERSION
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
            },
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fm_comparison::{ComparisonEngine, ComparisonResultsStore};
    use fm_events::EventBus;
    use fm_search::{SearchEngine, SearchResultsStore};
    use fm_transport_dto::StartSearchRequestDto;
    use fm_vfs::ProviderRegistry;

    use super::SearchComparisonCoordinator;
    use crate::error::ApplicationError;

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
        )
    }

    #[test]
    fn comparison_page_reports_an_unknown_comparison() {
        let error = coordinator()
            .comparison_page(uuid::Uuid::new_v4(), 0, 50, false)
            .expect_err("unknown comparisons must not return an empty page");

        assert_eq!(error, ApplicationError::NotFound);
    }

    #[test]
    fn start_search_rejects_an_invalid_content_regex_before_starting() {
        let error = coordinator()
            .start_search(StartSearchRequestDto {
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
            })
            .expect_err("invalid regex must be rejected");

        assert!(matches!(error, ApplicationError::InvalidRequest(_)));
    }
}
