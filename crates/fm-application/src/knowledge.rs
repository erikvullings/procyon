//! Canonical Structured Knowledge requests and deterministic logical planning.
//!
//! This module contains no worker, authorization, filesystem, or LLM behavior.
//! Hosts resolve authorized scopes before planning and execute the resulting
//! searches through the semantic capability.

use std::collections::HashMap;

use fm_semantic_worker::knowledge_retrieval::{
    KnowledgeCapabilities as WorkerKnowledgeCapabilities, KnowledgeRetrievalReason,
};
use serde::{Deserialize, Serialize};

const MAX_SUBJECTS: usize = 8;
const MAX_RELATED_TERMS: usize = 16;
const MAX_SCOPES: usize = 16;
const MAX_PLANNED_SEARCHES: usize = 8;
pub(crate) const MAX_TEXT_BYTES: usize = 8 * 1024;
pub(crate) const MAX_IDENTIFIER_BYTES: usize = 512;
pub(crate) const MAX_CONSTRAINTS: usize = 16;
const MAX_CANDIDATES: usize = 512;
const MAX_RESULTS: usize = 200;
const MAX_RESULTS_PER_FILE: usize = 32;
const MAX_CONTEXT_TOKENS: usize = 32_768;
const MAX_ADJACENT_RADIUS: u32 = 4;

/// One user-supplied subject. The wrapper permits future subject metadata
/// without changing the request's multi-subject shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeSubject {
    /// Exact subject text entered by the user.
    pub text: String,
}

/// Authorized selector inside a tenant library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum KnowledgeScopeSelector {
    /// Search the complete authorized library.
    #[serde(alias = "library")]
    WholeLibrary,
    /// Search one authorized indexed root.
    Root {
        /// Opaque root identity.
        root_id: String,
    },
    /// Search one authorized workspace.
    Workspace {
        /// Opaque workspace identity.
        workspace_id: String,
    },
}

/// One application-authorized search scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeScope {
    /// Tenant owning the indexed library.
    pub tenant_id: String,
    /// Authorized semantic library.
    pub library_id: String,
    /// Authorized subset inside the library.
    pub selector: KnowledgeScopeSelector,
}

/// User-selected information shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeNeed {
    /// Broad orientation material.
    Overview,
    /// Definitions and terminology.
    Definition,
    /// Procedures and ordered guidance.
    #[serde(alias = "howTo", alias = "steps")]
    Procedure,
    /// Concrete examples.
    #[serde(alias = "sample")]
    Examples,
    /// Supporting evidence.
    #[serde(alias = "support")]
    Evidence,
    /// Competing arguments or tradeoffs.
    #[serde(alias = "prosCons")]
    Arguments,
    /// Comparative material.
    #[serde(alias = "compare")]
    Comparison,
    /// Limitations, risks, and caveats.
    #[serde(alias = "risks")]
    Limitations,
    /// References and source-oriented material.
    #[serde(alias = "sources")]
    References,
}

impl KnowledgeNeed {
    /// All initial information needs in canonical display order.
    pub const ALL: [Self; 9] = [
        Self::Overview,
        Self::Definition,
        Self::Procedure,
        Self::Examples,
        Self::Evidence,
        Self::Arguments,
        Self::Comparison,
        Self::Limitations,
        Self::References,
    ];

    const fn query_term(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Definition => "definition",
            Self::Procedure => "procedure",
            Self::Examples => "examples",
            Self::Evidence => "evidence",
            Self::Arguments => "arguments",
            Self::Comparison => "comparison",
            Self::Limitations => "limitations",
            Self::References => "references",
        }
    }

    const fn retrieval_reason(self) -> KnowledgeRetrievalReason {
        match self {
            Self::Overview => KnowledgeRetrievalReason::Overview,
            Self::Definition => KnowledgeRetrievalReason::Definition,
            Self::Procedure => KnowledgeRetrievalReason::Procedure,
            Self::Examples => KnowledgeRetrievalReason::Examples,
            Self::Evidence => KnowledgeRetrievalReason::Evidence,
            Self::Arguments => KnowledgeRetrievalReason::Arguments,
            Self::Comparison => KnowledgeRetrievalReason::Comparison,
            Self::Limitations => KnowledgeRetrievalReason::Limitations,
            Self::References => KnowledgeRetrievalReason::References,
        }
    }
}

