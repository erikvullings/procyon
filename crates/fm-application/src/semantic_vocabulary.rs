//! Device-local SKOS vocabularies and reviewed concept labelling.
#![allow(
    dead_code,
    reason = "worker-facing labelling primitives are consumed by injected semantic capabilities"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest;
use thiserror::Error;

const SKOS_FORMAT: &str = "procyon-skos-1";
const MAX_CONCEPTS: usize = 100_000;
const MAX_TEXT_BYTES: usize = 32 * 1024;
const MAX_EXTENSION_BYTES: usize = 256 * 1024;

/// Stable device-local vocabulary identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct VocabularyId(String);

impl VocabularyId {
    fn parse(value: String) -> Result<Self, VocabularyError> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-._".contains(character))
        {
            return Err(VocabularyError::InvalidVocabularyId(value));
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// One authoritative SKOS concept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkosConcept {
    pub(crate) uri: String,
    #[serde(rename = "prefLabels")]
    pub(crate) pref_labels: BTreeMap<String, String>,
    #[serde(rename = "altLabels", default)]
    pub(crate) alt_labels: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub(crate) definitions: BTreeMap<String, String>,
    #[serde(rename = "scopeNotes", default)]
    pub(crate) scope_notes: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) broader: BTreeSet<String>,
    #[serde(default)]
    pub(crate) narrower: BTreeSet<String>,
    #[serde(default)]
    pub(crate) related: BTreeSet<String>,
    /// Unknown safe fields are retained only in this explicit namespaced map.
    #[serde(default)]
    pub(crate) extensions: BTreeMap<String, Value>,
}

/// Provenance for a review-only candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CandidateProvenance {
    Statistical,
    Embedding,
    Llm {
        profile_id: String,
        model_version: String,
    },
}

/// Review status; only accepted candidates become authoritative concepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CandidateStatus {
    Pending,
    Accepted,
    Rejected,
}

/// One derived concept proposal with bounded local evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConceptCandidate {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) synonyms: Vec<String>,
    pub(crate) supporting_chunk_ids: Vec<String>,
    pub(crate) confidence: f32,
    pub(crate) corpus_frequency: u64,
    pub(crate) provenance: CandidateProvenance,
    pub(crate) status: CandidateStatus,
}

impl ConceptCandidate {
    pub(crate) fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        mut synonyms: Vec<String>,
        mut supporting_chunk_ids: Vec<String>,
        confidence: f32,
        corpus_frequency: u64,
        provenance: CandidateProvenance,
    ) -> Result<Self, VocabularyError> {
        let id = id.into();
        let label = label.into();
        validate_short_identifier(&id)?;
        validate_text(&label)?;
        if !(0.0..=1.0).contains(&confidence) || corpus_frequency == 0 {
            return Err(VocabularyError::InvalidCandidate(id));
        }
        synonyms.sort();
        synonyms.dedup();
        supporting_chunk_ids.sort();
        supporting_chunk_ids.dedup();
        if supporting_chunk_ids.is_empty() || supporting_chunk_ids.len() > 32 {
            return Err(VocabularyError::InvalidCandidate(id));
        }
        for value in &synonyms {
            validate_text(value)?;
        }
        for value in &supporting_chunk_ids {
            validate_short_identifier(value)?;
        }
        Ok(Self {
            id,
            label,
            synonyms,
            supporting_chunk_ids,
            confidence,
            corpus_frequency,
            provenance,
            status: CandidateStatus::Pending,
        })
    }
}

/// Explicit review action.
pub(crate) enum ReviewDecision {
    Accept,
    AcceptEdited {
        concept_uri: String,
        pref_label: String,
    },
    Reject,
}

/// Authoritative vocabulary plus its durable review queue and attachments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Vocabulary {
    pub(crate) id: VocabularyId,
    pub(crate) name: String,
    pub(crate) concepts: BTreeMap<String, SkosConcept>,
    #[serde(default)]
    pub(crate) extensions: BTreeMap<String, Value>,
    #[serde(default)]
    pub(crate) workspace_ids: BTreeSet<String>,
    #[serde(default)]
    pub(crate) root_ids: BTreeSet<String>,
    #[serde(default)]
    pub(crate) review_queue: BTreeMap<String, ConceptCandidate>,
    #[serde(default)]
    pub(crate) revision: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VocabularyStoreDocument {
    schema_version: u32,
    vocabularies: BTreeMap<String, Vocabulary>,
}

/// Durable authoritative vocabulary/review store. Derived vectors and labels are deliberately
/// absent so ordinary settings backup contains only sources and user decisions.
pub(crate) struct VocabularyStore {
    path: PathBuf,
    document: Mutex<VocabularyStoreDocument>,
}

