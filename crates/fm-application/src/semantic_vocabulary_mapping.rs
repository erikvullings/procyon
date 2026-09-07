use fm_transport_dto::{
    ConceptCandidateDto, ConceptCandidateStatusDto, SemanticVocabularyDto, SkosConceptDto,
};

use crate::ApplicationError;
use crate::semantic_vocabulary::{CandidateStatus, Vocabulary, VocabularyError};

pub(crate) fn vocabulary_to_dto(vocabulary: Vocabulary) -> SemanticVocabularyDto {
    SemanticVocabularyDto {
        id: vocabulary.id.as_str().to_owned(),
        name: vocabulary.name,
        concepts: vocabulary
            .concepts
            .into_values()
            .map(|concept| SkosConceptDto {
                uri: concept.uri,
                pref_labels: concept.pref_labels,
                alt_labels: concept.alt_labels,
                definitions: concept.definitions,
                scope_notes: concept.scope_notes,
                broader: concept.broader.into_iter().collect(),
                narrower: concept.narrower.into_iter().collect(),
                related: concept.related.into_iter().collect(),
                extensions: concept.extensions,
            })
            .collect(),
        workspace_ids: vocabulary.workspace_ids.into_iter().collect(),
        root_ids: vocabulary.root_ids.into_iter().collect(),
        review_queue: vocabulary
            .review_queue
            .into_values()
            .map(|candidate| ConceptCandidateDto {
                id: candidate.id,
                label: candidate.label,
                synonyms: candidate.synonyms,
                supporting_chunk_ids: candidate.supporting_chunk_ids,
                confidence: candidate.confidence,
                corpus_frequency: candidate.corpus_frequency,
                status: match candidate.status {
                    CandidateStatus::Pending => ConceptCandidateStatusDto::Pending,
                    CandidateStatus::Accepted => ConceptCandidateStatusDto::Accepted,
                    CandidateStatus::Rejected => ConceptCandidateStatusDto::Rejected,
                },
            })
            .collect(),
        revision: vocabulary.revision,
    }
}

pub(crate) fn vocabulary_error(error: VocabularyError) -> ApplicationError {
    ApplicationError::InvalidRequest(error.to_string())
}

pub(crate) fn library_error(
    error: crate::semantic_library::SemanticLibraryError,
) -> ApplicationError {
    use crate::semantic_library::SemanticLibraryError;
    match error {
        SemanticLibraryError::Unavailable => ApplicationError::ProviderUnavailable,
        SemanticLibraryError::AuthorityDenied { .. } => ApplicationError::PermissionDenied,
        SemanticLibraryError::NotFound | SemanticLibraryError::NotEnrolled => {
            ApplicationError::NotFound
        }
        _ => ApplicationError::InvalidRequest(error.to_string()),
    }
}
