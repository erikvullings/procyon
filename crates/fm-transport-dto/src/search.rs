//! Wire types for starting and cancelling a recursive filesystem search
//! (spec §24, task 0068/0089).
#![allow(missing_docs)]

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::location::LocationDto;

/// Current structured search-query schema.
pub const SEARCH_QUERY_SCHEMA_VERSION: u32 = 1;

/// How a filename pattern is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SearchNameModeDto {
    /// Case-aware substring matching.
    Substring,
    /// `*` and `?` wildcard matching.
    Glob,
}

/// Entry kinds accepted by a structured query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SearchEntryKindDto {
    File,
    Directory,
    Symlink,
}

/// Git working-tree states accepted by a structured query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SearchGitStatusDto {
    Clean,
    Modified,
    Staged,
    Untracked,
    Ignored,
}

/// Predicates a provider may be unable to evaluate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SearchPredicateKindDto {
    Name,
    EntryKind,
    MimeType,
    Size,
    ModifiedTime,
    Content,
    GitStatus,
    Tags,
    Metadata,
}

/// A filename predicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchNamePredicateDto {
    pub pattern: String,
    pub mode: SearchNameModeDto,
    pub case_sensitive: bool,
}

/// A bounded content predicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchContentPredicateDto {
    pub query: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

/// Provider-neutral scope for a structured search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchScopeDto {
    pub locations: Vec<LocationDto>,
    pub recurse: bool,
    pub show_hidden: bool,
}

/// Structured, versioned search query suitable for durable persistence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchQueryDto {
    pub schema_version: u32,
    pub scope: SearchScopeDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<SearchNamePredicateDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_kinds: Vec<SearchEntryKindDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mime_types: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_after: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_before: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<SearchContentPredicateDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git_statuses: Vec<SearchGitStatusDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

/// Starts a new recursive, cancellable search (filename and/or content)
/// (`POST /api/v1/search`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "workspaceId": "7136d9bc-90f1-4c67-8527-9d30683167ec",
    "roots": [{"providerId": "local", "uri": "file:///Users/erik/Documents"}],
    "query": "report*.pdf",
    "contentQuery": "TODO",
    "recurse": true
}))]
pub struct StartSearchRequestDto {
    /// Workspace that owns the search and receives its result-batch events.
    pub workspace_id: Uuid,
    /// One or more roots to search.
    pub roots: Vec<LocationDto>,
    /// Filename query. Matched as a case-insensitive substring unless it
    /// contains `*` or `?`, in which case it is treated as a glob pattern.
    /// Empty string means "match all filenames".
    pub query: String,
    /// Optional content-search query. When present, files that pass the
    /// filename filter (or all files if `query` is empty) are scanned for
    /// this content pattern (task 0089).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_query: Option<String>,
    /// Treat `content_query` as a regular expression. Defaults to `false`.
    #[serde(default)]
    pub content_regex: bool,
    /// Make the content search case-sensitive. Defaults to `false` (case-insensitive).
    #[serde(default)]
    pub content_case_sensitive: bool,
    /// Only match `content_query` at word boundaries. Defaults to `false`.
    #[serde(default)]
    pub content_whole_word: bool,
    /// Recurse into subdirectories. Defaults to `true`. When `false`, only
    /// the root directories' immediate children are scanned.
    #[serde(default = "default_recurse")]
    pub recurse: bool,
    /// Search hidden files/directories (dotfiles, and anything ignored by
    /// `.gitignore`/`.ignore`). Defaults to `true` for back-compat with
    /// callers that don't set it; the frontend sends the pane's current
    /// "show hidden files" setting explicitly.
    #[serde(default = "default_show_hidden")]
    pub show_hidden: bool,
    /// Preferred versioned query representation. Legacy fields above remain
    /// accepted for transport compatibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_query: Option<SearchQueryDto>,
}

/// One provider's explicit structured-query limitations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchProviderLimitationDto {
    pub provider_id: String,
    pub unevaluated_predicates: Vec<SearchPredicateKindDto>,
}

/// How a search result set is being populated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SearchExecutionModeDto {
    /// Results are supplied exclusively by a native local index.
    Indexed,
    /// Results are supplied by live recursive traversal.
    LiveRecursive,
    /// Different roots, or an index fallback, use both paths.
    Mixed,
}

fn default_recurse() -> bool {
    true
}

fn default_show_hidden() -> bool {
    true
}

