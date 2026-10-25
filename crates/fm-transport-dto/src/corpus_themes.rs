//! Transport-neutral corpus theme requests and projections.
//!
//! Embedding vectors never appear here: a theme is described by its label
//! keywords, size, real example passages and a per-document mixture.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::RagScopeDto;

/// Requests themes for one authorized scope (library, folder, files, roots).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeCorpusThemesRequestDto {
    /// Visible authorized scope; re-authorized on every request.
    pub scope: RagScopeDto,
    /// Optional upper bound on discovered themes (2 to 24).
    #[serde(default)]
    pub max_themes: Option<u32>,
}

/// Asks whether the themes of a scope are available and current.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetCorpusThemeStatusRequestDto {
    /// Visible authorized scope.
    pub scope: RagScopeDto,
}

/// Why a scope can or cannot show themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum CorpusThemeOutcomeDto {
    /// Themes were computed.
    Ready,
    /// The compact token-hashing profile cannot separate topics meaningfully.
    ProfileTooWeak,
    /// The scope holds too few chunks to form themes.
    InsufficientContent,
    /// The scope has no indexed, available content.
    NoContent,
}

/// Current facts about a scope, without computing themes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CorpusThemeStatusDto {
    /// Digest of the documents and generations currently indexed in the scope.
    pub index_revision: String,
    /// Indexed documents in the scope.
    pub document_count: u32,
    /// Indexed chunks in the scope.
    pub chunk_count: u64,
    /// Active embedding model identity.
    pub model_id: String,
    /// Whether the active profile is too weak for meaningful themes.
    pub profile_too_weak: bool,
}

/// One scored label term.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThemeKeywordDto {
    /// Lower-cased term.
    pub term: String,
    /// c-TF-IDF weight.
    pub score: f32,
}

/// A real chunk that exemplifies a theme, with provenance to open it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThemePassageDto {
    /// Stable chunk identity.
    pub chunk_id: String,
    /// Opaque authorized source identity; resolve it to open the file.
    pub source_id: String,
    /// User-visible file title.
    pub title: String,
    /// Position of the chunk within its document.
    pub source_position: u32,
    /// Structural heading hierarchy.
    pub section_path: Vec<String>,
    /// Serialized provenance; carries the line number for JSONL records.
    pub provenance: String,
    /// Bounded opening of the chunk text.
    pub excerpt: String,
}

/// A SKOS concept shared by many chunks of a theme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThemeConceptDto {
    /// Stable concept identity.
    pub concept_id: String,
    /// Chunks of the theme carrying it.
    pub chunk_count: u64,
}

/// One discovered theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CorpusThemeDto {
    /// Stable id within this analysis; themes are ordered by size.
    pub id: u32,
    /// Chunks assigned to the theme.
    pub chunk_count: u64,
    /// Documents with at least one chunk in the theme.
    pub document_count: u32,
    /// Share of all analysed chunks.
    pub share: f32,
    /// Top c-TF-IDF terms.
    pub keywords: Vec<ThemeKeywordDto>,
    /// Chunks nearest the theme centroid.
    pub passages: Vec<ThemePassageDto>,
    /// Most common SKOS concepts, when the library is labelled.
    pub concepts: Vec<ThemeConceptDto>,
}

/// Chunks that fit no theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoThemeDto {
    /// Chunks in the bucket.
    pub chunk_count: u64,
    /// Documents with at least one such chunk.
    pub document_count: u32,
    /// Share of all analysed chunks.
    pub share: f32,
    /// A few examples.
    pub passages: Vec<ThemePassageDto>,
}

/// One slice of a document's theme distribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThemeShareDto {
    /// Theme id, or absent for the "no theme" bucket.
    pub theme: Option<u32>,
    /// Chunks of the document in this bucket.
    pub chunk_count: u32,
    /// Fraction of the document's chunks.
    pub share: f32,
}

/// A document's distribution over themes, largest share first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentThemeMixtureDto {
    /// Opaque authorized source identity.
    pub source_id: String,
    /// User-visible file title.
    pub title: String,
    /// Analysed chunks of the document.
    pub chunk_count: u32,
    /// Buckets with at least one chunk.
    pub shares: Vec<ThemeShareDto>,
}

/// Complete themes of one scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CorpusThemesDto {
    /// Digest of the indexed documents these themes describe; compare with the
    /// status revision to detect a stale analysis.
    pub index_revision: String,
    /// Digest of everything that determines this result.
    pub fingerprint: String,
    /// Versioned algorithm identity.
    pub algorithm_version: String,
    /// Embedding model that produced the clustered vectors.
    pub model_id: String,
    /// Chunks analysed.
    pub chunk_count: u64,
    /// Documents analysed.
    pub document_count: u32,
    /// Whether the clustering used a sample of the chunks.
    pub sampled: bool,
    /// Themes, largest first.
    pub themes: Vec<CorpusThemeDto>,
    /// Chunks that fit no theme.
    pub no_theme: NoThemeDto,
    /// Per-document theme distributions, bounded.
    pub document_mixtures: Vec<DocumentThemeMixtureDto>,
    /// Whether further document mixtures were left out.
    pub mixtures_truncated: bool,
}

/// Themes of a scope, or the typed reason there are none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CorpusThemesResponseDto {
    /// Typed outcome.
    pub outcome: CorpusThemeOutcomeDto,
    /// Active embedding model identity.
    pub model_id: String,
    /// Present only when the outcome is ready.
    pub themes: Option<CorpusThemesDto>,
}