impl VocabularyStore {
    pub(crate) fn open(path: impl Into<PathBuf>) -> Result<Self, VocabularyError> {
        let path = path.into();
        let document = match fs::read(&path) {
            Ok(bytes) => {
                let document: VocabularyStoreDocument = serde_json::from_slice(&bytes)
                    .map_err(|error| VocabularyError::Malformed(error.to_string()))?;
                if document.schema_version != 1 {
                    return Err(VocabularyError::UnsupportedStoreVersion(
                        document.schema_version,
                    ));
                }
                document
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => VocabularyStoreDocument {
                schema_version: 1,
                vocabularies: BTreeMap::new(),
            },
            Err(error) => return Err(VocabularyError::Storage(error.to_string())),
        };
        Ok(Self {
            path,
            document: Mutex::new(document),
        })
    }

    pub(crate) fn import(&self, source: &str) -> Result<Vocabulary, VocabularyError> {
        let vocabulary = import_skos_json(source)?;
        let mut document = self.lock()?;
        if document.vocabularies.contains_key(vocabulary.id.as_str()) {
            return Err(VocabularyError::DuplicateVocabulary(
                vocabulary.id.as_str().to_owned(),
            ));
        }
        document
            .vocabularies
            .insert(vocabulary.id.as_str().to_owned(), vocabulary.clone());
        self.persist(&document)?;
        Ok(vocabulary)
    }

    pub(crate) fn list(&self) -> Result<Vec<Vocabulary>, VocabularyError> {
        Ok(self.lock()?.vocabularies.values().cloned().collect())
    }

    pub(crate) fn get(&self, id: &str) -> Result<Vocabulary, VocabularyError> {
        self.lock()?
            .vocabularies
            .get(id)
            .cloned()
            .ok_or_else(|| VocabularyError::UnknownVocabulary(id.to_owned()))
    }

    pub(crate) fn update(
        &self,
        id: &str,
        update: impl FnOnce(&mut Vocabulary) -> Result<(), VocabularyError>,
    ) -> Result<Vocabulary, VocabularyError> {
        let mut document = self.lock()?;
        let vocabulary = document
            .vocabularies
            .get_mut(id)
            .ok_or_else(|| VocabularyError::UnknownVocabulary(id.to_owned()))?;
        update(vocabulary)?;
        let updated = vocabulary.clone();
        self.persist(&document)?;
        Ok(updated)
    }

    pub(crate) fn delete(&self, id: &str) -> Result<Vocabulary, VocabularyError> {
        let mut document = self.lock()?;
        let removed = document
            .vocabularies
            .remove(id)
            .ok_or_else(|| VocabularyError::UnknownVocabulary(id.to_owned()))?;
        self.persist(&document)?;
        Ok(removed)
    }

    fn lock(&self) -> Result<MutexGuard<'_, VocabularyStoreDocument>, VocabularyError> {
        self.document
            .lock()
            .map_err(|_| VocabularyError::Storage("vocabulary lock poisoned".into()))
    }

    fn persist(&self, document: &VocabularyStoreDocument) -> Result<(), VocabularyError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| VocabularyError::Storage(error.to_string()))?;
        }
        let bytes = serde_json::to_vec_pretty(document)
            .map_err(|error| VocabularyError::Malformed(error.to_string()))?;
        let temporary = temporary_path(&self.path);
        fs::write(&temporary, bytes)
            .map_err(|error| VocabularyError::Storage(error.to_string()))?;
        fs::rename(&temporary, &self.path)
            .map_err(|error| VocabularyError::Storage(error.to_string()))
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("json")
    ))
}

/// One locally extracted label occurrence used to build review-only candidates.
pub(crate) struct ExtractedConceptLabel {
    pub(crate) chunk_id: String,
    pub(crate) label: String,
    pub(crate) synonyms: Vec<String>,
}

