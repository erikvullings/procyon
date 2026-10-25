//! Host orchestration for corpus themes.
//!
//! The semantic worker clusters indexed chunk embeddings; this module only
//! carries an already-authorized scope to it and projects the result into
//! transport DTOs. Vectors never cross this boundary.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use fm_semantic_worker::corpus_theme_storage::CorpusScope;
use fm_semantic_worker::corpus_themes::{
    CorpusThemeConfig, CorpusThemeError, CorpusThemeService, CorpusThemeStatus, CorpusThemes,
    DocumentThemeMixture, NoTheme, Theme, ThemePassage,
};
use fm_transport_dto::{
    CorpusThemeDto, CorpusThemeOutcomeDto, CorpusThemeStatusDto, CorpusThemesDto,
    CorpusThemesResponseDto, DocumentThemeMixtureDto, NoThemeDto, ThemeConceptDto, ThemeKeywordDto,
    ThemeMapPointDto, ThemePassageDto, ThemeShareDto,
};
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;
use crate::rejection::{Rejection, reason};

/// Failure of the host-side theme capability.
#[derive(Debug, thiserror::Error)]
pub enum CorpusThemeCapabilityError {
    /// No semantic worker capability was injected.
    #[error("corpus theme capability is unavailable")]
    Unavailable,
    /// The worker refused or failed the request.
    #[error(transparent)]
    Worker(#[from] CorpusThemeError),
}

/// Worker-side operations needed by the host.
#[async_trait]
pub trait CorpusThemeCapability: Send + Sync {
    /// Reports the scope's revision and whether themes are meaningful.
    async fn status(
        &self,
        scope: CorpusScope,
    ) -> Result<CorpusThemeStatus, CorpusThemeCapabilityError>;

    /// Clusters the scope's chunk embeddings.
    async fn analyze(
        &self,
        scope: CorpusScope,
        config: CorpusThemeConfig,
        cancellation: &CancellationToken,
    ) -> Result<CorpusThemes, CorpusThemeCapabilityError>;
}

/// Inert capability used until a host configures a semantic worker.
pub struct UnavailableCorpusThemeCapability;

#[async_trait]
impl CorpusThemeCapability for UnavailableCorpusThemeCapability {
    async fn status(
        &self,
        _scope: CorpusScope,
    ) -> Result<CorpusThemeStatus, CorpusThemeCapabilityError> {
        Err(CorpusThemeCapabilityError::Unavailable)
    }

    async fn analyze(
        &self,
        _scope: CorpusScope,
        _config: CorpusThemeConfig,
        _cancellation: &CancellationToken,
    ) -> Result<CorpusThemes, CorpusThemeCapabilityError> {
        Err(CorpusThemeCapabilityError::Unavailable)
    }
}

#[async_trait]
impl CorpusThemeCapability for CorpusThemeService {
    async fn status(
        &self,
        scope: CorpusScope,
    ) -> Result<CorpusThemeStatus, CorpusThemeCapabilityError> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || service.status(&scope))
            .await
            .map_err(|_| CorpusThemeError::Cancelled)?
            .map_err(CorpusThemeCapabilityError::from)
    }

    async fn analyze(
        &self,
        scope: CorpusScope,
        config: CorpusThemeConfig,
        cancellation: &CancellationToken,
    ) -> Result<CorpusThemes, CorpusThemeCapabilityError> {
        let service = self.clone();
        // Dropping this future (a disconnected client) cancels the CPU-bound work.
        let token = cancellation.child_token();
        let _guard = token.clone().drop_guard();
        tokio::task::spawn_blocking(move || service.analyze(&scope, &config, &token))
            .await
            .map_err(|_| CorpusThemeError::Cancelled)?
            .map_err(CorpusThemeCapabilityError::from)
    }
}

/// Coordinates theme requests over one worker capability.
pub struct CorpusThemeCoordinator {
    capability: Arc<dyn CorpusThemeCapability>,
}

impl CorpusThemeCoordinator {
    /// Creates a coordinator around an explicit capability.
    #[must_use]
    pub fn new(capability: Arc<dyn CorpusThemeCapability>) -> Self {
        Self { capability }
    }

    /// Current facts about an authorized scope.
    pub(crate) async fn status(
        &self,
        scope: CorpusScope,
    ) -> Result<CorpusThemeStatusDto, ApplicationError> {
        self.capability
            .status(scope)
            .await
            .map(status_to_dto)
            .map_err(theme_error_to_application)
    }