/// Requested physical retrieval mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RetrievalMode {
    /// Fuse native full-text and dense retrieval when both are available.
    #[default]
    Hybrid,
    /// Use native full-text retrieval only.
    #[serde(alias = "full-text", alias = "full_text", alias = "fts")]
    FullText,
    /// Use dense semantic retrieval only.
    #[serde(alias = "vector")]
    Semantic,
}

/// Bounded retrieval and evidence options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeSearchOptions {
    /// Maximum logical searches emitted by the planner.
    pub maximum_searches: usize,
    /// Candidate count requested per physical route and logical search.
    pub candidate_limit: usize,
    /// Maximum primary results in the evidence set.
    pub result_limit: usize,
    /// Maximum primary results selected from one document.
    pub maximum_results_per_file: usize,
    /// Complete-chunk token budget for primary and adjacent evidence.
    pub context_token_budget: usize,
    /// Number of neighboring structural chunks included on each side.
    pub adjacent_chunk_radius: u32,
    /// Whether adjacent chunks must share the primary section.
    pub section_bounded_context: bool,
    /// Whether to retain a privacy-safe planning and retrieval trace.
    pub include_trace: bool,
}

impl Default for KnowledgeSearchOptions {
    fn default() -> Self {
        Self {
            maximum_searches: MAX_PLANNED_SEARCHES,
            candidate_limit: 64,
            result_limit: 20,
            maximum_results_per_file: 3,
            context_token_budget: 8_192,
            adjacent_chunk_radius: 1,
            section_bounded_context: true,
            include_trace: false,
        }
    }
}

impl KnowledgeSearchOptions {
    const fn valid(self, subject_count: usize) -> bool {
        self.maximum_searches >= subject_count
            && self.maximum_searches <= MAX_PLANNED_SEARCHES
            && self.candidate_limit >= 1
            && self.candidate_limit <= MAX_CANDIDATES
            && self.result_limit >= 1
            && self.result_limit <= MAX_RESULTS
            && self.maximum_results_per_file >= 1
            && self.maximum_results_per_file <= MAX_RESULTS_PER_FILE
            && self.context_token_budget >= 1
            && self.context_token_budget <= MAX_CONTEXT_TOKENS
            && self.adjacent_chunk_radius <= MAX_ADJACENT_RADIUS
    }
}

/// Canonical retrieval-only request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeSearchRequest {
    /// Ordered primary subjects.
    #[serde(alias = "subject")]
    pub subjects: Vec<KnowledgeSubject>,
    /// Explicit information needs.
    #[serde(default, alias = "need")]
    pub needs: Vec<KnowledgeNeed>,
    /// Explicit lower-priority search expansions.
    #[serde(default, alias = "related")]
    pub related_terms: Vec<String>,
    /// Application-authorized search scopes.
    #[serde(alias = "scope")]
    pub scopes: Vec<KnowledgeScope>,
    /// Requested physical retrieval mode.
    #[serde(default, alias = "retrievalMode")]
    pub mode: RetrievalMode,
    /// Bounded planning and evidence options.
    #[serde(default)]
    pub options: KnowledgeSearchOptions,
}