/// Groups unsorted extracted labels deterministically without publishing them.
pub(crate) fn extract_concept_candidates(
    labels: &[ExtractedConceptLabel],
    maximum_candidates: usize,
) -> Result<Vec<ConceptCandidate>, VocabularyError> {
    if maximum_candidates == 0 || maximum_candidates > 1_000 {
        return Err(VocabularyError::InvalidCandidateLimit);
    }
    let mut grouped = BTreeMap::<String, (String, BTreeSet<String>, BTreeSet<String>, u64)>::new();
    for occurrence in labels {
        validate_short_identifier(&occurrence.chunk_id)?;
        validate_text(&occurrence.label)?;
        let key = occurrence.label.trim().to_lowercase();
        let entry = grouped.entry(key).or_insert_with(|| {
            (
                occurrence.label.trim().to_owned(),
                BTreeSet::new(),
                BTreeSet::new(),
                0,
            )
        });
        if occurrence.label.trim() < entry.0.as_str() {
            entry.0 = occurrence.label.trim().to_owned();
        }
        entry.1.insert(occurrence.chunk_id.clone());
        entry.2.extend(occurrence.synonyms.iter().cloned());
        entry.3 = entry.3.saturating_add(1);
    }
    let mut candidates = grouped
        .into_iter()
        .map(|(normalized, (label, chunks, synonyms, frequency))| {
            let digest = sha2::Sha256::digest(normalized.as_bytes());
            ConceptCandidate::new(
                format!("candidate-{}", hex_prefix(&digest, 12)),
                label,
                synonyms.into_iter().collect(),
                chunks.into_iter().take(32).collect(),
                (0.5 + (frequency.min(10) as f32 * 0.04)).min(0.9),
                frequency,
                CandidateProvenance::Statistical,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    candidates.sort_by(|left, right| {
        right
            .corpus_frequency
            .cmp(&left.corpus_frequency)
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.id.cmp(&right.id))
    });
    candidates.truncate(maximum_candidates);
    Ok(candidates)
}

fn hex_prefix(bytes: &[u8], length: usize) -> String {
    bytes
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .take(length)
        .map(|nibble| char::from_digit(u32::from(nibble), 16).expect("hex nibble"))
        .collect()
}

impl Vocabulary {
    pub(crate) fn attach(
        &mut self,
        workspace_id: Option<String>,
        root_id: Option<String>,
    ) -> Result<(), VocabularyError> {
        if workspace_id.is_none() && root_id.is_none() {
            return Err(VocabularyError::MissingAttachmentScope);
        }
        if let Some(workspace_id) = workspace_id {
            validate_short_identifier(&workspace_id)?;
            self.workspace_ids.insert(workspace_id);
        }
        if let Some(root_id) = root_id {
            validate_short_identifier(&root_id)?;
            self.root_ids.insert(root_id);
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub(crate) fn expanded_concept_uris(
        &self,
        concept_uri: &str,
        include_broader: bool,
        include_narrower: bool,
    ) -> Result<Vec<String>, VocabularyError> {
        if !self.concepts.contains_key(concept_uri) {
            return Err(VocabularyError::UnknownConcept(concept_uri.to_owned()));
        }
        let mut selected = BTreeSet::from([concept_uri.to_owned()]);
        let mut pending = vec![concept_uri.to_owned()];
        while let Some(uri) = pending.pop() {
            let concept = &self.concepts[&uri];
            let related = concept
                .broader
                .iter()
                .filter(|_| include_broader)
                .chain(concept.narrower.iter().filter(|_| include_narrower));
            for next in related {
                if selected.insert(next.clone()) {
                    pending.push(next.clone());
                }
            }
        }
        Ok(selected.into_iter().collect())
    }

    pub(crate) fn queue_candidate(
        &mut self,
        candidate: ConceptCandidate,
    ) -> Result<(), VocabularyError> {
        if self.review_queue.contains_key(&candidate.id) {
            return Err(VocabularyError::DuplicateCandidate(candidate.id));
        }
        self.review_queue.insert(candidate.id.clone(), candidate);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub(crate) fn review_candidate(
        &mut self,
        candidate_id: &str,
        decision: ReviewDecision,
    ) -> Result<(), VocabularyError> {
        let candidate = self
            .review_queue
            .get_mut(candidate_id)
            .ok_or_else(|| VocabularyError::UnknownCandidate(candidate_id.to_owned()))?;
        if candidate.status != CandidateStatus::Pending {
            return Err(VocabularyError::CandidateAlreadyReviewed(
                candidate_id.to_owned(),
            ));
        }
        match decision {
            ReviewDecision::Reject => candidate.status = CandidateStatus::Rejected,
            ReviewDecision::Accept | ReviewDecision::AcceptEdited { .. } => {
                let (uri, label) = match decision {
                    ReviewDecision::Accept => (
                        format!("urn:procyon:{}", candidate.id),
                        candidate.label.clone(),
                    ),
                    ReviewDecision::AcceptEdited {
                        concept_uri,
                        pref_label,
                    } => (concept_uri, pref_label),
                    ReviewDecision::Reject => unreachable!(),
                };
                validate_uri(&uri)?;
                validate_text(&label)?;
                if self.concepts.contains_key(&uri) {
                    return Err(VocabularyError::DuplicateConcept(uri));
                }
                self.concepts.insert(
                    uri.clone(),
                    SkosConcept {
                        uri,
                        pref_labels: BTreeMap::from([("und".into(), label)]),
                        alt_labels: BTreeMap::from([("und".into(), candidate.synonyms.clone())]),
                        definitions: BTreeMap::new(),
                        scope_notes: BTreeMap::new(),
                        broader: BTreeSet::new(),
                        narrower: BTreeSet::new(),
                        related: BTreeSet::new(),
                        extensions: BTreeMap::new(),
                    },
                );
                candidate.status = CandidateStatus::Accepted;
            }
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct VocabularyDocument {
    format: String,
    id: String,
    name: String,
    concepts: Vec<SkosConcept>,
    #[serde(default)]
    extensions: BTreeMap<String, Value>,
}

/// Imports the documented deterministic JSON representation of Procyon's SKOS subset.
pub(crate) fn import_skos_json(source: &str) -> Result<Vocabulary, VocabularyError> {
    if source.len() > 16 * 1024 * 1024 {
        return Err(VocabularyError::DocumentTooLarge);
    }
    let document: VocabularyDocument = serde_json::from_str(source)
        .map_err(|error| VocabularyError::Malformed(error.to_string()))?;
    if document.format != SKOS_FORMAT {
        return Err(VocabularyError::UnsupportedFormat(document.format));
    }
    let id = VocabularyId::parse(document.id)?;
    validate_text(&document.name)?;
    if document.concepts.is_empty() || document.concepts.len() > MAX_CONCEPTS {
        return Err(VocabularyError::InvalidConceptCount);
    }
    validate_extensions(&document.extensions)?;
    let mut concepts = BTreeMap::new();
    for concept in document.concepts {
        validate_concept(&concept)?;
        let uri = concept.uri.clone();
        if concepts.insert(uri.clone(), concept).is_some() {
            return Err(VocabularyError::DuplicateConcept(uri));
        }
    }
    validate_relationships(&concepts)?;
    Ok(Vocabulary {
        id,
        name: document.name,
        concepts,
        extensions: document.extensions,
        workspace_ids: BTreeSet::new(),
        root_ids: BTreeSet::new(),
        review_queue: BTreeMap::new(),
        revision: 1,
    })
}

/// Exports stable authoritative content without derived embeddings or annotations.
pub(crate) fn export_skos_json(vocabulary: &Vocabulary) -> Result<String, VocabularyError> {
    let document = VocabularyDocument {
        format: SKOS_FORMAT.into(),
        id: vocabulary.id.as_str().to_owned(),
        name: vocabulary.name.clone(),
        concepts: vocabulary.concepts.values().cloned().collect(),
        extensions: vocabulary.extensions.clone(),
    };
    serde_json::to_string_pretty(&document)
        .map_err(|error| VocabularyError::Malformed(error.to_string()))
}

fn validate_concept(concept: &SkosConcept) -> Result<(), VocabularyError> {
    validate_uri(&concept.uri)?;
    if concept.pref_labels.is_empty() {
        return Err(VocabularyError::MissingPreferredLabel(concept.uri.clone()));
    }
    for (language, label) in &concept.pref_labels {
        validate_language(language)?;
        validate_text(label)?;
    }
    for (language, labels) in &concept.alt_labels {
        validate_language(language)?;
        for label in labels {
            validate_text(label)?;
        }
    }
    for (language, value) in concept.definitions.iter().chain(&concept.scope_notes) {
        validate_language(language)?;
        validate_text(value)?;
    }
    validate_extensions(&concept.extensions)?;
    Ok(())
}

fn validate_relationships(concepts: &BTreeMap<String, SkosConcept>) -> Result<(), VocabularyError> {
    for concept in concepts.values() {
        for target in concept
            .broader
            .iter()
            .chain(&concept.narrower)
            .chain(&concept.related)
        {
            if target == &concept.uri || !concepts.contains_key(target) {
                return Err(VocabularyError::UnknownRelationship {
                    source_uri: concept.uri.clone(),
                    target: target.clone(),
                });
            }
        }
        for narrower in &concept.narrower {
            if !concepts[narrower].broader.contains(&concept.uri) {
                return Err(VocabularyError::AsymmetricHierarchy {
                    broader: concept.uri.clone(),
                    narrower: narrower.clone(),
                });
            }
        }
        for broader in &concept.broader {
            if !concepts[broader].narrower.contains(&concept.uri) {
                return Err(VocabularyError::AsymmetricHierarchy {
                    broader: broader.clone(),
                    narrower: concept.uri.clone(),
                });
            }
        }
    }
    let mut complete = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    for uri in concepts.keys() {
        visit_broader(uri, concepts, &mut visiting, &mut complete)?;
    }
    Ok(())
}

fn visit_broader(
    uri: &str,
    concepts: &BTreeMap<String, SkosConcept>,
    visiting: &mut BTreeSet<String>,
    complete: &mut BTreeSet<String>,
) -> Result<(), VocabularyError> {
    if complete.contains(uri) {
        return Ok(());
    }
    if !visiting.insert(uri.to_owned()) {
        return Err(VocabularyError::HierarchyCycle(uri.to_owned()));
    }
    for broader in &concepts[uri].broader {
        visit_broader(broader, concepts, visiting, complete)?;
    }
    visiting.remove(uri);
    complete.insert(uri.to_owned());
    Ok(())
}

fn validate_uri(value: &str) -> Result<(), VocabularyError> {
    if value.starts_with("urn:") || url::Url::parse(value).is_ok_and(|uri| uri.has_host()) {
        Ok(())
    } else {
        Err(VocabularyError::InvalidConceptUri(value.to_owned()))
    }
}

fn validate_language(value: &str) -> Result<(), VocabularyError> {
    if value == "und"
        || (!value.is_empty()
            && value.len() <= 35
            && value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-'))
    {
        Ok(())
    } else {
        Err(VocabularyError::InvalidLanguage(value.to_owned()))
    }
}

fn validate_text(value: &str) -> Result<(), VocabularyError> {
    if value.trim().is_empty() || value.len() > MAX_TEXT_BYTES || value.contains('\0') {
        Err(VocabularyError::InvalidText)
    } else {
        Ok(())
    }
}

fn validate_short_identifier(value: &str) -> Result<(), VocabularyError> {
    if value.is_empty() || value.len() > 256 || value.contains(char::is_whitespace) {
        Err(VocabularyError::InvalidCandidate(value.to_owned()))
    } else {
        Ok(())
    }
}

fn validate_extensions(extensions: &BTreeMap<String, Value>) -> Result<(), VocabularyError> {
    for (key, value) in extensions {
        validate_uri(key)?;
        if serde_json::to_vec(value)
            .map_err(|error| VocabularyError::Malformed(error.to_string()))?
            .len()
            > MAX_EXTENSION_BYTES
        {
            return Err(VocabularyError::ExtensionTooLarge(key.clone()));
        }
    }
    Ok(())
}

/// Concept embedding retained independently from source chunk vectors.
pub(crate) struct EmbeddedConcept {
    concept_uri: String,
    vector: Vec<f32>,
}

impl EmbeddedConcept {
    pub(crate) fn new(
        concept_uri: impl Into<String>,
        vector: Vec<f32>,
    ) -> Result<Self, VocabularyError> {
        let concept_uri = concept_uri.into();
        validate_short_identifier(&concept_uri)?;
        validate_vector(&vector)?;
        Ok(Self {
            concept_uri,
            vector,
        })
    }
}

/// Existing source-chunk embedding used without mutation.
pub(crate) struct EmbeddedChunk {
    chunk_id: String,
    document_id: String,
    vector: Vec<f32>,
}

impl EmbeddedChunk {
    pub(crate) fn new(
        chunk_id: impl Into<String>,
        document_id: impl Into<String>,
        vector: Vec<f32>,
    ) -> Result<Self, VocabularyError> {
        let chunk_id = chunk_id.into();
        let document_id = document_id.into();
        validate_short_identifier(&chunk_id)?;
        validate_short_identifier(&document_id)?;
        validate_vector(&vector)?;
        Ok(Self {
            chunk_id,
            document_id,
            vector,
        })
    }
}

/// Explicit, bounded matching thresholds.
pub(crate) struct LabellingPolicy {
    pub(crate) active_threshold: f32,
    pub(crate) suggestion_threshold: f32,
    pub(crate) maximum_candidates_per_chunk: usize,
}

/// Replaceable active annotation or review suggestion.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConceptMatch {
    pub(crate) chunk_id: String,
    pub(crate) document_id: String,
    pub(crate) concept_uri: String,
    pub(crate) confidence: f32,
}

pub(crate) struct LabellingResult {
    pub(crate) active: Vec<ConceptMatch>,
    pub(crate) suggestions: Vec<ConceptMatch>,
}

/// Matches existing chunk vectors without changing or recomputing them.
pub(crate) fn label_chunks(
    chunks: &[EmbeddedChunk],
    concepts: &[EmbeddedConcept],
    policy: LabellingPolicy,
) -> Result<LabellingResult, VocabularyError> {
    if !(0.0..=1.0).contains(&policy.active_threshold)
        || !(0.0..policy.active_threshold).contains(&policy.suggestion_threshold)
        || policy.maximum_candidates_per_chunk == 0
        || policy.maximum_candidates_per_chunk > 32
    {
        return Err(VocabularyError::InvalidLabellingPolicy);
    }
    let dimensions = concepts
        .first()
        .map(|concept| concept.vector.len())
        .ok_or(VocabularyError::NoConcepts)?;
    if chunks.iter().any(|chunk| chunk.vector.len() != dimensions)
        || concepts
            .iter()
            .any(|concept| concept.vector.len() != dimensions)
    {
        return Err(VocabularyError::VectorDimensionMismatch);
    }
    let mut active = Vec::new();
    let mut suggestions = Vec::new();
    let mut ordered_chunks = chunks.iter().collect::<Vec<_>>();
    ordered_chunks.sort_by(|left, right| left.chunk_id.cmp(&right.chunk_id));
    for chunk in ordered_chunks {
        let mut candidates = concepts
            .iter()
            .map(|concept| (concept, cosine_similarity(&chunk.vector, &concept.vector)))
            .filter(|(_, score)| *score >= policy.suggestion_threshold)
            .collect::<Vec<_>>();
        candidates.sort_by(|(left_concept, left_score), (right_concept, right_score)| {
            right_score
                .total_cmp(left_score)
                .then_with(|| left_concept.concept_uri.cmp(&right_concept.concept_uri))
        });
        for (concept, confidence) in candidates
            .into_iter()
            .take(policy.maximum_candidates_per_chunk)
        {
            let matched = ConceptMatch {
                chunk_id: chunk.chunk_id.clone(),
                document_id: chunk.document_id.clone(),
                concept_uri: concept.concept_uri.clone(),
                confidence,
            };
            if confidence >= policy.active_threshold {
                active.push(matched);
            } else {
                suggestions.push(matched);
            }
        }
    }
    Ok(LabellingResult {
        active,
        suggestions,
    })
}

fn validate_vector(vector: &[f32]) -> Result<(), VocabularyError> {
    if vector.is_empty()
        || vector.len() > 65_536
        || vector.iter().any(|value| !value.is_finite())
        || vector.iter().all(|value| *value == 0.0)
    {
        Err(VocabularyError::InvalidVector)
    } else {
        Ok(())
    }
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    let dot = left
        .iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum::<f32>();
    let left_norm = left.iter().map(|value| value * value).sum::<f32>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f32>().sqrt();
    dot / (left_norm * right_norm)
}

#[derive(Debug, Error, PartialEq)]
pub(crate) enum VocabularyError {
    #[error("vocabulary JSON is malformed: {0}")]
    Malformed(String),
    #[error("unsupported SKOS interchange format `{0}`")]
    UnsupportedFormat(String),
    #[error("vocabulary document exceeds the 16 MiB limit")]
    DocumentTooLarge,
    #[error("vocabulary id `{0}` is invalid")]
    InvalidVocabularyId(String),
    #[error("vocabulary must contain between 1 and {MAX_CONCEPTS} concepts")]
    InvalidConceptCount,
    #[error("concept URI `{0}` is invalid")]
    InvalidConceptUri(String),
    #[error("concept `{0}` occurs more than once")]
    DuplicateConcept(String),
    #[error("concept `{0}` has no preferred label")]
    MissingPreferredLabel(String),
    #[error("language tag `{0}` is invalid")]
    InvalidLanguage(String),
    #[error("label, definition, or scope note is invalid")]
    InvalidText,
    #[error("relationship from `{source_uri}` points to missing or self concept `{target}`")]
    UnknownRelationship { source_uri: String, target: String },
    #[error("broader/narrower relationship `{broader}` -> `{narrower}` is not reciprocal")]
    AsymmetricHierarchy { broader: String, narrower: String },
    #[error("broader hierarchy contains a cycle at `{0}`")]
    HierarchyCycle(String),
    #[error("extension `{0}` exceeds the 256 KiB per-field limit")]
    ExtensionTooLarge(String),
    #[error("candidate `{0}` is invalid")]
    InvalidCandidate(String),
    #[error("candidate `{0}` already exists")]
    DuplicateCandidate(String),
    #[error("candidate `{0}` does not exist")]
    UnknownCandidate(String),
    #[error("candidate `{0}` was already reviewed")]
    CandidateAlreadyReviewed(String),
    #[error("candidate limit must be between 1 and 1,000")]
    InvalidCandidateLimit,
    #[error("concept or chunk vector is invalid")]
    InvalidVector,
    #[error("no concepts are available for labelling")]
    NoConcepts,
    #[error("concept and chunk vector dimensions differ")]
    VectorDimensionMismatch,
    #[error("labelling thresholds or candidate limit are invalid")]
    InvalidLabellingPolicy,
    #[error("vocabulary `{0}` is already imported")]
    DuplicateVocabulary(String),
    #[error("vocabulary `{0}` does not exist")]
    UnknownVocabulary(String),
    #[error("unsupported vocabulary store schema version `{0}`")]
    UnsupportedStoreVersion(u32),
    #[error("vocabulary storage failed: {0}")]
    Storage(String),
    #[error("a workspace or enrolled root attachment is required")]
    MissingAttachmentScope,
    #[error("concept `{0}` does not exist")]
    UnknownConcept(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const VALID_VOCABULARY: &str = r#"{
      "format": "procyon-skos-1",
      "id": "research-topics",
      "name": "Research topics",
      "concepts": [
        {
          "uri": "https://example.test/concepts/ai",
          "prefLabels": {"en": "Artificial intelligence", "nl": "Kunstmatige intelligentie"},
          "altLabels": {"en": ["AI"]},
          "definitions": {"en": "Systems performing tasks associated with intelligence."},
          "scopeNotes": {"en": "Use for documents substantially about AI."},
          "broader": [],
          "narrower": ["https://example.test/concepts/ml"],
          "related": [],
          "extensions": {"https://example.test/source": "curated"}
        },
        {
          "uri": "https://example.test/concepts/ml",
          "prefLabels": {"en": "Machine learning"},
          "altLabels": {},
          "definitions": {},
          "scopeNotes": {},
          "broader": ["https://example.test/concepts/ai"],
          "narrower": [],
          "related": [],
          "extensions": {}
        }
      ],
      "extensions": {"https://example.test/license": "CC0"}
    }"#;

    #[test]
    fn skos_subset_round_trips_multilingual_labels_and_safe_extensions() {
        let vocabulary = import_skos_json(VALID_VOCABULARY).expect("valid vocabulary");
        assert_eq!(vocabulary.id.as_str(), "research-topics");
        assert_eq!(
            vocabulary.concepts["https://example.test/concepts/ai"].pref_labels["nl"],
            "Kunstmatige intelligentie"
        );

        let exported = export_skos_json(&vocabulary).expect("export");
        let round_trip = import_skos_json(&exported).expect("round trip");
        assert_eq!(round_trip, vocabulary);
        assert_eq!(
            round_trip.extensions["https://example.test/license"],
            serde_json::json!("CC0")
        );
    }

    #[test]
    fn invalid_identities_missing_links_and_hierarchy_cycles_are_actionable() {
        let duplicate = VALID_VOCABULARY.replace(
            "\"https://example.test/concepts/ml\",\n          \"prefLabels\"",
            "\"https://example.test/concepts/ai\",\n          \"prefLabels\"",
        );
        assert!(matches!(
            import_skos_json(&duplicate),
            Err(VocabularyError::DuplicateConcept(uri)) if uri.ends_with("/ai")
        ));

        let missing = VALID_VOCABULARY.replace(
            "\"https://example.test/concepts/ml\"]",
            "\"https://example.test/concepts/missing\"]",
        );
        assert!(matches!(
            import_skos_json(&missing),
            Err(VocabularyError::UnknownRelationship { .. })
        ));

        let cycle = VALID_VOCABULARY
            .replace(
                "\"broader\": [],",
                "\"broader\": [\"https://example.test/concepts/ml\"],",
            )
            .replace(
                "\"narrower\": [],\n          \"related\"",
                "\"narrower\": [\"https://example.test/concepts/ai\"],\n          \"related\"",
            );
        assert!(matches!(
            import_skos_json(&cycle),
            Err(VocabularyError::HierarchyCycle(_))
        ));
    }

    #[test]
    fn candidates_remain_review_only_until_acceptance_and_decisions_persist() {
        let mut vocabulary = import_skos_json(VALID_VOCABULARY).expect("valid vocabulary");
        let candidate = ConceptCandidate::new(
            "candidate-neural",
            "Neural networks",
            vec!["Deep learning".into()],
            vec!["chunk-2".into(), "chunk-1".into()],
            0.71,
            14,
            CandidateProvenance::Statistical,
        )
        .expect("candidate");
        vocabulary.queue_candidate(candidate).expect("queue");
        assert!(
            !vocabulary
                .concepts
                .contains_key("urn:procyon:candidate-neural")
        );

        vocabulary
            .review_candidate(
                "candidate-neural",
                ReviewDecision::AcceptEdited {
                    concept_uri: "https://example.test/concepts/neural-networks".into(),
                    pref_label: "Neural network".into(),
                },
            )
            .expect("accept");
        assert!(
            vocabulary
                .concepts
                .contains_key("https://example.test/concepts/neural-networks")
        );
        assert_eq!(
            vocabulary.review_queue["candidate-neural"].status,
            CandidateStatus::Accepted
        );
    }

    #[test]
    fn deterministic_labelling_separates_active_matches_from_review_suggestions() {
        let concepts = vec![
            EmbeddedConcept::new("concept-b", vec![1.0, 0.0]).expect("concept"),
            EmbeddedConcept::new("concept-a", vec![0.0, 1.0]).expect("concept"),
        ];
        let chunks = vec![
            EmbeddedChunk::new("chunk-2", "doc-b", vec![0.8, 0.6]).expect("chunk"),
            EmbeddedChunk::new("chunk-1", "doc-a", vec![0.01, 0.99]).expect("chunk"),
        ];
        let result = label_chunks(
            &chunks,
            &concepts,
            LabellingPolicy {
                active_threshold: 0.95,
                suggestion_threshold: 0.75,
                maximum_candidates_per_chunk: 2,
            },
        )
        .expect("labels");

        assert_eq!(result.active.len(), 1);
        assert_eq!(result.active[0].chunk_id, "chunk-1");
        assert_eq!(result.active[0].concept_uri, "concept-a");
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].chunk_id, "chunk-2");
        assert_eq!(result.suggestions[0].concept_uri, "concept-b");
    }

    #[test]
    fn extracted_candidates_are_deterministic_review_only_aggregates() {
        let labels = vec![
            ExtractedConceptLabel {
                chunk_id: "chunk-b".into(),
                label: "Machine Learning".into(),
                synonyms: vec!["ML".into()],
            },
            ExtractedConceptLabel {
                chunk_id: "chunk-a".into(),
                label: "machine learning".into(),
                synonyms: vec!["Statistical learning".into()],
            },
            ExtractedConceptLabel {
                chunk_id: "chunk-c".into(),
                label: "Artificial intelligence".into(),
                synonyms: Vec::new(),
            },
        ];
        let mut reversed = labels
            .iter()
            .map(|label| ExtractedConceptLabel {
                chunk_id: label.chunk_id.clone(),
                label: label.label.clone(),
                synonyms: label.synonyms.clone(),
            })
            .collect::<Vec<_>>();
        reversed.reverse();

        let extracted = extract_concept_candidates(&labels, 10).unwrap();
        assert_eq!(
            extracted,
            extract_concept_candidates(&reversed, 10).unwrap()
        );
        assert_eq!(extracted[0].corpus_frequency, 2);
        assert_eq!(extracted[0].status, CandidateStatus::Pending);
        assert_eq!(
            extracted[0].supporting_chunk_ids,
            vec!["chunk-a", "chunk-b"]
        );
        assert_eq!(
            extracted[0].synonyms,
            vec!["ML".to_owned(), "Statistical learning".to_owned()]
        );
    }

    #[test]
    fn authoritative_store_round_trips_sources_reviews_and_deletion() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("semantic/vocabularies.json");
        let store = VocabularyStore::open(&path).unwrap();
        let imported = store.import(VALID_VOCABULARY).unwrap();
        store
            .update(imported.id.as_str(), |vocabulary| {
                vocabulary.queue_candidate(
                    ConceptCandidate::new(
                        "candidate-1",
                        "Retrieval augmented generation",
                        vec!["RAG".into()],
                        vec!["chunk-1".into()],
                        0.72,
                        4,
                        CandidateProvenance::Statistical,
                    )
                    .unwrap(),
                )
            })
            .unwrap();
        store
            .update(imported.id.as_str(), |vocabulary| {
                vocabulary.review_candidate("candidate-1", ReviewDecision::Reject)
            })
            .unwrap();
        drop(store);

        let reopened = VocabularyStore::open(&path).unwrap();
        let restored = reopened.get(imported.id.as_str()).unwrap();
        assert_eq!(
            restored.review_queue["candidate-1"].status,
            CandidateStatus::Rejected
        );
        assert_eq!(reopened.list().unwrap().len(), 1);
        reopened.delete(imported.id.as_str()).unwrap();
        assert!(matches!(
            reopened.get(imported.id.as_str()),
            Err(VocabularyError::UnknownVocabulary(_))
        ));
    }
}
