//! Deterministic Structured Knowledge DSL parser, formatter, and composer draft.
//!
//! This module is pure and rule-based: it contains no LLM, network, worker, or
//! authorization behavior. It parses a small text DSL (and a handful of common
//! natural-language question forms) into a typed [`KnowledgeQueryDraft`] that a
//! visual composer can edit, and converts that draft losslessly to and from the
//! canonical [`KnowledgeSearchRequest`]/[`KnowledgeAnswerRequest`] models owned by
//! [`crate::knowledge`]. Scope authorization (tenant/library identity) and the
//! evidence fingerprint an answer consumes are supplied explicitly by the caller
//! at conversion time; this module never resolves or checks authorization.
//!
//! ## DSL grammar
//!
//! The DSL is a sequence of `field: value` assignments, either one per line
//! (multiline form) or all on one line (compact form). Compact assignments may
//! use semicolons and omit spaces (`about:rust;need:definition`). Colons inside
//! values such as `workspace:abc-123` remain part of the value.
//! Leading text before the first recognized field assignment is treated as an
//! implicit `about:` value.
//!
//! Values may be bare text or `"quoted text"`. Quoting is required to embed a
//! comma, a colon followed by whitespace, a leading/trailing space, or a `"`
//! literal (escaped inside quotes as `\"`, with `\\` for a literal backslash).
//! Multiple values for a repeatable field are either comma-separated within one
//! occurrence or spread across repeated occurrences of the same field name;
//! both forms may be combined.
//!
//! Canonical retrieval fields: `about`, `need`, `related`, `scope`. Answer-only
//! fields, never copied into retrieval: `do`, `to`, `constraint`, `format`,
//! `depth`.
//! Every field has documented aliases; see [`KnowledgeDslField::from_key`].

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::knowledge::{
    KnowledgeAction, KnowledgeAnswerDepth, KnowledgeAnswerRequest, KnowledgeNeed,
    KnowledgeOutputFormat, KnowledgeRequestError, KnowledgeScope, KnowledgeScopeSelector,
    KnowledgeSearchOptions, KnowledgeSearchRequest, KnowledgeSubject, MAX_CONSTRAINTS,
    MAX_IDENTIFIER_BYTES, MAX_TEXT_BYTES, RetrievalMode,
};

/// Maximum accepted `about` (subject) entries, mirroring `knowledge::MAX_SUBJECTS`.
const MAX_ABOUT: usize = 8;
/// Maximum accepted `related` entries, mirroring `knowledge::MAX_RELATED_TERMS`.
const MAX_RELATED: usize = 16;
/// Maximum accepted `scope` entries, mirroring `knowledge::MAX_SCOPES`.
const MAX_SCOPES: usize = 16;
/// Maximum accepted `constraint` entries, mirroring `knowledge::MAX_CONSTRAINTS`.
// Covers the escaped formatter output of every maximum-sized canonical text
// and identifier field with headroom for delimiters and diagnostics.
const MAX_INPUT_BYTES: usize = 1024 * 1024;
/// Maximum accepted explicit `need` entries: one of every known need.
const MAX_NEEDS: usize = KnowledgeNeed::ALL.len();
/// Suggestion threshold: only surface a spelling suggestion this close or closer.
const MAX_SUGGESTION_DISTANCE: usize = 2;

/// Half-open byte-offset span into the original DSL source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
}

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    /// The affected value or field was dropped.
    Error,
    /// The value was accepted but is worth surfacing to the user.
    Warning,
}

/// Machine-matchable diagnostic classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeDslDiagnosticCode {
    /// A `field:` key did not match any canonical field or documented alias.
    UnknownField,
    /// A value-only field (`do`/`to`/`format`) was assigned more than once.
    DuplicateField,
    /// A value was empty after trimming and quote removal.
    EmptyValue,
    /// A `need:` value did not match a known need name or alias.
    InvalidNeedValue,
    /// A `scope:` value did not match the `library` / `root:<id>` / `workspace:<id>` grammar.
    InvalidScopeValue,
    /// A `do:` value did not match a known action name or alias.
    InvalidActionValue,
    /// A `format:` value did not match a known output format name or alias.
    InvalidFormatValue,
    /// A repeatable field exceeded its bounded entry count; the extra value was dropped.
    TooManyValues,
    /// A text or identifier value exceeded its canonical byte bound.
    ValueTooLong,
    /// A quoted value has no closing quote.
    UnterminatedQuote,
    /// A `depth:` value did not match a known answer depth.
    InvalidDepthValue,
}

/// One actionable parser diagnostic with a source span and optional suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeDslDiagnostic {
    /// Diagnostic severity.
    pub severity: DiagnosticSeverity,
    /// Source span the diagnostic refers to (the field key or the value token).
    pub span: Span,
    /// Machine-matchable diagnostic code.
    pub code: KnowledgeDslDiagnosticCode,
    /// Human-readable explanation.
    pub message: String,
    /// Suggested correction, when a close match was found.
    pub suggestion: Option<String>,
}

/// How the parser arrived at a draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeParseConfidence {
    /// Every field came from an explicit DSL assignment.
    Explicit,
    /// A natural-language rule matched exactly one deterministic template.
    Deterministic,
    /// Multiple deterministic templates matched; the first was applied and the
    /// rest are recorded as ambiguities instead of being silently discarded.
    Ambiguous,
}

/// One alternative interpretation recorded instead of being silently dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeParseAmbiguity {
    /// Human-readable explanation of the alternative reading.
    pub description: String,
    /// Alternative information need, when the ambiguity concerns retrieval shape.
    pub alternative_need: Option<KnowledgeNeed>,
    /// Alternative answer action, when the ambiguity concerns answer intent.
    pub alternative_action: Option<KnowledgeAction>,
}

/// Canonical field recognized by the DSL, independent of its spelling alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeDslField {
    /// Retrieval subject(s).
    About,
    /// Explicit information need(s).
    Need,
    /// Explicit lower-priority related term(s).
    Related,
    /// Authorized search scope selector(s).
    Scope,
    /// Answer-only: typed answer goal.
    Do,
    /// Answer-only: free-text application context.
    To,
    /// Answer-only: free-text constraint(s).
    Constraint,
    /// Answer-only: requested output presentation.
    Format,
    /// Answer-only: requested response depth.
    Depth,
}

impl KnowledgeDslField {
    /// Canonical field spelling used by the formatter.
    #[must_use]
    pub const fn canonical_key(self) -> &'static str {
        match self {
            Self::About => "about",
            Self::Need => "need",
            Self::Related => "related",
            Self::Scope => "scope",
            Self::Do => "do",
            Self::To => "to",
            Self::Constraint => "constraint",
            Self::Format => "format",
            Self::Depth => "depth",
        }
    }

    /// All canonical field names in DSL-formatting order.
    const ALL: [Self; 9] = [
        Self::About,
        Self::Need,
        Self::Related,
        Self::Scope,
        Self::Do,
        Self::To,
        Self::Constraint,
        Self::Format,
        Self::Depth,
    ];

    /// Whether the field accepts more than one value.
    const fn is_multi_valued(self) -> bool {
        matches!(
            self,
            Self::About | Self::Need | Self::Related | Self::Scope | Self::Constraint
        )
    }

    /// Documented aliases, including the canonical spelling itself.
    const fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::About => &["about", "subject", "topic", "question", "ask"],
            Self::Need => &["need", "needs", "info", "information"],
            Self::Related => &["related", "see-also", "seealso", "also"],
            Self::Scope => &["scope", "scopes", "library"],
            Self::Do => &["do", "action", "goal"],
            Self::To => &["to", "context", "for"],
            Self::Constraint => &["constraint", "constraints", "must"],
            Self::Format => &["format", "output", "as"],
            Self::Depth => &["depth", "detail", "detail-level"],
        }
    }

    /// Resolves a raw (case-insensitive) DSL key to its canonical field, honoring aliases.
    #[must_use]
    pub fn from_key(raw_key: &str) -> Option<Self> {
        let normalized = normalize_key(raw_key);
        Self::ALL.into_iter().find(|field| {
            field
                .aliases()
                .iter()
                .any(|alias| normalize_key(alias) == normalized)
        })
    }
}

/// Normalizes a field key for alias comparison: lowercase, hyphens/underscores removed.
fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// Typed, editable composer state produced by parsing and consumed by formatting
/// and by canonical-request conversion. Holds retrieval fields (`about`, `needs`,
/// `related`, `scopes`) and answer-only fields (`action`, `context`,
/// `constraints`, `format`) as strictly separate data so that answer text can
/// never leak into retrieval.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeQueryDraft {
    /// Ordered retrieval subjects.
    pub about: Vec<String>,
    /// Explicit information needs.
    pub needs: Vec<KnowledgeNeed>,
    /// Explicit lower-priority related terms.
    pub related: Vec<String>,
    /// Scope selectors, resolved against an authorized tenant/library at conversion time.
    pub scopes: Vec<KnowledgeScopeSelector>,
    /// Answer-only: typed answer goal.
    pub action: Option<KnowledgeAction>,
    /// Answer-only: free-text application context, never copied into retrieval.
    pub context: Option<String>,
    /// Answer-only: free-text constraints, never copied into retrieval.
    pub constraints: Vec<String>,
    /// Answer-only: requested output presentation.
    pub format: Option<KnowledgeOutputFormat>,
    /// Answer-only: requested response depth.
    pub depth: Option<KnowledgeAnswerDepth>,
}

