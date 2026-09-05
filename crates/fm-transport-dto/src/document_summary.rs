//! Transport-neutral document-summary requests and projections.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{LlmEndpointLocalityDto, LocationDto};

/// Exact file occurrence selected for summary operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSummaryTargetDto {
    /// Workspace through which the occurrence is authorized.
    pub workspace_id: Uuid,
    /// Stable provider entry identity.
    pub entry_id: Uuid,
    /// Exact provider-owned occurrence location.
    pub location: LocationDto,
}

/// Requests bounded representative key passages and generation disclosure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDocumentSummaryRequestDto {
    /// File occurrence to summarize.
    pub target: DocumentSummaryTargetDto,
    /// Maximum representative source tokens.
    pub input_token_budget: u32,
    /// Optional generation profile; omit to inspect key passages only.
    pub profile_id: Option<Uuid>,
}

/// One complete, unmodified source passage selected by the semantic worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SummaryKeyPassageDto {
    /// Opaque prompt/citation label.
    pub label: String,
    /// Stable extracted chunk identity.
    pub chunk_id: String,
    /// Complete selected structural chunk.
    pub content: String,
    /// Structural heading hierarchy.
    pub section_path: Vec<String>,
    /// Structured source position without host path metadata.
    pub provenance: String,
    /// Number of chunks represented by this passage.
    pub cluster_population: u32,
    /// Relative population weight.
    pub weight: f32,
    /// Whether structural coverage forced retention.
    pub structural_anchor: bool,
}

/// Content-free disclosure for the generation endpoint selected by the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SummaryProfileDisclosureDto {
    /// Selected saved profile.
    pub profile_id: Uuid,
    /// User-visible profile name.
    pub profile_name: String,
    /// Exact generation model configured by the profile.
    pub model_id: String,
    /// Whether representative text remains local or leaves the device.
    pub locality: LlmEndpointLocalityDto,
}

/// Confirmation preview. Without a profile, passages remain local and are
/// deliberately labelled as key passages rather than a generated summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSummaryPreviewDto {
    /// Stable confirmation fingerprint.
    pub selection_fingerprint: String,
    /// Conservative selected-input token estimate.
    pub representative_tokens: u32,
    /// Selected complete source passages.
    pub key_passages: Vec<SummaryKeyPassageDto>,
    /// Generation disclosure, absent in key-passages-only mode.
    pub profile: Option<SummaryProfileDisclosureDto>,
    /// Whether persisted support metadata was reused.
    pub reused_selection: bool,
}

/// Confirms generation against the exact representative-selection preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateDocumentSummaryRequestDto {
    /// File occurrence to summarize.
    pub target: DocumentSummaryTargetDto,
    /// Maximum representative source tokens.
    pub input_token_budget: u32,
    /// Fingerprint returned by the confirmed preview.
    pub expected_selection_fingerprint: String,
    /// Saved profile used for generation.
    pub profile_id: Uuid,
}

/// Reads the current summary for an authorized file occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetDocumentSummaryRequestDto {
    /// Authorized file occurrence whose current summary is requested.
    pub target: DocumentSummaryTargetDto,
}

/// Persisted generated summary with source-evidence identities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSummaryDto {
    /// Stable generated semantic-record identity.
    pub record_id: String,
    /// Source generation represented by the prose.
    pub source_generation: u64,
    /// Profile used to generate the prose.
    pub profile_id: String,
    /// Exact generation model identity.
    pub model_id: String,
    /// Extracted source chunks supporting this summary.
    pub supporting_chunk_ids: Vec<String>,
    /// Population weights aligned with supporting chunks.
    pub supporting_weights: Vec<f32>,
    /// Creation time in Unix milliseconds.
    pub created_at_ms: i64,
    /// Concise overview.
    pub brief: String,
    /// Full representative summary.
    pub full: String,
    /// Whether the indexed source changed after generation.
    pub stale: bool,
}
