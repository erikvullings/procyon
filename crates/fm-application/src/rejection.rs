//! Stable reasons attached to `invalidRequest` rejections.
//!
//! A rejection carries an English fallback message, a stable [`reason`] code and the structured
//! parameters the client interpolates into the localised sentence. Clients fall back to the
//! English message for reasons they do not know. Every value must have a translation in each
//! frontend locale; a frontend test reads this file to enforce that, so declare each reason as a
//! plain `pub const` string inside [`reason`].

use std::collections::BTreeMap;

/// A request rejected for a reason the user can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    /// Stable code from [`reason`].
    pub reason: String,
    /// English fallback text.
    pub message: String,
    /// Values the localised text interpolates.
    pub params: BTreeMap<String, String>,
}

impl Rejection {
    /// A rejection without parameters.
    #[must_use]
    pub fn new(reason: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            message: message.into(),
            params: BTreeMap::new(),
        }
    }

    /// Adds one structured interpolation value.
    #[must_use]
    pub fn with(mut self, key: &str, value: impl ToString) -> Self {
        self.params.insert(key.to_owned(), value.to_string());
        self
    }
}

impl Rejection {
    /// Reports the rejection as a platform/provider failure (`platformOperationFailed`).
    #[must_use]
    pub fn into_platform(self) -> crate::ApplicationError {
        crate::ApplicationError::PlatformFailed(self)
    }
}

impl From<Rejection> for crate::ApplicationError {
    fn from(rejection: Rejection) -> Self {
        Self::Rejected(rejection)
    }
}