impl KnowledgeQueryDraft {
    /// Whether any answer-only field is populated.
    #[must_use]
    pub fn has_answer_fields(&self) -> bool {
        self.action.is_some()
            || self.context.is_some()
            || !self.constraints.is_empty()
            || self.format.is_some()
            || self.depth.is_some()
    }

    /// Builds the canonical retrieval-only request. Authorization is supplied by
    /// the caller as `scope_context`; this never inspects or resolves it itself.
    ///
    /// # Errors
    ///
    /// Returns canonical request validation failures.
    pub fn to_search_request(
        &self,
        scope_context: &KnowledgeDslScopeContext,
        mode: RetrievalMode,
        options: KnowledgeSearchOptions,
    ) -> Result<KnowledgeSearchRequest, KnowledgeRequestError> {
        let request = KnowledgeSearchRequest {
            subjects: self
                .about
                .iter()
                .map(|text| KnowledgeSubject { text: text.clone() })
                .collect(),
            needs: self.needs.clone(),
            related_terms: self.related.clone(),
            scopes: self
                .scopes
                .iter()
                .map(|selector| KnowledgeScope {
                    tenant_id: scope_context.tenant_id.clone(),
                    library_id: scope_context.library_id.clone(),
                    selector: selector.clone(),
                })
                .collect(),
            mode,
            options,
        };
        request.validate()?;
        Ok(request)
    }

    /// Builds the optional answer-only request, consuming an already-inspected
    /// evidence fingerprint supplied by the caller. Returns `None` when no
    /// answer-only field is populated, matching "answer generation is optional".
    ///
    /// # Errors
    ///
    /// Returns canonical answer validation failures.
    pub fn to_answer_request(
        &self,
        evidence_fingerprint: &str,
    ) -> Result<Option<KnowledgeAnswerRequest>, KnowledgeRequestError> {
        if !self.has_answer_fields() {
            return Ok(None);
        }
        let request = KnowledgeAnswerRequest {
            evidence_fingerprint: evidence_fingerprint.to_owned(),
            action: self.action,
            context: self.context.clone(),
            constraints: self.constraints.clone(),
            depth: self.depth,
            output: self.format,
        };
        request.validate()?;
        Ok(Some(request))
    }

    /// Reconstructs a draft from the canonical requests. Scope identity
    /// (tenant/library) is intentionally discarded: it is authorization
    /// context, not DSL-expressible composer state.
    #[must_use]
    pub fn from_requests(
        search: &KnowledgeSearchRequest,
        answer: Option<&KnowledgeAnswerRequest>,
    ) -> Self {
        Self {
            about: search.subjects.iter().map(|s| s.text.clone()).collect(),
            needs: search.needs.clone(),
            related: search.related_terms.clone(),
            scopes: search.scopes.iter().map(|s| s.selector.clone()).collect(),
            action: answer.and_then(|a| a.action),
            context: answer.and_then(|a| a.context.clone()),
            constraints: answer.map(|a| a.constraints.clone()).unwrap_or_default(),
            format: answer.and_then(|a| a.output),
            depth: answer.and_then(|a| a.depth),
        }
    }
}

/// Authorized tenant/library identity applied to every scope selector in a
/// draft. Supplied explicitly by the host so authorization stays outside parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeDslScopeContext {
    /// Tenant owning the indexed library.
    pub tenant_id: String,
    /// Authorized semantic library.
    pub library_id: String,
}

/// Result of parsing DSL or natural-language input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeDslParse {
    /// The resulting composer draft.
    pub draft: KnowledgeQueryDraft,
    /// How confidently the draft was derived.
    pub confidence: KnowledgeParseConfidence,
    /// Alternative interpretations not applied, recorded instead of discarded.
    pub ambiguities: Vec<KnowledgeParseAmbiguity>,
    /// Actionable diagnostics: unknown fields, invalid values, and bound overflows.
    pub diagnostics: Vec<KnowledgeDslDiagnostic>,
}

/// Parses DSL text, falling back to deterministic natural-language rules when
/// no `field:` assignment is found anywhere in the input.
#[must_use]
pub fn parse(input: &str) -> KnowledgeDslParse {
    if input.len() > MAX_INPUT_BYTES {
        return KnowledgeDslParse {
            draft: KnowledgeQueryDraft::default(),
            confidence: KnowledgeParseConfidence::Explicit,
            ambiguities: Vec::new(),
            diagnostics: vec![KnowledgeDslDiagnostic {
                severity: DiagnosticSeverity::Error,
                span: Span {
                    start: 0,
                    end: input.len(),
                },
                code: KnowledgeDslDiagnosticCode::ValueTooLong,
                message: format!("input exceeds the {MAX_INPUT_BYTES}-byte parser limit"),
                suggestion: None,
            }],
        };
    }
    let (mut boundaries, unterminated_quote) = find_field_boundaries(input);
    if let Some(quote_start) = unterminated_quote {
        boundaries.retain(|(key_start, _, _)| *key_start < quote_start);
        let mut result = if boundaries.is_empty() {
            KnowledgeDslParse {
                draft: KnowledgeQueryDraft::default(),
                confidence: KnowledgeParseConfidence::Explicit,
                ambiguities: Vec::new(),
                diagnostics: Vec::new(),
            }
        } else {
            parse_dsl(&input[..quote_start], &boundaries)
        };
        result.diagnostics.push(KnowledgeDslDiagnostic {
            severity: DiagnosticSeverity::Error,
            span: Span {
                start: quote_start,
                end: input.len(),
            },
            code: KnowledgeDslDiagnosticCode::UnterminatedQuote,
            message: "quoted value is missing a closing quote".to_owned(),
            suggestion: Some("add a closing double quote".to_owned()),
        });
        return result;
    }
    if boundaries.is_empty() {
        return parse_natural_language(input);
    }
    parse_dsl(input, &boundaries)
}

/// One recognized `identifier:` boundary: `(key_start, value_start, raw_key)`.
type FieldBoundary = (usize, usize, String);

/// Scans `input` outside quoted spans for `identifier:` boundaries: an ASCII
/// word of 2+ characters at a whitespace/start-of-input boundary, immediately
/// followed by `:`. Known fields may omit following whitespace; unknown fields
/// use the stricter whitespace form unless they start the input or follow `;`.
/// Escaped quotes never change quote state.
fn find_field_boundaries(input: &str) -> (Vec<FieldBoundary>, Option<usize>) {
    let bytes: Vec<(usize, char)> = input.char_indices().collect();
    let mut boundaries = Vec::new();
    let mut in_quotes = false;
    let mut escaped = false;
    let mut quote_start = None;
    let mut i = 0usize;
    while i < bytes.len() {
        let (byte_idx, ch) = bytes[i];
        if in_quotes && escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if in_quotes && ch == '\\' {
            escaped = true;
            i += 1;
            continue;
        }
        if ch == '"' {
            in_quotes = !in_quotes;
            quote_start = in_quotes.then_some(byte_idx);
            i += 1;
            continue;
        }
        if in_quotes {
            i += 1;
            continue;
        }
        let hard_boundary = i == 0 || bytes[i - 1].1 == ';';
        let at_boundary = hard_boundary || bytes[i - 1].1.is_whitespace();
        if at_boundary && ch.is_ascii_alphabetic() {
            let mut j = i;
            while j < bytes.len()
                && (bytes[j].1.is_ascii_alphanumeric() || bytes[j].1 == '-' || bytes[j].1 == '_')
            {
                j += 1;
            }
            if j - i >= 2 && j < bytes.len() && bytes[j].1 == ':' {
                let raw_key: String = bytes[i..j].iter().map(|(_, c)| *c).collect();
                let after = j + 1;
                let ends_field = after >= bytes.len()
                    || bytes[after].1.is_whitespace()
                    || bytes[after].1 == '"'
                    || hard_boundary
                    || KnowledgeDslField::from_key(&raw_key).is_some();
                if ends_field {
                    let value_start = if after < bytes.len() {
                        bytes[after].0
                    } else {
                        input.len()
                    };
                    boundaries.push((byte_idx, value_start, raw_key));
                    i = after;
                    continue;
                }
            }
        }
        i += 1;
    }
    (boundaries, quote_start)
}