impl KnowledgeSearchRequest {
    /// Validates all request and resource bounds.
    ///
    /// # Errors
    ///
    /// Returns the first invalid field or bound.
    pub fn validate(&self) -> Result<(), KnowledgeRequestError> {
        if self.subjects.is_empty() || self.subjects.len() > MAX_SUBJECTS {
            return Err(KnowledgeRequestError::SubjectCount {
                actual: self.subjects.len(),
                maximum: MAX_SUBJECTS,
            });
        }
        for (index, subject) in self.subjects.iter().enumerate() {
            if !valid_text(&subject.text) {
                return Err(KnowledgeRequestError::InvalidSubject { index });
            }
        }
        if self.needs.len() > KnowledgeNeed::ALL.len() {
            return Err(KnowledgeRequestError::NeedCount {
                actual: self.needs.len(),
                maximum: KnowledgeNeed::ALL.len(),
            });
        }
        if self.related_terms.len() > MAX_RELATED_TERMS {
            return Err(KnowledgeRequestError::RelatedTermCount {
                actual: self.related_terms.len(),
                maximum: MAX_RELATED_TERMS,
            });
        }
        for (index, term) in self.related_terms.iter().enumerate() {
            if !valid_text(term) {
                return Err(KnowledgeRequestError::InvalidRelatedTerm { index });
            }
        }
        if self.scopes.is_empty() || self.scopes.len() > MAX_SCOPES {
            return Err(KnowledgeRequestError::ScopeCount {
                actual: self.scopes.len(),
                maximum: MAX_SCOPES,
            });
        }
        for (index, scope) in self.scopes.iter().enumerate() {
            if !valid_identifier(&scope.tenant_id)
                || !valid_identifier(&scope.library_id)
                || match &scope.selector {
                    KnowledgeScopeSelector::WholeLibrary => false,
                    KnowledgeScopeSelector::Root { root_id } => !valid_identifier(root_id),
                    KnowledgeScopeSelector::Workspace { workspace_id } => {
                        !valid_identifier(workspace_id)
                    }
                }
            {
                return Err(KnowledgeRequestError::InvalidScope { index });
            }
        }
        if !self.options.valid(self.subjects.len()) {
            return Err(KnowledgeRequestError::InvalidOptions);
        }
        Ok(())
    }
}

/// Optional answer intent. Only `action` may select default typed needs; no
/// string from this request is copied into retrieval text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeAnswerRequest {
    /// Fingerprint of the already-inspected evidence to consume.
    pub evidence_fingerprint: String,
    /// Optional typed answer goal.
    pub action: Option<KnowledgeAction>,
    /// Optional application context, never copied into retrieval.
    pub context: Option<String>,
    /// Optional answer constraints, never copied into retrieval.
    #[serde(default)]
    pub constraints: Vec<String>,
    /// Requested answer depth.
    pub depth: Option<KnowledgeAnswerDepth>,
    /// Requested answer presentation.
    pub output: Option<KnowledgeOutputFormat>,
}

impl KnowledgeAnswerRequest {
    /// Validates answer-only bounds.
    ///
    /// # Errors
    ///
    /// Rejects invalid evidence fingerprints and unbounded answer fields.
    pub fn validate(&self) -> Result<(), KnowledgeRequestError> {
        if !valid_identifier(&self.evidence_fingerprint) {
            return Err(KnowledgeRequestError::InvalidEvidenceFingerprint);
        }
        if self
            .context
            .as_ref()
            .is_some_and(|value| !valid_text(value))
            || self.constraints.len() > MAX_CONSTRAINTS
            || self.constraints.iter().any(|value| !valid_text(value))
        {
            return Err(KnowledgeRequestError::InvalidAnswerOptions);
        }
        Ok(())
    }
}

/// Typed answer goal used only for conservative default-need selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeAction {
    /// Explain the selected subject.
    Explain,
    /// Build general understanding with examples.
    Learn,
    /// Apply the subject as a procedure.
    #[serde(alias = "implement", alias = "howTo")]
    Apply,
    /// Evaluate evidence, tradeoffs, and limitations.
    Evaluate,
    /// Compare the selected subjects.
    Compare,
    /// Produce a source-oriented answer.
    #[serde(alias = "source")]
    Cite,
}

impl KnowledgeAction {
    const fn default_needs(self) -> &'static [KnowledgeNeed] {
        match self {
            Self::Explain => &[KnowledgeNeed::Overview, KnowledgeNeed::Definition],
            Self::Learn => &[KnowledgeNeed::Overview, KnowledgeNeed::Examples],
            Self::Apply => &[KnowledgeNeed::Procedure, KnowledgeNeed::Examples],
            Self::Evaluate => &[
                KnowledgeNeed::Evidence,
                KnowledgeNeed::Arguments,
                KnowledgeNeed::Limitations,
            ],
            Self::Compare => &[KnowledgeNeed::Comparison, KnowledgeNeed::Evidence],
            Self::Cite => &[KnowledgeNeed::References, KnowledgeNeed::Evidence],
        }
    }
}

/// Requested answer detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeAnswerDepth {
    /// Short answer.
    Brief,
    /// Normal answer depth.
    Standard,
    /// Expanded answer.
    Detailed,
}