    /// Themes of an authorized scope, or the typed reason there are none.
    pub(crate) async fn analyze(
        &self,
        scope: CorpusScope,
        max_themes: Option<u32>,
        titles: &HashMap<String, String>,
        cancellation: &CancellationToken,
    ) -> Result<CorpusThemesResponseDto, ApplicationError> {
        let mut config = CorpusThemeConfig::default();
        if let Some(max_themes) = max_themes {
            if !(2..=24).contains(&max_themes) {
                return Err(ApplicationError::InvalidRequest(
                    "maxThemes must be between 2 and 24".to_owned(),
                ));
            }
            config.max_themes = max_themes as usize;
        }
        let model_id = self
            .capability
            .status(scope.clone())
            .await
            .map_err(theme_error_to_application)?
            .model_id;
        let without_themes = |outcome| CorpusThemesResponseDto {
            outcome,
            model_id: model_id.clone(),
            themes: None,
        };
        match self.capability.analyze(scope, config, cancellation).await {
            Ok(themes) => Ok(CorpusThemesResponseDto {
                outcome: CorpusThemeOutcomeDto::Ready,
                model_id: model_id.clone(),
                themes: Some(themes_to_dto(themes, titles)),
            }),
            Err(CorpusThemeCapabilityError::Worker(CorpusThemeError::ProfileTooWeak {
                ..
            })) => Ok(without_themes(CorpusThemeOutcomeDto::ProfileTooWeak)),
            Err(CorpusThemeCapabilityError::Worker(CorpusThemeError::InsufficientContent {
                ..
            })) => Ok(without_themes(CorpusThemeOutcomeDto::InsufficientContent)),
            Err(CorpusThemeCapabilityError::Worker(CorpusThemeError::EmptyScope)) => {
                Ok(without_themes(CorpusThemeOutcomeDto::NoContent))
            }
            Err(error) => Err(theme_error_to_application(error)),
        }
    }
}

fn status_to_dto(status: CorpusThemeStatus) -> CorpusThemeStatusDto {
    CorpusThemeStatusDto {
        index_revision: status.index_revision,
        document_count: status.document_count,
        chunk_count: status.chunk_count,
        model_id: status.model_id,
        profile_too_weak: status.profile_too_weak,
    }
}

pub(crate) fn theme_error_to_application(error: CorpusThemeCapabilityError) -> ApplicationError {
    match error {
        CorpusThemeCapabilityError::Unavailable => ApplicationError::ProviderUnavailable,
        CorpusThemeCapabilityError::Worker(error) => match error {
            CorpusThemeError::Cancelled => ApplicationError::OperationCancelled,
            CorpusThemeError::ProfileMismatch => ApplicationError::from(Rejection::new(
                reason::CORPUS_THEME_PROFILE_MISMATCH,
                "the indexed vectors belong to a different embedding model; rebuild the index",
            )),
            CorpusThemeError::InvalidConfiguration => {
                ApplicationError::InvalidRequest(error.to_string())
            }
            CorpusThemeError::ProfileTooWeak { .. }
            | CorpusThemeError::EmptyScope
            | CorpusThemeError::InsufficientContent { .. }
            | CorpusThemeError::InvalidEmbeddings
            | CorpusThemeError::CorruptRun
            | CorpusThemeError::Storage(_) => ApplicationError::Internal,
        },
    }
}

fn themes_to_dto(themes: CorpusThemes, titles: &HashMap<String, String>) -> CorpusThemesDto {
    let title = |source_id: &str| titles.get(source_id).cloned().unwrap_or_default();
    CorpusThemesDto {
        index_revision: themes.index_revision,
        fingerprint: themes.fingerprint,
        algorithm_version: themes.algorithm_version,
        model_id: themes.model_tag,
        chunk_count: themes.chunk_count,
        document_count: themes.document_count,
        sampled: themes.sampled,
        themes: themes
            .themes
            .into_iter()
            .map(|theme| theme_to_dto(theme, &title))
            .collect(),
        no_theme: no_theme_to_dto(themes.no_theme, &title),
        document_mixtures: themes
            .document_mixtures
            .into_iter()
            .map(|mixture| mixture_to_dto(mixture, &title))
            .collect(),
        mixtures_truncated: themes.mixtures_truncated,
        map_points: themes
            .map_points
            .into_iter()
            .map(|point| ThemeMapPointDto {
                x: point.x,
                y: point.y,
                theme: point.theme,
                passage: passage_to_dto(point.passage, &title),
            })
            .collect(),
        map_sampled: themes.map_sampled,
    }
}

