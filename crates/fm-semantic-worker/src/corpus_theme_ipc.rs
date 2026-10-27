//! Wire model for the corpus-theme RPC.
//!
//! The request and response are serde projections of the worker's own theme
//! types, carried as JSON bytes inside one protobuf envelope. Fitted vectors
//! and centroids never cross this boundary: the response types hold keywords,
//! passages, mixtures and map coordinates only.

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::corpus_theme_storage::CorpusScope;
use crate::corpus_themes::{
    CorpusThemeConfig, CorpusThemeError, CorpusThemeService, CorpusThemeStatus, CorpusThemes,
    ThemeSources,
};

/// One corpus-theme operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase")]
pub enum ThemeRequest {
    /// Cheap revision and profile facts.
    Status {
        /// Authorized scope.
        scope: CorpusScope,
    },
    /// Cluster the scope.
    Analyze {
        /// Authorized scope.
        scope: CorpusScope,
        /// Analysis configuration.
        config: CorpusThemeConfig,
    },
    /// Sources contributing to one theme of a prior analysis.
    Sources {
        /// Authorized scope.
        scope: CorpusScope,
        /// Theme id, or `None` for the "no theme" bucket.
        theme: Option<u32>,
        /// Fingerprint of the analysis the caller holds.
        fingerprint: String,
        /// Maximum number of sources.
        limit: usize,
    },
}

/// Typed outcome of a corpus-theme operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum ThemeResponse {
    /// Answer to [`ThemeRequest::Status`].
    Status(CorpusThemeStatus),
    /// Answer to [`ThemeRequest::Analyze`].
    Themes(Box<CorpusThemes>),
    /// Answer to [`ThemeRequest::Sources`].
    Sources(ThemeSources),
    /// The operation failed in a way the host maps to a typed rejection.
    Failure(ThemeFailure),
}

/// Serializable mirror of [`CorpusThemeError`] without storage detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ThemeFailure {
    /// See [`CorpusThemeError::ProfileTooWeak`].
    ProfileTooWeak {
        /// Offending model identity.
        model_id: String,
    },
    /// See [`CorpusThemeError::ProfileMismatch`].
    ProfileMismatch,
    /// See [`CorpusThemeError::EmptyScope`].
    EmptyScope,
    /// See [`CorpusThemeError::InsufficientContent`].
    InsufficientContent {
        /// Chunks found.
        chunks: u64,
    },
    /// See [`CorpusThemeError::InvalidConfiguration`].
    InvalidConfiguration,
    /// See [`CorpusThemeError::InvalidEmbeddings`].
    InvalidEmbeddings,
    /// See [`CorpusThemeError::ThemesStale`].
    ThemesStale,
    /// See [`CorpusThemeError::CorruptRun`].
    CorruptRun,
    /// Any other worker-side failure.
    Other,
}

impl ThemeFailure {
    /// Converts a typed failure back for the host's existing mapping.
    #[must_use]
    pub fn into_error(self) -> CorpusThemeError {
        match self {
            Self::ProfileTooWeak { model_id } => CorpusThemeError::ProfileTooWeak { model_id },
            Self::ProfileMismatch => CorpusThemeError::ProfileMismatch,
            Self::EmptyScope => CorpusThemeError::EmptyScope,
            Self::InsufficientContent { chunks } => {
                CorpusThemeError::InsufficientContent { chunks }
            }
            Self::InvalidConfiguration => CorpusThemeError::InvalidConfiguration,
            Self::InvalidEmbeddings => CorpusThemeError::InvalidEmbeddings,
            Self::ThemesStale => CorpusThemeError::ThemesStale,
            Self::CorruptRun => CorpusThemeError::CorruptRun,
            Self::Other => CorpusThemeError::Remote,
        }
    }
}

/// Wire encoding failure; never carries host content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ThemeWireError {
    /// The payload is not a valid theme message.
    #[error("corpus theme message is malformed")]
    Malformed,
    /// Even the smallest map does not fit the negotiated message budget.
    #[error("corpus theme response exceeds the {maximum}-byte message budget")]
    ResponseTooLarge {
        /// Deterministic budget.
        maximum: usize,
    },
    /// The operation was cancelled.
    #[error("corpus theme operation was cancelled")]
    Cancelled,
}

/// Encodes a request.
///
/// # Errors
///
/// Returns [`ThemeWireError::Malformed`] when serialization fails.
pub fn encode_request(request: &ThemeRequest) -> Result<Vec<u8>, ThemeWireError> {
    serde_json::to_vec(request).map_err(|_| ThemeWireError::Malformed)
}

