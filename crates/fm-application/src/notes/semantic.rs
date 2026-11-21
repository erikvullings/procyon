//! The semantic index of the notes folder, reached through Structured Knowledge Search scoped to
//! that folder (task 0337). Consent and authorization stay with the knowledge service: a folder
//! that is not in the semantic library, a missing worker or a closed release gate all make the
//! search fail, and the notes search then falls back to text.

use std::sync::Arc;

use async_trait::async_trait;
use fm_domain::Location;
use fm_transport_dto::{
    ExecuteKnowledgeSearchRequestDto, KnowledgeQueryDraftDto, KnowledgeQueryIntentDto,
    KnowledgeRetrievalModeDto, KnowledgeScopeDto, KnowledgeScopeKindDto,
    ResolveKnowledgeSourceRequestDto,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::search::{SemanticNotes, SemanticPassage, SemanticUnavailable};
use crate::knowledge_service::KnowledgeService;
use crate::semantic_library::{SemanticAccessContext, SemanticLibraryService};

/// The knowledge search, scoped to the notes folder for one workspace's consent.
pub(crate) struct KnowledgeNotesIndex<'a> {
    pub(crate) knowledge: &'a KnowledgeService,
    pub(crate) library: Arc<SemanticLibraryService>,
    pub(crate) access: &'a SemanticAccessContext,
    /// The workspace whose consent applies (already the consent workspace).
    pub(crate) workspace_id: Uuid,
    pub(crate) folder: Location,
}

#[async_trait]
impl SemanticNotes for KnowledgeNotesIndex<'_> {
    async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SemanticPassage>, SemanticUnavailable> {
        let request = ExecuteKnowledgeSearchRequestDto {
            request_id: Uuid::new_v4(),
            draft: KnowledgeQueryDraftDto {
                about: vec![query.to_owned()],
                ..KnowledgeQueryDraftDto::default()
            },
            scope: KnowledgeScopeDto {
                kind: KnowledgeScopeKindDto::CurrentFolder,
                workspace_id: self.workspace_id,
                enrolled_root_ids: Vec::new(),
                folder: Some(self.folder.clone().into()),
                semantic_source_ids: Vec::new(),
            },
            mode: KnowledgeRetrievalModeDto::Hybrid,
            intent: KnowledgeQueryIntentDto::Search,
            options: None,
        };
        let result = self
            .knowledge
            .execute(
                self.library.clone(),
                self.access,
                request,
                false,
                &CancellationToken::new(),
            )
            .await
            .map_err(|_| SemanticUnavailable)?;
        let mut passages = Vec::new();
        for evidence in result.evidence {
            if passages.len() >= limit {
                break;
            }
            if evidence.unavailable {
                continue;
            }
            let Ok(source) = self.knowledge.resolve_source(
                self.library.as_ref(),
                self.access,
                ResolveKnowledgeSourceRequestDto {
                    workspace_id: self.workspace_id,
                    source_id: evidence.source_id,
                },
            ) else {
                continue;
            };
            passages.push(SemanticPassage {
                location: source.location.into(),
                excerpt: evidence.excerpt,
            });
        }
        Ok(passages)
    }
}
