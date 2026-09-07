use fm_transport_dto::{
    RagAnswerDto, RagAnswerEventDto, RagCitationDto, RagCoverageDto, RagEvidenceDto,
    RagPlanningFallbackReasonDto, RagPreviewDto, RagRetrievalStrategyDto, RagScopeDto,
    RagScopeKindDto, SavedRagConversationDto, SavedRagTurnDto,
};

use crate::error::ApplicationError;
use crate::llm_profile_mapping::locality_to_dto;
use crate::rag::{
    RagAnswer, RagAnswerEvent, RagError, RagPlanningFallbackReason, RagPreview,
    RagRetrievalStrategy, RagScope, SavedRagConversation, SavedRagTurn,
};

pub(crate) const fn strategy_from_dto(strategy: RagRetrievalStrategyDto) -> RagRetrievalStrategy {
    match strategy {
        RagRetrievalStrategyDto::SingleQuery => RagRetrievalStrategy::SingleQuery,
        RagRetrievalStrategyDto::MultiQuery => RagRetrievalStrategy::MultiQuery,
    }
}

const fn strategy_to_dto(strategy: RagRetrievalStrategy) -> RagRetrievalStrategyDto {
    match strategy {
        RagRetrievalStrategy::SingleQuery => RagRetrievalStrategyDto::SingleQuery,
        RagRetrievalStrategy::MultiQuery => RagRetrievalStrategyDto::MultiQuery,
    }
}

const fn fallback_to_dto(reason: RagPlanningFallbackReason) -> RagPlanningFallbackReasonDto {
    match reason {
        RagPlanningFallbackReason::Empty => RagPlanningFallbackReasonDto::Empty,
        RagPlanningFallbackReason::DuplicateOnly => RagPlanningFallbackReasonDto::DuplicateOnly,
        RagPlanningFallbackReason::Refused => RagPlanningFallbackReasonDto::Refused,
        RagPlanningFallbackReason::Malformed => RagPlanningFallbackReasonDto::Malformed,
        RagPlanningFallbackReason::TimedOut => RagPlanningFallbackReasonDto::TimedOut,
        RagPlanningFallbackReason::Unavailable => RagPlanningFallbackReasonDto::Unavailable,
        RagPlanningFallbackReason::Failed => RagPlanningFallbackReasonDto::Failed,
    }
}

pub(crate) fn scope_from_dto(scope: &RagScopeDto) -> RagScope {
    match scope.kind {
        RagScopeKindDto::EntireLibrary => RagScope::EntireLibrary {
            label: scope.label.clone(),
        },
        RagScopeKindDto::SelectedFiles => RagScope::SelectedFiles {
            label: scope.label.clone(),
        },
        RagScopeKindDto::CurrentFolder => RagScope::CurrentFolder {
            label: scope.label.clone(),
        },
        RagScopeKindDto::SemanticResults => RagScope::SemanticResults {
            label: scope.label.clone(),
        },
        RagScopeKindDto::EnrolledRoots => RagScope::EnrolledRoots {
            label: scope.label.clone(),
        },
    }
}

pub(crate) fn scope_to_dto(scope: RagScope, workspace_id: uuid::Uuid) -> RagScopeDto {
    let (kind, label) = match scope {
        RagScope::EntireLibrary { label } => (RagScopeKindDto::EntireLibrary, label),
        RagScope::SelectedFiles { label } => (RagScopeKindDto::SelectedFiles, label),
        RagScope::CurrentFolder { label } => (RagScopeKindDto::CurrentFolder, label),
        RagScope::SemanticResults { label } => (RagScopeKindDto::SemanticResults, label),
        RagScope::EnrolledRoots { label } => (RagScopeKindDto::EnrolledRoots, label),
    };
    RagScopeDto {
        kind,
        workspace_id,
        label,
        selected_files: Vec::new(),
        folder: None,
        semantic_source_ids: Vec::new(),
        enrolled_root_ids: Vec::new(),
    }
}

