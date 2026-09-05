//! Provider-neutral structured search queries (task 0162).
#![allow(missing_docs)]

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{GitFileStatus, Location};

/// Current durable query schema.
pub const SEARCH_QUERY_SCHEMA_VERSION: u32 = 2;

/// Explicit interpretation of the user's primary query text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SearchMode {
    /// Literal or glob filename matching.
    #[default]
    Name,
    /// Bounded literal or regular-expression content matching.
    Content,
    /// Dense-vector retrieval using the enrolled library's local model.
    Semantic,
}

/// User-visible semantic scope. Search never changes enrolment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SemanticSearchScope {
    /// Search recursively beneath the current folder.
    #[default]
    CurrentFolder,
    /// Search every ready document in the selected library.
    EntireLibrary,
    /// Search only the explicitly selected enrolled roots.
    EnrolledRoots,
}

/// Durable semantic predicate. Query vectors are deliberately not persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSemanticPredicate {
    pub query: String,
    pub library_id: String,
    pub scope: SemanticSearchScope,
    #[serde(default)]
    pub enrolled_root_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchNameMode {
    Substring,
    Glob,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchEntryKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchNamePredicate {
    pub pattern: String,
    pub mode: SearchNameMode,
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchContentPredicate {
    pub query: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchScope {
    pub locations: Vec<Location>,
    pub recurse: bool,
    pub show_hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub schema_version: u32,
    #[serde(default)]
    pub mode: SearchMode,
    pub scope: SearchScope,
    pub name: Option<SearchNamePredicate>,
    pub entry_kinds: Vec<SearchEntryKind>,
    pub mime_types: Vec<String>,
    pub min_size_bytes: Option<u64>,
    pub max_size_bytes: Option<u64>,
    pub modified_after: Option<DateTime<Utc>>,
    pub modified_before: Option<DateTime<Utc>>,
    pub content: Option<SearchContentPredicate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<SearchSemanticPredicate>,
    pub git_statuses: Vec<GitFileStatus>,
    pub tags: Vec<String>,
    pub metadata: BTreeMap<String, String>,
}

/// A durable smart folder definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSearch {
    pub id: Uuid,
    pub name: String,
    pub pinned: bool,
    pub query: SearchQuery,
}