/// Reason codes of [`Rejection`].
pub mod reason {
    /// A workspace failed structural validation.
    pub const WORKSPACE_INVALID: &str = "workspaceInvalid";
    /// The pane does not exist in the workspace.
    pub const PANE_NOT_FOUND: &str = "paneNotFound";
    /// The tab does not exist in the pane.
    pub const TAB_NOT_FOUND: &str = "tabNotFound";
    /// A workspace with this name already exists (`name`).
    pub const WORKSPACE_NAME_EXISTS: &str = "workspaceNameExists";
    /// The workspace is not an ephemeral workspace.
    pub const WORKSPACE_NOT_EPHEMERAL: &str = "workspaceNotEphemeral";
    /// An ephemeral workspace cannot be a resync target.
    pub const WORKSPACE_TARGET_EPHEMERAL: &str = "workspaceTargetEphemeral";
    /// A connection profile failed validation.
    pub const CONNECTION_INVALID: &str = "connectionInvalid";
    /// The credential store failed (`detail`).
    pub const CREDENTIAL_STORE_FAILED: &str = "credentialStoreFailed";
    /// Connecting failed (`detail`).
    pub const CONNECTION_FAILED: &str = "connectionFailed";
    /// An LLM profile failed validation.
    pub const LLM_PROFILE_INVALID: &str = "llmProfileInvalid";
    /// The cloud endpoint needs the user's consent first (`host`).
    pub const CLOUD_CONSENT_REQUIRED: &str = "cloudConsentRequired";
    /// The configured LLM model is unavailable.
    pub const LLM_MODEL_UNAVAILABLE: &str = "llmModelUnavailable";
    /// The LLM request timed out.
    pub const LLM_TIMEOUT: &str = "llmTimeout";
    /// The LLM TLS policy rejected the endpoint.
    pub const LLM_TLS: &str = "llmTls";
    /// The LLM streaming response was malformed.
    pub const LLM_MALFORMED_RESPONSE: &str = "llmMalformedResponse";
    /// The LLM transport failed.
    pub const LLM_TRANSPORT: &str = "llmTransport";
    /// The platform does not support the capability.
    pub const PLATFORM_UNSUPPORTED: &str = "platformUnsupported";
    /// A platform call could not find the path (`path`).
    pub const PATH_NOT_FOUND: &str = "pathNotFound";
    /// A platform call found the destination occupied (`path`).
    pub const PATH_ALREADY_EXISTS: &str = "pathAlreadyExists";
    /// A native platform call failed (`detail`).
    pub const PLATFORM_FAILED: &str = "platformFailed";
    /// Rejection reason `cannotEditRoot` (see the frontend `rejectionReason` catalogue).
    pub const CANNOT_EDIT_ROOT: &str = "cannotEditRoot";
    /// Rejection reason `notRegularFile` (see the frontend `rejectionReason` catalogue).
    pub const NOT_REGULAR_FILE: &str = "notRegularFile";
    /// Rejection reason `editorFileTooLarge` (see the frontend `rejectionReason` catalogue).
    pub const EDITOR_FILE_TOO_LARGE: &str = "editorFileTooLarge";
    /// Rejection reason `editorBinaryFile` (see the frontend `rejectionReason` catalogue).
    pub const EDITOR_BINARY_FILE: &str = "editorBinaryFile";
    /// Rejection reason `editorNotUtf8` (see the frontend `rejectionReason` catalogue).
    pub const EDITOR_NOT_UTF8: &str = "editorNotUtf8";
    /// Rejection reason `editorCannotReplace` (see the frontend `rejectionReason` catalogue).
    pub const EDITOR_CANNOT_REPLACE: &str = "editorCannotReplace";
    /// Rejection reason `editorContentTooLarge` (see the frontend `rejectionReason` catalogue).
    pub const EDITOR_CONTENT_TOO_LARGE: &str = "editorContentTooLarge";
    /// Rejection reason `invalidRangeLength` (see the frontend `rejectionReason` catalogue).
    pub const INVALID_RANGE_LENGTH: &str = "invalidRangeLength";
    /// Rejection reason `tooManyPreviews` (see the frontend `rejectionReason` catalogue).
    pub const TOO_MANY_PREVIEWS: &str = "tooManyPreviews";
    /// Rejection reason `previewMalformed` (see the frontend `rejectionReason` catalogue).
    pub const PREVIEW_MALFORMED: &str = "previewMalformed";
    /// Rejection reason `previewBudget` (see the frontend `rejectionReason` catalogue).
    pub const PREVIEW_BUDGET: &str = "previewBudget";
    /// Rejection reason `previewRenderFailed` (see the frontend `rejectionReason` catalogue).
    pub const PREVIEW_RENDER_FAILED: &str = "previewRenderFailed";
    /// Rejection reason `notSqlite` (see the frontend `rejectionReason` catalogue).
    pub const NOT_SQLITE: &str = "notSqlite";
    /// Rejection reason `remoteDatabaseTooLarge` (see the frontend `rejectionReason` catalogue).
    pub const REMOTE_DATABASE_TOO_LARGE: &str = "remoteDatabaseTooLarge";
    /// Rejection reason `unknownTable` (see the frontend `rejectionReason` catalogue).
    pub const UNKNOWN_TABLE: &str = "unknownTable";
    /// Rejection reason `tooManyFilters` (see the frontend `rejectionReason` catalogue).
    pub const TOO_MANY_FILTERS: &str = "tooManyFilters";
    /// Rejection reason `unknownColumn` (see the frontend `rejectionReason` catalogue).
    pub const UNKNOWN_COLUMN: &str = "unknownColumn";
    /// Rejection reason `sqlEmpty` (see the frontend `rejectionReason` catalogue).
    pub const SQL_EMPTY: &str = "sqlEmpty";
    /// Rejection reason `sqlTooLong` (see the frontend `rejectionReason` catalogue).
    pub const SQL_TOO_LONG: &str = "sqlTooLong";
    /// Rejection reason `sqlReadOnly` (see the frontend `rejectionReason` catalogue).
    pub const SQL_READ_ONLY: &str = "sqlReadOnly";
    /// Rejection reason `sheetRequired` (see the frontend `rejectionReason` catalogue).
    pub const SHEET_REQUIRED: &str = "sheetRequired";
    /// Rejection reason `workbookFallback` (see the frontend `rejectionReason` catalogue).
    pub const WORKBOOK_FALLBACK: &str = "workbookFallback";
    /// Rejection reason `sheetNotFound` (see the frontend `rejectionReason` catalogue).
    pub const SHEET_NOT_FOUND: &str = "sheetNotFound";
    /// Rejection reason `delimiterOptionsOnly` (see the frontend `rejectionReason` catalogue).
    pub const DELIMITER_OPTIONS_ONLY: &str = "delimiterOptionsOnly";
    /// Rejection reason `rowCountRange` (see the frontend `rejectionReason` catalogue).
    pub const ROW_COUNT_RANGE: &str = "rowCountRange";
    /// Rejection reason `noTableRows` (see the frontend `rejectionReason` catalogue).
    pub const NO_TABLE_ROWS: &str = "noTableRows";
    /// Rejection reason `noJsonText` (see the frontend `rejectionReason` catalogue).
    pub const NO_JSON_TEXT: &str = "noJsonText";
    /// Rejection reason `jsonWindowRange` (see the frontend `rejectionReason` catalogue).
    pub const JSON_WINDOW_RANGE: &str = "jsonWindowRange";
    /// Rejection reason `jsonWindowNotIndexed` (see the frontend `rejectionReason` catalogue).
    pub const JSON_WINDOW_NOT_INDEXED: &str = "jsonWindowNotIndexed";
    /// Rejection reason `searchQueryInvalid` (see the frontend `rejectionReason` catalogue).
    pub const SEARCH_QUERY_INVALID: &str = "searchQueryInvalid";
    /// Rejection reason `delimiterInvalid` (see the frontend `rejectionReason` catalogue).
    pub const DELIMITER_INVALID: &str = "delimiterInvalid";
    /// Rejection reason `delimitedMalformed` (see the frontend `rejectionReason` catalogue).
    pub const DELIMITED_MALFORMED: &str = "delimitedMalformed";
    /// Rejection reason `csvMalformedRecord` (see the frontend `rejectionReason` catalogue).
    pub const CSV_MALFORMED_RECORD: &str = "csvMalformedRecord";
    /// Rejection reason `sequentialOnlyRows` (see the frontend `rejectionReason` catalogue).
    pub const SEQUENTIAL_ONLY_ROWS: &str = "sequentialOnlyRows";
    /// Rejection reason `sequentialOnlyJson` (see the frontend `rejectionReason` catalogue).
    pub const SEQUENTIAL_ONLY_JSON: &str = "sequentialOnlyJson";
    /// Rejection reason `selectionSize` (see the frontend `rejectionReason` catalogue).
    pub const SELECTION_SIZE: &str = "selectionSize";
    /// Rejection reason `noAttributeSelected` (see the frontend `rejectionReason` catalogue).
    pub const NO_ATTRIBUTE_SELECTED: &str = "noAttributeSelected";
    /// Rejection reason `permissionBitsRange` (see the frontend `rejectionReason` catalogue).
    pub const PERMISSION_BITS_RANGE: &str = "permissionBitsRange";
    /// Rejection reason `tokensInvalid` (see the frontend `rejectionReason` catalogue).
    pub const TOKENS_INVALID: &str = "tokensInvalid";
    /// Rejection reason `unknownMetadataField` (see the frontend `rejectionReason` catalogue).
    pub const UNKNOWN_METADATA_FIELD: &str = "unknownMetadataField";
    /// Rejection reason `geoUnknownFormat` (see the frontend `rejectionReason` catalogue).
    pub const GEO_UNKNOWN_FORMAT: &str = "geoUnknownFormat";
    /// Rejection reason `geoTextTooLarge` (see the frontend `rejectionReason` catalogue).
    pub const GEO_TEXT_TOO_LARGE: &str = "geoTextTooLarge";
    /// Rejection reason `geoKmzBinary` (see the frontend `rejectionReason` catalogue).
    pub const GEO_KMZ_BINARY: &str = "geoKmzBinary";
    /// Rejection reason `geoNothingToDo` (see the frontend `rejectionReason` catalogue).
    pub const GEO_NOTHING_TO_DO: &str = "geoNothingToDo";
    /// Rejection reason `geoInPlaceFormat` (see the frontend `rejectionReason` catalogue).
    pub const GEO_IN_PLACE_FORMAT: &str = "geoInPlaceFormat";
    /// Rejection reason `fileNeedsFolder` (see the frontend `rejectionReason` catalogue).
    pub const FILE_NEEDS_FOLDER: &str = "fileNeedsFolder";
    /// Rejection reason `partSizeTooSmall` (see the frontend `rejectionReason` catalogue).
    pub const PART_SIZE_TOO_SMALL: &str = "partSizeTooSmall";
    /// Rejection reason `invalidOutputName` (see the frontend `rejectionReason` catalogue).
    pub const INVALID_OUTPUT_NAME: &str = "invalidOutputName";
    /// Rejection reason `selectPartFiles` (see the frontend `rejectionReason` catalogue).
    pub const SELECT_PART_FILES: &str = "selectPartFiles";
    /// Rejection reason `hostKeyChanged` (see the frontend `rejectionReason` catalogue).
    pub const HOST_KEY_CHANGED: &str = "hostKeyChanged";
    /// Rejection reason `knownHostRequired` (see the frontend `rejectionReason` catalogue).
    pub const KNOWN_HOST_REQUIRED: &str = "knownHostRequired";
    /// Rejection reason `notSshConnection` (see the frontend `rejectionReason` catalogue).
    pub const NOT_SSH_CONNECTION: &str = "notSshConnection";
    /// Rejection reason `notOnedriveConnection` (see the frontend `rejectionReason` catalogue).
    pub const NOT_ONEDRIVE_CONNECTION: &str = "notOnedriveConnection";
    /// Rejection reason `authorizationInProgress` (see the frontend `rejectionReason` catalogue).
    pub const AUTHORIZATION_IN_PROGRESS: &str = "authorizationInProgress";
    /// Rejection reason `pluginPermissionDenied` (see the frontend `rejectionReason` catalogue).
    pub const PLUGIN_PERMISSION_DENIED: &str = "pluginPermissionDenied";
    /// Rejection reason `pluginNoPreview` (see the frontend `rejectionReason` catalogue).
    pub const PLUGIN_NO_PREVIEW: &str = "pluginNoPreview";
    /// Rejection reason `pluginPreviewFailed` (see the frontend `rejectionReason` catalogue).
    pub const PLUGIN_PREVIEW_FAILED: &str = "pluginPreviewFailed";
    /// Rejection reason `pluginActionFailed` (see the frontend `rejectionReason` catalogue).
    pub const PLUGIN_ACTION_FAILED: &str = "pluginActionFailed";
    /// Rejection reason `snapshotTooLarge` (see the frontend `rejectionReason` catalogue).
    pub const SNAPSHOT_TOO_LARGE: &str = "snapshotTooLarge";
    /// Rejection reason `snapshotNotUtf8` (see the frontend `rejectionReason` catalogue).
    pub const SNAPSHOT_NOT_UTF8: &str = "snapshotNotUtf8";
    /// Rejection reason `unsupportedQuerySchema` (see the frontend `rejectionReason` catalogue).
    pub const UNSUPPORTED_QUERY_SCHEMA: &str = "unsupportedQuerySchema";
    /// Rejection reason `semanticPredicateRequired` (see the frontend `rejectionReason` catalogue).
    pub const SEMANTIC_PREDICATE_REQUIRED: &str = "semanticPredicateRequired";
    /// Rejection reason `semanticQueryEmpty` (see the frontend `rejectionReason` catalogue).
    pub const SEMANTIC_QUERY_EMPTY: &str = "semanticQueryEmpty";
    /// Rejection reason `semanticExclusive` (see the frontend `rejectionReason` catalogue).
    pub const SEMANTIC_EXCLUSIVE: &str = "semanticExclusive";
    /// Rejection reason `conceptPredicateRequired` (see the frontend `rejectionReason` catalogue).
    pub const CONCEPT_PREDICATE_REQUIRED: &str = "conceptPredicateRequired";
    /// Rejection reason `continuationOutOfRange` (see the frontend `rejectionReason` catalogue).
    pub const CONTINUATION_OUT_OF_RANGE: &str = "continuationOutOfRange";
    /// Rejection reason `entryIdentityChanged` (see the frontend `rejectionReason` catalogue).
    pub const ENTRY_IDENTITY_CHANGED: &str = "entryIdentityChanged";
    /// Rejection reason `entryMetadataChanged` (see the frontend `rejectionReason` catalogue).
    pub const ENTRY_METADATA_CHANGED: &str = "entryMetadataChanged";
    /// Rejection reason `localLocationRequired` (see the frontend `rejectionReason` catalogue).
    pub const LOCAL_LOCATION_REQUIRED: &str = "localLocationRequired";
    /// Rejection reason `directoryRequired` (see the frontend `rejectionReason` catalogue).
    pub const DIRECTORY_REQUIRED: &str = "directoryRequired";
    /// Rejection reason `scanAlreadyRunning` (see the frontend `rejectionReason` catalogue).
    pub const SCAN_ALREADY_RUNNING: &str = "scanAlreadyRunning";
    /// Rejection reason `basemapNotRaster` (see the frontend `rejectionReason` catalogue).
    pub const BASEMAP_NOT_RASTER: &str = "basemapNotRaster";
    /// Rejection reason `tileZoomRange` (see the frontend `rejectionReason` catalogue).
    pub const TILE_ZOOM_RANGE: &str = "tileZoomRange";
    /// Rejection reason `tileOutsideWorld` (see the frontend `rejectionReason` catalogue).
    pub const TILE_OUTSIDE_WORLD: &str = "tileOutsideWorld";
    /// Rejection reason `rootRequired` (see the frontend `rejectionReason` catalogue).
    pub const ROOT_REQUIRED: &str = "rootRequired";
    /// Rejection reason `rootAndTargetRequired` (see the frontend `rejectionReason` catalogue).
    pub const ROOT_AND_TARGET_REQUIRED: &str = "rootAndTargetRequired";
    /// Rejection reason `localArchiveRequired` (see the frontend `rejectionReason` catalogue).
    pub const LOCAL_ARCHIVE_REQUIRED: &str = "localArchiveRequired";
    /// Rejection reason `diffFailed` (see the frontend `rejectionReason` catalogue).
    pub const DIFF_FAILED: &str = "diffFailed";
    /// Rejection reason `summaryConfirmationStale` (see the frontend `rejectionReason` catalogue).
    pub const SUMMARY_CONFIRMATION_STALE: &str = "summaryConfirmationStale";
    /// Rejection reason `knowledgeScopeTooBroad` (see the frontend `rejectionReason` catalogue).
    pub const KNOWLEDGE_SCOPE_TOO_BROAD: &str = "knowledgeScopeTooBroad";
    /// Rejection reason `knowledgeScopeWorkspace` (see the frontend `rejectionReason` catalogue).
    pub const KNOWLEDGE_SCOPE_WORKSPACE: &str = "knowledgeScopeWorkspace";
    /// Rejection reason `scopeEmpty` (see the frontend `rejectionReason` catalogue).
    pub const SCOPE_EMPTY: &str = "scopeEmpty";
    /// Rejection reason `scopeRootInvalid` (see the frontend `rejectionReason` catalogue).
    pub const SCOPE_ROOT_INVALID: &str = "scopeRootInvalid";
    /// Rejection reason `scopeFolderRequired` (see the frontend `rejectionReason` catalogue).
    pub const SCOPE_FOLDER_REQUIRED: &str = "scopeFolderRequired";
    /// Rejection reason `summaryTokenBudget` (see the frontend `rejectionReason` catalogue).
    pub const SUMMARY_TOKEN_BUDGET: &str = "summaryTokenBudget";
    /// Rejection reason `conversationImmutable` (see the frontend `rejectionReason` catalogue).
    pub const CONVERSATION_IMMUTABLE: &str = "conversationImmutable";
    /// Rejection reason `scopeInvalid` (see the frontend `rejectionReason` catalogue).
    pub const SCOPE_INVALID: &str = "scopeInvalid";
    /// Rejection reason `conceptUriRequired` (see the frontend `rejectionReason` catalogue).
    pub const CONCEPT_URI_REQUIRED: &str = "conceptUriRequired";
    /// Rejection reason `preferredLabelRequired` (see the frontend `rejectionReason` catalogue).
    pub const PREFERRED_LABEL_REQUIRED: &str = "preferredLabelRequired";
    /// Rejection reason `keepNewerUnsupported` (see the frontend `rejectionReason` catalogue).
    pub const KEEP_NEWER_UNSUPPORTED: &str = "keepNewerUnsupported";
    /// Rejection reason `pathBusy` (see the frontend `rejectionReason` catalogue).
    pub const PATH_BUSY: &str = "pathBusy";
    /// Rejection reason `archiveDestinationExtension` (see the frontend `rejectionReason` catalogue).
    pub const ARCHIVE_DESTINATION_EXTENSION: &str = "archiveDestinationExtension";
    /// Rejection reason `archiveFormatMismatch` (see the frontend `rejectionReason` catalogue).
    pub const ARCHIVE_FORMAT_MISMATCH: &str = "archiveFormatMismatch";
    /// Rejection reason `archiveLevelRange` (see the frontend `rejectionReason` catalogue).
    pub const ARCHIVE_LEVEL_RANGE: &str = "archiveLevelRange";
    /// Rejection reason `sevenZipLevelUnsupported` (see the frontend `rejectionReason` catalogue).
    pub const SEVEN_ZIP_LEVEL_UNSUPPORTED: &str = "sevenZipLevelUnsupported";
    /// Rejection reason `cannotDuplicateRoot` (see the frontend `rejectionReason` catalogue).
    pub const CANNOT_DUPLICATE_ROOT: &str = "cannotDuplicateRoot";
    /// Rejection reason `renameAcrossProviders` (see the frontend `rejectionReason` catalogue).
    pub const RENAME_ACROSS_PROVIDERS: &str = "renameAcrossProviders";
    /// Rejection reason `crossProviderUndoCombined` (see the frontend `rejectionReason` catalogue).
    pub const CROSS_PROVIDER_UNDO_COMBINED: &str = "crossProviderUndoCombined";
    /// Rejection reason `providerRootNotRestorable` (see the frontend `rejectionReason` catalogue).
    pub const PROVIDER_ROOT_NOT_RESTORABLE: &str = "providerRootNotRestorable";
    /// Rejection reason `linkProviderMismatch` (see the frontend `rejectionReason` catalogue).
    pub const LINK_PROVIDER_MISMATCH: &str = "linkProviderMismatch";
    /// Rejection reason `signInListenerFailed` (see the frontend `rejectionReason` catalogue).
    pub const SIGN_IN_LISTENER_FAILED: &str = "signInListenerFailed";
    /// Rejection reason `splitMissingParts` (see the frontend `rejectionReason` catalogue).
    pub const SPLIT_MISSING_PARTS: &str = "splitMissingParts";
    /// Rejection reason `splitMissingPartsMore` (see the frontend `rejectionReason` catalogue).
    pub const SPLIT_MISSING_PARTS_MORE: &str = "splitMissingPartsMore";
    /// Rejection reason `splitMixed` (see the frontend `rejectionReason` catalogue).
    pub const SPLIT_MIXED: &str = "splitMixed";
    /// Rejection reason `splitNotAPart` (see the frontend `rejectionReason` catalogue).
    pub const SPLIT_NOT_A_PART: &str = "splitNotAPart";
    /// Rejection reason `splitManifestDamaged` (see the frontend `rejectionReason` catalogue).
    pub const SPLIT_MANIFEST_DAMAGED: &str = "splitManifestDamaged";
    /// Rejection reason `splitNoPartSelected` (see the frontend `rejectionReason` catalogue).
    pub const SPLIT_NO_PART_SELECTED: &str = "splitNoPartSelected";
    /// Rejection reason `splitDifferentFolders` (see the frontend `rejectionReason` catalogue).
    pub const SPLIT_DIFFERENT_FOLDERS: &str = "splitDifferentFolders";
    /// Rejection reason `splitPartNotFile` (see the frontend `rejectionReason` catalogue).
    pub const SPLIT_PART_NOT_FILE: &str = "splitPartNotFile";
    /// Rejection reason `splitManifestCount` (see the frontend `rejectionReason` catalogue).
    pub const SPLIT_MANIFEST_COUNT: &str = "splitManifestCount";
    /// Rejection reason `splitManifestName` (see the frontend `rejectionReason` catalogue).
    pub const SPLIT_MANIFEST_NAME: &str = "splitManifestName";
}

