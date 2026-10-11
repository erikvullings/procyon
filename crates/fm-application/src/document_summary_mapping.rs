use fm_semantic_worker::representative_selection::RepresentativeSelectionMode;
use fm_semantic_worker::semantic_storage::{
    StoredDocumentSummary, SummaryAttribution, SummaryAttributionSource,
};
use fm_transport_dto::document_summary::{SummaryAttributionDto, SummaryAttributionSourceDto};
use fm_transport_dto::{
    DocumentSummaryDto, DocumentSummaryPreviewDto, DocumentSummarySelectionModeDto,
    SummaryKeyPassageDto, SummaryProfileDisclosureDto,
};

use crate::document_summary::{DocumentSummaryError, DocumentSummaryPreview};
use crate::error::ApplicationError;
use crate::llm_profile_mapping::locality_to_dto;

pub(crate) fn preview_to_dto(value: DocumentSummaryPreview) -> DocumentSummaryPreviewDto {
    DocumentSummaryPreviewDto {
        selection_fingerprint: value.selection_fingerprint,
        representative_tokens: value.representative_tokens,
        selection_mode: match value.selection_mode {
            RepresentativeSelectionMode::AllChunks => DocumentSummarySelectionModeDto::FullDocument,
            RepresentativeSelectionMode::ClusterMedoids => {
                DocumentSummarySelectionModeDto::RepresentativePassages
            }
        },
        image_input_available: value.image_input_available,
        included_image_count: value.included_image_count,
        omitted_image_count: value.omitted_image_count,
        key_passages: value
            .representatives
            .into_iter()
            .enumerate()
            .map(|(index, representative)| SummaryKeyPassageDto {
                label: format!("S{}", index + 1),
                chunk_id: representative.source.chunk_id,
                content: representative.source.text,
                section_path: representative.source.section_path,
                provenance: representative.source.provenance,
                cluster_population: u32::try_from(representative.cluster_population)
                    .unwrap_or(u32::MAX),
                weight: representative.cluster_weight,
                structural_anchor: representative.structural_anchor,
            })
            .collect(),
        profile: value.profile.map(|profile| SummaryProfileDisclosureDto {
            profile_id: profile.profile_id,
            profile_name: profile.profile_name,
            model_id: profile.model_id,
            locality: locality_to_dto(profile.locality),
        }),
        reused_selection: value.reused_selection,
        source_tokens: value.source_tokens,
        source_pass_count: value.source_pass_count,
        effective_context_window: value.effective_context_window,
        provider_limit_verified: value.provider_limit_verified,
    }
}

pub(crate) fn summary_to_dto(value: StoredDocumentSummary, stale: bool) -> DocumentSummaryDto {
    DocumentSummaryDto {
        record_id: value.record_id,
        source_generation: value.source_generation,
        profile_id: value.profile_id,
        model_id: value.model_id,
        supporting_chunk_ids: value.supporting_chunk_ids,
        supporting_weights: value.supporting_weights,
        created_at_ms: value.created_at_ms,
        brief: value.brief_text,
        full: value.full_text,
        title: value.title.map(attribution_to_dto),
        authors: value.authors.into_iter().map(attribution_to_dto).collect(),
        stale,
    }
}

pub(crate) fn attribution_to_dto(value: SummaryAttribution) -> SummaryAttributionDto {
    SummaryAttributionDto {
        value: value.value,
        source: match value.source {
            SummaryAttributionSource::Metadata => SummaryAttributionSourceDto::Metadata,
            SummaryAttributionSource::Byline => SummaryAttributionSourceDto::Byline,
            SummaryAttributionSource::AiExtracted => SummaryAttributionSourceDto::AiExtracted,
        },
        supporting_chunk_id: value.supporting_chunk_id,
        provenance: value.provenance,
        quote: value.quote,
    }
}

pub(crate) fn summary_error_to_application(error: DocumentSummaryError) -> ApplicationError {
    match error {
        DocumentSummaryError::NotFound | DocumentSummaryError::ProfileUnavailable => {
            ApplicationError::NotFound
        }
        DocumentSummaryError::Cancelled => ApplicationError::OperationCancelled,
        DocumentSummaryError::ConsentRequired => ApplicationError::PermissionDenied,
        DocumentSummaryError::StaleConfirmation => {
            ApplicationError::InvalidRequest("document summary confirmation is stale".into())
        }
        DocumentSummaryError::InvalidRequest
        | DocumentSummaryError::ContextUnavailable
        | DocumentSummaryError::SourceBudgetExceeded
        | DocumentSummaryError::SourceIncomplete
        | DocumentSummaryError::InvalidGeneration => {
            ApplicationError::InvalidRequest(error.to_string())
        }
        DocumentSummaryError::Unavailable | DocumentSummaryError::GenerationFailed => {
            ApplicationError::ProviderUnavailable
        }
        DocumentSummaryError::Storage => ApplicationError::Internal,
    }
}