fn theme_to_dto(theme: Theme, title: &impl Fn(&str) -> String) -> CorpusThemeDto {
    CorpusThemeDto {
        id: theme.id,
        chunk_count: theme.chunk_count,
        document_count: theme.document_count,
        share: theme.share,
        keywords: theme
            .keywords
            .into_iter()
            .map(|keyword| ThemeKeywordDto {
                term: keyword.term,
                score: keyword.score,
            })
            .collect(),
        passages: theme
            .passages
            .into_iter()
            .map(|passage| passage_to_dto(passage, title))
            .collect(),
        concepts: theme
            .concepts
            .into_iter()
            .map(|concept| ThemeConceptDto {
                concept_id: concept.concept_id,
                chunk_count: concept.chunk_count,
            })
            .collect(),
    }
}

fn no_theme_to_dto(no_theme: NoTheme, title: &impl Fn(&str) -> String) -> NoThemeDto {
    NoThemeDto {
        chunk_count: no_theme.chunk_count,
        document_count: no_theme.document_count,
        share: no_theme.share,
        passages: no_theme
            .passages
            .into_iter()
            .map(|passage| passage_to_dto(passage, title))
            .collect(),
    }
}

fn passage_to_dto(passage: ThemePassage, title: &impl Fn(&str) -> String) -> ThemePassageDto {
    ThemePassageDto {
        title: title(&passage.source_id),
        chunk_id: passage.chunk_id,
        source_id: passage.source_id,
        source_position: passage.source_position,
        section_path: passage.section_path,
        provenance: passage.provenance,
        excerpt: passage.excerpt,
    }
}

fn mixture_to_dto(
    mixture: DocumentThemeMixture,
    title: &impl Fn(&str) -> String,
) -> DocumentThemeMixtureDto {
    DocumentThemeMixtureDto {
        title: title(&mixture.source_id),
        source_id: mixture.source_id,
        chunk_count: mixture.chunk_count,
        shares: mixture
            .shares
            .into_iter()
            .map(|share| ThemeShareDto {
                theme: share.theme,
                chunk_count: share.chunk_count,
                share: share.share,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use fm_semantic_worker::corpus_themes::{MapPoint, ThemeShare};

    use super::*;

    fn passage(source: &str) -> ThemePassage {
        ThemePassage {
            chunk_id: "chunk-1".into(),
            document_id: "document-1".into(),
            source_id: source.into(),
            source_position: 4,
            section_path: vec![],
            provenance: "{\"kind\":\"jsonlRecord\",\"line\":42}".into(),
            excerpt: "text".into(),
            distance: 0.0,
        }
    }

    fn themes() -> CorpusThemes {
        CorpusThemes {
            algorithm_version: "v".into(),
            model_tag: "m".into(),
            index_revision: "rev".into(),
            fingerprint: "fp".into(),
            chunk_count: 1,
            document_count: 1,
            sampled: false,
            fitted_chunks: 1,
            themes: vec![],
            no_theme: NoTheme {
                chunk_count: 1,
                document_count: 1,
                share: 1.0,
                passages: vec![passage("source-a")],
            },
            document_mixtures: vec![DocumentThemeMixture {
                document_id: "document-1".into(),
                source_id: "source-a".into(),
                chunk_count: 1,
                shares: vec![ThemeShare {
                    theme: None,
                    chunk_count: 1,
                    share: 1.0,
                }],
            }],
            mixtures_truncated: false,
            map_points: vec![MapPoint {
                x: 0.5,
                y: -0.5,
                theme: None,
                passage: passage("source-a"),
            }],
            map_sampled: true,
            centroids_recomputed: 0,
            centroids_reused: 0,
            from_cache: false,
        }
    }

    #[test]
    fn dto_carries_host_titles_line_provenance_and_no_worker_identities() {
        let titles = HashMap::from([("source-a".to_owned(), "feed.jsonl".to_owned())]);
        let dto = themes_to_dto(themes(), &titles);
        assert_eq!(dto.map_points[0].passage.title, "feed.jsonl");
        assert!(dto.map_points[0].passage.provenance.contains("\"line\":42"));
        assert!(dto.map_sampled);
        assert_eq!(dto.document_mixtures[0].title, "feed.jsonl");
        let json = serde_json::to_string(&dto).unwrap();
        assert!(
            !json.contains("document-1"),
            "opaque worker document ids stay internal"
        );
    }

    #[test]
    fn unknown_titles_stay_empty_rather_than_exposing_source_ids_as_names() {
        let dto = themes_to_dto(themes(), &HashMap::new());
        assert_eq!(dto.map_points[0].passage.title, "");
    }
}
