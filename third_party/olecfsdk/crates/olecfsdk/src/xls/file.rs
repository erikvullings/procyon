//! Typed file and substream roots for the Excel binary format.
//!
//! [`XlsFile`] is the write authority for every managed BIFF stream. Its private
//! source CFB remains an immutable snapshot for unrelated entries; serialization
//! rebuilds workbook, pivot-cache, revision, and user-name streams from the
//! current Rust trees. Strict entry points and saves are the default. Compatible
//! parsing returns structured diagnostics, and compatibility nodes require an
//! explicit compatibility-preserving save policy.

use std::{
  collections::{BTreeMap, BTreeSet},
  io::Write,
  ops::Range,
  path::Path,
  sync::Arc,
};

use crate::{
  Error, Result,
  cfb::{CfbStreamOverride, CfbStreamWriter, CompoundFile, Entry, EntryKind},
  common::Guid,
  forms::ParentControlStorageModel,
  io::{BinaryFormat, SdkEnumValue},
  limits::Limits,
  office_art::{
    OfficeArtClientAnchor, OfficeArtDrawingGraph, OfficeArtImageRef, OfficeArtPropertyValue,
    OfficeArtRecord, OfficeArtRecordData, OfficeArtShape, OfficeArtStream,
  },
  parse::{
    ParseDiagnostic, ParseDiagnosticCode, ParseOptions, ParseOutcome, SpecificationReference,
    compound_from_bytes, compound_from_path, compound_from_vec, compound_outcome,
  },
  save::SaveOptions,
  shared_content::{
    OfficeFormsMutation, OfficeHostKind, OfficePropertySetKind, OfficeSharedContent,
    OfficeVbaModuleMutation,
  },
};

use super::{
  ArrayRecord, BCUsrsRecord, BOOK_STREAM_PATH, BiffConstant, BiffRecord, BiffRecordData,
  BiffStream, BiffStreamWritePlan, BiffUnicodeString, BlankRecord, BoolErrRecord, BoolErrValue,
  BoundSheet8Record, CUsrRecord, CbUsrRecord, CellErrorCode, CellHeader, CellRange, ColInfoRecord,
  DevModeW, ExtSstRecord, ExternNameBody, ExternNameRecord, ExternSheetReference,
  FeatureHeaderData, FileLockRecord, FontRecord, FormatRecord, FormulaCachedResult, FormulaRecord,
  FormulaTokenData, FormulaTokenStream, FormulaTokens, HyperlinkMoniker, HyperlinkObject,
  HyperlinkRecord, LabelRecord, LabelSstRecord, MergeCellsRecord, MsoDrawingData,
  MsoDrawingHostData, MsoDrawingHostRecord, MsoDrawingRecord, MulBlankRecord, MulRkCell,
  MulRkRecord, NameRecord, NameValue, NoteRecord, NumberRecord, ObjCommonData, ObjFormulaData,
  ObjPictureFlags, ObjPictureFormula, ObjRecord, PIVOT_CACHE_STORAGE_NAME,
  PIVOT_CACHE_STORAGE_PATH, PivotCacheStream, PlsRecord, PrinterSettings, REVISION_LOG_STREAM_NAME,
  RevisionLogStream, RkRecord, RowRecord, RrAutoFmtRecord, RrFormatRecord, RrInsertShRecord,
  RrTabIdRecord, Rrd, RrdChgCellRecord, RrdConflictRecord, RrdDefNameRecord, RrdHeadRecord,
  RrdInfoRecord, RrdInsDelRecord, RrdMoveRecord, RrdRenSheetRecord, RrdTqsifRecord,
  RrdUserViewRecord, SharedFormulaRecord, ShortXlUnicodeString, SstCompletion, SstRecord,
  SstString, StringValueRecord, SupBookLink, SupBookRecord, SupBookSheetName, SxStreamIdRecord,
  SxViewRecord, SxVsRecord, TableRecord, TxoRecord, USER_NAMES_STREAM_NAME, UserBViewRecord,
  UserNamesStream, UserSViewBeginChartRecord, UserSViewBeginRecord, UserSViewEndRecord,
  UsrChkRecord, UsrExclRecord, UsrInfoRecord, WORKBOOK_STREAM_PATH, XctRecord, XfExtRecord,
  XfRecord, XlStringCharacters,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XlsStreamName {
  Workbook,
  Book,
}

impl XlsStreamName {
  pub const fn name(self) -> &'static str {
    match self {
      Self::Workbook => "Workbook",
      Self::Book => "Book",
    }
  }

  pub const fn path(self) -> &'static str {
    match self {
      Self::Workbook => WORKBOOK_STREAM_PATH,
      Self::Book => BOOK_STREAM_PATH,
    }
  }
}

/// Specification-level kind of a BOF/EOF BIFF substream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BiffSubstreamKind {
  WorkbookGlobals,
  WorksheetOrDialogSheet,
  ChartSheet,
  MacroSheet,
  /// BIFF producer/legacy kinds outside the current MS-XLS BOF table.
  Compatibility(u16),
}

/// A structural BOF/EOF node indexing records in [`BiffWorkbookTree::stream`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BiffSubstreamNode {
  pub kind: BiffSubstreamKind,
  /// Includes the opening BOF and closing EOF records.
  pub record_range: Range<usize>,
  pub children: Vec<BiffSubstreamNode>,
}

/// Full BIFF stream plus its lossless structural substream index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BiffWorkbookTree {
  pub stream: BiffStream,
  pub substreams: Vec<BiffSubstreamNode>,
  /// Ranges not enclosed by BOF/EOF, retained for compatibility analysis.
  pub outside_substream_ranges: Vec<Range<usize>>,
}

/// Complete typed root for an Excel binary file.
///
/// See the runnable `edit_xls` example for open, relationship traversal, sheet
/// name edit, save, and strict reopen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XlsFile {
  compound_file: CompoundFile,
  pub shared: OfficeSharedContent,
  /// Every root BIFF workbook stream. Some compatibility files contain both
  /// the modern `Workbook` name and the legacy `Book` name. Clones share
  /// this collection; use [`Arc::make_mut`] before direct collection edits.
  pub workbooks: Arc<Vec<XlsWorkbookStream>>,
  /// Clone-shared pivot-cache stream collection, detached on mutation with
  /// [`Arc::make_mut`].
  pub pivot_caches: Arc<Vec<XlsPivotCache>>,
  /// Clone-shared standalone revision stream; detach with [`Arc::make_mut`]
  /// before direct edits.
  pub revision_log: Option<Arc<XlsRevisionLog>>,
  /// Clone-shared shared-workbook user stream; detach with [`Arc::make_mut`]
  /// before direct edits.
  pub user_names: Option<Arc<XlsUserNames>>,
}

/// A named root BIFF stream and its full record/substream tree.
#[derive(Clone, Debug)]
pub struct XlsWorkbookStream {
  pub name: XlsStreamName,
  /// Clone-shared BIFF record tree. Call [`Arc::make_mut`] before direct
  /// field edits; transactional SDK methods detach it automatically.
  pub tree: Arc<BiffWorkbookTree>,
  sheet_ids: Vec<XlsSheetId>,
}

/// The specification role of an MS-XLS storage or stream.
///
/// Payloads owned by another specification remain available through the
/// source [`Entry`]. This enum identifies their MS-XLS boundary without
/// parsing XML, OLE, property-set, encryption, or embedded-object bytes as
/// BIFF records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XlsFileEntryRole {
  ComponentObjectStream,
  ControlStream,
  DataSpacesStorage,
  DocumentSummaryInformationStream,
  EmbeddingStorage {
    object_id: u32,
  },
  EncryptionStream,
  LinkStorage {
    object_id: u32,
  },
  ListDataStream,
  OfficeDataStoreStorage,
  OfficeToolbarsStream,
  OleStream,
  PivotCacheStorage,
  PivotCacheStream {
    cache_id: u16,
  },
  ProtectedContentStream,
  RevisionStream,
  SignaturesStream,
  SummaryInformationStream,
  UserNamesStream,
  VbaStorage,
  ViewerContentStream,
  WorkbookStream(XlsStreamName),
  XmlSignaturesStorage,
  XmlStream,
  /// A CFB entry not assigned a role by MS-XLS section 2.1.7. Its complete
  /// entry metadata and bytes are still preserved and reachable.
  Other,
}

#[derive(Clone, Copy, Debug)]
pub struct XlsFileEntryRef<'a> {
  entry: &'a Entry,
  role: XlsFileEntryRole,
}

/// One validation issue in the MS-XLS storage/stream graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XlsFileEntryIssue {
  ExpectedStream {
    path: std::path::PathBuf,
  },
  ExpectedStorage {
    path: std::path::PathBuf,
  },
  DuplicateSingleton {
    role: &'static str,
    first: std::path::PathBuf,
    duplicate: std::path::PathBuf,
  },
  InvalidPivotCacheChild {
    path: std::path::PathBuf,
  },
}

/// Zero-copy inventory of the MS-XLS storages and streams in section 2.1.7.
#[derive(Debug)]
pub struct XlsStoragesAndStreams<'a> {
  entries: Vec<XlsFileEntryRef<'a>>,
  issues: Vec<XlsFileEntryIssue>,
}

/// A sheet identity defined by MS-XLS `TabId`/`RRTabId`.
///
/// Valid BIFF8 workbooks persist these identifiers in the same order as the
/// `BoundSheet8` collection. Compatibility inputs without a usable `RRTabId`
/// receive an explicitly non-specification positional identity; callers can
/// distinguish that boundary with [`XlsSheetId::tab_id`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct XlsSheetId {
  value: u32,
  kind: XlsSheetIdKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum XlsSheetIdKind {
  TabId,
  /// MS-XLS derives identifiers from one-based BoundSheet8 order when a
  /// workbook contains more than 4,112 sheets and omits RRTabId.
  SheetOrdinal,
  CompatibilityPosition,
}

/// Zero-copy relationship view over one Workbook Stream.
///
/// It joins Globals `BoundSheet8` records to their BOF/EOF sheet substreams
/// without copying BIFF records. Recreate the view after mutating or
/// reindexing the public record tree.
#[derive(Debug)]
pub struct XlsWorkbookView<'a> {
  workbook: &'a XlsWorkbookStream,
  globals: &'a BiffSubstreamNode,
  sheets: Vec<XlsSheetRef<'a>>,
  unresolved_sheets: Vec<XlsUnresolvedSheetRef<'a>>,
  unlinked_substreams: Vec<&'a BiffSubstreamNode>,
  supporting_links: Vec<XlsSupportingLinkRef<'a>>,
  extern_sheet_records: Vec<&'a BiffRecord>,
  external_sheets: Vec<XlsExternalSheetRef<'a>>,
  defined_names: Vec<&'a NameRecord>,
  pivot_cache_definitions: Vec<XlsPivotCacheDefinitionRef<'a>>,
  workbook_sheet_identifiers: Option<&'a RrTabIdRecord>,
  custom_views: Vec<XlsCustomViewRef<'a>>,
  unlinked_custom_sheet_views: Vec<XlsCustomSheetViewRef<'a>>,
  unlinked_custom_view_records: Vec<&'a BiffRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct XlsSupportingLinkId(usize);

#[derive(Debug)]
pub struct XlsSupportingLinkRef<'a> {
  id: XlsSupportingLinkId,
  source_record: &'a BiffRecord,
  value: &'a SupBookRecord,
  records: &'a [BiffRecord],
  external_name_records: Vec<&'a BiffRecord>,
  external_names: Vec<&'a ExternNameRecord>,
  external_cell_caches: Vec<XlsExternalCellCacheRef<'a>>,
  extern_sheet_records: Vec<&'a BiffRecord>,
  continuation_records: Vec<&'a BiffRecord>,
}

/// One specification XCT followed by its CRN cell-value records.
#[derive(Debug)]
pub struct XlsExternalCellCacheRef<'a> {
  supporting_link: XlsSupportingLinkId,
  source_record: &'a BiffRecord,
  value: &'a XctRecord,
  crn_records: Vec<&'a BiffRecord>,
}

#[derive(Clone, Copy, Debug)]
pub struct XlsExternalSheetRef<'a> {
  index: u16,
  source_record: &'a BiffRecord,
  source_reference_index: u16,
  source: &'a ExternSheetReference,
  supporting_link: Option<XlsSupportingLinkId>,
}

#[derive(Clone, Copy, Debug)]
pub struct XlsExternalNameRef<'view, 'a> {
  external_sheet: XlsExternalSheetRef<'a>,
  supporting_link: &'view XlsSupportingLinkRef<'a>,
  name: &'a ExternNameRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct XlsPivotCacheDefinitionId(usize);

#[derive(Clone, Copy, Debug)]
pub struct XlsPivotCacheDefinitionRef<'a> {
  id: XlsPivotCacheDefinitionId,
  source_record: &'a BiffRecord,
  value: &'a SxStreamIdRecord,
  source_type_record: Option<&'a BiffRecord>,
  source_type: Option<&'a SxVsRecord>,
}

#[derive(Clone, Copy, Debug)]
pub struct XlsPivotTableViewRef<'a> {
  sheet: XlsSheetRef<'a>,
  source_record: &'a BiffRecord,
  value: &'a SxViewRecord,
}

/// One sheet PivotTable joined through Globals to its physical PivotCache
/// storage stream. Every field borrows the single file-root-owned tree.
#[derive(Clone, Copy, Debug)]
pub struct XlsPivotTableRef<'a> {
  view: XlsPivotTableViewRef<'a>,
  definition: XlsPivotCacheDefinitionRef<'a>,
  cache: &'a XlsPivotCache,
}

#[derive(Clone, Copy, Debug)]
pub enum XlsPivotTableLink<'a> {
  Resolved(XlsPivotTableRef<'a>),
  Unresolved {
    view: XlsPivotTableViewRef<'a>,
    error: XlsPivotTableLinkError,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XlsPivotTableLinkError {
  NegativeDefinitionIndex { cache_index: i16 },
  MissingDefinition { cache_index: i16 },
  MissingCacheStream { stream_id: u16 },
  AmbiguousCacheStream { stream_id: u16 },
  ForeignView,
}

#[derive(Clone, Copy, Debug)]
pub enum XlsPivotTableCacheLink<'a> {
  Resolved(XlsPivotCacheDefinitionRef<'a>),
  Unresolved {
    view: XlsPivotTableViewRef<'a>,
    error: XlsPivotTableCacheLinkError,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XlsPivotTableCacheLinkError {
  Negative { cache_index: i16 },
  Missing { cache_index: i16 },
  ForeignView,
}

/// One Globals `BoundSheet8` and the sheet substream selected by its
/// specification `lbPlyPos` file pointer.
#[derive(Clone, Copy, Debug)]
pub struct XlsSheetRef<'a> {
  id: XlsSheetId,
  metadata_record: &'a BiffRecord,
  metadata: &'a BoundSheet8Record,
  substream: &'a BiffSubstreamNode,
  records: &'a [BiffRecord],
}

/// One specification `CUSTOMVIEW` production owned by a sheet.
///
/// The records remain in the Workbook Stream; this handle only joins the
/// `UserSViewBegin` and `UserSViewEnd` delimiters and retains their contents.
#[derive(Clone, Copy, Debug)]
pub struct XlsCustomSheetViewRef<'a> {
  sheet: XlsSheetRef<'a>,
  begin_record: &'a BiffRecord,
  begin: XlsCustomSheetViewBeginRef<'a>,
  content_records: &'a [BiffRecord],
  end_record: &'a BiffRecord,
  end: &'a UserSViewEndRecord,
}

#[derive(Clone, Copy, Debug)]
pub enum XlsCustomSheetViewBeginRef<'a> {
  Sheet(&'a UserSViewBeginRecord),
  Chart(&'a UserSViewBeginChartRecord),
}

/// One workbook custom view, aggregating its Globals `UserBView` with all
/// sheet-local `CUSTOMVIEW` productions that share the official GUID.
#[derive(Debug)]
pub struct XlsCustomViewRef<'a> {
  source_record: &'a BiffRecord,
  value: &'a UserBViewRecord,
  sheet_views: Vec<XlsCustomSheetViewRef<'a>>,
  defined_names: Vec<XlsCustomViewDefinedNameRef<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XlsCustomViewDefinedNameKind {
  PrintTitles,
  PrintArea,
  HiddenRows,
  HiddenColumns,
  FilterData,
  FilterCriteria,
}

#[derive(Clone, Copy, Debug)]
pub struct XlsCustomViewDefinedNameRef<'a> {
  source_record: &'a BiffRecord,
  value: &'a NameRecord,
  kind: XlsCustomViewDefinedNameKind,
}

#[derive(Clone, Copy, Debug)]
pub enum XlsCustomViewLink<'view, 'a> {
  Resolved(&'view XlsCustomViewRef<'a>),
  Missing { guid: [u8; 16] },
  Ambiguous { guid: [u8; 16] },
}

#[derive(Clone, Copy, Debug)]
pub enum XlsCustomViewActiveSheetLink<'a> {
  Resolved(XlsSheetRef<'a>),
  NotSpecified,
  Missing { sheet_identifier: u16 },
  Ambiguous { sheet_identifier: u16 },
}

/// A compatibility `BoundSheet8` whose file pointer cannot be joined without
/// inventing a relationship not present in the file.
#[derive(Clone, Copy, Debug)]
pub struct XlsUnresolvedSheetRef<'a> {
  id: XlsSheetId,
  metadata_record: &'a BiffRecord,
  metadata: &'a BoundSheet8Record,
  error: XlsSheetLinkError,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XlsSheetLinkError {
  Missing { sheet_bof_offset: u32 },
  Ambiguous { sheet_bof_offset: u32 },
  Duplicate { sheet_bof_offset: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XlsNumberFormatRef<'a> {
  BuiltIn(u16),
  Custom(&'a FormatRecord),
  Compatibility(u16),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XlsCellFormatRef<'a> {
  pub xf: &'a XfRecord,
  pub font: &'a FontRecord,
  pub number_format: XlsNumberFormatRef<'a>,
  /// Native custom format code. Built-in formats have no file-owned string
  /// and therefore leave this field as `None`.
  pub custom_number_format_code: Option<String>,
}

/// One logical cell borrowing its exact source BIFF record. MULRK and
/// MULBLANK cells retain their parent record and element index rather than
/// being copied into a grid projection.
#[derive(Clone, Copy, Debug)]
pub struct XlsCellRef<'a> {
  source_record: &'a BiffRecord,
  cell: CellHeader,
  value: XlsCellValueRef<'a>,
}

#[derive(Clone, Copy, Debug)]
pub enum XlsCellValueRef<'a> {
  Formula(&'a FormulaRecord),
  Formula4Compatibility(&'a FormulaRecord),
  Blank(&'a BlankRecord),
  Number(&'a NumberRecord),
  BoolErr(&'a BoolErrRecord),
  Label(&'a LabelRecord),
  LabelSst(&'a LabelSstRecord),
  Rk(&'a RkRecord),
  MulRk {
    parent: &'a MulRkRecord,
    index: usize,
    value: &'a MulRkCell,
  },
  MulBlank {
    parent: &'a MulBlankRecord,
    index: usize,
  },
}

/// Native stored value of one logical BIFF cell. Formula tokens remain on
/// [`XlsFormulaRef`]; this value is the exact cached scalar stored in the file.
#[derive(Clone, Debug, PartialEq)]
pub enum XlsCellValue {
  Blank,
  Number(f64),
  Boolean(bool),
  Error(CellErrorCode),
  String(String),
  Formula(XlsFormulaCachedValue),
  /// Exact producer-compatible BoolErr representation that cannot be a
  /// conforming BIFF8 Boolean or error scalar.
  CompatibilityBoolErr {
    value: u16,
    is_error: u8,
  },
}

/// Native cached result stored by a Formula record. String results borrow no
/// second tree: they are decoded on demand from the adjacent String record.
#[derive(Clone, Debug, PartialEq)]
pub enum XlsFormulaCachedValue {
  Number(f64),
  String(String),
  Boolean(bool),
  Error(CellErrorCode),
  Empty,
}

/// One NoteSh cell comment joined to its Obj and TxO owners inside the same
/// MsoDrawing aggregate. String fields are native values; the physical BIFF
/// nodes remain uniquely owned by the workbook tree.
#[derive(Clone, Debug)]
pub struct XlsCommentRef<'a> {
  source_record: &'a BiffRecord,
  note_host: &'a MsoDrawingHostRecord,
  note: &'a NoteRecord,
  object: XlsObjectRef<'a>,
  text_host: &'a MsoDrawingHostRecord,
  text_object: &'a TxoRecord,
  pub author: String,
  pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XlsHyperlinkTarget<'a> {
  String(String),
  Url(String),
  File {
    short_name: &'a [u8],
    long_path: Option<String>,
  },
  Standard {
    class_id: [u8; 16],
    options: u16,
    data: &'a [u8],
  },
}

/// One HLink record with its file-native strings decoded losslessly. A record
/// can contain both a moniker target and a location fragment.
#[derive(Clone, Debug)]
pub struct XlsHyperlinkRef<'a> {
  source_record: &'a BiffRecord,
  value: &'a HyperlinkRecord,
  pub display_name: Option<String>,
  pub target_frame_name: Option<String>,
  pub location: Option<String>,
  pub target: Option<XlsHyperlinkTarget<'a>>,
}

/// Mutable view of one logical cell selected through the relationship tree.
/// MUL records expose only their exact logical element, not unrelated cells.
pub enum XlsCellMut<'a> {
  Formula(&'a mut FormulaRecord),
  Formula4Compatibility(&'a mut FormulaRecord),
  Blank(&'a mut BlankRecord),
  Number(&'a mut NumberRecord),
  BoolErr(&'a mut BoolErrRecord),
  Label(&'a mut LabelRecord),
  LabelSst(&'a mut LabelSstRecord),
  Rk(&'a mut RkRecord),
  MulRk(&'a mut MulRkCell),
  MulBlankFormat(&'a mut u16),
}

#[derive(Clone, Copy)]
enum XlsCellMutationTarget {
  Record(usize),
  MulRk { record: usize, element: usize },
  MulBlank { record: usize, element: usize },
}

pub struct XlsCells<'a> {
  records: std::iter::Enumerate<std::slice::Iter<'a, BiffRecord>>,
  substream: &'a BiffSubstreamNode,
  pending: Option<XlsPendingCells<'a>>,
}

/// Optional sparse `(row, column)` index over the exact logical cell refs of
/// one sheet. Building it copies only small handles, never record payloads.
#[derive(Debug)]
pub struct XlsSparseCellIndex<'a> {
  sheet: XlsSheetRef<'a>,
  cells: BTreeMap<(u16, u16), Vec<XlsCellRef<'a>>>,
  rows: BTreeMap<u16, Vec<&'a RowRecord>>,
  formula_groups: BTreeMap<(u16, u16, XlsFormulaGroupTokenKind), Vec<XlsFormulaDefinitionRef<'a>>>,
  cell_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum XlsFormulaGroupTokenKind {
  Exp,
  Table,
}

#[derive(Clone, Copy, Debug)]
pub struct XlsSparseRowRef<'index, 'a> {
  index: &'index XlsSparseCellIndex<'a>,
  row: u16,
  definitions: &'index [&'a RowRecord],
}

enum XlsPendingCells<'a> {
  MulRk {
    source_record: &'a BiffRecord,
    value: &'a MulRkRecord,
    index: usize,
  },
  MulBlank {
    source_record: &'a BiffRecord,
    value: &'a MulBlankRecord,
    index: usize,
  },
}

#[derive(Clone, Copy, Debug)]
pub struct XlsFormulaRef<'a> {
  source_record: &'a BiffRecord,
  formula: &'a FormulaRecord,
  cached_string: Option<&'a StringValueRecord>,
  definition: XlsFormulaDefinitionRef<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct XlsObjectId(u16);

#[derive(Clone, Copy, Debug)]
pub struct XlsObjectRef<'a> {
  source_record: &'a BiffRecord,
  host_record: Option<&'a MsoDrawingHostRecord>,
  value: &'a ObjRecord,
  common: Option<&'a ObjCommonData>,
  picture_flags: Option<ObjPictureFlags>,
  picture_formula: Option<&'a ObjPictureFormula>,
}

#[derive(Clone, Copy, Debug)]
pub struct XlsDrawingGroupRef<'a> {
  source_record: &'a BiffRecord,
  value: &'a MsoDrawingRecord,
}

#[derive(Clone, Copy, Debug)]
pub struct XlsDrawingRef<'a> {
  sheet: XlsSheetRef<'a>,
  source_record: &'a BiffRecord,
  value: &'a MsoDrawingRecord,
}

/// One worksheet picture joined to its OfficeArt shape, BIFF client anchor,
/// one-based workbook BLIP-store identity, and borrowed image payload.
#[derive(Clone, Copy, Debug)]
pub struct XlsPictureRef<'a> {
  sheet: XlsSheetRef<'a>,
  drawing_order: usize,
  shape_type: u16,
  shape: &'a OfficeArtShape,
  properties: &'a [OfficeArtRecord],
  anchor: OfficeArtClientAnchor,
  blip_identifier: u32,
  image: XlsPictureImageLink<'a>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XlsPictureCrop {
  top: i32,
  bottom: i32,
  left: i32,
  right: i32,
}

/// Resolution state of an XLS picture's workbook-global BLIP reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XlsPictureImageLink<'a> {
  Resolved(OfficeArtImageRef<'a>),
  Delayed { offset: u32 },
  Unsupported,
  Missing,
}

pub struct XlsObjects<'a> {
  sheet: XlsSheetRef<'a>,
  record_index: usize,
  host_record_index: usize,
}

#[derive(Debug)]
pub enum XlsObjectPersistenceRef<'view, 'a> {
  ControlStream {
    stream: &'a Entry,
    offset: u32,
    data: &'a [u8],
  },
  EmbeddingStorage {
    storage_id: u32,
    storage: &'a Entry,
    entries: Vec<&'a Entry>,
  },
  LinkStorage {
    storage_id: u32,
    storage: &'a Entry,
    entries: Vec<&'a Entry>,
    external_name: XlsExternalNameRef<'view, 'a>,
  },
  DdeDataItem {
    external_name: XlsExternalNameRef<'view, 'a>,
  },
}

#[derive(Clone, Copy, Debug)]
pub enum XlsFormulaDefinitionRef<'a> {
  Inline(&'a FormulaTokens),
  Shared(&'a SharedFormulaRecord),
  Array(&'a ArrayRecord),
  Table(&'a TableRecord),
  UnresolvedExp { row: u16, column: u16 },
  UnresolvedTable { row: u16, column: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XlsRevisionLog {
  Parsed(RevisionLogStream),
  Compatibility { bytes: Vec<u8>, reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XlsUserNames {
  Parsed(UserNamesStream),
  Compatibility { bytes: Vec<u8>, reason: String },
}

/// Zero-copy view of the MS-XLS user log carried by the `User Names` stream.
#[derive(Debug)]
pub struct XlsUserLogView<'a> {
  stream: &'a UserNamesStream,
  user_count_record: &'a BiffRecord,
  user_count: &'a CUsrRecord,
  version_record: &'a BiffRecord,
  version: &'a UsrChkRecord,
  size_table_record: &'a BiffRecord,
  size_table: &'a CbUsrRecord,
  user_collection_record: &'a BiffRecord,
  user_collection: &'a BCUsrsRecord,
  users: Vec<XlsUserInfoRef<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct XlsUserInfoRef<'a> {
  source_record: &'a BiffRecord,
  value: &'a UsrInfoRecord,
}

/// Zero-copy view of one `Revision Log` stream and the revision logs it owns.
#[derive(Debug)]
pub struct XlsRevisionStreamView<'a> {
  stream: &'a RevisionLogStream,
  revision_information: Option<(&'a BiffRecord, &'a RrdInfoRecord)>,
  file_lock: Option<(&'a BiffRecord, &'a FileLockRecord)>,
  user_exclusion: Option<(&'a BiffRecord, &'a UsrExclRecord)>,
  revision_logs: Vec<XlsRevisionLogRef<'a>>,
  eof_record: Option<&'a BiffRecord>,
  unlinked_records: Vec<&'a BiffRecord>,
}

/// One HEADER production (`RRDHead RRTabId`) and its revision records.
#[derive(Clone, Copy, Debug)]
pub struct XlsRevisionLogRef<'a> {
  header_record: &'a BiffRecord,
  header: &'a RrdHeadRecord,
  sheet_identifiers_record: &'a BiffRecord,
  sheet_identifiers: &'a RrTabIdRecord,
  records: &'a [BiffRecord],
}

/// Parsed top-level productions owned by one revision log. Every handle
/// borrows the exact BIFF records; no revision payload is copied.
#[derive(Debug)]
pub struct XlsRevisionRecordsView<'a> {
  revisions: Vec<XlsRevisionRecordRef<'a>>,
  unlinked_records: Vec<&'a BiffRecord>,
}

#[derive(Debug)]
pub enum XlsRevisionRecordRef<'a> {
  RenameSheet {
    source_record: &'a BiffRecord,
    value: &'a RrdRenSheetRecord,
  },
  InsertDelete(XlsRevisionInsertDeleteRef<'a>),
  Conflict {
    source_record: &'a BiffRecord,
    value: &'a RrdConflictRecord,
  },
  InsertSheet {
    source_record: &'a BiffRecord,
    value: &'a RrInsertShRecord,
  },
  ChangeCell(XlsRevisionChangeCellRef<'a>),
  Move(XlsRevisionMoveRef<'a>),
  Format {
    source_record: &'a BiffRecord,
    value: &'a RrFormatRecord,
  },
  AutoFormat {
    source_record: &'a BiffRecord,
    value: &'a RrAutoFmtRecord,
  },
  DefinedName {
    source_record: &'a BiffRecord,
    value: &'a RrdDefNameRecord,
  },
  UserView {
    source_record: &'a BiffRecord,
    value: &'a RrdUserViewRecord,
  },
  Note {
    source_record: &'a BiffRecord,
    value: &'a NoteRecord,
  },
  TrashQueryTableField {
    source_record: &'a BiffRecord,
    value: &'a RrdTqsifRecord,
  },
}

#[derive(Debug)]
pub struct XlsRevisionInsertDeleteRef<'a> {
  begin_record: Option<&'a BiffRecord>,
  operation_record: &'a BiffRecord,
  operation: &'a RrdInsDelRecord,
  changes: Vec<XlsRevisionCellOrFormatRef<'a>>,
  end_record: Option<&'a BiffRecord>,
}

#[derive(Debug)]
pub struct XlsRevisionMoveRef<'a> {
  begin_record: &'a BiffRecord,
  move_record: &'a BiffRecord,
  value: &'a RrdMoveRecord,
  changes: Vec<XlsRevisionCellOrFormatRef<'a>>,
  end_record: Option<&'a BiffRecord>,
}

#[derive(Clone, Copy, Debug)]
pub enum XlsRevisionCellOrFormatRef<'a> {
  ChangeCell(XlsRevisionChangeCellRef<'a>),
  Format {
    source_record: &'a BiffRecord,
    value: &'a RrFormatRecord,
  },
}

#[derive(Clone, Copy, Debug)]
pub struct XlsRevisionChangeCellRef<'a> {
  source_record: &'a BiffRecord,
  value: &'a RrdChgCellRecord,
  font_reset_records: &'a [BiffRecord],
}

/// File-root revision graph. It owns only small relationship handles; every
/// BIFF production and target remains borrowed from the authoritative trees.
#[derive(Debug)]
pub struct XlsRevisionGraph<'view, 'a> {
  stream: XlsRevisionStreamView<'a>,
  logs: Vec<XlsRevisionGraphLog<'view, 'a>>,
}

#[derive(Debug)]
pub struct XlsRevisionGraphLog<'view, 'a> {
  source: XlsRevisionLogRef<'a>,
  revisions: Vec<XlsRevisionNode<'view, 'a>>,
  unlinked_records: Vec<&'a BiffRecord>,
}

#[derive(Debug)]
pub struct XlsRevisionNode<'view, 'a> {
  source: XlsRevisionRecordRef<'a>,
  sheet: XlsRevisionSheetLink<'a>,
  local_name_sheet: XlsRevisionSheetLink<'a>,
  custom_view: Option<XlsCustomViewLink<'view, 'a>>,
}

#[derive(Clone, Debug)]
pub enum XlsRevisionSheetLink<'a> {
  Resolved(XlsSheetRef<'a>),
  NotSpecified,
  Unresolved {
    sheet_identifier: u16,
    reason: String,
  },
}

#[derive(Clone, Copy, Debug)]
pub enum XlsUserRevisionLogLink<'a> {
  Resolved(XlsRevisionLogRef<'a>),
  Missing(XlsUserInfoRef<'a>),
  Ambiguous(XlsUserInfoRef<'a>),
}

impl XlsRevisionLog {
  pub const fn stream(&self) -> Option<&RevisionLogStream> {
    match self {
      Self::Parsed(stream) => Some(stream),
      Self::Compatibility { .. } => None,
    }
  }

  pub fn stream_mut(&mut self) -> Option<&mut RevisionLogStream> {
    match self {
      Self::Parsed(stream) => Some(stream),
      Self::Compatibility { .. } => None,
    }
  }
}

impl XlsUserNames {
  pub const fn stream(&self) -> Option<&UserNamesStream> {
    match self {
      Self::Parsed(stream) => Some(stream),
      Self::Compatibility { .. } => None,
    }
  }

  pub fn stream_mut(&mut self) -> Option<&mut UserNamesStream> {
    match self {
      Self::Parsed(stream) => Some(stream),
      Self::Compatibility { .. } => None,
    }
  }
}

impl UserNamesStream {
  pub fn relationships(&self) -> Result<XlsUserLogView<'_>> {
    self.validate()?;
    self.relationships_compatible()
  }

  pub fn relationships_compatible(&self) -> Result<XlsUserLogView<'_>> {
    let [
      user_count_record,
      version_record,
      size_table_record,
      user_collection_record,
      users @ ..,
    ] = self.records.as_slice()
    else {
      return Err(Error::invalid(
        self
          .records
          .last()
          .map_or(0, |record| u64::from(record.offset)),
        "User Names stream lacks its four required records",
      ));
    };
    let BiffRecordData::CUsr(user_count) = &user_count_record.data else {
      return Err(Error::invalid(
        u64::from(user_count_record.offset),
        "User Names stream does not begin with CUsr",
      ));
    };
    let BiffRecordData::UsrChk(version) = &version_record.data else {
      return Err(Error::invalid(
        u64::from(version_record.offset),
        "CUsr is not followed by UsrChk",
      ));
    };
    let BiffRecordData::CbUsr(size_table) = &size_table_record.data else {
      return Err(Error::invalid(
        u64::from(size_table_record.offset),
        "UsrChk is not followed by CbUsr",
      ));
    };
    let BiffRecordData::BCUsrs(user_collection) = &user_collection_record.data else {
      return Err(Error::invalid(
        u64::from(user_collection_record.offset),
        "CbUsr is not followed by BCUsrs",
      ));
    };
    let users = users
      .iter()
      .map(|source_record| match &source_record.data {
        BiffRecordData::UsrInfo(value) => Ok(XlsUserInfoRef {
          source_record,
          value,
        }),
        _ => Err(Error::invalid(
          u64::from(source_record.offset),
          "User Names stream contains a non-UsrInfo record after BCUsrs",
        )),
      })
      .collect::<Result<Vec<_>>>()?;
    Ok(XlsUserLogView {
      stream: self,
      user_count_record,
      user_count,
      version_record,
      version,
      size_table_record,
      size_table,
      user_collection_record,
      user_collection,
      users,
    })
  }
}

impl<'a> XlsUserLogView<'a> {
  pub const fn stream(&self) -> &'a UserNamesStream {
    self.stream
  }

  pub const fn user_count_record(&self) -> &'a BiffRecord {
    self.user_count_record
  }

  pub const fn user_count(&self) -> &'a CUsrRecord {
    self.user_count
  }

  pub const fn version_record(&self) -> &'a BiffRecord {
    self.version_record
  }

  pub const fn version(&self) -> &'a UsrChkRecord {
    self.version
  }

  pub const fn size_table_record(&self) -> &'a BiffRecord {
    self.size_table_record
  }

  pub const fn size_table(&self) -> &'a CbUsrRecord {
    self.size_table
  }

  pub const fn user_collection_record(&self) -> &'a BiffRecord {
    self.user_collection_record
  }

  pub const fn user_collection(&self) -> &'a BCUsrsRecord {
    self.user_collection
  }

  pub fn users(&self) -> &[XlsUserInfoRef<'a>] {
    &self.users
  }

  pub fn resolve_revision_log(
    &self,
    user: XlsUserInfoRef<'a>,
    revisions: &XlsRevisionStreamView<'a>,
  ) -> Result<XlsRevisionLogRef<'a>> {
    let mut matches = revisions
      .revision_logs
      .iter()
      .copied()
      .filter(|log| log.header.revision_set_guid == user.value.last_revision_guid);
    let Some(log) = matches.next() else {
      return Err(Error::invalid(
        u64::from(user.source_record.offset),
        "UsrInfo.guid does not match an RRDHead.guid",
      ));
    };
    if matches.next().is_some() {
      return Err(Error::invalid(
        u64::from(user.source_record.offset),
        "UsrInfo.guid matches multiple RRDHead.guid values",
      ));
    }
    Ok(log)
  }

  pub fn resolve_revision_log_compatible(
    &self,
    user: XlsUserInfoRef<'a>,
    revisions: &XlsRevisionStreamView<'a>,
  ) -> XlsUserRevisionLogLink<'a> {
    let mut matches = revisions
      .revision_logs
      .iter()
      .copied()
      .filter(|log| log.header.revision_set_guid == user.value.last_revision_guid);
    match (matches.next(), matches.next()) {
      (Some(log), None) => XlsUserRevisionLogLink::Resolved(log),
      (None, _) => XlsUserRevisionLogLink::Missing(user),
      (Some(_), Some(_)) => XlsUserRevisionLogLink::Ambiguous(user),
    }
  }
}

