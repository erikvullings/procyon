//! Transport-neutral document-summary requests and projections.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{LlmEndpointLocalityDto, LocationDto};

/// How the document text was selected for summary generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DocumentSummarySelectionModeDto {
    /// Every extracted text chunk fit within the configured model context.
    FullDocument,
    /// Representative passages were selected to stay within the input budget.
    RepresentativePassages,
}

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
    /// Whether bounded embedded images may be sent to a verified vision model.
    #[serde(default)]
    pub include_images: bool,
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
    /// Estimated tokens across the entire extracted text, including passages not previewed.
    pub source_tokens: u32,
    /// First bounded characters of extracted text in source order, not representative chunks.
    #[serde(default)]
    pub text_preview: String,
    /// Whether the source contains more extracted text than the visible preview.
    #[serde(default)]
    pub text_preview_truncated: bool,
    /// Whether the preview contains the complete extracted text or selected passages.
    pub selection_mode: DocumentSummarySelectionModeDto,
    /// Whether the selected model and document support bounded image input.
    pub image_input_available: bool,
    /// Embedded document images that will be sent to a verified vision model.
    pub included_image_count: u32,
    /// Embedded images omitted because of model capability or safety budgets.
    pub omitted_image_count: u32,
    /// Selected complete source passages.
    pub key_passages: Vec<SummaryKeyPassageDto>,
    /// Generation disclosure, absent in key-passages-only mode.
    pub profile: Option<SummaryProfileDisclosureDto>,
    /// Whether persisted support metadata was reused.
    pub reused_selection: bool,
    /// Number of bounded generation passes over complete source evidence.
    #[serde(default)]
    pub source_pass_count: u32,
    /// Context window used for summary budgeting; may be trusted from the saved profile.
    #[serde(default)]
    pub effective_context_window: Option<u32>,
    /// Whether the provider's context limit was verified.
    #[serde(default)]
    pub provider_limit_verified: bool,
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
    /// Whether bounded embedded images may be sent to a verified vision model.
    #[serde(default)]
    pub include_images: bool,
}

/// Reads the current summary for an authorized file occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetDocumentSummaryRequestDto {
    /// Authorized file occurrence whose current summary is requested.
    pub target: DocumentSummaryTargetDto,
}

/// Origin of a document title or author name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SummaryAttributionSourceDto {
    /// Explicit document metadata.
    Metadata,
    /// Explicit document byline.
    Byline,
    /// Model-extracted name grounded in source text.
    AiExtracted,
}

/// Source-backed title or author name, with optional passage identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SummaryAttributionDto {
    /// Exact document title or person name.
    pub value: String,
    /// Explicit or AI-extracted origin.
    pub source: SummaryAttributionSourceDto,
    /// Extracted passage identity when sourced from document text.
    pub supporting_chunk_id: Option<String>,
    /// Structured source position of that passage.
    pub provenance: Option<String>,
    /// Exact supplied source excerpt establishing an AI-extracted value.
    pub quote: Option<String>,
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
    /// Full summary grounded in the selected document evidence.
    pub full: String,
    /// Optional verified title (not inferred from the file name).
    #[serde(default)]
    pub title: Option<SummaryAttributionDto>,
    /// Explicit or evidence-backed author names.
    #[serde(default)]
    pub authors: Vec<SummaryAttributionDto>,
    /// Whether the indexed source changed after generation.
    pub stale: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_summary_without_attribution_deserializes() {
        let legacy = serde_json::json!({
            "recordId": "summary-1",
            "sourceGeneration": 1,
            "profileId": "profile",
            "modelId": "model",
            "supportingChunkIds": ["chunk-1"],
            "supportingWeights": [1.0],
            "createdAtMs": 1000,
            "brief": "Brief",
            "full": "Full",
            "stale": false
        });
        let summary: DocumentSummaryDto = serde_json::from_value(legacy).unwrap();
        assert_eq!(summary.title, None);
        assert!(summary.authors.is_empty());
        let encoded = serde_json::to_value(summary).unwrap();
        assert!(encoded.get("title").is_some());
        assert_eq!(encoded["authors"], serde_json::json!([]));
    }

    #[test]
    fn attribution_source_and_passage_identity_round_trip() {
        let attribution = SummaryAttributionDto {
            value: "Ada Lovelace".into(),
            source: SummaryAttributionSourceDto::Byline,
            supporting_chunk_id: Some("chunk-1".into()),
            provenance: Some(r#"{"kind":"textLines","start_line":1}"#.into()),
            quote: None,
        };
        let json = serde_json::to_value(&attribution).unwrap();
        assert_eq!(json["source"], "byline");
        assert_eq!(json["supportingChunkId"], "chunk-1");
        assert_eq!(
            serde_json::from_value::<SummaryAttributionDto>(json).unwrap(),
            attribution
        );
        assert_eq!(
            serde_json::to_value(SummaryAttributionSourceDto::AiExtracted).unwrap(),
            "aiExtracted"
        );
    }

    #[test]
    fn legacy_preview_defaults_multi_pass_disclosure() {
        let preview = serde_json::json!({
            "selectionFingerprint": "fingerprint",
            "representativeTokens": 20,
            "sourceTokens": 20,
            "selectionMode": "representativePassages",
            "imageInputAvailable": false,
            "includedImageCount": 0,
            "omittedImageCount": 0,
            "keyPassages": [],
            "profile": null,
            "reusedSelection": false
        });
        let parsed: DocumentSummaryPreviewDto = serde_json::from_value(preview).unwrap();
        assert_eq!(parsed.source_pass_count, 0);
        assert_eq!(parsed.text_preview, "");
        assert!(!parsed.text_preview_truncated);
        assert_eq!(parsed.effective_context_window, None);
        assert!(!parsed.provider_limit_verified);
    }
}