fn parse_dsl(input: &str, boundaries: &[FieldBoundary]) -> KnowledgeDslParse {
    let mut draft = KnowledgeQueryDraft::default();
    let mut diagnostics = Vec::new();

    let leading = input[..boundaries[0].0].trim();
    if !leading.is_empty() {
        if leading.len() > MAX_TEXT_BYTES {
            diagnostics.push(value_too_long(
                "about",
                Span {
                    start: 0,
                    end: boundaries[0].0,
                },
                MAX_TEXT_BYTES,
            ));
        } else {
            draft.about.push(leading.to_owned());
        }
    }

    let mut seen_single_valued: HashSet<&'static str> = HashSet::new();

    for (index, (key_start, value_start, raw_key)) in boundaries.iter().enumerate() {
        let value_end = boundaries.get(index + 1).map_or(input.len(), |b| b.0);
        let raw_value = input[*value_start..value_end]
            .trim_end()
            .strip_suffix(';')
            .unwrap_or(&input[*value_start..value_end])
            .trim_end();
        let key_span = Span {
            start: *key_start,
            end: *key_start + raw_key.len(),
        };

        let Some(field) = KnowledgeDslField::from_key(raw_key) else {
            diagnostics.push(KnowledgeDslDiagnostic {
                severity: DiagnosticSeverity::Error,
                span: key_span,
                code: KnowledgeDslDiagnosticCode::UnknownField,
                message: format!("unknown field '{raw_key}'"),
                suggestion: suggest_field(raw_key),
            });
            continue;
        };

        if !field.is_multi_valued() {
            let key = field.canonical_key();
            if !seen_single_valued.insert(key) {
                diagnostics.push(KnowledgeDslDiagnostic {
                    severity: DiagnosticSeverity::Warning,
                    span: key_span,
                    code: KnowledgeDslDiagnosticCode::DuplicateField,
                    message: format!("field '{key}' was already set; using the last value"),
                    suggestion: None,
                });
            }
        }

        match field {
            KnowledgeDslField::About => push_bounded_text_list(
                raw_value,
                *value_start,
                &mut draft.about,
                MAX_ABOUT,
                "about",
                &mut diagnostics,
            ),
            KnowledgeDslField::Related => push_bounded_text_list(
                raw_value,
                *value_start,
                &mut draft.related,
                MAX_RELATED,
                "related",
                &mut diagnostics,
            ),
            KnowledgeDslField::Constraint => push_bounded_text_list(
                raw_value,
                *value_start,
                &mut draft.constraints,
                MAX_CONSTRAINTS,
                "constraint",
                &mut diagnostics,
            ),
            KnowledgeDslField::Need => {
                for (value, span) in split_value_list(raw_value, *value_start) {
                    if value.is_empty() {
                        continue;
                    }
                    match parse_need(&value) {
                        Some(need) => {
                            if draft.needs.len() >= MAX_NEEDS {
                                diagnostics.push(too_many_values("need", span, MAX_NEEDS));
                            } else {
                                draft.needs.push(need);
                            }
                        }
                        None => diagnostics.push(KnowledgeDslDiagnostic {
                            severity: DiagnosticSeverity::Error,
                            span,
                            code: KnowledgeDslDiagnosticCode::InvalidNeedValue,
                            message: format!("'{value}' is not a known knowledge need"),
                            suggestion: suggest(&value, NEED_NAMES),
                        }),
                    }
                }
            }
            KnowledgeDslField::Scope => {
                for (value, span) in split_value_list(raw_value, *value_start) {
                    if value.is_empty() {
                        continue;
                    }
                    match parse_scope_selector(&value) {
                        Some(selector) => {
                            if draft.scopes.len() >= MAX_SCOPES {
                                diagnostics.push(too_many_values("scope", span, MAX_SCOPES));
                            } else {
                                draft.scopes.push(selector);
                            }
                        }
                        None => diagnostics.push(KnowledgeDslDiagnostic {
                            severity: DiagnosticSeverity::Error,
                            span,
                            code: KnowledgeDslDiagnosticCode::InvalidScopeValue,
                            message: format!(
                                "'{value}' is not a valid scope; use 'library', 'root:<id>', or 'workspace:<id>'"
                            ),
                            suggestion: None,
                        }),
                    }
                }
            }
            KnowledgeDslField::Do => {
                let (value, span) = unquote_single(raw_value, *value_start);
                if value.is_empty() {
                    draft.action = None;
                } else if let Some(action) = parse_action(&value) {
                    draft.action = Some(action);
                } else {
                    diagnostics.push(KnowledgeDslDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        span,
                        code: KnowledgeDslDiagnosticCode::InvalidActionValue,
                        message: format!("'{value}' is not a known answer action"),
                        suggestion: suggest(&value, ACTION_NAMES),
                    });
                }
            }
            KnowledgeDslField::To => {
                let (value, span) = unquote_single(raw_value, *value_start);
                if value.len() > MAX_TEXT_BYTES {
                    diagnostics.push(value_too_long("to", span, MAX_TEXT_BYTES));
                    draft.context = None;
                } else {
                    draft.context = if value.is_empty() { None } else { Some(value) };
                }
            }
            KnowledgeDslField::Format => {
                let (value, span) = unquote_single(raw_value, *value_start);
                if value.is_empty() {
                    draft.format = None;
                } else if let Some(format) = parse_format(&value) {
                    draft.format = Some(format);
                } else {
                    diagnostics.push(KnowledgeDslDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        span,
                        code: KnowledgeDslDiagnosticCode::InvalidFormatValue,
                        message: format!("'{value}' is not a known output format"),
                        suggestion: suggest(&value, FORMAT_NAMES),
                    });
                }
            }
            KnowledgeDslField::Depth => {
                let (value, span) = unquote_single(raw_value, *value_start);
                if value.is_empty() {
                    draft.depth = None;
                } else if let Some(depth) = parse_depth(&value) {
                    draft.depth = Some(depth);
                } else {
                    diagnostics.push(KnowledgeDslDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        span,
                        code: KnowledgeDslDiagnosticCode::InvalidDepthValue,
                        message: format!("'{value}' is not a known answer depth"),
                        suggestion: suggest(&value, DEPTH_NAMES),
                    });
                }
            }
        }
    }

    KnowledgeDslParse {
        draft,
        confidence: KnowledgeParseConfidence::Explicit,
        ambiguities: Vec::new(),
        diagnostics,
    }
}

fn push_bounded_text_list(
    raw_value: &str,
    base_offset: usize,
    target: &mut Vec<String>,
    max: usize,
    field_name: &str,
    diagnostics: &mut Vec<KnowledgeDslDiagnostic>,
) {
    for (value, span) in split_value_list(raw_value, base_offset) {
        if value.is_empty() {
            diagnostics.push(KnowledgeDslDiagnostic {
                severity: DiagnosticSeverity::Warning,
                span,
                code: KnowledgeDslDiagnosticCode::EmptyValue,
                message: format!("'{field_name}' has an empty value"),
                suggestion: None,
            });
            continue;
        }
        if value.len() > MAX_TEXT_BYTES {
            diagnostics.push(value_too_long(field_name, span, MAX_TEXT_BYTES));
            continue;
        }
        if target.len() >= max {
            diagnostics.push(too_many_values(field_name, span, max));
            continue;
        }

        target.push(value);
    }
}

fn value_too_long(field_name: &str, span: Span, maximum: usize) -> KnowledgeDslDiagnostic {
    KnowledgeDslDiagnostic {
        severity: DiagnosticSeverity::Error,
        span,
        code: KnowledgeDslDiagnosticCode::ValueTooLong,
        message: format!("'{field_name}' exceeds the {maximum}-byte limit"),
        suggestion: None,
    }
}

fn too_many_values(field_name: &str, span: Span, max: usize) -> KnowledgeDslDiagnostic {
    KnowledgeDslDiagnostic {
        severity: DiagnosticSeverity::Warning,
        span,
        code: KnowledgeDslDiagnosticCode::TooManyValues,
        message: format!("'{field_name}' accepts at most {max} values; extra value dropped"),
        suggestion: None,
    }
}

/// Splits a raw field-value segment into comma-separated tokens, honoring
/// quotes (commas inside quotes are literal) and unescaping `\"`/`\\`. Returns
/// each token's unescaped text with its span in the original source.
fn split_value_list(raw: &str, base_offset: usize) -> Vec<(String, Span)> {
    let chars: Vec<(usize, char)> = raw.char_indices().collect();
    let mut tokens = Vec::new();
    let mut token_start = 0usize;
    let mut in_quotes = false;
    let mut escaped = false;
    let mut i = 0usize;
    while i < chars.len() {
        let (_, ch) = chars[i];
        if in_quotes && escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if in_quotes && ch == '\\' {
            escaped = true;
            i += 1;
            continue;
        }
        match ch {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                let end = chars[i].0;
                tokens.push(build_token(raw, token_start, end, base_offset));
                token_start = end + 1;
            }
            _ => {}
        }
        i += 1;
    }
    tokens.push(build_token(raw, token_start, raw.len(), base_offset));
    tokens
}

/// Unescapes and trims one value occurrence (no comma splitting): used for
/// single-valued fields (`do`, `to`, `format`).
fn unquote_single(raw: &str, base_offset: usize) -> (String, Span) {
    build_token(raw, 0, raw.len(), base_offset)
}

fn build_token(raw: &str, start: usize, end: usize, base_offset: usize) -> (String, Span) {
    let segment = &raw[start..end];
    let trimmed = segment.trim();
    let leading_trim = segment.len() - segment.trim_start().len();
    let trimmed_start = start + leading_trim;
    let span = Span {
        start: base_offset + trimmed_start,
        end: base_offset + trimmed_start + trimmed.len(),
    };
    (unescape(trimmed), span)
}

/// Strips a single pair of surrounding quotes (if present) and unescapes
/// `\"` and `\\`; otherwise returns the trimmed text unchanged.
fn unescape(value: &str) -> String {
    let Some(stripped) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return value.to_owned();
    };
    let mut out = String::with_capacity(stripped.len());
    let mut chars = stripped.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('"') => {
                    out.push('"');
                    chars.next();
                }
                Some('\\') => {
                    out.push('\\');
                    chars.next();
                }
                _ => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

const NEED_NAMES: &[&str] = &[
    "overview",
    "definition",
    "procedure",
    "examples",
    "evidence",
    "arguments",
    "comparison",
    "limitations",
    "references",
];