impl<'a> XlsUserInfoRef<'a> {
  pub const fn source_record(self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn value(self) -> &'a UsrInfoRecord {
    self.value
  }
}

impl RevisionLogStream {
  pub fn relationships(&self) -> Result<XlsRevisionStreamView<'_>> {
    self.validate()?;
    let view = self.relationships_compatible()?;
    if let Some(record) = view.unlinked_records.first() {
      return Err(Error::invalid(
        u64::from(record.offset),
        "Revision Stream record is outside its MS-XLS REVISION production",
      ));
    }
    for log in &view.revision_logs {
      if let Some(record) = log
        .records
        .iter()
        .find(|record| !is_revision_log_content(&record.data))
      {
        return Err(Error::invalid(
          u64::from(record.offset),
          "Revision log contains a record outside its revision productions",
        ));
      }
      log.revision_records()?;
    }
    Ok(view)
  }

  pub fn relationships_compatible(&self) -> Result<XlsRevisionStreamView<'_>> {
    let mut cursor = 0usize;
    let mut take_prefix = |predicate: fn(&BiffRecordData) -> bool| {
      let record = self
        .records
        .get(cursor)
        .filter(|record| predicate(&record.data));
      if record.is_some() {
        cursor += 1;
      }
      record
    };
    let revision_information = take_prefix(|data| matches!(data, BiffRecordData::RrdInfo(_)))
      .and_then(|record| match &record.data {
        BiffRecordData::RrdInfo(value) => Some((record, value)),
        _ => None,
      });
    let file_lock =
      take_prefix(|data| matches!(data, BiffRecordData::FileLock(_))).and_then(|record| {
        match &record.data {
          BiffRecordData::FileLock(value) => Some((record, value)),
          _ => None,
        }
      });
    let user_exclusion = take_prefix(|data| matches!(data, BiffRecordData::UsrExcl(_))).and_then(
      |record| match &record.data {
        BiffRecordData::UsrExcl(value) => Some((record, value)),
        _ => None,
      },
    );
    let mut revision_logs = Vec::new();
    let mut unlinked_records = Vec::new();
    let mut eof_record = None;
    while cursor < self.records.len() {
      let record = &self.records[cursor];
      if matches!(record.data, BiffRecordData::Eof) {
        eof_record = Some(record);
        cursor += 1;
        unlinked_records.extend(self.records[cursor..].iter());
        break;
      }
      let BiffRecordData::RrdHead(header) = &record.data else {
        unlinked_records.push(record);
        cursor += 1;
        continue;
      };
      let Some(sheet_identifiers_record) = self.records.get(cursor + 1) else {
        unlinked_records.push(record);
        break;
      };
      let BiffRecordData::RrTabId(sheet_identifiers) = &sheet_identifiers_record.data else {
        unlinked_records.push(record);
        cursor += 1;
        continue;
      };
      let content_start = cursor + 2;
      let content_end = self.records[content_start..]
        .iter()
        .position(|candidate| {
          matches!(
            candidate.data,
            BiffRecordData::RrdHead(_) | BiffRecordData::Eof
          )
        })
        .map_or(self.records.len(), |relative| content_start + relative);
      revision_logs.push(XlsRevisionLogRef {
        header_record: record,
        header,
        sheet_identifiers_record,
        sheet_identifiers,
        records: &self.records[content_start..content_end],
      });
      cursor = content_end;
    }
    Ok(XlsRevisionStreamView {
      stream: self,
      revision_information,
      file_lock,
      user_exclusion,
      revision_logs,
      eof_record,
      unlinked_records,
    })
  }
}

fn is_revision_log_content(data: &BiffRecordData) -> bool {
  matches!(
    data,
    BiffRecordData::RrdRenSheet(_)
      | BiffRecordData::RrdInsDel(_)
      | BiffRecordData::RrdInsDelBegin
      | BiffRecordData::RrdInsDelEnd
      | BiffRecordData::RrdConflict(_)
      | BiffRecordData::RrInsertSh(_)
      | BiffRecordData::RrdChgCell(_)
      | BiffRecordData::Continue { .. }
      | BiffRecordData::RrdRstEtxp(_)
      | BiffRecordData::RrdMoveBegin
      | BiffRecordData::RrdMove(_)
      | BiffRecordData::RrdMoveEnd
      | BiffRecordData::RrFormat(_)
      | BiffRecordData::RrAutoFmt(_)
      | BiffRecordData::RrdDefName(_)
      | BiffRecordData::RrdUserView(_)
      | BiffRecordData::RrdTqsif(_)
      | BiffRecordData::Note(_)
  )
}

impl<'a> XlsRevisionStreamView<'a> {
  pub const fn stream(&self) -> &'a RevisionLogStream {
    self.stream
  }

  pub const fn revision_information(&self) -> Option<(&'a BiffRecord, &'a RrdInfoRecord)> {
    self.revision_information
  }

  pub const fn file_lock(&self) -> Option<(&'a BiffRecord, &'a FileLockRecord)> {
    self.file_lock
  }

  pub const fn user_exclusion(&self) -> Option<(&'a BiffRecord, &'a UsrExclRecord)> {
    self.user_exclusion
  }

  pub fn revision_logs(&self) -> &[XlsRevisionLogRef<'a>] {
    &self.revision_logs
  }

  pub const fn eof_record(&self) -> Option<&'a BiffRecord> {
    self.eof_record
  }

  pub fn unlinked_records(&self) -> &[&'a BiffRecord] {
    &self.unlinked_records
  }
}

impl<'a> XlsRevisionLogRef<'a> {
  pub const fn header_record(self) -> &'a BiffRecord {
    self.header_record
  }

  pub const fn header(self) -> &'a RrdHeadRecord {
    self.header
  }

  pub const fn sheet_identifiers_record(self) -> &'a BiffRecord {
    self.sheet_identifiers_record
  }

  pub const fn sheet_identifiers(self) -> &'a RrTabIdRecord {
    self.sheet_identifiers
  }

  pub const fn records(self) -> &'a [BiffRecord] {
    self.records
  }

  /// Parses the complete MS-XLS revision production and rejects unmatched
  /// begin/end markers or records outside the grammar.
  pub fn revision_records(self) -> Result<Vec<XlsRevisionRecordRef<'a>>> {
    let view = self.revision_records_compatible();
    if let Some(record) = view.unlinked_records.first() {
      return Err(Error::invalid(
        u64::from(record.offset),
        "record is outside the Revision Stream revision productions",
      ));
    }
    for revision in &view.revisions {
      match revision {
        XlsRevisionRecordRef::InsertDelete(value)
          if value.begin_record.is_some() && value.end_record.is_none() =>
        {
          return Err(Error::invalid(
            u64::from(value.operation_record.offset),
            "RRDInsDelBegin production lacks RRDInsDelEnd",
          ));
        }
        XlsRevisionRecordRef::Move(value) if value.end_record.is_none() => {
          return Err(Error::invalid(
            u64::from(value.move_record.offset),
            "RRDMoveBegin production lacks RRDMoveEnd",
          ));
        }
        _ => {}
      }
    }
    Ok(view.revisions)
  }

  /// Retains every complete production and reports unmatched records
  /// separately instead of guessing a relationship.
  pub fn revision_records_compatible(self) -> XlsRevisionRecordsView<'a> {
    let mut revisions = Vec::new();
    let mut unlinked_records = Vec::new();
    let mut cursor = 0usize;
    while cursor < self.records.len() {
      let source_record = &self.records[cursor];
      match &source_record.data {
        BiffRecordData::RrdRenSheet(value) => {
          revisions.push(XlsRevisionRecordRef::RenameSheet {
            source_record,
            value,
          });
          cursor += 1;
        }
        BiffRecordData::RrdInsDelBegin => {
          let Some(operation_record) = self.records.get(cursor + 1) else {
            unlinked_records.push(source_record);
            break;
          };
          let BiffRecordData::RrdInsDel(operation) = &operation_record.data else {
            unlinked_records.push(source_record);
            cursor += 1;
            continue;
          };
          cursor += 2;
          let changes = parse_revision_changes(self.records, &mut cursor);
          let end_record = self
            .records
            .get(cursor)
            .filter(|record| matches!(record.data, BiffRecordData::RrdInsDelEnd));
          cursor += usize::from(end_record.is_some());
          revisions.push(XlsRevisionRecordRef::InsertDelete(
            XlsRevisionInsertDeleteRef {
              begin_record: Some(source_record),
              operation_record,
              operation,
              changes,
              end_record,
            },
          ));
        }
        BiffRecordData::RrdInsDel(operation) => {
          cursor += 1;
          let changes = parse_revision_changes(self.records, &mut cursor);
          revisions.push(XlsRevisionRecordRef::InsertDelete(
            XlsRevisionInsertDeleteRef {
              begin_record: None,
              operation_record: source_record,
              operation,
              changes,
              end_record: None,
            },
          ));
        }
        BiffRecordData::RrdConflict(value) => {
          revisions.push(XlsRevisionRecordRef::Conflict {
            source_record,
            value,
          });
          cursor += 1;
        }
        BiffRecordData::RrInsertSh(value) => {
          revisions.push(XlsRevisionRecordRef::InsertSheet {
            source_record,
            value,
          });
          cursor += 1;
        }
        BiffRecordData::RrdChgCell(_) => {
          revisions.push(XlsRevisionRecordRef::ChangeCell(
            parse_revision_change_cell(self.records, &mut cursor)
              .expect("RRDChgCell match was checked"),
          ));
        }
        BiffRecordData::RrdMoveBegin => {
          let Some(move_record) = self.records.get(cursor + 1) else {
            unlinked_records.push(source_record);
            break;
          };
          let BiffRecordData::RrdMove(value) = &move_record.data else {
            unlinked_records.push(source_record);
            cursor += 1;
            continue;
          };
          cursor += 2;
          let changes = parse_revision_changes(self.records, &mut cursor);
          let end_record = self
            .records
            .get(cursor)
            .filter(|record| matches!(record.data, BiffRecordData::RrdMoveEnd));
          cursor += usize::from(end_record.is_some());
          revisions.push(XlsRevisionRecordRef::Move(XlsRevisionMoveRef {
            begin_record: source_record,
            move_record,
            value,
            changes,
            end_record,
          }));
        }
        BiffRecordData::RrFormat(value) => {
          revisions.push(XlsRevisionRecordRef::Format {
            source_record,
            value,
          });
          cursor += 1;
        }
        BiffRecordData::RrAutoFmt(value) => {
          revisions.push(XlsRevisionRecordRef::AutoFormat {
            source_record,
            value,
          });
          cursor += 1;
        }
        BiffRecordData::RrdDefName(value) => {
          revisions.push(XlsRevisionRecordRef::DefinedName {
            source_record,
            value,
          });
          cursor += 1;
        }
        BiffRecordData::RrdUserView(value) => {
          revisions.push(XlsRevisionRecordRef::UserView {
            source_record,
            value,
          });
          cursor += 1;
        }
        BiffRecordData::Note(value) => {
          revisions.push(XlsRevisionRecordRef::Note {
            source_record,
            value,
          });
          cursor += 1;
        }
        BiffRecordData::RrdTqsif(value) => {
          revisions.push(XlsRevisionRecordRef::TrashQueryTableField {
            source_record,
            value,
          });
          cursor += 1;
        }
        _ => {
          unlinked_records.push(source_record);
          cursor += 1;
        }
      }
    }
    XlsRevisionRecordsView {
      revisions,
      unlinked_records,
    }
  }

  pub fn resolve_sheet(
    self,
    sheet_identifier: u16,
    workbook: &XlsWorkbookView<'a>,
  ) -> Result<XlsSheetRef<'a>> {
    let mut positions = self
      .sheet_identifiers
      .sheet_ids
      .iter()
      .enumerate()
      .filter(|(_, candidate)| **candidate == sheet_identifier)
      .map(|(index, _)| index);
    let Some(position) = positions.next() else {
      return Err(Error::invalid(
        u64::from(self.sheet_identifiers_record.offset),
        format!("sheet identifier {sheet_identifier} is absent from RRTabId"),
      ));
    };
    if positions.next().is_some() {
      return Err(Error::invalid(
        u64::from(self.sheet_identifiers_record.offset),
        format!("sheet identifier {sheet_identifier} occurs more than once in RRTabId"),
      ));
    }
    workbook.sheet_at_position(position).ok_or_else(|| {
      Error::invalid(
        u64::from(self.sheet_identifiers_record.offset),
        format!("RRTabId position {position} has no BoundSheet8 relationship"),
      )
    })
  }
}

fn parse_revision_change_cell<'a>(
  records: &'a [BiffRecord],
  cursor: &mut usize,
) -> Option<XlsRevisionChangeCellRef<'a>> {
  let source_record = records.get(*cursor)?;
  let BiffRecordData::RrdChgCell(value) = &source_record.data else {
    return None;
  };
  *cursor += 1;
  let reset_start = *cursor;
  while records
    .get(*cursor)
    .is_some_and(|record| matches!(record.data, BiffRecordData::RrdRstEtxp(_)))
  {
    *cursor += 1;
  }
  Some(XlsRevisionChangeCellRef {
    source_record,
    value,
    font_reset_records: &records[reset_start..*cursor],
  })
}

fn parse_revision_changes<'a>(
  records: &'a [BiffRecord],
  cursor: &mut usize,
) -> Vec<XlsRevisionCellOrFormatRef<'a>> {
  let mut changes = Vec::new();
  loop {
    if let Some(value) = parse_revision_change_cell(records, cursor) {
      changes.push(XlsRevisionCellOrFormatRef::ChangeCell(value));
      continue;
    }
    let Some(source_record) = records.get(*cursor) else {
      break;
    };
    let BiffRecordData::RrFormat(value) = &source_record.data else {
      break;
    };
    changes.push(XlsRevisionCellOrFormatRef::Format {
      source_record,
      value,
    });
    *cursor += 1;
  }
  changes
}

impl<'a> XlsRevisionRecordsView<'a> {
  pub fn revisions(&self) -> &[XlsRevisionRecordRef<'a>] {
    &self.revisions
  }

  pub fn unlinked_records(&self) -> &[&'a BiffRecord] {
    &self.unlinked_records
  }
}

impl<'a> XlsRevisionRecordRef<'a> {
  pub const fn revision(&self) -> Option<&'a Rrd> {
    Some(match self {
      Self::RenameSheet { value, .. } => &value.revision,
      Self::InsertDelete(value) => &value.operation.revision,
      Self::Conflict { value, .. } => &value.revision,
      Self::InsertSheet { value, .. } => &value.revision,
      Self::ChangeCell(value) => &value.value.revision,
      Self::Move(value) => &value.value.revision,
      Self::Format { value, .. } => &value.revision,
      Self::AutoFormat { value, .. } => &value.revision,
      Self::DefinedName { value, .. } => &value.revision,
      Self::UserView { value, .. } => &value.revision,
      Self::TrashQueryTableField { value, .. } => &value.revision,
      Self::Note { .. } => return None,
    })
  }

  pub const fn sheet_identifier(&self) -> Option<u16> {
    match self.revision() {
      Some(revision) if revision.sheet_id != u16::MAX => Some(revision.sheet_id),
      _ => None,
    }
  }

  pub fn resolve_sheet(
    &self,
    log: XlsRevisionLogRef<'a>,
    workbook: &XlsWorkbookView<'a>,
  ) -> Result<Option<XlsSheetRef<'a>>> {
    self
      .sheet_identifier()
      .map(|identifier| log.resolve_sheet(identifier, workbook))
      .transpose()
  }

  /// Resolves the independent `RRDDefName.tabidLocal` relationship. This is
  /// deliberately separate from `RRD.tabid`: 0xFFFF denotes a workbook-
  /// scoped defined name, while any other value addresses the log's
  /// `RRTabId` collection.
  pub fn resolve_defined_name_local_sheet(
    &self,
    log: XlsRevisionLogRef<'a>,
    workbook: &XlsWorkbookView<'a>,
  ) -> Result<Option<XlsSheetRef<'a>>> {
    let Self::DefinedName { value, .. } = self else {
      return Ok(None);
    };
    (value.local_sheet_id != u16::MAX)
      .then(|| log.resolve_sheet(value.local_sheet_id, workbook))
      .transpose()
  }

  /// Resolves `RRDUserView.guid` to the aggregate custom view rooted at the
  /// Globals `UserBView` record.
  pub fn resolve_custom_view<'view>(
    &self,
    workbook: &'view XlsWorkbookView<'a>,
  ) -> Result<Option<&'view XlsCustomViewRef<'a>>> {
    let Self::UserView { value, .. } = self else {
      return Ok(None);
    };
    workbook.resolve_custom_view(value.view_guid).map(Some)
  }

  pub fn resolve_custom_view_compatible<'view>(
    &self,
    workbook: &'view XlsWorkbookView<'a>,
  ) -> Option<XlsCustomViewLink<'view, 'a>> {
    let Self::UserView { value, .. } = self else {
      return None;
    };
    Some(workbook.resolve_custom_view_compatible(value.view_guid))
  }
}

impl<'a> XlsRevisionCellOrFormatRef<'a> {
  pub const fn revision(self) -> &'a Rrd {
    match self {
      Self::ChangeCell(value) => &value.value.revision,
      Self::Format { value, .. } => &value.revision,
    }
  }

  pub const fn sheet_identifier(self) -> Option<u16> {
    let revision = self.revision();
    if revision.sheet_id == u16::MAX {
      None
    } else {
      Some(revision.sheet_id)
    }
  }

  pub fn resolve_sheet(
    self,
    log: XlsRevisionLogRef<'a>,
    workbook: &XlsWorkbookView<'a>,
  ) -> Result<Option<XlsSheetRef<'a>>> {
    self
      .sheet_identifier()
      .map(|identifier| log.resolve_sheet(identifier, workbook))
      .transpose()
  }
}

impl<'a> XlsRevisionInsertDeleteRef<'a> {
  pub const fn begin_record(&self) -> Option<&'a BiffRecord> {
    self.begin_record
  }

  pub const fn operation_record(&self) -> &'a BiffRecord {
    self.operation_record
  }

  pub const fn operation(&self) -> &'a RrdInsDelRecord {
    self.operation
  }

  pub fn changes(&self) -> &[XlsRevisionCellOrFormatRef<'a>] {
    &self.changes
  }

  pub const fn end_record(&self) -> Option<&'a BiffRecord> {
    self.end_record
  }

  pub const fn is_delete(&self) -> bool {
    self.begin_record.is_some()
  }
}

impl<'a> XlsRevisionMoveRef<'a> {
  pub const fn begin_record(&self) -> &'a BiffRecord {
    self.begin_record
  }

  pub const fn move_record(&self) -> &'a BiffRecord {
    self.move_record
  }

  pub const fn value(&self) -> &'a RrdMoveRecord {
    self.value
  }

  pub fn changes(&self) -> &[XlsRevisionCellOrFormatRef<'a>] {
    &self.changes
  }

  pub const fn end_record(&self) -> Option<&'a BiffRecord> {
    self.end_record
  }

  pub fn resolve_source_sheet(
    &self,
    log: XlsRevisionLogRef<'a>,
    workbook: &XlsWorkbookView<'a>,
  ) -> Result<Option<XlsSheetRef<'a>>> {
    (self.value.source_sheet_id != u16::MAX)
      .then(|| log.resolve_sheet(self.value.source_sheet_id, workbook))
      .transpose()
  }
}

impl<'a> XlsRevisionChangeCellRef<'a> {
  pub const fn source_record(self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn value(self) -> &'a RrdChgCellRecord {
    self.value
  }

  pub const fn font_reset_records(self) -> &'a [BiffRecord] {
    self.font_reset_records
  }
}

impl<'view, 'a> XlsRevisionGraph<'view, 'a> {
  pub const fn stream(&self) -> &XlsRevisionStreamView<'a> {
    &self.stream
  }

  pub fn logs(&self) -> &[XlsRevisionGraphLog<'view, 'a>] {
    &self.logs
  }
}

impl<'view, 'a> XlsRevisionGraphLog<'view, 'a> {
  pub const fn source(&self) -> XlsRevisionLogRef<'a> {
    self.source
  }

  pub fn revisions(&self) -> &[XlsRevisionNode<'view, 'a>] {
    &self.revisions
  }

  pub fn unlinked_records(&self) -> &[&'a BiffRecord] {
    &self.unlinked_records
  }
}

impl<'view, 'a> XlsRevisionNode<'view, 'a> {
  pub const fn source(&self) -> &XlsRevisionRecordRef<'a> {
    &self.source
  }

  pub const fn sheet(&self) -> &XlsRevisionSheetLink<'a> {
    &self.sheet
  }

  pub const fn local_name_sheet(&self) -> &XlsRevisionSheetLink<'a> {
    &self.local_name_sheet
  }

  pub const fn custom_view(&self) -> Option<XlsCustomViewLink<'view, 'a>> {
    self.custom_view
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XlsPivotCache {
  Parsed {
    stream_id: u16,
    stream: PivotCacheStream,
  },
  Compatibility {
    stream_id: u16,
    bytes: Vec<u8>,
    reason: String,
  },
}

impl XlsPivotCache {
  pub const fn stream_id(&self) -> u16 {
    match self {
      Self::Parsed { stream_id, .. } | Self::Compatibility { stream_id, .. } => *stream_id,
    }
  }

  pub const fn stream(&self) -> Option<&PivotCacheStream> {
    match self {
      Self::Parsed { stream, .. } => Some(stream),
      Self::Compatibility { .. } => None,
    }
  }

  pub fn stream_mut(&mut self) -> Option<&mut PivotCacheStream> {
    match self {
      Self::Parsed { stream, .. } => Some(stream),
      Self::Compatibility { .. } => None,
    }
  }
}

impl BiffSubstreamKind {
  pub const fn from_document_type(value: u16) -> Self {
    match value {
      0x0005 => Self::WorkbookGlobals,
      0x0010 => Self::WorksheetOrDialogSheet,
      0x0020 => Self::ChartSheet,
      0x0040 => Self::MacroSheet,
      value => Self::Compatibility(value),
    }
  }
}

impl BiffSubstreamNode {
  pub fn records<'a>(&self, tree: &'a BiffWorkbookTree) -> Option<&'a [BiffRecord]> {
    tree.stream.records.get(self.record_range.clone())
  }
}

impl<'a> XlsFileEntryRef<'a> {
  pub const fn entry(self) -> &'a Entry {
    self.entry
  }

  pub const fn role(self) -> XlsFileEntryRole {
    self.role
  }
}

impl<'a> XlsStoragesAndStreams<'a> {
  pub fn entries(&self) -> &[XlsFileEntryRef<'a>] {
    &self.entries
  }

  pub fn issues(&self) -> &[XlsFileEntryIssue] {
    &self.issues
  }

  pub fn by_role(&self, role: XlsFileEntryRole) -> impl Iterator<Item = XlsFileEntryRef<'a>> + '_ {
    self
      .entries
      .iter()
      .copied()
      .filter(move |entry| entry.role == role)
  }

  pub fn embedding_storages(&self) -> impl Iterator<Item = XlsFileEntryRef<'a>> + '_ {
    self
      .entries
      .iter()
      .copied()
      .filter(|entry| matches!(entry.role, XlsFileEntryRole::EmbeddingStorage { .. }))
  }

  pub fn link_storages(&self) -> impl Iterator<Item = XlsFileEntryRef<'a>> + '_ {
    self
      .entries
      .iter()
      .copied()
      .filter(|entry| matches!(entry.role, XlsFileEntryRole::LinkStorage { .. }))
  }

  pub fn pivot_cache_streams(&self) -> impl Iterator<Item = XlsFileEntryRef<'a>> + '_ {
    self
      .entries
      .iter()
      .copied()
      .filter(|entry| matches!(entry.role, XlsFileEntryRole::PivotCacheStream { .. }))
  }

  /// Resolves an object data location across the sheet Obj, workbook link
  /// table, and file-level CFB inventory without interpreting ActiveX/OLE
  /// payload bytes as BIFF.
  pub fn resolve_object_persistence<'view>(
    &self,
    workbook: &'view XlsWorkbookView<'a>,
    object: XlsObjectRef<'a>,
  ) -> Result<Option<XlsObjectPersistenceRef<'view, 'a>>> {
    self.resolve_object_persistence_with_policy(workbook, object, false)
  }

  /// Preserves the narrow producer deviation where FtPioGrbit.fDde is set
  /// even though FtPictFmla begins with PtgTbl and therefore addresses an
  /// embedding/control location (Apache POI corpus 60460.xls).
  pub fn resolve_object_persistence_compatible<'view>(
    &self,
    workbook: &'view XlsWorkbookView<'a>,
    object: XlsObjectRef<'a>,
  ) -> Result<Option<XlsObjectPersistenceRef<'view, 'a>>> {
    self.resolve_object_persistence_with_policy(workbook, object, true)
  }

  fn resolve_object_persistence_with_policy<'view>(
    &self,
    workbook: &'view XlsWorkbookView<'a>,
    object: XlsObjectRef<'a>,
    preserve_compatibility: bool,
  ) -> Result<Option<XlsObjectPersistenceRef<'view, 'a>>> {
    let Some(flags) = object.picture_flags() else {
      return Ok(None);
    };
    let Some(formula) = object.picture_formula() else {
      return Ok(None);
    };
    let common = object.common().ok_or_else(|| {
      Error::invalid(
        u64::from(object.source_record().offset),
        "persisted Obj is missing its FtCmo common subrecord",
      )
    })?;
    if common.object_type != 8 {
      return Err(Error::invalid(
        u64::from(object.source_record().offset),
        format!(
          "persisted Obj cmo.ot is {}, expected picture type 8",
          common.object_type
        ),
      ));
    }
    let formula_starts_with_table = matches!(
        &formula.formula.data,
        ObjFormulaData::Parsed { tokens, .. }
            if matches!(tokens.tokens.first().map(|token| &token.data), Some(FormulaTokenData::Table { .. }))
    );
    if flags.contains(ObjPictureFlags::DDE) && formula_starts_with_table && !preserve_compatibility
    {
      return Err(Error::invalid(
        u64::from(object.source_record().offset),
        "Obj sets FtPioGrbit.fDde but FtPictFmla begins with PtgTbl",
      ));
    }

    if flags.contains(ObjPictureFlags::CONTROL_STREAM) {
      let offset = formula.control_stream_position.ok_or_else(|| {
        Error::invalid(
          u64::from(object.source_record().offset),
          "ActiveX Obj is missing FtPictFmla.lPosInCtlStm",
        )
      })?;
      let size = formula.control_stream_size.ok_or_else(|| {
        Error::invalid(
          u64::from(object.source_record().offset),
          "ActiveX Obj is missing FtPictFmla.cbBufInCtlStm",
        )
      })?;
      let stream = self.unique_entry(XlsFileEntryRole::ControlStream, "Ctls")?;
      let start = usize::try_from(offset)
        .map_err(|_| Error::Limit("Ctls object offset exceeds usize".into()))?;
      let size =
        usize::try_from(size).map_err(|_| Error::Limit("Ctls object size exceeds usize".into()))?;
      let end = start
        .checked_add(size)
        .ok_or_else(|| Error::Limit("Ctls object range overflow".into()))?;
      let data = stream.data.get(start..end).ok_or_else(|| {
        Error::invalid(
          u64::from(object.source_record().offset),
          "ActiveX Obj range is outside the Ctls stream",
        )
      })?;
      return Ok(Some(XlsObjectPersistenceRef::ControlStream {
        stream,
        offset,
        data,
      }));
    }

    if flags.contains(ObjPictureFlags::DDE) && !formula_starts_with_table {
      let ObjFormulaData::Parsed { tokens, .. } = &formula.formula.data else {
        return Err(Error::invalid(
          u64::from(object.source_record().offset),
          "linked Obj formula is not statically parsed",
        ));
      };
      let token = tokens
        .tokens
        .iter()
        .find(|token| matches!(token.data, FormulaTokenData::ExternalName { .. }))
        .ok_or_else(|| {
          Error::invalid(
            u64::from(object.source_record().offset),
            format!(
              "linked Obj formula does not reference an ExternName: {:?}",
              tokens.tokens
            ),
          )
        })?;
      let external_name = workbook
        .resolve_formula_token_external_name(&token.data)?
        .ok_or_else(|| Error::invalid(0, "linked Obj token is not PtgNameX"))?;
      let ExternNameBody::OleDdeLink(link) = &external_name.name().body else {
        return Err(Error::invalid(
          u64::from(object.source_record().offset),
          "linked Obj ExternName is not an ExternOleDdeLink",
        ));
      };
      if link.storage_id == 0 {
        return Ok(Some(XlsObjectPersistenceRef::DdeDataItem { external_name }));
      }
      let role = XlsFileEntryRole::LinkStorage {
        object_id: link.storage_id,
      };
      let storage = self.unique_entry(role, "Link Storage")?;
      return Ok(Some(XlsObjectPersistenceRef::LinkStorage {
        storage_id: link.storage_id,
        storage,
        entries: self.storage_entries(storage),
        external_name,
      }));
    }

    let Some(storage_id) = formula.control_stream_position else {
      return Ok(None);
    };
    let role = XlsFileEntryRole::EmbeddingStorage {
      object_id: storage_id,
    };
    let storage = self.unique_entry(role, "Embedding Storage")?;
    Ok(Some(XlsObjectPersistenceRef::EmbeddingStorage {
      storage_id,
      storage,
      entries: self.storage_entries(storage),
    }))
  }

  fn unique_entry(&self, role: XlsFileEntryRole, name: &str) -> Result<&'a Entry> {
    let mut entries = self.by_role(role).map(XlsFileEntryRef::entry);
    let first = entries
      .next()
      .ok_or_else(|| Error::invalid(0, format!("missing {name}")))?;
    if entries.next().is_some() {
      return Err(Error::invalid(0, format!("multiple entries match {name}")));
    }
    Ok(first)
  }

  fn storage_entries(&self, storage: &'a Entry) -> Vec<&'a Entry> {
    self
      .entries
      .iter()
      .map(|entry| entry.entry)
      .filter(|entry| entry.path == storage.path || entry.path.starts_with(&storage.path))
      .collect()
  }
}

impl XlsSupportingLinkId {
  pub const fn index(self) -> usize {
    self.0
  }
}

impl<'a> XlsSupportingLinkRef<'a> {
  pub const fn id(&self) -> XlsSupportingLinkId {
    self.id
  }

  pub const fn source_record(&self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn value(&self) -> &'a SupBookRecord {
    self.value
  }

  /// Complete contiguous MS-XLS `SUPBOOK` production beginning at SupBook.
  pub const fn records(&self) -> &'a [BiffRecord] {
    self.records
  }

  pub fn external_name_records(&self) -> &[&'a BiffRecord] {
    &self.external_name_records
  }

  /// ExternName records directly following this SupBook, in one-based
  /// PtgNameX `nameindex` order.
  pub fn external_names(&self) -> &[&'a ExternNameRecord] {
    &self.external_names
  }

  pub fn external_name(&self, one_based_index: u32) -> Option<&'a ExternNameRecord> {
    usize::try_from(one_based_index)
      .ok()
      .and_then(|index| index.checked_sub(1))
      .and_then(|index| self.external_names.get(index).copied())
  }

  pub fn external_cell_caches(&self) -> &[XlsExternalCellCacheRef<'a>] {
    &self.external_cell_caches
  }

  pub fn extern_sheet_records(&self) -> &[&'a BiffRecord] {
    &self.extern_sheet_records
  }

  pub fn continuation_records(&self) -> &[&'a BiffRecord] {
    &self.continuation_records
  }

  pub fn external_cache_sheet_name(
    &self,
    cache: &XlsExternalCellCacheRef<'a>,
  ) -> Option<&'a SupBookSheetName> {
    if cache.supporting_link != self.id {
      return None;
    }
    let super::SupBookLink::VirtualPath { sheet_names, .. } = &self.value.link else {
      return None;
    };
    sheet_names.get(usize::from(cache.value.sheet_table_index))
  }
}

impl<'a> XlsExternalCellCacheRef<'a> {
  pub const fn supporting_link_id(&self) -> XlsSupportingLinkId {
    self.supporting_link
  }

  pub const fn source_record(&self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn value(&self) -> &'a XctRecord {
    self.value
  }

  pub fn crn_records(&self) -> &[&'a BiffRecord] {
    &self.crn_records
  }

  pub fn crns(&self) -> impl ExactSizeIterator<Item = &'a super::CrnRecord> + '_ {
    self.crn_records.iter().map(|record| {
      let BiffRecordData::Crn(value) = &record.data else {
        unreachable!("XCT cache records are filtered to CRN")
      };
      value
    })
  }

  pub fn cell(&self, row: u16, column: u8) -> Option<&'a BiffConstant> {
    self.crns().find_map(|record| {
      if record.row != row || column < record.first_column || column > record.last_column {
        return None;
      }
      record.values.get(usize::from(column - record.first_column))
    })
  }
}

impl<'a> XlsExternalSheetRef<'a> {
  pub const fn index(self) -> u16 {
    self.index
  }

  pub const fn source(self) -> &'a ExternSheetReference {
    self.source
  }

  /// Physical ExternSheet record that owns this XTI. Compatibility files
  /// can contain several such records even though formulas address one
  /// logical XTI collection.
  pub const fn source_record(self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn source_reference_index(self) -> u16 {
    self.source_reference_index
  }

  pub const fn supporting_link_id(self) -> Option<XlsSupportingLinkId> {
    self.supporting_link
  }
}

impl<'view, 'a> XlsExternalNameRef<'view, 'a> {
  pub const fn external_sheet(self) -> XlsExternalSheetRef<'a> {
    self.external_sheet
  }

  pub const fn supporting_link(self) -> &'view XlsSupportingLinkRef<'a> {
    self.supporting_link
  }

  pub const fn name(self) -> &'a ExternNameRecord {
    self.name
  }
}

impl XlsPivotCacheDefinitionId {
  pub const fn index(self) -> usize {
    self.0
  }
}

impl<'a> XlsPivotCacheDefinitionRef<'a> {
  pub const fn id(self) -> XlsPivotCacheDefinitionId {
    self.id
  }

  pub const fn source_record(self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn value(self) -> &'a SxStreamIdRecord {
    self.value
  }

  pub const fn stream_id(self) -> u16 {
    self.value.stream_id
  }

  pub const fn source_type_record(self) -> Option<&'a BiffRecord> {
    self.source_type_record
  }

  pub const fn source_type(self) -> Option<&'a SxVsRecord> {
    self.source_type
  }
}

impl<'a> XlsPivotTableViewRef<'a> {
  pub const fn sheet(self) -> XlsSheetRef<'a> {
    self.sheet
  }

  pub const fn source_record(self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn value(self) -> &'a SxViewRecord {
    self.value
  }

  pub fn cache_definition_id(self) -> Result<XlsPivotCacheDefinitionId> {
    usize::try_from(self.value.cache_index)
      .map(XlsPivotCacheDefinitionId)
      .map_err(|_| {
        Error::invalid(
          u64::from(self.source_record.offset),
          "SxView.iCache is negative",
        )
      })
  }
}

impl<'a> XlsPivotTableRef<'a> {
  pub const fn view(self) -> XlsPivotTableViewRef<'a> {
    self.view
  }

  pub const fn sheet(self) -> XlsSheetRef<'a> {
    self.view.sheet
  }

  pub const fn definition(self) -> XlsPivotCacheDefinitionRef<'a> {
    self.definition
  }

  pub const fn cache(self) -> &'a XlsPivotCache {
    self.cache
  }

  pub const fn cache_stream(self) -> Option<&'a PivotCacheStream> {
    self.cache.stream()
  }
}

impl XlsSheetId {
  const fn tab_id_value(value: u16) -> Self {
    Self {
      value: value as u32,
      kind: XlsSheetIdKind::TabId,
    }
  }

  fn compatibility_position(position: usize) -> Result<Self> {
    let Ok(position) = u32::try_from(position) else {
      return Err(Error::Limit(
        "XLS sheet position exceeds the typed identity range".into(),
      ));
    };
    let Some(value) = position.checked_add(1) else {
      return Err(Error::Limit(
        "XLS sheet position exceeds the typed identity range".into(),
      ));
    };
    Ok(Self {
      value,
      kind: XlsSheetIdKind::CompatibilityPosition,
    })
  }

  fn from_sheet_ordinal(position: usize) -> Result<Self> {
    let mut value = Self::compatibility_position(position)?;
    value.kind = XlsSheetIdKind::SheetOrdinal;
    Ok(value)
  }

  pub const fn value(self) -> u32 {
    self.value
  }

  /// Returns the official MS-XLS `TabId`, or `None` when this handle was
  /// synthesized solely to preserve navigation for a compatibility input.
  pub const fn tab_id(self) -> Option<u16> {
    match self.kind {
      XlsSheetIdKind::TabId => Some(self.value as u16),
      XlsSheetIdKind::SheetOrdinal | XlsSheetIdKind::CompatibilityPosition => None,
    }
  }

  /// Returns the official one-based BoundSheet8 ordinal used instead of
  /// RRTabId for workbooks with more than 4,112 sheets.
  pub const fn sheet_ordinal(self) -> Option<u32> {
    match self.kind {
      XlsSheetIdKind::SheetOrdinal => Some(self.value),
      XlsSheetIdKind::TabId | XlsSheetIdKind::CompatibilityPosition => None,
    }
  }

  pub const fn is_specification_identity(self) -> bool {
    !matches!(self.kind, XlsSheetIdKind::CompatibilityPosition)
  }
}

impl<'a> XlsSheetRef<'a> {
  pub const fn id(self) -> XlsSheetId {
    self.id
  }