/// Requested answer presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeOutputFormat {
    /// Prose response.
    Narrative,
    /// Bulleted response.
    Bullets,
    /// Ordered steps.
    Steps,
    /// Tabular response.
    Table,
}

/// Independently reported user-facing capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeCapabilities {
    /// Native full-text retrieval is available.
    pub full_text: bool,
    /// Compatible query embeddings and vector retrieval are available.
    pub semantic: bool,
    /// Optional evidence-grounded answer generation is available.
    pub answer_generation: bool,
}

impl KnowledgeCapabilities {
    /// Nothing is offered: used by builds that must fail closed.
    pub const UNAVAILABLE: Self = Self {
        full_text: false,
        semantic: false,
        answer_generation: false,
    };

    /// Reports whether at least one route can satisfy the mode.
    #[must_use]
    pub const fn supports(self, mode: RetrievalMode) -> bool {
        match mode {
            RetrievalMode::Hybrid => self.full_text || self.semantic,
            RetrievalMode::FullText => self.full_text,
            RetrievalMode::Semantic => self.semantic,
        }
    }

    /// Combines worker retrieval capability with host-owned answer capability.
    #[must_use]
    pub const fn from_worker(worker: WorkerKnowledgeCapabilities, answer_generation: bool) -> Self {
        Self {
            full_text: worker.full_text,
            semantic: worker.query_embeddings,
            answer_generation,
        }
    }
}

/// Stable planner priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeSearchPriority {
    /// Raw subject.
    Primary,
    /// Need-derived expansion.
    Secondary,
    /// Explicit related term.
    Related,
}

/// Auditable reason for emitting a logical search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum KnowledgeSearchReason {
    /// Raw user subject.
    Subject {
        /// Position in the canonical request.
        subject_index: usize,
    },
    /// Explicit need expansion.
    Need {
        /// Position of the expanded subject.
        subject_index: usize,
        /// Information need used by the template.
        need: KnowledgeNeed,
    },
    /// Need defaulted from a typed answer action.
    ActionDefault {
        /// Answer action selecting the defaults.
        action: KnowledgeAction,
        /// Position of the expanded subject.
        subject_index: usize,
        /// Selected default need.
        need: KnowledgeNeed,
    },
    /// Explicit related term.
    RelatedTerm {
        /// Position in the canonical request.
        related_term_index: usize,
    },
}

/// One deterministic source search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedKnowledgeSearch {
    /// Exact query text sent to retrieval.
    pub text: String,
    /// Stable planning priority.
    pub priority: KnowledgeSearchPriority,
    /// All reasons retained after equivalent-query deduplication.
    pub reasons: Vec<KnowledgeSearchReason>,
}

impl PlannedKnowledgeSearch {
    /// Maps the highest-priority retained reason to the worker trace model.
    #[must_use]
    pub fn primary_retrieval_reason(&self) -> KnowledgeRetrievalReason {
        match self.reasons.first() {
            Some(KnowledgeSearchReason::Subject { .. }) | None => KnowledgeRetrievalReason::Subject,
            Some(KnowledgeSearchReason::Need { need, .. })
            | Some(KnowledgeSearchReason::ActionDefault { need, .. }) => need.retrieval_reason(),
            Some(KnowledgeSearchReason::RelatedTerm { .. }) => KnowledgeRetrievalReason::Related,
        }
    }
}

/// Validated deterministic logical plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSearchPlan {
    /// Planner identity for evaluation and cache fingerprints.
    pub version: String,
    /// Original ordered subjects.
    pub subjects: Vec<KnowledgeSubject>,
    /// Authorized scopes copied without interpretation.
    pub scopes: Vec<KnowledgeScope>,
    /// Requested retrieval mode.
    pub mode: RetrievalMode,
    /// Validated resource and evidence options.
    pub options: KnowledgeSearchOptions,
    /// Stable bounded source searches.
    pub searches: Vec<PlannedKnowledgeSearch>,
    /// Expansions omitted because of query-size or search-count bounds.
    pub omitted_searches: usize,
}