fn parse_need(value: &str) -> Option<KnowledgeNeed> {
    let normalized = normalize_key(value);
    let matches = |canonical: &str, aliases: &[&str]| {
        normalize_key(canonical) == normalized
            || aliases
                .iter()
                .any(|alias| normalize_key(alias) == normalized)
    };
    if matches("overview", &[]) {
        Some(KnowledgeNeed::Overview)
    } else if matches("definition", &["define", "definitions"]) {
        Some(KnowledgeNeed::Definition)
    } else if matches("procedure", &["howTo", "how-to", "steps", "procedures"]) {
        Some(KnowledgeNeed::Procedure)
    } else if matches("examples", &["sample", "example", "samples"]) {
        Some(KnowledgeNeed::Examples)
    } else if matches("evidence", &["support"]) {
        Some(KnowledgeNeed::Evidence)
    } else if matches("arguments", &["prosCons", "pros-cons", "argument"]) {
        Some(KnowledgeNeed::Arguments)
    } else if matches("comparison", &["compare", "comparisons"]) {
        Some(KnowledgeNeed::Comparison)
    } else if matches("limitations", &["risks", "limitation", "risk"]) {
        Some(KnowledgeNeed::Limitations)
    } else if matches("references", &["sources", "reference", "source"]) {
        Some(KnowledgeNeed::References)
    } else {
        None
    }
}

const ACTION_NAMES: &[&str] = &["explain", "learn", "apply", "evaluate", "compare", "cite"];

fn parse_action(value: &str) -> Option<KnowledgeAction> {
    let normalized = normalize_key(value);
    let matches = |canonical: &str, aliases: &[&str]| {
        normalize_key(canonical) == normalized
            || aliases
                .iter()
                .any(|alias| normalize_key(alias) == normalized)
    };
    if matches("explain", &[]) {
        Some(KnowledgeAction::Explain)
    } else if matches("learn", &[]) {
        Some(KnowledgeAction::Learn)
    } else if matches("apply", &["implement", "howTo", "use"]) {
        Some(KnowledgeAction::Apply)
    } else if matches("evaluate", &["analyse", "analyze"]) {
        Some(KnowledgeAction::Evaluate)
    } else if matches("compare", &[]) {
        Some(KnowledgeAction::Compare)
    } else if matches("cite", &["source"]) {
        Some(KnowledgeAction::Cite)
    } else {
        None
    }
}

const FORMAT_NAMES: &[&str] = &["narrative", "bullets", "steps", "table"];

fn parse_format(value: &str) -> Option<KnowledgeOutputFormat> {
    let normalized = normalize_key(value);
    let matches = |canonical: &str, aliases: &[&str]| {
        normalize_key(canonical) == normalized
            || aliases
                .iter()
                .any(|alias| normalize_key(alias) == normalized)
    };
    if matches("narrative", &["prose"]) {
        Some(KnowledgeOutputFormat::Narrative)
    } else if matches("bullets", &["list", "bulleted"]) {
        Some(KnowledgeOutputFormat::Bullets)
    } else if matches("steps", &["ordered", "numbered"]) {
        Some(KnowledgeOutputFormat::Steps)
    } else if matches("table", &["tabular", "grid"]) {
        Some(KnowledgeOutputFormat::Table)
    } else {
        None
    }
}

const DEPTH_NAMES: &[&str] = &["brief", "standard", "detailed"];

fn parse_depth(value: &str) -> Option<KnowledgeAnswerDepth> {
    match normalize_key(value).as_str() {
        "brief" | "short" => Some(KnowledgeAnswerDepth::Brief),
        "standard" | "normal" => Some(KnowledgeAnswerDepth::Standard),
        "detailed" | "deep" => Some(KnowledgeAnswerDepth::Detailed),
        _ => None,
    }
}

/// Parses `library`, `root:<id>`, or `workspace:<id>`.
fn parse_scope_selector(value: &str) -> Option<KnowledgeScopeSelector> {
    let normalized = normalize_key(value);
    if normalized == "library" || normalized == "wholelibrary" {
        return Some(KnowledgeScopeSelector::WholeLibrary);
    }
    let (prefix, id) = value.split_once(':')?;
    let id = id.trim();
    if id.is_empty() || id.len() > MAX_IDENTIFIER_BYTES {
        return None;
    }
    match normalize_key(prefix).as_str() {
        "root" => Some(KnowledgeScopeSelector::Root {
            root_id: id.to_owned(),
        }),
        "workspace" => Some(KnowledgeScopeSelector::Workspace {
            workspace_id: id.to_owned(),
        }),
        _ => None,
    }
}

fn suggest_field(raw_key: &str) -> Option<String> {
    let candidates: Vec<&str> = KnowledgeDslField::ALL
        .iter()
        .map(|field| field.canonical_key())
        .collect();
    suggest(raw_key, &candidates)
}

fn suggest(value: &str, candidates: &[&str]) -> Option<String> {
    let normalized = normalize_key(value);
    candidates
        .iter()
        .map(|candidate| {
            (
                *candidate,
                levenshtein(&normalized, &normalize_key(candidate)),
            )
        })
        .min_by_key(|(_, distance)| *distance)
        .filter(|(_, distance)| *distance <= MAX_SUGGESTION_DISTANCE && *distance > 0)
        .map(|(candidate, _)| candidate.to_owned())
}

/// Classic Wagner-Fischer edit distance over `char`s (locale-safe: operates on
/// Unicode scalar values, not bytes).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut previous_diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let temp = row[j + 1];
            row[j + 1] = if ca == cb {
                previous_diagonal
            } else {
                1 + previous_diagonal.min(row[j]).min(row[j + 1])
            };
            previous_diagonal = temp;
        }
    }
    row[b.len()]
}

// --- Natural-language rule templates -------------------------------------

/// Strips an ASCII, case-insensitive prefix without disturbing Unicode byte
/// boundaries: `to_ascii_lowercase` only rewrites single-byte ASCII bytes, so
/// it never changes the byte length or char boundaries of `input`.
fn strip_prefix_ci<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    if input.len() < prefix.len() || !input.is_char_boundary(prefix.len()) {
        return None;
    }
    if input[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&input[prefix.len()..])
    } else {
        None
    }
}

/// Finds the first case-insensitive occurrence of an ASCII `needle`, returning
/// a byte offset valid in `haystack` (same reasoning as [`strip_prefix_ci`]).
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
}

fn split_comparison_subjects(remainder: &str) -> Vec<String> {
    for separator in [" and ", " versus ", " vs. ", " vs "] {
        if let Some(pos) = find_ci(remainder, separator) {
            let left = remainder[..pos].trim();
            let right = remainder[pos + separator.len()..].trim();
            if !left.is_empty() && !right.is_empty() {
                return vec![left.to_owned(), right.to_owned()];
            }
        }
    }
    let trimmed = remainder.trim();
    if trimmed.is_empty() {
        Vec::new()
    } else {
        vec![trimmed.to_owned()]
    }
}

fn draft_with_about(
    subjects: Vec<String>,
    needs: Vec<KnowledgeNeed>,
    action: Option<KnowledgeAction>,
) -> KnowledgeQueryDraft {
    KnowledgeQueryDraft {
        about: subjects,
        needs,
        action,
        ..KnowledgeQueryDraft::default()
    }
}

fn split_application_context(value: &str) -> (&str, Option<&str>) {
    for separator in [" to ", " for "] {
        if let Some(position) = find_ci(value, separator) {
            let subject = value[..position].trim();
            let context = value[position + separator.len()..].trim();
            if !subject.is_empty() && !context.is_empty() {
                return (subject, Some(context));
            }
        }
    }
    (value.trim(), None)
}

fn application_result(
    value: &str,
    needs: Vec<KnowledgeNeed>,
    action: KnowledgeAction,
) -> KnowledgeDslParse {
    let (subject, context) = split_application_context(value);
    let mut draft = draft_with_about(vec![subject.to_owned()], needs, Some(action));
    draft.context = context.map(str::to_owned);
    let ambiguities = context.map_or_else(Vec::new, |_| {
        vec![KnowledgeParseAmbiguity {
            description: "the 'to' or 'for' phrase was treated as answer context; it may instead \
                be part of the subject"
                .to_owned(),
            alternative_need: None,
            alternative_action: Some(action),
        }]
    });
    natural_result(
        draft,
        if ambiguities.is_empty() {
            KnowledgeParseConfidence::Deterministic
        } else {
            KnowledgeParseConfidence::Ambiguous
        },
        ambiguities,
    )
}

