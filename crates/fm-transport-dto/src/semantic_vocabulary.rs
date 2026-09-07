//! Stable HTTP/Tauri wire types for SKOS vocabularies and concept folders.
#![allow(missing_docs)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticVocabularyDto {
    pub id: String,
    pub name: String,
    pub concepts: Vec<SkosConceptDto>,
    pub workspace_ids: Vec<String>,
    pub root_ids: Vec<String>,
    pub review_queue: Vec<ConceptCandidateDto>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SkosConceptDto {
    pub uri: String,
    pub pref_labels: BTreeMap<String, String>,
    pub alt_labels: BTreeMap<String, Vec<String>>,
    pub definitions: BTreeMap<String, String>,
    pub scope_notes: BTreeMap<String, String>,
    pub broader: Vec<String>,
    pub narrower: Vec<String>,
    pub related: Vec<String>,
    #[schema(value_type = Object)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ConceptCandidateStatusDto {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConceptCandidateDto {
    pub id: String,
    pub label: String,
    pub synonyms: Vec<String>,
    pub supporting_chunk_ids: Vec<String>,
    pub confidence: f32,
    pub corpus_frequency: u64,
    pub status: ConceptCandidateStatusDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportSemanticVocabularyRequestDto {
    pub skos_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VocabularyIdRequestDto {
    pub vocabulary_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSemanticVocabularyRequestDto {
    pub vocabulary_id: String,
    #[serde(default)]
    pub confirm_affected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttachSemanticVocabularyRequestDto {
    pub vocabulary_id: String,
    pub workspace_id: Option<String>,
    pub root_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ReviewConceptCandidateActionDto {
    Accept,
    Edit,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewConceptCandidateRequestDto {
    pub vocabulary_id: String,
    pub candidate_id: String,
    pub action: ReviewConceptCandidateActionDto,
    pub concept_uri: Option<String>,
    pub preferred_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportSemanticVocabularyResponseDto {
    pub skos_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSemanticVocabularyImpactDto {
    pub vocabulary_id: String,
    pub affected_workspace_ids: Vec<String>,
    pub affected_root_ids: Vec<String>,
    pub requires_confirmation: bool,
    pub deleted: bool,
}