/// Keys of the budget a preview exceeded (`resource` parameter of `previewBudget`).
pub mod preview_resource {
    /// The source file is larger than the preview accepts.
    pub const SOURCE_FILE_SIZE: &str = "sourceFileSize";
    /// The expanded archive contents are too large.
    pub const EXPANDED_ZIP: &str = "expandedZip";
    /// A single embedded image is too large.
    pub const PER_IMAGE_BYTES: &str = "perImageBytes";
    /// The embedded images together are too large.
    pub const TOTAL_IMAGE_BYTES: &str = "totalImageBytes";
    /// The rendered output is too large.
    pub const RENDERED_OUTPUT: &str = "renderedOutput";
    /// A single embedded media item is too large.
    pub const PER_MEDIA_BYTES: &str = "perMediaBytes";
    /// The embedded media together are too large.
    pub const TOTAL_MEDIA_BYTES: &str = "totalMediaBytes";
    /// A single decoded image has too many pixels.
    pub const PER_IMAGE_PIXELS: &str = "perImagePixels";
    /// The decoded images together have too many pixels.
    pub const TOTAL_IMAGE_PIXELS: &str = "totalImagePixels";
    /// The extracted text is too large.
    pub const TEXT: &str = "text";
    /// The package has too many entries.
    pub const ZIP_ENTRIES: &str = "zipEntries";
    /// The document has too many images.
    pub const IMAGE_COUNT: &str = "imageCount";
    /// The presentation has too many media items.
    pub const MEDIA_COUNT: &str = "mediaCount";
    /// The presentation has too many slides.
    pub const SLIDE_COUNT: &str = "slideCount";
    /// The XML nests too deeply.
    pub const XML_DEPTH: &str = "xmlDepth";
}

/// Maps the English budget label used by the preview services to its [`preview_resource`] key.
#[must_use]
pub(crate) fn resource_key(label: &str) -> &'static str {
    use preview_resource as key;
    match label {
        "source file size" => key::SOURCE_FILE_SIZE,
        "expanded ZIP" => key::EXPANDED_ZIP,
        "per-image bytes" => key::PER_IMAGE_BYTES,
        "total image bytes" => key::TOTAL_IMAGE_BYTES,
        "rendered output" => key::RENDERED_OUTPUT,
        "per-media bytes" => key::PER_MEDIA_BYTES,
        "total media bytes" => key::TOTAL_MEDIA_BYTES,
        "per-image decoded pixels" => key::PER_IMAGE_PIXELS,
        "total decoded image pixels" => key::TOTAL_IMAGE_PIXELS,
        _ => key::TEXT,
    }
}