  /// Exact Globals record that owns this sheet relationship.
  pub const fn metadata_record(self) -> &'a BiffRecord {
    self.metadata_record
  }

  pub const fn metadata(self) -> &'a BoundSheet8Record {
    self.metadata
  }

  pub const fn kind(self) -> BiffSubstreamKind {
    self.substream.kind
  }

  pub const fn substream(self) -> &'a BiffSubstreamNode {
    self.substream
  }

  /// Exact BOF-through-EOF BIFF records for this sheet.
  pub const fn records(self) -> &'a [BiffRecord] {
    self.records
  }

  /// Records owned directly by this substream, excluding complete nested
  /// chart substreams while preserving them through [`Self::substream`].
  pub fn direct_records(self) -> impl Iterator<Item = &'a BiffRecord> {
    self
      .records
      .iter()
      .enumerate()
      .filter(move |(index, _)| self.is_direct_record_index(*index))
      .map(|(_, record)| record)
  }

  fn is_direct_record_index(self, relative_index: usize) -> bool {
    let absolute_index = self.substream.record_range.start + relative_index;
    !is_nested_substream_record(self.substream, absolute_index)
  }

  /// Joins every complete `UserSViewBegin ... UserSViewEnd` production in
  /// this sheet. Unmatched delimiters are retained separately so compatible
  /// callers never have to guess a collection boundary.
  pub fn custom_sheet_views_compatible(
    self,
  ) -> (Vec<XlsCustomSheetViewRef<'a>>, Vec<&'a BiffRecord>) {
    let mut views = Vec::new();
    let mut unlinked_records = Vec::new();
    let mut cursor = 0usize;
    while cursor < self.records.len() {
      if !self.is_direct_record_index(cursor) {
        cursor += 1;
        continue;
      }
      let begin_record = &self.records[cursor];
      let begin = match &begin_record.data {
        BiffRecordData::UserSViewBegin(value) => XlsCustomSheetViewBeginRef::Sheet(value),
        BiffRecordData::UserSViewBeginChart(value) => XlsCustomSheetViewBeginRef::Chart(value),
        BiffRecordData::UserSViewEnd(_) => {
          unlinked_records.push(begin_record);
          cursor += 1;
          continue;
        }
        _ => {
          cursor += 1;
          continue;
        }
      };
      let content_start = cursor + 1;
      let mut end_index = None;
      let mut next_begin = None;
      for index in content_start..self.records.len() {
        if !self.is_direct_record_index(index) {
          continue;
        }
        match self.records[index].data {
          BiffRecordData::UserSViewEnd(_) => {
            end_index = Some(index);
            break;
          }
          BiffRecordData::UserSViewBegin(_) | BiffRecordData::UserSViewBeginChart(_) => {
            next_begin = Some(index);
            break;
          }
          _ => {}
        }
      }
      let Some(end_index) = end_index else {
        unlinked_records.push(begin_record);
        cursor = next_begin.unwrap_or(self.records.len());
        continue;
      };
      let end_record = &self.records[end_index];
      let BiffRecordData::UserSViewEnd(end) = &end_record.data else {
        unreachable!("custom-view end record was selected by its static variant")
      };
      views.push(XlsCustomSheetViewRef {
        sheet: self,
        begin_record,
        begin,
        content_records: &self.records[content_start..end_index],
        end_record,
        end,
      });
      cursor = end_index + 1;
    }
    (views, unlinked_records)
  }

  pub fn custom_sheet_views(self) -> Result<Vec<XlsCustomSheetViewRef<'a>>> {
    let (views, unlinked_records) = self.custom_sheet_views_compatible();
    if let Some(record) = unlinked_records.first() {
      return Err(Error::invalid(
        u64::from(record.offset),
        "sheet contains an unmatched UserSViewBegin/UserSViewEnd delimiter",
      ));
    }
    Ok(views)
  }

  /// Cell-bearing BIFF records without flattening MULRK or MULBLANK into a
  /// lossy grid projection.
  pub fn cell_records(self) -> impl Iterator<Item = &'a BiffRecord> {
    self.direct_records().filter(|record| {
      matches!(
        record.data,
        BiffRecordData::Formula(_)
          | BiffRecordData::Formula4Compatibility(_)
          | BiffRecordData::Blank(_)
          | BiffRecordData::Number(_)
          | BiffRecordData::BoolErr(_)
          | BiffRecordData::Label(_)
          | BiffRecordData::LabelSst(_)
          | BiffRecordData::Rk(_)
          | BiffRecordData::MulRk(_)
          | BiffRecordData::MulBlank(_)
      )
    })
  }

  /// Iterates logical cells while retaining the exact source record for
  /// every value. Invalid MULRK/MULBLANK cardinality is surfaced instead of
  /// truncating or inventing column positions.
  pub fn cells(self) -> XlsCells<'a> {
    XlsCells {
      records: self.records.iter().enumerate(),
      substream: self.substream,
      pending: None,
    }
  }

  /// Physical Row records in sheet order. MS-XLS permits cell tables whose
  /// cells have no Row record, so this is intentionally separate from the
  /// logical row inventory in [`Self::sparse_cell_index`].
  pub fn row_records(self) -> impl Iterator<Item = &'a RowRecord> {
    self
      .direct_records()
      .filter_map(|record| match &record.data {
        BiffRecordData::Row(value) => Some(value),
        _ => None,
      })
  }

  pub fn row_record(self, row: u16) -> Result<Option<&'a RowRecord>> {
    let mut rows = self.row_records().filter(|value| value.row == row);
    let first = rows.next();
    if rows.next().is_some() {
      return Err(Error::invalid(
        0,
        format!("sheet contains multiple Row records for row {row}"),
      ));
    }
    Ok(first)
  }

  pub fn column_infos(self) -> impl Iterator<Item = &'a ColInfoRecord> {
    self
      .direct_records()
      .filter_map(|record| match &record.data {
        BiffRecordData::ColInfo(value) => Some(value),
        _ => None,
      })
  }

  /// Resolves the unique ColInfo range that contains a column. Overlapping
  /// ranges are reported as ambiguous instead of selecting by record order.
  pub fn column_info(self, column: u16) -> Result<Option<&'a ColInfoRecord>> {
    let mut values = self
      .column_infos()
      .filter(|value| (value.first_column..=value.last_column).contains(&column));
    let first = values.next();
    if values.next().is_some() {
      return Err(Error::invalid(
        0,
        format!("column {column} is covered by multiple ColInfo records"),
      ));
    }
    Ok(first)
  }

  /// Builds a sparse lookup index while retaining exact source records and
  /// synthesizing logical entries only for MULRK/MULBLANK elements.
  pub fn sparse_cell_index(self) -> Result<XlsSparseCellIndex<'a>> {
    self.sparse_cell_index_with_policy(false)
  }

  /// Preserves duplicate Row/cell coordinates as explicit source lists.
  pub fn sparse_cell_index_compatible(self) -> Result<XlsSparseCellIndex<'a>> {
    self.sparse_cell_index_with_policy(true)
  }

  fn sparse_cell_index_with_policy(
    self,
    preserve_compatibility: bool,
  ) -> Result<XlsSparseCellIndex<'a>> {
    let mut cells = BTreeMap::new();
    let mut rows = BTreeMap::new();
    for definition in self.row_records() {
      let definitions = rows.entry(definition.row).or_insert_with(Vec::new);
      if !definitions.is_empty() && !preserve_compatibility {
        return Err(Error::invalid(
          0,
          format!(
            "sheet contains multiple Row records for row {}",
            definition.row
          ),
        ));
      }
      definitions.push(definition);
    }
    let mut cell_count = 0usize;
    for cell in self.cells() {
      let cell = cell?;
      let coordinate = (cell.cell.row, cell.cell.column);
      let coordinate_cells = cells.entry(coordinate).or_insert_with(Vec::new);
      if !coordinate_cells.is_empty() && !preserve_compatibility {
        return Err(Error::invalid(
          u64::from(cell.source_record.offset),
          format!(
            "sheet contains multiple logical cells at ({}, {})",
            coordinate.0, coordinate.1
          ),
        ));
      }
      coordinate_cells.push(cell);
      cell_count += 1;
      rows.entry(coordinate.0).or_insert_with(Vec::new);
    }
    let mut formula_groups = BTreeMap::<_, Vec<_>>::new();
    let mut previous: Option<&BiffRecord> = None;
    for record in self.direct_records() {
      if let BiffRecordData::Table(value) = &record.data {
        formula_groups
          .entry((
            value.range.first_row,
            u16::from(value.range.first_column),
            XlsFormulaGroupTokenKind::Table,
          ))
          .or_default()
          .push(XlsFormulaDefinitionRef::Table(value));
      }
      if let Some(previous_record) = previous {
        let locator = match &previous_record.data {
          BiffRecordData::Formula(value) | BiffRecordData::Formula4Compatibility(value) => {
            Some(value.cell)
          }
          _ => None,
        };
        if let Some(locator) = locator {
          let definition = match &record.data {
            BiffRecordData::SharedFormula(value) => Some(XlsFormulaDefinitionRef::Shared(value)),
            BiffRecordData::Array(value) => Some(XlsFormulaDefinitionRef::Array(value)),
            _ => None,
          };
          if let Some(definition) = definition {
            formula_groups
              .entry((locator.row, locator.column, XlsFormulaGroupTokenKind::Exp))
              .or_default()
              .push(definition);
          }
        }
      }
      previous = Some(record);
    }
    Ok(XlsSparseCellIndex {
      sheet: self,
      cells,
      rows,
      formula_groups,
      cell_count,
    })
  }

  pub fn merge_records(self) -> impl Iterator<Item = &'a MergeCellsRecord> {
    self
      .direct_records()
      .filter_map(|record| match &record.data {
        BiffRecordData::MergeCells(value) => Some(value),
        _ => None,
      })
  }

  /// MS-XLS `MergeCells.rgref` values in physical record order. The source
  /// grouping remains available through [`Self::merge_records`].
  pub fn merged_cells(self) -> impl Iterator<Item = &'a CellRange> {
    self.merge_records().flat_map(|record| record.ranges.iter())
  }

  pub fn hyperlinks(self) -> Result<Vec<XlsHyperlinkRef<'a>>> {
    self
      .direct_records()
      .filter_map(|record| match &record.data {
        BiffRecordData::Hyperlink(value) => Some((record, value)),
        _ => None,
      })
      .map(|(record, value)| make_hyperlink_ref(record, value))
      .collect()
  }

  pub fn comments(self) -> Result<Vec<XlsCommentRef<'a>>> {
    let mut comments = Vec::new();
    for source_record in self.direct_records() {
      let BiffRecordData::MsoDrawing(drawing) = &source_record.data else {
        continue;
      };
      for note_host in &drawing.host_records {
        let MsoDrawingHostData::Note(note) = &note_host.data else {
          continue;
        };
        let mut objects =
          drawing
            .host_records
            .iter()
            .enumerate()
            .filter_map(|(host_index, host)| {
              let value = match &host.data {
                MsoDrawingHostData::Obj(value)
                | MsoDrawingHostData::ObjCompatibility { value, .. } => value,
                _ => return None,
              };
              (value
                .common()
                .is_some_and(|common| common.object_id == note.object_id))
              .then_some((host_index, host, value))
            });
        let Some((object_index, object_host, object_value)) = objects.next() else {
          return Err(Error::invalid(
            u64::from(source_record.offset),
            format!("NoteSh object ID {} has no Obj owner", note.object_id),
          ));
        };
        if objects.next().is_some() {
          return Err(Error::invalid(
            u64::from(source_record.offset),
            format!(
              "NoteSh object ID {} has multiple Obj owners",
              note.object_id
            ),
          ));
        }
        let text_host = drawing.host_records.get(object_index + 1).ok_or_else(|| {
          Error::invalid(
            u64::from(source_record.offset),
            format!("comment Obj {} has no following TxO", note.object_id),
          )
        })?;
        let MsoDrawingHostData::Txo(text_object) = &text_host.data else {
          return Err(Error::invalid(
            u64::from(source_record.offset),
            format!("comment Obj {} is not followed by TxO", note.object_id),
          ));
        };
        let author = decode_biff_unicode_string(&note.author)?;
        let content = decode_xl_string_sequence(
          text_object
            .text_chunks
            .iter()
            .map(|chunk| &chunk.characters),
        )?;
        comments.push(XlsCommentRef {
          source_record,
          note_host,
          note,
          object: XlsObjectRef::new(source_record, Some(object_host), object_value),
          text_host,
          text_object,
          author,
          content,
        });
      }
    }
    Ok(comments)
  }

  pub fn drawings(self) -> impl Iterator<Item = XlsDrawingRef<'a>> {
    self
      .direct_records()
      .filter_map(move |record| match &record.data {
        BiffRecordData::MsoDrawing(value) => Some(XlsDrawingRef {
          sheet: self,
          source_record: record,
          value,
        }),
        _ => None,
      })
  }

  /// Obj records owned by this sheet, including Obj host records interleaved
  /// inside logical MsoDrawing aggregates. Nested chart substreams remain
  /// independently reachable and are not flattened into the parent sheet.
  pub fn objects(self) -> XlsObjects<'a> {
    XlsObjects {
      sheet: self,
      record_index: 0,
      host_record_index: 0,
    }
  }

  pub fn pivot_table_views(self) -> impl Iterator<Item = XlsPivotTableViewRef<'a>> {
    self
      .direct_records()
      .filter_map(move |record| match &record.data {
        BiffRecordData::SxView(value) => Some(XlsPivotTableViewRef {
          sheet: self,
          source_record: record,
          value,
        }),
        _ => None,
      })
  }

  pub fn object(self, id: XlsObjectId) -> Result<Option<XlsObjectRef<'a>>> {
    let mut matches = self.objects().filter(|object| object.id() == Some(id));
    let first = matches.next();
    if matches.next().is_some() {
      return Err(Error::invalid(
        0,
        format!(
          "sheet contains multiple Obj records with cmo.id {}",
          id.value()
        ),
      ));
    }
    Ok(first)
  }

  pub fn resolve_formula(self, formula: &'a FormulaRecord) -> Result<XlsFormulaRef<'a>> {
    self.resolve_formula_with_policy(formula, false, None)
  }

  pub fn resolve_formula_compatible(self, formula: &'a FormulaRecord) -> Result<XlsFormulaRef<'a>> {
    self.resolve_formula_with_policy(formula, true, None)
  }

  fn resolve_formula_with_policy(
    self,
    formula: &'a FormulaRecord,
    preserve_compatibility: bool,
    indexed_definitions: Option<&[XlsFormulaDefinitionRef<'a>]>,
  ) -> Result<XlsFormulaRef<'a>> {
    const SHARED_FORMULA_FLAG: u16 = 1 << 3;

    let (record_index, source_record) = self
      .records
      .iter()
      .enumerate()
      .find(|(index, record)| {
        self.is_direct_record_index(*index)
          && match &record.data {
            BiffRecordData::Formula(value) | BiffRecordData::Formula4Compatibility(value) => {
              std::ptr::eq(value, formula)
            }
            _ => false,
          }
      })
      .ok_or_else(|| Error::invalid(0, "Formula record does not belong to this sheet"))?;

    let cached_string = if matches!(
      formula.cached_result,
      super::FormulaCachedResult::Special(super::FormulaSpecialCachedResult { kind: 0, .. })
    ) {
      let value = self
        .records
        .iter()
        .enumerate()
        .skip(record_index + 1)
        .filter(|(index, _)| self.is_direct_record_index(*index))
        .find(|(_, record)| {
          !matches!(
            record.data,
            BiffRecordData::SharedFormula(_) | BiffRecordData::Array(_) | BiffRecordData::Table(_)
          )
        })
        .and_then(|(_, record)| match &record.data {
          BiffRecordData::StringValue(value) => Some(value),
          _ => None,
        });
      if value.is_none() && !preserve_compatibility {
        return Err(Error::invalid(
          u64::from(source_record.offset),
          "Formula string result has no following String record",
        ));
      }
      value
    } else {
      None
    };

    let group_reference = formula_group_reference(formula);
    let definition = match group_reference {
      None if formula.flags & SHARED_FORMULA_FLAG != 0 && !preserve_compatibility => {
        return Err(Error::invalid(
          u64::from(source_record.offset),
          "Formula.fShrFmla is set but formula does not begin with PtgExp",
        ));
      }
      None => XlsFormulaDefinitionRef::Inline(&formula.tokens),
      Some((row, column, token_kind)) => {
        let table_only = token_kind == XlsFormulaGroupTokenKind::Table;
        let mut first = None;
        let mut ambiguous = false;
        let mut add_definition = |definition| {
          if first.is_some() {
            ambiguous = true;
          } else {
            first = Some(definition);
          }
        };
        if let Some(definitions) = indexed_definitions {
          for &definition in definitions {
            let contains_cell = match definition {
              XlsFormulaDefinitionRef::Shared(value) => cell_in_u8_range(
                &formula.cell,
                value.first_row,
                value.last_row,
                value.first_column,
                value.last_column,
              ),
              XlsFormulaDefinitionRef::Array(value) => cell_in_u8_range(
                &formula.cell,
                value.first_row,
                value.last_row,
                value.first_column,
                value.last_column,
              ),
              XlsFormulaDefinitionRef::Table(value) => cell_in_u8_range(
                &formula.cell,
                value.range.first_row,
                value.range.last_row,
                value.range.first_column,
                value.range.last_column,
              ),
              _ => false,
            };
            if contains_cell {
              add_definition(definition);
            }
          }
        } else if table_only {
          for record in self.direct_records() {
            if let BiffRecordData::Table(value) = &record.data
              && value.range.first_row == row
              && u16::from(value.range.first_column) == column
              && cell_in_u8_range(
                &formula.cell,
                value.range.first_row,
                value.range.last_row,
                value.range.first_column,
                value.range.last_column,
              )
            {
              add_definition(XlsFormulaDefinitionRef::Table(value));
            }
          }
        } else {
          // MS-XLS 2.5.198.58 defines PtgExp through a locator Formula
          // followed immediately by ShrFmla or Array. A shared formula's
          // locator can be anywhere inside its sparse/overlapping range;
          // it is not necessarily the top-left cell.
          let mut direct_records = self.direct_records();
          let mut previous = direct_records.next();
          for record in direct_records {
            let Some(previous_record) = previous.replace(record) else {
              continue;
            };
            let locator = match &previous_record.data {
              BiffRecordData::Formula(value) | BiffRecordData::Formula4Compatibility(value)
                if value.cell.row == row && value.cell.column == column =>
              {
                value
              }
              _ => continue,
            };
            match &record.data {
              BiffRecordData::SharedFormula(value)
                if cell_in_u8_range(
                  &locator.cell,
                  value.first_row,
                  value.last_row,
                  value.first_column,
                  value.last_column,
                ) && cell_in_u8_range(
                  &formula.cell,
                  value.first_row,
                  value.last_row,
                  value.first_column,
                  value.last_column,
                ) =>
              {
                add_definition(XlsFormulaDefinitionRef::Shared(value))
              }
              BiffRecordData::Array(value)
                if value.first_row == row
                  && u16::from(value.first_column) == column
                  && cell_in_u8_range(
                    &formula.cell,
                    value.first_row,
                    value.last_row,
                    value.first_column,
                    value.last_column,
                  ) =>
              {
                add_definition(XlsFormulaDefinitionRef::Array(value))
              }
              _ => {}
            }
          }
        }
        if ambiguous {
          return Err(Error::invalid(
            u64::from(source_record.offset),
            "formula group token resolves to multiple definitions",
          ));
        }
        match first {
          Some(XlsFormulaDefinitionRef::Shared(_))
            if formula.flags & SHARED_FORMULA_FLAG == 0 && !preserve_compatibility =>
          {
            return Err(Error::invalid(
              u64::from(source_record.offset),
              "PtgExp resolves to ShrFmla but Formula.fShrFmla is clear",
            ));
          }
          Some(value) => value,
          None if preserve_compatibility && table_only => {
            XlsFormulaDefinitionRef::UnresolvedTable { row, column }
          }
          None if preserve_compatibility => XlsFormulaDefinitionRef::UnresolvedExp { row, column },
          None => {
            return Err(Error::invalid(
              u64::from(source_record.offset),
              format!(
                "{} ({row}, {column}) has no formula definition",
                if table_only { "PtgTbl" } else { "PtgExp" }
              ),
            ));
          }
        }
      }
    };

    Ok(XlsFormulaRef {
      source_record,
      formula,
      cached_string,
      definition,
    })
  }
}

fn formula_group_reference(
  formula: &FormulaRecord,
) -> Option<(u16, u16, XlsFormulaGroupTokenKind)> {
  formula
    .tokens
    .rgce
    .tokens
    .first()
    .and_then(|token| match token.data {
      FormulaTokenData::Exp { row, column } => Some((row, column, XlsFormulaGroupTokenKind::Exp)),
      FormulaTokenData::Table { row, column } => {
        Some((row, column, XlsFormulaGroupTokenKind::Table))
      }
      _ => None,
    })
}

fn is_nested_substream_record(substream: &BiffSubstreamNode, absolute_index: usize) -> bool {
  substream
    .children
    .iter()
    .any(|child| child.record_range.contains(&absolute_index))
}

fn cell_in_u8_range(
  cell: &CellHeader,
  first_row: u16,
  last_row: u16,
  first_column: u8,
  last_column: u8,
) -> bool {
  (first_row..=last_row).contains(&cell.row)
    && (u16::from(first_column)..=u16::from(last_column)).contains(&cell.column)
}

impl<'a> XlsCellRef<'a> {
  pub const fn source_record(self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn cell(self) -> CellHeader {
    self.cell
  }

  pub const fn value(self) -> XlsCellValueRef<'a> {
    self.value
  }

  pub const fn formula(self) -> Option<&'a FormulaRecord> {
    match self.value {
      XlsCellValueRef::Formula(value) | XlsCellValueRef::Formula4Compatibility(value) => {
        Some(value)
      }
      _ => None,
    }
  }

  pub const fn label_sst(self) -> Option<&'a LabelSstRecord> {
    match self.value {
      XlsCellValueRef::LabelSst(value) => Some(value),
      _ => None,
    }
  }
}

impl<'a> XlsSparseCellIndex<'a> {
  pub fn len(&self) -> usize {
    self.cell_count
  }

  pub fn is_empty(&self) -> bool {
    self.cells.is_empty()
  }

  pub fn cell(&self, row: u16, column: u16) -> Result<Option<XlsCellRef<'a>>> {
    let Some(cells) = self.cells.get(&(row, column)) else {
      return Ok(None);
    };
    let [cell] = cells.as_slice() else {
      return Err(Error::invalid(
        u64::from(cells[1].source_record.offset),
        format!(
          "sheet contains {} logical cells at ({row}, {column})",
          cells.len()
        ),
      ));
    };
    Ok(Some(*cell))
  }

  pub fn cells_at(&self, row: u16, column: u16) -> &[XlsCellRef<'a>] {
    self.cells.get(&(row, column)).map_or(&[], Vec::as_slice)
  }

  fn ensure_cell(&self, cell: XlsCellRef<'a>) -> Result<()> {
    if self
      .cells_at(cell.cell.row, cell.cell.column)
      .iter()
      .any(|candidate| {
        candidate.cell == cell.cell && std::ptr::eq(candidate.source_record, cell.source_record)
      })
    {
      return Ok(());
    }
    Err(Error::invalid(
      u64::from(cell.source_record.offset),
      "cell does not belong to this sparse index",
    ))
  }

  pub fn merged_ranges(&self, cell: XlsCellRef<'a>) -> Result<Vec<&'a CellRange>> {
    self.ensure_cell(cell)?;
    Ok(
      self
        .sheet
        .merged_cells()
        .filter(|range| {
          (range.first_row..=range.last_row).contains(&cell.cell.row)
            && (range.first_column..=range.last_column).contains(&cell.cell.column)
        })
        .collect(),
    )
  }

  pub fn hyperlinks(&self, cell: XlsCellRef<'a>) -> Result<Vec<XlsHyperlinkRef<'a>>> {
    self.ensure_cell(cell)?;
    Ok(
      self
        .sheet
        .hyperlinks()?
        .into_iter()
        .filter(|link| {
          (link.value.first_row..=link.value.last_row).contains(&cell.cell.row)
            && (link.value.first_column..=link.value.last_column).contains(&cell.cell.column)
        })
        .collect(),
    )
  }

  pub fn comment(&self, cell: XlsCellRef<'a>) -> Result<Option<XlsCommentRef<'a>>> {
    self.ensure_cell(cell)?;
    let mut comments = self.sheet.comments()?.into_iter().filter(|comment| {
      comment.note.row == cell.cell.row && comment.note.column == cell.cell.column
    });
    let first = comments.next();
    if comments.next().is_some() {
      return Err(Error::invalid(
        u64::from(cell.source_record.offset),
        format!(
          "cell ({}, {}) has multiple NoteSh comments",
          cell.cell.row, cell.cell.column
        ),
      ));
    }
    Ok(first)
  }

  pub fn duplicate_cells(&self) -> impl Iterator<Item = ((u16, u16), &[XlsCellRef<'a>])> + '_ {
    self.cells.iter().filter_map(|(&coordinate, cells)| {
      (cells.len() > 1).then_some((coordinate, cells.as_slice()))
    })
  }

  /// Resolves Formula/String/ShrFmla/Array/Table relationships through the
  /// prebuilt locator index rather than rescanning the sheet.
  pub fn resolve_cell_formula(&self, cell: XlsCellRef<'a>) -> Result<Option<XlsFormulaRef<'a>>> {
    self.resolve_cell_formula_with_policy(cell, false)
  }

  pub fn resolve_cell_formula_compatible(
    &self,
    cell: XlsCellRef<'a>,
  ) -> Result<Option<XlsFormulaRef<'a>>> {
    self.resolve_cell_formula_with_policy(cell, true)
  }

  fn resolve_cell_formula_with_policy(
    &self,
    cell: XlsCellRef<'a>,
    preserve_compatibility: bool,
  ) -> Result<Option<XlsFormulaRef<'a>>> {
    let Some(formula) = cell.formula() else {
      return Ok(None);
    };
    let indexed_definitions = formula_group_reference(formula)
      .map(|key| self.formula_groups.get(&key).map_or(&[][..], Vec::as_slice));
    self
      .sheet
      .resolve_formula_with_policy(formula, preserve_compatibility, indexed_definitions)
      .map(Some)
  }

  pub fn row(&self, row: u16) -> Option<XlsSparseRowRef<'_, 'a>> {
    self.rows.get(&row).map(|definitions| XlsSparseRowRef {
      index: self,
      row,
      definitions,
    })
  }

  pub fn rows(&self) -> impl Iterator<Item = XlsSparseRowRef<'_, 'a>> + '_ {
    self.rows.iter().map(|(&row, definitions)| XlsSparseRowRef {
      index: self,
      row,
      definitions,
    })
  }
}

impl<'index, 'a> XlsSparseRowRef<'index, 'a> {
  pub const fn row(self) -> u16 {
    self.row
  }

  /// The physical Row record, or `None` for the record-less cell table form
  /// explicitly permitted by MS-XLS 2.1.7.20.6.
  pub fn definition(self) -> Result<Option<&'a RowRecord>> {
    match self.definitions {
      [] => Ok(None),
      [definition] => Ok(Some(*definition)),
      definitions => Err(Error::invalid(
        0,
        format!(
          "sheet contains {} Row records for row {}",
          definitions.len(),
          self.row
        ),
      )),
    }
  }

  pub const fn definitions(self) -> &'index [&'a RowRecord] {
    self.definitions
  }

  pub fn cells(self) -> impl Iterator<Item = XlsCellRef<'a>> + 'index {
    self
      .index
      .cells
      .range((self.row, 0)..=(self.row, u16::MAX))
      .flat_map(|(_, cells)| cells.iter().copied())
  }
}

impl XlsObjectId {
  pub const fn new(value: u16) -> Self {
    Self(value)
  }

  pub const fn value(self) -> u16 {
    self.0
  }
}

impl<'a> XlsDrawingGroupRef<'a> {
  pub const fn source_record(self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn value(self) -> &'a MsoDrawingRecord {
    self.value
  }

  pub const fn office_art(self) -> Option<&'a OfficeArtStream> {
    match &self.value.data {
      MsoDrawingData::Complete(value) => Some(value),
      MsoDrawingData::Partial(_) | MsoDrawingData::Incomplete { .. } => None,
    }
  }
}

impl<'a> XlsCommentRef<'a> {
  pub const fn source_record(&self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn note_host(&self) -> &'a MsoDrawingHostRecord {
    self.note_host
  }

  pub const fn note(&self) -> &'a NoteRecord {
    self.note
  }

  pub const fn object(&self) -> XlsObjectRef<'a> {
    self.object
  }

  pub const fn text_host(&self) -> &'a MsoDrawingHostRecord {
    self.text_host
  }

  pub const fn text_object(&self) -> &'a TxoRecord {
    self.text_object
  }
}

impl<'a> XlsHyperlinkRef<'a> {
  pub const fn source_record(&self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn value(&self) -> &'a HyperlinkRecord {
    self.value
  }
}

impl<'a> XlsDrawingRef<'a> {
  pub const fn sheet(self) -> XlsSheetRef<'a> {
    self.sheet
  }

  pub const fn source_record(self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn value(self) -> &'a MsoDrawingRecord {
    self.value
  }

  pub const fn office_art(self) -> Option<&'a OfficeArtStream> {
    match &self.value.data {
      MsoDrawingData::Complete(value) => Some(value),
      MsoDrawingData::Partial(_) | MsoDrawingData::Incomplete { .. } => None,
    }
  }

  pub fn host_records(self) -> impl Iterator<Item = &'a MsoDrawingHostRecord> {
    self.value.host_records.iter()
  }

  pub fn objects(self) -> impl Iterator<Item = XlsObjectRef<'a>> {
    self.value.host_records.iter().filter_map(move |host| {
      let value = match &host.data {
        MsoDrawingHostData::Obj(value) | MsoDrawingHostData::ObjCompatibility { value, .. } => {
          value
        }
        _ => return None,
      };
      Some(XlsObjectRef::new(self.source_record, Some(host), value))
    })
  }
}

impl<'a> XlsPictureRef<'a> {
  pub const fn sheet(self) -> XlsSheetRef<'a> {
    self.sheet
  }

  /// Zero-based non-patriarch shape order within the worksheet drawing.
  pub const fn drawing_order(self) -> usize {
    self.drawing_order
  }

  pub const fn shape_type(self) -> u16 {
    self.shape_type
  }

  pub const fn shape(self) -> &'a OfficeArtShape {
    self.shape
  }

  pub fn crop(self) -> XlsPictureCrop {
    XlsPictureCrop {
      top: xls_signed_shape_property_or(self.properties, 0x0100, 0),
      bottom: xls_signed_shape_property_or(self.properties, 0x0101, 0),
      left: xls_signed_shape_property_or(self.properties, 0x0102, 0),
      right: xls_signed_shape_property_or(self.properties, 0x0103, 0),
    }
  }

  pub const fn anchor(self) -> OfficeArtClientAnchor {
    self.anchor
  }

  pub const fn blip_identifier(self) -> u32 {
    self.blip_identifier
  }

  pub const fn image(self) -> XlsPictureImageLink<'a> {
    self.image
  }
}

impl XlsPictureCrop {
  pub const fn top(self) -> i32 {
    self.top
  }

  pub const fn bottom(self) -> i32 {
    self.bottom
  }

  pub const fn left(self) -> i32 {
    self.left
  }

  pub const fn right(self) -> i32 {
    self.right
  }
}

fn collect_office_art_containers<'a>(
  records: &'a [OfficeArtRecord],
  record_type: u16,
  result: &mut Vec<&'a OfficeArtRecord>,
) {
  for record in records {
    if record.header.record_type == record_type {
      result.push(record);
    }
    if let OfficeArtRecordData::Container(children)
    | OfficeArtRecordData::CompatibilityContainer(children) = &record.data
    {
      collect_office_art_containers(children, record_type, result);
    }
  }
}

fn collect_xls_pictures<'a>(
  records: &'a [OfficeArtRecord],
  sheet: XlsSheetRef<'a>,
  blip_store_entries: &'a [OfficeArtRecord],
  drawing_order: &mut usize,
  result: &mut Vec<XlsPictureRef<'a>>,
) -> Result<()> {
  for record in records {
    let children = match &record.data {
      OfficeArtRecordData::Container(children)
      | OfficeArtRecordData::CompatibilityContainer(children) => Some(children.as_slice()),
      _ => None,
    };
    if record.header.record_type == 0xf004
      && let Some(children) = children
    {
      let shape = children.iter().find_map(|child| match &child.data {
        OfficeArtRecordData::Shape(shape) => Some((child.header.instance, shape)),
        _ => None,
      });
      let anchor = children.iter().find_map(|child| match &child.data {
        OfficeArtRecordData::ClientAnchor(anchor) => Some(*anchor),
        _ => None,
      });
      if let Some((shape_type, shape)) = shape
        && !shape
          .flags
          .contains(crate::office_art::OfficeArtShapeFlags::PATRIARCH)
      {
        let current_order = *drawing_order;
        *drawing_order = drawing_order
          .checked_add(1)
          .ok_or_else(|| Error::Limit("XLS drawing order overflow".into()))?;
        if let Some(blip_identifier) = xls_shape_blip_identifier(children)? {
          let anchor = anchor.ok_or_else(|| {
            Error::invalid(
              0,
              format!("XLS picture shape {} has no client anchor", shape.shape_id),
            )
          })?;
          result.push(XlsPictureRef {
            sheet,
            drawing_order: current_order,
            shape_type,
            shape,
            properties: children,
            anchor,
            blip_identifier,
            image: resolve_xls_picture_image(blip_store_entries, blip_identifier),
          });
        }
      }
    }
    if let Some(children) = children {
      collect_xls_pictures(children, sheet, blip_store_entries, drawing_order, result)?;
    }
  }
  Ok(())
}

fn xls_shape_blip_identifier(records: &[OfficeArtRecord]) -> Result<Option<u32>> {
  let property = records
    .iter()
    .filter_map(|record| match &record.data {
      OfficeArtRecordData::PropertyTable(table) => Some(table.properties.as_slice()),
      _ => None,
    })
    .flat_map(|properties| properties.iter())
    .rfind(|property| property.property_id == 0x0104);
  let Some(property) = property else {
    return Ok(None);
  };
  let OfficeArtPropertyValue::Simple(identifier) = property.value else {
    return Err(Error::invalid(
      0,
      "XLS picture BLIP property is not a simple value",
    ));
  };
  if !property.is_blip_id {
    return Err(Error::invalid(
      0,
      "XLS picture BLIP property does not set fBid",
    ));
  }
  Ok((identifier != 0).then_some(identifier))
}

fn xls_signed_shape_property_or(
  records: &[OfficeArtRecord],
  property_id: u16,
  default: i32,
) -> i32 {
  records
    .iter()
    .filter_map(|record| match &record.data {
      OfficeArtRecordData::PropertyTable(table) => Some(table.properties.as_slice()),
      _ => None,
    })
    .flat_map(|properties| properties.iter())
    .rfind(|property| property.property_id == property_id)
    .and_then(|property| match property.value {
      OfficeArtPropertyValue::Simple(value) => Some(i32::from_le_bytes(value.to_le_bytes())),
      _ => None,
    })
    .unwrap_or(default)
}

fn resolve_xls_picture_image<'a>(
  entries: &'a [OfficeArtRecord],
  blip_identifier: u32,
) -> XlsPictureImageLink<'a> {
  let Some(index) = blip_identifier
    .checked_sub(1)
    .and_then(|value| usize::try_from(value).ok())
  else {
    return XlsPictureImageLink::Missing;
  };
  let Some(record) = entries.get(index) else {
    return XlsPictureImageLink::Missing;
  };
  match &record.data {
    OfficeArtRecordData::Fbse(fbse) => {
      if let Some(image) = fbse
        .embedded_blip
        .as_deref()
        .and_then(OfficeArtRecord::image_ref)
      {
        XlsPictureImageLink::Resolved(image)
      } else if fbse.delay_offset != u32::MAX {
        XlsPictureImageLink::Delayed {
          offset: fbse.delay_offset,
        }
      } else {
        XlsPictureImageLink::Unsupported
      }
    }
    _ => record.image_ref().map_or(
      XlsPictureImageLink::Unsupported,
      XlsPictureImageLink::Resolved,
    ),
  }
}

impl<'a> XlsObjectRef<'a> {
  fn new(
    source_record: &'a BiffRecord,
    host_record: Option<&'a MsoDrawingHostRecord>,
    value: &'a ObjRecord,
  ) -> Self {
    Self {
      source_record,
      host_record,
      value,
      common: value.common(),
      picture_flags: value.picture_flags(),
      picture_formula: value.picture_formula(),
    }
  }

  pub const fn source_record(self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn host_record(self) -> Option<&'a MsoDrawingHostRecord> {
    self.host_record
  }

  pub const fn value(self) -> &'a ObjRecord {
    self.value
  }

  pub const fn common(self) -> Option<&'a ObjCommonData> {
    self.common
  }

  pub fn id(self) -> Option<XlsObjectId> {
    self.common.map(|common| XlsObjectId(common.object_id))
  }

  pub const fn picture_flags(self) -> Option<ObjPictureFlags> {
    self.picture_flags
  }

  pub const fn picture_formula(self) -> Option<&'a ObjPictureFormula> {
    self.picture_formula
  }
}