/// Deterministically recognizes common natural-language question forms
/// (find, definitions, procedures, examples, limitations, evidence,
/// apply/use/analyse, and comparisons) with no field syntax present at all.
/// Application context is never inferred from free text: only the typed
/// `about`/`needs`/`action` shape is derived, keeping subject/need strictly
/// separate from any hypothetical answer context.
fn parse_natural_language(input: &str) -> KnowledgeDslParse {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return KnowledgeDslParse {
            draft: KnowledgeQueryDraft::default(),
            confidence: KnowledgeParseConfidence::Deterministic,
            ambiguities: Vec::new(),
            diagnostics: vec![KnowledgeDslDiagnostic {
                severity: DiagnosticSeverity::Warning,
                span: Span {
                    start: 0,
                    end: input.len(),
                },
                code: KnowledgeDslDiagnosticCode::EmptyValue,
                message: "input is empty".to_owned(),
                suggestion: None,
            }],
        };
    }
    let normalized = trimmed.trim_end_matches(['?', '.', '!']).trim();

    if let Some(rest) = strip_prefix_ci(normalized, "how to ")
        .or_else(|| strip_prefix_ci(normalized, "how do i "))
        .or_else(|| strip_prefix_ci(normalized, "how can i "))
    {
        if let Some(compare_rest) = strip_prefix_ci(rest, "compare ") {
            let subjects = split_comparison_subjects(compare_rest);
            let draft = draft_with_about(
                subjects,
                vec![KnowledgeNeed::Comparison],
                Some(KnowledgeAction::Compare),
            );
            return natural_result(
                draft,
                KnowledgeParseConfidence::Ambiguous,
                vec![KnowledgeParseAmbiguity {
                    description: "the 'how to' phrasing could also be read as a request for a \
                        step-by-step procedure rather than a comparison"
                        .to_owned(),
                    alternative_need: Some(KnowledgeNeed::Procedure),
                    alternative_action: Some(KnowledgeAction::Apply),
                }],
            );
        }
        if let Some(analyse_rest) =
            strip_prefix_ci(rest, "analyse ").or_else(|| strip_prefix_ci(rest, "analyze "))
        {
            return application_result(
                analyse_rest,
                vec![
                    KnowledgeNeed::Evidence,
                    KnowledgeNeed::Arguments,
                    KnowledgeNeed::Limitations,
                ],
                KnowledgeAction::Evaluate,
            );
        }
        let subject = strip_prefix_ci(rest, "use ")
            .or_else(|| strip_prefix_ci(rest, "apply "))
            .unwrap_or(rest);
        return application_result(
            subject,
            vec![KnowledgeNeed::Procedure, KnowledgeNeed::Examples],
            KnowledgeAction::Apply,
        );
    }
    for prefix in ["steps to ", "steps for "] {
        if let Some(rest) = strip_prefix_ci(normalized, prefix) {
            return deterministic_result(draft_with_about(
                vec![rest.trim().to_owned()],
                vec![KnowledgeNeed::Procedure, KnowledgeNeed::Examples],
                Some(KnowledgeAction::Apply),
            ));
        }
    }
    for prefix in ["analyse ", "analyze ", "evaluate "] {
        if let Some(rest) = strip_prefix_ci(normalized, prefix) {
            return application_result(
                rest,
                vec![
                    KnowledgeNeed::Evidence,
                    KnowledgeNeed::Arguments,
                    KnowledgeNeed::Limitations,
                ],
                KnowledgeAction::Evaluate,
            );
        }
    }
    for prefix in ["apply ", "use "] {
        if let Some(rest) = strip_prefix_ci(normalized, prefix) {
            return application_result(
                rest,
                vec![KnowledgeNeed::Procedure, KnowledgeNeed::Examples],
                KnowledgeAction::Apply,
            );
        }
    }
    for prefix in [
        "what is ",
        "what are ",
        "define ",
        "definition of ",
        "meaning of ",
    ] {
        if let Some(rest) = strip_prefix_ci(normalized, prefix) {
            return deterministic_result(draft_with_about(
                vec![rest.trim().to_owned()],
                vec![KnowledgeNeed::Definition],
                None,
            ));
        }
    }
    for prefix in [
        "examples of ",
        "example of ",
        "show me examples of ",
        "give me examples of ",
    ] {
        if let Some(rest) = strip_prefix_ci(normalized, prefix) {
            return deterministic_result(draft_with_about(
                vec![rest.trim().to_owned()],
                vec![KnowledgeNeed::Examples],
                None,
            ));
        }
    }
    for prefix in [
        "limitations of ",
        "drawbacks of ",
        "risks of ",
        "downsides of ",
        "caveats of ",
    ] {
        if let Some(rest) = strip_prefix_ci(normalized, prefix) {
            return deterministic_result(draft_with_about(
                vec![rest.trim().to_owned()],
                vec![KnowledgeNeed::Limitations],
                None,
            ));
        }
    }
    for prefix in [
        "evidence for ",
        "evidence of ",
        "proof of ",
        "support for ",
        "citations for ",
    ] {
        if let Some(rest) = strip_prefix_ci(normalized, prefix) {
            return deterministic_result(draft_with_about(
                vec![rest.trim().to_owned()],
                vec![KnowledgeNeed::Evidence],
                None,
            ));
        }
    }
    if let Some(rest) = strip_prefix_ci(normalized, "difference between ") {
        let subjects = split_comparison_subjects(rest);
        return deterministic_result(draft_with_about(
            subjects,
            vec![KnowledgeNeed::Comparison],
            Some(KnowledgeAction::Compare),
        ));
    }
    if let Some(rest) = strip_prefix_ci(normalized, "compare ") {
        let subjects = split_comparison_subjects(rest);
        return deterministic_result(draft_with_about(
            subjects,
            vec![KnowledgeNeed::Comparison],
            Some(KnowledgeAction::Compare),
        ));
    }
    for separator in [" vs. ", " vs ", " versus "] {
        if let Some(pos) = find_ci(normalized, separator) {
            let left = normalized[..pos].trim();
            let right = normalized[pos + separator.len()..].trim();
            if !left.is_empty() && !right.is_empty() {
                return deterministic_result(draft_with_about(
                    vec![left.to_owned(), right.to_owned()],
                    vec![KnowledgeNeed::Comparison],
                    Some(KnowledgeAction::Compare),
                ));
            }
        }
    }
    for prefix in ["find ", "search for ", "look up ", "look for "] {
        if let Some(rest) = strip_prefix_ci(normalized, prefix) {
            return deterministic_result(draft_with_about(
                vec![rest.trim().to_owned()],
                Vec::new(),
                None,
            ));
        }
    }

    deterministic_result(draft_with_about(
        vec![normalized.to_owned()],
        Vec::new(),
        None,
    ))
}

fn deterministic_result(draft: KnowledgeQueryDraft) -> KnowledgeDslParse {
    natural_result(draft, KnowledgeParseConfidence::Deterministic, Vec::new())
}

fn natural_result(
    draft: KnowledgeQueryDraft,
    confidence: KnowledgeParseConfidence,
    ambiguities: Vec<KnowledgeParseAmbiguity>,
) -> KnowledgeDslParse {
    let mut draft = draft;
    let mut diagnostics = Vec::new();
    draft.about.retain(|value| {
        if value.len() <= MAX_TEXT_BYTES {
            true
        } else {
            diagnostics.push(value_too_long(
                "about",
                Span {
                    start: 0,
                    end: value.len(),
                },
                MAX_TEXT_BYTES,
            ));
            false
        }
    });
    if draft
        .context
        .as_ref()
        .is_some_and(|value| value.len() > MAX_TEXT_BYTES)
    {
        let length = draft.context.as_ref().map_or(0, String::len);
        draft.context = None;
        diagnostics.push(value_too_long(
            "to",
            Span {
                start: 0,
                end: length,
            },
            MAX_TEXT_BYTES,
        ));
    }
    KnowledgeDslParse {
        draft,
        confidence,
        ambiguities,
        diagnostics,
    }
}

// --- Formatting -----------------------------------------------------------

/// Formats a draft as multiline DSL: one canonical field per line, values
/// comma-joined and quoted whenever quoting is required for lossless
/// round-tripping. Empty/default fields are omitted.
#[must_use]
pub fn format_multiline(draft: &KnowledgeQueryDraft) -> String {
    field_lines(draft).join("\n")
}

/// Formats a draft as compact single-line DSL, in the same canonical field order.
#[must_use]
pub fn format_compact(draft: &KnowledgeQueryDraft) -> String {
    field_lines(draft).join(" ")
}

fn field_lines(draft: &KnowledgeQueryDraft) -> Vec<String> {
    let mut lines = Vec::new();
    if !draft.about.is_empty() {
        lines.push(format!(
            "about: {}",
            join_quoted(draft.about.iter().map(String::as_str))
        ));
    }
    if !draft.needs.is_empty() {
        let names = draft.needs.iter().map(|need| need_name(*need));
        lines.push(format!("need: {}", join_quoted(names)));
    }
    if !draft.related.is_empty() {
        lines.push(format!(
            "related: {}",
            join_quoted(draft.related.iter().map(String::as_str))
        ));
    }
    if !draft.scopes.is_empty() {
        let selectors = draft.scopes.iter().map(scope_selector_text);
        lines.push(format!("scope: {}", join_quoted_owned(selectors)));
    }
    if let Some(action) = draft.action {
        lines.push(format!("do: {}", quote_if_needed(action_name(action))));
    }
    if let Some(context) = &draft.context {
        lines.push(format!("to: {}", quote_if_needed(context)));
    }
    if !draft.constraints.is_empty() {
        lines.push(format!(
            "constraint: {}",
            join_quoted(draft.constraints.iter().map(String::as_str))
        ));
    }
    if let Some(format) = draft.format {
        lines.push(format!("format: {}", quote_if_needed(format_name(format))));
    }
    if let Some(depth) = draft.depth {
        lines.push(format!("depth: {}", quote_if_needed(depth_name(depth))));
    }
    lines
}

fn need_name(need: KnowledgeNeed) -> &'static str {
    match need {
        KnowledgeNeed::Overview => "overview",
        KnowledgeNeed::Definition => "definition",
        KnowledgeNeed::Procedure => "procedure",
        KnowledgeNeed::Examples => "examples",
        KnowledgeNeed::Evidence => "evidence",
        KnowledgeNeed::Arguments => "arguments",
        KnowledgeNeed::Comparison => "comparison",
        KnowledgeNeed::Limitations => "limitations",
        KnowledgeNeed::References => "references",
    }
}

fn action_name(action: KnowledgeAction) -> &'static str {
    match action {
        KnowledgeAction::Explain => "explain",
        KnowledgeAction::Learn => "learn",
        KnowledgeAction::Apply => "apply",
        KnowledgeAction::Evaluate => "evaluate",
        KnowledgeAction::Compare => "compare",
        KnowledgeAction::Cite => "cite",
    }
}