/// Invalid canonical request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KnowledgeRequestError {
    /// Subject count is outside the supported range.
    #[error("knowledge search requires 1 to {maximum} subjects, got {actual}")]
    SubjectCount {
        /// Supplied subject count.
        actual: usize,
        /// Maximum supported count.
        maximum: usize,
    },
    /// One subject is empty or exceeds the text bound.
    #[error("knowledge subject {index} is empty or too large")]
    InvalidSubject {
        /// Invalid subject position.
        index: usize,
    },
    /// Too many explicit needs were supplied.
    #[error("knowledge search accepts at most {maximum} needs, got {actual}")]
    NeedCount {
        /// Supplied need count.
        actual: usize,
        /// Maximum supported count.
        maximum: usize,
    },
    /// Too many explicit related terms were supplied.
    #[error("knowledge search accepts at most {maximum} related terms, got {actual}")]
    RelatedTermCount {
        /// Supplied related-term count.
        actual: usize,
        /// Maximum supported count.
        maximum: usize,
    },
    /// One related term is empty or exceeds the text bound.
    #[error("related term {index} is empty or too large")]
    InvalidRelatedTerm {
        /// Invalid related-term position.
        index: usize,
    },
    /// Scope count is outside the supported range.
    #[error("knowledge search requires 1 to {maximum} scopes, got {actual}")]
    ScopeCount {
        /// Supplied scope count.
        actual: usize,
        /// Maximum supported count.
        maximum: usize,
    },
    /// One scope contains an invalid opaque identifier.
    #[error("knowledge scope {index} is invalid")]
    InvalidScope {
        /// Invalid scope position.
        index: usize,
    },
    /// Retrieval or evidence bounds are invalid.
    #[error("knowledge search options are invalid")]
    InvalidOptions,
    /// Evidence fingerprint is empty or exceeds its bound.
    #[error("evidence fingerprint is invalid")]
    InvalidEvidenceFingerprint,
    /// Answer-only context or constraints are invalid.
    #[error("knowledge answer options are invalid")]
    InvalidAnswerOptions,
}

/// Pure deterministic Structured Knowledge planner.
pub struct KnowledgePlanner;

impl KnowledgePlanner {
    /// Planner identity retained in plans and evaluation fingerprints.
    pub const VERSION: &'static str = "structured-knowledge-planner/1";

    /// Builds a stable bounded plan without contacting a worker or an LLM.
    ///
    /// Answer text is never read. A typed action can select conservative
    /// default needs only when the search request has no explicit needs.
    ///
    /// # Errors
    ///
    /// Returns canonical search or answer validation failures.
    pub fn plan(
        request: &KnowledgeSearchRequest,
        action: Option<KnowledgeAction>,
    ) -> Result<KnowledgeSearchPlan, KnowledgeRequestError> {
        request.validate()?;

        let mut searches = Vec::new();
        let mut search_indexes = HashMap::<String, usize>::new();
        for (subject_index, subject) in request.subjects.iter().enumerate() {
            add_search(
                &mut searches,
                &mut search_indexes,
                subject.text.trim(),
                KnowledgeSearchPriority::Primary,
                KnowledgeSearchReason::Subject { subject_index },
            );
        }

        let needs = if request.needs.is_empty() {
            action.map_or(&[][..], KnowledgeAction::default_needs)
        } else {
            request.needs.as_slice()
        };
        let mut omitted_searches = 0usize;
        for need in needs.iter().copied() {
            for (subject_index, subject) in request.subjects.iter().enumerate() {
                let text = format!("{} {}", subject.text.trim(), need.query_term());
                if text.len() > MAX_TEXT_BYTES {
                    omitted_searches += 1;
                    continue;
                }
                let reason = action.filter(|_| request.needs.is_empty()).map_or(
                    KnowledgeSearchReason::Need {
                        subject_index,
                        need,
                    },
                    |action| KnowledgeSearchReason::ActionDefault {
                        action,
                        subject_index,
                        need,
                    },
                );
                add_search(
                    &mut searches,
                    &mut search_indexes,
                    &text,
                    KnowledgeSearchPriority::Secondary,
                    reason,
                );
            }
        }
        for (related_term_index, related) in request.related_terms.iter().enumerate() {
            add_search(
                &mut searches,
                &mut search_indexes,
                related.trim(),
                KnowledgeSearchPriority::Related,
                KnowledgeSearchReason::RelatedTerm { related_term_index },
            );
        }
        omitted_searches += searches
            .len()
            .saturating_sub(request.options.maximum_searches);
        searches.truncate(request.options.maximum_searches);

        Ok(KnowledgeSearchPlan {
            version: Self::VERSION.into(),
            subjects: request.subjects.clone(),
            scopes: request.scopes.clone(),
            mode: request.mode,
            options: request.options,
            searches,
            omitted_searches,
        })
    }
}