impl<'a> Iterator for XlsObjects<'a> {
  type Item = XlsObjectRef<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    loop {
      let record = self.sheet.records.get(self.record_index)?;
      if !self.sheet.is_direct_record_index(self.record_index) {
        self.record_index += 1;
        self.host_record_index = 0;
        continue;
      }
      if let BiffRecordData::MsoDrawing(drawing) = &record.data {
        while let Some(host) = drawing.host_records.get(self.host_record_index) {
          self.host_record_index += 1;
          let value = match &host.data {
            MsoDrawingHostData::Obj(value) | MsoDrawingHostData::ObjCompatibility { value, .. } => {
              value
            }
            _ => continue,
          };
          return Some(XlsObjectRef::new(record, Some(host), value));
        }
        self.record_index += 1;
        self.host_record_index = 0;
        continue;
      }
      self.record_index += 1;
      self.host_record_index = 0;
      let value = match &record.data {
        BiffRecordData::Obj(value) | BiffRecordData::ObjCompatibility { value, .. } => value,
        _ => continue,
      };
      return Some(XlsObjectRef::new(record, None, value));
    }
  }
}

impl<'a> XlsFormulaRef<'a> {
  pub const fn source_record(self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn formula(self) -> &'a FormulaRecord {
    self.formula
  }

  pub const fn cached_string(self) -> Option<&'a StringValueRecord> {
    self.cached_string
  }

  pub const fn definition(self) -> XlsFormulaDefinitionRef<'a> {
    self.definition
  }

  pub fn cached_value(self) -> Result<XlsFormulaCachedValue> {
    let special = match self.formula.cached_result {
      FormulaCachedResult::NumberBits(bits) => {
        return Ok(XlsFormulaCachedValue::Number(f64::from_bits(bits)));
      }
      FormulaCachedResult::Special(special) => special,
    };
    match special.kind {
      0 => self
        .cached_string
        .ok_or_else(|| {
          Error::invalid(
            u64::from(self.source_record.offset),
            "Formula string result has no following String record",
          )
        })
        .and_then(decode_formula_string)
        .map(XlsFormulaCachedValue::String),
      1 => Ok(XlsFormulaCachedValue::Boolean(special.value != 0)),
      2 => CellErrorCode::from_raw(special.value)
        .map(XlsFormulaCachedValue::Error)
        .ok_or_else(|| {
          Error::invalid(
            u64::from(self.source_record.offset),
            format!(
              "Formula cached error code 0x{:02x} is invalid",
              special.value
            ),
          )
        }),
      3 => Ok(XlsFormulaCachedValue::Empty),
      _ => unreachable!("FormulaCachedResult parsing validates special kinds"),
    }
  }
}

fn decode_formula_string(value: &StringValueRecord) -> Result<String> {
  decode_xl_string_sequence(value.chunks.iter().map(|chunk| &chunk.characters))
}

fn decode_biff_unicode_string(value: &BiffUnicodeString) -> Result<String> {
  decode_xl_string_sequence(std::iter::once(&value.characters))
}

impl TryFrom<&BiffUnicodeString> for String {
  type Error = Error;

  fn try_from(value: &BiffUnicodeString) -> Result<Self> {
    decode_biff_unicode_string(value)
  }
}

fn decode_sst_string(value: &SstString) -> Result<String> {
  decode_xl_string_sequence(value.character_chunks.iter().map(|chunk| &chunk.characters))
}

fn decode_xl_string_sequence<'a>(
  values: impl IntoIterator<Item = &'a XlStringCharacters>,
) -> Result<String> {
  let mut code_units = Vec::new();
  for value in values {
    match value {
      XlStringCharacters::Compressed(bytes) => {
        code_units.extend(bytes.iter().copied().map(u16::from));
      }
      XlStringCharacters::Unicode(units) => code_units.extend_from_slice(units),
    }
  }
  String::from_utf16(&code_units)
    .map_err(|_| Error::invalid(0, "XLS string contains an unpaired UTF-16 surrogate"))
}

fn decode_hyperlink_utf16(code_units: &[u16]) -> Result<String> {
  let value = code_units.strip_suffix(&[0]).unwrap_or(code_units);
  String::from_utf16(value)
    .map_err(|_| Error::invalid(0, "XLS hyperlink contains an unpaired UTF-16 surrogate"))
}

fn make_hyperlink_ref<'a>(
  source_record: &'a BiffRecord,
  value: &'a HyperlinkRecord,
) -> Result<XlsHyperlinkRef<'a>> {
  let HyperlinkObject::Parsed {
    display_name,
    target_frame_name,
    moniker,
    location,
    ..
  } = &value.object
  else {
    return Err(Error::invalid(
      u64::from(source_record.offset),
      "HLink object is not a conforming parsed hyperlink",
    ));
  };
  let display_name = display_name
    .as_ref()
    .map(|value| decode_hyperlink_utf16(&value.characters))
    .transpose()?;
  let target_frame_name = target_frame_name
    .as_ref()
    .map(|value| decode_hyperlink_utf16(&value.characters))
    .transpose()?;
  let location = location
    .as_ref()
    .map(|value| decode_hyperlink_utf16(&value.characters))
    .transpose()?;
  let target = moniker
    .as_deref()
    .map(|moniker| match moniker {
      HyperlinkMoniker::String(value) => {
        decode_hyperlink_utf16(&value.characters).map(XlsHyperlinkTarget::String)
      }
      HyperlinkMoniker::Url { address, .. } => {
        decode_hyperlink_utf16(address).map(XlsHyperlinkTarget::Url)
      }
      HyperlinkMoniker::File {
        short_name,
        long_path,
        ..
      } => Ok(XlsHyperlinkTarget::File {
        short_name,
        long_path: long_path
          .as_ref()
          .map(|value| decode_hyperlink_utf16(&value.characters))
          .transpose()?,
      }),
      HyperlinkMoniker::Standard {
        class_id,
        options,
        data,
        ..
      } => Ok(XlsHyperlinkTarget::Standard {
        class_id: *class_id,
        options: *options,
        data,
      }),
    })
    .transpose()?;
  Ok(XlsHyperlinkRef {
    source_record,
    value,
    display_name,
    target_frame_name,
    location,
    target,
  })
}

fn decode_bool_err(record: &BoolErrRecord) -> XlsCellValue {
  let raw = match record.value {
    BoolErrValue::Byte(value) => u16::from(value),
    BoolErrValue::Word(value) => {
      return XlsCellValue::CompatibilityBoolErr {
        value,
        is_error: record.is_error,
      };
    }
  };
  match (record.is_error, raw) {
    (0, 0) => XlsCellValue::Boolean(false),
    (0, 1) => XlsCellValue::Boolean(true),
    (1, raw) => CellErrorCode::from_raw(raw as u8).map_or(
      XlsCellValue::CompatibilityBoolErr {
        value: raw,
        is_error: record.is_error,
      },
      XlsCellValue::Error,
    ),
    _ => XlsCellValue::CompatibilityBoolErr {
      value: raw,
      is_error: record.is_error,
    },
  }
}

fn decode_rk_number(bits: u32) -> f64 {
  let mut value = if bits & 2 != 0 {
    f64::from((bits as i32) >> 2)
  } else {
    f64::from_bits(u64::from(bits & !3) << 32)
  };
  if bits & 1 != 0 {
    value /= 100.0;
  }
  value
}

impl<'a> Iterator for XlsCells<'a> {
  type Item = Result<XlsCellRef<'a>>;

  fn next(&mut self) -> Option<Self::Item> {
    loop {
      if let Some(pending) = &mut self.pending {
        match pending {
          XlsPendingCells::MulRk {
            source_record,
            value,
            index,
          } => {
            if let Some(cell) = value.cells.get(*index) {
              let cell_index = *index;
              *index += 1;
              return Some(Ok(XlsCellRef {
                source_record,
                cell: CellHeader {
                  row: value.row,
                  column: value.first_column + cell_index as u16,
                  format_index: cell.format_index,
                },
                value: XlsCellValueRef::MulRk {
                  parent: value,
                  index: cell_index,
                  value: cell,
                },
              }));
            }
          }
          XlsPendingCells::MulBlank {
            source_record,
            value,
            index,
          } => {
            if let Some(format_index) = value.format_indices.get(*index) {
              let cell_index = *index;
              *index += 1;
              return Some(Ok(XlsCellRef {
                source_record,
                cell: CellHeader {
                  row: value.row,
                  column: value.first_column + cell_index as u16,
                  format_index: *format_index,
                },
                value: XlsCellValueRef::MulBlank {
                  parent: value,
                  index: cell_index,
                },
              }));
            }
          }
        }
        self.pending = None;
      }

      let (relative_index, record) = self.records.next()?;
      let absolute_index = self.substream.record_range.start + relative_index;
      if is_nested_substream_record(self.substream, absolute_index) {
        continue;
      }
      let direct = match &record.data {
        BiffRecordData::Formula(value) => Some((value.cell, XlsCellValueRef::Formula(value))),
        BiffRecordData::Formula4Compatibility(value) => {
          Some((value.cell, XlsCellValueRef::Formula4Compatibility(value)))
        }
        BiffRecordData::Blank(value) => Some((value.cell, XlsCellValueRef::Blank(value))),
        BiffRecordData::Number(value) => Some((value.cell, XlsCellValueRef::Number(value))),
        BiffRecordData::BoolErr(value) => Some((value.cell, XlsCellValueRef::BoolErr(value))),
        BiffRecordData::Label(value) => Some((value.cell, XlsCellValueRef::Label(value))),
        BiffRecordData::LabelSst(value) => Some((value.cell, XlsCellValueRef::LabelSst(value))),
        BiffRecordData::Rk(value) => Some((value.cell, XlsCellValueRef::Rk(value))),
        BiffRecordData::MulRk(value) => {
          if let Err(error) = validate_mul_cells(
            record.offset,
            value.first_column,
            value.last_column,
            value.cells.len(),
            "MulRk",
          ) {
            return Some(Err(error));
          }
          self.pending = Some(XlsPendingCells::MulRk {
            source_record: record,
            value,
            index: 0,
          });
          continue;
        }
        BiffRecordData::MulBlank(value) => {
          if let Err(error) = validate_mul_cells(
            record.offset,
            value.first_column,
            value.last_column,
            value.format_indices.len(),
            "MulBlank",
          ) {
            return Some(Err(error));
          }
          self.pending = Some(XlsPendingCells::MulBlank {
            source_record: record,
            value,
            index: 0,
          });
          continue;
        }
        _ => None,
      };
      if let Some((cell, value)) = direct {
        return Some(Ok(XlsCellRef {
          source_record: record,
          cell,
          value,
        }));
      }
    }
  }
}

fn validate_mul_cells(
  record_offset: u32,
  first_column: u16,
  last_column: u16,
  count: usize,
  structure: &str,
) -> Result<()> {
  let count = u16::try_from(count)
    .map_err(|_| Error::Limit(format!("{structure} cell count exceeds u16")))?;
  let expected_last = first_column
    .checked_add(count.saturating_sub(1))
    .ok_or_else(|| Error::Limit(format!("{structure} column range overflows u16")))?;
  if count == 0 || expected_last != last_column {
    return Err(Error::invalid(
      u64::from(record_offset),
      format!("{structure} first/last columns do not match its {count} cell values"),
    ));
  }
  Ok(())
}

impl<'a> XlsCustomSheetViewRef<'a> {
  pub const fn sheet(self) -> XlsSheetRef<'a> {
    self.sheet
  }

  pub const fn begin_record(self) -> &'a BiffRecord {
    self.begin_record
  }

  pub const fn begin(self) -> XlsCustomSheetViewBeginRef<'a> {
    self.begin
  }

  pub const fn guid(self) -> [u8; 16] {
    self.begin.guid()
  }

  pub const fn sheet_identifier(self) -> u32 {
    self.begin.sheet_identifier()
  }

  pub const fn content_records(self) -> &'a [BiffRecord] {
    self.content_records
  }

  pub const fn end_record(self) -> &'a BiffRecord {
    self.end_record
  }

  pub const fn end(self) -> &'a UserSViewEndRecord {
    self.end
  }
}

impl XlsCustomSheetViewBeginRef<'_> {
  pub const fn guid(self) -> [u8; 16] {
    match self {
      Self::Sheet(value) => value.guid,
      Self::Chart(value) => value.guid,
    }
  }

  pub const fn sheet_identifier(self) -> u32 {
    match self {
      Self::Sheet(value) => value.sheet_id as u32,
      Self::Chart(value) => value.sheet_id,
    }
  }

  pub const fn is_chart(self) -> bool {
    matches!(self, Self::Chart(_))
  }
}

impl<'a> XlsCustomViewRef<'a> {
  pub const fn source_record(&self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn value(&self) -> &'a UserBViewRecord {
    self.value
  }

  pub const fn guid(&self) -> [u8; 16] {
    self.value.guid
  }

  pub fn sheet_views(&self) -> &[XlsCustomSheetViewRef<'a>] {
    &self.sheet_views
  }

  pub fn defined_names(&self) -> &[XlsCustomViewDefinedNameRef<'a>] {
    &self.defined_names
  }

  pub fn sheet_view(&self, sheet: XlsSheetId) -> Result<Option<XlsCustomSheetViewRef<'a>>> {
    let mut matches = self
      .sheet_views
      .iter()
      .copied()
      .filter(|view| view.sheet.id == sheet);
    let first = matches.next();
    if matches.next().is_some() {
      return Err(Error::invalid(
        u64::from(self.source_record.offset),
        format!(
          "custom view contains multiple UserSViewBegin collections for sheet {}",
          sheet.value()
        ),
      ));
    }
    Ok(first)
  }
}

impl<'a> XlsCustomViewDefinedNameRef<'a> {
  pub const fn source_record(self) -> &'a BiffRecord {
    self.source_record
  }

  pub const fn value(self) -> &'a NameRecord {
    self.value
  }

  pub const fn kind(self) -> XlsCustomViewDefinedNameKind {
    self.kind
  }
}

impl<'a> XlsUnresolvedSheetRef<'a> {
  pub const fn id(self) -> XlsSheetId {
    self.id
  }

  pub const fn metadata_record(self) -> &'a BiffRecord {
    self.metadata_record
  }

  pub const fn metadata(self) -> &'a BoundSheet8Record {
    self.metadata
  }

  pub const fn error(self) -> XlsSheetLinkError {
    self.error
  }
}

impl<'a> XlsWorkbookView<'a> {
  pub const fn workbook(&self) -> &'a XlsWorkbookStream {
    self.workbook
  }

  pub const fn globals(&self) -> &'a BiffSubstreamNode {
    self.globals
  }

  pub fn globals_records(&self) -> &'a [BiffRecord] {
    // Construction proves this range belongs to the workbook tree.
    &self.workbook.tree.stream.records[self.globals.record_range.clone()]
  }

  pub fn drawing_groups(&self) -> impl Iterator<Item = XlsDrawingGroupRef<'a>> {
    self
      .globals_records()
      .iter()
      .filter_map(|record| match &record.data {
        BiffRecordData::MsoDrawingGroup(value) => Some(XlsDrawingGroupRef {
          source_record: record,
          value,
        }),
        _ => None,
      })
  }

  /// Joins worksheet picture shapes to the workbook-global OfficeArt BLIP
  /// store without copying image bytes or rebuilding the OfficeArt tree.
  pub fn pictures(&self) -> Result<Vec<XlsPictureRef<'a>>> {
    let mut stores = self
      .drawing_groups()
      .filter_map(XlsDrawingGroupRef::office_art)
      .flat_map(|stream| {
        let mut stores = Vec::new();
        collect_office_art_containers(&stream.records, 0xf001, &mut stores);
        stores
      });
    let store = stores.next();
    if stores.next().is_some() {
      return Err(Error::invalid(
        0,
        "XLS workbook contains multiple OfficeArt BLIP stores",
      ));
    }
    let entries = store.map_or(&[][..], |record| match &record.data {
      OfficeArtRecordData::Container(children)
      | OfficeArtRecordData::CompatibilityContainer(children) => children.as_slice(),
      _ => &[],
    });

    let mut pictures = Vec::new();
    for sheet in &self.sheets {
      let mut drawing_order = 0;
      for drawing in sheet.drawings().filter_map(XlsDrawingRef::office_art) {
        collect_xls_pictures(
          &drawing.records,
          *sheet,
          entries,
          &mut drawing_order,
          &mut pictures,
        )?;
      }
    }
    Ok(pictures)
  }

  pub fn sheets(&self) -> &[XlsSheetRef<'a>] {
    &self.sheets
  }

  pub fn sheet(&self, id: XlsSheetId) -> Option<XlsSheetRef<'a>> {
    self.sheets.iter().find(|sheet| sheet.id == id).copied()
  }

  /// Resolves a specification sheet position to its `TabId`-backed handle.
  /// Position-based wire references pass through this mapping so reorder
  /// retains the official sheet identity.
  pub fn sheet_at_position(&self, position: usize) -> Option<XlsSheetRef<'a>> {
    self
      .workbook
      .sheet_ids
      .get(position)
      .and_then(|id| self.sheet(*id))
  }

  pub fn unresolved_sheets(&self) -> &[XlsUnresolvedSheetRef<'a>] {
    &self.unresolved_sheets
  }

  /// Top-level non-Globals substreams not selected by any valid
  /// `BoundSheet8`. These remain fully typed and reachable in compatibility
  /// mode even though their workbook identity is unavailable.
  pub fn unlinked_substreams(&self) -> &[&'a BiffSubstreamNode] {
    &self.unlinked_substreams
  }

  /// Workbook custom views in physical Globals `UserBView` order. Each
  /// entry aggregates its sheet-local `CUSTOMVIEW` productions by GUID.
  pub fn custom_views(&self) -> &[XlsCustomViewRef<'a>] {
    &self.custom_views
  }

  pub fn unlinked_custom_sheet_views(&self) -> &[XlsCustomSheetViewRef<'a>] {
    &self.unlinked_custom_sheet_views
  }

  pub fn unlinked_custom_view_records(&self) -> &[&'a BiffRecord] {
    &self.unlinked_custom_view_records
  }

  pub const fn workbook_sheet_identifiers(&self) -> Option<&'a RrTabIdRecord> {
    self.workbook_sheet_identifiers
  }

  /// Resolves a Globals `TabId` through the workbook `RRTabId` collection.
  pub fn resolve_sheet_identifier(&self, identifier: u16) -> Result<XlsSheetRef<'a>> {
    let identifiers = self
      .workbook_sheet_identifiers
      .ok_or_else(|| Error::invalid(0, "workbook has no Globals RRTabId collection"))?;
    let mut positions = identifiers
      .sheet_ids
      .iter()
      .enumerate()
      .filter(|(_, candidate)| **candidate == identifier)
      .map(|(position, _)| position);
    let Some(position) = positions.next() else {
      return Err(Error::invalid(
        0,
        format!("sheet identifier {identifier} is absent from Globals RRTabId"),
      ));
    };
    if positions.next().is_some() {
      return Err(Error::invalid(
        0,
        format!("sheet identifier {identifier} occurs more than once in Globals RRTabId"),
      ));
    }
    self.sheet_at_position(position).ok_or_else(|| {
      Error::invalid(
        0,
        format!(
          "RRTabId position {} has no BoundSheet8 relationship",
          position
        ),
      )
    })
  }

  /// Resolves `UserBView.tabId`; `fInvalidTabId` explicitly suppresses the
  /// relationship as required by MS-XLS.
  pub fn resolve_custom_view_active_sheet(
    &self,
    view: &XlsCustomViewRef<'a>,
  ) -> Result<Option<XlsSheetRef<'a>>> {
    match self.resolve_custom_view_active_sheet_compatible(view) {
      XlsCustomViewActiveSheetLink::Resolved(sheet) => Ok(Some(sheet)),
      XlsCustomViewActiveSheetLink::NotSpecified => Ok(None),
      XlsCustomViewActiveSheetLink::Missing { sheet_identifier } => Err(Error::invalid(
        u64::from(view.source_record.offset),
        format!("UserBView.tabId {sheet_identifier} has no sheet relationship"),
      )),
      XlsCustomViewActiveSheetLink::Ambiguous { sheet_identifier } => Err(Error::invalid(
        u64::from(view.source_record.offset),
        format!("UserBView.tabId {sheet_identifier} has an ambiguous sheet relationship"),
      )),
    }
  }

  pub fn resolve_custom_view_active_sheet_compatible(
    &self,
    view: &XlsCustomViewRef<'a>,
  ) -> XlsCustomViewActiveSheetLink<'a> {
    if view
      .value
      .flags
      .contains(super::UserBViewFlags::INVALID_SHEET_ID)
    {
      return XlsCustomViewActiveSheetLink::NotSpecified;
    }
    let identifier = view.value.active_sheet_id;
    let position = if let Some(identifiers) = self.workbook_sheet_identifiers {
      let mut positions = identifiers
        .sheet_ids
        .iter()
        .enumerate()
        .filter(|(_, candidate)| **candidate == identifier)
        .map(|(position, _)| position);
      match (positions.next(), positions.next()) {
        (Some(position), None) => Some(position),
        (None, _) => {
          return XlsCustomViewActiveSheetLink::Missing {
            sheet_identifier: identifier,
          };
        }
        (Some(_), Some(_)) => {
          return XlsCustomViewActiveSheetLink::Ambiguous {
            sheet_identifier: identifier,
          };
        }
      }
    } else if self.sheets.len() > 4_112 {
      usize::from(identifier).checked_sub(1)
    } else {
      None
    };
    let Some(position) = position else {
      return XlsCustomViewActiveSheetLink::Missing {
        sheet_identifier: identifier,
      };
    };
    match self.sheet_at_position(position) {
      Some(sheet) => XlsCustomViewActiveSheetLink::Resolved(sheet),
      None => XlsCustomViewActiveSheetLink::Missing {
        sheet_identifier: identifier,
      },
    }
  }

  pub fn resolve_custom_view(&self, guid: [u8; 16]) -> Result<&XlsCustomViewRef<'a>> {
    let mut matches = self.custom_views.iter().filter(|view| view.guid() == guid);
    let Some(view) = matches.next() else {
      return Err(Error::invalid(
        0,
        "RRDUserView.guid does not match a Globals UserBView",
      ));
    };
    if matches.next().is_some() {
      return Err(Error::invalid(
        0,
        "RRDUserView.guid matches multiple Globals UserBView records",
      ));
    }
    Ok(view)
  }

  pub fn resolve_custom_view_compatible(&self, guid: [u8; 16]) -> XlsCustomViewLink<'_, 'a> {
    let mut matches = self.custom_views.iter().filter(|view| view.guid() == guid);
    match (matches.next(), matches.next()) {
      (Some(view), None) => XlsCustomViewLink::Resolved(view),
      (None, _) => XlsCustomViewLink::Missing { guid },
      (Some(_), Some(_)) => XlsCustomViewLink::Ambiguous { guid },
    }
  }

  pub fn supporting_links(&self) -> &[XlsSupportingLinkRef<'a>] {
    &self.supporting_links
  }

  pub fn supporting_link(&self, id: XlsSupportingLinkId) -> Option<&XlsSupportingLinkRef<'a>> {
    self.supporting_links.get(id.0)
  }

  /// Physical ExternSheet records in workbook order. Strict relationships
  /// accept at most one; compatible relationships retain all of them.
  pub fn extern_sheet_records(&self) -> &[&'a BiffRecord] {
    &self.extern_sheet_records
  }

  /// Logical XTI collection addressed by formula `ixti` values. Every XTI
  /// retains its physical source record and source index.
  pub fn external_sheets(&self) -> &[XlsExternalSheetRef<'a>] {
    &self.external_sheets
  }

  pub fn external_sheet(&self, index: u16) -> Option<XlsExternalSheetRef<'a>> {
    self.external_sheets.get(usize::from(index)).copied()
  }

  pub fn resolve_external_sheet(&self, index: u16) -> Result<XlsExternalSheetRef<'a>> {
    self.external_sheet(index).ok_or_else(|| {
      Error::invalid(
        0,
        format!("formula references missing ExternSheet XTI {index}"),
      )
    })
  }

  pub fn resolve_external_name(
    &self,
    external_sheet_index: u16,
    one_based_name_index: u32,
  ) -> Result<XlsExternalNameRef<'_, 'a>> {
    let external_sheet = self.resolve_external_sheet(external_sheet_index)?;
    let supporting_link_id = external_sheet.supporting_link.ok_or_else(|| {
      Error::invalid(
        0,
        format!(
          "ExternSheet XTI {external_sheet_index} references missing SupBook {}",
          external_sheet.source.sup_book_index
        ),
      )
    })?;
    let supporting_link = self.supporting_link(supporting_link_id).ok_or_else(|| {
      Error::invalid(
        0,
        "resolved SupBook identity is outside the relationship view",
      )
    })?;
    let name = supporting_link
      .external_name(one_based_name_index)
      .ok_or_else(|| {
        Error::invalid(
          0,
          format!("PtgNameX references missing one-based ExternName {one_based_name_index}"),
        )
      })?;
    Ok(XlsExternalNameRef {
      external_sheet,
      supporting_link,
      name,
    })
  }

  /// Globals PIVOTCACHEDEFINITION rules in zero-based SxView.iCache order.
  pub fn pivot_cache_definitions(&self) -> &[XlsPivotCacheDefinitionRef<'a>] {
    &self.pivot_cache_definitions
  }

  pub fn pivot_cache_definition(
    &self,
    id: XlsPivotCacheDefinitionId,
  ) -> Option<XlsPivotCacheDefinitionRef<'a>> {
    self.pivot_cache_definitions.get(id.0).copied()
  }

  pub fn resolve_pivot_table_cache_definition(
    &self,
    view: XlsPivotTableViewRef<'a>,
  ) -> Result<XlsPivotCacheDefinitionRef<'a>> {
    if !self.owns_pivot_table_view(view) {
      return Err(Error::invalid(
        u64::from(view.source_record.offset),
        "SxView does not belong to this Workbook relationship tree",
      ));
    }
    let id = view.cache_definition_id()?;
    self.pivot_cache_definition(id).ok_or_else(|| {
      Error::invalid(
        u64::from(view.source_record.offset),
        format!(
          "SxView.iCache {} is outside Globals SXStreamID collection",
          id.0
        ),
      )
    })
  }

  pub fn resolve_pivot_table_cache_definition_compatible(
    &self,
    view: XlsPivotTableViewRef<'a>,
  ) -> XlsPivotTableCacheLink<'a> {
    if !self.owns_pivot_table_view(view) {
      return XlsPivotTableCacheLink::Unresolved {
        view,
        error: XlsPivotTableCacheLinkError::ForeignView,
      };
    }
    let Ok(id) = view.cache_definition_id() else {
      return XlsPivotTableCacheLink::Unresolved {
        view,
        error: XlsPivotTableCacheLinkError::Negative {
          cache_index: view.value.cache_index,
        },
      };
    };
    match self.pivot_cache_definition(id) {
      Some(definition) => XlsPivotTableCacheLink::Resolved(definition),
      None => XlsPivotTableCacheLink::Unresolved {
        view,
        error: XlsPivotTableCacheLinkError::Missing {
          cache_index: view.value.cache_index,
        },
      },
    }
  }

  fn owns_pivot_table_view(&self, view: XlsPivotTableViewRef<'a>) -> bool {
    self.sheets.iter().any(|sheet| {
      sheet.id == view.sheet.id
        && std::ptr::eq(sheet.metadata_record, view.sheet.metadata_record)
        && std::ptr::eq(sheet.substream, view.sheet.substream)
    }) && view.sheet.direct_records().any(|record| {
      std::ptr::eq(record, view.source_record)
        && matches!(&record.data, BiffRecordData::SxView(value) if std::ptr::eq(value, view.value))
    })
  }

  /// Lbl/Name records in their one-based PtgName index order.
  pub fn defined_names(&self) -> &[&'a NameRecord] {
    &self.defined_names
  }

  pub fn defined_name(&self, one_based_index: u32) -> Option<&'a NameRecord> {
    usize::try_from(one_based_index)
      .ok()
      .and_then(|index| index.checked_sub(1))
      .and_then(|index| self.defined_names.get(index).copied())
  }

  pub fn resolve_defined_name(&self, one_based_index: u32) -> Result<&'a NameRecord> {
    self.defined_name(one_based_index).ok_or_else(|| {
      Error::invalid(
        0,
        format!("PtgName references missing one-based Lbl {one_based_index}"),
      )
    })
  }

  /// Resolves the one-based `Name.itab` local-name scope through the stable
  /// sheet identity table. A zero value is workbook scoped.
  pub fn defined_name_scope(&self, one_based_index: u32) -> Result<Option<XlsSheetRef<'a>>> {
    let name = self.resolve_defined_name(one_based_index)?;
    if name.sheet_index == 0 {
      return Ok(None);
    }
    self
      .sheet_at_position(usize::from(name.sheet_index - 1))
      .map(Some)
      .ok_or_else(|| {
        Error::invalid(
          0,
          format!(
            "Name.itab {} is outside the BoundSheet8 collection",
            name.sheet_index
          ),
        )
      })
  }

  pub fn resolve_formula_token_defined_name(
    &self,
    token: &FormulaTokenData,
  ) -> Result<Option<&'a NameRecord>> {
    match token {
      FormulaTokenData::Name { name_index } => self.resolve_defined_name(*name_index).map(Some),
      _ => Ok(None),
    }
  }

  pub fn resolve_formula_token_external_sheet(
    &self,
    token: &FormulaTokenData,
  ) -> Result<Option<XlsExternalSheetRef<'a>>> {
    let index = match token {
      FormulaTokenData::ExternalName {
        external_sheet_index,
        ..
      }
      | FormulaTokenData::Reference3d {
        external_sheet_index,
        ..
      }
      | FormulaTokenData::Area3d {
        external_sheet_index,
        ..
      }
      | FormulaTokenData::DeletedReference3d {
        external_sheet_index,
        ..
      }
      | FormulaTokenData::DeletedArea3d {
        external_sheet_index,
        ..
      } => *external_sheet_index,
      _ => return Ok(None),
    };
    self.resolve_external_sheet(index).map(Some)
  }

  pub fn resolve_formula_token_external_name(
    &self,
    token: &FormulaTokenData,
  ) -> Result<Option<XlsExternalNameRef<'_, 'a>>> {
    match token {
      FormulaTokenData::ExternalName {
        external_sheet_index,
        name_index,
      } => self
        .resolve_external_name(*external_sheet_index, *name_index)
        .map(Some),
      _ => Ok(None),
    }
  }

  /// The single Globals SST record. Multiple SST records are rejected as
  /// an ambiguous relationship rather than silently selecting one.
  pub fn shared_string_table(&self) -> Result<Option<&'a SstRecord>> {
    unique_globals_record(self.globals_records(), "SST", |data| match data {
      BiffRecordData::Sst(value) => Some(value),
      _ => None,
    })
  }

  pub fn extended_shared_string_table(&self) -> Result<Option<&'a ExtSstRecord>> {
    unique_globals_record(self.globals_records(), "ExtSST", |data| match data {
      BiffRecordData::ExtSst(value) => Some(value),
      _ => None,
    })
  }

  pub fn shared_string(&self, index: u32) -> Result<Option<&'a SstString>> {
    let Some(table) = self.shared_string_table()? else {
      return Ok(None);
    };
    Ok(
      usize::try_from(index)
        .ok()
        .and_then(|index| table.strings.get(index)),
    )
  }

  pub fn resolve_shared_string(&self, index: u32) -> Result<&'a SstString> {
    self
      .shared_string(index)?
      .ok_or_else(|| Error::invalid(0, format!("LabelSst references missing SST string {index}")))
  }

  /// Decodes one SST entry to its normal Rust string value.
  ///
  /// Callers that construct another shared-string table can invoke this
  /// once per source index instead of decoding the same SST entry at every
  /// referring cell.
  pub fn shared_string_value(&self, index: u32) -> Result<Option<String>> {
    self
      .shared_string(index)?
      .map(decode_sst_string)
      .transpose()
  }

  pub fn resolve_label_sst(&self, label: &LabelSstRecord) -> Result<&'a SstString> {
    self.resolve_shared_string(label.shared_string_index)
  }

  pub fn resolve_cell_shared_string(&self, cell: XlsCellRef<'a>) -> Result<Option<&'a SstString>> {
    cell
      .label_sst()
      .map(|label| self.resolve_label_sst(label))
      .transpose()
  }

  /// Globals XF collection in specification order. Its zero-based position
  /// is the XF index stored by cell records.
  pub fn xfs(&self) -> impl Iterator<Item = &'a XfRecord> {
    self
      .globals_records()
      .iter()
      .filter_map(|record| match &record.data {
        BiffRecordData::Xf(value) | BiffRecordData::XfCompatibility { value, .. } => Some(value),
        _ => None,
      })
  }

  pub fn xf(&self, index: u16) -> Option<&'a XfRecord> {
    self.xfs().nth(usize::from(index))
  }

  pub fn resolve_xf(&self, index: u16) -> Result<&'a XfRecord> {
    self
      .xf(index)
      .ok_or_else(|| Error::invalid(0, format!("cell references missing XF {index}")))
  }

  pub fn resolve_cell_xf(&self, cell: &CellHeader) -> Result<&'a XfRecord> {
    self.resolve_xf(cell.format_index)
  }

  /// Globals XF extension records in specification order.
  pub fn xf_extensions(&self) -> impl Iterator<Item = &'a XfExtRecord> {
    self
      .globals_records()
      .iter()
      .filter_map(|record| match &record.data {
        BiffRecordData::XfExt(value) => Some(value),
        _ => None,
      })
  }

  /// Returns the extension associated with an XF index, if present.
  pub fn xf_extension(&self, index: u16) -> Option<&'a XfExtRecord> {
    self
      .xf_extensions()
      .find(|extension| extension.xf_index == index)
  }

  pub fn fonts(&self) -> impl Iterator<Item = &'a FontRecord> {
    self
      .globals_records()
      .iter()
      .filter_map(|record| match &record.data {
        BiffRecordData::Font(value) | BiffRecordData::FontCompatibility { value, .. } => {
          Some(value)
        }
        _ => None,
      })
  }

  /// Resolves an MS-XLS `FontIndex`. Value 4 is reserved; values greater
  /// than 4 are one-based, as required by MS-XLS 2.5.112.
  pub fn font(&self, index: u16) -> Option<&'a FontRecord> {
    let record_index = match index {
      0..=3 => usize::from(index),
      4 => return None,
      _ => usize::from(index - 1),
    };
    self.fonts().nth(record_index)
  }

  pub fn resolve_font(&self, index: u16) -> Result<&'a FontRecord> {
    self
      .font(index)
      .ok_or_else(|| Error::invalid(0, format!("XF references invalid or missing Font {index}")))
  }

  pub fn formats(&self) -> impl Iterator<Item = &'a FormatRecord> {
    self
      .globals_records()
      .iter()
      .filter_map(|record| match &record.data {
        BiffRecordData::Format(value) => Some(value),
        _ => None,
      })
  }

  /// Resolves a custom number-format index. Built-in format indexes do not
  /// have a Format record and therefore return `None`.
  pub fn custom_number_format(&self, index: u16) -> Option<&'a FormatRecord> {
    self.formats().find(|format| format.format_index == index)
  }

  pub fn resolve_number_format(&self, index: u16) -> Result<XlsNumberFormatRef<'a>> {
    if index < 0x00a4 {
      return Ok(XlsNumberFormatRef::BuiltIn(index));
    }
    if index > 0x0188 {
      return Err(Error::invalid(
        0,
        format!("XF number-format index {index} is outside MS-XLS FormatIndex"),
      ));
    }
    self
      .custom_number_format(index)
      .map(XlsNumberFormatRef::Custom)
      .ok_or_else(|| Error::invalid(0, format!("XF references missing custom Format {index}")))
  }

  pub fn resolve_number_format_compatible(&self, index: u16) -> XlsNumberFormatRef<'a> {
    self
      .resolve_number_format(index)
      .unwrap_or(XlsNumberFormatRef::Compatibility(index))
  }

  pub fn resolve_cell_format(&self, cell: &CellHeader) -> Result<XlsCellFormatRef<'a>> {
    let xf = self.resolve_cell_xf(cell)?;
    let number_format = self.resolve_number_format(xf.number_format_index)?;
    let custom_number_format_code = match number_format {
      XlsNumberFormatRef::Custom(value) => Some(decode_biff_unicode_string(&value.format_string)?),
      XlsNumberFormatRef::BuiltIn(_) | XlsNumberFormatRef::Compatibility(_) => None,
    };
    Ok(XlsCellFormatRef {
      xf,
      font: self.resolve_font(xf.font_index)?,
      number_format,
      custom_number_format_code,
    })
  }

  pub fn resolve_cell_format_ref(&self, cell: XlsCellRef<'a>) -> Result<XlsCellFormatRef<'a>> {
    self.resolve_cell_format(&cell.cell())
  }

  pub fn resolve_cell_format_ref_compatible(
    &self,
    cell: XlsCellRef<'a>,
  ) -> Result<XlsCellFormatRef<'a>> {
    let xf = self.resolve_cell_xf(&cell.cell())?;
    let number_format = self.resolve_number_format_compatible(xf.number_format_index);
    let custom_number_format_code = match number_format {
      XlsNumberFormatRef::Custom(value) => Some(decode_biff_unicode_string(&value.format_string)?),
      XlsNumberFormatRef::BuiltIn(_) | XlsNumberFormatRef::Compatibility(_) => None,
    };
    Ok(XlsCellFormatRef {
      xf,
      font: self.resolve_font(xf.font_index)?,
      number_format,
      custom_number_format_code,
    })
  }

  /// Resolves the native stored scalar for one cell in a sparse index.
  /// Formula cells return their file-owned cached result; this method never
  /// calculates a formula or formats a number for display.
  pub fn resolve_cell_value(
    &self,
    index: &XlsSparseCellIndex<'a>,
    cell: XlsCellRef<'a>,
  ) -> Result<XlsCellValue> {
    self.resolve_cell_value_with_policy(index, cell, false)
  }

  pub fn resolve_cell_value_compatible(
    &self,
    index: &XlsSparseCellIndex<'a>,
    cell: XlsCellRef<'a>,
  ) -> Result<XlsCellValue> {
    self.resolve_cell_value_with_policy(index, cell, true)
  }

  fn resolve_cell_value_with_policy(
    &self,
    index: &XlsSparseCellIndex<'a>,
    cell: XlsCellRef<'a>,
    preserve_compatibility: bool,
  ) -> Result<XlsCellValue> {
    index.ensure_cell(cell)?;
    match cell.value {
      XlsCellValueRef::Formula(_) | XlsCellValueRef::Formula4Compatibility(_) => {
        (if preserve_compatibility {
          index.resolve_cell_formula_compatible(cell)
        } else {
          index.resolve_cell_formula(cell)
        })?
        .ok_or_else(|| {
          Error::invalid(
            u64::from(cell.source_record.offset),
            "formula cell has no formula relationship",
          )
        })?
        .cached_value()
        .map(XlsCellValue::Formula)
      }
      XlsCellValueRef::Blank(_) | XlsCellValueRef::MulBlank { .. } => Ok(XlsCellValue::Blank),
      XlsCellValueRef::Number(value) => Ok(XlsCellValue::Number(f64::from_bits(value.value_bits))),
      XlsCellValueRef::BoolErr(value) => Ok(decode_bool_err(value)),
      XlsCellValueRef::Label(value) => {
        decode_biff_unicode_string(&value.text).map(XlsCellValue::String)
      }
      XlsCellValueRef::LabelSst(value) => self
        .resolve_label_sst(value)
        .and_then(decode_sst_string)
        .map(XlsCellValue::String),
      XlsCellValueRef::Rk(value) => Ok(XlsCellValue::Number(decode_rk_number(value.value))),
      XlsCellValueRef::MulRk { value, .. } => {
        Ok(XlsCellValue::Number(decode_rk_number(value.value)))
      }
    }
  }

  pub fn resolve_cell_formula(
    &self,
    sheet: XlsSheetRef<'a>,
    cell: XlsCellRef<'a>,
  ) -> Result<Option<XlsFormulaRef<'a>>> {
    cell
      .formula()
      .map(|formula| sheet.resolve_formula(formula))
      .transpose()
  }

  pub fn resolve_cell_formula_compatible(
    &self,
    sheet: XlsSheetRef<'a>,
    cell: XlsCellRef<'a>,
  ) -> Result<Option<XlsFormulaRef<'a>>> {
    cell
      .formula()
      .map(|formula| sheet.resolve_formula_compatible(formula))
      .transpose()
  }
}