fn format_name(format: KnowledgeOutputFormat) -> &'static str {
    match format {
        KnowledgeOutputFormat::Narrative => "narrative",
        KnowledgeOutputFormat::Bullets => "bullets",
        KnowledgeOutputFormat::Steps => "steps",
        KnowledgeOutputFormat::Table => "table",
    }
}

fn depth_name(depth: KnowledgeAnswerDepth) -> &'static str {
    match depth {
        KnowledgeAnswerDepth::Brief => "brief",
        KnowledgeAnswerDepth::Standard => "standard",
        KnowledgeAnswerDepth::Detailed => "detailed",
    }
}

fn scope_selector_text(selector: &KnowledgeScopeSelector) -> String {
    match selector {
        KnowledgeScopeSelector::WholeLibrary => "library".to_owned(),
        KnowledgeScopeSelector::Root { root_id } => format!("root:{root_id}"),
        KnowledgeScopeSelector::Workspace { workspace_id } => format!("workspace:{workspace_id}"),
    }
}

fn join_quoted<'a>(values: impl Iterator<Item = &'a str>) -> String {
    values.map(quote_if_needed).collect::<Vec<_>>().join(", ")
}

fn join_quoted_owned(values: impl Iterator<Item = String>) -> String {
    values
        .map(|value| quote_if_needed(&value))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Quotes and escapes a value whenever unquoted output would not round-trip:
/// empty text, a comma, a `"`, a newline, leading/trailing whitespace, or any
/// `:` (which could otherwise be misread as a new field boundary).
fn quote_if_needed(value: &str) -> String {
    let needs_quoting = value.is_empty()
        || value.trim() != value
        || value.contains([',', '"', '\\', '\n', ':', ';']);
    if !needs_quoting {
        return value.to_owned();
    }
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::KnowledgeSearchOptions;

    fn scope_context() -> KnowledgeDslScopeContext {
        KnowledgeDslScopeContext {
            tenant_id: "tenant-a".into(),
            library_id: "library-a".into(),
        }
    }

    fn about(draft: &KnowledgeQueryDraft) -> Vec<&str> {
        draft.about.iter().map(String::as_str).collect()
    }

    // --- DSL field parsing ------------------------------------------------

    #[test]
    fn parses_compact_dsl_with_all_canonical_fields() {
        let result = parse(
            r#"about: "rust async runtimes" need: definition, procedure related: tokio scope: library do: apply to: "write a scheduler" constraint: "no unsafe" format: steps"#,
        );
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.confidence, KnowledgeParseConfidence::Explicit);
        assert_eq!(about(&result.draft), vec!["rust async runtimes"]);
        assert_eq!(
            result.draft.needs,
            vec![KnowledgeNeed::Definition, KnowledgeNeed::Procedure]
        );
        assert_eq!(result.draft.related, vec!["tokio".to_owned()]);
        assert_eq!(
            result.draft.scopes,
            vec![KnowledgeScopeSelector::WholeLibrary]
        );
        assert_eq!(result.draft.action, Some(KnowledgeAction::Apply));
        assert_eq!(result.draft.context.as_deref(), Some("write a scheduler"));
        assert_eq!(result.draft.constraints, vec!["no unsafe".to_owned()]);
        assert_eq!(result.draft.format, Some(KnowledgeOutputFormat::Steps));
    }

    #[test]
    fn parses_multiline_dsl_with_repeated_fields() {
        let input = "about: rust\nneed: definition\nneed: procedure\nrelated: tokio\nrelated: async-std\nscope: workspace:workspace-a\n";
        let result = parse(input);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(about(&result.draft), vec!["rust"]);
        assert_eq!(
            result.draft.needs,
            vec![KnowledgeNeed::Definition, KnowledgeNeed::Procedure]
        );
        assert_eq!(
            result.draft.related,
            vec!["tokio".to_owned(), "async-std".to_owned()]
        );
        assert_eq!(
            result.draft.scopes,
            vec![KnowledgeScopeSelector::Workspace {
                workspace_id: "workspace-a".into()
            }]
        );
    }

    #[test]
    fn repeated_field_occurrences_and_comma_lists_combine() {
        let result = parse("need: definition, procedure need: examples");
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            result.draft.needs,
            vec![
                KnowledgeNeed::Definition,
                KnowledgeNeed::Procedure,
                KnowledgeNeed::Examples
            ]
        );
    }

    #[test]
    fn leading_bare_text_becomes_implicit_about() {
        let result = parse("rust async runtimes need: definition");
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(about(&result.draft), vec!["rust async runtimes"]);
        assert_eq!(result.draft.needs, vec![KnowledgeNeed::Definition]);
    }

    #[test]
    fn quoted_values_protect_commas_and_colons() {
        let result = parse(r#"about: "rust: the async story, part one""#);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            about(&result.draft),
            vec!["rust: the async story, part one"]
        );
    }

    #[test]
    fn unquoted_colon_inside_value_without_trailing_space_is_not_a_field() {
        let result = parse("scope: workspace:workspace-a");
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            result.draft.scopes,
            vec![KnowledgeScopeSelector::Workspace {
                workspace_id: "workspace-a".into()
            }]
        );
    }

    #[test]
    fn escaped_quote_inside_quoted_value_round_trips() {
        let result = parse(r#"about: "say \"hello\" politely""#);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(about(&result.draft), vec![r#"say "hello" politely"#]);
    }

    #[test]
    fn escaped_quotes_protect_commas_and_following_answer_fields() {
        let result = parse(r#"about: "say \"hi, there\"" need: examples do: apply to: "a demo""#);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(about(&result.draft), vec![r#"say "hi, there""#]);
        assert_eq!(result.draft.needs, vec![KnowledgeNeed::Examples]);
        assert_eq!(result.draft.action, Some(KnowledgeAction::Apply));
        assert_eq!(result.draft.context.as_deref(), Some("a demo"));
    }

    #[test]
    fn unterminated_quote_is_rejected_without_absorbing_answer_fields() {
        let result = parse(r#"about: "broken do: apply"#);
        assert!(result.draft.about.is_empty());
        assert!(result.draft.action.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == KnowledgeDslDiagnosticCode::UnterminatedQuote)
        );
    }

    #[test]
    fn no_space_semicolon_compact_form_is_supported() {
        let result = parse("about:rust;need:definition;related:ownership");
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(about(&result.draft), vec!["rust"]);
        assert_eq!(result.draft.needs, vec![KnowledgeNeed::Definition]);
        assert_eq!(result.draft.related, vec!["ownership"]);
    }

    #[test]
    fn whitespace_and_blank_lines_are_tolerated() {
        let result = parse("  about:   rust   \n\n   need:   definition  \n");
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(about(&result.draft), vec!["rust"]);
        assert_eq!(result.draft.needs, vec![KnowledgeNeed::Definition]);
    }

    #[test]
    fn documented_field_aliases_are_recognized() {
        let result = parse(
            r#"subject: rust needs: procedure also: tokio library: workspace:w1 action: apply context: "ship a demo" must: "no unsafe" output: steps"#,
        );
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(about(&result.draft), vec!["rust"]);
        assert_eq!(result.draft.needs, vec![KnowledgeNeed::Procedure]);
        assert_eq!(result.draft.related, vec!["tokio".to_owned()]);
        assert_eq!(
            result.draft.scopes,
            vec![KnowledgeScopeSelector::Workspace {
                workspace_id: "w1".into()
            }]
        );
        assert_eq!(result.draft.action, Some(KnowledgeAction::Apply));
        assert_eq!(result.draft.context.as_deref(), Some("ship a demo"));
        assert_eq!(result.draft.constraints, vec!["no unsafe".to_owned()]);
        assert_eq!(result.draft.format, Some(KnowledgeOutputFormat::Steps));
    }

    #[test]
    fn need_value_aliases_are_recognized() {
        let result =
            parse("about: rust need: howTo, sample, support, prosCons, compare, risks, sources");
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            result.draft.needs,
            vec![
                KnowledgeNeed::Procedure,
                KnowledgeNeed::Examples,
                KnowledgeNeed::Evidence,
                KnowledgeNeed::Arguments,
                KnowledgeNeed::Comparison,
                KnowledgeNeed::Limitations,
                KnowledgeNeed::References,
            ]
        );
    }

    #[test]
    fn unknown_field_produces_typo_diagnostic_with_suggestion() {
        let result = parse("abot: rust");
        assert_eq!(result.draft.about, Vec::<String>::new());
        assert_eq!(result.diagnostics.len(), 1);
        let diagnostic = &result.diagnostics[0];
        assert_eq!(diagnostic.code, KnowledgeDslDiagnosticCode::UnknownField);
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
        assert_eq!(diagnostic.suggestion.as_deref(), Some("about"));
        assert_eq!(diagnostic.span, Span { start: 0, end: 4 });
    }

    #[test]
    fn unrecognized_colon_word_does_not_silently_contaminate_about() {
        let result = parse("about: rust: basics");
        // "rust:" is itself a field-boundary shape; it is flagged, not merged
        // silently into the `about` value.
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == KnowledgeDslDiagnosticCode::UnknownField)
        );
        assert!(
            !about(&result.draft)
                .iter()
                .any(|value| value.contains("basics"))
        );
    }

    #[test]
    fn invalid_need_value_reports_suggestion() {
        let result = parse("about: rust need: definiton");
        assert_eq!(result.draft.needs, Vec::new());
        let diagnostic = result
            .diagnostics
            .iter()
            .find(|d| d.code == KnowledgeDslDiagnosticCode::InvalidNeedValue)
            .expect("expected invalid need diagnostic");
        assert_eq!(diagnostic.suggestion.as_deref(), Some("definition"));
    }

    #[test]
    fn invalid_scope_value_is_reported() {
        let result = parse("about: rust scope: mystery-place");
        assert!(result.draft.scopes.is_empty());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == KnowledgeDslDiagnosticCode::InvalidScopeValue)
        );
    }

    #[test]
    fn invalid_action_and_format_values_are_reported_with_suggestions() {
        let result = parse("about: rust do: implament format: buletts");
        let do_diag = result
            .diagnostics
            .iter()
            .find(|d| d.code == KnowledgeDslDiagnosticCode::InvalidActionValue)
            .expect("expected invalid action diagnostic");
        assert_eq!(do_diag.suggestion.as_deref(), None); // "implament" is closer to "apply" alias, not a canonical name match distance<=2
        let format_diag = result
            .diagnostics
            .iter()
            .find(|d| d.code == KnowledgeDslDiagnosticCode::InvalidFormatValue)
            .expect("expected invalid format diagnostic");
        assert_eq!(format_diag.suggestion.as_deref(), Some("bullets"));
    }

    #[test]
    fn duplicate_single_valued_field_uses_last_value_and_warns() {
        let result = parse("about: rust do: explain do: learn");
        assert_eq!(result.draft.action, Some(KnowledgeAction::Learn));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == KnowledgeDslDiagnosticCode::DuplicateField
                    && d.severity == DiagnosticSeverity::Warning)
        );
    }

    #[test]
    fn too_many_related_terms_are_diagnosed_and_truncated() {
        let values = (0..MAX_RELATED + 2)
            .map(|i| format!("term{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let result = parse(&format!("about: rust related: {values}"));
        assert_eq!(result.draft.related.len(), MAX_RELATED);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == KnowledgeDslDiagnosticCode::TooManyValues)
        );
    }

    #[test]
    fn oversized_values_and_inputs_are_rejected_with_diagnostics() {
        let result = parse(&format!(r#"about: "{}""#, "x".repeat(MAX_TEXT_BYTES + 1)));
        assert!(result.draft.about.is_empty());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == KnowledgeDslDiagnosticCode::ValueTooLong)
        );

        let result = parse(&"x".repeat(MAX_INPUT_BYTES + 1));
        assert!(result.draft.about.is_empty());
        assert_eq!(
            result.diagnostics[0].code,
            KnowledgeDslDiagnosticCode::ValueTooLong
        );

        let implicit = parse(&format!(
            "{} need: definition",
            "x".repeat(MAX_TEXT_BYTES + 1)
        ));
        assert!(implicit.draft.about.is_empty());
        assert!(
            implicit
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == KnowledgeDslDiagnosticCode::ValueTooLong)
        );
    }

    #[test]
    fn unicode_values_are_locale_safe() {
        let result = parse(
            r#"about: "Über Rüst-Programmierung: für Fortgeschrittene, wirklich" need: definition"#,
        );
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            about(&result.draft),
            vec!["Über Rüst-Programmierung: für Fortgeschrittene, wirklich"]
        );
    }

    #[test]
    fn unicode_subject_without_quotes_round_trips() {
        let result = parse("about: 日本語のプログラミング need: overview");
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(about(&result.draft), vec!["日本語のプログラミング"]);
    }

    // --- Natural-language rules --------------------------------------------

    #[test]
    fn recognizes_find_form() {
        let result = parse("find rust async runtimes");
        assert_eq!(result.confidence, KnowledgeParseConfidence::Deterministic);
        assert_eq!(about(&result.draft), vec!["rust async runtimes"]);
        assert!(result.draft.needs.is_empty());
        assert!(result.draft.action.is_none());
    }

    #[test]
    fn recognizes_definition_form() {
        let result = parse("What is a borrow checker?");
        assert_eq!(result.confidence, KnowledgeParseConfidence::Deterministic);
        assert_eq!(about(&result.draft), vec!["a borrow checker"]);
        assert_eq!(result.draft.needs, vec![KnowledgeNeed::Definition]);
    }

    #[test]
    fn recognizes_procedure_form() {
        let result = parse("How do I set up a Rust workspace?");
        assert_eq!(result.confidence, KnowledgeParseConfidence::Deterministic);
        assert_eq!(about(&result.draft), vec!["set up a Rust workspace"]);
        assert_eq!(
            result.draft.needs,
            vec![KnowledgeNeed::Procedure, KnowledgeNeed::Examples]
        );
        assert_eq!(result.draft.action, Some(KnowledgeAction::Apply));
    }

    #[test]
    fn recognizes_examples_form() {
        let result = parse("examples of async trait usage");
        assert_eq!(result.confidence, KnowledgeParseConfidence::Deterministic);
        assert_eq!(about(&result.draft), vec!["async trait usage"]);
        assert_eq!(result.draft.needs, vec![KnowledgeNeed::Examples]);
    }

    #[test]
    fn recognizes_limitations_form() {
        let result = parse("limitations of async Rust");
        assert_eq!(about(&result.draft), vec!["async Rust"]);
        assert_eq!(result.draft.needs, vec![KnowledgeNeed::Limitations]);
    }

    #[test]
    fn recognizes_evidence_form() {
        let result = parse("evidence for zero-cost abstractions");
        assert_eq!(about(&result.draft), vec!["zero-cost abstractions"]);
        assert_eq!(result.draft.needs, vec![KnowledgeNeed::Evidence]);
    }

    #[test]
    fn recognizes_apply_use_form() {
        let result = parse("how to use tokio channels");
        assert_eq!(about(&result.draft), vec!["tokio channels"]);
        assert_eq!(
            result.draft.needs,
            vec![KnowledgeNeed::Procedure, KnowledgeNeed::Examples]
        );
        assert_eq!(result.draft.action, Some(KnowledgeAction::Apply));
    }

    #[test]
    fn bare_apply_and_use_forms_separate_application_context() {
        for input in [
            "apply graph matching to route planning",
            "use graph matching for route planning",
        ] {
            let result = parse(input);
            assert_eq!(about(&result.draft), vec!["graph matching"]);
            assert_eq!(result.draft.context.as_deref(), Some("route planning"));
            assert_eq!(result.draft.action, Some(KnowledgeAction::Apply));
            assert_eq!(result.confidence, KnowledgeParseConfidence::Ambiguous);
            assert_eq!(result.ambiguities.len(), 1);
        }
    }

    #[test]
    fn how_to_use_keeps_answer_context_out_of_the_subject() {
        let result = parse("how can I use Tokio channels to build a scheduler?");
        assert_eq!(about(&result.draft), vec!["Tokio channels"]);
        assert_eq!(result.draft.context.as_deref(), Some("build a scheduler"));
        assert_eq!(result.draft.action, Some(KnowledgeAction::Apply));
        assert_eq!(result.confidence, KnowledgeParseConfidence::Ambiguous);
    }

    #[test]
    fn recognizes_analyse_form() {
        let result = parse("analyse the tradeoffs of async runtimes");
        assert_eq!(
            about(&result.draft),
            vec!["the tradeoffs of async runtimes"]
        );
        assert_eq!(
            result.draft.needs,
            vec![
                KnowledgeNeed::Evidence,
                KnowledgeNeed::Arguments,
                KnowledgeNeed::Limitations
            ]
        );
        assert_eq!(result.draft.action, Some(KnowledgeAction::Evaluate));
    }

    #[test]
    fn recognizes_comparison_form_with_compare_keyword() {
        let result = parse("compare tokio and async-std");
        assert_eq!(about(&result.draft), vec!["tokio", "async-std"]);
        assert_eq!(result.draft.needs, vec![KnowledgeNeed::Comparison]);
        assert_eq!(result.draft.action, Some(KnowledgeAction::Compare));
        assert_eq!(result.confidence, KnowledgeParseConfidence::Deterministic);
    }

    #[test]
    fn recognizes_comparison_form_with_vs_keyword() {
        let result = parse("tokio vs async-std");
        assert_eq!(about(&result.draft), vec!["tokio", "async-std"]);
        assert_eq!(result.draft.needs, vec![KnowledgeNeed::Comparison]);
    }

    #[test]
    fn recognizes_difference_between_form() {
        let result = parse("difference between tokio and async-std");
        assert_eq!(about(&result.draft), vec!["tokio", "async-std"]);
        assert_eq!(result.draft.needs, vec![KnowledgeNeed::Comparison]);
    }

    #[test]
    fn ambiguous_how_to_compare_records_alternative_without_discarding_it() {
        let result = parse("how to compare tokio and async-std");
        assert_eq!(result.confidence, KnowledgeParseConfidence::Ambiguous);
        assert_eq!(result.draft.needs, vec![KnowledgeNeed::Comparison]);
        assert_eq!(result.draft.action, Some(KnowledgeAction::Compare));
        assert_eq!(result.ambiguities.len(), 1);
        assert_eq!(
            result.ambiguities[0].alternative_need,
            Some(KnowledgeNeed::Procedure)
        );
        assert_eq!(
            result.ambiguities[0].alternative_action,
            Some(KnowledgeAction::Apply)
        );
    }

    #[test]
    fn plain_subject_without_any_trigger_falls_back_to_about_only() {
        let result = parse("quantum entanglement");
        assert_eq!(result.confidence, KnowledgeParseConfidence::Deterministic);
        assert_eq!(about(&result.draft), vec!["quantum entanglement"]);
        assert!(result.draft.needs.is_empty());
        assert!(result.draft.action.is_none());
    }

    #[test]
    fn natural_language_keeps_inferred_context_out_of_retrieval_fields() {
        let result = parse("how to use tokio channels for a production trading system");
        assert_eq!(about(&result.draft), vec!["tokio channels"]);
        assert_eq!(
            result.draft.context.as_deref(),
            Some("a production trading system")
        );
        assert!(result.draft.constraints.is_empty());
        assert!(result.draft.format.is_none());
        assert!(
            result
                .draft
                .about
                .iter()
                .all(|subject| !subject.contains("production trading system"))
        );
    }

    // --- Formatting and round-trips ----------------------------------------

    fn sample_draft() -> KnowledgeQueryDraft {
        KnowledgeQueryDraft {
            about: vec!["rust async runtimes".into(), "say \"hi\", ok".into()],
            needs: vec![KnowledgeNeed::Definition, KnowledgeNeed::Procedure],
            related: vec!["tokio".into()],
            scopes: vec![
                KnowledgeScopeSelector::WholeLibrary,
                KnowledgeScopeSelector::Workspace {
                    workspace_id: "w1".into(),
                },
            ],
            action: Some(KnowledgeAction::Apply),
            context: Some("write a scheduler, carefully".into()),
            constraints: vec!["no unsafe".into()],
            format: Some(KnowledgeOutputFormat::Steps),
            depth: Some(KnowledgeAnswerDepth::Detailed),
        }
    }

    #[test]
    fn multiline_format_round_trips_through_parse() {
        let draft = sample_draft();
        let text = format_multiline(&draft);
        let reparsed = parse(&text);
        assert!(
            reparsed.diagnostics.is_empty(),
            "{:?}\n---\n{text}",
            reparsed.diagnostics
        );
        assert_eq!(reparsed.draft, draft);
    }

    #[test]
    fn compact_format_round_trips_through_parse() {
        let draft = sample_draft();
        let text = format_compact(&draft);
        let reparsed = parse(&text);
        assert!(
            reparsed.diagnostics.is_empty(),
            "{:?}\n---\n{text}",
            reparsed.diagnostics
        );
        assert_eq!(reparsed.draft, draft);
    }

    #[test]
    fn empty_draft_formats_to_empty_text() {
        assert_eq!(format_multiline(&KnowledgeQueryDraft::default()), "");
        assert_eq!(format_compact(&KnowledgeQueryDraft::default()), "");
    }

    #[test]
    fn formatter_always_quotes_values_containing_colons() {
        let draft = draft_with_about(vec!["rust: the async story".into()], Vec::new(), None);
        let text = format_multiline(&draft);
        assert_eq!(text, "about: \"rust: the async story\"");
    }

    #[test]
    fn formatter_round_trips_backslashes_and_answer_depth() {
        let draft = KnowledgeQueryDraft {
            about: vec![r"\\server\share".into()],
            depth: Some(KnowledgeAnswerDepth::Detailed),
            ..KnowledgeQueryDraft::default()
        };

        let formatted = format_compact(&draft);
        let reparsed = parse(&formatted);

        assert!(
            reparsed.diagnostics.is_empty(),
            "{:?}",
            reparsed.diagnostics
        );
        assert_eq!(reparsed.draft, draft);
    }

    #[test]
    fn formatter_output_for_maximum_sized_subjects_remains_parseable() {
        let draft = KnowledgeQueryDraft {
            about: vec!["\\".repeat(MAX_TEXT_BYTES); MAX_ABOUT],
            ..KnowledgeQueryDraft::default()
        };

        let formatted = format_compact(&draft);
        let reparsed = parse(&formatted);

        assert!(formatted.len() < MAX_INPUT_BYTES);
        assert!(
            reparsed.diagnostics.is_empty(),
            "{:?}",
            reparsed.diagnostics
        );
        assert_eq!(reparsed.draft, draft);
    }

    // --- Canonical request conversion --------------------------------------

    #[test]
    fn draft_converts_to_search_request_with_explicit_scope_context() {
        let draft = KnowledgeQueryDraft {
            about: vec!["rust".into()],
            needs: vec![KnowledgeNeed::Definition],
            related: vec!["tokio".into()],
            scopes: vec![KnowledgeScopeSelector::WholeLibrary],
            ..KnowledgeQueryDraft::default()
        };
        let request = draft
            .to_search_request(
                &scope_context(),
                RetrievalMode::Hybrid,
                KnowledgeSearchOptions::default(),
            )
            .expect("valid search request");
        assert_eq!(
            request.subjects,
            vec![KnowledgeSubject {
                text: "rust".into()
            }]
        );
        assert_eq!(request.needs, vec![KnowledgeNeed::Definition]);
        assert_eq!(request.related_terms, vec!["tokio".to_owned()]);
        assert_eq!(
            request.scopes,
            vec![KnowledgeScope {
                tenant_id: "tenant-a".into(),
                library_id: "library-a".into(),
                selector: KnowledgeScopeSelector::WholeLibrary,
            }]
        );
        assert!(request.validate().is_ok());
    }

    #[test]
    fn draft_with_no_answer_fields_produces_no_answer_request() {
        let draft = KnowledgeQueryDraft {
            about: vec!["rust".into()],
            scopes: vec![KnowledgeScopeSelector::WholeLibrary],
            ..KnowledgeQueryDraft::default()
        };
        assert!(
            draft
                .to_answer_request("sha256:evidence")
                .expect("valid empty answer")
                .is_none()
        );
    }

    #[test]
    fn draft_with_answer_fields_produces_answer_request_carrying_evidence_fingerprint() {
        let draft = KnowledgeQueryDraft {
            about: vec!["rust".into()],
            scopes: vec![KnowledgeScopeSelector::WholeLibrary],
            action: Some(KnowledgeAction::Explain),
            context: Some("onboarding a new hire".into()),
            constraints: vec!["keep it short".into()],
            format: Some(KnowledgeOutputFormat::Bullets),
            ..KnowledgeQueryDraft::default()
        };
        let answer = draft
            .to_answer_request("sha256:evidence")
            .expect("valid answer")
            .expect("expected an answer request");
        assert_eq!(answer.evidence_fingerprint, "sha256:evidence");
        assert_eq!(answer.action, Some(KnowledgeAction::Explain));
        assert_eq!(answer.context.as_deref(), Some("onboarding a new hire"));
        assert_eq!(answer.constraints, vec!["keep it short".to_owned()]);
        assert_eq!(answer.output, Some(KnowledgeOutputFormat::Bullets));
        assert_eq!(answer.depth, None);
        assert!(answer.validate().is_ok());
    }

    #[test]
    fn draft_round_trips_through_canonical_requests() {
        let draft = KnowledgeQueryDraft {
            about: vec!["rust".into(), "async".into()],
            needs: vec![KnowledgeNeed::Definition, KnowledgeNeed::Procedure],
            related: vec!["tokio".into()],
            scopes: vec![
                KnowledgeScopeSelector::WholeLibrary,
                KnowledgeScopeSelector::Root {
                    root_id: "root-1".into(),
                },
            ],
            action: Some(KnowledgeAction::Apply),
            context: Some("write a scheduler".into()),
            constraints: vec!["no unsafe".into()],
            format: Some(KnowledgeOutputFormat::Steps),
            depth: Some(KnowledgeAnswerDepth::Detailed),
        };
        let search = draft
            .to_search_request(
                &scope_context(),
                RetrievalMode::Hybrid,
                KnowledgeSearchOptions::default(),
            )
            .expect("valid search");
        let answer = draft
            .to_answer_request("sha256:evidence")
            .expect("valid answer");
        let rebuilt = KnowledgeQueryDraft::from_requests(&search, answer.as_ref());
        assert_eq!(rebuilt, draft);
    }

    #[test]
    fn answer_only_fields_never_appear_in_search_request_text() {
        // Contamination regression: `to`/`do`/`constraint` text must never
        // leak into any retrieval-bound string.
        let draft = KnowledgeQueryDraft {
            about: vec!["database indexing".into()],
            scopes: vec![KnowledgeScopeSelector::WholeLibrary],
            action: Some(KnowledgeAction::Apply),
            context: Some("unrelated-marker-xyz".into()),
            constraints: vec!["another-marker-abc".into()],
            format: Some(KnowledgeOutputFormat::Table),
            ..KnowledgeQueryDraft::default()
        };
        let request = draft
            .to_search_request(
                &scope_context(),
                RetrievalMode::Hybrid,
                KnowledgeSearchOptions::default(),
            )
            .expect("valid search");
        let haystack = request
            .subjects
            .iter()
            .map(|s| s.text.as_str())
            .chain(request.related_terms.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!haystack.contains("unrelated-marker-xyz"));
        assert!(!haystack.contains("another-marker-abc"));
    }

    #[test]
    fn dsl_do_and_to_never_populate_search_fields() {
        let result = parse(
            r#"about: database indexing do: apply to: "unrelated-marker-xyz" constraint: "another-marker-abc""#,
        );
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let haystack = result.draft.about.join(" ") + " " + &result.draft.related.join(" ");
        assert!(!haystack.contains("unrelated-marker-xyz"));
        assert!(!haystack.contains("another-marker-abc"));
    }
}