pub(crate) fn preview_to_dto(preview: RagPreview, scope: RagScopeDto) -> RagPreviewDto {
    RagPreviewDto {
        retrieval_fingerprint: preview.retrieval_fingerprint,
        scope,
        profile_id: preview.profile_id,
        profile_name: preview.profile_name,
        locality: locality_to_dto(preview.locality),
        evidence_tokens: u64::try_from(preview.evidence_tokens).unwrap_or(u64::MAX),
        evidence: preview
            .evidence
            .into_iter()
            .map(|evidence| RagEvidenceDto {
                label: evidence.label,
                source_id: evidence.source_id,
                excerpt: evidence.excerpt,
                title: evidence.title,
                section_path: evidence.section_path,
                provenance: evidence.provenance,
                score: evidence.score,
                generated: evidence.generated,
                stale: evidence.stale,
                available: evidence.available,
            })
            .collect(),
        coverage: RagCoverageDto {
            eligible: preview.coverage.eligible,
            indexed: preview.coverage.indexed,
            stale: preview.coverage.stale,
            pending: preview.coverage.pending,
            excluded: preview.coverage.excluded,
            failed: preview.coverage.failed,
            unavailable: preview.coverage.unavailable,
        },
        insufficient: preview.insufficient,
        requested_strategy: strategy_to_dto(preview.requested_strategy),
        applied_strategy: strategy_to_dto(preview.applied_strategy),
        planned_queries: preview.planned_queries,
        planner_version: preview.planner_version,
        fusion_version: preview.fusion_version,
        fallback_reason: preview.fallback_reason.map(fallback_to_dto),
    }
}

pub(crate) fn answer_to_dto(answer: RagAnswer) -> RagAnswerDto {
    RagAnswerDto {
        text: answer.text,
        citations: answer
            .citations
            .into_iter()
            .map(|citation| RagCitationDto {
                label: citation.label,
                source_id: citation.source_id,
                provenance: citation.provenance,
                unavailable: citation.unavailable,
                stale: citation.stale,
                generated: citation.generated,
            })
            .collect(),
        model_knowledge_allowed: answer.model_knowledge_allowed,
    }
}

pub(crate) fn events_to_dto(
    events: Vec<RagAnswerEvent>,
    scope: &RagScopeDto,
) -> Vec<RagAnswerEventDto> {
    events
        .into_iter()
        .map(|event| match event {
            RagAnswerEvent::Retrieval { preview } => RagAnswerEventDto::Retrieval {
                preview: Box::new(preview_to_dto(*preview, scope.clone())),
            },
            RagAnswerEvent::Token { text } => RagAnswerEventDto::Token { text },
            RagAnswerEvent::Done { answer } => RagAnswerEventDto::Done {
                answer: answer_to_dto(answer),
            },
        })
        .collect()
}

pub(crate) fn saved_to_dto(
    saved: SavedRagConversation,
    workspace_id: uuid::Uuid,
) -> SavedRagConversationDto {
    SavedRagConversationDto {
        id: saved.id,
        profile_id: saved.profile_id,
        scope: scope_to_dto(saved.scope, workspace_id),
        model_knowledge_allowed: saved.model_knowledge_allowed,
        retrieval_strategy: strategy_to_dto(saved.retrieval_strategy),
        turns: saved
            .turns
            .into_iter()
            .map(|turn: SavedRagTurn| SavedRagTurnDto {
                question: turn.question,
                answer: answer_to_dto(turn.answer),
            })
            .collect(),
        storage_bytes: saved.storage_bytes,
    }
}

pub(crate) fn rag_error_to_application(error: RagError) -> ApplicationError {
    match error {
        RagError::Unavailable | RagError::ProfileUnavailable => {
            ApplicationError::ProviderUnavailable
        }
        RagError::InvalidRequest | RagError::StaleConfirmation => {
            ApplicationError::InvalidRequest(error.to_string())
        }
        RagError::Cancelled => ApplicationError::OperationCancelled,
        RagError::ConsentRequired => ApplicationError::PermissionDenied,
        RagError::RetrievalFailed
        | RagError::GenerationFailed
        | RagError::Persistence
        | RagError::Serialization(_) => ApplicationError::Internal,
    }
}
