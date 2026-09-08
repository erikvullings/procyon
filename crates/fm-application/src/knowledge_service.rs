//! Host-facing Structured Knowledge Search capability.
//!
//! This module owns the orchestration that turns a transport request into an
//! authorized, deterministically planned, exactly scoped retrieval and back
//! into transport DTOs. [`crate::service::FileManagerService`] only delegates
//! to it, and the coordinator in [`crate::knowledge_search`] stays free of any
//! knowledge of workspaces, consent policy, or the file manager service.
//!
//! Authorization is resolved through the narrow [`KnowledgeAuthority`] port, is
//! re-resolved immediately before evidence is projected, and always wins over
//! whatever the worker index believes.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use fm_semantic_worker::knowledge_retrieval::{KnowledgeSourceRestriction, MAX_ALLOWED_SOURCES};
use fm_semantic_worker::semantic_storage::QueryFilters;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ApplicationError;
use crate::knowledge::{
    KnowledgePlanner, KnowledgeScope, KnowledgeScopeSelector, KnowledgeSearchOptions,
    KnowledgeSearchRequest, KnowledgeSubject,
};
use crate::knowledge_answer::{
    InspectedKnowledgeEvidence, KnowledgeAnswerCoordinator, KnowledgeAnswerIntent,
};
use crate::knowledge_dsl::{KnowledgeDslScopeContext, KnowledgeQueryDraft};
use crate::knowledge_evidence_cache::{
    CachedKnowledgeEvidence, KnowledgeEvidenceBinding, KnowledgeEvidenceCache,
};
use crate::knowledge_mapping::{
    answer_error_to_application, answer_evidence_from_dto, answer_request_from_dto, answer_to_dto,
    capabilities_to_dto, coverage_to_dto, draft_from_dto, evidence_to_dto, excluded_fields,
    interpretation_to_dto, mode_from_dto, options_from_dto, plan_to_dto, route_outcome_to_dto,
    trace_to_dto,
};
use crate::knowledge_search::{
    AuthorizedKnowledgeSearch, KnowledgeAuthorizationRefresh, KnowledgeAuthorizationSnapshot,
    KnowledgeRetrievalCapability, KnowledgeRetrievalPartition, KnowledgeSearchCoordinator,
    KnowledgeSearchError,
};
use crate::llm_profiles::LlmProfileService;
use crate::semantic_library::{
    RagScopeSelection, ResolvedKnowledgeScope, ResolvedSemanticOccurrence, SemanticAccessContext,
    SemanticLibraryError, SemanticLibraryService, SemanticLibraryStatus, SemanticRootAvailability,
    parse_semantic_root_id,
};

/// Authoritative authorization needed by knowledge search.
///
/// Deliberately narrow: three questions the host catalog can answer, with no
/// retrieval, planning, or transport concepts attached. Implemented by the
/// device-local library service, and by fakes in tests that need to change
/// authorization between retrieval and projection.
pub(crate) trait KnowledgeAuthority: Send + Sync {
    /// Reports enrolled roots and library availability.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, or persistence failure.
    fn status(
        &self,
        access: &SemanticAccessContext,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError>;

    /// Resolves one visible scope into current authorization.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, or persistence failure.
    fn resolve_knowledge_scope(
        &self,
        access: &SemanticAccessContext,
        workspace_id: fm_domain::WorkspaceId,
        selection: &RagScopeSelection,
    ) -> Result<ResolvedKnowledgeScope, SemanticLibraryError>;

    /// Resolves one opaque source into an exact authorized location.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, or persistence failure.
    fn resolve_occurrence(
        &self,
        access: &SemanticAccessContext,
        workspace_id: fm_domain::WorkspaceId,
        source_id: &str,
    ) -> Result<Option<ResolvedSemanticOccurrence>, SemanticLibraryError>;
}

impl KnowledgeAuthority for SemanticLibraryService {
    fn status(
        &self,
        access: &SemanticAccessContext,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        Self::status(self, access)
    }

    fn resolve_knowledge_scope(
        &self,
        access: &SemanticAccessContext,
        workspace_id: fm_domain::WorkspaceId,
        selection: &RagScopeSelection,
    ) -> Result<ResolvedKnowledgeScope, SemanticLibraryError> {
        Self::resolve_knowledge_scope(self, access, workspace_id, selection)
    }

    fn resolve_occurrence(
        &self,
        access: &SemanticAccessContext,
        workspace_id: fm_domain::WorkspaceId,
        source_id: &str,
    ) -> Result<Option<ResolvedSemanticOccurrence>, SemanticLibraryError> {
        Self::resolve_occurrence(self, access, workspace_id, source_id)
    }
}

/// Re-resolves the exact scope of one running search from the host catalog.
struct ScopeRefresh {
    authority: Arc<dyn KnowledgeAuthority>,
    access: SemanticAccessContext,
    workspace_id: fm_domain::WorkspaceId,
    selection: RagScopeSelection,
}

#[async_trait]
impl KnowledgeAuthorizationRefresh for ScopeRefresh {
    async fn refresh(&self) -> Result<KnowledgeAuthorizationSnapshot, KnowledgeSearchError> {
        let resolved = self
            .authority
            .resolve_knowledge_scope(&self.access, self.workspace_id, &self.selection)
            .map_err(|error| KnowledgeSearchError::AuthorizationUnavailable(error.to_string()))?;
        Ok(snapshot_of(&resolved))
    }
}

fn snapshot_of(resolved: &ResolvedKnowledgeScope) -> KnowledgeAuthorizationSnapshot {
    KnowledgeAuthorizationSnapshot {
        allowed_source_ids: resolved.allowed_source_ids.clone(),
        unavailable_source_ids: resolved.unavailable_source_ids.clone(),
        fingerprints: resolved.fingerprints.clone(),
    }
}

/// Search-first knowledge capability composed over one retrieval capability.
///
/// Optional answer generation is composed alongside search rather than inside
/// it: [`KnowledgeAnswerCoordinator`] holds no retrieval capability at all, and
/// the only bridge between the two is a bounded cache of evidence sets that a
/// search already produced and displayed.
pub(crate) struct KnowledgeService {
    coordinator: KnowledgeSearchCoordinator,
    answers: KnowledgeAnswerCoordinator,
    evidence: KnowledgeEvidenceCache,
}

impl KnowledgeService {
    /// Composes the capability over one worker-backed retrieval capability.
    pub(crate) fn new(capability: Arc<dyn KnowledgeRetrievalCapability>) -> Self {
        Self {
            coordinator: KnowledgeSearchCoordinator::new(capability),
            answers: KnowledgeAnswerCoordinator::default(),
            evidence: KnowledgeEvidenceCache::default(),
        }
    }

    /// Reports retrieval and answer capabilities independently.
    pub(crate) async fn capabilities(
        &self,
        answer_generation: bool,
    ) -> fm_transport_dto::KnowledgeCapabilitiesDto {
        capabilities_to_dto(self.coordinator.capabilities(answer_generation).await)
    }

    /// Lists indexed roots a knowledge search may be scoped to.
    pub(crate) fn roots(
        &self,
        authority: &dyn KnowledgeAuthority,
        access: &SemanticAccessContext,
        request: fm_transport_dto::ListKnowledgeRootsRequestDto,
    ) -> Result<Vec<fm_transport_dto::KnowledgeRootDto>, ApplicationError> {
        let workspace_id = request.workspace_id;
        let status = authority.status(access).map_err(library_error)?;
        Ok(status
            .roots
            .into_iter()
            .filter(|root| root.workspace_references.contains(&workspace_id))
            .map(|root| fm_transport_dto::KnowledgeRootDto {
                label: location_label(&root.location.uri),
                root_id: root.id,
                location: root.location.into(),
                recursive: root.recursive,
                available: matches!(root.availability, SemanticRootAvailability::Available),
                indexed_generation: root.indexed_generation,
            })
            .collect())
    }

    /// Interprets DSL or natural-language composer text deterministically.
    pub(crate) fn parse(
        &self,
        request: fm_transport_dto::ParseKnowledgeQueryRequestDto,
    ) -> fm_transport_dto::KnowledgeQueryInterpretationDto {
        interpretation_to_dto(&crate::knowledge_dsl::parse(&request.text))
    }

    /// Builds the deterministic plan for an authorized scope without retrieving.
    pub(crate) fn plan(
        &self,
        authority: &dyn KnowledgeAuthority,
        access: &SemanticAccessContext,
        request: fm_transport_dto::PlanKnowledgeSearchRequestDto,
    ) -> Result<fm_transport_dto::KnowledgeSearchPlanDto, ApplicationError> {
        Ok(self
            .resolve(
                authority,
                access,
                &request.draft,
                &request.scope,
                request.mode,
                request.options,
            )?
            .plan_dto())
    }

    /// Executes one search-only retrieval using a host-owned cancellation token.
    pub(crate) async fn execute(
        &self,
        authority: Arc<dyn KnowledgeAuthority>,
        access: &SemanticAccessContext,
        request: fm_transport_dto::ExecuteKnowledgeSearchRequestDto,
        answer_generation: bool,
        cancellation: &CancellationToken,
    ) -> Result<fm_transport_dto::KnowledgeSearchResultDto, ApplicationError> {
        let request_id = request.request_id;
        let resolved = self.resolve(
            authority.as_ref(),
            access,
            &request.draft,
            &request.scope,
            request.mode,
            request.options,
        )?;
        let plan_dto = resolved.plan_dto();
        let ResolvedKnowledgeRequest {
            authorized,
            titles,
            selection,
            workspace_id,
            tenant_id,
            library_id,
            ..
        } = resolved;
        let include_trace = authorized.plan.options.include_trace;
        let plan = authorized.plan.clone();
        let refresh = ScopeRefresh {
            authority,
            access: access.clone(),
            workspace_id,
            selection: selection.clone(),
        };
        let outcome = self
            .coordinator
            .execute(request_id, authorized, &refresh, cancellation)
            .await
            .map_err(search_error)?;
        let indexed_content_hashes = outcome
            .evidence
            .iter()
            .map(|row| (row.record_id.clone(), row.indexed_content_hash.clone()))
            .collect::<HashMap<_, _>>();
        let evidence = outcome
            .evidence
            .into_iter()
            .map(|evidence| {
                evidence_to_dto(
                    evidence,
                    &plan,
                    &outcome.rankings,
                    &outcome.freshness,
                    &titles,
                )
            })
            .collect::<Vec<_>>();
        // Retaining the displayed set is what makes a later answer possible
        // without rerunning retrieval. It is bounded, lifetime-limited, and
        // bound to this exact tenant/library/workspace.
        self.evidence.record(
            &outcome.evidence_fingerprint,
            CachedKnowledgeEvidence {
                binding: KnowledgeEvidenceBinding {
                    tenant_id,
                    library_id,
                    workspace_id,
                },
                selection,
                plan,
                evidence: evidence.clone(),
                indexed_content_hashes,
            },
        );
        Ok(fm_transport_dto::KnowledgeSearchResultDto {
            request_id,
            plan: plan_dto,
            route: route_outcome_to_dto(outcome.route),
            capabilities: capabilities_to_dto(
                crate::knowledge::KnowledgeCapabilities::from_worker(
                    outcome.capabilities,
                    answer_generation,
                ),
            ),
            coverage: coverage_to_dto(outcome.coverage),
            evidence,
            token_count: u64::try_from(outcome.token_count).unwrap_or(u64::MAX),
            evidence_fingerprint: outcome.evidence_fingerprint,
            withheld_unauthorized: outcome.withheld_unauthorized,
            trace: outcome
                .trace
                .as_ref()
                .filter(|_| include_trace)
                .map(trace_to_dto),
        })
    }

