//! Read-only SQLite database viewer DTOs shared by REST and Tauri (task 0251).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::location::LocationDto;

/// Opens a read-only SQLite viewer session on a database file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenSqliteViewRequestDto {
    /// Provider-neutral database location.
    pub location: LocationDto,
}

/// Whether a schema object is a table or a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SqliteObjectKindDto {
    /// A base table (including virtual tables).
    Table,
    /// A view.
    View,
}

/// One column of a table or view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SqliteColumnDto {
    /// Column name.
    pub name: String,
    /// Declared type text, empty when the column is untyped.
    pub declared_type: String,
    /// Whether the column is declared `NOT NULL`.
    pub not_null: bool,
    /// Whether the column is part of the primary key.
    pub primary_key: bool,
}

/// One table or view in the database schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SqliteObjectDto {
    /// Object name.
    pub name: String,
    /// Table or view.
    pub kind: SqliteObjectKindDto,
    /// Columns in declaration order.
    pub columns: Vec<SqliteColumnDto>,
    /// The original `CREATE` statement, when SQLite stores one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_sql: Option<String>,
}

/// An opened viewer session and the database schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenSqliteViewResponseDto {
    /// Session handle for later queries.
    pub session_id: Uuid,
    /// Database file size in bytes.
    pub source_bytes: u64,
    /// SQLite library version used to read the file.
    pub sqlite_version: String,
    /// Tables and views, sorted by name; internal `sqlite_*` tables are omitted.
    pub objects: Vec<SqliteObjectDto>,
    /// A user-facing caveat, e.g. a WAL file whose recent writes are not shown (English fallback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Stable code of `warning` that clients localise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning_code: Option<String>,
}

/// Request addressing an existing viewer session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SqliteViewSessionRequestDto {
    /// Session handle from [`OpenSqliteViewResponseDto`].
    pub session_id: Uuid,
}

/// Comparison applied by a per-column filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SqliteFilterOperatorDto {
    /// Case-insensitive substring match.
    Contains,
    /// Exact match.
    Equals,
    /// Not equal.
    NotEquals,
    /// Strictly greater than.
    Greater,
    /// Strictly less than.
    Less,
    /// The value is `NULL`.
    IsNull,
    /// The value is not `NULL`.
    IsNotNull,
}

/// One per-column filter; all filters of a request are combined with `AND`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SqliteColumnFilterDto {
    /// Column to filter on; must exist in the browsed object.
    pub column: String,
    /// Comparison to apply.
    pub operator: SqliteFilterOperatorDto,
    /// Comparison operand; ignored for `isNull` and `isNotNull`.
    #[serde(default)]
    pub value: String,
}

/// Sort order for a browsed table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SqliteSortDto {
    /// Column to sort by; must exist in the browsed object.
    pub column: String,
    /// Sort descending instead of ascending.
    #[serde(default)]
    pub descending: bool,
}

/// What a query reads: a stored table/view with filters, or a user SQL statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SqliteQuerySourceDto {
    /// Browse a table or view.
    #[serde(rename_all = "camelCase")]
    Browse {
        /// Table or view name.
        object: String,
        /// Free-text search across every column (case-insensitive substring).
        #[serde(default)]
        search: String,
        /// Per-column filters.
        #[serde(default)]
        filters: Vec<SqliteColumnFilterDto>,
        /// Optional sort.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sort: Option<SqliteSortDto>,
    },
    /// Run one read-only SQL statement.
    #[serde(rename_all = "camelCase")]
    Sql {
        /// A single `SELECT`, `WITH`, `EXPLAIN` or read-only `PRAGMA` statement.
        sql: String,
    },
}

/// Reads one bounded page from a viewer session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuerySqliteViewRequestDto {
    /// Session handle.
    pub session_id: Uuid,
    /// What to read.
    pub source: SqliteQuerySourceDto,
    /// Zero-based index of the first row of the page.
    #[serde(default)]
    pub offset: u64,
    /// Maximum rows in the page (capped by the server).
    pub limit: u16,
}

/// SQLite storage class of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SqliteValueTypeDto {
    /// SQL `NULL`, distinct from an empty string.
    Null,
    /// 64-bit integer, sent as decimal text to avoid precision loss.
    Integer,
    /// Floating-point number.
    Real,
    /// Text, possibly truncated.
    Text,
    /// Binary data; `text` holds a hex preview of the first bytes.
    Blob,
}

/// One cell of a result row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SqliteCellDto {
    /// Storage class.
    pub value_type: SqliteValueTypeDto,
    /// Display text; absent for `null`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Full size in bytes of a truncated text or blob value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_length: Option<u64>,
    /// Whether `text` omits part of the stored value.
    #[serde(default)]
    pub truncated: bool,
}

/// One bounded result page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuerySqliteViewResponseDto {
    /// Result column names.
    pub columns: Vec<String>,
    /// Rows of this page.
    pub rows: Vec<Vec<SqliteCellDto>>,
    /// Zero-based index of the first returned row.
    pub offset: u64,
    /// Whether more rows exist after this page.
    pub has_more: bool,
    /// Total matching rows for a browse query, when counting finished in time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_rows: Option<u64>,
    /// Whether the page was cut short by the response size budget.
    pub truncated: bool,
    /// Wall-clock query time in milliseconds.
    pub elapsed_ms: u64,
}