fn unique_globals_record<'a, T>(
  records: &'a [BiffRecord],
  name: &str,
  select: impl Fn(&'a BiffRecordData) -> Option<&'a T>,
) -> Result<Option<&'a T>> {
  let mut values = records.iter().filter_map(|record| select(&record.data));
  let value = values.next();
  if values.next().is_some() {
    return Err(Error::invalid(
      0,
      format!("Globals Substream contains multiple {name} records"),
    ));
  }
  Ok(value)
}

fn custom_view_name_prefix(raw: [u8; 16]) -> String {
  let guid = Guid::from_fields(
    u32::from_le_bytes(raw[0..4].try_into().expect("four GUID bytes")),
    u16::from_le_bytes(raw[4..6].try_into().expect("two GUID bytes")),
    u16::from_le_bytes(raw[6..8].try_into().expect("two GUID bytes")),
    raw[8..16].try_into().expect("eight GUID bytes"),
  );
  format!("Z_{}_.wvu.", guid.to_string().replace('-', "_"))
}

fn custom_view_defined_name_kind(
  name: &NameRecord,
  prefix: &str,
) -> Option<XlsCustomViewDefinedNameKind> {
  const SUFFIXES: [(&str, XlsCustomViewDefinedNameKind); 6] = [
    ("PrintTitles", XlsCustomViewDefinedNameKind::PrintTitles),
    ("PrintArea", XlsCustomViewDefinedNameKind::PrintArea),
    ("Rows", XlsCustomViewDefinedNameKind::HiddenRows),
    ("Cols", XlsCustomViewDefinedNameKind::HiddenColumns),
    ("FilterData", XlsCustomViewDefinedNameKind::FilterData),
    (
      "FilterCriteria",
      XlsCustomViewDefinedNameKind::FilterCriteria,
    ),
  ];
  SUFFIXES.iter().find_map(|(suffix, kind)| {
    let expected = format!("{prefix}{suffix}");
    name_value_eq_ascii(&name.name, expected.as_bytes()).then_some(*kind)
  })
}

fn name_value_eq_ascii(value: &NameValue, expected: &[u8]) -> bool {
  match value {
    NameValue::BuiltIn(_) => false,
    NameValue::User(XlStringCharacters::Compressed(bytes)) => bytes.eq_ignore_ascii_case(expected),
    NameValue::User(XlStringCharacters::Unicode(words)) => {
      words.len() == expected.len()
        && words
          .iter()
          .zip(expected)
          .all(|(word, byte)| u8::try_from(*word).is_ok_and(|word| word.eq_ignore_ascii_case(byte)))
    }
  }
}

impl BiffWorkbookTree {
  pub fn from_stream(stream: BiffStream) -> Result<Self> {
    let (substreams, outside_substream_ranges) = index_biff_substreams(&stream)?;
    Ok(Self {
      stream,
      substreams,
      outside_substream_ranges,
    })
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_stream(BiffStream::from_bytes(bytes)?)
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    Self::from_stream(BiffStream::from_bytes_with_limits(bytes, limits)?)
  }

  /// Rebuilds the BOF/EOF tree after inserting, removing, or moving records.
  pub fn reindex(&mut self) -> Result<()> {
    let (substreams, outside_substream_ranges) = index_biff_substreams(&self.stream)?;
    self.substreams = substreams;
    self.outside_substream_ranges = outside_substream_ranges;
    Ok(())
  }

  fn relayout_in_place(&mut self, preserve_invalid_references: bool) -> Result<()> {
    self.stream.relayout_in_place(preserve_invalid_references)?;
    self.reindex()
  }

  /// Rebuilds physical BIFF positions, specification file pointers, and the
  /// BOF/EOF substream index after record-tree edits.
  pub fn relayout(&mut self) -> Result<()> {
    let mut rebuilt = self.clone();
    rebuilt.relayout_in_place(false)?;
    *self = rebuilt;
    Ok(())
  }

  /// Rebuilds the physical layout while retaining pre-existing invalid
  /// reference fields that cannot be relocated unambiguously.
  pub fn relayout_preserving_compatibility(&mut self) -> Result<()> {
    let mut rebuilt = self.clone();
    rebuilt.relayout_in_place(true)?;
    *self = rebuilt;
    Ok(())
  }

  /// Aggregates the workbook-global `OfficeArtDggContainer` and the
  /// `OfficeArtDgContainer` values owned by sheet substreams.
  ///
  /// The BIFF and OfficeArt record trees remain the editable source of
  /// truth. A workbook without OfficeArt returns `None`; partial or
  /// ambiguous OfficeArt framing is rejected instead of being presented as
  /// a complete drawing graph.
  pub fn drawing_graph(&self) -> Result<Option<OfficeArtDrawingGraph>> {
    let mut drawing_groups = Vec::<&OfficeArtStream>::new();
    let mut drawings = Vec::<&OfficeArtStream>::new();
    let mut incomplete_kinds = Vec::new();

    for record in &self.stream.records {
      match &record.data {
        BiffRecordData::MsoDrawingGroup(value) => match &value.data {
          MsoDrawingData::Complete(stream) => drawing_groups.push(stream),
          MsoDrawingData::Partial(_) => incomplete_kinds.push("partial MsoDrawingGroup"),
          MsoDrawingData::Incomplete { .. } => incomplete_kinds.push("incomplete MsoDrawingGroup"),
        },
        BiffRecordData::MsoDrawing(value) => match &value.data {
          MsoDrawingData::Complete(stream) => drawings.push(stream),
          MsoDrawingData::Partial(_) => incomplete_kinds.push("partial MsoDrawing"),
          MsoDrawingData::Incomplete { .. } => incomplete_kinds.push("incomplete MsoDrawing"),
        },
        _ => {}
      }
    }

    if drawing_groups.is_empty() && drawings.is_empty() && incomplete_kinds.is_empty() {
      return Ok(None);
    }
    if !incomplete_kinds.is_empty() {
      return Err(Error::invalid(
        0,
        format!(
          "XLS drawing graph contains non-complete OfficeArt aggregates: {}",
          incomplete_kinds.join(", ")
        ),
      ));
    }
    let [drawing_group] = drawing_groups.as_slice() else {
      return Err(Error::invalid(
        0,
        format!(
          "XLS drawing graph contains {} complete MsoDrawingGroup records, expected 1",
          drawing_groups.len()
        ),
      ));
    };
    OfficeArtDrawingGraph::from_streams(drawing_group, &drawings).map(Some)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    index_biff_substreams(&self.stream)?;
    self.stream.to_bytes()
  }

  pub fn to_bytes_preserving_compatibility(&self) -> Result<Vec<u8>> {
    index_biff_substreams(&self.stream)?;
    self.stream.to_bytes_preserving_compatibility()
  }

  fn write_plan(&self, preserve_compatibility: bool) -> Result<BiffStreamWritePlan<'_>> {
    index_biff_substreams(&self.stream)?;
    self.stream.write_plan(preserve_compatibility)
  }
}

impl CfbStreamWriter for BiffStreamWritePlan<'_> {
  fn write_to(&self, writer: &mut dyn Write) -> Result<()> {
    BiffStreamWritePlan::write_to(self, writer)
  }
}

fn index_biff_substreams(
  stream: &BiffStream,
) -> Result<(Vec<BiffSubstreamNode>, Vec<Range<usize>>)> {
  let mut roots = Vec::new();
  let mut stack = Vec::<OpenSubstream>::new();
  let mut covered = vec![false; stream.records.len()];
  for (index, record) in stream.records.iter().enumerate() {
    match &record.data {
      BiffRecordData::Bof(bof) => stack.push(OpenSubstream {
        kind: BiffSubstreamKind::from_document_type(bof.document_type),
        start: index,
        children: Vec::new(),
      }),
      BiffRecordData::LegacyBof { .. } => stack.push(OpenSubstream {
        kind: BiffSubstreamKind::Compatibility(0xffff),
        start: index,
        children: Vec::new(),
      }),
      BiffRecordData::Eof => {
        let open = stack
          .pop()
          .ok_or_else(|| Error::invalid(record.offset.into(), "BIFF EOF has no matching BOF"))?;
        covered[open.start..=index].fill(true);
        let node = BiffSubstreamNode {
          kind: open.kind,
          record_range: open.start..index + 1,
          children: open.children,
        };
        if let Some(parent) = stack.last_mut() {
          parent.children.push(node);
        } else {
          roots.push(node);
        }
      }
      _ => {}
    }
  }
  if let Some(open) = stack.last() {
    return Err(Error::invalid(
      stream.records[open.start].offset.into(),
      "BIFF BOF has no matching EOF",
    ));
  }
  let outside_substream_ranges = matching_ranges(&covered, false);
  Ok((roots, outside_substream_ranges))
}

impl PartialEq for XlsWorkbookStream {
  fn eq(&self, other: &Self) -> bool {
    self.name == other.name && self.tree == other.tree
  }
}

impl Eq for XlsWorkbookStream {}

impl XlsWorkbookStream {
  pub fn from_tree(name: XlsStreamName, tree: BiffWorkbookTree) -> Result<Self> {
    let globals = tree
      .substreams
      .iter()
      .find(|node| node.kind == BiffSubstreamKind::WorkbookGlobals)
      .or_else(|| tree.substreams.first())
      .ok_or_else(|| Error::invalid(0, "Workbook Stream has no BOF/EOF substream"))?;
    let globals_records = globals.records(&tree).ok_or_else(|| {
      Error::invalid(0, "Globals Substream record range is outside the BIFF tree")
    })?;
    let sheet_count = globals_records
      .iter()
      .filter(|record| {
        matches!(
          record.data,
          BiffRecordData::BoundSheet8(_) | BiffRecordData::BoundSheet8Compatibility { .. }
        )
      })
      .count();
    let rr_tab_ids = globals_records
      .iter()
      .filter_map(|record| match &record.data {
        BiffRecordData::RrTabId(value) => Some(value),
        _ => None,
      })
      .collect::<Vec<_>>();
    let sheet_ids = match rr_tab_ids.as_slice() {
      [identifiers]
        if identifiers.sheet_ids.len() == sheet_count
          && identifiers
            .sheet_ids
            .iter()
            .all(|identifier| (1..=0xfffe).contains(identifier))
          && identifiers
            .sheet_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            == sheet_count =>
      {
        identifiers
          .sheet_ids
          .iter()
          .copied()
          .map(XlsSheetId::tab_id_value)
          .collect()
      }
      [] if sheet_count > 4_112 => (0..sheet_count)
        .map(XlsSheetId::from_sheet_ordinal)
        .collect::<Result<Vec<_>>>()?,
      _ => (0..sheet_count)
        .map(XlsSheetId::compatibility_position)
        .collect::<Result<Vec<_>>>()?,
    };
    Ok(Self {
      name,
      tree: tree.into(),
      sheet_ids,
    })
  }

  /// Joins Globals `BoundSheet8` records to their sheet substreams using
  /// `lbPlyPos`, the relationship defined by MS-XLS 2.4.28.
  pub fn relationships(&self) -> Result<XlsWorkbookView<'_>> {
    self.relationships_with_policy(false)
  }

  /// Builds every relationship that is unambiguous while retaining dangling
  /// `BoundSheet8` records and unlinked typed substreams explicitly.
  pub fn relationships_compatible(&self) -> Result<XlsWorkbookView<'_>> {
    self.relationships_with_policy(true)
  }

  fn relationships_with_policy(&self, preserve_compatibility: bool) -> Result<XlsWorkbookView<'_>> {
    if self
      .sheet_ids
      .iter()
      .copied()
      .collect::<BTreeSet<_>>()
      .len()
      != self.sheet_ids.len()
    {
      return Err(Error::invalid(
        0,
        "workbook sheet identity table is internally inconsistent",
      ));
    }
    let mut globals_candidates = self
      .tree
      .substreams
      .iter()
      .filter(|node| node.kind == BiffSubstreamKind::WorkbookGlobals);
    let first_globals = globals_candidates.next();
    if globals_candidates.next().is_some() {
      return Err(Error::invalid(
        0,
        "Workbook Stream has multiple Globals Substreams",
      ));
    }
    // BIFF2-BIFF7 BOF payloads are preserved as compatibility records, so
    // they do not carry the current-specification 0x0005 kind in the
    // structural index. Their first top-level substream is nevertheless
    // the workbook-global substream used by BoundSheet relationships.
    let globals = match first_globals {
      Some(globals) => globals,
      None if !self.tree.stream.is_biff8() || preserve_compatibility => self
        .tree
        .substreams
        .first()
        .ok_or_else(|| Error::invalid(0, "Workbook Stream has no BOF/EOF substream"))?,
      None => {
        return Err(Error::invalid(
          0,
          "Workbook Stream has no Globals Substream",
        ));
      }
    };
    let globals_records = globals.records(&self.tree).ok_or_else(|| {
      Error::invalid(0, "Globals Substream record range is outside the BIFF tree")
    })?;
    let rr_tab_ids = globals_records
      .iter()
      .filter_map(|record| match &record.data {
        BiffRecordData::RrTabId(value) => Some(value),
        _ => None,
      })
      .collect::<Vec<_>>();
    match self.sheet_ids.first().map(|identity| identity.kind) {
      Some(XlsSheetIdKind::TabId) => {
        let [identifiers] = rr_tab_ids.as_slice() else {
          return Err(Error::invalid(
            0,
            "workbook sheet identities no longer match a unique RRTabId record",
          ));
        };
        if identifiers.sheet_ids.len() != self.sheet_ids.len()
          || identifiers
            .sheet_ids
            .iter()
            .copied()
            .zip(self.sheet_ids.iter().copied())
            .any(|(identifier, identity)| identity.tab_id() != Some(identifier))
        {
          return Err(Error::invalid(
            0,
            "workbook RRTabId changed outside the sheet identity transaction",
          ));
        }
      }
      Some(XlsSheetIdKind::SheetOrdinal) => {
        if self.sheet_ids.len() <= 4_112 || !rr_tab_ids.is_empty() {
          return Err(Error::invalid(
            0,
            "large-workbook sheet ordinals do not match the RRTabId omission rule",
          ));
        }
      }
      Some(XlsSheetIdKind::CompatibilityPosition)
        if !preserve_compatibility && self.tree.stream.is_biff8() =>
      {
        return Err(Error::invalid(
          0,
          "BIFF8 workbook has no unique, complete RRTabId sheet identity collection",
        ));
      }
      Some(XlsSheetIdKind::CompatibilityPosition) | None => {}
    }

    let mut sheets = Vec::new();
    let mut unresolved_sheets = Vec::new();
    let mut linked_starts = BTreeSet::new();
    let mut sheet_index = 0usize;
    for metadata_record in globals_records {
      let metadata = match &metadata_record.data {
        BiffRecordData::BoundSheet8(value)
        | BiffRecordData::BoundSheet8Compatibility { value, .. } => value,
        _ => continue,
      };
      let id = *self.sheet_ids.get(sheet_index).ok_or_else(|| {
        Error::invalid(
          u64::from(metadata_record.offset),
          "BoundSheet8 collection changed outside the workbook identity transaction",
        )
      })?;
      sheet_index += 1;
      let mut candidates = self.tree.substreams.iter().filter(|node| {
        !std::ptr::eq(*node, globals)
          && self
            .tree
            .stream
            .records
            .get(node.record_range.start)
            .is_some_and(|record| record.offset == metadata.sheet_bof_offset)
      });
      let Some(substream) = candidates.next() else {
        if preserve_compatibility {
          unresolved_sheets.push(XlsUnresolvedSheetRef {
            id,
            metadata_record,
            metadata,
            error: XlsSheetLinkError::Missing {
              sheet_bof_offset: metadata.sheet_bof_offset,
            },
          });
          continue;
        }
        return Err(Error::invalid(
          u64::from(metadata_record.offset),
          format!(
            "BoundSheet8.lbPlyPos {} does not select a top-level sheet BOF",
            metadata.sheet_bof_offset
          ),
        ));
      };
      if candidates.next().is_some() {
        if preserve_compatibility {
          unresolved_sheets.push(XlsUnresolvedSheetRef {
            id,
            metadata_record,
            metadata,
            error: XlsSheetLinkError::Ambiguous {
              sheet_bof_offset: metadata.sheet_bof_offset,
            },
          });
          continue;
        }
        return Err(Error::invalid(
          u64::from(metadata_record.offset),
          "BoundSheet8.lbPlyPos selects multiple sheet substreams",
        ));
      }
      if !linked_starts.insert(substream.record_range.start) {
        if preserve_compatibility {
          unresolved_sheets.push(XlsUnresolvedSheetRef {
            id,
            metadata_record,
            metadata,
            error: XlsSheetLinkError::Duplicate {
              sheet_bof_offset: metadata.sheet_bof_offset,
            },
          });
          continue;
        }
        return Err(Error::invalid(
          u64::from(metadata_record.offset),
          "multiple BoundSheet8 records select the same sheet substream",
        ));
      }
      let records = substream.records(&self.tree).ok_or_else(|| {
        Error::invalid(
          u64::from(metadata_record.offset),
          "sheet substream record range is outside the BIFF tree",
        )
      })?;
      sheets.push(XlsSheetRef {
        id,
        metadata_record,
        metadata,
        substream,
        records,
      });
    }
    if sheet_index != self.sheet_ids.len() {
      return Err(Error::invalid(
        u64::from(globals_records.first().map_or(0, |record| record.offset)),
        "workbook sheet identity table does not match the BoundSheet8 collection",
      ));
    }
    let unlinked_substreams = self
      .tree
      .substreams
      .iter()
      .filter(|node| {
        !std::ptr::eq(*node, globals) && !linked_starts.contains(&node.record_range.start)
      })
      .collect();
    let mut supporting_links = Vec::new();
    for (record_index, source_record) in globals_records.iter().enumerate() {
      let BiffRecordData::SupBook(value) = &source_record.data else {
        continue;
      };
      let id = XlsSupportingLinkId(supporting_links.len());
      let mut cursor = record_index + 1;
      let mut external_name_records = Vec::new();
      let mut external_names = Vec::new();
      while let Some(record) = globals_records.get(cursor) {
        let BiffRecordData::ExternName(external_name) = &record.data else {
          break;
        };
        external_name_records.push(record);
        external_names.push(external_name);
        cursor += 1;
      }
      let mut external_cell_caches = Vec::new();
      while let Some(xct_record) = globals_records.get(cursor) {
        let BiffRecordData::Xct(xct) = &xct_record.data else {
          break;
        };
        cursor += 1;
        let mut crn_records = Vec::new();
        while let Some(record) = globals_records.get(cursor) {
          if !matches!(record.data, BiffRecordData::Crn(_)) {
            break;
          }
          crn_records.push(record);
          cursor += 1;
        }
        if crn_records.len() != usize::from(xct.crn_count()) && !preserve_compatibility {
          return Err(Error::invalid(
            u64::from(xct_record.offset),
            format!(
              "XCT declares {} CRN records but {} immediately follow",
              xct.crn_count(),
              crn_records.len()
            ),
          ));
        }
        if xct.sheet_table_index >= value.sheet_count && !preserve_compatibility {
          return Err(Error::invalid(
            u64::from(xct_record.offset),
            format!(
              "XCT sheet index {} is outside SupBook sheet count {}",
              xct.sheet_table_index, value.sheet_count
            ),
          ));
        }
        external_cell_caches.push(XlsExternalCellCacheRef {
          supporting_link: id,
          source_record: xct_record,
          value: xct,
          crn_records,
        });
      }
      let mut link_extern_sheet_records = Vec::new();
      while let Some(record) = globals_records.get(cursor) {
        if !matches!(record.data, BiffRecordData::ExternSheet(_)) {
          break;
        }
        link_extern_sheet_records.push(record);
        cursor += 1;
      }
      let mut continuation_records = Vec::new();
      while let Some(record) = globals_records.get(cursor) {
        if !matches!(record.data, BiffRecordData::Continue { .. }) {
          break;
        }
        continuation_records.push(record);
        cursor += 1;
      }
      supporting_links.push(XlsSupportingLinkRef {
        id,
        source_record,
        value,
        records: &globals_records[record_index..cursor],
        external_name_records,
        external_names,
        external_cell_caches,
        extern_sheet_records: link_extern_sheet_records,
        continuation_records,
      });
    }
    let extern_sheet_records = globals_records
      .iter()
      .filter(|record| matches!(record.data, BiffRecordData::ExternSheet(_)))
      .collect::<Vec<_>>();
    if extern_sheet_records.len() > 1 && !preserve_compatibility {
      return Err(Error::invalid(
        0,
        "Globals Substream contains multiple ExternSheet records",
      ));
    }
    let mut external_sheets = Vec::new();
    // Excel and LibreOffice interpret producer-split ExternSheet records
    // by prepending each later physical record to the logical XTI table.
    // This also matches the real POI bug-45698 corpus layout, whose
    // physical one-XTI records run from the last sheet to the first.
    for source_record in extern_sheet_records.iter().rev().copied() {
      let BiffRecordData::ExternSheet(extern_sheet) = &source_record.data else {
        unreachable!("ExternSheet records were filtered above")
      };
      for (source_reference_index, source) in extern_sheet.references.iter().enumerate() {
        let index = external_sheets.len();
        let supporting_link = supporting_links
          .get(usize::from(source.sup_book_index))
          .map(XlsSupportingLinkRef::id);
        if supporting_link.is_none() && !preserve_compatibility {
          return Err(Error::invalid(
            0,
            format!(
              "ExternSheet XTI {index} references missing SupBook {}",
              source.sup_book_index
            ),
          ));
        }
        let index = u16::try_from(index)
          .map_err(|_| Error::Limit("ExternSheet XTI count exceeds u16".into()))?;
        let source_reference_index = u16::try_from(source_reference_index)
          .map_err(|_| Error::Limit("ExternSheet source XTI count exceeds u16".into()))?;
        external_sheets.push(XlsExternalSheetRef {
          index,
          source_record,
          source_reference_index,
          source,
          supporting_link,
        });
      }
    }
    let defined_names = globals_records
      .iter()
      .filter_map(|record| match &record.data {
        BiffRecordData::Name(value) => Some(value),
        _ => None,
      })
      .collect();
    let mut pivot_cache_definitions = Vec::new();
    for (record_index, source_record) in globals_records.iter().enumerate() {
      let BiffRecordData::SxStreamId(value) = &source_record.data else {
        continue;
      };
      let (source_type_record, source_type) = match globals_records.get(record_index + 1) {
        Some(record) => match &record.data {
          BiffRecordData::SxVs(value) => (Some(record), Some(value)),
          _ => (None, None),
        },
        None => (None, None),
      };
      if source_type.is_none() && !preserve_compatibility {
        return Err(Error::invalid(
          u64::from(source_record.offset),
          "SXStreamID is not immediately followed by required SXVS",
        ));
      }
      pivot_cache_definitions.push(XlsPivotCacheDefinitionRef {
        id: XlsPivotCacheDefinitionId(pivot_cache_definitions.len()),
        source_record,
        value,
        source_type_record,
        source_type,
      });
    }

    let mut all_custom_sheet_views = Vec::new();
    let mut unlinked_custom_view_records = Vec::new();
    for sheet in &sheets {
      let (views, unlinked) = sheet.custom_sheet_views_compatible();
      all_custom_sheet_views.extend(views);
      unlinked_custom_view_records.extend(unlinked);
    }
    if !preserve_compatibility && let Some(record) = unlinked_custom_view_records.first() {
      return Err(Error::invalid(
        u64::from(record.offset),
        "sheet contains an unmatched UserSViewBegin/UserSViewEnd delimiter",
      ));
    }

    let workbook_sheet_identifiers =
      unique_globals_record(globals_records, "RRTabId", |data| match data {
        BiffRecordData::RrTabId(value) => Some(value),
        _ => None,
      })?;
    for sheet_view in &all_custom_sheet_views {
      let resolved_sheet = workbook_sheet_identifiers.and_then(|identifiers| {
        let mut positions = identifiers
          .sheet_ids
          .iter()
          .enumerate()
          .filter(|(_, identifier)| u32::from(**identifier) == sheet_view.sheet_identifier())
          .filter_map(|(position, _)| self.sheet_ids.get(position).copied());
        let first = positions.next();
        (positions.next().is_none()).then_some(first).flatten()
      });
      if !preserve_compatibility && resolved_sheet != Some(sheet_view.sheet.id) {
        return Err(Error::invalid(
          u64::from(sheet_view.begin_record.offset),
          format!(
            "UserSViewBegin.iTabid {} does not resolve to its owning sheet {}",
            sheet_view.sheet_identifier(),
            sheet_view.sheet.id.value()
          ),
        ));
      }
      if !preserve_compatibility
        && (sheet_view.begin.is_chart()
          != matches!(sheet_view.sheet.kind(), BiffSubstreamKind::ChartSheet))
      {
        return Err(Error::invalid(
          u64::from(sheet_view.begin_record.offset),
          "UserSViewBegin layout does not match its owning sheet kind",
        ));
      }
    }

    let user_workbook_views = globals_records
      .iter()
      .filter_map(|source_record| match &source_record.data {
        BiffRecordData::UserBView(value) => Some((source_record, value)),
        _ => None,
      })
      .collect::<Vec<_>>();
    let custom_views = user_workbook_views
      .iter()
      .map(|(source_record, value)| {
        let name_prefix = custom_view_name_prefix(value.guid);
        let defined_names = globals_records
          .iter()
          .filter_map(|record| {
            let BiffRecordData::Name(name) = &record.data else {
              return None;
            };
            let kind = custom_view_defined_name_kind(name, &name_prefix)?;
            Some(XlsCustomViewDefinedNameRef {
              source_record: record,
              value: name,
              kind,
            })
          })
          .collect();
        XlsCustomViewRef {
          source_record,
          value,
          sheet_views: all_custom_sheet_views
            .iter()
            .copied()
            .filter(|view| view.guid() == value.guid)
            .collect(),
          defined_names,
        }
      })
      .collect::<Vec<_>>();
    let unlinked_custom_sheet_views = all_custom_sheet_views
      .iter()
      .copied()
      .filter(|sheet_view| {
        !user_workbook_views
          .iter()
          .any(|(_, workbook_view)| workbook_view.guid == sheet_view.guid())
      })
      .collect::<Vec<_>>();
    if !preserve_compatibility {
      if let Some(view) = unlinked_custom_sheet_views.first() {
        return Err(Error::invalid(
          u64::from(view.begin_record.offset),
          "UserSViewBegin.guid does not match a Globals UserBView.guid",
        ));
      }
      for custom_view in &custom_views {
        if custom_views
          .iter()
          .filter(|candidate| candidate.guid() == custom_view.guid())
          .count()
          != 1
        {
          return Err(Error::invalid(
            u64::from(custom_view.source_record.offset),
            "Globals contains duplicate UserBView GUID values",
          ));
        }
        if !custom_view
          .value
          .flags
          .contains(super::UserBViewFlags::INVALID_SHEET_ID)
        {
          let active_position = if let Some(identifiers) = workbook_sheet_identifiers {
            let matches = identifiers
              .sheet_ids
              .iter()
              .enumerate()
              .filter(|(_, identifier)| **identifier == custom_view.value.active_sheet_id)
              .map(|(position, _)| position)
              .collect::<Vec<_>>();
            if matches.len() == 1 {
              Some(matches[0])
            } else {
              None
            }
          } else if sheets.len() > 4_112 {
            usize::from(custom_view.value.active_sheet_id).checked_sub(1)
          } else {
            None
          };
          if active_position
            .and_then(|position| self.sheet_ids.get(position))
            .and_then(|id| sheets.iter().find(|sheet| sheet.id == *id))
            .is_none()
          {
            return Err(Error::invalid(
              u64::from(custom_view.source_record.offset),
              format!(
                "UserBView.tabId {} has no unique sheet relationship",
                custom_view.value.active_sheet_id
              ),
            ));
          }
        }
        for sheet in &sheets {
          if custom_view
            .sheet_views
            .iter()
            .filter(|view| view.sheet.id == sheet.id)
            .count()
            != 1
          {
            return Err(Error::invalid(
              u64::from(custom_view.source_record.offset),
              format!(
                "UserBView has no unique UserSViewBegin collection for sheet {}",
                sheet.id.value()
              ),
            ));
          }
        }
      }
    }
    Ok(XlsWorkbookView {
      workbook: self,
      globals,
      sheets,
      unresolved_sheets,
      unlinked_substreams,
      supporting_links,
      extern_sheet_records,
      external_sheets,
      defined_names,
      pivot_cache_definitions,
      workbook_sheet_identifiers,
      custom_views,
      unlinked_custom_sheet_views,
      unlinked_custom_view_records,
    })
  }

  /// Returns the complete drawing graph for this Workbook Stream when one
  /// is present.
  pub fn drawing_graph(&self) -> Result<Option<OfficeArtDrawingGraph>> {
    self.tree.drawing_graph()
  }
}

fn revision_sheet_link<'a>(
  log: XlsRevisionLogRef<'a>,
  workbook: &XlsWorkbookView<'a>,
  sheet_identifier: Option<u16>,
  preserve_compatibility: bool,
) -> Result<XlsRevisionSheetLink<'a>> {
  let Some(sheet_identifier) = sheet_identifier else {
    return Ok(XlsRevisionSheetLink::NotSpecified);
  };
  match log.resolve_sheet(sheet_identifier, workbook) {
    Ok(sheet) => Ok(XlsRevisionSheetLink::Resolved(sheet)),
    Err(error) if preserve_compatibility => Ok(XlsRevisionSheetLink::Unresolved {
      sheet_identifier,
      reason: error.to_string(),
    }),
    Err(error) => Err(error),
  }
}

fn sheet_permutation(old: &[XlsSheetId], new: &[XlsSheetId]) -> Result<Vec<usize>> {
  if old.len() != new.len() {
    return Err(Error::invalid(
      0,
      "sheet reorder must contain every workbook sheet exactly once",
    ));
  }
  let old_positions = old
    .iter()
    .copied()
    .enumerate()
    .map(|(position, id)| (id, position))
    .collect::<BTreeMap<_, _>>();
  if old_positions.len() != old.len() {
    return Err(Error::invalid(
      0,
      "workbook sheet identities are not unique",
    ));
  }
  let mut seen = BTreeSet::new();
  let mut old_to_new = vec![0usize; old.len()];
  for (new_position, id) in new.iter().copied().enumerate() {
    let old_position = *old_positions.get(&id).ok_or_else(|| {
      Error::invalid(
        0,
        format!(
          "sheet identity {} does not belong to this workbook",
          id.value()
        ),
      )
    })?;
    if !seen.insert(id) {
      return Err(Error::invalid(
        0,
        format!("sheet identity {} occurs more than once", id.value()),
      ));
    }
    old_to_new[old_position] = new_position;
  }
  Ok(old_to_new)
}

fn remap_zero_based_sheet_index(
  value: u16,
  old_to_new: &[usize],
  offset: u64,
  field: &str,
  preserve_compatibility: bool,
) -> Result<u16> {
  let Some(&position) = old_to_new.get(usize::from(value)) else {
    if preserve_compatibility {
      return Ok(value);
    }
    return Err(Error::invalid(
      offset,
      format!("{field} {value} is outside the BoundSheet8 collection"),
    ));
  };
  u16::try_from(position).map_err(|_| Error::Limit("XLS sheet position exceeds u16".into()))
}

fn remap_one_based_sheet_index(
  value: u16,
  old_to_new: &[usize],
  offset: u64,
  field: &str,
  preserve_compatibility: bool,
) -> Result<u16> {
  if value == 0 {
    return Ok(0);
  }
  remap_zero_based_sheet_index(value - 1, old_to_new, offset, field, preserve_compatibility)?
    .checked_add(1)
    .ok_or_else(|| Error::Limit("XLS one-based sheet position overflow".into()))
}

fn remap_self_referencing_sheet_range(
  reference: &mut ExternSheetReference,
  old_to_new: &[usize],
  offset: u64,
  preserve_compatibility: bool,
) -> Result<()> {
  let first_signed = reference.first_sheet_index as i16;
  let last_signed = reference.last_sheet_index as i16;
  if first_signed < 0 || last_signed < 0 {
    return Ok(());
  }
  let first = usize::from(reference.first_sheet_index);
  let last = usize::from(reference.last_sheet_index);
  if first > last || last >= old_to_new.len() {
    if preserve_compatibility {
      return Ok(());
    }
    return Err(Error::invalid(
      offset,
      "self-referencing XTI sheet range is outside the BoundSheet8 collection",
    ));
  }
  let mut mapped = old_to_new[first..=last].to_vec();
  mapped.sort_unstable();
  if mapped
    .windows(2)
    .any(|pair| pair[1] != pair[0].saturating_add(1))
  {
    return Err(Error::invalid(
      offset,
      "sheet reorder would make a self-referencing XTI range non-contiguous",
    ));
  }
  reference.first_sheet_index =
    u16::try_from(mapped[0]).map_err(|_| Error::Limit("XLS sheet position exceeds u16".into()))?;
  reference.last_sheet_index = u16::try_from(*mapped.last().expect("range is nonempty"))
    .map_err(|_| Error::Limit("XLS sheet position exceeds u16".into()))?;
  Ok(())
}

fn reorder_rr_tab_id(value: &mut RrTabIdRecord, old_to_new: &[usize], offset: u64) -> Result<()> {
  if value.sheet_ids.len() != old_to_new.len() {
    return Err(Error::invalid(
      offset,
      "RRTabId count does not match the BoundSheet8 collection during reorder",
    ));
  }
  let old = value.sheet_ids.clone();
  for (old_position, &new_position) in old_to_new.iter().enumerate() {
    value.sheet_ids[new_position] = old[old_position];
  }
  Ok(())
}

fn remap_workbook_sheet_positions(
  records: &mut [BiffRecord],
  old_to_new: &[usize],
  preserve_compatibility: bool,
) -> Result<()> {
  let self_referencing_sup_books = records
    .iter()
    .filter_map(|record| match &record.data {
      BiffRecordData::SupBook(value) => Some(matches!(value.link, SupBookLink::SelfReference)),
      _ => None,
    })
    .collect::<Vec<_>>();
  for record in records {
    let offset = u64::from(record.offset);
    match &mut record.data {
      BiffRecordData::Window1(value) => {
        value.active_sheet = remap_zero_based_sheet_index(
          value.active_sheet,
          old_to_new,
          offset,
          "Window1.itabCur",
          preserve_compatibility,
        )?;
        value.first_visible_tab = remap_zero_based_sheet_index(
          value.first_visible_tab,
          old_to_new,
          offset,
          "Window1.itabFirst",
          preserve_compatibility,
        )?;
      }
      BiffRecordData::Name(value) => {
        value.sheet_index = remap_one_based_sheet_index(
          value.sheet_index,
          old_to_new,
          offset,
          "Name.itab",
          preserve_compatibility,
        )?;
      }
      BiffRecordData::RealTimeData(value) => {
        for cell in &mut value.cells {
          cell.sheet_index = remap_zero_based_sheet_index(
            cell.sheet_index,
            old_to_new,
            offset,
            "RTDCell.itab",
            preserve_compatibility,
          )?;
        }
      }
      BiffRecordData::RrTabId(value) => reorder_rr_tab_id(value, old_to_new, offset)?,
      BiffRecordData::ExternSheet(value) => {
        for reference in &mut value.references {
          if self_referencing_sup_books
            .get(usize::from(reference.sup_book_index))
            .copied()
            .unwrap_or(false)
          {
            remap_self_referencing_sheet_range(
              reference,
              old_to_new,
              offset,
              preserve_compatibility,
            )?;
          }
        }
      }
      _ => {}
    }
  }
  Ok(())
}