fn add_search(
    searches: &mut Vec<PlannedKnowledgeSearch>,
    search_indexes: &mut HashMap<String, usize>,
    text: &str,
    priority: KnowledgeSearchPriority,
    reason: KnowledgeSearchReason,
) {
    let canonical = canonical_search(text);
    if let Some(index) = search_indexes.get(&canonical).copied() {
        if !searches[index].reasons.contains(&reason) {
            searches[index].reasons.push(reason);
        }
        return;
    }
    search_indexes.insert(canonical, searches.len());
    searches.push(PlannedKnowledgeSearch {
        text: text.to_owned(),
        priority,
        reasons: vec![reason],
    });
}

fn canonical_search(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_TEXT_BYTES
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && value.len() <= MAX_IDENTIFIER_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(text: &str) -> KnowledgeSubject {
        KnowledgeSubject {
            text: text.to_owned(),
        }
    }

    fn scope() -> KnowledgeScope {
        KnowledgeScope {
            tenant_id: "tenant-a".into(),
            library_id: "library-a".into(),
            selector: KnowledgeScopeSelector::WholeLibrary,
        }
    }

    fn request(subjects: Vec<KnowledgeSubject>) -> KnowledgeSearchRequest {
        KnowledgeSearchRequest {
            subjects,
            needs: Vec::new(),
            related_terms: Vec::new(),
            scopes: vec![scope()],
            mode: RetrievalMode::Hybrid,
            options: KnowledgeSearchOptions::default(),
        }
    }

    fn answer(action: Option<KnowledgeAction>, context: Option<&str>) -> KnowledgeAnswerRequest {
        KnowledgeAnswerRequest {
            evidence_fingerprint: "sha256:evidence".into(),
            action,
            context: context.map(str::to_owned),
            constraints: Vec::new(),
            depth: None,
            output: None,
        }
    }

    fn search_texts(plan: &KnowledgeSearchPlan) -> Vec<&str> {
        plan.searches
            .iter()
            .map(|search| search.text.as_str())
            .collect()
    }

    #[test]
    fn plan_order_is_primary_subjects_then_needs_then_related_terms() {
        let mut request = request(vec![subject("Rust ownership"), subject("borrow checking")]);
        request.needs = vec![KnowledgeNeed::Definition, KnowledgeNeed::Examples];
        request.related_terms = vec!["lifetimes".into()];

        let plan = KnowledgePlanner::plan(&request, None).expect("plan");

        assert_eq!(
            search_texts(&plan),
            [
                "Rust ownership",
                "borrow checking",
                "Rust ownership definition",
                "borrow checking definition",
                "Rust ownership examples",
                "borrow checking examples",
                "lifetimes",
            ]
        );
        assert_eq!(plan.searches[0].priority, KnowledgeSearchPriority::Primary);
        assert_eq!(
            plan.searches[2].priority,
            KnowledgeSearchPriority::Secondary
        );
        assert_eq!(plan.searches[6].priority, KnowledgeSearchPriority::Related);
        assert_eq!(plan.subjects, request.subjects);
        assert_eq!(plan.scopes, request.scopes);
    }

    #[test]
    fn equivalent_searches_keep_first_spelling_and_combine_reasons() {
        let mut request = request(vec![subject("Rust")]);
        request.needs = vec![KnowledgeNeed::Definition];
        request.related_terms = vec!["  rust   DEFINITION ".into(), " rust ".into()];

        let plan = KnowledgePlanner::plan(&request, None).expect("plan");

        assert_eq!(search_texts(&plan), ["Rust", "Rust definition"]);
        assert_eq!(
            plan.searches[0].reasons,
            [
                KnowledgeSearchReason::Subject { subject_index: 0 },
                KnowledgeSearchReason::RelatedTerm {
                    related_term_index: 1,
                },
            ]
        );
        assert_eq!(
            plan.searches[1].reasons,
            [
                KnowledgeSearchReason::Need {
                    subject_index: 0,
                    need: KnowledgeNeed::Definition,
                },
                KnowledgeSearchReason::RelatedTerm {
                    related_term_index: 0,
                },
            ]
        );
    }

    #[test]
    fn explicit_needs_prevent_answer_fields_from_contaminating_retrieval() {
        let mut request = request(vec![subject("bipartite matching")]);
        request.needs = vec![KnowledgeNeed::Procedure, KnowledgeNeed::Examples];
        let answer = answer(
            Some(KnowledgeAction::Compare),
            Some("sorting algorithms and output tables"),
        );

        let plan = KnowledgePlanner::plan(&request, answer.action).expect("plan");

        assert_eq!(
            search_texts(&plan),
            [
                "bipartite matching",
                "bipartite matching procedure",
                "bipartite matching examples",
            ]
        );
        assert!(
            plan.searches
                .iter()
                .all(|search| !search.text.contains("sorting")
                    && !search.text.contains("compare")
                    && !search.text.contains("table"))
        );
    }

    #[test]
    fn typed_action_selects_default_needs_without_injecting_action_text() {
        let request = request(vec![subject("storage migration")]);
        let answer = answer(Some(KnowledgeAction::Evaluate), None);

        let plan = KnowledgePlanner::plan(&request, answer.action).expect("plan");

        assert_eq!(
            search_texts(&plan),
            [
                "storage migration",
                "storage migration evidence",
                "storage migration arguments",
                "storage migration limitations",
            ]
        );
        assert_eq!(
            plan.searches[1].reasons,
            [KnowledgeSearchReason::ActionDefault {
                action: KnowledgeAction::Evaluate,
                subject_index: 0,
                need: KnowledgeNeed::Evidence,
            }]
        );
    }

    #[test]
    fn boundary_aliases_deserialize_to_canonical_values() {
        let request: KnowledgeSearchRequest = serde_json::from_value(serde_json::json!({
            "subject": [{"text": "fault tolerance"}],
            "need": ["howTo", "prosCons", "sources"],
            "related": ["recovery"],
            "scope": [{
                "tenantId": "tenant-a",
                "libraryId": "library-a",
                "selector": {"kind": "library"}
            }],
            "retrievalMode": "fts",
            "options": {}
        }))
        .expect("aliases");

        assert_eq!(
            request.needs,
            [
                KnowledgeNeed::Procedure,
                KnowledgeNeed::Arguments,
                KnowledgeNeed::References,
            ]
        );
        assert_eq!(request.mode, RetrievalMode::FullText);
        assert_eq!(request.related_terms, ["recovery"]);
    }

    #[test]
    fn validation_rejects_empty_and_excessive_inputs() {
        let mut empty = request(Vec::new());
        assert_eq!(
            empty.validate(),
            Err(KnowledgeRequestError::SubjectCount {
                actual: 0,
                maximum: MAX_SUBJECTS,
            })
        );

        empty.subjects = vec![subject("valid")];
        empty.related_terms = (0..=MAX_RELATED_TERMS)
            .map(|index| format!("related-{index}"))
            .collect();
        assert_eq!(
            empty.validate(),
            Err(KnowledgeRequestError::RelatedTermCount {
                actual: MAX_RELATED_TERMS + 1,
                maximum: MAX_RELATED_TERMS,
            })
        );

        let invalid_options = KnowledgeSearchRequest {
            options: KnowledgeSearchOptions {
                result_limit: 0,
                ..KnowledgeSearchOptions::default()
            },
            ..request(vec![subject("valid")])
        };
        assert_eq!(
            invalid_options.validate(),
            Err(KnowledgeRequestError::InvalidOptions)
        );

        let too_many_subjects = request(
            (0..=MAX_SUBJECTS)
                .map(|index| subject(&format!("subject-{index}")))
                .collect(),
        );
        assert_eq!(
            too_many_subjects.validate(),
            Err(KnowledgeRequestError::SubjectCount {
                actual: MAX_SUBJECTS + 1,
                maximum: MAX_SUBJECTS,
            })
        );

        let invalid_scope = KnowledgeSearchRequest {
            scopes: vec![KnowledgeScope {
                tenant_id: "tenant-a".into(),
                library_id: "library-a".into(),
                selector: KnowledgeScopeSelector::Root {
                    root_id: " ".into(),
                },
            }],
            ..request(vec![subject("valid")])
        };
        assert_eq!(
            invalid_scope.validate(),
            Err(KnowledgeRequestError::InvalidScope { index: 0 })
        );

        let padded_scope = KnowledgeSearchRequest {
            scopes: vec![KnowledgeScope {
                tenant_id: "tenant-a".into(),
                library_id: "library-a".into(),
                selector: KnowledgeScopeSelector::Root {
                    root_id: " root-a".into(),
                },
            }],
            ..request(vec![subject("valid")])
        };
        assert_eq!(
            padded_scope.validate(),
            Err(KnowledgeRequestError::InvalidScope { index: 0 })
        );

        let oversized_whitespace = request(vec![subject(&format!(
            "{}valid",
            " ".repeat(MAX_TEXT_BYTES)
        ))]);
        assert_eq!(
            oversized_whitespace.validate(),
            Err(KnowledgeRequestError::InvalidSubject { index: 0 })
        );

        let invalid_answer = KnowledgeAnswerRequest {
            constraints: vec!["constraint".into(); MAX_CONSTRAINTS + 1],
            ..answer(None, None)
        };
        assert_eq!(
            invalid_answer.validate(),
            Err(KnowledgeRequestError::InvalidAnswerOptions)
        );
    }

    #[test]
    fn search_bound_preserves_every_raw_subject_before_expansions() {
        let mut request = request(vec![subject("alpha"), subject("beta")]);
        request.needs = vec![KnowledgeNeed::Overview];
        request.related_terms = vec!["gamma".into()];
        request.options.maximum_searches = 2;

        let plan = KnowledgePlanner::plan(&request, None).expect("bounded plan");

        assert_eq!(search_texts(&plan), ["alpha", "beta"]);
        assert_eq!(plan.omitted_searches, 3);
    }

    #[test]
    fn need_expansion_cannot_exceed_the_worker_query_bound() {
        let mut request = request(vec![subject(&"x".repeat(MAX_TEXT_BYTES))]);
        request.needs = vec![KnowledgeNeed::Limitations];

        let plan = KnowledgePlanner::plan(&request, None).expect("bounded plan");

        assert_eq!(plan.searches.len(), 1);
        assert_eq!(plan.searches[0].text.len(), MAX_TEXT_BYTES);
        assert_eq!(plan.omitted_searches, 1);
        assert_eq!(
            plan.searches[0].primary_retrieval_reason(),
            KnowledgeRetrievalReason::Subject
        );
    }

    #[test]
    fn capabilities_distinguish_full_text_vectors_and_answers() {
        let capabilities = KnowledgeCapabilities {
            full_text: true,
            semantic: false,
            answer_generation: false,
        };

        assert!(capabilities.supports(RetrievalMode::FullText));
        assert!(capabilities.supports(RetrievalMode::Hybrid));
        assert!(!capabilities.supports(RetrievalMode::Semantic));
        assert!(!capabilities.answer_generation);

        let mapped = KnowledgeCapabilities::from_worker(
            WorkerKnowledgeCapabilities {
                full_text: false,
                query_embeddings: true,
            },
            true,
        );
        assert_eq!(
            mapped,
            KnowledgeCapabilities {
                full_text: false,
                semantic: true,
                answer_generation: true,
            }
        );
    }

    #[test]
    fn reason_mapping_preserves_need_and_related_semantics() {
        let mut request = request(vec![subject("alpha")]);
        request.needs = vec![KnowledgeNeed::Procedure];
        request.related_terms = vec!["beta".into()];

        let plan = KnowledgePlanner::plan(&request, None).expect("plan");

        assert_eq!(
            plan.searches
                .iter()
                .map(PlannedKnowledgeSearch::primary_retrieval_reason)
                .collect::<Vec<_>>(),
            [
                KnowledgeRetrievalReason::Subject,
                KnowledgeRetrievalReason::Procedure,
                KnowledgeRetrievalReason::Related,
            ]
        );
    }
}