/// Identifies a started search and its virtual result location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "searchId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b",
    "location": {
        "providerId": "search",
        "uri": "search://local/5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b"
    }
}))]
pub struct StartSearchResponseDto {
    /// The started search's identifier, used to cancel it.
    pub search_id: Uuid,
    /// The virtual `search://local/{searchId}` location that lists this
    /// search's streamed results; opening it in a pane renders results
    /// through the existing directory table unchanged (spec §24).
    pub location: LocationDto,
    /// Predicates deliberately not evaluated for one or more providers.
    pub limitations: Vec<SearchProviderLimitationDto>,
    /// Planned execution path for this search. Result-batch events report the
    /// final mode if a native index becomes unavailable and falls back.
    pub execution_mode: SearchExecutionModeDto,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_location() -> LocationDto {
        LocationDto {
            provider_id: "local".to_owned(),
            uri: "file:///Users/erik".to_owned(),
        }
    }

    #[test]
    fn start_search_request_round_trips_and_uses_camel_case_field_names() {
        let request = StartSearchRequestDto {
            workspace_id: Uuid::new_v4(),
            roots: vec![sample_location()],
            query: "report*.pdf".to_owned(),
            content_query: None,
            content_regex: false,
            content_case_sensitive: false,
            content_whole_word: false,
            recurse: true,
            show_hidden: true,
            structured_query: None,
        };
        let json = serde_json::to_string(&request).expect("serialization must succeed");
        assert!(json.contains("\"workspaceId\""));
        assert!(json.contains("\"roots\""));
        assert!(json.contains("\"query\""));
        let parsed: StartSearchRequestDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(request, parsed);
    }

    #[test]
    fn start_search_request_with_content_query_round_trips() {
        let request = StartSearchRequestDto {
            workspace_id: Uuid::new_v4(),
            roots: vec![sample_location()],
            query: String::new(),
            content_query: Some("TODO".to_owned()),
            content_regex: true,
            content_case_sensitive: false,
            content_whole_word: true,
            recurse: false,
            show_hidden: false,
            structured_query: None,
        };
        let json = serde_json::to_string(&request).expect("serialization must succeed");
        assert!(json.contains("\"contentQuery\""));
        assert!(json.contains("\"contentRegex\""));
        assert!(json.contains("\"recurse\""));
        let parsed: StartSearchRequestDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(request, parsed);
    }

    #[test]
    fn start_search_request_defaults_are_correct_for_back_compat() {
        let json =
            r#"{"workspaceId":"00000000-0000-0000-0000-000000000000","roots":[],"query":"x"}"#;
        let parsed: StartSearchRequestDto =
            serde_json::from_str(json).expect("defaults must fill in missing fields");
        assert_eq!(parsed.query, "x");
        assert!(parsed.content_query.is_none());
        assert!(!parsed.content_regex);
        assert!(!parsed.content_case_sensitive);
        assert!(!parsed.content_whole_word);
        assert!(parsed.recurse, "recurse defaults to true");
        assert!(parsed.show_hidden, "show_hidden defaults to true");
        assert!(parsed.structured_query.is_none());
    }

    #[test]
    fn start_search_response_round_trips_and_uses_camel_case_field_names() {
        let response = StartSearchResponseDto {
            search_id: Uuid::new_v4(),
            location: LocationDto {
                provider_id: "search".to_owned(),
                uri: "search://local/5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b".to_owned(),
            },
            limitations: Vec::new(),
            execution_mode: SearchExecutionModeDto::LiveRecursive,
        };
        let json = serde_json::to_string(&response).expect("serialization must succeed");
        assert!(json.contains("\"searchId\""));
        assert!(json.contains("\"location\""));
        let parsed: StartSearchResponseDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(response, parsed);
    }

    #[test]
    fn structured_query_round_trips_combined_predicates() {
        let query = SearchQueryDto {
            schema_version: SEARCH_QUERY_SCHEMA_VERSION,
            scope: SearchScopeDto {
                locations: vec![sample_location()],
                recurse: true,
                show_hidden: false,
            },
            name: Some(SearchNamePredicateDto {
                pattern: "*.mp4".to_owned(),
                mode: SearchNameModeDto::Glob,
                case_sensitive: false,
            }),
            entry_kinds: vec![SearchEntryKindDto::File],
            mime_types: vec!["video/*".to_owned()],
            min_size_bytes: Some(1_000_000),
            max_size_bytes: None,
            modified_after: Some("2026-08-01T00:00:00Z".parse().unwrap()),
            modified_before: None,
            content: None,
            git_statuses: vec![SearchGitStatusDto::Modified, SearchGitStatusDto::Untracked],
            tags: vec!["review".to_owned()],
            metadata: [("project".to_owned(), "procyon".to_owned())].into(),
        };

        let json = serde_json::to_string(&query).expect("serialize structured query");
        let parsed: SearchQueryDto =
            serde_json::from_str(&json).expect("deserialize structured query");

        assert_eq!(parsed, query);
    }
}
