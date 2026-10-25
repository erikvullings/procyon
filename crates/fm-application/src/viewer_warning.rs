//! Caveats shown beside the structured and SQLite viewers: a stable code the frontend
//! localises, the English text as fallback, and optional raw diagnostic detail.

/// A caveat about what a viewer can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ViewerWarning {
    /// Stable code from [`codes`].
    pub code: &'static str,
    /// English fallback text.
    pub text: String,
    /// Raw diagnostic text the localised sentence appends, when there is one.
    pub detail: Option<String>,
}

impl ViewerWarning {
    pub(crate) fn new(code: &'static str, text: impl Into<String>) -> Self {
        Self {
            code,
            text: text.into(),
            detail: None,
        }
    }

    pub(crate) fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// Codes of [`ViewerWarning`]. Every value must have a translation in each frontend locale; a
/// frontend test reads this file to enforce that.
pub(crate) mod codes {
    /// A write-ahead log sits next to the database; uncheckpointed writes are not shown.
    pub(crate) const WAL_NOT_SHOWN: &str = "walNotShown";
    /// The provider is sequential-only, so arbitrary jumps are disabled.
    pub(crate) const SEQUENTIAL_ONLY: &str = "sequentialOnly";
    /// The workbook cannot be shown in the app (`detail` says why).
    pub(crate) const SPREADSHEET_FALLBACK: &str = "spreadsheetFallback";
    /// Indexing stopped early (`detail` says why).
    pub(crate) const INDEXING_FAILED: &str = "indexingFailed";
}