/// Decodes a request.
///
/// # Errors
///
/// Returns [`ThemeWireError::Malformed`] for invalid bytes.
pub fn decode_request(bytes: &[u8]) -> Result<ThemeRequest, ThemeWireError> {
    serde_json::from_slice(bytes).map_err(|_| ThemeWireError::Malformed)
}

/// Decodes a response.
///
/// # Errors
///
/// Returns [`ThemeWireError::Malformed`] for invalid bytes.
pub fn decode_response(bytes: &[u8]) -> Result<ThemeResponse, ThemeWireError> {
    serde_json::from_slice(bytes).map_err(|_| ThemeWireError::Malformed)
}

/// Encodes a response inside `budget` bytes.
///
/// An analysis whose map would exceed the budget is thinned (every second
/// point dropped, `map_sampled` set) until it fits; themes and mixtures are
/// never altered, so the thinned map is still a faithful sample.
///
/// # Errors
///
/// Returns [`ThemeWireError::ResponseTooLarge`] when no thinning fits.
pub fn encode_response_bounded(
    response: ThemeResponse,
    budget: usize,
) -> Result<Vec<u8>, ThemeWireError> {
    let mut response = response;
    loop {
        let bytes = serde_json::to_vec(&response).map_err(|_| ThemeWireError::Malformed)?;
        if bytes.len() <= budget {
            return Ok(bytes);
        }
        match &mut response {
            ThemeResponse::Themes(themes) if !themes.map_points.is_empty() => {
                let kept = themes
                    .map_points
                    .iter()
                    .step_by(2)
                    .cloned()
                    .collect::<Vec<_>>();
                themes.map_points = if themes.map_points.len() == 1 {
                    Vec::new()
                } else {
                    kept
                };
                themes.map_sampled = true;
            }
            _ => return Err(ThemeWireError::ResponseTooLarge { maximum: budget }),
        }
    }
}

/// Injectable corpus-theme backend used by the local IPC server.
pub trait WorkerCorpusThemeBackend: Send + Sync {
    /// Executes one operation; blocking and cancellable.
    ///
    /// # Errors
    ///
    /// Returns the typed worker failure.
    fn handle(
        &self,
        request: ThemeRequest,
        cancellation: &CancellationToken,
    ) -> Result<ThemeResponse, CorpusThemeError>;
}

impl WorkerCorpusThemeBackend for CorpusThemeService {
    fn handle(
        &self,
        request: ThemeRequest,
        cancellation: &CancellationToken,
    ) -> Result<ThemeResponse, CorpusThemeError> {
        match request {
            ThemeRequest::Status { scope } => self.status(&scope).map(ThemeResponse::Status),
            ThemeRequest::Analyze { scope, config } => self
                .analyze(&scope, &config, cancellation)
                .map(|themes| ThemeResponse::Themes(Box::new(themes))),
            ThemeRequest::Sources {
                scope,
                theme,
                fingerprint,
                limit,
            } => self
                .theme_sources(&scope, theme, &fingerprint, limit, cancellation)
                .map(ThemeResponse::Sources),
        }
    }
}

/// Maps a worker error to its wire failure; `Cancelled` stays an error.
///
/// # Errors
///
/// Returns [`ThemeWireError::Cancelled`] for cancellation.
pub fn failure_of(error: CorpusThemeError) -> Result<ThemeFailure, ThemeWireError> {
    Ok(match error {
        CorpusThemeError::Cancelled => return Err(ThemeWireError::Cancelled),
        CorpusThemeError::ProfileTooWeak { model_id } => ThemeFailure::ProfileTooWeak { model_id },
        CorpusThemeError::ProfileMismatch => ThemeFailure::ProfileMismatch,
        CorpusThemeError::EmptyScope => ThemeFailure::EmptyScope,
        CorpusThemeError::InsufficientContent { chunks } => {
            ThemeFailure::InsufficientContent { chunks }
        }
        CorpusThemeError::InvalidConfiguration => ThemeFailure::InvalidConfiguration,
        CorpusThemeError::InvalidEmbeddings => ThemeFailure::InvalidEmbeddings,
        CorpusThemeError::ThemesStale => ThemeFailure::ThemesStale,
        CorpusThemeError::CorruptRun => ThemeFailure::CorruptRun,
        CorpusThemeError::Storage(_) | CorpusThemeError::Remote => ThemeFailure::Other,
    })
}