    /// Generates one optional answer from an already-inspected evidence set.
    ///
    /// This never plans, retrieves, or contacts the semantic worker. The
    /// evidence is the exact set an earlier successful search displayed, found
    /// by fingerprint inside this caller's tenant/library/workspace binding.
    /// A miss, an eviction, an expiry, or a binding mismatch is reported as one
    /// refresh-required failure, so nothing about another binding's retained
    /// evidence is observable and retrieval is never rerun implicitly.
    pub(crate) async fn answer(
        &self,
        authority: Arc<dyn KnowledgeAuthority>,
        access: &SemanticAccessContext,
        request: fm_transport_dto::GenerateKnowledgeAnswerRequestDto,
        profiles: &LlmProfileService,
        cancellation: &CancellationToken,
    ) -> Result<fm_transport_dto::KnowledgeAnswerDto, ApplicationError> {
        let refresh_required = || ApplicationError::KnowledgeEvidenceRefreshRequired {
            evidence_fingerprint: request.evidence_fingerprint.clone(),
        };
        let workspace_id = fm_domain::WorkspaceId::from(request.workspace_id);
        // Authorization is resolved first so a caller who may no longer search
        // this scope cannot probe which fingerprints are retained.
        let resolved = authority
            .resolve_knowledge_scope(access, workspace_id, &RagScopeSelection::EntireLibrary)
            .map_err(library_error)?;
        let binding = KnowledgeEvidenceBinding {
            tenant_id: resolved.filters.tenant_id.clone(),
            library_id: resolved
                .filters
                .library_id
                .clone()
                .ok_or(ApplicationError::ProviderUnavailable)?,
            workspace_id,
        };
        let cached = self
            .evidence
            .get(&request.evidence_fingerprint, &binding)
            .ok_or_else(refresh_required)?;
        let evidence = cached
            .evidence
            .iter()
            .enumerate()
            .map(|(index, row)| {
                answer_evidence_from_dto(
                    index,
                    row,
                    cached
                        .indexed_content_hashes
                        .get(&row.record_id)
                        .cloned()
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        let refresh = ScopeRefresh {
            authority,
            access: access.clone(),
            workspace_id,
            selection: cached.selection.clone(),
        };
        let intent = KnowledgeAnswerIntent {
            request: answer_request_from_dto(&request),
            profile_id: request.profile_id,
            allow_model_knowledge: request.allow_model_knowledge,
        };
        self.answers
            .generate(
                InspectedKnowledgeEvidence {
                    request_id: request.request_id,
                    fingerprint: &request.evidence_fingerprint,
                    plan: &cached.plan,
                    evidence,
                },
                &intent,
                &refresh,
                profiles,
                cancellation,
            )
            .await
            .map(|answer| answer_to_dto(request.request_id, answer))
            .map_err(answer_error_to_application)
    }

    /// Cancels one running or not-yet-started knowledge answer.
    pub(crate) fn cancel_answer(&self, request_id: Uuid) -> bool {
        self.answers.cancel(request_id)
    }

    /// Cancels one running or not-yet-started knowledge search.
    pub(crate) fn cancel(&self, request_id: Uuid) -> bool {
        self.coordinator.cancel(request_id)
    }

    /// Resolves one opaque evidence source into an exact navigable location.
    pub(crate) fn resolve_source(
        &self,
        authority: &dyn KnowledgeAuthority,
        access: &SemanticAccessContext,
        request: fm_transport_dto::ResolveKnowledgeSourceRequestDto,
    ) -> Result<fm_transport_dto::KnowledgeSourceLocationDto, ApplicationError> {
        let occurrence = authority
            .resolve_occurrence(access, request.workspace_id.into(), &request.source_id)
            .map_err(library_error)?
            .ok_or(ApplicationError::NotFound)?;
        Ok(fm_transport_dto::KnowledgeSourceLocationDto {
            entry_id: occurrence.entry_id.into_inner(),
            location: occurrence.location.into(),
            available: occurrence.available,
        })
    }

    /// Resolves the visible scope, applies authorization, and plans.
    fn resolve(
        &self,
        authority: &dyn KnowledgeAuthority,
        access: &SemanticAccessContext,
        draft: &fm_transport_dto::KnowledgeQueryDraftDto,
        scope: &fm_transport_dto::KnowledgeScopeDto,
        mode: fm_transport_dto::KnowledgeRetrievalModeDto,
        options: Option<fm_transport_dto::KnowledgeSearchOptionsDto>,
    ) -> Result<ResolvedKnowledgeRequest, ApplicationError> {
        let draft = draft_from_dto(draft)?;
        let selection = scope_selection(scope, &draft)?;
        let workspace_id = scope.workspace_id.into();
        let resolved = authority
            .resolve_knowledge_scope(access, workspace_id, &selection)
            .map_err(library_error)?;
        if resolved.allowed_source_ids.is_empty() {
            return Err(ApplicationError::NotFound);
        }
        let scope_context = KnowledgeDslScopeContext {
            tenant_id: resolved.filters.tenant_id.clone(),
            library_id: resolved
                .filters
                .library_id
                .clone()
                .ok_or(ApplicationError::ProviderUnavailable)?,
        };
        let selectors = authorized_selectors(&selection, &resolved);
        let search_request = KnowledgeSearchRequest {
            subjects: draft
                .about
                .iter()
                .map(|text| KnowledgeSubject { text: text.clone() })
                .collect(),
            needs: draft.needs.clone(),
            related_terms: draft.related.clone(),
            scopes: selectors
                .iter()
                .cloned()
                .map(|selector| KnowledgeScope {
                    tenant_id: scope_context.tenant_id.clone(),
                    library_id: scope_context.library_id.clone(),
                    selector,
                })
                .collect(),
            mode: mode_from_dto(mode),
            options: options.map_or_else(KnowledgeSearchOptions::default, options_from_dto),
        };
        let plan = KnowledgePlanner::plan(&search_request, draft.action)
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        let (partitions, scope_is_exact) = partition_scope(
            &resolved,
            matches!(selection, RagScopeSelection::EntireLibrary),
        );
        Ok(ResolvedKnowledgeRequest {
            effective_scope: effective_scope_dto(scope, &selection, &selectors),
            scope_label: scope_label(scope, &selection),
            authorized_sources: u64::try_from(resolved.allowed_source_ids.len())
                .unwrap_or(u64::MAX),
            excluded_from_retrieval: excluded_fields(&draft),
            titles: resolved.titles.clone(),
            selection,
            workspace_id,
            tenant_id: scope_context.tenant_id.clone(),
            library_id: scope_context.library_id.clone(),
            authorized: AuthorizedKnowledgeSearch {
                plan,
                partitions,
                scope_is_exact,
                eligible: resolved.eligible,
                snapshot: snapshot_of(&resolved),
            },
        })
    }
}

/// Authorized scope, plan, and host-only display data for one knowledge search.
struct ResolvedKnowledgeRequest {
    authorized: AuthorizedKnowledgeSearch,
    effective_scope: fm_transport_dto::KnowledgeScopeDto,
    scope_label: String,
    authorized_sources: u64,
    excluded_from_retrieval: Vec<fm_transport_dto::KnowledgeExcludedFieldDto>,
    titles: HashMap<String, String>,
    selection: RagScopeSelection,
    workspace_id: fm_domain::WorkspaceId,
    tenant_id: String,
    library_id: String,
}

impl ResolvedKnowledgeRequest {
    fn plan_dto(&self) -> fm_transport_dto::KnowledgeSearchPlanDto {
        plan_to_dto(
            &self.authorized.plan,
            self.effective_scope.clone(),
            self.scope_label.clone(),
            self.authorized_sources,
            self.authorized.scope_is_exact,
            self.excluded_from_retrieval.clone(),
        )
    }
}

/// Splits one authorized scope into exactly-scoped worker retrievals.
///
/// Whole-library and enrolled-root scopes are expressible as index filters, so
/// they are partitioned by root and the index itself never ranks anything
/// outside the scope. Folder, semantic-result, and selected-file scopes are
/// not, so the exact authorized source identities travel with the request and
/// the worker applies them before spending its result budget. When neither is
/// possible the scope is reported as inexact rather than silently broadened.
fn partition_scope(
    resolved: &ResolvedKnowledgeScope,
    whole_library: bool,
) -> (Vec<KnowledgeRetrievalPartition>, bool) {
    if whole_library {
        return (
            vec![KnowledgeRetrievalPartition {
                filters: resolved.filters.clone(),
                restriction: KnowledgeSourceRestriction::default(),
            }],
            true,
        );
    }
    if resolved.filters_are_exact {
        let roots = resolved.sources_by_root.keys().cloned().collect::<Vec<_>>();
        if roots.is_empty() {
            return (
                vec![KnowledgeRetrievalPartition {
                    filters: resolved.filters.clone(),
                    restriction: KnowledgeSourceRestriction::default(),
                }],
                true,
            );
        }
        if roots.len() <= crate::knowledge_search::MAX_SCOPE_PARTITIONS {
            return (
                roots
                    .into_iter()
                    .map(|root_id| KnowledgeRetrievalPartition {
                        filters: QueryFilters {
                            root_id: Some(root_id),
                            ..resolved.filters.clone()
                        },
                        restriction: KnowledgeSourceRestriction::default(),
                    })
                    .collect(),
                true,
            );
        }
    }
    restricted_partitions(resolved)
}

/// Describes an inexpressible scope by its exact authorized source identities.
fn restricted_partitions(
    resolved: &ResolvedKnowledgeScope,
) -> (Vec<KnowledgeRetrievalPartition>, bool) {
    if resolved.allowed_source_ids.len() <= MAX_ALLOWED_SOURCES {
        return (
            vec![KnowledgeRetrievalPartition {
                filters: resolved.filters.clone(),
                restriction: KnowledgeSourceRestriction {
                    allowed_source_ids: resolved.allowed_source_ids.clone(),
                },
            }],
            true,
        );
    }
    let groups = bounded_root_groups(&resolved.sources_by_root, &resolved.allowed_source_ids);
    if let Some(groups) = groups {
        return (
            groups
                .into_iter()
                .map(
                    |(root_id, allowed_source_ids)| KnowledgeRetrievalPartition {
                        filters: QueryFilters {
                            root_id: Some(root_id),
                            ..resolved.filters.clone()
                        },
                        restriction: KnowledgeSourceRestriction { allowed_source_ids },
                    },
                )
                .collect(),
            true,
        );
    }
    // The scope is larger than any exact representation this protocol version
    // can carry. Search one tenant/library superset and let the host filter
    // every row afterwards. Never truncate roots: that would silently search
    // a subset while reporting the opposite coverage semantics.
    (
        vec![KnowledgeRetrievalPartition {
            filters: resolved.filters.clone(),
            restriction: KnowledgeSourceRestriction::default(),
        }],
        false,
    )
}

/// Groups authorized sources per root when every group stays bounded.
fn bounded_root_groups(
    sources_by_root: &BTreeMap<String, BTreeSet<String>>,
    allowed_source_ids: &BTreeSet<String>,
) -> Option<Vec<(String, BTreeSet<String>)>> {
    if sources_by_root.is_empty()
        || sources_by_root.len() > crate::knowledge_search::MAX_SCOPE_PARTITIONS
    {
        return None;
    }
    let mut groups = Vec::with_capacity(sources_by_root.len());
    let mut covered = BTreeSet::new();
    for (root_id, sources) in sources_by_root {
        let allowed = sources
            .iter()
            .filter(|source_id| allowed_source_ids.contains(*source_id))
            .cloned()
            .collect::<BTreeSet<_>>();
        if allowed.len() > MAX_ALLOWED_SOURCES {
            return None;
        }
        covered.extend(allowed.iter().cloned());
        if !allowed.is_empty() {
            groups.push((root_id.clone(), allowed));
        }
    }
    // Every authorized source must be reachable through exactly one partition.
    (covered.len() == allowed_source_ids.len()).then_some(groups)
}

/// Maps the visible scope (and any DSL `scope:` selector) onto the authorized
/// host selection. DSL root selectors are honored only when the visible scope
/// did not already name roots, so the request's scope always wins.
///
/// A `workspace:` selector names the workspace the search already runs inside;
/// it is accepted only when it matches, because this capability cannot search
/// another workspace's authorization and must not pretend the selector was
/// applied.
fn scope_selection(
    scope: &fm_transport_dto::KnowledgeScopeDto,
    draft: &KnowledgeQueryDraft,
) -> Result<RagScopeSelection, ApplicationError> {
    let mut dsl_roots = Vec::new();
    for selector in &draft.scopes {
        match selector {
            KnowledgeScopeSelector::Root { root_id } => dsl_roots.push(root_id.clone()),
            KnowledgeScopeSelector::Workspace { workspace_id } => {
                if !workspace_id.eq_ignore_ascii_case(&scope.workspace_id.to_string()) {
                    return Err(ApplicationError::InvalidRequest(
                        "workspace knowledge scope selector must name the requested workspace"
                            .into(),
                    ));
                }
            }
            KnowledgeScopeSelector::WholeLibrary => {}
        }
    }
    let root_ids = if scope.enrolled_root_ids.is_empty()
        && matches!(
            scope.kind,
            fm_transport_dto::KnowledgeScopeKindDto::EntireLibrary
        ) {
        dsl_roots
    } else {
        scope.enrolled_root_ids.clone()
    };
    match scope.kind {
        fm_transport_dto::KnowledgeScopeKindDto::EntireLibrary if root_ids.is_empty() => {
            Ok(RagScopeSelection::EntireLibrary)
        }
        fm_transport_dto::KnowledgeScopeKindDto::EntireLibrary
        | fm_transport_dto::KnowledgeScopeKindDto::EnrolledRoots => {
            if root_ids.is_empty() {
                return Err(ApplicationError::InvalidRequest(
                    "enrolled-root knowledge scope is empty".into(),
                ));
            }
            Ok(RagScopeSelection::EnrolledRoots(
                root_ids
                    .iter()
                    .map(|value| parse_semantic_root_id(value))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| {
                        ApplicationError::InvalidRequest("invalid knowledge root id".into())
                    })?,
            ))
        }
        fm_transport_dto::KnowledgeScopeKindDto::CurrentFolder => {
            Ok(RagScopeSelection::CurrentFolder(
                scope
                    .folder
                    .clone()
                    .ok_or_else(|| {
                        ApplicationError::InvalidRequest(
                            "current-folder knowledge scope requires a folder".into(),
                        )
                    })?
                    .into(),
            ))
        }
        fm_transport_dto::KnowledgeScopeKindDto::SemanticResults => {
            if scope.semantic_source_ids.is_empty() {
                return Err(ApplicationError::InvalidRequest(
                    "semantic-result knowledge scope is empty".into(),
                ));
            }
            Ok(RagScopeSelection::SemanticResults(
                scope.semantic_source_ids.clone(),
            ))
        }
    }
}

/// Canonical selectors recorded in the plan for the authorized selection.
///
/// A whole-library request that resolved to specific roots — for example
/// through a DSL `scope:` selector — records those roots, so the plan describes
/// what was actually searched rather than what was asked for.
fn authorized_selectors(
    selection: &RagScopeSelection,
    resolved: &ResolvedKnowledgeScope,
) -> Vec<KnowledgeScopeSelector> {
    match selection {
        RagScopeSelection::EnrolledRoots(roots) => roots
            .iter()
            .map(|root_id| KnowledgeScopeSelector::Root {
                root_id: root_id.to_string(),
            })
            .collect(),
        RagScopeSelection::EntireLibrary => vec![KnowledgeScopeSelector::WholeLibrary],
        _ => {
            let roots = resolved
                .sources_by_root
                .keys()
                .map(|root_id| KnowledgeScopeSelector::Root {
                    root_id: root_id.clone(),
                })
                .collect::<Vec<_>>();
            if roots.is_empty() {
                vec![KnowledgeScopeSelector::WholeLibrary]
            } else {
                roots
            }
        }
    }
}

/// Describes the scope that was actually resolved and searched.
fn effective_scope_dto(
    requested: &fm_transport_dto::KnowledgeScopeDto,
    selection: &RagScopeSelection,
    selectors: &[KnowledgeScopeSelector],
) -> fm_transport_dto::KnowledgeScopeDto {
    let root_ids = selectors
        .iter()
        .filter_map(|selector| match selector {
            KnowledgeScopeSelector::Root { root_id } => Some(root_id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let kind = match selection {
        RagScopeSelection::EntireLibrary => fm_transport_dto::KnowledgeScopeKindDto::EntireLibrary,
        RagScopeSelection::EnrolledRoots(_) => {
            fm_transport_dto::KnowledgeScopeKindDto::EnrolledRoots
        }
        RagScopeSelection::CurrentFolder(_) => {
            fm_transport_dto::KnowledgeScopeKindDto::CurrentFolder
        }
        RagScopeSelection::SemanticResults(_) | RagScopeSelection::SelectedFiles(_) => {
            fm_transport_dto::KnowledgeScopeKindDto::SemanticResults
        }
    };
    fm_transport_dto::KnowledgeScopeDto {
        kind,
        workspace_id: requested.workspace_id,
        enrolled_root_ids: root_ids,
        folder: match selection {
            RagScopeSelection::CurrentFolder(folder) => Some(folder.clone().into()),
            _ => None,
        },
        semantic_source_ids: match selection {
            RagScopeSelection::SemanticResults(ids) => ids.clone(),
            _ => Vec::new(),
        },
    }
}

fn scope_label(
    scope: &fm_transport_dto::KnowledgeScopeDto,
    selection: &RagScopeSelection,
) -> String {
    match selection {
        RagScopeSelection::EntireLibrary => "Entire indexed library".to_owned(),
        RagScopeSelection::EnrolledRoots(roots) => format!("{} indexed root(s)", roots.len()),
        RagScopeSelection::CurrentFolder(folder) => location_label(&folder.uri),
        RagScopeSelection::SemanticResults(_) => {
            format!("{} semantic result(s)", scope.semantic_source_ids.len())
        }
        RagScopeSelection::SelectedFiles(files) => format!("{} selected file(s)", files.len()),
    }
}

fn location_label(uri: &str) -> String {
    uri.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(uri)
        .to_owned()
}

fn library_error(error: SemanticLibraryError) -> ApplicationError {
    match error {
        SemanticLibraryError::Unavailable => ApplicationError::ProviderUnavailable,
        SemanticLibraryError::AuthorityDenied { .. } => ApplicationError::PermissionDenied,
        _ => ApplicationError::InvalidRequest(error.to_string()),
    }
}

fn search_error(error: KnowledgeSearchError) -> ApplicationError {
    match error {
        KnowledgeSearchError::Unavailable
        | KnowledgeSearchError::RouteUnavailable(_)
        | KnowledgeSearchError::AuthorizationUnavailable(_) => {
            ApplicationError::ProviderUnavailable
        }
        KnowledgeSearchError::InvalidRequest(message) => ApplicationError::InvalidRequest(message),
        KnowledgeSearchError::DuplicateRequest => {
            ApplicationError::InvalidRequest(error.to_string())
        }
        KnowledgeSearchError::Cancelled => ApplicationError::OperationCancelled,
        KnowledgeSearchError::RetrievalFailed(_) => ApplicationError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use fm_semantic_conversion::{ChunkProvenance, Provenance};
    use fm_semantic_worker::knowledge_retrieval::{
        KnowledgeCapabilities as WorkerKnowledgeCapabilities, KnowledgeEvidence,
        KnowledgeRetrieval, KnowledgeRetrievalRequest, KnowledgeRoute, RankContribution,
        RetrievalTrace, RouteFallbackReason, RouteOutcome, TracedEvidence,
    };
    use fm_transport_dto::{
        CancelKnowledgeSearchRequestDto, ExecuteKnowledgeSearchRequestDto,
        GenerateKnowledgeAnswerRequestDto, KnowledgeNeedDto, KnowledgeQueryDraftDto,
        KnowledgeRetrievalModeDto, KnowledgeScopeDto, KnowledgeScopeKindDto,
        ListKnowledgeRootsRequestDto, ParseKnowledgeQueryRequestDto, PlanKnowledgeSearchRequestDto,
        ResolveKnowledgeSourceRequestDto, RuntimeKindDto,
    };
    use tokio_util::sync::CancellationToken;
    use uuid::Uuid;

    use crate::error::ApplicationError;
    use crate::knowledge_search::KnowledgeRetrievalCapability;
    use crate::semantic_library::{
        SemanticAccessContext, SemanticFolderContext, SemanticIndexingObservation,
        SemanticLibraryService,
    };
    use crate::service::FileManagerService;

    pub(super) struct RecordingCapability {
        evidence: Mutex<Vec<KnowledgeEvidence>>,
        requests: Mutex<Vec<KnowledgeRetrievalRequest>>,
    }

    impl RecordingCapability {
        pub(super) fn new(evidence: Vec<KnowledgeEvidence>) -> Self {
            Self {
                evidence: Mutex::new(evidence),
                requests: Mutex::new(Vec::new()),
            }
        }

        /// Replaces the returned evidence once the fixture knows the exact
        /// authorized occurrence identity the host catalog assigned.
        /// Returns every request the coordinator issued to the worker.
        pub(super) fn requests(&self) -> Vec<KnowledgeRetrievalRequest> {
            self.requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        pub(super) fn set_evidence(&self, evidence: Vec<KnowledgeEvidence>) {
            *self
                .evidence
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = evidence;
        }

        fn evidence(&self) -> Vec<KnowledgeEvidence> {
            self.evidence
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    #[async_trait]
    impl KnowledgeRetrievalCapability for RecordingCapability {
        async fn capabilities(&self) -> WorkerKnowledgeCapabilities {
            WorkerKnowledgeCapabilities {
                full_text: true,
                query_embeddings: false,
            }
        }

        async fn retrieve(
            &self,
            request: KnowledgeRetrievalRequest,
            cancellation: &CancellationToken,
        ) -> Result<KnowledgeRetrieval, crate::knowledge_search::KnowledgeSearchError> {
            if cancellation.is_cancelled() {
                return Err(crate::knowledge_search::KnowledgeSearchError::Cancelled);
            }
            let coverage = request.coverage;
            self.requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(request);
            let evidence = self.evidence();
            let route = RouteOutcome {
                requested: KnowledgeRoute::Hybrid,
                applied: KnowledgeRoute::FullText,
                fallback_reason: Some(RouteFallbackReason::QueryEmbeddingsUnavailable),
            };
            let capabilities = WorkerKnowledgeCapabilities {
                full_text: true,
                query_embeddings: false,
            };
            Ok(KnowledgeRetrieval {
                route,
                capabilities,
                token_count: evidence.iter().map(|row| row.token_count).sum(),
                coverage,
                trace: Some(RetrievalTrace {
                    route,
                    capabilities,
                    rank_constant: 60,
                    queries: Vec::new(),
                    entries: evidence
                        .iter()
                        .map(|row| TracedEvidence {
                            record_id: row.record_id.clone(),
                            occurrence_id: row.occurrence_id.clone(),
                            source_id: row.source_id.clone(),
                            document_id: row.document_id.clone(),
                            contributions: vec![RankContribution {
                                query_index: 1,
                                route: KnowledgeRoute::FullText,
                                rank: 1,
                            }],
                            fused_score: 0.016,
                            final_rank: row.final_rank,
                            provenance: row.provenance.clone(),
                            adjacent: row.adjacent,
                            generated: row.generated,
                            unavailable: row.unavailable,
                            stale: row.stale,
                        })
                        .collect(),
                }),
                evidence,
            })
        }
    }

    pub(super) fn evidence(record_id: &str, source_id: &str) -> KnowledgeEvidence {
        KnowledgeEvidence {
            record_id: record_id.to_owned(),
            occurrence_id: format!("{source_id}#0"),
            source_id: source_id.to_owned(),
            duplicate_source_ids: Vec::new(),
            document_id: format!("{source_id}-doc"),
            library_id: "library".to_owned(),
            chunk_kind: "chunk".to_owned(),
            excerpt: "Rotor blades".to_owned(),
            content: "Rotor blades convert wind into torque.".to_owned(),
            token_count: 9,
            section_path: vec!["Design".to_owned()],
            provenance: ChunkProvenance::Exact(Provenance::TextLines {
                start_line: 1,
                end_line: 4,
            }),
            media_type: Some("text/plain".to_owned()),
            modified_at_ms: Some(10),
            indexed_content_hash: "sha256:rotor".to_owned(),
            generation: 1,
            source_position: 0,
            generated: false,
            unavailable: false,
            stale: false,
            adjacent: false,
            final_rank: 1,
        }
    }

    pub(super) struct Fixture {
        _directory: tempfile::TempDir,
        pub(super) service: FileManagerService,
        pub(super) workspace_id: Uuid,
        pub(super) source_id: String,
        pub(super) root_id: String,
    }

    pub(super) fn fixture(capability: Arc<dyn KnowledgeRetrievalCapability>) -> Fixture {
        let directory = tempfile::tempdir().expect("temporary directory");
        let library = SemanticLibraryService::deterministic_mock();
        let workspace_id = fm_domain::WorkspaceId::new();
        let folder = SemanticFolderContext::new(
            workspace_id,
            fm_domain::Location::parse("file:///indexed-library").expect("location"),
        );
        let preview = library
            .preview_enrolment(&SemanticAccessContext::Host, folder.clone(), true)
            .expect("preview enrolment");
        library
            .confirm_enrolment(
                &SemanticAccessContext::Host,
                &preview.confirmation_id,
                preview.policy_revision,
                &folder,
            )
            .expect("confirm enrolment");
        let root_id = library
            .status(&SemanticAccessContext::Host)
            .expect("status")
            .roots[0]
            .id
            .clone();
        let source_id = library
            .record_indexing_observation(
                &SemanticAccessContext::Host,
                SemanticIndexingObservation {
                    entry_id: fm_domain::EntryId::new(),
                    location: fm_domain::Location::parse("file:///indexed-library/turbines.md")
                        .expect("location"),
                    content_fingerprint: fm_semantic_library::ContentFingerprint::new(
                        "sha256:turbines",
                    )
                    .expect("fingerprint"),
                    root_id: root_id.parse().expect("root id"),
                    workspace_ids: vec![workspace_id],
                    source_bytes: 512,
                },
            )
            .expect("record observation")
            .to_string();
        let service = FileManagerService::new(
            RuntimeKindDto::Tauri,
            directory.path(),
            directory.path().join("settings"),
        )
        .with_semantic_library_service(library)
        .with_knowledge_retrieval_capability(capability);
        Fixture {
            _directory: directory,
            service,
            workspace_id: workspace_id.into(),
            source_id,
            root_id,
        }
    }

    fn draft() -> KnowledgeQueryDraftDto {
        KnowledgeQueryDraftDto {
            about: vec!["wind turbines".to_owned()],
            needs: vec![KnowledgeNeedDto::Definition],
            ..KnowledgeQueryDraftDto::default()
        }
    }

    fn scope(workspace_id: Uuid) -> KnowledgeScopeDto {
        KnowledgeScopeDto {
            kind: KnowledgeScopeKindDto::EntireLibrary,
            workspace_id,
            enrolled_root_ids: Vec::new(),
            folder: None,
            semantic_source_ids: Vec::new(),
        }
    }

    #[tokio::test]
    async fn knowledge_search_completes_full_text_only_with_no_llm_profile_configured() {
        let capability = Arc::new(RecordingCapability::new(Vec::new()));
        let fixture = fixture(capability.clone());
        capability.set_evidence(vec![evidence("r1", &fixture.source_id)]);

        let capabilities = fixture.service.knowledge_capabilities().await;
        assert!(!capabilities.answer_generation);

        let result = fixture
            .service
            .execute_knowledge_search(
                &SemanticAccessContext::Host,
                ExecuteKnowledgeSearchRequestDto {
                    request_id: Uuid::new_v4(),
                    draft: draft(),
                    scope: scope(fixture.workspace_id),
                    mode: KnowledgeRetrievalModeDto::Hybrid,
                    options: None,
                },
            )
            .await
            .expect("search must succeed without any generation profile");

        assert!(!result.capabilities.answer_generation);
        assert!(result.capabilities.full_text);
        assert_eq!(
            result.route.applied,
            fm_transport_dto::KnowledgeRouteDto::FullText
        );
        assert!(result.route.fallback_reason.is_some());
        assert_eq!(result.evidence.len(), 1);
        assert_eq!(result.evidence[0].source_id, fixture.source_id);
        assert_eq!(result.evidence[0].final_rank, 1);
        assert!(!result.evidence[0].reasons.is_empty());
        assert!(!result.evidence[0].provenance.is_empty());
        assert_eq!(result.withheld_unauthorized, 0);
        assert!(result.evidence_fingerprint.starts_with("sha256:"));
        assert_eq!(result.coverage.eligible, 1);
        assert!(result.plan.searches.len() >= 2);
    }

    #[tokio::test]
    async fn evidence_outside_current_authorization_is_withheld_from_the_result() {
        let fixture = fixture(Arc::new(RecordingCapability::new(vec![evidence(
            "r1",
            "unauthorized-source",
        )])));

        let result = fixture
            .service
            .execute_knowledge_search(
                &SemanticAccessContext::Host,
                ExecuteKnowledgeSearchRequestDto {
                    request_id: Uuid::new_v4(),
                    draft: draft(),
                    scope: scope(fixture.workspace_id),
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect("search runs and reports honest authorization");

        assert!(result.evidence.is_empty());
        assert_eq!(result.withheld_unauthorized, 1);
    }

    #[tokio::test]
    async fn a_scope_naming_an_unauthorized_root_is_denied_before_retrieval() {
        let capability = Arc::new(RecordingCapability::new(vec![evidence("r1", "source")]));
        let fixture = fixture(capability.clone());

        let error = fixture
            .service
            .execute_knowledge_search(
                &SemanticAccessContext::Host,
                ExecuteKnowledgeSearchRequestDto {
                    request_id: Uuid::new_v4(),
                    draft: draft(),
                    scope: KnowledgeScopeDto {
                        kind: KnowledgeScopeKindDto::EnrolledRoots,
                        workspace_id: Uuid::new_v4(),
                        enrolled_root_ids: vec![fixture.root_id.clone()],
                        folder: None,
                        semantic_source_ids: Vec::new(),
                    },
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect_err("a root outside the workspace must be denied");

        assert!(matches!(error, ApplicationError::InvalidRequest(_)));
        assert!(
            capability
                .requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
    }

    #[tokio::test]
    async fn cancelling_a_search_before_it_starts_stops_it() {
        let capability = Arc::new(RecordingCapability::new(vec![evidence("r1", "source")]));
        let fixture = fixture(capability.clone());
        let request_id = Uuid::new_v4();

        assert!(
            !fixture
                .service
                .cancel_knowledge_search(CancelKnowledgeSearchRequestDto { request_id })
        );
        let error = fixture
            .service
            .execute_knowledge_search(
                &SemanticAccessContext::Host,
                ExecuteKnowledgeSearchRequestDto {
                    request_id,
                    draft: draft(),
                    scope: scope(fixture.workspace_id),
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect_err("a cancelled search must not return evidence");

        assert!(matches!(error, ApplicationError::OperationCancelled));
    }

    #[tokio::test]
    async fn planning_previews_searches_and_the_answer_fields_retrieval_never_reads() {
        let fixture = fixture(Arc::new(RecordingCapability::new(Vec::new())));

        let plan = fixture
            .service
            .plan_knowledge_search(
                &SemanticAccessContext::Host,
                PlanKnowledgeSearchRequestDto {
                    draft: KnowledgeQueryDraftDto {
                        about: vec!["wind turbines".to_owned()],
                        needs: vec![KnowledgeNeedDto::Procedure],
                        context: Some("a maintenance report".to_owned()),
                        constraints: vec!["cite sources".to_owned()],
                        ..KnowledgeQueryDraftDto::default()
                    },
                    scope: scope(fixture.workspace_id),
                    mode: KnowledgeRetrievalModeDto::Hybrid,
                    options: None,
                },
            )
            .await
            .expect("plan must succeed");

        assert_eq!(plan.subjects, vec!["wind turbines".to_owned()]);
        assert!(
            plan.searches
                .iter()
                .all(|search| !search.text.contains("maintenance report"))
        );
        assert!(
            plan.excluded_from_retrieval
                .iter()
                .any(|field| field.field == "to")
        );
        assert!(
            plan.excluded_from_retrieval
                .iter()
                .any(|field| field.field == "constraint")
        );
        assert_eq!(plan.authorized_sources, 1);
        assert_eq!(plan.scope_label, "Entire indexed library");
    }

    #[tokio::test]
    async fn roots_parsing_and_source_navigation_stay_available_without_an_llm() {
        let fixture = fixture(Arc::new(RecordingCapability::new(Vec::new())));

        let roots = fixture
            .service
            .list_knowledge_roots(
                &SemanticAccessContext::Host,
                ListKnowledgeRootsRequestDto {
                    workspace_id: fixture.workspace_id,
                },
            )
            .await
            .expect("roots");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].root_id, fixture.root_id);
        assert_eq!(roots[0].label, "indexed-library");

        let interpretation = fixture
            .service
            .parse_knowledge_query(ParseKnowledgeQueryRequestDto {
                text: "about: wind turbines\nneed: procedure\ndo: apply\nto: a maintenance report"
                    .to_owned(),
            });
        assert_eq!(interpretation.draft.about, vec!["wind turbines".to_owned()]);
        assert_eq!(
            interpretation.draft.needs,
            vec![KnowledgeNeedDto::Procedure]
        );
        assert!(
            interpretation
                .excluded_from_retrieval
                .iter()
                .any(|field| field.field == "do")
        );
        assert!(
            interpretation
                .dsl_multiline
                .contains("about: wind turbines")
        );

        let resolved = fixture
            .service
            .resolve_knowledge_source(
                &SemanticAccessContext::Host,
                ResolveKnowledgeSourceRequestDto {
                    workspace_id: fixture.workspace_id,
                    source_id: fixture.source_id.clone(),
                },
            )
            .await
            .expect("evidence sources resolve to an exact location");
        assert_eq!(resolved.location.uri, "file:///indexed-library/turbines.md");
    }

    fn answer_request(
        workspace_id: Uuid,
        evidence_fingerprint: &str,
    ) -> GenerateKnowledgeAnswerRequestDto {
        GenerateKnowledgeAnswerRequestDto {
            request_id: Uuid::new_v4(),
            workspace_id,
            evidence_fingerprint: evidence_fingerprint.to_owned(),
            profile_id: Uuid::new_v4(),
            allow_model_knowledge: false,
            action: Some(fm_transport_dto::KnowledgeActionDto::Explain),
            context: Some("a maintenance report".to_owned()),
            constraints: vec!["cite sources".to_owned()],
            depth: Some(fm_transport_dto::KnowledgeAnswerDepthDto::Brief),
            output: Some(fm_transport_dto::KnowledgeOutputFormatDto::Bullets),
        }
    }

    /// Search must remain complete and answering must be refused — never
    /// silently attempted — when nothing is configured to generate with.
    #[tokio::test]
    async fn answering_without_a_generation_profile_is_refused_and_search_stays_complete() {
        let capability = Arc::new(RecordingCapability::new(Vec::new()));
        let fixture = fixture(capability.clone());
        capability.set_evidence(vec![evidence("r1", &fixture.source_id)]);
        let result = fixture
            .service
            .execute_knowledge_search(
                &SemanticAccessContext::Host,
                ExecuteKnowledgeSearchRequestDto {
                    request_id: Uuid::new_v4(),
                    draft: draft(),
                    scope: scope(fixture.workspace_id),
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect("search succeeds without generation");
        assert!(!result.capabilities.answer_generation);

        let error = fixture
            .service
            .generate_knowledge_answer(
                &SemanticAccessContext::Host,
                answer_request(fixture.workspace_id, &result.evidence_fingerprint),
            )
            .await
            .expect_err("no profile means no answer");

        assert!(matches!(error, ApplicationError::ProviderUnavailable));
        assert_eq!(
            capability.requests().len(),
            1,
            "answering must not retrieve"
        );
    }

    /// The cache is the only bridge between search and answer. A fingerprint
    /// that was never produced — or was evicted — must demand an explicit new
    /// search instead of triggering one.
    #[tokio::test]
    async fn an_unknown_evidence_fingerprint_requires_an_explicit_refresh() {
        let capability = Arc::new(RecordingCapability::new(Vec::new()));
        let fixture = fixture(capability.clone());
        capability.set_evidence(vec![evidence("r1", &fixture.source_id)]);
        fixture
            .service
            .execute_knowledge_search(
                &SemanticAccessContext::Host,
                ExecuteKnowledgeSearchRequestDto {
                    request_id: Uuid::new_v4(),
                    draft: draft(),
                    scope: scope(fixture.workspace_id),
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect("search");

        let error = fixture
            .service
            .generate_knowledge_answer(
                &SemanticAccessContext::Host,
                answer_request(fixture.workspace_id, "sha256:never-produced"),
            )
            .await
            .expect_err("an unknown evidence set is not answerable");

        match error {
            ApplicationError::KnowledgeEvidenceRefreshRequired {
                evidence_fingerprint,
            } => assert_eq!(evidence_fingerprint, "sha256:never-produced"),
            other => panic!("expected a typed refresh-required failure, got {other:?}"),
        }
        assert_eq!(
            capability.requests().len(),
            1,
            "a cache miss must never rerun retrieval"
        );
    }

    /// A retained evidence set belongs to the workspace that produced it. A
    /// different workspace must observe a plain refresh-required miss rather
    /// than any evidence, and must not cause a retrieval either.
    #[tokio::test]
    async fn a_retained_evidence_set_is_not_answerable_from_another_workspace() {
        let capability = Arc::new(RecordingCapability::new(Vec::new()));
        let fixture = fixture(capability.clone());
        capability.set_evidence(vec![evidence("r1", &fixture.source_id)]);
        let result = fixture
            .service
            .execute_knowledge_search(
                &SemanticAccessContext::Host,
                ExecuteKnowledgeSearchRequestDto {
                    request_id: Uuid::new_v4(),
                    draft: draft(),
                    scope: scope(fixture.workspace_id),
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect("search");

        let error = fixture
            .service
            .generate_knowledge_answer(
                &SemanticAccessContext::Host,
                answer_request(Uuid::new_v4(), &result.evidence_fingerprint),
            )
            .await
            .expect_err("another workspace must not answer from this evidence");

        assert!(
            matches!(
                error,
                ApplicationError::KnowledgeEvidenceRefreshRequired { .. }
                    | ApplicationError::NotFound
                    | ApplicationError::PermissionDenied
                    | ApplicationError::ProviderUnavailable
            ),
            "unexpected failure: {error:?}"
        );
        assert_eq!(capability.requests().len(), 1);
    }

    /// Knowledge answers must not introduce persistent conversation storage,
    /// and must leave the existing saved-Ask evidence lifecycle untouched:
    /// nothing is created by searching, and deleting an unknown conversation
    /// still reports that it does not exist.
    #[tokio::test]
    async fn knowledge_search_and_answers_add_no_saved_conversation_storage() {
        let capability = Arc::new(RecordingCapability::new(Vec::new()));
        let fixture = fixture(capability.clone());
        capability.set_evidence(vec![evidence("r1", &fixture.source_id)]);
        let result = fixture
            .service
            .execute_knowledge_search(
                &SemanticAccessContext::Host,
                ExecuteKnowledgeSearchRequestDto {
                    request_id: Uuid::new_v4(),
                    draft: draft(),
                    scope: scope(fixture.workspace_id),
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect("search");

        let _ = fixture
            .service
            .generate_knowledge_answer(
                &SemanticAccessContext::Host,
                answer_request(fixture.workspace_id, &result.evidence_fingerprint),
            )
            .await;

        let saved = fixture
            .service
            .list_saved_rag_conversations(&SemanticAccessContext::Host, fixture.workspace_id)
            .expect("saved Ask conversations remain listable");
        assert!(
            saved.is_empty(),
            "knowledge search and answers must not persist conversations"
        );
        let error = fixture
            .service
            .delete_rag_conversation(
                &SemanticAccessContext::Host,
                fm_transport_dto::DeleteRagConversationRequestDto {
                    conversation_id: Uuid::new_v4(),
                },
            )
            .expect_err("deleting an unknown conversation still reports not found");
        assert!(matches!(error, ApplicationError::NotFound));
    }

    /// Cancellation must be available before generation starts, exactly like
    /// search cancellation, on both hosts.
    #[tokio::test]
    async fn cancelling_an_answer_before_it_starts_is_accepted() {
        let fixture = fixture(Arc::new(RecordingCapability::new(Vec::new())));
        let request_id = Uuid::new_v4();

        assert!(!fixture.service.cancel_knowledge_answer(
            fm_transport_dto::CancelKnowledgeAnswerRequestDto { request_id }
        ));
    }
}

#[cfg(test)]
mod resolved_scope_tests {
    use std::sync::Arc;

    use fm_transport_dto::{
        ExecuteKnowledgeSearchRequestDto, KnowledgeNeedDto, KnowledgeQueryDraftDto,
        KnowledgeRetrievalModeDto, KnowledgeScopeDto, KnowledgeScopeKindDto,
        KnowledgeScopeSelectorDto, KnowledgeScopeSelectorKindDto, PlanKnowledgeSearchRequestDto,
    };
    use uuid::Uuid;

    use super::tests::{RecordingCapability, evidence, fixture};
    use crate::semantic_library::SemanticAccessContext;

    fn draft() -> KnowledgeQueryDraftDto {
        KnowledgeQueryDraftDto {
            about: vec!["wind turbines".to_owned()],
            needs: vec![KnowledgeNeedDto::Definition],
            ..KnowledgeQueryDraftDto::default()
        }
    }

    /// The plan must describe the scope that was actually resolved, including
    /// roots that only a DSL `scope:` selector named, not the broad request.
    #[tokio::test]
    async fn the_plan_describes_the_resolved_scope_including_dsl_root_selectors() {
        let fixture = fixture(Arc::new(RecordingCapability::new(Vec::new())));

        let plan = fixture
            .service
            .plan_knowledge_search(
                &SemanticAccessContext::Host,
                PlanKnowledgeSearchRequestDto {
                    draft: KnowledgeQueryDraftDto {
                        scopes: vec![KnowledgeScopeSelectorDto {
                            kind: KnowledgeScopeSelectorKindDto::Root,
                            id: Some(fixture.root_id.clone()),
                        }],
                        ..draft()
                    },
                    scope: KnowledgeScopeDto {
                        kind: KnowledgeScopeKindDto::EntireLibrary,
                        workspace_id: fixture.workspace_id,
                        enrolled_root_ids: Vec::new(),
                        folder: None,
                        semantic_source_ids: Vec::new(),
                    },
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect("plan");

        assert_eq!(plan.scope.kind, KnowledgeScopeKindDto::EnrolledRoots);
        assert_eq!(plan.scope.enrolled_root_ids, vec![fixture.root_id.clone()]);
        assert_eq!(plan.scope_label, "1 indexed root(s)");
        assert!(plan.scope_is_exact);
        assert_eq!(plan.authorized_sources, 1);
    }

    /// A `root:` selector without an identity names nothing. Widening it to the
    /// whole library would search every enrolled root the caller has, so it is
    /// rejected instead.
    #[tokio::test]
    async fn a_root_selector_without_an_id_is_rejected_rather_than_widened() {
        let capability = Arc::new(RecordingCapability::new(Vec::new()));
        let fixture = fixture(capability.clone());

        let error = fixture
            .service
            .plan_knowledge_search(
                &SemanticAccessContext::Host,
                PlanKnowledgeSearchRequestDto {
                    draft: KnowledgeQueryDraftDto {
                        scopes: vec![KnowledgeScopeSelectorDto {
                            kind: KnowledgeScopeSelectorKindDto::Root,
                            id: None,
                        }],
                        ..draft()
                    },
                    scope: KnowledgeScopeDto {
                        kind: KnowledgeScopeKindDto::EntireLibrary,
                        workspace_id: fixture.workspace_id,
                        enrolled_root_ids: Vec::new(),
                        folder: None,
                        semantic_source_ids: Vec::new(),
                    },
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect_err("an identity-less root selector must be rejected");

        assert!(matches!(
            error,
            crate::error::ApplicationError::InvalidRequest(_)
        ));
        assert!(capability.requests().is_empty());
    }

    /// The same rule holds for `workspace:`, and it holds on the execution
    /// path, not only while planning.
    #[tokio::test]
    async fn a_workspace_selector_without_an_id_is_rejected_before_retrieval() {
        let capability = Arc::new(RecordingCapability::new(Vec::new()));
        let fixture = fixture(capability.clone());

        let error = fixture
            .service
            .execute_knowledge_search(
                &SemanticAccessContext::Host,
                ExecuteKnowledgeSearchRequestDto {
                    request_id: Uuid::new_v4(),
                    draft: KnowledgeQueryDraftDto {
                        scopes: vec![KnowledgeScopeSelectorDto {
                            kind: KnowledgeScopeSelectorKindDto::Workspace,
                            id: None,
                        }],
                        ..draft()
                    },
                    scope: KnowledgeScopeDto {
                        kind: KnowledgeScopeKindDto::EntireLibrary,
                        workspace_id: fixture.workspace_id,
                        enrolled_root_ids: Vec::new(),
                        folder: None,
                        semantic_source_ids: Vec::new(),
                    },
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect_err("an identity-less workspace selector must be rejected");

        assert!(matches!(
            error,
            crate::error::ApplicationError::InvalidRequest(_)
        ));
        assert!(capability.requests().is_empty());
    }

    /// A search runs inside exactly one workspace's authorization, so a
    /// `workspace:` selector naming a different one cannot be honored and is
    /// refused rather than quietly ignored.
    #[tokio::test]
    async fn a_workspace_selector_for_another_workspace_is_refused() {
        let capability = Arc::new(RecordingCapability::new(Vec::new()));
        let fixture = fixture(capability.clone());

        let error = fixture
            .service
            .plan_knowledge_search(
                &SemanticAccessContext::Host,
                PlanKnowledgeSearchRequestDto {
                    draft: KnowledgeQueryDraftDto {
                        scopes: vec![KnowledgeScopeSelectorDto {
                            kind: KnowledgeScopeSelectorKindDto::Workspace,
                            id: Some(Uuid::new_v4().to_string()),
                        }],
                        ..draft()
                    },
                    scope: KnowledgeScopeDto {
                        kind: KnowledgeScopeKindDto::EntireLibrary,
                        workspace_id: fixture.workspace_id,
                        enrolled_root_ids: Vec::new(),
                        folder: None,
                        semantic_source_ids: Vec::new(),
                    },
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect_err("a foreign workspace selector must be refused");

        assert!(matches!(
            error,
            crate::error::ApplicationError::InvalidRequest(_)
        ));
        assert!(capability.requests().is_empty());
    }

    /// A `workspace:` selector that names the requested workspace is honored,
    /// and the returned plan still describes exactly what was searched.
    #[tokio::test]
    async fn a_matching_workspace_selector_is_honored_and_reported_exactly() {
        let fixture = fixture(Arc::new(RecordingCapability::new(Vec::new())));

        let plan = fixture
            .service
            .plan_knowledge_search(
                &SemanticAccessContext::Host,
                PlanKnowledgeSearchRequestDto {
                    draft: KnowledgeQueryDraftDto {
                        scopes: vec![KnowledgeScopeSelectorDto {
                            kind: KnowledgeScopeSelectorKindDto::Workspace,
                            id: Some(fixture.workspace_id.to_string()),
                        }],
                        ..draft()
                    },
                    scope: KnowledgeScopeDto {
                        kind: KnowledgeScopeKindDto::EntireLibrary,
                        workspace_id: fixture.workspace_id,
                        enrolled_root_ids: Vec::new(),
                        folder: None,
                        semantic_source_ids: Vec::new(),
                    },
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect("a selector naming the requested workspace is honored");

        assert_eq!(plan.scope.kind, KnowledgeScopeKindDto::EntireLibrary);
        assert!(plan.scope.enrolled_root_ids.is_empty());
        assert!(plan.scope_is_exact);
    }

    /// A folder scope is not expressible as an index filter, so its exact
    /// authorized sources travel with the request and the worker applies them
    /// before spending its result budget.
    #[tokio::test]
    async fn a_folder_scope_sends_its_exact_authorized_sources_to_the_worker() {
        let capability = Arc::new(RecordingCapability::new(Vec::new()));
        let fixture = fixture(capability.clone());
        capability.set_evidence(vec![evidence("r1", &fixture.source_id)]);

        let result = fixture
            .service
            .execute_knowledge_search(
                &SemanticAccessContext::Host,
                ExecuteKnowledgeSearchRequestDto {
                    request_id: Uuid::new_v4(),
                    draft: draft(),
                    scope: KnowledgeScopeDto {
                        kind: KnowledgeScopeKindDto::CurrentFolder,
                        workspace_id: fixture.workspace_id,
                        enrolled_root_ids: Vec::new(),
                        folder: Some(
                            fm_domain::Location::parse("file:///indexed-library")
                                .expect("location")
                                .into(),
                        ),
                        semantic_source_ids: Vec::new(),
                    },
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect("folder-scoped search");

        let issued = capability.requests();
        assert_eq!(issued.len(), 1);
        assert_eq!(issued[0].scopes.len(), 1);
        assert!(
            issued[0].scopes[0]
                .source_restriction
                .allowed_source_ids
                .contains(&fixture.source_id)
        );
        assert_eq!(result.plan.scope.kind, KnowledgeScopeKindDto::CurrentFolder);
        assert!(result.plan.scope_is_exact);
        assert!(result.coverage.scope_is_exact);
    }

    /// Coverage and evidence state come from a fresh host snapshot: publication
    /// is unknown rather than assumed, and staleness is measured against the
    /// authoritative current fingerprint.
    #[tokio::test]
    async fn coverage_reports_unknown_publication_and_measured_staleness() {
        let capability = Arc::new(RecordingCapability::new(Vec::new()));
        let fixture = fixture(capability.clone());
        let mut indexed_from_older_bytes = evidence("r1", &fixture.source_id);
        indexed_from_older_bytes.indexed_content_hash = "sha256:older".to_owned();
        capability.set_evidence(vec![indexed_from_older_bytes]);

        let result = fixture
            .service
            .execute_knowledge_search(
                &SemanticAccessContext::Host,
                ExecuteKnowledgeSearchRequestDto {
                    request_id: Uuid::new_v4(),
                    draft: draft(),
                    scope: KnowledgeScopeDto {
                        kind: KnowledgeScopeKindDto::EntireLibrary,
                        workspace_id: fixture.workspace_id,
                        enrolled_root_ids: Vec::new(),
                        folder: None,
                        semantic_source_ids: Vec::new(),
                    },
                    mode: KnowledgeRetrievalModeDto::FullText,
                    options: None,
                },
            )
            .await
            .expect("search");

        assert_eq!(result.evidence[0].stale, Some(true));
        assert!(!result.evidence[0].unavailable);
        assert_eq!(result.coverage.eligible, 1);
        assert_eq!(result.coverage.indexed, None);
        assert_eq!(result.coverage.fingerprinted, 1);
        assert_eq!(result.coverage.stale_evidence, 1);
        assert_eq!(result.coverage.unknown_freshness_evidence, 0);
        assert!(result.coverage.partial);
    }
}

#[cfg(test)]
mod partition_tests {
    use std::collections::{BTreeMap, BTreeSet, HashMap};

    use fm_semantic_worker::knowledge_retrieval::MAX_ALLOWED_SOURCES;
    use fm_semantic_worker::semantic_storage::QueryFilters;

    use super::partition_scope;
    use crate::semantic_library::ResolvedKnowledgeScope;

    fn sources(prefix: &str, count: usize) -> BTreeSet<String> {
        (0..count)
            .map(|index| format!("{prefix}-{index}"))
            .collect()
    }

    fn scope(
        filters_are_exact: bool,
        sources_by_root: BTreeMap<String, BTreeSet<String>>,
    ) -> ResolvedKnowledgeScope {
        let allowed_source_ids = sources_by_root.values().flatten().cloned().collect();
        ResolvedKnowledgeScope {
            filters: QueryFilters {
                tenant_id: "tenant".to_owned(),
                library_id: Some("library".to_owned()),
                include_unavailable: true,
                ..QueryFilters::default()
            },
            filters_are_exact,
            allowed_source_ids,
            sources_by_root,
            unavailable_source_ids: BTreeSet::new(),
            fingerprints: HashMap::new(),
            titles: HashMap::new(),
            eligible: 0,
        }
    }

    #[test]
    fn a_multi_root_scope_is_partitioned_by_root_so_no_root_truncates_another() {
        let resolved = scope(
            true,
            BTreeMap::from([
                ("root-a".to_owned(), sources("a", 2)),
                ("root-b".to_owned(), sources("b", 2)),
            ]),
        );

        let (partitions, exact) = partition_scope(&resolved, false);

        assert!(exact);
        assert_eq!(partitions.len(), 2);
        assert_eq!(partitions[0].filters.root_id.as_deref(), Some("root-a"));
        assert_eq!(partitions[1].filters.root_id.as_deref(), Some("root-b"));
        assert!(
            partitions
                .iter()
                .all(|partition| partition.restriction.allowed_source_ids.is_empty())
        );
    }

    #[test]
    fn an_inexpressible_scope_travels_as_its_exact_authorized_sources() {
        let resolved = scope(
            false,
            BTreeMap::from([("root-a".to_owned(), sources("a", 3))]),
        );

        let (partitions, exact) = partition_scope(&resolved, false);

        assert!(exact);
        assert_eq!(partitions.len(), 1);
        assert_eq!(
            partitions[0].restriction.allowed_source_ids,
            resolved.allowed_source_ids
        );
    }

    #[test]
    fn an_oversized_scope_is_split_per_root_rather_than_truncated() {
        let resolved = scope(
            false,
            BTreeMap::from([
                ("root-a".to_owned(), sources("a", MAX_ALLOWED_SOURCES)),
                ("root-b".to_owned(), sources("b", 4)),
            ]),
        );

        let (partitions, exact) = partition_scope(&resolved, false);

        assert!(exact);
        assert_eq!(partitions.len(), 2);
        assert_eq!(
            partitions[0].restriction.allowed_source_ids.len(),
            MAX_ALLOWED_SOURCES
        );
        assert_eq!(partitions[1].restriction.allowed_source_ids.len(), 4);
    }

    /// A scope larger than any exact representation is still searched, but the
    /// broadening is reported rather than presented as an exact result.
    #[test]
    fn a_scope_too_large_to_express_exactly_is_reported_not_hidden() {
        let resolved = scope(
            false,
            BTreeMap::from([("root-a".to_owned(), sources("a", MAX_ALLOWED_SOURCES + 1))]),
        );

        let (partitions, exact) = partition_scope(&resolved, false);

        assert!(!exact);
        assert_eq!(partitions.len(), 1);
        assert!(partitions[0].filters.root_id.is_none());
        assert!(partitions[0].restriction.allowed_source_ids.is_empty());
    }

    #[test]
    fn an_oversized_multi_root_scope_broadens_instead_of_truncating_roots() {
        let roots = (0..=crate::knowledge_search::MAX_SCOPE_PARTITIONS)
            .map(|index| {
                (
                    format!("root-{index}"),
                    sources(&format!("source-{index}"), MAX_ALLOWED_SOURCES),
                )
            })
            .collect();
        let resolved = scope(false, roots);

        let (partitions, exact) = partition_scope(&resolved, false);

        assert!(!exact);
        assert_eq!(partitions.len(), 1);
        assert!(partitions[0].filters.root_id.is_none());
        assert!(partitions[0].restriction.allowed_source_ids.is_empty());
    }

    #[test]
    fn whole_library_uses_one_exact_partition_regardless_of_root_count() {
        let roots = (0..=crate::knowledge_search::MAX_SCOPE_PARTITIONS)
            .map(|index| {
                (
                    format!("root-{index}"),
                    sources(&format!("source-{index}"), 1),
                )
            })
            .collect();
        let resolved = scope(true, roots);

        let (partitions, exact) = partition_scope(&resolved, true);

        assert!(exact);
        assert_eq!(partitions.len(), 1);
        assert!(partitions[0].filters.root_id.is_none());
        assert!(partitions[0].restriction.allowed_source_ids.is_empty());
    }
}

/// End-to-end coverage of the optional answer path over a real profile.
///
/// These exercise the only bridge between search and answer: a bounded cache
/// of the evidence a search already displayed. The retrieval capability counts
/// its calls, so "answering never reruns retrieval" is asserted rather than
/// assumed.
#[cfg(test)]
mod answer_flow_tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use fm_credentials::InMemoryCredentialStore;
    use fm_settings::SettingsStore;
    use fm_transport_dto::{
        ExecuteKnowledgeSearchRequestDto, GenerateKnowledgeAnswerRequestDto, KnowledgeActionDto,
        KnowledgeAnswerDepthDto, KnowledgeNeedDto, KnowledgeOutputFormatDto,
        KnowledgeQueryDraftDto, KnowledgeRetrievalModeDto, KnowledgeScopeDto,
        KnowledgeScopeKindDto, ResolveKnowledgeSourceRequestDto,
    };
    use tokio_util::sync::CancellationToken;
    use uuid::Uuid;

    use super::tests::{RecordingCapability, evidence};
    use super::{KnowledgeAuthority, KnowledgeService};
    use crate::error::ApplicationError;
    use crate::llm_profiles::{
        LlmChatGeneration, LlmHostPolicy, LlmProbeRequest, LlmProbeResponse, LlmProbeTransport,
        LlmProfileError, LlmProfileService,
    };
    use crate::semantic_library::{
        SemanticAccessContext, SemanticFolderContext, SemanticIndexingObservation,
        SemanticLibraryService,
    };

    struct AnswerTransport {
        generations: Mutex<Vec<LlmChatGeneration>>,
        answer: String,
    }

    impl AnswerTransport {
        fn new(answer: &str) -> Self {
            Self {
                generations: Mutex::new(Vec::new()),
                answer: answer.to_owned(),
            }
        }

        fn generations(&self) -> Vec<LlmChatGeneration> {
            self.generations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    #[async_trait]
    impl LlmProbeTransport for AnswerTransport {
        async fn discover_models(
            &self,
            _request: &LlmProbeRequest,
            _cancellation: &CancellationToken,
        ) -> Result<Option<Vec<String>>, LlmProfileError> {
            Ok(None)
        }

        async fn stream_chat(
            &self,
            _request: &LlmProbeRequest,
            _cancellation: &CancellationToken,
        ) -> Result<LlmProbeResponse, LlmProfileError> {
            Ok(LlmProbeResponse {
                status: 200,
                body: Vec::new(),
            })
        }

        async fn generate_chat(
            &self,
            _request: &LlmProbeRequest,
            generation: &LlmChatGeneration,
            cancellation: &CancellationToken,
        ) -> Result<String, LlmProfileError> {
            if cancellation.is_cancelled() {
                return Err(LlmProfileError::Cancelled);
            }
            self.generations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(generation.clone());
            Ok(self.answer.clone())
        }
    }

    struct Fixture {
        _directory: tempfile::TempDir,
        knowledge: KnowledgeService,
        authority: Arc<dyn KnowledgeAuthority>,
        library: Arc<SemanticLibraryService>,
        profiles: LlmProfileService,
        profile_id: Uuid,
        transport: Arc<AnswerTransport>,
        capability: Arc<RecordingCapability>,
        workspace_id: Uuid,
        source_id: String,
    }

    async fn fixture(answer: &str) -> Fixture {
        let directory = tempfile::tempdir().expect("temporary directory");
        let library = Arc::new(SemanticLibraryService::deterministic_mock());
        let workspace_id = fm_domain::WorkspaceId::new();
        let folder = SemanticFolderContext::new(
            workspace_id,
            fm_domain::Location::parse("file:///indexed-library").expect("location"),
        );
        let preview = library
            .preview_enrolment(&SemanticAccessContext::Host, folder.clone(), true)
            .expect("preview enrolment");
        library
            .confirm_enrolment(
                &SemanticAccessContext::Host,
                &preview.confirmation_id,
                preview.policy_revision,
                &folder,
            )
            .expect("confirm enrolment");
        let root_id = library
            .status(&SemanticAccessContext::Host)
            .expect("status")
            .roots[0]
            .id
            .clone();
        let source_id = library
            .record_indexing_observation(
                &SemanticAccessContext::Host,
                SemanticIndexingObservation {
                    entry_id: fm_domain::EntryId::new(),
                    location: fm_domain::Location::parse("file:///indexed-library/turbines.md")
                        .expect("location"),
                    content_fingerprint: fm_semantic_library::ContentFingerprint::new(
                        "sha256:turbines",
                    )
                    .expect("fingerprint"),
                    root_id: root_id.parse().expect("root id"),
                    workspace_ids: vec![workspace_id],
                    source_bytes: 512,
                },
            )
            .expect("record observation")
            .to_string();
        let capability = Arc::new(RecordingCapability::new(Vec::new()));
        capability.set_evidence(vec![evidence("r1", &source_id)]);
        let transport = Arc::new(AnswerTransport::new(answer));
        let profiles = LlmProfileService::new(
            SettingsStore::new(directory.path().join("settings")),
            Arc::new(InMemoryCredentialStore::new()),
            transport.clone(),
            LlmHostPolicy::desktop(),
        )
        .expect("profile service");
        let mut draft = LlmProfileService::presets().remove(0);
        draft.model = "answer-model".to_owned();
        let profile = profiles.create(draft).await.expect("profile");
        Fixture {
            _directory: directory,
            knowledge: KnowledgeService::new(capability.clone()),
            authority: library.clone(),
            library,
            profiles,
            profile_id: profile.id,
            transport,
            capability,
            workspace_id: workspace_id.into(),
            source_id,
        }
    }

    fn search_request(workspace_id: Uuid) -> ExecuteKnowledgeSearchRequestDto {
        ExecuteKnowledgeSearchRequestDto {
            request_id: Uuid::new_v4(),
            draft: KnowledgeQueryDraftDto {
                about: vec!["wind turbines".to_owned()],
                needs: vec![KnowledgeNeedDto::Definition],
                ..KnowledgeQueryDraftDto::default()
            },
            scope: KnowledgeScopeDto {
                kind: KnowledgeScopeKindDto::EntireLibrary,
                workspace_id,
                enrolled_root_ids: Vec::new(),
                folder: None,
                semantic_source_ids: Vec::new(),
            },
            mode: KnowledgeRetrievalModeDto::FullText,
            options: None,
        }
    }

    fn answer_request(
        workspace_id: Uuid,
        profile_id: Uuid,
        evidence_fingerprint: &str,
    ) -> GenerateKnowledgeAnswerRequestDto {
        GenerateKnowledgeAnswerRequestDto {
            request_id: Uuid::new_v4(),
            workspace_id,
            evidence_fingerprint: evidence_fingerprint.to_owned(),
            profile_id,
            allow_model_knowledge: false,
            action: Some(KnowledgeActionDto::Explain),
            context: Some("a maintenance report".to_owned()),
            constraints: vec!["cite sources".to_owned()],
            depth: Some(KnowledgeAnswerDepthDto::Brief),
            output: Some(KnowledgeOutputFormatDto::Bullets),
        }
    }

    #[tokio::test]
    async fn an_answer_consumes_the_inspected_evidence_without_retrieving_again() {
        let fixture = fixture("Rotor blades convert wind into torque [E1].").await;
        let result = fixture
            .knowledge
            .execute(
                fixture.authority.clone(),
                &SemanticAccessContext::Host,
                search_request(fixture.workspace_id),
                true,
                &CancellationToken::new(),
            )
            .await
            .expect("search");
        assert_eq!(result.evidence.len(), 1);
        assert_eq!(result.evidence[0].source_id, fixture.source_id);
        assert_eq!(fixture.capability.requests().len(), 1);

        let answer = fixture
            .knowledge
            .answer(
                fixture.authority.clone(),
                &SemanticAccessContext::Host,
                answer_request(
                    fixture.workspace_id,
                    fixture.profile_id,
                    &result.evidence_fingerprint,
                ),
                &fixture.profiles,
                &CancellationToken::new(),
            )
            .await
            .expect("answer from the inspected evidence");

        assert_eq!(
            fixture.capability.requests().len(),
            1,
            "answer generation must never rerun retrieval"
        );
        assert_eq!(answer.evidence_fingerprint, result.evidence_fingerprint);
        assert!(!answer.insufficient);
        assert!(!answer.model_knowledge_allowed);
        assert_eq!(answer.citations.len(), 1);
        // The citation must name evidence the user already saw, and it must
        // open through the existing knowledge source authority.
        assert_eq!(answer.citations[0].record_id, result.evidence[0].record_id);
        assert_eq!(answer.citations[0].source_id, result.evidence[0].source_id);
        assert_eq!(
            answer.citations[0].final_rank,
            result.evidence[0].final_rank
        );
        let location = fixture
            .knowledge
            .resolve_source(
                fixture.library.as_ref(),
                &SemanticAccessContext::Host,
                ResolveKnowledgeSourceRequestDto {
                    workspace_id: fixture.workspace_id,
                    source_id: answer.citations[0].source_id.clone(),
                },
            )
            .expect("a citation resolves through the existing source authority");
        assert_eq!(location.location.uri, "file:///indexed-library/turbines.md");

        // Answer-only fields shape the answer, never a search.
        let generation = fixture.transport.generations().remove(0);
        assert!(generation.user_prompt.contains("a maintenance report"));
        assert!(generation.system_prompt.contains("brief"));
        assert!(generation.system_prompt.contains("bullet points"));
        assert!(
            !generation.system_prompt.contains("a maintenance report"),
            "answer-only free text must stay data"
        );
    }

    /// The same fingerprint must keep producing the same citation identities,
    /// and answering twice must still never retrieve again.
    #[tokio::test]
    async fn repeated_answers_over_one_evidence_set_keep_citation_identities_stable() {
        let fixture = fixture("Blades convert wind [E1].").await;
        let result = fixture
            .knowledge
            .execute(
                fixture.authority.clone(),
                &SemanticAccessContext::Host,
                search_request(fixture.workspace_id),
                true,
                &CancellationToken::new(),
            )
            .await
            .expect("search");

        let mut identities = Vec::new();
        for _ in 0..2 {
            let answer = fixture
                .knowledge
                .answer(
                    fixture.authority.clone(),
                    &SemanticAccessContext::Host,
                    answer_request(
                        fixture.workspace_id,
                        fixture.profile_id,
                        &result.evidence_fingerprint,
                    ),
                    &fixture.profiles,
                    &CancellationToken::new(),
                )
                .await
                .expect("answer");
            identities.push(
                answer
                    .citations
                    .iter()
                    .map(|citation| {
                        (
                            citation.label.clone(),
                            citation.record_id.clone(),
                            citation.source_id.clone(),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
        }

        assert_eq!(identities[0], identities[1]);
        assert!(!identities[0].is_empty());
        assert_eq!(fixture.capability.requests().len(), 1);
    }

    /// Consent revoked between search and answer must remove evidence before
    /// any prompt is built, without disclosing what was removed.
    #[tokio::test]
    async fn evidence_revoked_after_the_search_is_denied_before_generation() {
        let fixture = fixture("unused").await;
        let result = fixture
            .knowledge
            .execute(
                fixture.authority.clone(),
                &SemanticAccessContext::Host,
                search_request(fixture.workspace_id),
                true,
                &CancellationToken::new(),
            )
            .await
            .expect("search");
        assert_eq!(result.evidence.len(), 1);
        let folder = SemanticFolderContext::new(
            fixture.workspace_id.into(),
            fm_domain::Location::parse("file:///indexed-library").expect("location"),
        );
        let revision = fixture
            .library
            .status(&SemanticAccessContext::Host)
            .expect("status")
            .revision;
        let plan = fixture
            .library
            .plan_exclusion(&SemanticAccessContext::Host, folder.clone(), revision)
            .expect("plan exclusion");
        fixture
            .library
            .confirm_exclusion(
                &SemanticAccessContext::Host,
                &plan.confirmation_id,
                plan.policy_revision,
                &folder,
            )
            .expect("exclusion revokes the scope");

        let error = fixture
            .knowledge
            .answer(
                fixture.authority.clone(),
                &SemanticAccessContext::Host,
                answer_request(
                    fixture.workspace_id,
                    fixture.profile_id,
                    &result.evidence_fingerprint,
                ),
                &fixture.profiles,
                &CancellationToken::new(),
            )
            .await
            .expect_err("revoked evidence must never be answered from");

        assert!(
            matches!(error, ApplicationError::PermissionDenied),
            "revoked evidence must be denied, got {error:?}"
        );
        assert!(
            fixture.transport.generations().is_empty(),
            "revoked evidence must never reach an endpoint"
        );
        assert_eq!(fixture.capability.requests().len(), 1);
    }

    /// Retention is bounded, so an evidence set displaced by newer searches
    /// must demand an explicit new search rather than be silently re-retrieved.
    #[tokio::test]
    async fn an_evicted_evidence_set_requires_an_explicit_refresh() {
        let fixture = fixture("unused").await;
        let first = fixture
            .knowledge
            .execute(
                fixture.authority.clone(),
                &SemanticAccessContext::Host,
                search_request(fixture.workspace_id),
                true,
                &CancellationToken::new(),
            )
            .await
            .expect("first search");

        for index in 0..=crate::knowledge_evidence_cache::MAX_CACHED_EVIDENCE_SETS {
            let mut request = search_request(fixture.workspace_id);
            request.draft.about = vec![format!("displacing subject {index}")];
            fixture
                .knowledge
                .execute(
                    fixture.authority.clone(),
                    &SemanticAccessContext::Host,
                    request,
                    true,
                    &CancellationToken::new(),
                )
                .await
                .expect("displacing search");
        }
        let retrievals = fixture.capability.requests().len();

        let error = fixture
            .knowledge
            .answer(
                fixture.authority.clone(),
                &SemanticAccessContext::Host,
                answer_request(
                    fixture.workspace_id,
                    fixture.profile_id,
                    &first.evidence_fingerprint,
                ),
                &fixture.profiles,
                &CancellationToken::new(),
            )
            .await
            .expect_err("an evicted evidence set is not answerable");

        match error {
            ApplicationError::KnowledgeEvidenceRefreshRequired {
                evidence_fingerprint,
            } => assert_eq!(evidence_fingerprint, first.evidence_fingerprint),
            other => panic!("expected a typed refresh-required failure, got {other:?}"),
        }
        assert_eq!(
            fixture.capability.requests().len(),
            retrievals,
            "an eviction must never trigger a replacement retrieval"
        );
        assert!(fixture.transport.generations().is_empty());
    }

    /// Cancellation must stop an answer before the endpoint is contacted, and
    /// must not fall back to a fresh retrieval.
    #[tokio::test]
    async fn a_cancelled_answer_contacts_no_endpoint_and_starts_no_retrieval() {
        let fixture = fixture("unused").await;
        let result = fixture
            .knowledge
            .execute(
                fixture.authority.clone(),
                &SemanticAccessContext::Host,
                search_request(fixture.workspace_id),
                true,
                &CancellationToken::new(),
            )
            .await
            .expect("search");
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let error = fixture
            .knowledge
            .answer(
                fixture.authority.clone(),
                &SemanticAccessContext::Host,
                answer_request(
                    fixture.workspace_id,
                    fixture.profile_id,
                    &result.evidence_fingerprint,
                ),
                &fixture.profiles,
                &cancellation,
            )
            .await
            .expect_err("a cancelled answer must not generate");

        assert!(matches!(error, ApplicationError::OperationCancelled));
        assert!(fixture.transport.generations().is_empty());
        assert_eq!(fixture.capability.requests().len(), 1);
    }
}