fn reorder_bound_sheet_records(
  workbook: &mut XlsWorkbookStream,
  order: &[XlsSheetId],
) -> Result<()> {
  let record_indices = workbook
    .tree
    .stream
    .records
    .iter()
    .enumerate()
    .filter(|(_, record)| {
      matches!(
        record.data,
        BiffRecordData::BoundSheet8(_) | BiffRecordData::BoundSheet8Compatibility { .. }
      )
    })
    .map(|(index, _)| index)
    .collect::<Vec<_>>();
  if record_indices.len() != workbook.sheet_ids.len() {
    return Err(Error::invalid(
      0,
      "BoundSheet8 collection does not match the workbook identity table",
    ));
  }
  let old_positions = workbook
    .sheet_ids
    .iter()
    .copied()
    .enumerate()
    .map(|(position, id)| (id, position))
    .collect::<BTreeMap<_, _>>();
  let tree = Arc::make_mut(&mut workbook.tree);
  let mut old_at_slot = (0..record_indices.len()).collect::<Vec<_>>();
  let mut slot_of_old = old_at_slot.clone();
  for (new_position, id) in order.iter().copied().enumerate() {
    let old_position = old_positions[&id];
    let current_position = slot_of_old[old_position];
    if current_position == new_position {
      continue;
    }
    tree.stream.records.swap(
      record_indices[new_position],
      record_indices[current_position],
    );
    old_at_slot.swap(new_position, current_position);
    slot_of_old[old_at_slot[new_position]] = new_position;
    slot_of_old[old_at_slot[current_position]] = current_position;
  }
  workbook.sheet_ids.clone_from_slice(order);
  Ok(())
}

impl XlsFile {
  /// Transactionally reorders the complete `BoundSheet8` collection by
  /// specification sheet identity. Positional MS-XLS references are
  /// remapped centrally; a permutation that would make a self-referencing
  /// multi-sheet XTI range non-contiguous is rejected without mutation.
  pub fn reorder_sheets(
    &mut self,
    workbook_name: XlsStreamName,
    order: &[XlsSheetId],
  ) -> Result<()> {
    self.reorder_sheets_with_policy(workbook_name, order, false)
  }

  pub fn reorder_sheets_preserving_compatibility(
    &mut self,
    workbook_name: XlsStreamName,
    order: &[XlsSheetId],
  ) -> Result<()> {
    self.reorder_sheets_with_policy(workbook_name, order, true)
  }

  fn reorder_sheets_with_policy(
    &mut self,
    workbook_name: XlsStreamName,
    order: &[XlsSheetId],
    preserve_compatibility: bool,
  ) -> Result<()> {
    let mut rebuilt = self.clone();
    let mut workbook_indices = rebuilt
      .workbooks
      .iter()
      .enumerate()
      .filter(|(_, workbook)| workbook.name == workbook_name)
      .map(|(index, _)| index);
    let workbook_index = workbook_indices
      .next()
      .ok_or_else(|| Error::invalid(0, format!("missing {} BIFF stream", workbook_name.path())))?;
    if workbook_indices.next().is_some() {
      return Err(Error::invalid(
        0,
        format!("multiple {} BIFF streams", workbook_name.path()),
      ));
    }
    let old_ids = rebuilt.workbooks[workbook_index].sheet_ids.clone();
    if old_ids
      .iter()
      .any(|identity| !identity.is_specification_identity())
    {
      return Err(Error::invalid(
        0,
        "sheet reorder requires specification RRTabId or large-workbook ordinal identities",
      ));
    }
    let uses_sheet_ordinals = old_ids
      .first()
      .is_some_and(|identity| identity.sheet_ordinal().is_some());
    let old_to_new = sheet_permutation(&old_ids, order)?;
    let workbooks = Arc::make_mut(&mut rebuilt.workbooks);
    remap_workbook_sheet_positions(
      &mut Arc::make_mut(&mut workbooks[workbook_index].tree)
        .stream
        .records,
      &old_to_new,
      preserve_compatibility,
    )?;
    reorder_bound_sheet_records(&mut workbooks[workbook_index], order)?;
    if uses_sheet_ordinals {
      workbooks[workbook_index].sheet_ids = (0..order.len())
        .map(XlsSheetId::from_sheet_ordinal)
        .collect::<Result<Vec<_>>>()?;
    }
    if let Some(XlsRevisionLog::Parsed(log)) = rebuilt.revision_log.as_mut().map(Arc::make_mut) {
      for record in &mut log.records {
        if let BiffRecordData::RrTabId(value) = &mut record.data {
          reorder_rr_tab_id(value, &old_to_new, u64::from(record.offset))?;
        }
      }
    }
    if preserve_compatibility {
      Arc::make_mut(&mut workbooks[workbook_index].tree).relayout_preserving_compatibility()?;
      workbooks[workbook_index].relationships_compatible()?;
    } else {
      Arc::make_mut(&mut workbooks[workbook_index].tree).relayout()?;
      workbooks[workbook_index].relationships()?;
    }
    *self = rebuilt;
    Ok(())
  }

  /// Transactionally replaces one `BoundSheet8.stName` selected by the
  /// Workbook relationship identity. A failed validation or relocation
  /// leaves the file root unchanged.
  pub fn set_sheet_name(
    &mut self,
    workbook_name: XlsStreamName,
    sheet_id: XlsSheetId,
    name: ShortXlUnicodeString,
  ) -> Result<()> {
    self.set_sheet_name_with_policy(workbook_name, sheet_id, name, false)
  }

  pub fn set_sheet_name_preserving_compatibility(
    &mut self,
    workbook_name: XlsStreamName,
    sheet_id: XlsSheetId,
    name: ShortXlUnicodeString,
  ) -> Result<()> {
    self.set_sheet_name_with_policy(workbook_name, sheet_id, name, true)
  }

  fn set_sheet_name_with_policy(
    &mut self,
    workbook_name: XlsStreamName,
    sheet_id: XlsSheetId,
    name: ShortXlUnicodeString,
    preserve_compatibility: bool,
  ) -> Result<()> {
    let code_units = short_xl_string_code_units(&name);
    let character_count = code_units.len();
    if !(1..=31).contains(&character_count) {
      return Err(Error::invalid(
        0,
        "BoundSheet8 sheet name must contain 1 through 31 characters",
      ));
    }
    const INVALID_SHEET_NAME_CHARACTERS: [u16; 9] = [
      0x0000,
      0x0003,
      b':' as u16,
      b'\\' as u16,
      b'*' as u16,
      b'?' as u16,
      b'/' as u16,
      b'[' as u16,
      b']' as u16,
    ];
    if code_units
      .iter()
      .any(|value| INVALID_SHEET_NAME_CHARACTERS.contains(value))
    {
      return Err(Error::invalid(
        0,
        "BoundSheet8 sheet name contains a forbidden character",
      ));
    }
    if code_units.first() == Some(&(b'\'' as u16)) || code_units.last() == Some(&(b'\'' as u16)) {
      return Err(Error::invalid(
        0,
        "BoundSheet8 sheet name must not begin or end with a single quote",
      ));
    }
    name.write(&mut Vec::new())?;

    let mut rebuilt = self.clone();
    let mut workbook_indices = rebuilt
      .workbooks
      .iter()
      .enumerate()
      .filter(|(_, workbook)| workbook.name == workbook_name)
      .map(|(index, _)| index);
    let workbook_index = workbook_indices
      .next()
      .ok_or_else(|| Error::invalid(0, format!("missing {} BIFF stream", workbook_name.path())))?;
    if workbook_indices.next().is_some() {
      return Err(Error::invalid(
        0,
        format!("multiple {} BIFF streams", workbook_name.path()),
      ));
    }
    let record_index = {
      let workbook = &rebuilt.workbooks[workbook_index];
      let relationships = if preserve_compatibility {
        workbook.relationships_compatible()?
      } else {
        workbook.relationships()?
      };
      let sheet = relationships.sheet(sheet_id).ok_or_else(|| {
        Error::invalid(
          0,
          format!("missing resolved sheet identity {}", sheet_id.value()),
        )
      })?;
      workbook
        .tree
        .stream
        .records
        .iter()
        .position(|record| std::ptr::eq(record, sheet.metadata_record))
        .expect("sheet metadata record belongs to its Workbook stream")
    };
    let normalized_name = normalized_short_xl_string(&name);
    let existing_names = rebuilt.workbooks[workbook_index]
      .tree
      .stream
      .records
      .iter()
      .enumerate()
      .filter(|(index, _)| *index != record_index)
      .filter_map(|(_, record)| match &record.data {
        BiffRecordData::BoundSheet8(value)
        | BiffRecordData::BoundSheet8Compatibility { value, .. } => Some(&value.name),
        _ => None,
      })
      .map(normalized_short_xl_string)
      .collect::<Vec<_>>();
    let has_duplicate = existing_names.contains(&normalized_name);
    if has_duplicate {
      return Err(Error::invalid(
        0,
        "BoundSheet8 sheet name must be unique ignoring case",
      ));
    }
    let workbooks = Arc::make_mut(&mut rebuilt.workbooks);
    let value = match &mut Arc::make_mut(&mut workbooks[workbook_index].tree)
      .stream
      .records[record_index]
      .data
    {
      BiffRecordData::BoundSheet8(value)
      | BiffRecordData::BoundSheet8Compatibility { value, .. } => value,
      _ => unreachable!("sheet identity points to BoundSheet8"),
    };
    value.name = name;
    if preserve_compatibility {
      Arc::make_mut(&mut workbooks[workbook_index].tree).relayout_preserving_compatibility()?;
    } else {
      Arc::make_mut(&mut workbooks[workbook_index].tree).relayout()?;
    }
    *self = rebuilt;
    Ok(())
  }

  /// Transactionally edits one unique logical cell selected by sheet
  /// identity and coordinate. The closure receives its exact static wire
  /// variant; failed edits or relationship/layout validation do not mutate
  /// the original file root.
  pub fn edit_cell<T>(
    &mut self,
    workbook_name: XlsStreamName,
    sheet_id: XlsSheetId,
    row: u16,
    column: u16,
    edit: impl FnOnce(XlsCellMut<'_>) -> Result<T>,
  ) -> Result<T> {
    self.edit_cell_with_policy(workbook_name, sheet_id, row, column, false, edit)
  }

  pub fn edit_cell_preserving_compatibility<T>(
    &mut self,
    workbook_name: XlsStreamName,
    sheet_id: XlsSheetId,
    row: u16,
    column: u16,
    edit: impl FnOnce(XlsCellMut<'_>) -> Result<T>,
  ) -> Result<T> {
    self.edit_cell_with_policy(workbook_name, sheet_id, row, column, true, edit)
  }

  fn edit_cell_with_policy<T>(
    &mut self,
    workbook_name: XlsStreamName,
    sheet_id: XlsSheetId,
    row: u16,
    column: u16,
    preserve_compatibility: bool,
    edit: impl FnOnce(XlsCellMut<'_>) -> Result<T>,
  ) -> Result<T> {
    let mut rebuilt = self.clone();
    let mut workbook_indices = rebuilt
      .workbooks
      .iter()
      .enumerate()
      .filter(|(_, workbook)| workbook.name == workbook_name)
      .map(|(index, _)| index);
    let workbook_index = workbook_indices
      .next()
      .ok_or_else(|| Error::invalid(0, format!("missing {} BIFF stream", workbook_name.path())))?;
    if workbook_indices.next().is_some() {
      return Err(Error::invalid(
        0,
        format!("multiple {} BIFF streams", workbook_name.path()),
      ));
    }
    let target = {
      let workbook = &rebuilt.workbooks[workbook_index];
      let relationships = if preserve_compatibility {
        workbook.relationships_compatible()?
      } else {
        workbook.relationships()?
      };
      let sheet = relationships.sheet(sheet_id).ok_or_else(|| {
        Error::invalid(
          0,
          format!("missing resolved sheet identity {}", sheet_id.value()),
        )
      })?;
      let index = if preserve_compatibility {
        sheet.sparse_cell_index_compatible()?
      } else {
        sheet.sparse_cell_index()?
      };
      let cell = index
        .cell(row, column)?
        .ok_or_else(|| Error::invalid(0, format!("sheet has no cell at ({row}, {column})")))?;
      let record = workbook
        .tree
        .stream
        .records
        .iter()
        .position(|record| std::ptr::eq(record, cell.source_record))
        .expect("cell source record belongs to its Workbook stream");
      match cell.value {
        XlsCellValueRef::MulRk { index, .. } => XlsCellMutationTarget::MulRk {
          record,
          element: index,
        },
        XlsCellValueRef::MulBlank { index, .. } => XlsCellMutationTarget::MulBlank {
          record,
          element: index,
        },
        _ => XlsCellMutationTarget::Record(record),
      }
    };
    let record_index = match target {
      XlsCellMutationTarget::Record(record)
      | XlsCellMutationTarget::MulRk { record, .. }
      | XlsCellMutationTarget::MulBlank { record, .. } => record,
    };
    let workbooks = Arc::make_mut(&mut rebuilt.workbooks);
    let cell = match (
      &mut Arc::make_mut(&mut workbooks[workbook_index].tree)
        .stream
        .records[record_index]
        .data,
      target,
    ) {
      (BiffRecordData::Formula(value), XlsCellMutationTarget::Record(_)) => {
        XlsCellMut::Formula(value)
      }
      (BiffRecordData::Formula4Compatibility(value), XlsCellMutationTarget::Record(_)) => {
        XlsCellMut::Formula4Compatibility(value)
      }
      (BiffRecordData::Blank(value), XlsCellMutationTarget::Record(_)) => XlsCellMut::Blank(value),
      (BiffRecordData::Number(value), XlsCellMutationTarget::Record(_)) => {
        XlsCellMut::Number(value)
      }
      (BiffRecordData::BoolErr(value), XlsCellMutationTarget::Record(_)) => {
        XlsCellMut::BoolErr(value)
      }
      (BiffRecordData::Label(value), XlsCellMutationTarget::Record(_)) => XlsCellMut::Label(value),
      (BiffRecordData::LabelSst(value), XlsCellMutationTarget::Record(_)) => {
        XlsCellMut::LabelSst(value)
      }
      (BiffRecordData::Rk(value), XlsCellMutationTarget::Record(_)) => XlsCellMut::Rk(value),
      (BiffRecordData::MulRk(value), XlsCellMutationTarget::MulRk { element, .. }) => {
        XlsCellMut::MulRk(&mut value.cells[element])
      }
      (BiffRecordData::MulBlank(value), XlsCellMutationTarget::MulBlank { element, .. }) => {
        XlsCellMut::MulBlankFormat(&mut value.format_indices[element])
      }
      _ => unreachable!("logical cell target retains its static record identity"),
    };
    let result = edit(cell)?;
    if preserve_compatibility {
      Arc::make_mut(&mut workbooks[workbook_index].tree).relayout_preserving_compatibility()?;
      workbooks[workbook_index].relationships_compatible()?;
    } else {
      Arc::make_mut(&mut workbooks[workbook_index].tree).relayout()?;
      workbooks[workbook_index].relationships()?;
    }
    *self = rebuilt;
    Ok(result)
  }

  /// Returns the immutable parse-time CFB backing used to preserve unknown
  /// and externally-owned entries.
  ///
  /// Managed stream bytes in this snapshot do not reflect subsequent typed
  /// edits. Use [`Self::to_compound_file`] to inspect the current serialized
  /// file.
  pub fn source_compound_file(&self) -> &CompoundFile {
    &self.compound_file
  }

  /// Builds the strict MS-XLS 2.1.7 storage/stream inventory.
  pub fn storages_and_streams(&self) -> Result<XlsStoragesAndStreams<'_>> {
    let inventory = self.build_storages_and_streams();
    if let Some(issue) = inventory.issues.first() {
      return Err(Error::invalid(
        0,
        format!("MS-XLS storage/stream relationship is invalid: {issue:?}"),
      ));
    }
    Ok(inventory)
  }

  /// Builds the same zero-copy inventory while retaining producer-specific
  /// entries and exposing all relationship problems through `issues()`.
  pub fn storages_and_streams_compatible(&self) -> XlsStoragesAndStreams<'_> {
    self.build_storages_and_streams()
  }

  fn build_storages_and_streams(&self) -> XlsStoragesAndStreams<'_> {
    let pivot_cache_path = self
      .compound_file
      .entries()
      .iter()
      .find(|entry| {
        is_root_child(entry) && entry.name.eq_ignore_ascii_case(PIVOT_CACHE_STORAGE_NAME)
      })
      .map(|entry| entry.path.as_path());
    let mut entries = Vec::new();
    let mut issues = Vec::new();
    let mut singletons = BTreeMap::<&'static str, &Entry>::new();

    for entry in self
      .compound_file
      .entries()
      .iter()
      .filter(|entry| entry.kind != EntryKind::Root)
    {
      let role = if is_root_child(entry) {
        classify_root_xls_entry(&entry.name)
      } else if entry.path.parent() == pivot_cache_path {
        match parse_fixed_hex_name(&entry.name, b"", 4) {
          Some(cache_id) if entry.kind == EntryKind::Stream => XlsFileEntryRole::PivotCacheStream {
            cache_id: cache_id as u16,
          },
          _ => {
            issues.push(XlsFileEntryIssue::InvalidPivotCacheChild {
              path: entry.path.clone(),
            });
            XlsFileEntryRole::Other
          }
        }
      } else {
        XlsFileEntryRole::Other
      };

      match expected_entry_kind(role) {
        Some(EntryKind::Stream) if entry.kind != EntryKind::Stream => {
          issues.push(XlsFileEntryIssue::ExpectedStream {
            path: entry.path.clone(),
          });
        }
        Some(EntryKind::Storage) if entry.kind != EntryKind::Storage => {
          issues.push(XlsFileEntryIssue::ExpectedStorage {
            path: entry.path.clone(),
          });
        }
        _ => {}
      }
      if let Some(key) = singleton_role_name(role)
        && let Some(first) = singletons.insert(key, entry)
      {
        issues.push(XlsFileEntryIssue::DuplicateSingleton {
          role: key,
          first: first.path.clone(),
          duplicate: entry.path.clone(),
        });
      }
      entries.push(XlsFileEntryRef { entry, role });
    }

    XlsStoragesAndStreams { entries, issues }
  }

  pub fn workbook_stream(&self, name: XlsStreamName) -> Option<&XlsWorkbookStream> {
    self.workbooks.iter().find(|workbook| workbook.name == name)
  }

  pub fn pivot_cache(&self, stream_id: u16) -> Result<Option<&XlsPivotCache>> {
    let mut caches = self
      .pivot_caches
      .iter()
      .filter(|cache| cache.stream_id() == stream_id);
    let first = caches.next();
    if caches.next().is_some() {
      return Err(Error::invalid(
        0,
        format!("multiple PivotCache streams have id {stream_id:04X}"),
      ));
    }
    Ok(first)
  }

  pub fn resolve_pivot_cache(
    &self,
    definition: XlsPivotCacheDefinitionRef<'_>,
  ) -> Result<&XlsPivotCache> {
    self.pivot_cache(definition.stream_id())?.ok_or_else(|| {
      Error::invalid(
        u64::from(definition.source_record.offset),
        format!(
          "SXStreamID references missing PivotCache stream {:04X}",
          definition.stream_id()
        ),
      )
    })
  }

  /// Resolves the complete PivotTable relationship from a sheet `SxView`
  /// through Globals `SXStreamID` to the file-root-owned PivotCache stream.
  pub fn resolve_pivot_table<'a>(
    &'a self,
    workbook: &XlsWorkbookView<'a>,
    view: XlsPivotTableViewRef<'a>,
  ) -> Result<XlsPivotTableRef<'a>> {
    let definition = workbook.resolve_pivot_table_cache_definition(view)?;
    let cache = self.resolve_pivot_cache(definition)?;
    Ok(XlsPivotTableRef {
      view,
      definition,
      cache,
    })
  }

  pub fn resolve_pivot_table_compatible<'a>(
    &'a self,
    workbook: &XlsWorkbookView<'a>,
    view: XlsPivotTableViewRef<'a>,
  ) -> XlsPivotTableLink<'a> {
    let definition = match workbook.resolve_pivot_table_cache_definition_compatible(view) {
      XlsPivotTableCacheLink::Resolved(value) => value,
      XlsPivotTableCacheLink::Unresolved { error, .. } => {
        let error = match error {
          XlsPivotTableCacheLinkError::Negative { cache_index } => {
            XlsPivotTableLinkError::NegativeDefinitionIndex { cache_index }
          }
          XlsPivotTableCacheLinkError::Missing { cache_index } => {
            XlsPivotTableLinkError::MissingDefinition { cache_index }
          }
          XlsPivotTableCacheLinkError::ForeignView => XlsPivotTableLinkError::ForeignView,
        };
        return XlsPivotTableLink::Unresolved { view, error };
      }
    };
    let mut caches = self
      .pivot_caches
      .iter()
      .filter(|cache| cache.stream_id() == definition.stream_id());
    let Some(cache) = caches.next() else {
      return XlsPivotTableLink::Unresolved {
        view,
        error: XlsPivotTableLinkError::MissingCacheStream {
          stream_id: definition.stream_id(),
        },
      };
    };
    if caches.next().is_some() {
      return XlsPivotTableLink::Unresolved {
        view,
        error: XlsPivotTableLinkError::AmbiguousCacheStream {
          stream_id: definition.stream_id(),
        },
      };
    }
    XlsPivotTableLink::Resolved(XlsPivotTableRef {
      view,
      definition,
      cache,
    })
  }

  /// Starts zero-copy relationship navigation at a named Workbook Stream.
  pub fn workbook_view(&self, name: XlsStreamName) -> Result<Option<XlsWorkbookView<'_>>> {
    self
      .workbook_stream(name)
      .map(XlsWorkbookStream::relationships)
      .transpose()
  }

  pub fn workbook_view_compatible(
    &self,
    name: XlsStreamName,
  ) -> Result<Option<XlsWorkbookView<'_>>> {
    self
      .workbook_stream(name)
      .map(XlsWorkbookStream::relationships_compatible)
      .transpose()
  }

  /// Starts zero-copy navigation at the standalone Revision Stream.
  pub fn revision_stream_view(&self) -> Result<Option<XlsRevisionStreamView<'_>>> {
    self
      .revision_log
      .as_deref()
      .and_then(XlsRevisionLog::stream)
      .map(RevisionLogStream::relationships)
      .transpose()
  }

  pub fn revision_stream_view_compatible(&self) -> Result<Option<XlsRevisionStreamView<'_>>> {
    self
      .revision_log
      .as_deref()
      .and_then(XlsRevisionLog::stream)
      .map(RevisionLogStream::relationships_compatible)
      .transpose()
  }

  /// Builds every Revision Stream production and its Workbook relationships
  /// in one root-level graph. Strict mode rejects an unresolved sheet,
  /// custom view, or production boundary.
  pub fn revision_graph<'view, 'a>(
    &'a self,
    workbook: &'view XlsWorkbookView<'a>,
  ) -> Result<Option<XlsRevisionGraph<'view, 'a>>>
  where
    'a: 'view,
  {
    self.revision_graph_with_policy(workbook, false)
  }

  pub fn revision_graph_compatible<'view, 'a>(
    &'a self,
    workbook: &'view XlsWorkbookView<'a>,
  ) -> Result<Option<XlsRevisionGraph<'view, 'a>>>
  where
    'a: 'view,
  {
    self.revision_graph_with_policy(workbook, true)
  }

  fn revision_graph_with_policy<'view, 'a>(
    &'a self,
    workbook: &'view XlsWorkbookView<'a>,
    preserve_compatibility: bool,
  ) -> Result<Option<XlsRevisionGraph<'view, 'a>>>
  where
    'a: 'view,
  {
    if !self
      .workbooks
      .iter()
      .any(|candidate| std::ptr::eq(candidate, workbook.workbook))
    {
      return Err(Error::invalid(
        0,
        "revision graph Workbook view does not belong to this XlsFile",
      ));
    }
    let Some(stream) = (if preserve_compatibility {
      self.revision_stream_view_compatible()?
    } else {
      self.revision_stream_view()?
    }) else {
      return Ok(None);
    };
    let mut logs = Vec::with_capacity(stream.revision_logs.len());
    for log in stream.revision_logs.iter().copied() {
      let (revisions, unlinked_records) = if preserve_compatibility {
        let view = log.revision_records_compatible();
        (view.revisions, view.unlinked_records)
      } else {
        (log.revision_records()?, Vec::new())
      };
      let mut nodes = Vec::with_capacity(revisions.len());
      for source in revisions {
        let sheet = revision_sheet_link(
          log,
          workbook,
          source.sheet_identifier(),
          preserve_compatibility,
        )?;
        let local_sheet_identifier = match &source {
          XlsRevisionRecordRef::DefinedName { value, .. } if value.local_sheet_id != u16::MAX => {
            Some(value.local_sheet_id)
          }
          _ => None,
        };
        let local_name_sheet = revision_sheet_link(
          log,
          workbook,
          local_sheet_identifier,
          preserve_compatibility,
        )?;
        let custom_view = if preserve_compatibility {
          source.resolve_custom_view_compatible(workbook)
        } else {
          source
            .resolve_custom_view(workbook)?
            .map(XlsCustomViewLink::Resolved)
        };
        nodes.push(XlsRevisionNode {
          source,
          sheet,
          local_name_sheet,
          custom_view,
        });
      }
      logs.push(XlsRevisionGraphLog {
        source: log,
        revisions: nodes,
        unlinked_records,
      });
    }
    Ok(Some(XlsRevisionGraph { stream, logs }))
  }

  /// Starts zero-copy navigation at the standalone shared-workbook user log.
  pub fn user_log_view(&self) -> Result<Option<XlsUserLogView<'_>>> {
    self
      .user_names
      .as_deref()
      .and_then(XlsUserNames::stream)
      .map(UserNamesStream::relationships)
      .transpose()
  }

  pub fn user_log_view_compatible(&self) -> Result<Option<XlsUserLogView<'_>>> {
    self
      .user_names
      .as_deref()
      .and_then(XlsUserNames::stream)
      .map(UserNamesStream::relationships_compatible)
      .transpose()
  }

  /// Opens a path in strict mode and returns its owned MS-XLS tree.
  pub fn open(path: impl AsRef<Path>) -> Result<Self> {
    Ok(Self::open_with_options(path, ParseOptions::default())?.into_value())
  }

  /// Opens a path in compatible mode, returning every structured diagnostic
  /// alongside the owned tree.
  pub fn open_compatible(path: impl AsRef<Path>) -> Result<ParseOutcome<Self>> {
    Self::open_with_options(path, ParseOptions::compatible(Limits::default()))
  }

  pub fn open_with_options(
    path: impl AsRef<Path>,
    options: ParseOptions,
  ) -> Result<ParseOutcome<Self>> {
    let compound = compound_from_path(path.as_ref(), options, BinaryFormat::Xls)?;
    Self::from_compound_outcome(compound, options)
  }

  /// Parses a complete CFB byte slice in strict mode.
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Ok(Self::from_bytes_with_options(bytes, ParseOptions::default())?.into_value())
  }

  /// Parses a complete CFB byte slice in compatible mode.
  pub fn from_bytes_compatible(bytes: &[u8]) -> Result<ParseOutcome<Self>> {
    Self::from_bytes_with_options(bytes, ParseOptions::compatible(Limits::default()))
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    Ok(Self::from_bytes_with_options(bytes, ParseOptions::strict(limits))?.into_value())
  }

  pub fn from_bytes_with_options(
    bytes: &[u8],
    options: ParseOptions,
  ) -> Result<ParseOutcome<Self>> {
    let compound = compound_from_bytes(bytes, options, BinaryFormat::Xls)?;
    Self::from_compound_outcome(compound, options)
  }

  /// Consumes a complete CFB image without copying its full archive buffer.
  pub fn from_vec(bytes: Vec<u8>) -> Result<Self> {
    Ok(Self::from_vec_with_options(bytes, ParseOptions::default())?.into_value())
  }

  pub fn from_vec_compatible(bytes: Vec<u8>) -> Result<ParseOutcome<Self>> {
    Self::from_vec_with_options(bytes, ParseOptions::compatible(Limits::default()))
  }

  pub fn from_vec_with_limits(bytes: Vec<u8>, limits: Limits) -> Result<Self> {
    Ok(Self::from_vec_with_options(bytes, ParseOptions::strict(limits))?.into_value())
  }

  pub fn from_vec_with_options(
    bytes: Vec<u8>,
    options: ParseOptions,
  ) -> Result<ParseOutcome<Self>> {
    let compound = compound_from_vec(bytes, options, BinaryFormat::Xls)?;
    Self::from_compound_outcome(compound, options)
  }

  /// Consumes an owned CFB and parses its managed streams in strict mode.
  pub fn from_compound_file(compound_file: CompoundFile) -> Result<Self> {
    Ok(Self::from_compound_file_with_options(compound_file, ParseOptions::default())?.into_value())
  }

  pub fn from_compound_file_compatible(compound_file: CompoundFile) -> Result<ParseOutcome<Self>> {
    Self::from_compound_file_with_options(
      compound_file,
      ParseOptions::compatible(Limits::default()),
    )
  }

  pub fn from_compound_file_with_limits(
    compound_file: CompoundFile,
    limits: Limits,
  ) -> Result<Self> {
    Ok(
      Self::from_compound_file_with_options(compound_file, ParseOptions::strict(limits))?
        .into_value(),
    )
  }

  pub fn from_compound_file_with_options(
    compound_file: CompoundFile,
    options: ParseOptions,
  ) -> Result<ParseOutcome<Self>> {
    let compound = compound_outcome(compound_file, options, BinaryFormat::Xls)?;
    Self::from_compound_outcome(compound, options)
  }

  /// Rebuilds every managed BIFF stream's derived physical layout after
  /// callers edit the public Rust record trees.
  pub fn relayout(&mut self) -> Result<()> {
    let mut rebuilt = self.clone();
    for workbook in Arc::make_mut(&mut rebuilt.workbooks) {
      Arc::make_mut(&mut workbook.tree).relayout()?;
    }
    for cache in Arc::make_mut(&mut rebuilt.pivot_caches) {
      if let XlsPivotCache::Parsed { stream, .. } = cache {
        stream.relayout()?;
      }
    }
    if let Some(XlsRevisionLog::Parsed(log)) = rebuilt.revision_log.as_mut().map(Arc::make_mut) {
      log.relayout()?;
    }
    if let Some(XlsUserNames::Parsed(users)) = rebuilt.user_names.as_mut().map(Arc::make_mut) {
      users.relayout()?;
    }
    *self = rebuilt;
    Ok(())
  }

  fn from_compound_outcome(
    compound: ParseOutcome<CompoundFile>,
    options: ParseOptions,
  ) -> Result<ParseOutcome<Self>> {
    let ParseOutcome {
      value: compound_file,
      mut diagnostics,
    } = compound;
    let mut workbooks = Vec::new();
    for name in [XlsStreamName::Workbook, XlsStreamName::Book] {
      if let Some(bytes) = compound_file.stream(name.path()) {
        let workbook = XlsWorkbookStream::from_tree(
          name,
          BiffWorkbookTree::from_bytes_with_limits(bytes, options.limits)?,
        )?;
        audit_workbook(&workbook, options.is_strict(), &mut diagnostics)?;
        workbooks.push(workbook);
      }
    }
    if workbooks.is_empty() {
      return Err(Error::invalid(0, "Workbook/Book stream is missing"));
    }
    let pivot_cache_storage = compound_file.entries().iter().find(|entry| {
      is_root_child(entry)
        && entry.kind == EntryKind::Storage
        && entry.name.eq_ignore_ascii_case(PIVOT_CACHE_STORAGE_NAME)
    });
    let mut pivot_caches = Vec::new();
    if let Some(storage) = pivot_cache_storage {
      for entry in compound_file.entries().iter().filter(|entry| {
        entry.kind == EntryKind::Stream && entry.path.parent() == Some(storage.path.as_path())
      }) {
        let Some(stream_id) = parse_fixed_hex_name(&entry.name, b"", 4) else {
          continue;
        };
        let stream_id = stream_id as u16;
        match PivotCacheStream::from_bytes_with_limits(&entry.data, options.limits) {
          Ok(stream) => {
            if stream
              .properties()
              .is_some_and(|properties| properties.has_zero_length_refresh_user_compatibility())
            {
              if options.is_strict() {
                return Err(Error::invalid(
                  0,
                  "PivotCache SXDB.cchWho is zero; MS-XLS requires 0xFFFF or 1..=255",
                ));
              }
              diagnostics.push(ParseDiagnostic::warning(
                ParseDiagnosticCode::NonconformingRecord,
                BinaryFormat::Xls,
                entry.path.to_str(),
                Some(0),
                "SXDB",
                SpecificationReference {
                  document: "MS-XLS",
                  section: "2.4.275",
                },
                "preserved LibreOffice SXDB.cchWho=0 compatibility encoding",
              ));
            }
            if stream
              .properties()
              .is_some_and(|properties| properties.stream_id != stream_id)
            {
              if options.is_strict() {
                return Err(Error::invalid(
                  0,
                  format!(
                    "PivotCache SXDB stream id does not match {}",
                    entry.path.display()
                  ),
                ));
              }
              diagnostics.push(ParseDiagnostic::warning(
                ParseDiagnosticCode::InvalidReference,
                BinaryFormat::Xls,
                entry.path.to_str(),
                Some(0),
                "PivotCache",
                SpecificationReference {
                  document: "MS-XLS",
                  section: "2.1.7.12",
                },
                "preserved a PivotCache whose SXDB.idstm differs from its stream name",
              ));
            }
            pivot_caches.push(XlsPivotCache::Parsed { stream_id, stream });
          }
          Err(error) if options.is_strict() => {
            return Err(Error::invalid(
              error.offset().unwrap_or(0),
              format!("PivotCache {} is invalid: {error}", entry.path.display()),
            ));
          }
          Err(error) => {
            diagnostics.push(ParseDiagnostic::warning(
              ParseDiagnosticCode::InvalidStreamPreserved,
              BinaryFormat::Xls,
              entry.path.to_str(),
              error.offset(),
              "PivotCache",
              SpecificationReference {
                document: "MS-XLS",
                section: "2.1.7.12",
              },
              format!("preserved an invalid PivotCache stream: {error}"),
            ));
            pivot_caches.push(XlsPivotCache::Compatibility {
              stream_id,
              bytes: entry.data.to_vec(),
              reason: error.to_string(),
            });
          }
        }
      }
    }
    pivot_caches.sort_by_key(XlsPivotCache::stream_id);
    let revision_log = match compound_file.stream(super::REVISION_LOG_STREAM_PATH) {
      Some(bytes) => match if options.is_strict() {
        RevisionLogStream::from_bytes_with_limits(bytes, options.limits)
      } else {
        RevisionLogStream::from_bytes_compatible_with_limits(bytes, options.limits)
      } {
        Ok(value) => {
          if !options.is_strict()
            && let Err(error) = value.validate()
          {
            diagnostics.push(ParseDiagnostic::warning(
              ParseDiagnosticCode::NonconformingRecord,
              BinaryFormat::Xls,
              Some(super::REVISION_LOG_STREAM_PATH),
              error.offset(),
              "Revision Stream",
              SpecificationReference {
                document: "MS-XLS",
                section: "2.1.7.14",
              },
              format!("preserved a typed nonconforming Revision Stream: {error}"),
            ));
          }
          Some(XlsRevisionLog::Parsed(value))
        }
        Err(error) if options.is_strict() => {
          let offset = error.offset().unwrap_or(0);
          return Err(Error::invalid(
            offset,
            format!("Revision Stream violates MS-XLS 2.1.7.14: {error}"),
          ));
        }
        Err(error) => {
          let offset = error.offset().unwrap_or(0);
          diagnostics.push(ParseDiagnostic::warning(
            ParseDiagnosticCode::InvalidStreamPreserved,
            BinaryFormat::Xls,
            Some(super::REVISION_LOG_STREAM_PATH),
            Some(offset),
            "Revision Stream",
            SpecificationReference {
              document: "MS-XLS",
              section: "2.1.7.14",
            },
            format!("preserved an invalid Revision Stream: {error}"),
          ));
          Some(XlsRevisionLog::Compatibility {
            bytes: bytes.to_vec(),
            reason: error.to_string(),
          })
        }
      },
      None => None,
    };
    let user_names = match compound_file.stream(super::USER_NAMES_STREAM_PATH) {
      Some(bytes) => {
        let parsed = if options.is_strict() {
          UserNamesStream::from_bytes_with_limits(bytes, options.limits)
        } else {
          UserNamesStream::from_bytes_compatible_with_limits(bytes, options.limits)
        };
        match parsed {
          Ok(value) => {
            if !options.is_strict()
              && let Err(error) = value.validate()
            {
              diagnostics.push(ParseDiagnostic::warning(
                ParseDiagnosticCode::NonconformingRecord,
                BinaryFormat::Xls,
                Some(super::USER_NAMES_STREAM_PATH),
                error.offset(),
                "User Names Stream",
                SpecificationReference {
                  document: "MS-XLS",
                  section: "2.1.7.17",
                },
                format!("preserved a typed nonconforming User Names Stream: {error}"),
              ));
            }
            Some(XlsUserNames::Parsed(value))
          }
          Err(error) if options.is_strict() => {
            return Err(Error::invalid(
              error.offset().unwrap_or(0),
              format!("User Names Stream violates MS-XLS 2.1.7.17: {error}"),
            ));
          }
          Err(error) => {
            diagnostics.push(ParseDiagnostic::warning(
              ParseDiagnosticCode::InvalidStreamPreserved,
              BinaryFormat::Xls,
              Some(super::USER_NAMES_STREAM_PATH),
              error.offset(),
              "User Names Stream",
              SpecificationReference {
                document: "MS-XLS",
                section: "2.1.7.17",
              },
              format!("preserved an invalid User Names Stream: {error}"),
            ));
            Some(XlsUserNames::Compatibility {
              bytes: bytes.to_vec(),
              reason: error.to_string(),
            })
          }
        }
      }
      None => None,
    };
    if options.is_strict()
      && let Some(XlsRevisionLog::Parsed(revisions)) = &revision_log
    {
      revisions.relationships()?;
    }
    if options.is_strict()
      && let Some(XlsUserNames::Parsed(users)) = &user_names
    {
      let users = users.relationships()?;
      let Some(XlsRevisionLog::Parsed(revisions)) = &revision_log else {
        return Err(Error::invalid(
          0,
          "shared workbook has a User Names Stream but no valid Revision Stream",
        ));
      };
      let revisions = revisions.relationships()?;
      for user in users.users() {
        users.resolve_revision_log(*user, &revisions)?;
      }
    }
    let shared = OfficeSharedContent::from_compound_file_with_host(
      &compound_file,
      options,
      Some(OfficeHostKind::Xls),
    )?;
    diagnostics.extend(shared.diagnostics);
    Ok(ParseOutcome::new(
      Self {
        compound_file,
        shared: shared.value,
        workbooks: Arc::new(workbooks),
        pivot_caches: Arc::new(pivot_caches),
        revision_log: revision_log.map(Arc::new),
        user_names: user_names.map(Arc::new),
      },
      diagnostics,
    ))
  }

  /// Transactionally replaces one host VBA module source. VBA caches, SRP
  /// streams and the OLEPS VBA signature are invalidated by the shared tree.
  pub fn replace_vba_module_source(
    &mut self,
    stream_name: &str,
    source: &[u8],
  ) -> Result<OfficeVbaModuleMutation> {
    let mut candidate = self.clone();
    let report = candidate
      .shared
      .replace_vba_module_source(stream_name, source)?;
    *self = candidate;
    Ok(report)
  }

  /// Transactionally edits one VBA Designer storage through the shared Office tree.
  pub fn edit_vba_designer_storage(
    &mut self,
    index: usize,
    edit: impl FnOnce(&mut ParentControlStorageModel) -> Result<()>,
  ) -> Result<OfficeFormsMutation> {
    let mut candidate = self.clone();
    let report = candidate.shared.edit_vba_designer_storage(index, edit)?;
    *self = candidate;
    Ok(report)
  }

  /// Rebuilds every managed BIFF stream and returns a strict CFB.
  pub fn to_compound_file(&self) -> Result<CompoundFile> {
    self.to_compound_file_with_options(SaveOptions::default())
  }

  /// Rebuilds managed BIFF streams while retaining compatibility nodes.
  pub fn to_compound_file_preserving_compatibility(&self) -> Result<CompoundFile> {
    self.to_compound_file_with_options(SaveOptions::preserving_compatibility())
  }

  /// Rebuilds managed BIFF streams under the requested compatibility policy.
  pub fn to_compound_file_with_options(&self, options: SaveOptions) -> Result<CompoundFile> {
    self.build_compound_file(options, true)
  }

  fn build_compound_file(
    &self,
    options: SaveOptions,
    materialize_workbooks: bool,
  ) -> Result<CompoundFile> {
    if self.workbooks.is_empty() {
      return Err(Error::invalid(0, "XLS file has no BIFF workbook stream"));
    }
    if [XlsStreamName::Workbook, XlsStreamName::Book]
      .into_iter()
      .any(|name| {
        self
          .workbooks
          .iter()
          .filter(|workbook| workbook.name == name)
          .count()
          > 1
      })
    {
      return Err(Error::invalid(
        0,
        "XLS file has duplicate BIFF stream names",
      ));
    }
    if !options.preserves_compatibility() {
      for workbook in self.workbooks.iter() {
        audit_workbook(workbook, true, &mut Vec::new())?;
      }
    }
    let mut compound = self.compound_file.clone();
    for name in [XlsStreamName::Workbook, XlsStreamName::Book] {
      if let Some(workbook) = self.workbooks.iter().find(|workbook| workbook.name == name) {
        if materialize_workbooks {
          let bytes = if options.preserves_compatibility() {
            workbook.tree.to_bytes_preserving_compatibility()?
          } else {
            workbook.tree.to_bytes()?
          };
          compound.upsert_stream(name.path(), bytes)?;
        } else if !compound.is_stream(name.path()) {
          compound.upsert_stream(name.path(), Vec::new())?;
        }
      } else if compound.is_stream(name.path()) {
        compound.remove_stream(name.path())?;
      }
    }
    let source_pivot_streams = compound
      .entries()
      .iter()
      .filter(|entry| {
        entry.kind == EntryKind::Stream
          && entry.path.parent().is_some_and(|parent| {
            compound
              .entry(parent)
              .is_some_and(|storage| storage.name.eq_ignore_ascii_case(PIVOT_CACHE_STORAGE_NAME))
          })
          && parse_fixed_hex_name(&entry.name, b"", 4).is_some()
      })
      .map(|entry| entry.path.clone())
      .collect::<Vec<_>>();
    for path in source_pivot_streams {
      compound.remove_stream(path)?;
    }
    if !self.pivot_caches.is_empty() && !compound.is_storage(PIVOT_CACHE_STORAGE_PATH) {
      compound.create_storage(PIVOT_CACHE_STORAGE_PATH)?;
    }
    for cache in self.pivot_caches.iter() {
      let path = format!("/{PIVOT_CACHE_STORAGE_NAME}/{:04X}", cache.stream_id());
      let bytes = match cache {
        XlsPivotCache::Parsed { stream, .. } => stream.to_bytes()?,
        XlsPivotCache::Compatibility { .. } if !options.preserves_compatibility() => {
          return Err(Error::invalid(
            0,
            "strict save rejects an invalid PivotCache stream",
          ));
        }
        XlsPivotCache::Compatibility { bytes, .. } => bytes.clone(),
      };
      compound.upsert_stream(path, bytes)?;
    }
    match self.revision_log.as_deref() {
      Some(XlsRevisionLog::Parsed(log)) => {
        if !options.preserves_compatibility() {
          log.validate()?;
          log.relationships()?;
        }
        compound.upsert_stream(super::REVISION_LOG_STREAM_PATH, log.to_bytes()?)?;
      }
      Some(XlsRevisionLog::Compatibility { .. }) if !options.preserves_compatibility() => {
        return Err(Error::invalid(
          0,
          "strict save rejects an invalid Revision Stream",
        ));
      }
      Some(XlsRevisionLog::Compatibility { bytes, .. }) => {
        compound.overwrite_stream(super::REVISION_LOG_STREAM_PATH, bytes.clone())?;
      }
      None if compound.is_stream(super::REVISION_LOG_STREAM_PATH) => {
        compound.remove_stream(super::REVISION_LOG_STREAM_PATH)?;
      }
      None => {}
    }
    match self.user_names.as_deref() {
      Some(XlsUserNames::Parsed(users)) => {
        if !options.preserves_compatibility() {
          users.validate()?;
        }
        compound.upsert_stream(super::USER_NAMES_STREAM_PATH, users.to_bytes()?)?;
      }
      Some(XlsUserNames::Compatibility { .. }) if !options.preserves_compatibility() => {
        return Err(Error::invalid(
          0,
          "strict save rejects an invalid User Names Stream",
        ));
      }
      Some(XlsUserNames::Compatibility { bytes, .. }) => {
        compound.upsert_stream(super::USER_NAMES_STREAM_PATH, bytes.clone())?;
      }
      None if compound.is_stream(super::USER_NAMES_STREAM_PATH) => {
        compound.remove_stream(super::USER_NAMES_STREAM_PATH)?;
      }
      None => {}
    }
    self.shared.write_to_compound_file(&mut compound, options)?;
    Ok(compound)
  }

  fn workbook_write_plans(
    &self,
    preserve_compatibility: bool,
  ) -> Result<Vec<(XlsStreamName, BiffStreamWritePlan<'_>)>> {
    self
      .workbooks
      .iter()
      .map(|workbook| {
        Ok((
          workbook.name,
          workbook.tree.write_plan(preserve_compatibility)?,
        ))
      })
      .collect()
  }

  fn workbook_stream_overrides<'a>(
    plans: &'a [(XlsStreamName, BiffStreamWritePlan<'a>)],
  ) -> Result<Vec<CfbStreamOverride<'a>>> {
    plans
      .iter()
      .map(|(name, plan)| {
        Ok(CfbStreamOverride::new(
          Path::new(name.path()),
          plan.encoded_len()?,
          plan,
        ))
      })
      .collect()
  }

  fn to_bytes_streaming_workbooks(&self, options: SaveOptions) -> Result<Vec<u8>> {
    let compound = self.build_compound_file(options, false)?;
    let plans = self.workbook_write_plans(options.preserves_compatibility())?;
    let overrides = Self::workbook_stream_overrides(&plans)?;
    compound.to_bytes_with_stream_overrides(&overrides)
  }

  fn write_streaming_workbooks(&self, writer: impl Write, options: SaveOptions) -> Result<()> {
    let compound = self.build_compound_file(options, false)?;
    let plans = self.workbook_write_plans(options.preserves_compatibility())?;
    let overrides = Self::workbook_stream_overrides(&plans)?;
    compound.write_to_with_stream_overrides(&overrides, writer)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.to_bytes_with_options(SaveOptions::default())
  }

  pub fn to_bytes_preserving_compatibility(&self) -> Result<Vec<u8>> {
    self.to_bytes_with_options(SaveOptions::preserving_compatibility())
  }

  pub fn to_bytes_with_options(&self, options: SaveOptions) -> Result<Vec<u8>> {
    self.to_bytes_streaming_workbooks(options)
  }

  pub fn write_to(&self, writer: impl Write) -> Result<()> {
    self.write_to_with_options(writer, SaveOptions::default())
  }

  pub fn write_to_preserving_compatibility(&self, writer: impl Write) -> Result<()> {
    self.write_to_with_options(writer, SaveOptions::preserving_compatibility())
  }

  pub fn write_to_with_options(&self, writer: impl Write, options: SaveOptions) -> Result<()> {
    self.write_streaming_workbooks(writer, options)
  }

  pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
    self.save_with_options(path, SaveOptions::default())
  }

  pub fn save_preserving_compatibility(&self, path: impl AsRef<Path>) -> Result<()> {
    self.save_with_options(path, SaveOptions::preserving_compatibility())
  }

  pub fn save_with_options(&self, path: impl AsRef<Path>, options: SaveOptions) -> Result<()> {
    self.write_streaming_workbooks(std::io::sink(), options)?;
    self.write_streaming_workbooks(std::fs::File::create(path)?, options)
  }
}

fn short_xl_string_code_units(value: &ShortXlUnicodeString) -> Vec<u16> {
  value.value.encode_utf16().collect()
}

fn normalized_short_xl_string(value: &ShortXlUnicodeString) -> String {
  value.value.to_lowercase()
}

fn is_root_child(entry: &Entry) -> bool {
  entry.path.parent() == Some(Path::new("/"))
}

fn parse_fixed_hex_name(name: &str, prefix: &[u8], digits: usize) -> Option<u32> {
  let bytes = name.as_bytes();
  if bytes.len() != prefix.len().checked_add(digits)?
    || !bytes.get(..prefix.len())?.eq_ignore_ascii_case(prefix)
  {
    return None;
  }
  let suffix = bytes.get(prefix.len()..)?;
  if !suffix.iter().all(u8::is_ascii_hexdigit) {
    return None;
  }
  std::str::from_utf8(suffix)
    .ok()
    .and_then(|value| u32::from_str_radix(value, 16).ok())
}

fn classify_root_xls_entry(name: &str) -> XlsFileEntryRole {
  if let Some(object_id) = parse_fixed_hex_name(name, b"MBD", 8) {
    return XlsFileEntryRole::EmbeddingStorage { object_id };
  }
  if let Some(object_id) = parse_fixed_hex_name(name, b"LNK", 8) {
    return XlsFileEntryRole::LinkStorage { object_id };
  }
  if name.eq_ignore_ascii_case("\u{1}CompObj") {
    XlsFileEntryRole::ComponentObjectStream
  } else if name.eq_ignore_ascii_case("Ctls") {
    XlsFileEntryRole::ControlStream
  } else if name.eq_ignore_ascii_case("\u{6}DataSpaces") {
    XlsFileEntryRole::DataSpacesStorage
  } else if name
    .eq_ignore_ascii_case(OfficePropertySetKind::DocumentSummaryInformation.stream_name())
  {
    XlsFileEntryRole::DocumentSummaryInformationStream
  } else if name.eq_ignore_ascii_case("encryption") {
    XlsFileEntryRole::EncryptionStream
  } else if name.eq_ignore_ascii_case("List Data") {
    XlsFileEntryRole::ListDataStream
  } else if name.eq_ignore_ascii_case("MsoDataStore") {
    XlsFileEntryRole::OfficeDataStoreStorage
  } else if name.eq_ignore_ascii_case("XCB") {
    XlsFileEntryRole::OfficeToolbarsStream
  } else if name.eq_ignore_ascii_case("\u{1}Ole") {
    XlsFileEntryRole::OleStream
  } else if name.eq_ignore_ascii_case(PIVOT_CACHE_STORAGE_NAME) {
    XlsFileEntryRole::PivotCacheStorage
  } else if name.eq_ignore_ascii_case("\u{9}DRMContent") {
    XlsFileEntryRole::ProtectedContentStream
  } else if name.eq_ignore_ascii_case(REVISION_LOG_STREAM_NAME) {
    XlsFileEntryRole::RevisionStream
  } else if name.eq_ignore_ascii_case("_signatures") {
    XlsFileEntryRole::SignaturesStream
  } else if name.eq_ignore_ascii_case(OfficePropertySetKind::SummaryInformation.stream_name()) {
    XlsFileEntryRole::SummaryInformationStream
  } else if name.eq_ignore_ascii_case(USER_NAMES_STREAM_NAME) {
    XlsFileEntryRole::UserNamesStream
  } else if name.eq_ignore_ascii_case(crate::vba::XLS_VBA_PROJECT_STORAGE_NAME) {
    XlsFileEntryRole::VbaStorage
  } else if name.eq_ignore_ascii_case("\u{9}DRMViewerContent") {
    XlsFileEntryRole::ViewerContentStream
  } else if name.eq_ignore_ascii_case(XlsStreamName::Workbook.name()) {
    XlsFileEntryRole::WorkbookStream(XlsStreamName::Workbook)
  } else if name.eq_ignore_ascii_case(XlsStreamName::Book.name()) {
    XlsFileEntryRole::WorkbookStream(XlsStreamName::Book)
  } else if name.eq_ignore_ascii_case("_xmlsignatures") {
    XlsFileEntryRole::XmlSignaturesStorage
  } else if name.eq_ignore_ascii_case("XML") {
    XlsFileEntryRole::XmlStream
  } else {
    XlsFileEntryRole::Other
  }
}

fn expected_entry_kind(role: XlsFileEntryRole) -> Option<EntryKind> {
  match role {
    XlsFileEntryRole::DataSpacesStorage
    | XlsFileEntryRole::EmbeddingStorage { .. }
    | XlsFileEntryRole::LinkStorage { .. }
    | XlsFileEntryRole::OfficeDataStoreStorage
    | XlsFileEntryRole::PivotCacheStorage
    | XlsFileEntryRole::VbaStorage
    | XlsFileEntryRole::XmlSignaturesStorage => Some(EntryKind::Storage),
    XlsFileEntryRole::Other => None,
    _ => Some(EntryKind::Stream),
  }
}

fn singleton_role_name(role: XlsFileEntryRole) -> Option<&'static str> {
  match role {
    XlsFileEntryRole::ComponentObjectStream => Some("Component Object Stream"),
    XlsFileEntryRole::ControlStream => Some("Control Stream"),
    XlsFileEntryRole::DataSpacesStorage => Some("Data Spaces Storage"),
    XlsFileEntryRole::DocumentSummaryInformationStream => {
      Some("Document Summary Information Stream")
    }
    XlsFileEntryRole::EncryptionStream => Some("Encryption Stream"),
    XlsFileEntryRole::ListDataStream => Some("List Data Stream"),
    XlsFileEntryRole::OfficeDataStoreStorage => Some("Office Data Store Storage"),
    XlsFileEntryRole::OfficeToolbarsStream => Some("Office Toolbars Stream"),
    XlsFileEntryRole::OleStream => Some("OLE Stream"),
    XlsFileEntryRole::PivotCacheStorage => Some("Pivot Cache Storage"),
    XlsFileEntryRole::ProtectedContentStream => Some("Protected Content Stream"),
    XlsFileEntryRole::RevisionStream => Some("Revision Stream"),
    XlsFileEntryRole::SignaturesStream => Some("Signatures Stream"),
    XlsFileEntryRole::SummaryInformationStream => Some("Summary Information Stream"),
    XlsFileEntryRole::UserNamesStream => Some("User Names Stream"),
    XlsFileEntryRole::VbaStorage => Some("VBA Storage"),
    XlsFileEntryRole::ViewerContentStream => Some("Viewer Content Stream"),
    XlsFileEntryRole::WorkbookStream(XlsStreamName::Workbook) => Some("Workbook Stream"),
    XlsFileEntryRole::WorkbookStream(XlsStreamName::Book) => Some("Book Stream"),
    XlsFileEntryRole::XmlSignaturesStorage => Some("XML Signatures Storage"),
    XlsFileEntryRole::XmlStream => Some("XML Stream"),
    XlsFileEntryRole::EmbeddingStorage { .. }
    | XlsFileEntryRole::LinkStorage { .. }
    | XlsFileEntryRole::PivotCacheStream { .. }
    | XlsFileEntryRole::Other => None,
  }
}

fn audit_workbook(
  workbook: &XlsWorkbookStream,
  strict: bool,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<()> {
  audit_workbook_topology(workbook, strict, diagnostics)?;
  audit_auto_filter_context(workbook, strict, diagnostics)?;
  let biff8 = workbook.tree.stream.is_biff8();
  for record in &workbook.tree.stream.records {
    match &record.data {
      BiffRecordData::Bof(value) if biff8 => {
        audit_bof(workbook, record, value, strict, diagnostics)?;
      }
      BiffRecordData::Formula(value) | BiffRecordData::Formula4Compatibility(value) => {
        audit_formula_streams(
          workbook,
          record,
          [&value.tokens.rgce],
          strict,
          diagnostics,
          "Formula",
          "2.4.127",
        )?;
      }
      BiffRecordData::SharedFormula(value) => audit_formula_streams(
        workbook,
        record,
        [&value.tokens.rgce],
        strict,
        diagnostics,
        "ShrFmla",
        "2.4.260",
      )?,
      BiffRecordData::Array(value) => audit_formula_streams(
        workbook,
        record,
        [&value.tokens.rgce],
        strict,
        diagnostics,
        "Array",
        "2.4.4",
      )?,
      BiffRecordData::DataValidation(value) => audit_formula_streams(
        workbook,
        record,
        [&value.formula1.tokens, &value.formula2.tokens],
        strict,
        diagnostics,
        "Dv",
        "2.4.95",
      )?,
      BiffRecordData::ConditionalFormatting(value) => audit_formula_streams(
        workbook,
        record,
        [&value.formula1, &value.formula2],
        strict,
        diagnostics,
        "CF",
        "2.4.42",
      )?,
      BiffRecordData::ConditionalFormatting12(value) => audit_formula_streams(
        workbook,
        record,
        [&value.formula1, &value.formula2, &value.active_formula],
        strict,
        diagnostics,
        "CF12",
        "2.4.43",
      )?,
      BiffRecordData::Name(value) => audit_formula_streams(
        workbook,
        record,
        [&value.formula],
        strict,
        diagnostics,
        "Lbl",
        "2.4.150",
      )?,
      BiffRecordData::ChartLinkedData(value) => audit_formula_streams(
        workbook,
        record,
        [&value.formula],
        strict,
        diagnostics,
        "BRAI",
        "2.4.29",
      )?,
      BiffRecordData::ChartEndObject(value) if !matches!(value.object_kind, 0x0010..=0x0012) => {
        report_record_issue(
          workbook,
          record,
          strict,
          diagnostics,
          xls_issue(
            ParseDiagnosticCode::NonconformingRecord,
            "EndObject",
            "2.4.101",
          ),
          format!(
            "iObjectKind is {:#06x}, outside the specified 0x0010..=0x0012 range",
            value.object_kind
          ),
        )?;
      }
      BiffRecordData::Guts(value)
        if super::validate_guts(value, u64::from(record.offset)).is_err() =>
      {
        report_record_issue(
          workbook,
          record,
          strict,
          diagnostics,
          xls_issue(ParseDiagnosticCode::NonconformingRecord, "Guts", "2.4.134"),
          format!(
            "iLevelRwMac/iLevelColMac values are outside 0 or 2 through 8 ({:#06x}, {:#06x})",
            value.maximum_row_outline_level, value.maximum_column_outline_level
          ),
        )?;
      }
      BiffRecordData::PhoneticInfo(value)
        if super::validate_phonetic_info(value, u64::from(record.offset)).is_err() =>
      {
        report_record_issue(
          workbook,
          record,
          strict,
          diagnostics,
          xls_issue(
            ParseDiagnosticCode::NonconformingRecord,
            "PhoneticInfo",
            "2.4.192",
          ),
          "Phs or SqRef fields violate the FontIndex, range-count, or Ref8 constraints".into(),
        )?;
      }
      BiffRecordData::ExtSst(value) => {
        let nonzero_reserved = value
          .buckets
          .iter()
          .filter(|bucket| bucket.reserved != 0)
          .count();
        let invalid_offsets = value
          .buckets
          .iter()
          .filter(|bucket| u32::from(bucket.record_offset) >= bucket.stream_offset)
          .count();
        let invalid_buckets = value
          .buckets
          .iter()
          .filter(|bucket| {
            bucket.reserved != 0 || u32::from(bucket.record_offset) >= bucket.stream_offset
          })
          .count();
        if invalid_buckets != 0 {
          report_record_issue(
            workbook,
            record,
            strict,
            diagnostics,
            xls_issue(
              ParseDiagnosticCode::NonconformingRecord,
              "ISSTInf",
              "2.5.167",
            ),
            format!(
              "ExtSST contains {invalid_buckets} nonconforming bucket(s): {nonzero_reserved} with a nonzero reserved field and {invalid_offsets} with cbOffset not less than ib"
            ),
          )?;
        }
      }
      BiffRecordData::Hyperlink(value) => match &value.object {
        HyperlinkObject::Parsed { .. } => {}
        HyperlinkObject::Truncated { payload, .. } => report_record_issue(
          workbook,
          record,
          strict,
          diagnostics,
          xls_issue(ParseDiagnosticCode::TruncatedRecord, "HLink", "2.4.140"),
          format!(
            "Hyperlink Object is truncated with {} retained bytes",
            payload.len()
          ),
        )?,
        HyperlinkObject::TruncatedUrlMoniker {
          declared_byte_length,
          address,
          ..
        } => report_record_issue(
          workbook,
          record,
          strict,
          diagnostics,
          xls_issue(ParseDiagnosticCode::TruncatedRecord, "HLink", "2.4.140"),
          format!(
            "URL moniker declares {declared_byte_length} bytes but only {} UTF-16 units are available",
            address.len()
          ),
        )?,
        HyperlinkObject::Compatibility(bytes) => report_record_issue(
          workbook,
          record,
          strict,
          diagnostics,
          xls_issue(ParseDiagnosticCode::NonconformingRecord, "HLink", "2.4.140"),
          format!("Hyperlink Object has {} nonconforming bytes", bytes.len()),
        )?,
      },
      BiffRecordData::FeatureHeader(value)
        if matches!(value.data, FeatureHeaderData::Malformed { .. }) =>
      {
        report_record_issue(
          workbook,
          record,
          strict,
          diagnostics,
          xls_issue(
            ParseDiagnosticCode::NonconformingRecord,
            "FeatHdr",
            "2.4.112",
          ),
          "FeatHdr contains a marker or payload outside its shared-feature schema".into(),
        )?;
      }
      BiffRecordData::Pls(value) => {
        if let Some(devmode) = pls_devmode(value) {
          // MS-RPRN 2.2.2.1 explicitly requires consumers to accept
          // _DEVMODE values with truncated public information. Only
          // missing bytes from the declared driver-private tail make
          // the containing Pls record incomplete.
          if !devmode.driver_extra_complete {
            report_record_issue(
              workbook,
              record,
              strict,
              diagnostics,
              xls_issue(ParseDiagnosticCode::TruncatedRecord, "Pls", "2.4.199"),
              format!(
                "DEVMODEW declares {} driver-private bytes but only {} are available",
                devmode.declared_driver_extra_size,
                devmode.driver_extra.len()
              ),
            )?;
          }
        }
      }
      BiffRecordData::MsoDrawingGroup(value) => audit_drawing(
        workbook,
        record,
        value,
        strict,
        diagnostics,
        "MsoDrawingGroup",
        "2.4.171",
      )?,
      BiffRecordData::MsoDrawing(value) => audit_drawing(
        workbook,
        record,
        value,
        strict,
        diagnostics,
        "MsoDrawing",
        "2.4.170",
      )?,
      BiffRecordData::Sst(value) => {
        if let SstCompletion::Truncated {
          first_unparsed_string,
          reason,
        } = &value.completion
        {
          report_record_issue(
            workbook,
            record,
            strict,
            diagnostics,
            xls_issue(ParseDiagnosticCode::TruncatedRecord, "SST", "2.4.265"),
            format!("SST stopped at string {first_unparsed_string}: {reason}"),
          )?;
        }
      }
      _ => {}
    }
  }
  Ok(())
}

fn audit_auto_filter_context(
  workbook: &XlsWorkbookStream,
  strict: bool,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<()> {
  let mut document_type = None;
  let mut auto_filter_entries = None;
  for record in &workbook.tree.stream.records {
    if let BiffRecordData::Bof(value) = &record.data {
      document_type = Some(value.document_type);
      auto_filter_entries = None;
    }
    let allowed_substream = matches!(document_type, Some(0x0010 | 0x0040));
    match &record.data {
      BiffRecordData::FixedU16 {
        kind: super::FixedU16RecordKind::AutoFilterInfo,
        value,
      } => {
        let valid_count = (1..=256).contains(value);
        let unique = auto_filter_entries.is_none();
        if !allowed_substream || !valid_count || !unique {
          report_record_issue(
            workbook,
            record,
            strict,
            diagnostics,
            xls_issue(
              ParseDiagnosticCode::NonconformingRecord,
              "AutoFilterInfo",
              "2.4.8",
            ),
            format!(
              "cEntries={value} has invalid substream, range, or cardinality (unique={unique})"
            ),
          )?;
        }
        if allowed_substream && valid_count && unique {
          auto_filter_entries = Some(*value);
        }
      }
      BiffRecordData::AutoFilter(value) => {
        if !allowed_substream
          || auto_filter_entries.is_none_or(|entries| value.entry_index >= entries)
        {
          report_record_issue(
            workbook,
            record,
            strict,
            diagnostics,
            xls_issue(
              ParseDiagnosticCode::NonconformingRecord,
              "AutoFilter",
              "2.4.6",
            ),
            format!(
              "iEntry={} is outside its worksheet/macro-sheet AutoFilterInfo",
              value.entry_index
            ),
          )?;
        }
      }
      BiffRecordData::AutoFilter12(value) => {
        if !allowed_substream
          || value.flags.worksheet
            && auto_filter_entries.is_none_or(|entries| value.entry_index >= entries)
        {
          report_record_issue(
            workbook,
            record,
            strict,
            diagnostics,
            xls_issue(
              ParseDiagnosticCode::NonconformingRecord,
              "AutoFilter12",
              "2.4.7",
            ),
            format!(
              "iEntry={} is outside its worksheet/macro-sheet AutoFilterInfo context",
              value.entry_index
            ),
          )?;
        }
      }
      BiffRecordData::SortData(_) if !allowed_substream => {
        report_record_issue(
          workbook,
          record,
          strict,
          diagnostics,
          xls_issue(
            ParseDiagnosticCode::NonconformingRecord,
            "SortData",
            "2.4.264",
          ),
          "SortData is outside a worksheet or macro-sheet substream".into(),
        )?;
      }
      _ => {}
    }
  }
  Ok(())
}

fn audit_bof(
  workbook: &XlsWorkbookStream,
  record: &BiffRecord,
  value: &super::BofRecord,
  strict: bool,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<()> {
  let mut violations = Vec::new();
  if value.version != 0x0600 {
    violations.push(format!("vers is {:#06x}, expected 0x0600", value.version));
  }
  if !matches!(value.document_type, 0x0005 | 0x0010 | 0x0020 | 0x0040) {
    violations.push(format!(
      "dt is {:#06x}, outside the four specified substream kinds",
      value.document_type
    ));
  }
  if !matches!(value.build_year, 0x07cc | 0x07cd) {
    violations.push(format!(
      "rupYear is {:#06x}, expected 0x07cc or 0x07cd",
      value.build_year
    ));
  }
  let history = value.history_flags;
  let must_be_zero = (history & 0x0000_0136) | (history & 0xfff8_0000);
  if history & 1 == 0 || must_be_zero != 0 {
    violations.push(format!(
      "history flags violate fWin/reserved MUST values (raw {history:#010x})"
    ));
  }
  let highest_version = (history >> 14) & 0x0f;
  if !matches!(highest_version, 0 | 1 | 2 | 3 | 4 | 6 | 7) {
    violations.push(format!("verXLHigh has reserved value {highest_version:#x}"));
  }
  let lowest = value.lowest_version;
  let lowest_biff = lowest & 0xff;
  let last_saved = (lowest >> 8) & 0x0f;
  if lowest_biff != 6
    || !matches!(last_saved, 0 | 1 | 2 | 3 | 4 | 6 | 7)
    || last_saved > highest_version
    || lowest & 0xffff_f000 != 0
  {
    violations.push(format!(
            "verLowestBiff/verLastXLSaved/reserved values are invalid (raw {lowest:#010x}, verXLHigh {highest_version:#x})"
        ));
  }
  if violations.is_empty() {
    return Ok(());
  }
  report_record_issue(
    workbook,
    record,
    strict,
    diagnostics,
    xls_issue(ParseDiagnosticCode::NonconformingRecord, "BOF", "2.4.21"),
    violations.join("; "),
  )
}

fn audit_workbook_topology(
  workbook: &XlsWorkbookStream,
  strict: bool,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<()> {
  let first_offset = workbook
    .tree
    .stream
    .records
    .first()
    .map_or(0, |record| u64::from(record.offset));
  if workbook.name == XlsStreamName::Book {
    report_workbook_issue(
      workbook,
      first_offset,
      strict,
      diagnostics,
      xls_issue(
        ParseDiagnosticCode::NonconformingRecord,
        "Workbook Stream",
        "2.1.7.20",
      ),
      "legacy stream name is Book; MS-XLS requires Workbook".into(),
    )?;
  }

  if !workbook.tree.stream.is_biff8() {
    report_workbook_issue(
      workbook,
      first_offset,
      strict,
      diagnostics,
      xls_issue(
        ParseDiagnosticCode::NonconformingRecord,
        "Workbook Stream",
        "2.1.7.20",
      ),
      "legacy BIFF stream is outside the current MS-XLS Workbook Stream grammar".into(),
    )?;
    return Ok(());
  }

  if !workbook.tree.outside_substream_ranges.is_empty() {
    report_workbook_issue(
      workbook,
      first_offset,
      strict,
      diagnostics,
      xls_issue(
        ParseDiagnosticCode::NonconformingRecord,
        "Workbook Stream",
        "2.1.7.20",
      ),
      format!(
        "records outside BOF/EOF substreams occur in ranges {:?}",
        workbook.tree.outside_substream_ranges
      ),
    )?;
  }

  let roots = &workbook.tree.substreams;
  let globals_count = roots
    .iter()
    .filter(|node| node.kind == BiffSubstreamKind::WorkbookGlobals)
    .count();
  if globals_count != 1
    || roots
      .first()
      .is_none_or(|node| node.kind != BiffSubstreamKind::WorkbookGlobals)
  {
    report_workbook_issue(
      workbook,
      first_offset,
      strict,
      diagnostics,
      xls_issue(
        ParseDiagnosticCode::NonconformingRecord,
        "Globals Substream",
        "2.1.7.20.3",
      ),
      format!(
        "Workbook Stream has {globals_count} top-level Globals Substreams and the first substream kind is {:?}",
        roots.first().map(|node| node.kind)
      ),
    )?;
  }

  let following = roots.get(1..).unwrap_or_default();
  let invalid_following = following
    .iter()
    .enumerate()
    .filter_map(|(index, node)| {
      (!matches!(
        node.kind,
        BiffSubstreamKind::WorksheetOrDialogSheet
          | BiffSubstreamKind::ChartSheet
          | BiffSubstreamKind::MacroSheet
      ))
      .then_some((index + 1, node.kind))
    })
    .collect::<Vec<_>>();
  if following.is_empty() || !invalid_following.is_empty() {
    report_workbook_issue(
      workbook,
      first_offset,
      strict,
      diagnostics,
      xls_issue(
        ParseDiagnosticCode::NonconformingRecord,
        "Workbook Stream",
        "2.1.7.20",
      ),
      format!(
        "Workbook Stream has {} following sheet substreams; invalid (index, kind) values are {invalid_following:?}",
        following.len()
      ),
    )?;
  }
  Ok(())
}

fn audit_formula_streams<'a>(
  workbook: &XlsWorkbookStream,
  record: &BiffRecord,
  formulas: impl IntoIterator<Item = &'a FormulaTokenStream>,
  strict: bool,
  diagnostics: &mut Vec<ParseDiagnostic>,
  structure: &'static str,
  section: &'static str,
) -> Result<()> {
  let mut unparsed_bytes = 0usize;
  let mut missing_extra_count = 0usize;
  let mut nonconforming_token_count = 0usize;
  for formula in formulas {
    unparsed_bytes = unparsed_bytes.saturating_add(formula.unparsed_tail.len());
    missing_extra_count = missing_extra_count.saturating_add(formula.missing_extra_count());
    nonconforming_token_count =
      nonconforming_token_count.saturating_add(formula.nonconforming_token_count());
  }
  if unparsed_bytes != 0 {
    report_record_issue(
      workbook,
      record,
      strict,
      diagnostics,
      xls_issue(ParseDiagnosticCode::NonconformingRecord, structure, section),
      format!(
        "formula contains {unparsed_bytes} bytes beginning with an unknown or reserved Ptg opcode"
      ),
    )?;
  }
  if nonconforming_token_count != 0 {
    report_record_issue(
      workbook,
      record,
      strict,
      diagnostics,
      xls_issue(ParseDiagnosticCode::NonconformingRecord, structure, section),
      format!(
        "formula contains {nonconforming_token_count} Ptg token(s) with a reserved opcode, reserved bit, or out-of-range natural-language cell reference"
      ),
    )?;
  }
  if missing_extra_count == 0 {
    return Ok(());
  }
  report_record_issue(
    workbook,
    record,
    strict,
    diagnostics,
    xls_issue(ParseDiagnosticCode::TruncatedRecord, structure, section),
    format!(
      "formula is missing {missing_extra_count} required MS-XLS 2.5.198.103 RgbExtra structures"
    ),
  )
}

fn pls_devmode(value: &PlsRecord) -> Option<&DevModeW> {
  match &value.settings {
    PrinterSettings::WindowsUnicode(value)
    | PrinterSettings::LengthPrefixedWindowsUnicode { devmode: value, .. } => Some(value),
    _ => None,
  }
}

fn audit_drawing(
  workbook: &XlsWorkbookStream,
  record: &BiffRecord,
  value: &super::MsoDrawingRecord,
  strict: bool,
  diagnostics: &mut Vec<ParseDiagnostic>,
  structure: &'static str,
  section: &'static str,
) -> Result<()> {
  let message = match &value.data {
    MsoDrawingData::Complete(_) => return Ok(()),
    MsoDrawingData::Partial(value) => format!(
      "OfficeArt sequence is partial: {} incomplete records and {} unparsed bytes",
      value.incomplete_record_count(),
      value.unparsed_byte_count()
    ),
    MsoDrawingData::Incomplete { bytes, reason } => {
      format!(
        "OfficeArt sequence retains {} incomplete bytes: {reason}",
        bytes.len()
      )
    }
  };
  report_record_issue(
    workbook,
    record,
    strict,
    diagnostics,
    xls_issue(ParseDiagnosticCode::TruncatedRecord, structure, section),
    message,
  )
}

#[derive(Clone, Copy)]
struct XlsIssueSpec {
  code: ParseDiagnosticCode,
  structure: &'static str,
  section: &'static str,
}

const fn xls_issue(
  code: ParseDiagnosticCode,
  structure: &'static str,
  section: &'static str,
) -> XlsIssueSpec {
  XlsIssueSpec {
    code,
    structure,
    section,
  }
}

fn report_record_issue(
  workbook: &XlsWorkbookStream,
  record: &BiffRecord,
  strict: bool,
  diagnostics: &mut Vec<ParseDiagnostic>,
  issue: XlsIssueSpec,
  message: String,
) -> Result<()> {
  report_workbook_issue(
    workbook,
    u64::from(record.offset),
    strict,
    diagnostics,
    issue,
    message,
  )
}

fn report_workbook_issue(
  workbook: &XlsWorkbookStream,
  offset: u64,
  strict: bool,
  diagnostics: &mut Vec<ParseDiagnostic>,
  issue: XlsIssueSpec,
  message: String,
) -> Result<()> {
  if strict {
    return Err(Error::invalid(
      offset,
      format!(
        "{} violates MS-XLS {}: {message}",
        workbook.name.path(),
        issue.section,
      ),
    ));
  }
  diagnostics.push(ParseDiagnostic::warning(
    issue.code,
    BinaryFormat::Xls,
    Some(workbook.name.path()),
    Some(offset),
    issue.structure,
    SpecificationReference {
      document: "MS-XLS",
      section: issue.section,
    },
    message,
  ));
  Ok(())
}

#[derive(Debug)]
struct OpenSubstream {
  kind: BiffSubstreamKind,
  start: usize,
  children: Vec<BiffSubstreamNode>,
}

fn matching_ranges(values: &[bool], target: bool) -> Vec<Range<usize>> {
  let mut ranges = Vec::new();
  let mut start = None;
  for (index, value) in values.iter().copied().enumerate() {
    match (value == target, start) {
      (true, None) => start = Some(index),
      (false, Some(from)) => {
        ranges.push(from..index);
        start = None;
      }
      _ => {}
    }
  }
  if let Some(from) = start {
    ranges.push(from..values.len());
  }
  ranges
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    cfb::Version,
    xls::{
      BiffUnicodeString, BofRecord, BoundSheet8Record, CellHeader, ChartEndObjectRecord,
      ColInfoReserved, DevModeFields, DevModeWPublic, ExtSstRecord, FixedU16RecordKind,
      FontAttributes, FontRecord, FormatRecord, FormulaCachedResult, FormulaRecord,
      FormulaSpecialCachedResult, FormulaToken, FormulaTokenStream, FormulaTokens, FrtFlags,
      FrtHeaderOld, GutsRecord, IsstInf, NumberRecord, PhoneticFlags, PhoneticInfoRecord,
      ShortXlUnicodeString, SupBookLink, XfRecord, XlStringCharacters,
    },
  };

  fn record(offset: u32, data: BiffRecordData) -> BiffRecord {
    BiffRecord { offset, data }
  }

  fn bof(document_type: u16) -> BofRecord {
    BofRecord {
      version: 0x0600,
      document_type,
      build_identifier: 0,
      build_year: 0x07cc,
      history_flags: 1,
      lowest_version: 6,
    }
  }

  #[test]
  fn indexes_nested_bof_eof_without_flattening_records() {
    let stream = BiffStream {
      records: vec![
        record(0, BiffRecordData::Bof(bof(0x0010))),
        record(20, BiffRecordData::Bof(bof(0x0020))),
        record(40, BiffRecordData::Eof),
        record(44, BiffRecordData::Eof),
      ],
      trailing_padding: Vec::new(),
    };
    let mut tree = BiffWorkbookTree::from_stream(stream).unwrap();
    assert_eq!(tree.substreams.len(), 1);
    assert_eq!(tree.substreams[0].record_range, 0..4);
    assert_eq!(tree.substreams[0].children[0].record_range, 1..3);
    assert!(tree.outside_substream_ranges.is_empty());

    tree
      .stream
      .records
      .insert(2, record(30, BiffRecordData::CodePage { code_page: 1252 }));
    tree.relayout().unwrap();
    assert_eq!(tree.substreams[0].record_range, 0..5);
    assert_eq!(tree.substreams[0].children[0].record_range, 1..4);
    assert_eq!(tree.stream.records[2].offset, 40);
    assert_eq!(tree.to_bytes().unwrap().len(), 54);
  }

  fn workbook_bytes() -> Vec<u8> {
    BiffStream {
      records: vec![
        record(0, BiffRecordData::Bof(bof(0x0005))),
        record(20, BiffRecordData::Eof),
        record(24, BiffRecordData::Bof(bof(0x0010))),
        record(44, BiffRecordData::Eof),
      ],
      trailing_padding: Vec::new(),
    }
    .to_bytes()
    .unwrap()
  }

  #[test]
  fn workbook_relationships_join_globals_sheet_and_cell_records_without_copying() {
    let metadata = BoundSheet8Record {
      sheet_bof_offset: 100,
      state: 0,
      sheet_type: 0,
      name: ShortXlUnicodeString {
        flags: 0,
        value: "Sheet1".to_owned(),
      },
    };
    let xf = XfRecord {
      font_index: 0,
      number_format_index: 0x00a4,
      cell_flags: 0,
      alignment_flags: 0,
      indentation_flags: 0,
      border_style_flags: 0,
      border_color_flags: 0,
      additional_border_color_flags: 0,
      fill_flags: 0,
    };
    let font = FontRecord {
      height_twips: 200,
      attributes: FontAttributes::empty(),
      color_index: 0,
      bold_weight: 400,
      escapement: 0,
      underline: 0,
      family: 0,
      charset: 0,
      reserved: 0,
      name: BiffUnicodeString {
        flags: 0,
        characters: XlStringCharacters::Compressed(b"Arial".to_vec()),
        trailing_byte: None,
      },
    };
    let format = FormatRecord {
      format_index: 0x00a4,
      declared_character_count: 4,
      format_string: BiffUnicodeString {
        flags: 0,
        characters: XlStringCharacters::Compressed(b"0.00".to_vec()),
        trailing_byte: None,
      },
    };
    let workbook = XlsWorkbookStream::from_tree(
      XlsStreamName::Workbook,
      BiffWorkbookTree::from_stream(BiffStream {
        records: vec![
          record(0, BiffRecordData::Bof(bof(0x0005))),
          record(20, BiffRecordData::BoundSheet8(metadata.clone())),
          record(40, BiffRecordData::Xf(xf)),
          record(50, BiffRecordData::Font(font.clone())),
          record(60, BiffRecordData::Format(format.clone())),
          record(
            65,
            BiffRecordData::SupBook(SupBookRecord {
              sheet_count: 1,
              link: SupBookLink::SelfReference,
            }),
          ),
          record(
            70,
            BiffRecordData::ExternSheet(super::super::ExternSheetRecord {
              reference_count: 1,
              references: vec![ExternSheetReference {
                sup_book_index: 0,
                first_sheet_index: 0,
                last_sheet_index: 0,
              }],
            }),
          ),
          record(80, BiffRecordData::Eof),
          record(100, BiffRecordData::Bof(bof(0x0010))),
          record(
            120,
            BiffRecordData::Number(NumberRecord {
              cell: CellHeader {
                row: 2,
                column: 3,
                format_index: 0,
              },
              value_bits: 42.0f64.to_bits(),
            }),
          ),
          record(140, BiffRecordData::Eof),
        ],
        trailing_padding: Vec::new(),
      })
      .unwrap(),
    )
    .unwrap();

    let view = workbook.relationships_compatible().unwrap();
    assert_eq!(view.globals_records().len(), 8);
    assert_eq!(view.sheets().len(), 1);
    let sheet = view.sheets()[0];
    assert_eq!(sheet.id().value(), 1);
    assert_eq!(sheet.metadata(), &metadata);
    assert_eq!(sheet.kind(), BiffSubstreamKind::WorksheetOrDialogSheet);
    assert!(std::ptr::eq(
      sheet.metadata_record(),
      &workbook.tree.stream.records[1]
    ));
    assert!(std::ptr::eq(
      sheet.records().as_ptr(),
      workbook.tree.stream.records[8..11].as_ptr()
    ));
    let cells = sheet.cell_records().collect::<Vec<_>>();
    assert_eq!(cells.len(), 1);
    assert!(matches!(cells[0].data, BiffRecordData::Number(_)));
    assert_eq!(view.xf(0), Some(&xf));
    assert_eq!(view.supporting_links().len(), 1);
    assert_eq!(view.supporting_links()[0].id().index(), 0);
    assert_eq!(view.external_sheets().len(), 1);
    assert_eq!(
      view.resolve_external_sheet(0).unwrap().supporting_link_id(),
      Some(XlsSupportingLinkId(0))
    );
    let BiffRecordData::Number(number) = &cells[0].data else {
      unreachable!()
    };
    assert_eq!(
      view.resolve_cell_format(&number.cell).unwrap(),
      XlsCellFormatRef {
        xf: &xf,
        font: &font,
        number_format: XlsNumberFormatRef::Custom(&format),
        custom_number_format_code: Some("0.00".to_owned()),
      }
    );
  }

  #[test]
  fn logical_cells_expand_mul_records_and_preserve_parent_identity() {
    let metadata = BoundSheet8Record {
      sheet_bof_offset: 100,
      state: 0,
      sheet_type: 0,
      name: ShortXlUnicodeString {
        flags: 0,
        value: "Cells".to_owned(),
      },
    };
    let mul_rk = MulRkRecord {
      row: 2,
      first_column: 3,
      cells: vec![
        MulRkCell {
          format_index: 7,
          value: 10,
        },
        MulRkCell {
          format_index: 8,
          value: 20,
        },
      ],
      last_column: 4,
    };
    let mul_blank = MulBlankRecord {
      row: 5,
      first_column: 6,
      format_indices: vec![9, 10],
      last_column: 7,
    };
    let merge = MergeCellsRecord {
      range_count: 1,
      ranges: vec![CellRange {
        first_row: 2,
        last_row: 2,
        first_column: 3,
        last_column: 4,
      }],
    };
    let workbook = XlsWorkbookStream::from_tree(
      XlsStreamName::Workbook,
      BiffWorkbookTree::from_stream(BiffStream {
        records: vec![
          record(0, BiffRecordData::Bof(bof(0x0005))),
          record(20, BiffRecordData::BoundSheet8(metadata)),
          record(60, BiffRecordData::Eof),
          record(100, BiffRecordData::Bof(bof(0x0010))),
          record(
            108,
            BiffRecordData::Row(RowRecord {
              row: 2,
              first_column: 3,
              last_column_exclusive: 5,
              height: 300,
              reserved1: 0,
              reserved2: 0,
              flags: 0,
            }),
          ),
          record(
            112,
            BiffRecordData::ColInfo(ColInfoRecord {
              first_column: 3,
              last_column: 7,
              width: 2048,
              format_index: 0,
              flags: 0,
              reserved: ColInfoReserved::Word(0),
            }),
          ),
          record(120, BiffRecordData::MulRk(mul_rk)),
          record(140, BiffRecordData::MulBlank(mul_blank)),
          record(160, BiffRecordData::MergeCells(merge.clone())),
          record(170, BiffRecordData::Bof(bof(0x0020))),
          record(
            174,
            BiffRecordData::Number(NumberRecord {
              cell: CellHeader {
                row: 2,
                column: 3,
                format_index: 0,
              },
              value_bits: 99.0f64.to_bits(),
            }),
          ),
          record(178, BiffRecordData::Eof),
          record(180, BiffRecordData::Eof),
        ],
        trailing_padding: Vec::new(),
      })
      .unwrap(),
    )
    .unwrap();
    let view = workbook.relationships_compatible().unwrap();
    let sheet = view.sheets()[0];
    assert_eq!(sheet.substream().children.len(), 1);
    assert!(sheet.records().len() > sheet.direct_records().count());
    let cells = sheet.cells().collect::<Result<Vec<_>>>().unwrap();
    assert_eq!(
      cells.iter().map(|cell| cell.cell()).collect::<Vec<_>>(),
      vec![
        CellHeader {
          row: 2,
          column: 3,
          format_index: 7,
        },
        CellHeader {
          row: 2,
          column: 4,
          format_index: 8,
        },
        CellHeader {
          row: 5,
          column: 6,
          format_index: 9,
        },
        CellHeader {
          row: 5,
          column: 7,
          format_index: 10,
        },
      ]
    );
    assert!(matches!(
      cells[1].value(),
      XlsCellValueRef::MulRk { index: 1, .. }
    ));
    assert!(matches!(
      cells[3].value(),
      XlsCellValueRef::MulBlank { index: 1, .. }
    ));
    assert_eq!(
      sheet.merged_cells().collect::<Vec<_>>(),
      vec![&merge.ranges[0]]
    );
    let index = sheet.sparse_cell_index().unwrap();
    assert_eq!(index.len(), 4);
    assert!(matches!(
      index.cell(2, 4).unwrap().unwrap().value(),
      XlsCellValueRef::MulRk { index: 1, .. }
    ));
    let row2 = index.row(2).unwrap();
    assert_eq!(row2.definition().unwrap().unwrap().height, 300);
    assert_eq!(row2.cells().count(), 2);
    let row5 = index.row(5).unwrap();
    assert_eq!(row5.definition().unwrap(), None);
    assert_eq!(row5.cells().count(), 2);
    assert_eq!(sheet.column_info(5).unwrap().unwrap().width, 2048);
  }

  #[test]
  fn formula_relationships_resolve_shared_definition_and_cached_string() {
    let exp_tokens = |row, column| FormulaTokens {
      rgce: FormulaTokenStream {
        tokens: vec![FormulaToken {
          opcode: 0x01,
          data: FormulaTokenData::Exp { row, column },
        }],
        unparsed_tail: Vec::new(),
      },
      rgcb_tail: Vec::new(),
    };
    let shared_formula = |row, column| FormulaRecord {
      cell: CellHeader {
        row,
        column,
        format_index: 0,
      },
      cached_result: FormulaCachedResult::NumberBits(0),
      flags: 1 << 3,
      calculation_chain_id: 0,
      // The locator is deliberately not the range's top-left cell.
      tokens: exp_tokens(0, 1),
    };
    let string_formula = FormulaRecord {
      cell: CellHeader {
        row: 2,
        column: 0,
        format_index: 0,
      },
      cached_result: FormulaCachedResult::Special(FormulaSpecialCachedResult {
        kind: 0,
        reserved1: 0,
        value: 0,
        reserved2: [0; 3],
      }),
      flags: 0,
      calculation_chain_id: 0,
      tokens: FormulaTokens {
        rgce: FormulaTokenStream {
          tokens: vec![FormulaToken {
            opcode: 0x1e,
            data: FormulaTokenData::Integer(1),
          }],
          unparsed_tail: Vec::new(),
        },
        rgcb_tail: Vec::new(),
      },
    };
    let shared = SharedFormulaRecord {
      first_row: 0,
      last_row: 1,
      first_column: 0,
      last_column: 1,
      reserved: 0,
      use_count: 2,
      tokens: FormulaTokens {
        rgce: FormulaTokenStream {
          tokens: vec![FormulaToken {
            opcode: 0x1e,
            data: FormulaTokenData::Integer(42),
          }],
          unparsed_tail: Vec::new(),
        },
        rgcb_tail: Vec::new(),
      },
    };
    let cached_string = StringValueRecord {
      declared_character_count: 1,
      chunks: vec![super::super::ContinuedStringChunk {
        flags: 0,
        characters: XlStringCharacters::Compressed(b"x".to_vec()),
        trailing: Vec::new(),
      }],
    };
    let metadata = BoundSheet8Record {
      sheet_bof_offset: 100,
      state: 0,
      sheet_type: 0,
      name: ShortXlUnicodeString {
        flags: 0,
        value: "Formula".to_owned(),
      },
    };
    let workbook = XlsWorkbookStream::from_tree(
      XlsStreamName::Workbook,
      BiffWorkbookTree::from_stream(BiffStream {
        records: vec![
          record(0, BiffRecordData::Bof(bof(0x0005))),
          record(20, BiffRecordData::BoundSheet8(metadata)),
          record(60, BiffRecordData::Eof),
          record(100, BiffRecordData::Bof(bof(0x0010))),
          record(120, BiffRecordData::Formula(shared_formula(0, 1))),
          record(160, BiffRecordData::SharedFormula(shared.clone())),
          record(200, BiffRecordData::Formula(shared_formula(1, 0))),
          record(240, BiffRecordData::Formula(string_formula)),
          record(280, BiffRecordData::StringValue(cached_string.clone())),
          record(320, BiffRecordData::Eof),
        ],
        trailing_padding: Vec::new(),
      })
      .unwrap(),
    )
    .unwrap();
    let view = workbook.relationships_compatible().unwrap();
    let sheet = view.sheets()[0];
    let index = sheet.sparse_cell_index().unwrap();
    let cells = index.rows().flat_map(|row| row.cells()).collect::<Vec<_>>();
    let first = index.resolve_cell_formula(cells[0]).unwrap().unwrap();
    assert!(matches!(
        first.definition(),
        XlsFormulaDefinitionRef::Shared(value) if value == &shared
    ));
    let second = index.resolve_cell_formula(cells[1]).unwrap().unwrap();
    assert!(matches!(
        second.definition(),
        XlsFormulaDefinitionRef::Shared(value) if value == &shared
    ));
    let string = index.resolve_cell_formula(cells[2]).unwrap().unwrap();
    assert_eq!(string.cached_string(), Some(&cached_string));
    assert!(matches!(
      string.definition(),
      XlsFormulaDefinitionRef::Inline(_)
    ));
  }

  #[test]
  fn workbook_relationships_reject_dangling_or_duplicate_sheet_links() {
    let bound_sheet = |sheet_bof_offset| BoundSheet8Record {
      sheet_bof_offset,
      state: 0,
      sheet_type: 0,
      name: ShortXlUnicodeString {
        flags: 0,
        value: "Sheet".to_owned(),
      },
    };
    let workbook = |second_offset| {
      XlsWorkbookStream::from_tree(
        XlsStreamName::Workbook,
        BiffWorkbookTree::from_stream(BiffStream {
          records: vec![
            record(0, BiffRecordData::Bof(bof(0x0005))),
            record(20, BiffRecordData::BoundSheet8(bound_sheet(100))),
            record(40, BiffRecordData::BoundSheet8(bound_sheet(second_offset))),
            record(60, BiffRecordData::Eof),
            record(100, BiffRecordData::Bof(bof(0x0010))),
            record(120, BiffRecordData::Eof),
          ],
          trailing_padding: Vec::new(),
        })
        .unwrap(),
      )
      .unwrap()
    };

    assert!(workbook(999).relationships().is_err());
    assert!(workbook(100).relationships().is_err());

    let dangling_workbook = workbook(999);
    let dangling = dangling_workbook.relationships_compatible().unwrap();
    assert_eq!(dangling.sheets().len(), 1);
    assert_eq!(dangling.unresolved_sheets().len(), 1);
    assert_eq!(dangling.unresolved_sheets()[0].id().value(), 2);
    assert_eq!(
      dangling.unresolved_sheets()[0].error(),
      XlsSheetLinkError::Missing {
        sheet_bof_offset: 999
      }
    );

    let duplicate_workbook = workbook(100);
    let duplicate = duplicate_workbook.relationships_compatible().unwrap();
    assert_eq!(duplicate.sheets().len(), 1);
    assert_eq!(duplicate.unresolved_sheets().len(), 1);
    assert_eq!(
      duplicate.unresolved_sheets()[0].error(),
      XlsSheetLinkError::Duplicate {
        sheet_bof_offset: 100
      }
    );
  }

  #[test]
  fn bound_sheet_reorder_permutates_record_slots_without_record_clones() {
    let bound_sheet = |offset, name: &str| {
      record(
        offset,
        BiffRecordData::BoundSheet8(BoundSheet8Record {
          sheet_bof_offset: 100,
          state: 0,
          sheet_type: 0,
          name: ShortXlUnicodeString::new(name),
        }),
      )
    };
    let mut workbook = XlsWorkbookStream::from_tree(
      XlsStreamName::Workbook,
      BiffWorkbookTree::from_stream(BiffStream {
        records: vec![
          record(0, BiffRecordData::Bof(bof(0x0005))),
          bound_sheet(20, "A"),
          bound_sheet(40, "B"),
          bound_sheet(60, "C"),
          record(80, BiffRecordData::Eof),
        ],
        trailing_padding: Vec::new(),
      })
      .unwrap(),
    )
    .unwrap();
    let order = [
      workbook.sheet_ids[2],
      workbook.sheet_ids[0],
      workbook.sheet_ids[1],
    ];

    reorder_bound_sheet_records(&mut workbook, &order).unwrap();

    let names = workbook
      .tree
      .stream
      .records
      .iter()
      .filter_map(|record| match &record.data {
        BiffRecordData::BoundSheet8(value) => Some(value.name.value.as_str()),
        _ => None,
      })
      .collect::<Vec<_>>();
    assert_eq!(names, ["C", "A", "B"]);
    assert_eq!(workbook.sheet_ids, order);
  }

  #[test]
  fn invalid_revision_stream_is_preserved_only_in_compatible_mode() {
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound
      .create_or_replace_stream(WORKBOOK_STREAM_PATH, workbook_bytes())
      .unwrap();
    compound
      .create_or_replace_stream(super::super::REVISION_LOG_STREAM_PATH, vec![1, 2, 3])
      .unwrap();

    assert!(XlsFile::from_compound_file(compound.clone()).is_err());
    let outcome = XlsFile::from_compound_file_compatible(compound).unwrap();
    assert!(matches!(
        outcome.value.revision_log.as_deref(),
        Some(XlsRevisionLog::Compatibility { bytes, .. }) if bytes == &[1, 2, 3]
    ));
    assert_eq!(outcome.diagnostics.len(), 1);
    assert_eq!(
      outcome.diagnostics[0].code,
      ParseDiagnosticCode::InvalidStreamPreserved
    );
    assert_eq!(
      outcome.diagnostics[0].location.path.as_deref(),
      Some(super::super::REVISION_LOG_STREAM_PATH)
    );
    assert!(outcome.value.to_compound_file().is_err());
    assert!(
      outcome
        .value
        .to_bytes_with_options(SaveOptions::default())
        .is_err()
    );
    let preserved_bytes = outcome
      .value
      .to_bytes_with_options(SaveOptions::preserving_compatibility())
      .unwrap();
    let preserved_bytes = CompoundFile::from_bytes(&preserved_bytes).unwrap();
    assert_eq!(
      preserved_bytes.stream(super::super::REVISION_LOG_STREAM_PATH),
      Some([1, 2, 3].as_slice())
    );
    let preserved = outcome
      .value
      .to_compound_file_preserving_compatibility()
      .unwrap();
    assert_eq!(
      outcome
        .value
        .to_bytes_with_options(SaveOptions::preserving_compatibility())
        .unwrap(),
      preserved.to_bytes().unwrap()
    );
    assert_eq!(
      preserved.stream(super::super::REVISION_LOG_STREAM_PATH),
      Some([1, 2, 3].as_slice())
    );
  }

  #[test]
  fn workbook_stream_lookup_uses_cfb_case_insensitive_names() {
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound
      .create_or_replace_stream("/WORKBOOK", workbook_bytes())
      .unwrap();

    let file = XlsFile::from_compound_file(compound).unwrap();
    assert_eq!(file.workbooks.len(), 1);
    assert_eq!(file.workbooks[0].name, XlsStreamName::Workbook);
    let rebuilt = file.to_compound_file().unwrap();
    assert_eq!(rebuilt.entry("/Workbook").unwrap().name, "WORKBOOK");
    let direct = file.to_bytes().unwrap();
    assert_eq!(direct, rebuilt.to_bytes().unwrap());
    let mut streamed = Vec::new();
    file.write_to(&mut streamed).unwrap();
    assert_eq!(streamed, direct);
  }

  #[test]
  fn storages_and_streams_use_ms_xls_roles_without_copying_entries() {
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound
      .create_or_replace_stream(WORKBOOK_STREAM_PATH, workbook_bytes())
      .unwrap();
    compound.create_storage("/_SX_DB_CUR").unwrap();
    compound
      .create_or_replace_stream("/_SX_DB_CUR/000A", vec![1, 2])
      .unwrap();
    compound.create_storage("/MBD0000002A").unwrap();
    compound
      .create_or_replace_stream("/\u{5}SummaryInformation", vec![3, 4])
      .unwrap();

    let file = XlsFile::from_compound_file_compatible(compound)
      .unwrap()
      .value;
    let inventory = file.storages_and_streams().unwrap();
    assert!(inventory.issues().is_empty());
    let workbook = inventory
      .by_role(XlsFileEntryRole::WorkbookStream(XlsStreamName::Workbook))
      .next()
      .unwrap();
    assert!(std::ptr::eq(
      workbook.entry(),
      file.source_compound_file().entry("/Workbook").unwrap()
    ));
    assert_eq!(
      inventory.embedding_storages().next().unwrap().role(),
      XlsFileEntryRole::EmbeddingStorage { object_id: 0x2a }
    );
    assert_eq!(
      inventory.pivot_cache_streams().next().unwrap().role(),
      XlsFileEntryRole::PivotCacheStream { cache_id: 0x000a }
    );
    assert_eq!(
      inventory
        .by_role(XlsFileEntryRole::SummaryInformationStream)
        .count(),
      1
    );
  }

  #[test]
  fn storages_and_streams_separate_strict_validation_from_compatibility() {
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound
      .create_or_replace_stream(WORKBOOK_STREAM_PATH, workbook_bytes())
      .unwrap();
    compound.create_storage("/Ctls").unwrap();
    compound.create_storage("/_SX_DB_CUR").unwrap();
    compound
      .create_or_replace_stream("/_SX_DB_CUR/not-a-cache", vec![1])
      .unwrap();

    let file = XlsFile::from_compound_file(compound).unwrap();
    assert!(file.storages_and_streams().is_err());
    let inventory = file.storages_and_streams_compatible();
    assert!(matches!(
      inventory.issues(),
      [
        XlsFileEntryIssue::ExpectedStream { .. },
        XlsFileEntryIssue::InvalidPivotCacheChild { .. }
      ]
    ));
    assert_eq!(
      inventory.by_role(XlsFileEntryRole::ControlStream).count(),
      1
    );
  }

  #[test]
  fn workbook_name_and_substream_cardinality_use_compatible_diagnostics() {
    let mut legacy_name = CompoundFile::new(Version::V3).unwrap();
    legacy_name
      .create_or_replace_stream(BOOK_STREAM_PATH, workbook_bytes())
      .unwrap();
    assert!(XlsFile::from_compound_file(legacy_name.clone()).is_err());
    let outcome = XlsFile::from_compound_file_compatible(legacy_name).unwrap();
    assert_eq!(outcome.diagnostics.len(), 1);
    assert_eq!(outcome.diagnostics[0].structure, "Workbook Stream");
    assert_eq!(outcome.diagnostics[0].specification.section, "2.1.7.20");
    assert!(outcome.value.to_compound_file().is_err());
    assert!(
      outcome
        .value
        .to_compound_file_preserving_compatibility()
        .is_ok()
    );

    let sheet_only = BiffStream {
      records: vec![
        record(0, BiffRecordData::Bof(bof(0x0010))),
        record(20, BiffRecordData::Eof),
      ],
      trailing_padding: Vec::new(),
    }
    .to_bytes()
    .unwrap();
    let mut invalid_topology = CompoundFile::new(Version::V3).unwrap();
    invalid_topology
      .create_or_replace_stream(WORKBOOK_STREAM_PATH, sheet_only)
      .unwrap();
    assert!(XlsFile::from_compound_file(invalid_topology.clone()).is_err());
    let outcome = XlsFile::from_compound_file_compatible(invalid_topology).unwrap();
    assert_eq!(outcome.diagnostics.len(), 2);
    assert_eq!(outcome.diagnostics[0].structure, "Globals Substream");
    assert_eq!(outcome.diagnostics[0].specification.section, "2.1.7.20.3");
    assert_eq!(outcome.diagnostics[1].structure, "Workbook Stream");
  }

  #[test]
  fn bof_must_fields_use_the_root_strictness_gate() {
    let mut workbook = XlsWorkbookStream::from_tree(
      XlsStreamName::Workbook,
      BiffWorkbookTree::from_bytes(&workbook_bytes()).unwrap(),
    )
    .unwrap();
    let BiffRecordData::Bof(value) = &mut Arc::make_mut(&mut workbook.tree).stream.records[0].data
    else {
      panic!("first record is not BOF");
    };
    value.build_year = 0;
    value.history_flags = 0;
    value.lowest_version = 0;

    assert!(audit_workbook(&workbook, true, &mut Vec::new()).is_err());
    let mut diagnostics = Vec::new();
    audit_workbook(&workbook, false, &mut diagnostics).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].structure, "BOF");
    assert_eq!(diagnostics[0].specification.section, "2.4.21");
  }

  #[test]
  fn worksheet_must_fields_use_the_root_strictness_gate() {
    let mut tree = BiffWorkbookTree::from_bytes(&workbook_bytes()).unwrap();
    tree.stream.records.insert(
      3,
      record(
        44,
        BiffRecordData::Guts(GutsRecord {
          unused1: 0,
          unused2: 0,
          maximum_row_outline_level: 1,
          maximum_column_outline_level: 9,
        }),
      ),
    );
    tree.stream.records.insert(
      4,
      record(
        56,
        BiffRecordData::PhoneticInfo(PhoneticInfoRecord {
          font_index: 4,
          flags: PhoneticFlags::empty(),
          range_count: 0,
          ranges: Vec::new(),
        }),
      ),
    );
    tree.stream.records.insert(
      5,
      record(
        64,
        BiffRecordData::FixedU16 {
          kind: FixedU16RecordKind::AutoFilterInfo,
          value: 0,
        },
      ),
    );
    tree.relayout().unwrap();
    let workbook = XlsWorkbookStream::from_tree(XlsStreamName::Workbook, tree).unwrap();

    assert!(audit_workbook(&workbook, true, &mut Vec::new()).is_err());
    let mut diagnostics = Vec::new();
    audit_workbook(&workbook, false, &mut diagnostics).unwrap();
    let guts = diagnostics
      .iter()
      .find(|diagnostic| diagnostic.structure == "Guts")
      .expect("Guts diagnostic");
    assert_eq!(guts.specification.section, "2.4.134");
    let phonetic = diagnostics
      .iter()
      .find(|diagnostic| diagnostic.structure == "PhoneticInfo")
      .expect("PhoneticInfo diagnostic");
    assert_eq!(phonetic.specification.section, "2.4.192");
    let auto_filter = diagnostics
      .iter()
      .find(|diagnostic| diagnostic.structure == "AutoFilterInfo")
      .expect("AutoFilterInfo diagnostic");
    assert_eq!(auto_filter.specification.section, "2.4.8");
  }

  #[test]
  fn workbook_clone_shares_record_tree_until_explicit_mutation() {
    let workbook = XlsWorkbookStream::from_tree(
      XlsStreamName::Workbook,
      BiffWorkbookTree::from_bytes(&workbook_bytes()).unwrap(),
    )
    .unwrap();
    let mut cloned = workbook.clone();
    assert!(Arc::ptr_eq(&workbook.tree, &cloned.tree));

    Arc::make_mut(&mut cloned.tree)
      .stream
      .trailing_padding
      .push(0xff);
    assert!(workbook.tree.stream.trailing_padding.is_empty());
    assert_eq!(cloned.tree.stream.trailing_padding, [0xff]);
    assert!(!Arc::ptr_eq(&workbook.tree, &cloned.tree));
  }

  #[test]
  fn end_object_kind_uses_the_root_strictness_gate() {
    let workbook = XlsWorkbookStream::from_tree(
      XlsStreamName::Workbook,
      BiffWorkbookTree::from_stream(BiffStream {
        records: vec![
          record(0, BiffRecordData::Bof(bof(0x0005))),
          record(20, BiffRecordData::Eof),
          record(24, BiffRecordData::Bof(bof(0x0020))),
          record(
            44,
            BiffRecordData::ChartEndObject(ChartEndObjectRecord {
              header: FrtHeaderOld {
                record_type: 0x0855,
                flags: FrtFlags::empty(),
              },
              object_kind: 0x0013,
              unused1: None,
              unused2: None,
              unused3: None,
            }),
          ),
          record(54, BiffRecordData::Eof),
        ],
        trailing_padding: Vec::new(),
      })
      .unwrap(),
    )
    .unwrap();

    assert!(audit_workbook(&workbook, true, &mut Vec::new()).is_err());
    let mut diagnostics = Vec::new();
    audit_workbook(&workbook, false, &mut diagnostics).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].structure, "EndObject");
    assert_eq!(diagnostics[0].specification.section, "2.4.101");
  }

  #[test]
  fn ext_sst_bucket_must_fields_use_the_root_strictness_gate() {
    let workbook = XlsWorkbookStream::from_tree(
      XlsStreamName::Workbook,
      BiffWorkbookTree::from_stream(BiffStream {
        records: vec![
          record(0, BiffRecordData::Bof(bof(0x0005))),
          record(
            20,
            BiffRecordData::ExtSst(ExtSstRecord {
              strings_per_bucket: 8,
              buckets: vec![IsstInf {
                stream_offset: 4,
                record_offset: 4,
                reserved: 1,
              }],
            }),
          ),
          record(34, BiffRecordData::Eof),
        ],
        trailing_padding: Vec::new(),
      })
      .unwrap(),
    )
    .unwrap();

    assert!(audit_workbook(&workbook, true, &mut Vec::new()).is_err());
    let mut diagnostics = Vec::new();
    audit_workbook(&workbook, false, &mut diagnostics).unwrap();
    let issue = diagnostics
      .iter()
      .find(|diagnostic| diagnostic.structure == "ISSTInf")
      .expect("ISSTInf diagnostic");
    assert_eq!(issue.specification.section, "2.5.167");
  }

  #[test]
  fn spec_truncated_devmode_public_fields_are_not_a_damage_diagnostic() {
    let devmode = DevModeW {
      device_name: [0; 32],
      specification_version: 0x0401,
      driver_version: 0,
      declared_public_size: 76,
      declared_driver_extra_size: 0,
      fields: DevModeFields::empty(),
      public_fields: DevModeWPublic::Truncated(Vec::new()),
      driver_extra: Vec::new(),
      driver_extra_complete: true,
      trailing: Vec::new(),
    };
    let workbook = |devmode| {
      XlsWorkbookStream::from_tree(
        XlsStreamName::Workbook,
        BiffWorkbookTree::from_stream(BiffStream {
          records: vec![
            record(0, BiffRecordData::Bof(bof(0x0005))),
            record(
              20,
              BiffRecordData::Pls(PlsRecord {
                reserved: 0,
                settings: PrinterSettings::WindowsUnicode(devmode),
                physical_segment_lengths: vec![78],
              }),
            ),
            record(102, BiffRecordData::Eof),
            record(106, BiffRecordData::Bof(bof(0x0010))),
            record(126, BiffRecordData::Eof),
          ],
          trailing_padding: Vec::new(),
        })
        .unwrap(),
      )
      .unwrap()
    };

    let valid = workbook(devmode.clone());
    let mut diagnostics = Vec::new();
    audit_workbook(&valid, true, &mut diagnostics).unwrap();
    assert!(diagnostics.is_empty());

    let mut incomplete = devmode;
    incomplete.declared_driver_extra_size = 4;
    incomplete.driver_extra_complete = false;
    let invalid = workbook(incomplete);
    assert!(audit_workbook(&invalid, true, &mut Vec::new()).is_err());
    let mut diagnostics = Vec::new();
    audit_workbook(&invalid, false, &mut diagnostics).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, ParseDiagnosticCode::TruncatedRecord);
  }

  #[test]
  fn missing_formula_rgb_extra_requires_explicit_compatibility() {
    let formula = FormulaRecord {
      cell: CellHeader {
        row: 0,
        column: 0,
        format_index: 0,
      },
      cached_result: FormulaCachedResult::NumberBits(0),
      flags: 0,
      calculation_chain_id: 0,
      tokens: FormulaTokens {
        rgce: FormulaTokenStream::from_bytes(&[0x60, 0, 0, 0, 0, 0, 0, 0]).unwrap(),
        rgcb_tail: Vec::new(),
      },
    };
    let bytes = BiffStream {
      records: vec![
        record(0, BiffRecordData::Bof(bof(0x0005))),
        record(20, BiffRecordData::Formula(formula)),
        record(54, BiffRecordData::Eof),
        record(58, BiffRecordData::Bof(bof(0x0010))),
        record(78, BiffRecordData::Eof),
      ],
      trailing_padding: Vec::new(),
    }
    .to_bytes()
    .unwrap();
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound
      .create_or_replace_stream(WORKBOOK_STREAM_PATH, bytes)
      .unwrap();

    assert!(XlsFile::from_compound_file(compound.clone()).is_err());
    let outcome = XlsFile::from_compound_file_compatible(compound).unwrap();
    assert_eq!(outcome.diagnostics.len(), 1);
    assert_eq!(
      outcome.diagnostics[0].code,
      ParseDiagnosticCode::TruncatedRecord
    );
    assert_eq!(outcome.diagnostics[0].specification.section, "2.4.127");
    assert!(outcome.value.to_compound_file().is_err());
    assert!(
      outcome
        .value
        .to_compound_file_preserving_compatibility()
        .is_ok()
    );
  }

  #[test]
  fn unknown_formula_ptg_tail_requires_explicit_compatibility() {
    let formula = FormulaRecord {
      cell: CellHeader {
        row: 0,
        column: 0,
        format_index: 0,
      },
      cached_result: FormulaCachedResult::NumberBits(0),
      flags: 0,
      calculation_chain_id: 0,
      tokens: FormulaTokens {
        rgce: FormulaTokenStream::from_bytes(&[0x1e, 7, 0, 0x18, 0x04, 0]).unwrap(),
        rgcb_tail: Vec::new(),
      },
    };
    let bytes = BiffStream {
      records: vec![
        record(0, BiffRecordData::Bof(bof(0x0005))),
        record(20, BiffRecordData::Formula(formula)),
        record(54, BiffRecordData::Eof),
        record(58, BiffRecordData::Bof(bof(0x0010))),
        record(78, BiffRecordData::Eof),
      ],
      trailing_padding: Vec::new(),
    }
    .to_bytes()
    .unwrap();
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound
      .create_or_replace_stream(WORKBOOK_STREAM_PATH, bytes)
      .unwrap();

    assert!(XlsFile::from_compound_file(compound.clone()).is_err());
    let outcome = XlsFile::from_compound_file_compatible(compound).unwrap();
    assert_eq!(outcome.diagnostics.len(), 1);
    assert_eq!(
      outcome.diagnostics[0].code,
      ParseDiagnosticCode::NonconformingRecord
    );
    assert!(
      outcome.diagnostics[0]
        .message
        .contains("unknown or reserved Ptg")
    );
    assert!(outcome.value.to_compound_file().is_err());
    assert!(
      outcome
        .value
        .to_compound_file_preserving_compatibility()
        .is_ok()
    );
  }
}
