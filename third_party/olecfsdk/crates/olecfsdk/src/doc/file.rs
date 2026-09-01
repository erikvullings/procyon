//! Typed file and primary content-tree roots for the Word binary format.
//!
//! [`DocFile`] owns the parsed MS-DOC tree and keeps its source CFB private as
//! an immutable preservation snapshot. Typed edits do not mutate that snapshot;
//! serialization rebuilds `WordDocument`, the selected Table stream, `Data`,
//! and `ObjectPool` state from the current tree, then carries unrelated entries
//! forward. Strict entry points and saves are the default. Compatible parsing
//! returns structured diagnostics, and compatibility nodes require an explicit
//! compatibility-preserving save policy.

use std::{
  collections::{BTreeMap, BTreeSet},
  fs,
  io::Write,
  ops::Range,
  path::{Path, PathBuf},
  sync::Arc,
};

use crate::{
  Error, Result,
  cfb::{CfbStreamData, CfbStreamOverride, CfbStreamWriter, CompoundFile},
  forms::ParentControlStorageModel,
  io::BinaryFormat,
  limits::Limits,
  office_art::{
    OfficeArtArrayData, OfficeArtPoint16, OfficeArtPoint32, OfficeArtProperty,
    OfficeArtPropertyValue, OfficeArtRecord, OfficeArtRecordData, OfficeArtShape,
    OfficeArtWordClientTextbox, image_ref_from_record_bytes,
  },
  parse::{
    ParseDiagnostic, ParseDiagnosticCode, ParseOptions, ParseOutcome, SpecificationReference,
    compound_from_bytes, compound_from_path, compound_from_vec, compound_outcome,
  },
  save::SaveOptions,
  shared::MsoEnvelope,
  shared_content::{
    OfficeFormsMutation, OfficeHostKind, OfficeSharedContent, OfficeVbaModuleMutation,
  },
};

use super::{
  AnnotationBookmarkInfo, AnnotationBookmarks, AnnotationExtendedData, AnnotationOwners,
  AnnotationPost10, AnnotationReference, AnnotationReferenceTable, AssociatedStrings,
  AutoCaptionDefinitions, AutoSummaryRangeTable, BookmarkStart, Bookmarks, CaptionDefinitions,
  ChpxFkp, ChpxFkpRun, Clx, CommandCustomizations, CpOnlyTable, DATA_STREAM_PATH,
  DocOfficeArtContent, DocOfficeArtImageLink, DocumentProperties, EmbeddedFontTable,
  ExternalFileNameTable, Fib, FibBaseFlags, FibFcLcb, FieldDocumentPart, FieldTable, FkpPageNumber,
  FontTable, FormatConsistencyBookmarks, FrameAndListRecords, GrammarCheckerCookieTable,
  GrammarCookieStore, GrammarOptionSets, GrammarStateTable, GrpPrl, HeaderTextTable, KnownSprm,
  LanguageDetectionStateTable, LegacyGrammarCheckerCookieTable, LegacyGrammarOptionSets,
  ListDefinitions, ListNamesTable, ListOverrides, ListStyleTemplates, MailMergeState,
  NilPicfAndBinData, NilPicfFieldType, NoteReferenceTable, OBJECT_INFO_STREAM_NAME,
  OBJECT_POOL_STORAGE_PATH, OfficeDataSource, OleControlInfos, OleObjectDescriptor, PapxFkp,
  PapxFkpRun, ParagraphGroupProperties, Pcd, PicfAndOfficeArtData, PlcBte, PlcfSed, PrcData,
  PrinterDriverInfo, PrivateFieldType, Prm, PrmPropertiesRef, RangeProtection, RepairBookmarks,
  RevisionAuthors, RevisionMessageThreading, RevisionSaveIdTable, SaveHistory, SelectionState,
  Sepx, ShapeAnchor, ShapeAnchorTable, SmartTagBookmarks, SmartTagData,
  SmartTagRecognizerStateTable, SpellingStateTable, SprmGroup, SprmKind, SprmOperand,
  StructuredTagBookmarks, StructuredTagType, StyleFormatting, StyleSheet, SubdocumentTable,
  TABLE0_STREAM_PATH, TABLE1_STREAM_PATH, TableCharacterCacheTable, TextPiece, TextPieceCharacters,
  TextPieceEncoding, TextboxBreak, TextboxBreakTable, TextboxDocumentPart, TextboxStory,
  TextboxStoryChain, TextboxStoryTable, UserInputMethods, UserVariables, WORD_DOCUMENT_STREAM_PATH,
  XmlSchemaReferences, XmlTransformPath,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocTableStreamName {
  Table0,
  Table1,
}

impl DocTableStreamName {
  pub const fn path(self) -> &'static str {
    match self {
      Self::Table0 => TABLE0_STREAM_PATH,
      Self::Table1 => TABLE1_STREAM_PATH,
    }
  }
}

/// A typed value together with the FIB location that owns its physical bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocLocated<T> {
  pub location: FibFcLcb,
  pub value: T,
}

/// A typed 512-byte formatting page referenced by a PlcBte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocFkpPage<T> {
  pub page: FkpPageNumber,
  pub value: T,
}

/// A text piece retains CP/FC boundaries and its physical character encoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocTextPiece {
  pub piece_index: usize,
  pub value: TextPiece,
}

/// A non-negative MS-DOC character position. A CP counts characters in the
/// aggregate document or in one explicitly identified document part; it is
/// never interchangeable with a byte position in WordDocument.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocCp(u32);

/// A byte position in the WordDocument stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocFc(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocCpRange {
  pub start: DocCp,
  pub end: DocCp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocFcRange {
  pub start: DocFc,
  pub end: DocFc,
}

/// Borrowed root of the MS-DOC logical content relationship graph.
#[derive(Debug)]
pub struct DocContentTree<'a> {
  file: &'a DocFile,
  parts: [DocDocumentPartRef<'a>; 7],
  preserve_compatibility: bool,
}

/// A borrowed selection in one MS-DOC document part. Related text pieces and
/// formatting nodes remain owned by the file and can overlap the selection at
/// either boundary.
#[derive(Clone, Copy, Debug)]
pub struct DocTextRangeRef<'a> {
  document_part: DocDocumentPartRef<'a>,
  local_cp_range: DocCpRange,
  global_cp_range: DocCpRange,
}

/// A non-fatal relationship defect retained by a compatible content tree.
/// Strict content trees return the same condition as an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocRelationshipDiagnostic {
  pub index: Option<usize>,
  pub reason: String,
}

#[derive(Clone, Copy, Debug)]
pub struct DocBookmarkRef<'a> {
  index: usize,
  name: &'a [u16],
  properties: &'a BookmarkStart,
  text: DocTextRangeRef<'a>,
}

#[derive(Clone, Debug)]
pub struct DocBookmarks<'a> {
  bookmarks: Vec<DocBookmarkRef<'a>>,
  diagnostics: Vec<DocRelationshipDiagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocNoteKind {
  Footnote,
  Endnote,
}

#[derive(Clone, Copy, Debug)]
pub struct DocNoteRef<'a> {
  kind: DocNoteKind,
  index: usize,
  reference_document: DocDocumentPartRef<'a>,
  reference_cp: DocCp,
  numbering_index: &'a u16,
  text: DocTextRangeRef<'a>,
}

#[derive(Clone, Debug)]
pub struct DocNotes<'a> {
  kind: DocNoteKind,
  notes: Vec<DocNoteRef<'a>>,
  diagnostics: Vec<DocRelationshipDiagnostic>,
}

#[derive(Clone, Copy, Debug)]
pub struct DocAnnotationBookmarkRef<'a> {
  index: usize,
  info: &'a AnnotationBookmarkInfo,
  properties: &'a BookmarkStart,
  text: DocTextRangeRef<'a>,
}

#[derive(Clone, Copy, Debug)]
pub struct DocCommentRef<'a> {
  file: &'a DocFile,
  preserve_compatibility: bool,
  index: usize,
  reference_document: DocDocumentPartRef<'a>,
  reference_cp: DocCp,
  annotation: &'a AnnotationReference,
  author: Option<&'a [u16]>,
  extended: Option<&'a AnnotationPost10>,
  annotation_bookmark: Option<DocAnnotationBookmarkRef<'a>>,
  text: DocTextRangeRef<'a>,
}

#[derive(Clone, Debug)]
pub struct DocComments<'a> {
  comments: Vec<DocCommentRef<'a>>,
  diagnostics: Vec<DocRelationshipDiagnostic>,
}

#[derive(Clone, Copy, Debug)]
pub struct DocOfficeArtShapeRef<'a> {
  document_part: TextboxDocumentPart,
  z_order: usize,
  container: &'a OfficeArtRecord,
  properties: &'a [OfficeArtRecord],
  shape_type: u16,
  shape: &'a OfficeArtShape,
  text_id_property: Option<&'a OfficeArtProperty>,
  next_shape_id_property: Option<&'a OfficeArtProperty>,
  client_textbox: Option<&'a OfficeArtWordClientTextbox>,
}

/// Effective OfficeArt text insets in English Metric Units (EMUs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocOfficeArtTextInsets {
  left: i32,
  top: i32,
  right: i32,
  bottom: i32,
}

/// Effective OfficeArt distances between a floating shape and surrounding text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocOfficeArtWrapDistances {
  left: i32,
  top: i32,
  right: i32,
  bottom: i32,
}

/// OfficeArt picture crop fractions in signed 16.16 fixed-point units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocOfficeArtPictureCrop {
  top: i32,
  bottom: i32,
  left: i32,
  right: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct DocOfficeArtWrapPolygonRef<'a> {
  points: DocOfficeArtWrapPoints<'a>,
}

#[derive(Clone, Copy, Debug)]
enum DocOfficeArtWrapPoints<'a> {
  I16(&'a [OfficeArtPoint16]),
  I32(&'a [OfficeArtPoint32]),
}

/// A host-independent OfficeArt color. Direct RGB values are decoded without
/// allocation; indexed, scheme, system, and transformed colors retain the
/// original MSO_CLR value for an explicit conversion decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocOfficeArtColor {
  Rgb { red: u8, green: u8, blue: u8 },
  Other(u32),
}

/// Effective OfficeArt fill after applying the Boolean use/value bits and
/// OfficeArt defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocOfficeArtFill {
  None,
  Solid(DocOfficeArtColor),
  Other { fill_type: u32 },
}

/// Effective OfficeArt outline after applying the Boolean use/value bits and
/// OfficeArt defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocOfficeArtLine {
  None,
  Solid {
    color: DocOfficeArtColor,
    width_emu: i32,
  },
  Other,
}

/// The host-defined MS-DOC interpretation of an OfficeArt text identifier.
/// The high word selects a one-based FTXBXS and the low word selects a
/// zero-based shape within that textbox chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocTextboxShapeLink {
  text_id: u32,
  story_index: u16,
  chain_index: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct DocTextboxBreakRef<'a> {
  index: usize,
  source: &'a TextboxBreak,
  text: DocTextRangeRef<'a>,
  story_index: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct DocTextboxStoryRef<'a> {
  document_part: TextboxDocumentPart,
  index: usize,
  source: &'a TextboxStory,
  text: DocTextRangeRef<'a>,
  reusable: bool,
  shapes: Vec<DocOfficeArtShapeRef<'a>>,
  breaks: Vec<DocTextboxBreakRef<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct DocShapeAnchorRef<'a> {
  index: usize,
  anchor_document: DocDocumentPartRef<'a>,
  anchor_cp: DocCp,
  source: &'a ShapeAnchor,
  shape: Option<DocOfficeArtShapeRef<'a>>,
}

#[derive(Clone, Debug)]
pub struct DocTextboxes<'a> {
  document_part: TextboxDocumentPart,
  stories: Vec<DocTextboxStoryRef<'a>>,
  breaks: Vec<DocTextboxBreakRef<'a>>,
  anchors: Vec<DocShapeAnchorRef<'a>>,
  shapes: Vec<DocOfficeArtShapeRef<'a>>,
  diagnostics: Vec<DocRelationshipDiagnostic>,
}

/// One of the seven text-bearing MS-DOC document parts described by the FIB
/// `ccp*` fields. `Macro` is intentionally excluded because its Plcfld is not
/// a document-part text range.
#[derive(Clone, Copy, Debug)]
pub struct DocDocumentPartRef<'a> {
  file: &'a DocFile,
  part: FieldDocumentPart,
  global_cp_range: DocCpRange,
  preserve_compatibility: bool,
}

/// The intersection of one physical PlcPcd text piece with one document part.
/// The Pcd descriptor and decoded character units remain borrowed from their
/// single owners in the CLX and WordDocument trees.
#[derive(Clone, Copy, Debug)]
pub struct DocTextPieceRef<'a> {
  document_part: DocDocumentPartRef<'a>,
  source: &'a DocTextPiece,
  descriptor: Option<&'a Pcd>,
  global_cp_range: DocCpRange,
  local_cp_range: DocCpRange,
  fc_range: DocFcRange,
  character_start: usize,
  character_end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocTextPieceValueRef<'a> {
  String {
    value: &'a str,
    encoding: TextPieceEncoding,
  },
  CompatibilityUtf16(&'a [u16]),
}

/// One PAPX FKP interval intersected with a document part.
#[derive(Clone, Copy, Debug)]
pub struct DocParagraphRef<'a> {
  document_part: DocDocumentPartRef<'a>,
  source: &'a DocPapxRun,
  global_cp_range: DocCpRange,
  local_cp_range: DocCpRange,
}

/// The paragraph style selected after applying the PAPX style index and its
/// ordered `sprmPIstd`/`sprmPIstdPermute` direct modifications. The style
/// definition remains borrowed from the one STSH owner.
#[derive(Clone, Copy, Debug)]
pub struct DocParagraphStyleRef<'a> {
  document_part: DocDocumentPartRef<'a>,
  style_index: u16,
  source: &'a super::StyleDefinition,
}

/// Paragraph style identity and normative outline level resolved from one
/// expansion of the paragraph's direct SPRM layer.
#[derive(Clone, Copy, Debug)]
pub struct DocParagraphStyleStateRef<'a> {
  style: DocParagraphStyleRef<'a>,
  outline_level: DocOutlineLevel,
}

/// MS-DOC paragraph outline level. Values zero through eight are the nine
/// outline levels; 0x09 is body text and therefore not an outline paragraph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DocOutlineLevel {
  Level1,
  Level2,
  Level3,
  Level4,
  Level5,
  Level6,
  Level7,
  Level8,
  Level9,
  BodyText,
}

/// One top-level content block in a document part. A completed outer table is
/// emitted once; its member paragraphs remain reachable through the table,
/// row, and cell relationships instead of appearing again beside it.
#[derive(Clone, Debug)]
pub enum DocBlockRef<'a> {
  Paragraph(DocParagraphRef<'a>),
  Table(DocTableRef<'a>),
}

/// Document-order block relationship index for one document part.
#[derive(Clone, Debug)]
pub struct DocBlocks<'a> {
  blocks: Vec<DocBlockRef<'a>>,
  diagnostics: Vec<DocTableDiagnostic>,
}

/// One PlcfSed interval in the Main Document, joined to its SED and optional
/// Sepx owner without copying either structure.
#[derive(Clone, Copy, Debug)]
pub struct DocSectionRef<'a> {
  document_part: DocDocumentPartRef<'a>,
  section_index: usize,
  local_cp_range: DocCpRange,
  global_cp_range: DocCpRange,
  source: &'a super::Sed,
  properties: &'a DocSectionProperties,
}

#[derive(Clone, Debug)]
pub struct DocSections<'a> {
  sections: Vec<DocSectionRef<'a>>,
}

/// The role of a PAPX interval after applying the MS-DOC table-depth and
/// table-marker paragraph properties at its final character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocParagraphKind {
  NonTable,
  TableParagraph { table_depth: u32 },
  CellMark { table_depth: u32 },
  TableTerminatingParagraph { table_depth: u32 },
}

/// A completed MS-DOC table row. The range includes the TTP mark and can also
/// contain nested rows at greater table depths. The physical PAPX owner of the
/// TTP remains available through [`Self::terminating_paragraph`].
#[derive(Clone, Copy, Debug)]
pub struct DocTableRowRef<'a> {
  document_part: DocDocumentPartRef<'a>,
  table_depth: u32,
  global_cp_range: DocCpRange,
  local_cp_range: DocCpRange,
  terminating_paragraph: DocParagraphRef<'a>,
  cell_count: usize,
  defined_cell_count: Option<usize>,
}

/// One MS-DOC table cell, including its cell mark and any nested tables. The
/// cell is a borrowed relationship view; text, PAPX, and nested row nodes stay
/// owned by the document part.
#[derive(Clone, Copy, Debug)]
pub struct DocTableCellRef<'a> {
  row: DocTableRowRef<'a>,
  cell_index: usize,
  global_cp_range: DocCpRange,
  local_cp_range: DocCpRange,
  cell_mark: DocParagraphRef<'a>,
}

/// A compatibility observation produced while deriving rows from damaged or
/// nonconforming paragraph properties. Strict content trees return the first
/// such condition as an error instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocTableDiagnostic {
  pub global_cp_range: DocCpRange,
  pub table_depth: Option<u32>,
  pub reason: String,
}

/// The row relationship index for one document part. Its rows are small
/// borrowed handles; diagnostics are allocated only for compatible input.
#[derive(Clone, Debug)]
pub struct DocTableRows<'a> {
  rows: Vec<DocTableRowRef<'a>>,
  diagnostics: Vec<DocTableDiagnostic>,
}

/// The cell relationship index for one row.
#[derive(Clone, Debug)]
pub struct DocTableCells<'a> {
  cells: Vec<DocTableCellRef<'a>>,
  diagnostics: Vec<DocTableDiagnostic>,
}

/// One table according to the MS-DOC adjacent-row identity rules. Nested
/// tables are separate values at their own table depth.
#[derive(Clone, Debug)]
pub struct DocTableRef<'a> {
  document_part: DocDocumentPartRef<'a>,
  table_depth: u32,
  global_cp_range: DocCpRange,
  local_cp_range: DocCpRange,
  rows: Vec<DocTableRowRef<'a>>,
}

/// The table relationship index for one document part.
#[derive(Clone, Debug)]
pub struct DocTables<'a> {
  document_part: DocDocumentPartRef<'a>,
  tables: Vec<DocTableRef<'a>>,
  diagnostics: Vec<DocTableDiagnostic>,
}

/// One CHPX FKP interval intersected with a document part.
#[derive(Clone, Copy, Debug)]
pub struct DocCharacterRunRef<'a> {
  document_part: DocDocumentPartRef<'a>,
  source: &'a DocChpxRun,
  global_cp_range: DocCpRange,
  local_cp_range: DocCpRange,
}

/// One text slice bounded by both a PlcPcd piece and a CHPX run.
///
/// The slice and formatting owner stay borrowed; the handle only carries the
/// intersected CP/FC bounds needed by a streaming consumer.
#[derive(Clone, Copy, Debug)]
pub struct DocFormattedTextRef<'a> {
  text: DocTextPieceRef<'a>,
  character_run: DocCharacterRunRef<'a>,
}

/// A zero-copy join of one CP to its physical Pcd, PAPX and CHPX owners.
/// Expanding SPRM arrays into an applied formatting value remains an explicit
/// operation because it allocates a derived value rather than another owner.
#[derive(Clone, Copy, Debug)]
pub struct DocDirectFormattingRef<'a> {
  document_part: DocDocumentPartRef<'a>,
  local_cp: DocCp,
  global_cp: DocCp,
  text_piece: DocTextPieceRef<'a>,
  paragraph: &'a DocPapxRun,
  character_run: &'a DocChpxRun,
}

/// One field in the recursive Plcfld production, with both part-local and
/// aggregate CP identity retained.
#[derive(Clone, Copy, Debug)]
pub struct DocFieldRef<'a> {
  document_part: DocDocumentPartRef<'a>,
  source: &'a super::Field,
}

#[derive(Clone, Copy, Debug)]
pub enum DocSpecialContentRef<'a> {
  Picture {
    character: DocCp,
    location_property: &'a super::Prl,
    data_node: &'a DocDataNode,
  },
  Binary {
    character: DocCp,
    location_property: &'a super::Prl,
    data_node: &'a DocDataNode,
  },
  OleObject {
    character: DocCp,
    location_property: &'a super::Prl,
    field: DocFieldRef<'a>,
    object: &'a DocEmbeddedObjectStorage,
  },
}

#[derive(Clone, Debug)]
pub enum DocSpecialContentLink<'a> {
  Resolved(DocSpecialContentRef<'a>),
  CompatibilityOleObject {
    character: DocCp,
    location_property: &'a super::Prl,
    field: DocFieldRef<'a>,
    storage: &'a DocCompatibilityObjectStorage,
  },
  Unresolved {
    character: DocCp,
    reason: String,
  },
}

/// A CHPX FKP text run mapped from its physical FC interval to the document
/// CP coordinate space. Pcd.Prm and style-derived properties remain separate
/// specification layers and are not folded into this value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocChpxRun {
  pub cp_start: u32,
  pub cp_end: u32,
  /// Clone-shared direct formatting; use [`Arc::make_mut`] for field edits.
  pub properties: Option<Arc<GrpPrl>>,
}

/// A PAPX FKP paragraph, table-row, or table-cell run mapped from its physical
/// FC interval to the document CP coordinate space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocPapxRun {
  pub cp_start: u32,
  pub cp_end: u32,
  pub paragraph_height_info: [u8; 12],
  /// Clone-shared paragraph formatting; use [`Arc::make_mut`] for field edits.
  pub properties: Option<Arc<super::PapxInFkp>>,
}

/// The two normative direct-formatting layers at one MS-DOC character
/// position. This is intentionally not an "effective formatting" value:
/// styles, lists, table styles, and conditional table formatting remain
/// separate specification layers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocDirectFormatting {
  pub part: FieldDocumentPart,
  pub local_cp: u32,
  pub global_cp: u32,
  pub piece_index: usize,
  pub paragraph: DocDirectParagraphFormatting,
  pub character: DocDirectCharacterFormatting,
}

/// Direct paragraph formatting in the order and source layers defined by
/// MS-DOC 2.4.6.1. Keeping PAPX and Pcd.Prm properties separate permits an
/// editor to write a change back to its original physical owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocDirectParagraphFormatting {
  pub style_index: u16,
  pub papx_properties: GrpPrl,
  pub piece_properties: GrpPrl,
  /// The normative direct-paragraph property array after recursively
  /// following sprmPHugePapx/sprmPTableProps and applying their stop rules.
  /// The physical source arrays above and the referenced `DocDataNode`s are
  /// retained unchanged for precise write-back.
  pub applied_properties: GrpPrl,
}

/// Direct character formatting in the order and source layers defined by
/// MS-DOC 2.4.6.1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocDirectCharacterFormatting {
  pub chpx_properties: GrpPrl,
  pub piece_properties: GrpPrl,
}

/// The table-membership values produced by applying the direct paragraph
/// property array. `depth_is_explicit` distinguishes the default depth zero
/// from a value written by sprmPItap/sprmPDtap; this matters for diagnosing
/// legacy or nonconforming producers without inventing an implicit depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocDirectTableState {
  pub in_table: bool,
  pub depth: u32,
  pub depth_is_explicit: bool,
  pub table_terminating_paragraph: bool,
  pub inner_table_cell: bool,
  pub inner_table_terminating_paragraph: bool,
}

/// The inherited property arrays for one STSH style. `lineage` is ordered
/// base-first and the three property arrays follow the same order, so later
/// properties from the requested style retain normal SPRM precedence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocStyleProperties {
  pub style_index: u16,
  pub style_kind: super::StyleKind,
  pub lineage: Vec<u16>,
  pub paragraph_properties: GrpPrl,
  pub character_properties: GrpPrl,
  pub table_properties: GrpPrl,
}

impl DocDirectParagraphFormatting {
  /// Applies sprmPFInTable, sprmPItap, and sprmPDtap in specification order.
  pub fn table_state(&self) -> Result<DocDirectTableState> {
    let mut in_table = false;
    let mut depth = 0i32;
    let mut depth_is_explicit = false;
    let mut table_terminating_paragraph = false;
    let mut inner_table_cell = false;
    let mut inner_table_terminating_paragraph = false;
    for property in &self.applied_properties.properties {
      match property.sprm.kind() {
        SprmKind::Known(KnownSprm::PFInTable) => {
          let SprmOperand::Byte(value) = &property.operand else {
            return Err(Error::invalid(0, "sprmPFInTable operand is not Bool8"));
          };
          if *value > 1 {
            return Err(Error::invalid(0, "sprmPFInTable Bool8 operand exceeds one"));
          }
          in_table = *value != 0;
        }
        SprmKind::Known(KnownSprm::PItap) => {
          let SprmOperand::Dword(value) = &property.operand else {
            return Err(Error::invalid(0, "sprmPItap operand is not a signed dword"));
          };
          depth = i32::from_le_bytes(*value);
          depth_is_explicit = true;
          if depth < 0 {
            return Err(Error::invalid(0, "sprmPItap table depth is negative"));
          }
        }
        SprmKind::Known(KnownSprm::PDtap) => {
          let SprmOperand::Dword(value) = &property.operand else {
            return Err(Error::invalid(0, "sprmPDtap operand is not a signed dword"));
          };
          depth = depth
            .checked_add(i32::from_le_bytes(*value))
            .ok_or_else(|| Error::Limit("sprmPDtap table depth overflow".into()))?;
          depth_is_explicit = true;
          if depth < 0 {
            return Err(Error::invalid(
              0,
              "sprmPDtap produces a negative table depth",
            ));
          }
        }
        SprmKind::Known(KnownSprm::PFTtp) => {
          table_terminating_paragraph = table_bool8(&property.operand, "sprmPFTtp")?;
        }
        SprmKind::Known(KnownSprm::PFInnerTableCell) => {
          inner_table_cell = table_bool8(&property.operand, "sprmPFInnerTableCell")?;
        }
        SprmKind::Known(KnownSprm::PFInnerTtp) => {
          inner_table_terminating_paragraph = table_bool8(&property.operand, "sprmPFInnerTtp")?;
        }
        _ => {}
      }
    }
    // For the non-nested Word 97 table form, sprmPFInTable itself
    // establishes depth one. sprmPItap/sprmPDtap are needed to state or
    // adjust an explicit depth, particularly for nested tables. This is
    // also the interpretation used by Word-compatible WW8 readers.
    if in_table && !depth_is_explicit {
      depth = 1;
    }
    Ok(DocDirectTableState {
      in_table,
      depth: u32::try_from(depth)
        .map_err(|_| Error::invalid(0, "direct table depth is negative"))?,
      depth_is_explicit,
      table_terminating_paragraph,
      inner_table_cell,
      inner_table_terminating_paragraph,
    })
  }
}

fn table_bool8(operand: &SprmOperand, name: &str) -> Result<bool> {
  let SprmOperand::Byte(value) = operand else {
    return Err(Error::invalid(0, format!("{name} operand is not Bool8")));
  };
  if *value > 1 {
    return Err(Error::invalid(
      0,
      format!("{name} Bool8 operand exceeds one"),
    ));
  }
  Ok(*value != 0)
}

/// Section properties live in WordDocument while their SED index lives in the
/// selected Table stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocSectionProperties {
  pub section_index: usize,
  pub offset: i32,
  pub physical_len: usize,
  pub value: Option<Sepx>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocWordDocumentStream {
  pub fib: Fib,
  pub text_pieces: Vec<DocTextPiece>,
  /// Aggregated CHPX FKP runs. `None` is reserved for compatible-mode input
  /// whose nonconforming FKP ordering cannot form a CP tree.
  pub chpx_runs: Option<Vec<DocChpxRun>>,
  /// Aggregated PAPX FKP runs. `None` is reserved for compatible-mode input
  /// whose nonconforming FKP ordering cannot form a CP tree.
  pub papx_runs: Option<Vec<DocPapxRun>>,
  character_format_pages: Vec<DocFkpPage<ChpxFkp>>,
  paragraph_format_pages: Vec<DocFkpPage<PapxFkp>>,
  pub section_properties: Vec<DocSectionProperties>,
  physical_bytes: CfbStreamData,
  source_fib_len: usize,
  // Maps each current PlcPcd piece to its immutable index in the source CLX.
  // Current indices stay contiguous when an edit removes an entire piece.
  source_piece_indices: Vec<usize>,
  source_chpx_runs: Option<Vec<DocChpxRun>>,
  source_papx_runs: Option<Vec<DocPapxRun>>,
  // Each edit is expressed in the piece coordinate space produced by all
  // preceding edits. The ordered map is part of save semantics because FKP
  // FC boundaries still refer to the source WordDocument until layout.
  pending_text_edits: BTreeMap<usize, Vec<CpReplacement>>,
  rebuild_character_formatting: bool,
  rebuild_paragraph_formatting: bool,
}

impl DocWordDocumentStream {
  /// Physical ChpxFkp pages retained for byte-preserving diagnostics. Edit
  /// `chpx_runs` to change character formatting.
  pub fn chpx_fkp_pages(&self) -> &[DocFkpPage<ChpxFkp>] {
    &self.character_format_pages
  }

  /// Physical PapxFkp pages retained for byte-preserving diagnostics. Edit
  /// `papx_runs` to change paragraph, table-row, or table-cell formatting.
  pub fn papx_fkp_pages(&self) -> &[DocFkpPage<PapxFkp>] {
    &self.paragraph_format_pages
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParagraphMarkEdit {
  PreserveAll,
  ExplicitPapx,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocTableStream {
  pub name: DocTableStreamName,
  pub clx: DocLocated<Clx>,
  pub character_bin_table: DocLocated<PlcBte>,
  pub paragraph_bin_table: DocLocated<PlcBte>,
  pub sections: DocLocated<PlcfSed>,
  pub styles: Option<DocLocated<StyleSheet>>,
  pub fonts: Option<DocLocated<FontTable>>,
  pub fields: BTreeMap<FieldDocumentPart, DocLocated<FieldTable>>,
  pub bookmarks: Option<DocLocatedBookmarks>,
  pub header_text: Option<DocLocated<HeaderTextTable>>,
  pub footnotes: Option<DocNoteTables>,
  pub endnotes: Option<DocNoteTables>,
  pub annotations: Option<DocAnnotationTables>,
  pub annotation_owners: Option<DocLocated<AnnotationOwners>>,
  pub annotation_bookmarks: Option<DocLocatedAnnotationBookmarks>,
  pub annotation_extended_data: Option<DocLocated<AnnotationExtendedData>>,
  pub textbox_stories: BTreeMap<TextboxDocumentPart, DocLocated<TextboxStoryTable>>,
  pub textbox_breaks: BTreeMap<TextboxDocumentPart, DocLocated<TextboxBreakTable>>,
  pub shape_anchors: BTreeMap<TextboxDocumentPart, DocLocated<ShapeAnchorTable>>,
  pub office_art: Option<DocLocated<DocOfficeArtContent>>,
  pub revision_authors: Option<DocLocated<RevisionAuthors>>,
  pub captions: Option<DocCaptionTables>,
  pub subdocuments: Option<DocLocated<SubdocumentTable>>,
  pub user_variables: Option<DocLocated<UserVariables>>,
  pub embedded_fonts: Option<DocLocated<EmbeddedFontTable>>,
  pub spelling_state: Option<DocLocated<SpellingStateTable>>,
  pub grammar_state: Option<DocLocated<GrammarStateTable>>,
  pub language_detection_state: Option<DocLocated<LanguageDetectionStateTable>>,
  pub list_definitions: Option<DocListDefinitions>,
  pub list_names: Option<DocLocated<ListNamesTable>>,
  pub list_overrides: Option<DocLocated<ListOverrides>>,
  pub document_properties: Option<DocLocated<DocumentProperties>>,
  pub associated_strings: Option<DocLocated<AssociatedStrings>>,
  pub external_file_names: Option<DocLocated<ExternalFileNameTable>>,
  pub mail_merge_state: Option<DocLocated<MailMergeState>>,
  pub new_mail_merge_state: Option<DocLocated<MailMergeState>>,
  pub office_data_source: Option<DocLocated<OfficeDataSource>>,
  pub printer_driver_info: Option<DocLocated<PrinterDriverInfo>>,
  pub ole_control_infos: Option<DocLocated<OleControlInfos>>,
  pub table_character_cache: Option<DocLocated<TableCharacterCacheTable>>,
  pub revision_message_threading: Option<DocLocated<RevisionMessageThreading>>,
  pub list_style_templates: Option<DocLocated<ListStyleTemplates>>,
  pub frame_and_list_records: Option<DocLocated<FrameAndListRecords>>,
  pub grammar_option_sets: Option<DocLocated<GrammarOptionSets>>,
  pub legacy_grammar_option_sets: Option<DocLocated<LegacyGrammarOptionSets>>,
  pub auto_summary_ranges: Option<DocLocated<AutoSummaryRangeTable>>,
  pub smart_tag_recognizer_state: Option<DocLocated<SmartTagRecognizerStateTable>>,
  pub xml_schema_references: Option<DocLocated<XmlSchemaReferences>>,
  pub xml_transform_path: Option<DocLocated<XmlTransformPath>>,
  pub paragraph_group_properties: Option<DocLocated<ParagraphGroupProperties>>,
  pub save_history: Option<DocLocated<SaveHistory>>,
  pub grammar_checker_cookies: Option<DocLocated<GrammarCheckerCookieTable>>,
  pub legacy_grammar_checker_cookies: Option<DocLocated<LegacyGrammarCheckerCookieTable>>,
  pub grammar_cookie_data: Option<DocLocated<GrammarCookieStore>>,
  pub smart_tag_data: Option<DocLocated<SmartTagData>>,
  pub revision_save_ids: Option<DocLocated<RevisionSaveIdTable>>,
  pub selection_state: Option<DocLocated<SelectionState>>,
  pub command_customizations: Option<DocLocated<CommandCustomizations>>,
  pub structured_tag_bookmarks: Option<DocBookmarkSet<StructuredTagBookmarks>>,
  pub range_protection: Option<DocRangeProtectionTables>,
  pub smart_tag_bookmarks: Option<DocBookmarkSet<SmartTagBookmarks>>,
  pub format_consistency_bookmarks: Option<DocBookmarkSet<FormatConsistencyBookmarks>>,
  pub repair_bookmarks: Option<DocBookmarkSet<RepairBookmarks>>,
  pub user_input_methods: Option<DocUserInputMethodTables>,
  pub mso_envelope: Option<DocLocated<MsoEnvelope>>,
  pub deprecated_numbering_field_cache: Option<DocDeprecatedNumberingFieldCache>,
  /// Nonconforming FIB-referenced payloads retained only in compatible mode.
  pub compatibility_tables: Vec<DocCompatibilityTable>,
  physical_bytes: CfbStreamData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocLocatedBookmarks {
  pub names_location: FibFcLcb,
  pub starts_location: FibFcLcb,
  pub ends_location: FibFcLcb,
  pub value: Bookmarks,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocLocatedAnnotationBookmarks {
  pub infos_location: FibFcLcb,
  pub starts_location: FibFcLcb,
  pub ends_location: FibFcLcb,
  pub value: AnnotationBookmarks,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocCompatibilityTable {
  pub label: String,
  pub location: FibFcLcb,
  pub physical_bytes: Option<Vec<u8>>,
  pub reason: String,
}

/// A note story is indexed by a reference PLC and a CP-only text PLC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocNoteTables {
  pub references: DocLocated<NoteReferenceTable>,
  pub text: DocLocated<CpOnlyTable>,
}

/// Comment references and their corresponding comment-story boundaries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocAnnotationTables {
  pub references: DocLocated<AnnotationReferenceTable>,
  pub text: DocLocated<CpOnlyTable>,
}

/// User-defined caption labels and automatic-caption mappings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocCaptionTables {
  pub definitions: DocLocated<CaptionDefinitions>,
  pub automatic: DocLocated<AutoCaptionDefinitions>,
}

/// `PlfLst` can place its variable-length LVL array immediately after the
/// FIB-declared range; both physical regions form one logical list tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocListDefinitions {
  pub location: FibFcLcb,
  pub value: ListDefinitions,
  trailing_levels_len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocBookmarkSet<T> {
  pub metadata_location: FibFcLcb,
  pub starts_location: FibFcLcb,
  pub ends_location: FibFcLcb,
  pub value: T,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocRangeProtectionTables {
  pub permissions_location: FibFcLcb,
  pub starts_location: FibFcLcb,
  pub ends_location: FibFcLcb,
  pub users_location: FibFcLcb,
  pub value: RangeProtection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocUserInputMethodTables {
  pub methods_location: FibFcLcb,
  pub service_guids_location: FibFcLcb,
  pub value: UserInputMethods,
}

/// MS-DOC marks this cache as deprecated and says it SHOULD be ignored. Its
/// bounded bytes remain explicit without inventing an undocumented layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocDeprecatedNumberingFieldCache {
  pub location: FibFcLcb,
  pub physical_bytes: Vec<u8>,
}

/// MS-DOC assigns structure-specific offsets into this stream. Until each
/// referenced payload is promoted, the stream remains one explicit physical
/// node rather than being mislabeled as arbitrary content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocDataStream {
  pub physical_bytes: CfbStreamData,
  pub nodes: Vec<DocDataNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocDataNode {
  pub offset: u32,
  pub physical_len: usize,
  pub value: DocDataNodeValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocDataNodeValue {
  Picture(PicfAndOfficeArtData),
  Binary(Box<NilPicfAndBinData>),
  ParagraphProperties(PrcData),
}

struct RebuiltDataStream<'a> {
  plan: Option<TableWritePlan<'a>>,
  relocations: BTreeMap<u32, u32>,
}

/// The MS-DOC ObjectPool storage aggregates each embedded object storage with
/// its required ObjInfo/ODT stream. Other streams remain CFB-managed payloads
/// for their owning format libraries and are identified by `entry_paths`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocObjectPoolStorage {
  pub path: PathBuf,
  pub objects: Vec<DocEmbeddedObjectStorage>,
  pub compatibility_objects: Vec<DocCompatibilityObjectStorage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocEmbeddedObjectStorage {
  pub path: PathBuf,
  pub descriptor_stream_path: PathBuf,
  pub descriptor: OleObjectDescriptor,
  pub entry_paths: Vec<PathBuf>,
}

/// An ObjectPool child storage retained in compatible mode when its required
/// ObjInfo/ODT cannot be typed. The storage hierarchy remains reachable and
/// byte-preserved without inventing an OLE descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocCompatibilityObjectStorage {
  pub path: PathBuf,
  pub descriptor_stream_path: Option<PathBuf>,
  pub entry_paths: Vec<PathBuf>,
  pub reason: String,
}

/// Complete file root with the primary MS-DOC content structures linked into
/// a Rust tree.
///
/// The source CFB image is private so managed streams and this typed tree
/// cannot become competing write authorities. Unknown and externally-owned
/// entries remain available through [`Self::source_compound_file`]; saving
/// preserves that source hierarchy and replaces managed streams from these
/// typed fields.
///
/// See the runnable `edit_doc` example for open, traversal, semantic text edit,
/// save, and strict reopen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocFile {
  compound_file: CompoundFile,
  data_link_baseline: Arc<DocDataLinkBaseline>,
  pub shared: OfficeSharedContent,
  /// Clone-shared WordDocument typed tree. Call [`Arc::make_mut`] before
  /// direct field edits; transactional SDK methods detach it automatically.
  pub word_document: Arc<DocWordDocumentStream>,
  /// Clone-shared Table-stream typed tree, detached automatically by SDK
  /// mutations and explicitly with [`Arc::make_mut`] for direct edits.
  pub table: Arc<DocTableStream>,
  /// Clone-shared Data-stream typed tree and physical preservation backing.
  pub data: Option<Arc<DocDataStream>>,
  /// Clone-shared embedded-object graph retained from the ObjectPool storage.
  pub object_pool: Option<Arc<DocObjectPoolStorage>>,
}

impl DocCp {
  pub const fn new(value: u32) -> Self {
    Self(value)
  }

  pub const fn value(self) -> u32 {
    self.0
  }
}

impl DocFc {
  pub const fn new(value: u32) -> Self {
    Self(value)
  }

  pub const fn value(self) -> u32 {
    self.0
  }
}

impl DocCpRange {
  pub const fn len(self) -> u32 {
    self.end.0 - self.start.0
  }

  pub const fn is_empty(self) -> bool {
    self.start.0 == self.end.0
  }

  pub const fn contains(self, cp: DocCp) -> bool {
    self.start.0 <= cp.0 && cp.0 < self.end.0
  }
}

impl DocFcRange {
  pub const fn len(self) -> u32 {
    self.end.0 - self.start.0
  }

  pub const fn is_empty(self) -> bool {
    self.start.0 == self.end.0
  }
}

impl<'a> DocContentTree<'a> {
  pub fn parts(&self) -> &[DocDocumentPartRef<'a>; 7] {
    &self.parts
  }

  pub fn part(&self, part: FieldDocumentPart) -> Option<DocDocumentPartRef<'a>> {
    self
      .parts
      .iter()
      .find(|candidate| candidate.part == part)
      .copied()
  }

  pub fn macro_fields(&self) -> Option<&'a DocLocated<FieldTable>> {
    self.file.table.fields.get(&FieldDocumentPart::Macro)
  }

  pub fn data_nodes(&self) -> impl Iterator<Item = &'a DocDataNode> + '_ {
    self.file.data.iter().flat_map(|data| data.nodes.iter())
  }

  pub fn object_pool(&self) -> Option<&'a DocObjectPoolStorage> {
    self.file.object_pool.as_deref()
  }

  pub const fn preserves_compatibility(&self) -> bool {
    self.preserve_compatibility
  }

  /// Joins SttbfBkmk, PlcfBkf, and PlcfBkl into named ranges in the
  /// aggregate coordinate space of all document parts.
  pub fn bookmarks(&self) -> Result<DocBookmarks<'a>> {
    build_bookmarks(self.file, &self.parts, self.preserve_compatibility)
  }

  /// Joins Main Document footnote references to Footnote Document ranges.
  pub fn footnotes(&self) -> Result<DocNotes<'a>> {
    build_notes(
      self.file,
      self.parts[0],
      self
        .part(FieldDocumentPart::Footnote)
        .expect("the seven-part inventory contains Footnote"),
      DocNoteKind::Footnote,
      self.preserve_compatibility,
    )
  }

  /// Joins Main Document endnote references to Endnote Document ranges.
  pub fn endnotes(&self) -> Result<DocNotes<'a>> {
    build_notes(
      self.file,
      self.parts[0],
      self
        .part(FieldDocumentPart::Endnote)
        .expect("the seven-part inventory contains Endnote"),
      DocNoteKind::Endnote,
      self.preserve_compatibility,
    )
  }

  /// Joins comment anchors, comment text, authors, optional thread data,
  /// and annotation bookmarks into one borrowed relationship inventory.
  pub fn comments(&self) -> Result<DocComments<'a>> {
    build_comments(
      self.file,
      self.parts[0],
      self
        .part(FieldDocumentPart::Comment)
        .expect("the seven-part inventory contains Comment"),
      self.preserve_compatibility,
    )
  }

  pub fn main_textboxes(&self) -> Result<DocTextboxes<'a>> {
    build_textboxes(
      self.file,
      self
        .part(FieldDocumentPart::Main)
        .expect("the seven-part inventory contains Main"),
      self
        .part(FieldDocumentPart::Textbox)
        .expect("the seven-part inventory contains Textbox"),
      TextboxDocumentPart::Main,
      self.preserve_compatibility,
    )
  }

  pub fn header_textboxes(&self) -> Result<DocTextboxes<'a>> {
    build_textboxes(
      self.file,
      self
        .part(FieldDocumentPart::Header)
        .expect("the seven-part inventory contains Header"),
      self
        .part(FieldDocumentPart::HeaderTextbox)
        .expect("the seven-part inventory contains HeaderTextbox"),
      TextboxDocumentPart::Header,
      self.preserve_compatibility,
    )
  }
}

impl<'a> DocTextRangeRef<'a> {
  pub const fn document_part(self) -> DocDocumentPartRef<'a> {
    self.document_part
  }

  pub const fn local_cp_range(self) -> DocCpRange {
    self.local_cp_range
  }

  pub const fn global_cp_range(self) -> DocCpRange {
    self.global_cp_range
  }

  pub fn character_at(self, relative_cp: DocCp) -> Option<u16> {
    if relative_cp.0 >= self.local_cp_range.len() {
      return None;
    }
    self.document_part.character_at(DocCp(
      self.local_cp_range.start.0.checked_add(relative_cp.0)?,
    ))
  }

  pub fn text_pieces(self) -> impl Iterator<Item = DocTextPieceRef<'a>> {
    self
      .document_part
      .text_pieces()
      .filter(move |piece| cp_ranges_overlap(piece.local_cp_range, self.local_cp_range))
  }

  pub fn paragraphs(self) -> impl Iterator<Item = DocParagraphRef<'a>> {
    self
      .document_part
      .paragraphs()
      .filter(move |paragraph| cp_ranges_overlap(paragraph.local_cp_range, self.local_cp_range))
  }

  pub fn character_runs(self) -> impl Iterator<Item = DocCharacterRunRef<'a>> {
    self
      .document_part
      .character_runs()
      .filter(move |run| cp_ranges_overlap(run.local_cp_range, self.local_cp_range))
  }

  pub fn fields(self) -> impl Iterator<Item = DocFieldRef<'a>> {
    self.document_part.fields().filter(move |field| {
      field
        .local_cp_range()
        .is_ok_and(|range| cp_ranges_overlap(range, self.local_cp_range))
    })
  }

  pub fn special_contents(self) -> Result<Vec<DocSpecialContentRef<'a>>> {
    Ok(
      self
        .document_part
        .special_contents()?
        .into_iter()
        .filter(|content| self.local_cp_range.contains(content.character()))
        .collect(),
    )
  }

  pub fn tables(self) -> Result<DocTables<'a>> {
    let mut tables = self.document_part.tables()?;
    tables
      .tables
      .retain(|table| cp_ranges_overlap(table.local_cp_range, self.local_cp_range));
    Ok(tables)
  }

  /// Combines the range's ordinary paragraphs and completed outer tables in
  /// document order. Table member paragraphs are not duplicated.
  pub fn blocks(self) -> Result<DocBlocks<'a>> {
    let tables = self.tables()?;
    self.blocks_with_tables(&tables)
  }

  /// Builds document-order blocks from a caller-retained table index for
  /// the same document part.
  pub fn blocks_with_tables(self, tables: &DocTables<'a>) -> Result<DocBlocks<'a>> {
    validate_table_index_owner(self.document_part, tables)?;
    let outer_tables = tables
      .tables
      .iter()
      .filter(|candidate| {
        cp_ranges_overlap(candidate.local_cp_range, self.local_cp_range)
          && !tables.tables.iter().any(|container| {
            container.table_depth < candidate.table_depth
              && container.global_cp_range.start.0 <= candidate.global_cp_range.start.0
              && candidate.global_cp_range.end.0 <= container.global_cp_range.end.0
          })
      })
      .cloned()
      .collect::<Vec<_>>();
    let mut blocks = self
      .paragraphs()
      .filter(|paragraph| {
        !outer_tables.iter().any(|table| {
          table.global_cp_range.start.0 <= paragraph.global_cp_range.start.0
            && paragraph.global_cp_range.end.0 <= table.global_cp_range.end.0
        })
      })
      .map(DocBlockRef::Paragraph)
      .collect::<Vec<_>>();
    blocks.extend(outer_tables.into_iter().map(DocBlockRef::Table));
    blocks.sort_by_key(DocBlockRef::global_cp_start);
    Ok(DocBlocks {
      blocks,
      diagnostics: tables.diagnostics.clone(),
    })
  }
}

impl<'a> DocBookmarks<'a> {
  pub fn bookmarks(&self) -> &[DocBookmarkRef<'a>] {
    &self.bookmarks
  }

  pub fn diagnostics(&self) -> &[DocRelationshipDiagnostic] {
    &self.diagnostics
  }
}

impl<'a> DocBookmarkRef<'a> {
  pub const fn index(self) -> usize {
    self.index
  }

  pub const fn name(self) -> &'a [u16] {
    self.name
  }

  pub const fn properties(self) -> &'a BookmarkStart {
    self.properties
  }

  pub const fn text(self) -> DocTextRangeRef<'a> {
    self.text
  }

  pub fn is_hidden(self) -> bool {
    self.name.first() == Some(&0x005f)
  }
}

impl<'a> DocNotes<'a> {
  pub const fn kind(&self) -> DocNoteKind {
    self.kind
  }

  pub fn notes(&self) -> &[DocNoteRef<'a>] {
    &self.notes
  }

  pub fn diagnostics(&self) -> &[DocRelationshipDiagnostic] {
    &self.diagnostics
  }

  pub fn note_at_reference_cp(&self, cp: DocCp) -> Option<DocNoteRef<'a>> {
    self
      .notes
      .binary_search_by_key(&cp, |note| note.reference_cp)
      .ok()
      .map(|index| self.notes[index])
  }
}

impl<'a> DocNoteRef<'a> {
  pub const fn kind(self) -> DocNoteKind {
    self.kind
  }

  pub const fn index(self) -> usize {
    self.index
  }

  pub const fn reference_document(self) -> DocDocumentPartRef<'a> {
    self.reference_document
  }

  pub const fn reference_cp(self) -> DocCp {
    self.reference_cp
  }

  pub const fn numbering_index(self) -> u16 {
    *self.numbering_index
  }

  pub const fn is_automatically_numbered(self) -> bool {
    *self.numbering_index != 0
  }

  pub fn reference_character(self) -> Option<u16> {
    self.reference_document.character_at(self.reference_cp)
  }

  pub fn reference_has_effective_cf_spec(self) -> Result<bool> {
    self
      .reference_document
      .file
      .effective_cf_spec_at_cp(FieldDocumentPart::Main, self.reference_cp.0)
  }

  pub const fn text(self) -> DocTextRangeRef<'a> {
    self.text
  }
}

impl<'a> DocAnnotationBookmarkRef<'a> {
  pub const fn index(self) -> usize {
    self.index
  }

  pub const fn info(self) -> &'a AnnotationBookmarkInfo {
    self.info
  }

  pub const fn properties(self) -> &'a BookmarkStart {
    self.properties
  }

  pub const fn text(self) -> DocTextRangeRef<'a> {
    self.text
  }
}

impl<'a> DocComments<'a> {
  pub fn comments(&self) -> &[DocCommentRef<'a>] {
    &self.comments
  }

  pub fn diagnostics(&self) -> &[DocRelationshipDiagnostic] {
    &self.diagnostics
  }

  pub fn comment_at_reference_cp(&self, cp: DocCp) -> Option<DocCommentRef<'a>> {
    self
      .comments
      .binary_search_by_key(&cp, |comment| comment.reference_cp)
      .ok()
      .map(|index| self.comments[index])
  }
}

impl<'a> DocCommentRef<'a> {
  pub const fn index(self) -> usize {
    self.index
  }

  pub const fn reference_document(self) -> DocDocumentPartRef<'a> {
    self.reference_document
  }

  pub const fn reference_cp(self) -> DocCp {
    self.reference_cp
  }

  pub const fn annotation(self) -> &'a AnnotationReference {
    self.annotation
  }

  pub fn initials(self) -> &'a [u16] {
    &self.annotation.initials_buffer[..usize::from(self.annotation.initials_length)]
  }

  pub const fn author(self) -> Option<&'a [u16]> {
    self.author
  }

  pub const fn extended(self) -> Option<&'a AnnotationPost10> {
    self.extended
  }

  pub const fn annotation_bookmark(self) -> Option<DocAnnotationBookmarkRef<'a>> {
    self.annotation_bookmark
  }

  /// The selected Main Document text on which the comment is placed. A
  /// comment with `lTagBkmk == -1` is an insertion point at its reference CP.
  pub fn commented_text(self) -> DocTextRangeRef<'a> {
    self.annotation_bookmark.map_or_else(
      || DocTextRangeRef {
        document_part: self.reference_document,
        local_cp_range: DocCpRange {
          start: self.reference_cp,
          end: self.reference_cp,
        },
        global_cp_range: DocCpRange {
          start: self.reference_cp,
          end: self.reference_cp,
        },
      },
      DocAnnotationBookmarkRef::text,
    )
  }

  pub const fn text(self) -> DocTextRangeRef<'a> {
    self.text
  }

  pub fn reference_has_effective_cf_spec(self) -> Result<bool> {
    self
      .file
      .effective_cf_spec_at_cp(FieldDocumentPart::Main, self.reference_cp.0)
  }

  pub fn text_marker_has_effective_cf_spec(self) -> Result<bool> {
    self
      .file
      .effective_cf_spec_at_cp(FieldDocumentPart::Comment, self.text.local_cp_range.start.0)
  }

  pub fn parent(self) -> Option<DocCommentRef<'a>> {
    let extended = self.extended?;
    if extended.depth == 0 {
      return None;
    }
    let parent = i64::try_from(self.index)
      .ok()?
      .checked_add(i64::from(extended.parent_offset))?;
    let parent = usize::try_from(parent).ok()?;
    comment_ref_at(
      self.file,
      self.reference_document,
      self.text.document_part,
      self.preserve_compatibility,
      parent,
    )
  }

  pub fn children(self) -> impl Iterator<Item = DocCommentRef<'a>> {
    let count = self
      .file
      .table
      .annotations
      .as_ref()
      .map_or(0, |tables| tables.references.value.annotations.len());
    (0..count).filter_map(move |index| {
      let child = comment_ref_at(
        self.file,
        self.reference_document,
        self.text.document_part,
        self.preserve_compatibility,
        index,
      )?;
      (comment_parent_index(child.extended, index) == Some(self.index)).then_some(child)
    })
  }
}

impl<'a> DocOfficeArtShapeRef<'a> {
  pub const fn document_part(self) -> TextboxDocumentPart {
    self.document_part
  }

  /// Zero-based OfficeArt shape-container order within this drawing. The
  /// order is the host's relative z-order input for floating objects.
  pub const fn z_order(self) -> usize {
    self.z_order
  }

  pub const fn container(self) -> &'a OfficeArtRecord {
    self.container
  }

  /// The OfficeArt `shapeType` stored in the shape record header.
  pub const fn shape_type(self) -> u16 {
    self.shape_type
  }

  pub const fn shape(self) -> &'a OfficeArtShape {
    self.shape
  }

  pub const fn client_textbox(self) -> Option<&'a OfficeArtWordClientTextbox> {
    self.client_textbox
  }

  pub const fn text_id_property(self) -> Option<&'a OfficeArtProperty> {
    self.text_id_property
  }

  pub const fn next_shape_id_property(self) -> Option<&'a OfficeArtProperty> {
    self.next_shape_id_property
  }

  /// Resolves the MSOPSText_lTxid property, falling back to the equivalent
  /// Word OfficeArtClientTextbox payload only when the property is absent.
  pub fn textbox_link(self) -> Option<DocTextboxShapeLink> {
    let text_id = simple_office_art_property(self.text_id_property).or_else(|| {
      self
        .client_textbox
        .map(|value| (u32::from(value.story_index) << 16) | u32::from(value.chain_index))
    })?;
    Some(DocTextboxShapeLink::from_text_id(text_id))
  }

  pub fn next_shape_id(self) -> Option<u32> {
    simple_office_art_property(self.next_shape_id_property)
  }

  /// Resolves the shape-owned `pib` property to its one-based OfficeArt
  /// BLIP-store identifier. A malformed property is rejected instead of
  /// being mistaken for an image index.
  pub fn primary_blip_identifier(self) -> Result<Option<u32>> {
    let Some(property) = self.property(0x0104) else {
      return Ok(None);
    };
    let OfficeArtPropertyValue::Simple(identifier) = &property.value else {
      return Err(Error::invalid(
        0,
        "DOC shape primary BLIP property is not a simple value",
      ));
    };
    if !property.is_blip_id {
      return Err(Error::invalid(
        0,
        "DOC shape primary BLIP property does not set fBid",
      ));
    }
    Ok((*identifier != 0).then_some(*identifier))
  }

  /// Returns effective text insets. Missing values use the [MS-ODRAW]
  /// defaults: 0.1 inch horizontally and 0.05 inch vertically.
  pub fn text_insets(self) -> DocOfficeArtTextInsets {
    DocOfficeArtTextInsets {
      left: self.signed_property_or(0x0081, 91_440),
      top: self.signed_property_or(0x0082, 45_720),
      right: self.signed_property_or(0x0083, 91_440),
      bottom: self.signed_property_or(0x0084, 45_720),
    }
  }

  pub fn wrap_distances(self) -> DocOfficeArtWrapDistances {
    DocOfficeArtWrapDistances {
      left: self.signed_property_or(0x0384, 0),
      top: self.signed_property_or(0x0385, 0),
      right: self.signed_property_or(0x0386, 0),
      bottom: self.signed_property_or(0x0387, 0),
    }
  }

  pub fn picture_crop(self) -> DocOfficeArtPictureCrop {
    DocOfficeArtPictureCrop {
      top: self.signed_property_or(0x0100, 0),
      bottom: self.signed_property_or(0x0101, 0),
      left: self.signed_property_or(0x0102, 0),
      right: self.signed_property_or(0x0103, 0),
    }
  }

  /// Borrows the custom wrapping polygon in its OfficeArt 21600-based shape
  /// coordinate space. An encoded non-point array is rejected explicitly.
  pub fn wrap_polygon(self) -> Result<Option<DocOfficeArtWrapPolygonRef<'a>>> {
    let Some(property) = self.property(0x0383) else {
      return Ok(None);
    };
    let OfficeArtPropertyValue::Array { value, .. } = &property.value else {
      return Err(Error::invalid(
        0,
        "DOC shape wrapping polygon property is not an OfficeArt array",
      ));
    };
    let points = match &value.data {
      OfficeArtArrayData::Points16(points) => DocOfficeArtWrapPoints::I16(points.as_slice()),
      OfficeArtArrayData::Points32(points) => DocOfficeArtWrapPoints::I32(points.as_slice()),
      _ => {
        return Err(Error::invalid(
          0,
          "DOC shape wrapping polygon array does not contain points",
        ));
      }
    };
    Ok(Some(DocOfficeArtWrapPolygonRef { points }))
  }

  pub fn fill(self) -> DocOfficeArtFill {
    if !self.boolean_property_or(0x01bb, true) {
      return DocOfficeArtFill::None;
    }
    let fill_type = self.simple_property(0x0180).unwrap_or(0);
    if fill_type != 0 {
      return DocOfficeArtFill::Other { fill_type };
    }
    DocOfficeArtFill::Solid(office_art_color(
      self.simple_property(0x0181).unwrap_or(0x00ff_ffff),
    ))
  }

  pub fn line(self) -> DocOfficeArtLine {
    if !self.boolean_property_or(0x01fc, true) {
      return DocOfficeArtLine::None;
    }
    if self.simple_property(0x01c4).is_some_and(|value| value != 0)
      || self.simple_property(0x01cd).is_some_and(|value| value != 0)
      || self.simple_property(0x01ce).is_some_and(|value| value != 0)
      || self.simple_property(0x01cf).is_some()
    {
      return DocOfficeArtLine::Other;
    }
    DocOfficeArtLine::Solid {
      color: office_art_color(self.simple_property(0x01c0).unwrap_or(0)),
      width_emu: self.signed_property_or(0x01cb, 9_525),
    }
  }

  /// Whether the floating object participates in its containing table cell.
  /// [MS-ODRAW] defines the default as true when the use bit is absent.
  pub fn layout_in_cell(self) -> bool {
    self.boolean_property_or(0x03b0, true)
  }

  /// Whether the object can overlap another object. [MS-ODRAW] defines the
  /// default as true when the use bit is absent.
  pub fn allow_overlap(self) -> bool {
    self.boolean_property_or(0x03b6, true)
  }

  pub fn hidden(self) -> bool {
    self.boolean_property_or(0x03be, false)
  }

  fn property(self, property_id: u16) -> Option<&'a OfficeArtProperty> {
    last_office_art_property(self.properties, property_id)
  }

  fn simple_property(self, property_id: u16) -> Option<u32> {
    simple_office_art_property(self.property(property_id))
  }

  fn signed_property_or(self, property_id: u16, default: i32) -> i32 {
    self
      .simple_property(property_id)
      .map(|value| i32::from_le_bytes(value.to_le_bytes()))
      .unwrap_or(default)
  }

  fn boolean_property_or(self, property_id: u16, default: bool) -> bool {
    let base_id = property_id | 0x000f;
    let Some(value) = self.simple_property(base_id) else {
      return default;
    };
    let value_bit = u32::from(base_id - property_id);
    let use_bit = value_bit + 16;
    if value & (1 << use_bit) == 0 {
      default
    } else {
      value & (1 << value_bit) != 0
    }
  }
}

impl DocOfficeArtTextInsets {
  pub const fn left(self) -> i32 {
    self.left
  }

  pub const fn top(self) -> i32 {
    self.top
  }

  pub const fn right(self) -> i32 {
    self.right
  }

  pub const fn bottom(self) -> i32 {
    self.bottom
  }
}

impl DocOfficeArtWrapDistances {
  pub const fn left(self) -> i32 {
    self.left
  }

  pub const fn top(self) -> i32 {
    self.top
  }

  pub const fn right(self) -> i32 {
    self.right
  }

  pub const fn bottom(self) -> i32 {
    self.bottom
  }
}

impl DocOfficeArtPictureCrop {
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

impl DocOfficeArtWrapPolygonRef<'_> {
  pub fn len(self) -> usize {
    match self.points {
      DocOfficeArtWrapPoints::I16(points) => points.len(),
      DocOfficeArtWrapPoints::I32(points) => points.len(),
    }
  }

  pub fn is_empty(self) -> bool {
    self.len() == 0
  }

  pub fn point(self, index: usize) -> Option<(i64, i64)> {
    match self.points {
      DocOfficeArtWrapPoints::I16(points) => points
        .get(index)
        .map(|point| (i64::from(point.x), i64::from(point.y))),
      DocOfficeArtWrapPoints::I32(points) => points
        .get(index)
        .map(|point| (i64::from(point.x), i64::from(point.y))),
    }
  }
}

const fn office_art_color(value: u32) -> DocOfficeArtColor {
  if value & 0xff00_0000 == 0 {
    DocOfficeArtColor::Rgb {
      red: value as u8,
      green: (value >> 8) as u8,
      blue: (value >> 16) as u8,
    }
  } else {
    DocOfficeArtColor::Other(value)
  }
}

impl DocTextboxShapeLink {
  const fn from_text_id(text_id: u32) -> Self {
    Self {
      text_id,
      story_index: (text_id >> 16) as u16,
      chain_index: text_id as u16,
    }
  }

  pub const fn text_id(self) -> u32 {
    self.text_id
  }

  pub const fn story_index(self) -> u16 {
    self.story_index
  }

  pub const fn chain_index(self) -> u16 {
    self.chain_index
  }
}

impl<'a> DocTextboxBreakRef<'a> {
  pub const fn index(self) -> usize {
    self.index
  }

  pub const fn source(self) -> &'a TextboxBreak {
    self.source
  }

  pub const fn text(self) -> DocTextRangeRef<'a> {
    self.text
  }

  pub const fn story_index(self) -> Option<usize> {
    self.story_index
  }
}

impl<'a> DocTextboxStoryRef<'a> {
  pub const fn document_part(&self) -> TextboxDocumentPart {
    self.document_part
  }

  pub const fn index(&self) -> usize {
    self.index
  }

  pub const fn source(&self) -> &'a TextboxStory {
    self.source
  }

  pub const fn text(&self) -> DocTextRangeRef<'a> {
    self.text
  }

  pub const fn is_reusable(&self) -> bool {
    self.reusable
  }

  pub fn shapes(&self) -> &[DocOfficeArtShapeRef<'a>] {
    &self.shapes
  }

  pub fn breaks(&self) -> &[DocTextboxBreakRef<'a>] {
    &self.breaks
  }
}

impl<'a> DocShapeAnchorRef<'a> {
  pub const fn index(self) -> usize {
    self.index
  }

  pub const fn anchor_document(self) -> DocDocumentPartRef<'a> {
    self.anchor_document
  }

  pub const fn anchor_cp(self) -> DocCp {
    self.anchor_cp
  }

  pub const fn source(self) -> &'a ShapeAnchor {
    self.source
  }

  pub const fn shape(self) -> Option<DocOfficeArtShapeRef<'a>> {
    self.shape
  }

  pub fn anchor_character(self) -> Option<u16> {
    self.anchor_document.character_at(self.anchor_cp)
  }

  pub fn anchor_has_effective_cf_spec(self) -> Result<bool> {
    self
      .anchor_document
      .file
      .effective_cf_spec_at_cp(self.anchor_document.part, self.anchor_cp.0)
  }
}

impl<'a> DocTextboxes<'a> {
  pub const fn document_part(&self) -> TextboxDocumentPart {
    self.document_part
  }

  pub fn stories(&self) -> &[DocTextboxStoryRef<'a>] {
    &self.stories
  }

  pub fn breaks(&self) -> &[DocTextboxBreakRef<'a>] {
    &self.breaks
  }

  pub fn anchors(&self) -> &[DocShapeAnchorRef<'a>] {
    &self.anchors
  }

  pub fn shapes(&self) -> &[DocOfficeArtShapeRef<'a>] {
    &self.shapes
  }

  pub fn diagnostics(&self) -> &[DocRelationshipDiagnostic] {
    &self.diagnostics
  }

  pub fn story(&self, index: usize) -> Option<&DocTextboxStoryRef<'a>> {
    self.stories.iter().find(|story| story.index == index)
  }

  pub fn shape(&self, shape_id: u32) -> Option<DocOfficeArtShapeRef<'a>> {
    self
      .shapes
      .iter()
      .find(|shape| shape.shape.shape_id == shape_id)
      .copied()
  }
}

impl<'a> DocDocumentPartRef<'a> {
  pub const fn part(self) -> FieldDocumentPart {
    self.part
  }

  pub const fn global_cp_range(self) -> DocCpRange {
    self.global_cp_range
  }

  pub const fn local_cp_range(self) -> DocCpRange {
    DocCpRange {
      start: DocCp(0),
      end: DocCp(self.global_cp_range.len()),
    }
  }

  pub fn text_pieces(self) -> impl Iterator<Item = DocTextPieceRef<'a>> {
    let part_start = self.global_cp_range.start.0;
    self
      .file
      .word_document
      .text_pieces
      .iter()
      .filter_map(move |source| {
        let source_start = u32::try_from(source.value.cp_start).ok()?;
        let source_end = u32::try_from(source.value.cp_end).ok()?;
        let start = source_start.max(self.global_cp_range.start.0);
        let end = source_end.min(self.global_cp_range.end.0);
        if start >= end {
          return None;
        }
        let character_start = usize::try_from(start - source_start).ok()?;
        let character_end = usize::try_from(end - source_start).ok()?;
        let width = match source.value.characters.encoding() {
          TextPieceEncoding::Compressed => 1,
          TextPieceEncoding::Utf16 => 2,
        };
        let fc_start = source
          .value
          .file_offset
          .checked_add((start - source_start).checked_mul(width)?)?;
        let fc_end = source
          .value
          .file_offset
          .checked_add((end - source_start).checked_mul(width)?)?;
        Some(DocTextPieceRef {
          document_part: self,
          source,
          descriptor: self
            .file
            .table
            .clx
            .value
            .piece_table
            .pieces
            .get(source.piece_index),
          global_cp_range: DocCpRange {
            start: DocCp(start),
            end: DocCp(end),
          },
          local_cp_range: DocCpRange {
            start: DocCp(start - part_start),
            end: DocCp(end - part_start),
          },
          fc_range: DocFcRange {
            start: DocFc(fc_start),
            end: DocFc(fc_end),
          },
          character_start,
          character_end,
        })
      })
  }

  pub fn paragraphs(self) -> impl Iterator<Item = DocParagraphRef<'a>> {
    let part_start = self.global_cp_range.start.0;
    self
      .file
      .word_document
      .papx_runs
      .as_deref()
      .into_iter()
      .flat_map(|runs| runs.iter())
      .filter_map(move |source| {
        let start = source.cp_start.max(self.global_cp_range.start.0);
        let end = source.cp_end.min(self.global_cp_range.end.0);
        (start < end).then(|| DocParagraphRef {
          document_part: self,
          source,
          global_cp_range: DocCpRange {
            start: DocCp(start),
            end: DocCp(end),
          },
          local_cp_range: DocCpRange {
            start: DocCp(start - part_start),
            end: DocCp(end - part_start),
          },
        })
      })
  }

  /// Combines ordinary paragraphs and completed outer tables in document
  /// order. Table member paragraphs are not duplicated as sibling blocks.
  pub fn blocks(self) -> Result<DocBlocks<'a>> {
    let tables = self.tables()?;
    self.blocks_with_tables(&tables)
  }

  /// Builds document-order blocks from an already resolved table index.
  ///
  /// A converter or editor that recursively walks table cells can retain
  /// one [`DocTables`] value for the whole document part instead of
  /// rebuilding every row/table relationship for every cell.
  pub fn blocks_with_tables(self, tables: &DocTables<'a>) -> Result<DocBlocks<'a>> {
    validate_table_index_owner(self, tables)?;
    let outer_tables = tables
      .tables
      .iter()
      .filter(|candidate| {
        !tables.tables.iter().any(|container| {
          container.table_depth < candidate.table_depth
            && container.global_cp_range.start.0 <= candidate.global_cp_range.start.0
            && candidate.global_cp_range.end.0 <= container.global_cp_range.end.0
        })
      })
      .cloned()
      .collect::<Vec<_>>();
    let mut blocks = self
      .paragraphs()
      .filter(|paragraph| {
        !outer_tables.iter().any(|table| {
          table.global_cp_range.start.0 <= paragraph.global_cp_range.start.0
            && paragraph.global_cp_range.end.0 <= table.global_cp_range.end.0
        })
      })
      .map(DocBlockRef::Paragraph)
      .collect::<Vec<_>>();
    blocks.extend(outer_tables.into_iter().map(DocBlockRef::Table));
    blocks.sort_by_key(DocBlockRef::global_cp_start);
    Ok(DocBlocks {
      blocks,
      diagnostics: tables.diagnostics.clone(),
    })
  }

  /// Joins the Main Document's PlcfSed ranges to their SED and Sepx owners.
  pub fn sections(self) -> Result<DocSections<'a>> {
    if self.part != FieldDocumentPart::Main {
      return Err(Error::invalid(
        u64::from(self.global_cp_range.start.0),
        "PlcfSed ranges belong only to the Main Document",
      ));
    }
    let table = &self.file.table.sections.value;
    if table.character_positions.len() != table.sections.len() + 1
      || table.sections.len() != self.file.word_document.section_properties.len()
    {
      return Err(Error::invalid(0, "PlcfSed/SED/Sepx cardinality changed"));
    }
    let mut sections = Vec::with_capacity(table.sections.len());
    for (section_index, ((range, source), properties)) in table
      .character_positions
      .windows(2)
      .zip(&table.sections)
      .zip(&self.file.word_document.section_properties)
      .enumerate()
    {
      let start = u32::try_from(range[0])
        .map_err(|_| Error::invalid(0, "PlcfSed has a negative start CP"))?;
      let end =
        u32::try_from(range[1]).map_err(|_| Error::invalid(0, "PlcfSed has a negative end CP"))?;
      if start > end || end > self.local_cp_range().end.0 {
        return Err(Error::invalid(
          u64::from(start),
          "PlcfSed range exceeds the Main Document",
        ));
      }
      if properties.section_index != section_index || properties.offset != source.sepx_offset {
        return Err(Error::invalid(
          u64::try_from(section_index).unwrap_or(u64::MAX),
          "SED/Sepx relationship identity changed",
        ));
      }
      sections.push(DocSectionRef {
        document_part: self,
        section_index,
        local_cp_range: DocCpRange {
          start: DocCp(start),
          end: DocCp(end),
        },
        global_cp_range: DocCpRange {
          start: DocCp(
            self
              .global_cp_range
              .start
              .0
              .checked_add(start)
              .ok_or_else(|| Error::Limit("DOC section CP overflow".into()))?,
          ),
          end: DocCp(
            self
              .global_cp_range
              .start
              .0
              .checked_add(end)
              .ok_or_else(|| Error::Limit("DOC section CP overflow".into()))?,
          ),
        },
        source,
        properties,
      });
    }
    Ok(DocSections { sections })
  }

  pub fn character_runs(self) -> impl Iterator<Item = DocCharacterRunRef<'a>> {
    let part_start = self.global_cp_range.start.0;
    self
      .file
      .word_document
      .chpx_runs
      .as_deref()
      .into_iter()
      .flat_map(|runs| runs.iter())
      .filter_map(move |source| {
        let start = source.cp_start.max(self.global_cp_range.start.0);
        let end = source.cp_end.min(self.global_cp_range.end.0);
        (start < end).then(|| DocCharacterRunRef {
          document_part: self,
          source,
          global_cp_range: DocCpRange {
            start: DocCp(start),
            end: DocCp(end),
          },
          local_cp_range: DocCpRange {
            start: DocCp(start - part_start),
            end: DocCp(end - part_start),
          },
        })
      })
  }

  /// Returns the original character unit at a part-local CP without decoding
  /// or concatenating text pieces.
  pub fn character_at(self, local_cp: DocCp) -> Option<u16> {
    if !self.local_cp_range().contains(local_cp) {
      return None;
    }
    self.text_pieces().find_map(|piece| {
      if !piece.local_cp_range.contains(local_cp) {
        return None;
      }
      let index = usize::try_from(local_cp.0 - piece.local_cp_range.start.0).ok()?;
      piece
        .source
        .value
        .characters
        .code_units_iter()
        .nth(piece.character_start + index)
    })
  }

  /// Derives all completed table rows in this document part from PAPX table
  /// depth, cell marks, and TTP marks. Strict roots reject malformed grammar;
  /// compatible roots retain completed rows and report damaged fragments.
  pub fn table_rows(self) -> Result<DocTableRows<'a>> {
    #[derive(Debug)]
    struct ActiveRow {
      start: usize,
      cell_marks: Vec<usize>,
    }

    let paragraphs = self.paragraphs().collect::<Vec<_>>();
    let mut kinds = Vec::with_capacity(paragraphs.len());
    let mut diagnostics = Vec::new();
    for paragraph in &paragraphs {
      let (kind, diagnostic) = self.classify_table_paragraph(*paragraph)?;
      if let Some(reason) = diagnostic {
        diagnostics.push(DocTableDiagnostic {
          global_cp_range: paragraph.global_cp_range,
          table_depth: kind.table_depth(),
          reason,
        });
      }
      kinds.push(kind);
    }

    let mut active = BTreeMap::<u32, ActiveRow>::new();
    let mut rows = Vec::new();
    for (index, (&paragraph, &kind)) in paragraphs.iter().zip(&kinds).enumerate() {
      let depth = kind.table_depth().unwrap_or(0);
      let abandoned = active
        .keys()
        .copied()
        .filter(|active_depth| *active_depth > depth)
        .collect::<Vec<_>>();
      for abandoned_depth in abandoned {
        let abandoned_row = active.remove(&abandoned_depth).expect("key was collected");
        let range = paragraph_range(&paragraphs, abandoned_row.start, index.saturating_sub(1));
        self.table_problem(
          &mut diagnostics,
          range,
          Some(abandoned_depth),
          "table row ends without a TTP mark before table depth decreases",
        )?;
      }

      if depth == 0 {
        continue;
      }
      if depth > 1 && !active.contains_key(&(depth - 1)) {
        self.table_problem(
          &mut diagnostics,
          paragraph.global_cp_range,
          Some(depth),
          "nested table row has no active containing row at depth N-1",
        )?;
      }
      let row = active.entry(depth).or_insert_with(|| ActiveRow {
        start: index,
        cell_marks: Vec::new(),
      });
      match kind {
        DocParagraphKind::CellMark { .. } => row.cell_marks.push(index),
        DocParagraphKind::TableTerminatingParagraph { .. } => {
          let row = active.remove(&depth).expect("current depth was inserted");
          let global_cp_range = paragraph_range(&paragraphs, row.start, index);
          let local_cp_range = local_range(self, global_cp_range)?;
          let final_cell_is_adjacent = row.cell_marks.last().copied() == index.checked_sub(1);
          if !final_cell_is_adjacent {
            self.table_problem(
              &mut diagnostics,
              paragraph.global_cp_range,
              Some(depth),
              "TTP mark is not immediately preceded by a cell mark",
            )?;
          }
          if !(1..=63).contains(&row.cell_marks.len()) {
            self.table_problem(
              &mut diagnostics,
              global_cp_range,
              Some(depth),
              "table row cell count is outside 1..=63",
            )?;
          }
          let defined_cell_count = match paragraph.defined_table_cell_count() {
            Ok(value) => Some(value),
            Err(error) if self.preserve_compatibility => {
              diagnostics.push(DocTableDiagnostic {
                global_cp_range: paragraph.global_cp_range,
                table_depth: Some(depth),
                reason: error.to_string(),
              });
              None
            }
            Err(error) => return Err(error),
          };
          if defined_cell_count.is_some_and(|count| count != row.cell_marks.len()) {
            self.table_problem(
              &mut diagnostics,
              global_cp_range,
              Some(depth),
              "row-mark cell definitions do not match its cell marks",
            )?;
          }
          rows.push(DocTableRowRef {
            document_part: self,
            table_depth: depth,
            global_cp_range,
            local_cp_range,
            terminating_paragraph: paragraph,
            cell_count: row.cell_marks.len(),
            defined_cell_count,
          });
        }
        DocParagraphKind::NonTable | DocParagraphKind::TableParagraph { .. } => {}
      }
    }

    for (depth, row) in active {
      let range = paragraph_range(&paragraphs, row.start, paragraphs.len().saturating_sub(1));
      self.table_problem(
        &mut diagnostics,
        range,
        Some(depth),
        "table row reaches the end of its document part without a TTP mark",
      )?;
    }
    rows.sort_by_key(|row| (row.global_cp_range.start.0, row.table_depth));
    Ok(DocTableRows { rows, diagnostics })
  }

  /// Groups completed rows into tables using the complete row-identity list
  /// in MS-DOC 2.4.3, including paragraph frame properties for inline rows
  /// and paragraph-level structured-tag bookmark boundaries.
  pub fn tables(self) -> Result<DocTables<'a>> {
    let row_index = self.table_rows()?;
    let mut diagnostics = row_index.diagnostics;
    let mut rows_by_depth = BTreeMap::<u32, Vec<DocTableRowRef<'a>>>::new();
    for row in row_index.rows {
      rows_by_depth.entry(row.table_depth).or_default().push(row);
    }
    let mut tables = Vec::new();
    for (depth, rows) in rows_by_depth {
      let mut current = Vec::<DocTableRowRef<'a>>::new();
      for row in rows {
        let continues = if let Some(previous) = current.last().copied() {
          if previous.global_cp_range.end != row.global_cp_range.start {
            false
          } else {
            match rows_share_table(previous, row) {
              Ok(false) => false,
              Ok(true) => match self.structured_tag_splits_rows(previous, row) {
                Ok(value) => !value,
                Err(error) if self.preserve_compatibility => {
                  diagnostics.push(DocTableDiagnostic {
                    global_cp_range: DocCpRange {
                      start: previous.global_cp_range.start,
                      end: row.global_cp_range.end,
                    },
                    table_depth: Some(depth),
                    reason: error.to_string(),
                  });
                  false
                }
                Err(error) => return Err(error),
              },
              Err(error) if self.preserve_compatibility => {
                diagnostics.push(DocTableDiagnostic {
                  global_cp_range: DocCpRange {
                    start: previous.global_cp_range.start,
                    end: row.global_cp_range.end,
                  },
                  table_depth: Some(depth),
                  reason: error.to_string(),
                });
                false
              }
              Err(error) => return Err(error),
            }
          }
        } else {
          false
        };
        if !continues && !current.is_empty() {
          tables.push(table_from_rows(self, depth, std::mem::take(&mut current))?);
        }
        current.push(row);
      }
      if !current.is_empty() {
        tables.push(table_from_rows(self, depth, current)?);
      }
    }
    tables.sort_by_key(|table| (table.global_cp_range.start.0, table.table_depth));
    Ok(DocTables {
      document_part: self,
      tables,
      diagnostics,
    })
  }

  fn structured_tag_splits_rows(
    self,
    first: DocTableRowRef<'a>,
    second: DocTableRowRef<'a>,
  ) -> Result<bool> {
    if self.part != FieldDocumentPart::Main {
      return Ok(false);
    }
    let Some(bookmarks) = &self.file.table.structured_tag_bookmarks else {
      return Ok(false);
    };
    let first_cells = first.cells()?;
    let second_cells = second.cells()?;
    let cells = first_cells
      .cells
      .iter()
      .chain(&second_cells.cells)
      .copied()
      .collect::<Vec<_>>();
    let value = &bookmarks.value;
    for (index, (tag, start)) in value.tags.iter().zip(&value.starts.bookmarks).enumerate() {
      if tag.tag_type != StructuredTagType::Paragraphs {
        continue;
      }
      let Some(&bookmark_start) = value.starts.positions.get(index) else {
        return Err(Error::invalid(0, "structured-tag start CP is unavailable"));
      };
      let bookmark_end = *value
        .ends
        .positions
        .get(usize::from(start.end_index))
        .ok_or_else(|| Error::invalid(0, "structured-tag end index is outside PlcfBklSdt"))?;
      if bookmark_start >= bookmark_end {
        continue;
      }
      let touched_cells = cells
        .iter()
        .filter(|cell| {
          cell.global_cp_range.start.0 < bookmark_end && bookmark_start < cell.global_cp_range.end.0
        })
        .count();
      let contains_both_rows = bookmark_start <= first.global_cp_range.start.0
        && second.global_cp_range.end.0 <= bookmark_end;
      if touched_cells > 1 && !contains_both_rows {
        return Ok(true);
      }
    }
    Ok(false)
  }

  fn classify_table_paragraph(
    self,
    paragraph: DocParagraphRef<'a>,
  ) -> Result<(DocParagraphKind, Option<String>)> {
    let state = match paragraph.direct_table_state() {
      Ok(value) => value,
      Err(error) if self.preserve_compatibility => {
        return Ok((DocParagraphKind::NonTable, Some(error.to_string())));
      }
      Err(error) => return Err(error),
    };
    let terminal = match paragraph.terminal_character() {
      Ok(value) => value,
      Err(error) if self.preserve_compatibility => {
        return Ok((DocParagraphKind::NonTable, Some(error.to_string())));
      }
      Err(error) => return Err(error),
    };
    match classify_table_paragraph(state, terminal) {
      Ok(kind) => Ok((kind, None)),
      Err(error) if self.preserve_compatibility => Ok((
        infer_compatible_table_paragraph(state, terminal),
        Some(error.to_string()),
      )),
      Err(error) => Err(error),
    }
  }

  fn table_problem(
    self,
    diagnostics: &mut Vec<DocTableDiagnostic>,
    global_cp_range: DocCpRange,
    table_depth: Option<u32>,
    reason: &str,
  ) -> Result<()> {
    if !self.preserve_compatibility {
      return Err(Error::invalid(u64::from(global_cp_range.start.0), reason));
    }
    diagnostics.push(DocTableDiagnostic {
      global_cp_range,
      table_depth,
      reason: reason.to_owned(),
    });
    Ok(())
  }

  pub fn fields(self) -> impl Iterator<Item = DocFieldRef<'a>> {
    self
      .file
      .table
      .fields
      .get(&self.part)
      .into_iter()
      .flat_map(|table| table.value.fields.iter())
      .map(move |source| DocFieldRef {
        document_part: self,
        source,
      })
  }

  /// Resolves inline pictures, NilPICF binary payloads and ObjectPool OLE
  /// objects from their special character and direct-character-formatting
  /// relationship. Every returned target remains borrowed from Data or
  /// ObjectPool; external image/OLE payload internals are not re-parsed.
  pub fn special_content_at(self, local_cp: DocCp) -> Result<Option<DocSpecialContentRef<'a>>> {
    let character = self.character_at(local_cp).ok_or_else(|| {
      Error::invalid(
        u64::from(local_cp.0),
        "special-content CP exceeds its MS-DOC document part",
      )
    })?;
    if !matches!(character, 0x0001 | 0x0014) {
      return Ok(None);
    }
    match self.resolve_special_content_at(local_cp, character)? {
      Some(DocSpecialContentLink::Resolved(value)) => Ok(Some(value)),
      Some(DocSpecialContentLink::CompatibilityOleObject { character, .. }) => Err(Error::invalid(
        u64::from(character.0),
        "ObjectPool storage has no valid ObjInfo/ODT",
      )),
      Some(DocSpecialContentLink::Unresolved { character, reason }) => {
        Err(Error::invalid(u64::from(character.0), reason))
      }
      None => Ok(None),
    }
  }

  /// Resolves all strict special-content relationships in this document
  /// part. Use [`Self::special_content_at`] when a streaming consumer already
  /// knows the CP of a special character and wants to avoid the result Vec.
  pub fn special_contents(self) -> Result<Vec<DocSpecialContentRef<'a>>> {
    self
      .special_contents_compatible()?
      .into_iter()
      .map(|link| match link {
        DocSpecialContentLink::Resolved(value) => Ok(value),
        DocSpecialContentLink::CompatibilityOleObject { character, .. } => Err(Error::invalid(
          u64::from(character.0),
          "ObjectPool storage has no valid ObjInfo/ODT",
        )),
        DocSpecialContentLink::Unresolved { character, reason } => {
          Err(Error::invalid(u64::from(character.0), reason))
        }
      })
      .collect()
  }

  pub fn special_contents_compatible(self) -> Result<Vec<DocSpecialContentLink<'a>>> {
    let mut links = Vec::new();
    for piece in self.text_pieces() {
      let piece_start = piece.local_cp_range.start.0;
      for (index, character) in piece
        .source
        .value
        .characters
        .code_units_iter()
        .skip(piece.character_start)
        .take(piece.character_end - piece.character_start)
        .enumerate()
      {
        if !matches!(character, 0x0001 | 0x0014) {
          continue;
        }
        let local_cp = piece_start
          .checked_add(
            u32::try_from(index)
              .map_err(|_| Error::Limit("DOC special-character index exceeds u32".into()))?,
          )
          .ok_or_else(|| Error::Limit("DOC special-character CP overflow".into()))?;
        self.push_special_content_link(DocCp(local_cp), character, &mut links);
      }
    }
    Ok(links)
  }

  fn push_special_content_link(
    self,
    local_cp: DocCp,
    character: u16,
    links: &mut Vec<DocSpecialContentLink<'a>>,
  ) {
    match self.resolve_special_content_at(local_cp, character) {
      Ok(Some(value)) => links.push(value),
      Ok(None) => {}
      Err(error) => links.push(DocSpecialContentLink::Unresolved {
        character: local_cp,
        reason: error.to_string(),
      }),
    }
  }

  fn resolve_special_content_at(
    self,
    local_cp: DocCp,
    character: u16,
  ) -> Result<Option<DocSpecialContentLink<'a>>> {
    let formatting_ref = self.formatting_at(local_cp)?;
    let location_property = direct_picture_location_property(formatting_ref)?;
    let Some(location_property) = location_property else {
      return Ok(None);
    };
    let SprmOperand::Dword(raw_location) = location_property.operand else {
      return Err(Error::invalid(
        u64::from(local_cp.0),
        "sprmCPicLocation operand is not a dword",
      ));
    };
    let location = u32::from_le_bytes(raw_location);
    if !effective_character_toggle_from_ref(self.file, formatting_ref, KnownSprm::CFSpec)? {
      return Err(Error::invalid(
        u64::from(local_cp.0),
        "sprmCPicLocation character does not have effective sprmCFSpec",
      ));
    }
    match character {
      0x0001 => {
        let binary =
          effective_character_toggle_from_ref(self.file, formatting_ref, KnownSprm::CFData)?;
        let data_node = self
          .file
          .data
          .as_ref()
          .and_then(|data| data.nodes.iter().find(|node| node.offset == location))
          .ok_or_else(|| {
            Error::invalid(
              u64::from(local_cp.0),
              format!("sprmCPicLocation {location} has no Data node"),
            )
          })?;
        if binary && !matches!(data_node.value, DocDataNodeValue::Binary(_)) {
          return Err(Error::invalid(
            u64::from(local_cp.0),
            "sprmCFData selects a non-NilPICF Data node",
          ));
        }
        if !binary && !matches!(data_node.value, DocDataNodeValue::Picture(_)) {
          return Err(Error::invalid(
            u64::from(local_cp.0),
            "picture character selects a non-picture Data node",
          ));
        }
        Ok(Some(DocSpecialContentLink::Resolved(if binary {
          DocSpecialContentRef::Binary {
            character: local_cp,
            location_property,
            data_node,
          }
        } else {
          DocSpecialContentRef::Picture {
            character: local_cp,
            location_property,
            data_node,
          }
        })))
      }
      0x0014 => {
        let ole2 =
          effective_character_toggle_from_ref(self.file, formatting_ref, KnownSprm::CFOle2)?;
        if !ole2 {
          return Ok(None);
        }
        if !effective_character_toggle_from_ref(self.file, formatting_ref, KnownSprm::CFObj)? {
          return Err(Error::invalid(
            u64::from(local_cp.0),
            "sprmCFOle2 is true without effective sprmCFObj",
          ));
        }
        let field = self
          .fields()
          .find_map(|field| find_field_separator(field, local_cp.0))
          .ok_or_else(|| {
            Error::invalid(
              u64::from(local_cp.0),
              "OLE separator has no containing EMBED/LINK/CONTROL field",
            )
          })?;
        if field
          .source
          .end
          .flags
          .contains(super::FieldEndFlags::ZOMBIE_EMBED)
        {
          return Ok(None);
        }
        if !matches!(field.source.begin.field_type, 0x38 | 0x3a | 0x57) {
          return Err(Error::invalid(
            u64::from(local_cp.0),
            "OLE separator belongs to a field other than LINK, EMBED or CONTROL",
          ));
        }
        let expected_name = format!("_{location}");
        let pool = self.file.object_pool.as_ref().ok_or_else(|| {
          Error::invalid(
            u64::from(local_cp.0),
            format!("sprmCPicLocation has no ObjectPool/{expected_name} storage"),
          )
        })?;
        if let Some(object) = pool.objects.iter().find(|object| {
          object
            .path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(&expected_name))
        }) {
          return Ok(Some(DocSpecialContentLink::Resolved(
            DocSpecialContentRef::OleObject {
              character: local_cp,
              location_property,
              field,
              object,
            },
          )));
        }
        if let Some(storage) = pool.compatibility_objects.iter().find(|object| {
          object
            .path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(&expected_name))
        }) {
          return Ok(Some(DocSpecialContentLink::CompatibilityOleObject {
            character: local_cp,
            location_property,
            field,
            storage,
          }));
        }
        Err(Error::invalid(
          u64::from(local_cp.0),
          format!("sprmCPicLocation has no ObjectPool/{expected_name} storage"),
        ))
      }
      _ => Ok(None),
    }
  }

  pub fn paragraph_at(self, local_cp: DocCp) -> Option<DocParagraphRef<'a>> {
    self
      .paragraphs()
      .find(|paragraph| paragraph.local_cp_range.contains(local_cp))
  }

  pub fn character_run_at(self, local_cp: DocCp) -> Option<DocCharacterRunRef<'a>> {
    self
      .character_runs()
      .find(|run| run.local_cp_range.contains(local_cp))
  }

  pub fn formatting_at(self, local_cp: DocCp) -> Result<DocDirectFormattingRef<'a>> {
    if !self.local_cp_range().contains(local_cp) {
      return Err(Error::invalid(
        u64::from(local_cp.0),
        "formatting CP exceeds its MS-DOC document part",
      ));
    }
    let global_cp = DocCp(
      self
        .global_cp_range
        .start
        .0
        .checked_add(local_cp.0)
        .ok_or_else(|| Error::Limit("DOC aggregate CP overflow".into()))?,
    );
    let text_piece = self
      .text_pieces()
      .find(|piece| piece.global_cp_range.contains(global_cp))
      .ok_or_else(|| {
        Error::invalid(
          u64::from(global_cp.0),
          "CP has no containing PlcPcd text piece",
        )
      })?;
    if text_piece.descriptor.is_none() {
      return Err(Error::invalid(
        u64::from(global_cp.0),
        "text piece has no Pcd descriptor",
      ));
    }
    let paragraph = self
      .file
      .word_document
      .papx_runs
      .as_deref()
      .and_then(|runs| {
        runs
          .iter()
          .find(|run| run.cp_start <= global_cp.0 && global_cp.0 < run.cp_end)
      })
      .ok_or_else(|| Error::invalid(u64::from(global_cp.0), "CP has no containing PAPX run"))?;
    let character_run = self
      .file
      .word_document
      .chpx_runs
      .as_deref()
      .and_then(|runs| {
        runs
          .iter()
          .find(|run| run.cp_start <= global_cp.0 && global_cp.0 < run.cp_end)
      })
      .ok_or_else(|| Error::invalid(u64::from(global_cp.0), "CP has no containing CHPX run"))?;
    Ok(DocDirectFormattingRef {
      document_part: self,
      local_cp,
      global_cp,
      text_piece,
      paragraph,
      character_run,
    })
  }
}

impl<'a> DocTextPieceRef<'a> {
  pub const fn document_part(self) -> DocDocumentPartRef<'a> {
    self.document_part
  }

  pub const fn source(self) -> &'a DocTextPiece {
    self.source
  }

  pub const fn descriptor(self) -> Option<&'a Pcd> {
    self.descriptor
  }

  /// Resolves the containing Pcd.Prm as a borrowed/inline property view.
  pub fn property_modifications(self) -> Result<PrmPropertiesRef<'a>> {
    self
      .descriptor
      .ok_or_else(|| Error::invalid(0, "text piece has no Pcd descriptor"))?
      .property_modifier
      .property_modifications_ref(&self.document_part.file.table.clx.value)
  }

  pub const fn global_cp_range(self) -> DocCpRange {
    self.global_cp_range
  }

  pub const fn local_cp_range(self) -> DocCpRange {
    self.local_cp_range
  }

  pub const fn fc_range(self) -> DocFcRange {
    self.fc_range
  }

  /// Returns the zero-copy intersection with one part-local CP range.
  ///
  /// The returned handle still borrows the same PlcPcd owner and decoded
  /// string. Only its CP/FC bounds and string-range indexes are narrowed, so
  /// consumers do not need to reimplement UTF-16 code-unit accounting when
  /// a paragraph, field, table cell, or other logical range cuts through a
  /// physical text piece.
  pub fn intersection(self, local_cp_range: DocCpRange) -> Option<Self> {
    let start = self.local_cp_range.start.0.max(local_cp_range.start.0);
    let end = self.local_cp_range.end.0.min(local_cp_range.end.0);
    if start >= end {
      return None;
    }

    let start_delta = start.checked_sub(self.local_cp_range.start.0)?;
    let end_delta = end.checked_sub(self.local_cp_range.start.0)?;
    let character_start = self
      .character_start
      .checked_add(usize::try_from(start_delta).ok()?)?;
    let character_end = self
      .character_start
      .checked_add(usize::try_from(end_delta).ok()?)?;
    let byte_width = match self.source.value.characters.encoding() {
      TextPieceEncoding::Compressed => 1,
      TextPieceEncoding::Utf16 => 2,
    };
    let fc_start = self
      .fc_range
      .start
      .0
      .checked_add(start_delta.checked_mul(byte_width)?)?;
    let fc_end = self
      .fc_range
      .start
      .0
      .checked_add(end_delta.checked_mul(byte_width)?)?;
    let global_part_start = self
      .global_cp_range
      .start
      .0
      .checked_sub(self.local_cp_range.start.0)?;

    Some(Self {
      global_cp_range: DocCpRange {
        start: DocCp(global_part_start.checked_add(start)?),
        end: DocCp(global_part_start.checked_add(end)?),
      },
      local_cp_range: DocCpRange {
        start: DocCp(start),
        end: DocCp(end),
      },
      fc_range: DocFcRange {
        start: DocFc(fc_start),
        end: DocFc(fc_end),
      },
      character_start,
      character_end,
      ..self
    })
  }

  /// Returns the conforming Rust string slice for this CP intersection, or
  /// the exact invalid UTF-16 units retained by compatible parsing.
  pub fn value(self) -> Result<DocTextPieceValueRef<'a>> {
    match &self.source.value.characters {
      TextPieceCharacters::String(value) => Ok(DocTextPieceValueRef::String {
        value: self
          .source
          .value
          .characters
          .string_range(self.character_start..self.character_end)?
          .expect("conforming DOC String variant has a string range"),
        encoding: value.encoding,
      }),
      TextPieceCharacters::CompatibilityUtf16 { code_units } => {
        Ok(DocTextPieceValueRef::CompatibilityUtf16(
          &code_units[self.character_start..self.character_end],
        ))
      }
    }
  }
}

impl<'a> DocParagraphRef<'a> {
  pub const fn document_part(self) -> DocDocumentPartRef<'a> {
    self.document_part
  }

  pub const fn source(self) -> &'a DocPapxRun {
    self.source
  }

  pub const fn global_cp_range(self) -> DocCpRange {
    self.global_cp_range
  }

  pub const fn local_cp_range(self) -> DocCpRange {
    self.local_cp_range
  }

  /// Returns this paragraph as the existing CP-range relationship view, so
  /// its pieces, character runs, fields, special content, and tables can be
  /// traversed without materializing a second content node.
  pub const fn range(self) -> DocTextRangeRef<'a> {
    DocTextRangeRef {
      document_part: self.document_part,
      local_cp_range: self.local_cp_range,
      global_cp_range: self.global_cp_range,
    }
  }

  pub fn text_pieces(self) -> impl Iterator<Item = DocTextPieceRef<'a>> {
    self.document_part.text_pieces().filter(move |piece| {
      piece.global_cp_range.start.0 < self.global_cp_range.end.0
        && self.global_cp_range.start.0 < piece.global_cp_range.end.0
    })
  }

  /// Returns physical text-piece handles clipped to this paragraph's exact
  /// part-local CP range without allocating or copying decoded text.
  pub fn text_segments(self) -> impl Iterator<Item = DocTextPieceRef<'a>> {
    self
      .document_part
      .text_pieces()
      .filter_map(move |piece| piece.intersection(self.local_cp_range))
  }

  pub fn character_runs(self) -> impl Iterator<Item = DocCharacterRunRef<'a>> {
    self.document_part.character_runs().filter(move |run| {
      run.global_cp_range.start.0 < self.global_cp_range.end.0
        && self.global_cp_range.start.0 < run.global_cp_range.end.0
    })
  }

  /// Intersects text pieces and CHPX runs in one forward pass.
  ///
  /// Each yielded slice has one physical text owner and one physical
  /// character-formatting owner. No text, property array, or index is
  /// allocated. A malformed coverage gap is returned as an error instead of
  /// silently dropping the uncovered characters.
  pub fn formatted_text_segments(self) -> impl Iterator<Item = Result<DocFormattedTextRef<'a>>> {
    let mut pieces = self.text_segments().peekable();
    let mut runs = self.character_runs().peekable();
    let mut cursor = self.local_cp_range.start.0;
    let end = self.local_cp_range.end.0;
    std::iter::from_fn(move || {
      if cursor >= end {
        return None;
      }
      while pieces
        .peek()
        .is_some_and(|piece| piece.local_cp_range.end.0 <= cursor)
      {
        pieces.next();
      }
      while runs
        .peek()
        .is_some_and(|run| run.local_cp_range.end.0 <= cursor)
      {
        runs.next();
      }
      let Some(piece) = pieces.peek().copied() else {
        let missing_cp = cursor;
        cursor = end;
        return Some(Err(Error::invalid(
          u64::from(missing_cp),
          "paragraph CP has no containing PlcPcd text piece",
        )));
      };
      let Some(character_run) = runs.peek().copied() else {
        let missing_cp = cursor;
        cursor = end;
        return Some(Err(Error::invalid(
          u64::from(missing_cp),
          "paragraph CP has no containing CHPX run",
        )));
      };
      if piece.local_cp_range.start.0 > cursor {
        let missing_cp = cursor;
        cursor = end;
        return Some(Err(Error::invalid(
          u64::from(missing_cp),
          "paragraph text-piece coverage has a CP gap",
        )));
      }
      if character_run.local_cp_range.start.0 > cursor {
        let missing_cp = cursor;
        cursor = end;
        return Some(Err(Error::invalid(
          u64::from(missing_cp),
          "paragraph CHPX coverage has a CP gap",
        )));
      }
      let segment_end = piece
        .local_cp_range
        .end
        .0
        .min(character_run.local_cp_range.end.0)
        .min(end);
      let range = DocCpRange {
        start: DocCp(cursor),
        end: DocCp(segment_end),
      };
      let text = piece
        .intersection(range)
        .expect("the cursor is inside the current text piece");
      cursor = segment_end;
      Some(Ok(DocFormattedTextRef {
        text,
        character_run,
      }))
    })
  }

  /// Resolves the paragraph style selected by the PAPX and Pcd.Prm layers.
  /// This does not infer headings from a style name or visual formatting.
  pub fn style(self) -> Result<DocParagraphStyleRef<'a>> {
    let direct = self
      .document_part
      .file
      .direct_paragraph_formatting_at_cp(self.document_part.part, self.local_cp_range.start.0)?;
    self.style_from_direct(&direct)
  }

  fn style_from_direct(
    self,
    direct: &DocDirectParagraphFormatting,
  ) -> Result<DocParagraphStyleRef<'a>> {
    let style_index = effective_paragraph_style_index(direct)?;
    let source = self
      .document_part
      .file
      .table
      .styles
      .as_ref()
      .and_then(|styles| styles.value.styles.get(usize::from(style_index)))
      .and_then(|style| style.definition.as_ref())
      .ok_or_else(|| {
        Error::invalid(
          u64::from(style_index),
          "paragraph references an unavailable STSH style",
        )
      })?;
    if source.base.style_kind != super::StyleKind::Paragraph {
      return Err(Error::invalid(
        u64::from(style_index),
        "paragraph references a non-paragraph STSH style",
      ));
    }
    Ok(DocParagraphStyleRef {
      document_part: self.document_part,
      style_index,
      source,
    })
  }

  /// Resolves style identity and outline semantics while expanding the
  /// direct paragraph SPRM layer only once.
  pub fn style_state(self) -> Result<DocParagraphStyleStateRef<'a>> {
    let direct = self
      .document_part
      .file
      .direct_paragraph_formatting_at_cp(self.document_part.part, self.local_cp_range.start.0)?;
    let style = self.style_from_direct(&direct)?;
    let properties = style.properties()?;
    let mut level = 9;
    apply_outline_properties(&properties.paragraph_properties, &mut level)?;
    apply_outline_properties(&direct.applied_properties, &mut level)?;
    Ok(DocParagraphStyleStateRef {
      style,
      outline_level: DocOutlineLevel::from_raw(level)?,
    })
  }

  /// Computes the paragraph's normative outline level from its selected
  /// style hierarchy followed by direct paragraph properties.
  pub fn outline_level(self) -> Result<DocOutlineLevel> {
    let direct = self
      .document_part
      .file
      .direct_paragraph_formatting_at_cp(self.document_part.part, self.local_cp_range.start.0)?;
    let style_index = effective_paragraph_style_index(&direct)?;
    let style = self.document_part.file.style_properties(style_index)?;
    if style.style_kind != super::StyleKind::Paragraph {
      return Err(Error::invalid(
        u64::from(style_index),
        "paragraph references a non-paragraph STSH style",
      ));
    }
    let mut level = 9;
    apply_outline_properties(&style.paragraph_properties, &mut level)?;
    apply_outline_properties(&direct.applied_properties, &mut level)?;
    DocOutlineLevel::from_raw(level)
  }

  pub fn formatting_at_start(self) -> Result<DocDirectFormattingRef<'a>> {
    self.document_part.formatting_at(self.local_cp_range.start)
  }

  /// Returns the final character that defines this paragraph boundary.
  pub fn terminal_character(self) -> Result<u16> {
    let local_cp = self
      .local_cp_range
      .end
      .0
      .checked_sub(1)
      .map(DocCp)
      .ok_or_else(|| Error::invalid(0, "DOC paragraph has an empty CP range"))?;
    self.document_part.character_at(local_cp).ok_or_else(|| {
      Error::invalid(
        u64::from(local_cp.0),
        "DOC paragraph terminator has no text-piece character",
      )
    })
  }

  /// Resolves direct formatting at the paragraph mark. Table membership and
  /// row/cell marker properties are defined at this position, not at the
  /// first character of the paragraph.
  pub fn formatting_at_mark(self) -> Result<DocDirectFormattingRef<'a>> {
    let local_cp = self
      .local_cp_range
      .end
      .0
      .checked_sub(1)
      .map(DocCp)
      .ok_or_else(|| Error::invalid(0, "DOC paragraph has an empty CP range"))?;
    self.document_part.formatting_at(local_cp)
  }

  pub fn direct_table_state(self) -> Result<DocDirectTableState> {
    let local_cp = self
      .local_cp_range
      .end
      .0
      .checked_sub(1)
      .ok_or_else(|| Error::invalid(0, "DOC paragraph has an empty CP range"))?;
    self
      .document_part
      .file
      .direct_paragraph_formatting_at_cp(self.document_part.part, local_cp)?
      .table_state()
  }

  fn defined_table_cell_count(self) -> Result<usize> {
    let local_cp = self
      .local_cp_range
      .end
      .0
      .checked_sub(1)
      .ok_or_else(|| Error::invalid(0, "DOC paragraph has an empty CP range"))?;
    let formatting = self
      .document_part
      .file
      .direct_paragraph_formatting_at_cp(self.document_part.part, local_cp)?;
    table_cell_definition_count(&formatting.applied_properties)
  }

  pub fn kind(self) -> Result<DocParagraphKind> {
    classify_table_paragraph(self.direct_table_state()?, self.terminal_character()?)
  }
}

impl<'a> DocParagraphStyleRef<'a> {
  pub const fn document_part(self) -> DocDocumentPartRef<'a> {
    self.document_part
  }

  pub const fn style_index(self) -> u16 {
    self.style_index
  }

  pub const fn source(self) -> &'a super::StyleDefinition {
    self.source
  }

  pub fn properties(self) -> Result<DocStyleProperties> {
    self.document_part.file.style_properties(self.style_index)
  }
}

impl<'a> DocParagraphStyleStateRef<'a> {
  pub const fn style(self) -> DocParagraphStyleRef<'a> {
    self.style
  }

  pub const fn outline_level(self) -> DocOutlineLevel {
    self.outline_level
  }
}

impl DocOutlineLevel {
  pub const fn raw(self) -> u8 {
    match self {
      Self::Level1 => 0,
      Self::Level2 => 1,
      Self::Level3 => 2,
      Self::Level4 => 3,
      Self::Level5 => 4,
      Self::Level6 => 5,
      Self::Level7 => 6,
      Self::Level8 => 7,
      Self::Level9 => 8,
      Self::BodyText => 9,
    }
  }

  fn from_raw(value: u8) -> Result<Self> {
    match value {
      0 => Ok(Self::Level1),
      1 => Ok(Self::Level2),
      2 => Ok(Self::Level3),
      3 => Ok(Self::Level4),
      4 => Ok(Self::Level5),
      5 => Ok(Self::Level6),
      6 => Ok(Self::Level7),
      7 => Ok(Self::Level8),
      8 => Ok(Self::Level9),
      9 => Ok(Self::BodyText),
      _ => Err(Error::invalid(
        u64::from(value),
        "paragraph outline level exceeds 0x09",
      )),
    }
  }
}

impl DocParagraphKind {
  pub const fn table_depth(self) -> Option<u32> {
    match self {
      Self::NonTable => None,
      Self::TableParagraph { table_depth }
      | Self::CellMark { table_depth }
      | Self::TableTerminatingParagraph { table_depth } => Some(table_depth),
    }
  }
}

impl DocSpecialContentRef<'_> {
  pub const fn character(self) -> DocCp {
    match self {
      Self::Picture { character, .. }
      | Self::Binary { character, .. }
      | Self::OleObject { character, .. } => character,
    }
  }
}

impl<'a> DocTableRows<'a> {
  pub fn rows(&self) -> &[DocTableRowRef<'a>] {
    &self.rows
  }

  pub fn diagnostics(&self) -> &[DocTableDiagnostic] {
    &self.diagnostics
  }
}

impl<'a> DocTableRowRef<'a> {
  pub const fn document_part(self) -> DocDocumentPartRef<'a> {
    self.document_part
  }

  pub const fn table_depth(self) -> u32 {
    self.table_depth
  }

  pub const fn global_cp_range(self) -> DocCpRange {
    self.global_cp_range
  }

  pub const fn local_cp_range(self) -> DocCpRange {
    self.local_cp_range
  }

  pub const fn terminating_paragraph(self) -> DocParagraphRef<'a> {
    self.terminating_paragraph
  }

  pub const fn cell_count(self) -> usize {
    self.cell_count
  }

  pub const fn defined_cell_count(self) -> Option<usize> {
    self.defined_cell_count
  }

  pub fn paragraphs(self) -> impl Iterator<Item = DocParagraphRef<'a>> {
    self.document_part.paragraphs().filter(move |paragraph| {
      paragraph.global_cp_range.start.0 < self.global_cp_range.end.0
        && self.global_cp_range.start.0 < paragraph.global_cp_range.end.0
    })
  }

  pub fn cells(self) -> Result<DocTableCells<'a>> {
    let mut cells = Vec::with_capacity(self.cell_count);
    let mut diagnostics = Vec::new();
    let mut cell_start = self.global_cp_range.start;
    for paragraph in self.paragraphs() {
      let (kind, diagnostic) = self.document_part.classify_table_paragraph(paragraph)?;
      if let Some(reason) = diagnostic {
        diagnostics.push(DocTableDiagnostic {
          global_cp_range: paragraph.global_cp_range,
          table_depth: kind.table_depth(),
          reason,
        });
      }
      if kind
        != (DocParagraphKind::CellMark {
          table_depth: self.table_depth,
        })
      {
        continue;
      }
      let global_cp_range = DocCpRange {
        start: cell_start,
        end: paragraph.global_cp_range.end,
      };
      cells.push(DocTableCellRef {
        row: self,
        cell_index: cells.len(),
        global_cp_range,
        local_cp_range: local_range(self.document_part, global_cp_range)?,
        cell_mark: paragraph,
      });
      cell_start = paragraph.global_cp_range.end;
    }
    if cells.len() != self.cell_count {
      self.document_part.table_problem(
        &mut diagnostics,
        self.global_cp_range,
        Some(self.table_depth),
        "derived table-cell count changed while traversing its row",
      )?;
    }
    Ok(DocTableCells { cells, diagnostics })
  }
}

impl<'a> DocTableCells<'a> {
  pub fn cells(&self) -> &[DocTableCellRef<'a>] {
    &self.cells
  }

  pub fn diagnostics(&self) -> &[DocTableDiagnostic] {
    &self.diagnostics
  }
}

impl<'a> DocTableCellRef<'a> {
  pub const fn row(self) -> DocTableRowRef<'a> {
    self.row
  }

  pub const fn cell_index(self) -> usize {
    self.cell_index
  }

  pub const fn global_cp_range(self) -> DocCpRange {
    self.global_cp_range
  }

  pub const fn local_cp_range(self) -> DocCpRange {
    self.local_cp_range
  }

  pub const fn cell_mark(self) -> DocParagraphRef<'a> {
    self.cell_mark
  }

  pub fn paragraphs(self) -> impl Iterator<Item = DocParagraphRef<'a>> {
    self
      .row
      .document_part
      .paragraphs()
      .filter(move |paragraph| {
        paragraph.global_cp_range.start.0 < self.global_cp_range.end.0
          && self.global_cp_range.start.0 < paragraph.global_cp_range.end.0
      })
  }

  pub fn nested_rows(self) -> Result<DocTableRows<'a>> {
    let expected_depth = self
      .row
      .table_depth
      .checked_add(1)
      .ok_or_else(|| Error::Limit("DOC nested table depth overflow".into()))?;
    let index = self.row.document_part.table_rows()?;
    Ok(DocTableRows {
      rows: index
        .rows
        .into_iter()
        .filter(|row| {
          row.table_depth == expected_depth
            && self.global_cp_range.start.0 <= row.global_cp_range.start.0
            && row.global_cp_range.end.0 <= self.global_cp_range.end.0
        })
        .collect(),
      diagnostics: index
        .diagnostics
        .into_iter()
        .filter(|diagnostic| {
          diagnostic.global_cp_range.start.0 < self.global_cp_range.end.0
            && self.global_cp_range.start.0 < diagnostic.global_cp_range.end.0
        })
        .collect(),
    })
  }

  pub fn nested_tables(self) -> Result<DocTables<'a>> {
    let index = self.row.document_part.tables()?;
    self.nested_tables_with_index(&index)
  }

  /// Selects directly nested tables from a caller-retained document-part
  /// table index without rescanning and rematerializing the full PAPX table
  /// graph.
  pub fn nested_tables_with_index(self, index: &DocTables<'a>) -> Result<DocTables<'a>> {
    validate_table_index_owner(self.row.document_part, index)?;
    let expected_depth = self
      .row
      .table_depth
      .checked_add(1)
      .ok_or_else(|| Error::Limit("DOC nested table depth overflow".into()))?;
    Ok(DocTables {
      document_part: index.document_part,
      tables: index
        .tables
        .iter()
        .filter(|table| {
          table.table_depth == expected_depth
            && self.global_cp_range.start.0 <= table.global_cp_range.start.0
            && table.global_cp_range.end.0 <= self.global_cp_range.end.0
        })
        .cloned()
        .collect(),
      diagnostics: index
        .diagnostics
        .iter()
        .filter(|diagnostic| {
          diagnostic.global_cp_range.start.0 < self.global_cp_range.end.0
            && self.global_cp_range.start.0 < diagnostic.global_cp_range.end.0
        })
        .cloned()
        .collect(),
    })
  }

  /// Combines this cell's direct paragraphs and directly nested tables in
  /// document order. Paragraphs owned by a nested table are reachable from
  /// that table instead of being duplicated beside it.
  pub fn blocks(self) -> Result<DocBlocks<'a>> {
    let nested_tables = self.nested_tables()?;
    self.blocks_with_nested_tables(nested_tables)
  }

  /// Combines this cell's paragraphs with nested tables selected from a
  /// caller-retained document-part index.
  pub fn blocks_with_tables(self, tables: &DocTables<'a>) -> Result<DocBlocks<'a>> {
    let nested_tables = self.nested_tables_with_index(tables)?;
    self.blocks_with_nested_tables(nested_tables)
  }

  fn blocks_with_nested_tables(self, nested_tables: DocTables<'a>) -> Result<DocBlocks<'a>> {
    let mut blocks = self
      .paragraphs()
      .filter(|paragraph| {
        !nested_tables.tables.iter().any(|table| {
          table.global_cp_range.start.0 <= paragraph.global_cp_range.start.0
            && paragraph.global_cp_range.end.0 <= table.global_cp_range.end.0
        })
      })
      .map(DocBlockRef::Paragraph)
      .collect::<Vec<_>>();
    blocks.extend(nested_tables.tables.into_iter().map(DocBlockRef::Table));
    blocks.sort_by_key(DocBlockRef::global_cp_start);
    Ok(DocBlocks {
      blocks,
      diagnostics: nested_tables.diagnostics,
    })
  }
}

fn validate_table_index_owner(
  document_part: DocDocumentPartRef<'_>,
  tables: &DocTables<'_>,
) -> Result<()> {
  if tables.document_part.part != document_part.part
    || !std::ptr::eq(tables.document_part.file, document_part.file)
  {
    return Err(Error::invalid(
      0,
      "DOC table index belongs to a different document part",
    ));
  }
  Ok(())
}

impl<'a> DocTables<'a> {
  pub fn tables(&self) -> &[DocTableRef<'a>] {
    &self.tables
  }

  pub fn diagnostics(&self) -> &[DocTableDiagnostic] {
    &self.diagnostics
  }

  /// Resolves the directly nested tables contained by a cell using this
  /// already-built relationship index; no PAPX or raw-record scan occurs.
  pub fn tables_in_cell(
    &self,
    cell: DocTableCellRef<'a>,
  ) -> impl Iterator<Item = &DocTableRef<'a>> {
    let expected_depth = cell.row.table_depth.checked_add(1);
    self.tables.iter().filter(move |table| {
      Some(table.table_depth) == expected_depth
        && cell.global_cp_range.start.0 <= table.global_cp_range.start.0
        && table.global_cp_range.end.0 <= cell.global_cp_range.end.0
    })
  }
}

impl<'a> DocBlocks<'a> {
  pub fn blocks(&self) -> &[DocBlockRef<'a>] {
    &self.blocks
  }

  pub fn diagnostics(&self) -> &[DocTableDiagnostic] {
    &self.diagnostics
  }
}

impl<'a> DocSections<'a> {
  pub fn sections(&self) -> &[DocSectionRef<'a>] {
    &self.sections
  }
}

impl<'a> DocSectionRef<'a> {
  pub const fn document_part(self) -> DocDocumentPartRef<'a> {
    self.document_part
  }

  pub const fn section_index(self) -> usize {
    self.section_index
  }

  pub const fn local_cp_range(self) -> DocCpRange {
    self.local_cp_range
  }

  pub const fn global_cp_range(self) -> DocCpRange {
    self.global_cp_range
  }

  pub const fn source(self) -> &'a super::Sed {
    self.source
  }

  pub const fn properties(self) -> &'a DocSectionProperties {
    self.properties
  }

  pub fn blocks(self) -> Result<DocBlocks<'a>> {
    let mut blocks = self.document_part.blocks()?;
    blocks
      .blocks
      .retain(|block| cp_ranges_overlap(block.local_cp_range(), self.local_cp_range));
    Ok(blocks)
  }
}

impl DocBlockRef<'_> {
  pub const fn global_cp_range(&self) -> DocCpRange {
    match self {
      Self::Paragraph(paragraph) => paragraph.global_cp_range,
      Self::Table(table) => table.global_cp_range,
    }
  }

  pub const fn local_cp_range(&self) -> DocCpRange {
    match self {
      Self::Paragraph(paragraph) => paragraph.local_cp_range,
      Self::Table(table) => table.local_cp_range,
    }
  }

  const fn global_cp_start(&self) -> u32 {
    self.global_cp_range().start.0
  }
}

impl<'a> DocTableRef<'a> {
  pub const fn document_part(&self) -> DocDocumentPartRef<'a> {
    self.document_part
  }

  pub const fn table_depth(&self) -> u32 {
    self.table_depth
  }

  pub const fn global_cp_range(&self) -> DocCpRange {
    self.global_cp_range
  }

  pub const fn local_cp_range(&self) -> DocCpRange {
    self.local_cp_range
  }

  pub fn rows(&self) -> &[DocTableRowRef<'a>] {
    &self.rows
  }
}

fn classify_table_paragraph(state: DocDirectTableState, terminal: u16) -> Result<DocParagraphKind> {
  if !state.in_table {
    if state.depth != 0
      || state.table_terminating_paragraph
      || state.inner_table_cell
      || state.inner_table_terminating_paragraph
    {
      return Err(Error::invalid(
        0,
        "non-table paragraph has table depth or table-marker properties",
      ));
    }
    return Ok(DocParagraphKind::NonTable);
  }
  if state.depth == 0 {
    return Err(Error::invalid(
      0,
      "sprmPFInTable paragraph has table depth zero",
    ));
  }
  if state.depth == 1 {
    if state.inner_table_cell || state.inner_table_terminating_paragraph {
      return Err(Error::invalid(
        0,
        "nested table marker property is present at table depth one",
      ));
    }
    if state.table_terminating_paragraph {
      if terminal != 0x0007 {
        return Err(Error::invalid(
          0,
          "depth-one TTP mark is not Unicode 0x0007",
        ));
      }
      return Ok(DocParagraphKind::TableTerminatingParagraph { table_depth: 1 });
    }
    return Ok(if terminal == 0x0007 {
      DocParagraphKind::CellMark { table_depth: 1 }
    } else {
      DocParagraphKind::TableParagraph { table_depth: 1 }
    });
  }

  if state.table_terminating_paragraph {
    return Err(Error::invalid(
      0,
      "sprmPFTtp is only valid at table depth one",
    ));
  }
  if state.inner_table_terminating_paragraph {
    if terminal != 0x000d {
      return Err(Error::invalid(0, "nested TTP mark is not a paragraph mark"));
    }
    return Ok(DocParagraphKind::TableTerminatingParagraph {
      table_depth: state.depth,
    });
  }
  if state.inner_table_cell {
    if terminal != 0x000d {
      return Err(Error::invalid(
        0,
        "nested cell mark is not a paragraph mark",
      ));
    }
    return Ok(DocParagraphKind::CellMark {
      table_depth: state.depth,
    });
  }
  Ok(DocParagraphKind::TableParagraph {
    table_depth: state.depth,
  })
}

fn table_cell_definition_count(properties: &GrpPrl) -> Result<usize> {
  let mut count = 0usize;
  let mut has_definition = false;
  for property in &properties.properties {
    match property.sprm.kind() {
      SprmKind::Known(KnownSprm::TDefTable) => {
        let SprmOperand::TableDefinition(definition) = &property.operand else {
          return Err(Error::invalid(
            0,
            "sprmTDefTable operand is not a TDefTableOperand",
          ));
        };
        count = definition
          .column_boundaries
          .len()
          .checked_sub(1)
          .ok_or_else(|| Error::invalid(0, "sprmTDefTable defines no boundaries"))?;
        if count > 63 {
          return Err(Error::invalid(0, "sprmTDefTable cell count exceeds 63"));
        }
        has_definition = true;
      }
      SprmKind::Known(KnownSprm::TInsert) => {
        let SprmOperand::Dword(value) = property.operand else {
          return Err(Error::invalid(
            0,
            "sprmTInsert operand is not a TInsertOperand",
          ));
        };
        let first = usize::from(value[0]);
        let inserted = usize::from(value[1]);
        if first > count || inserted == 0 {
          return Err(Error::invalid(0, "sprmTInsert cell range is invalid"));
        }
        count = count
          .checked_add(inserted)
          .ok_or_else(|| Error::Limit("DOC table cell count overflow".into()))?;
        if count > 63 {
          return Err(Error::invalid(0, "sprmTInsert cell count exceeds 63"));
        }
        has_definition = true;
      }
      SprmKind::Known(KnownSprm::TDelete) => {
        let SprmOperand::Word(value) = property.operand else {
          return Err(Error::invalid(
            0,
            "sprmTDelete operand is not an ItcFirstLim",
          ));
        };
        let first = usize::from(value[0]);
        let limit = usize::from(value[1]);
        if first > limit || limit > count {
          return Err(Error::invalid(0, "sprmTDelete cell range is invalid"));
        }
        count -= limit - first;
        if count == 0 {
          return Err(Error::invalid(0, "sprmTDelete removes every table cell"));
        }
      }
      _ => {}
    }
  }
  if !has_definition || count == 0 {
    return Err(Error::invalid(
      0,
      "table row has no sprmTDefTable or sprmTInsert cell definition",
    ));
  }
  Ok(count)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TableRowIdentity {
  paragraph_group: Option<SprmOperand>,
  table_style: Option<SprmOperand>,
  right_to_left: bool,
  position: Vec<Option<SprmOperand>>,
  frame: Vec<Option<SprmOperand>>,
}

impl TableRowIdentity {
  fn has_nondefault_position(&self) -> bool {
    self.position.iter().any(Option::is_some)
  }
}

fn rows_share_table(first: DocTableRowRef<'_>, second: DocTableRowRef<'_>) -> Result<bool> {
  let first = table_row_identity(first)?;
  let second = table_row_identity(second)?;
  if first.paragraph_group != second.paragraph_group
    || first.table_style != second.table_style
    || first.right_to_left != second.right_to_left
    || first.position != second.position
  {
    return Ok(false);
  }
  Ok(
    first.has_nondefault_position()
      || second.has_nondefault_position()
      || first.frame == second.frame,
  )
}

fn table_row_identity(row: DocTableRowRef<'_>) -> Result<TableRowIdentity> {
  let row_properties = paragraph_applied_properties(row.terminating_paragraph)?;
  let first_cell = row
    .cells()?
    .cells
    .into_iter()
    .next()
    .ok_or_else(|| Error::invalid(0, "table row has no first cell"))?;
  let first_paragraph = first_cell
    .paragraphs()
    .next()
    .ok_or_else(|| Error::invalid(0, "table cell has no first paragraph"))?;
  let frame_properties = paragraph_applied_properties(first_paragraph)?;
  let right_to_left = table_direction(&row_properties, KnownSprm::TFBiDi)?
    || table_direction(&row_properties, KnownSprm::TFBiDi90)?;
  Ok(TableRowIdentity {
    paragraph_group: normalized_property(&row_properties, KnownSprm::TIpgp),
    table_style: normalized_property(&row_properties, KnownSprm::TIstd),
    right_to_left,
    position: [
      KnownSprm::TPc,
      KnownSprm::TFNoAllowOverlap,
      KnownSprm::TDxaAbs,
      KnownSprm::TDyaAbs,
      KnownSprm::TDxaFromText,
      KnownSprm::TDyaFromText,
      KnownSprm::TDxaFromTextRight,
      KnownSprm::TDyaFromTextBottom,
    ]
    .into_iter()
    .map(|kind| normalized_property(&row_properties, kind))
    .collect(),
    frame: [
      KnownSprm::PPc,
      KnownSprm::PDxaAbs,
      KnownSprm::PDyaAbs,
      KnownSprm::PDxaWidth,
      KnownSprm::PWHeightAbs,
      KnownSprm::PDcs,
      KnownSprm::PWr,
      KnownSprm::PDxaFromText,
      KnownSprm::PDyaFromText,
      KnownSprm::PFLocked,
      KnownSprm::PFNoAllowOverlap,
      KnownSprm::PFrameTextFlow,
    ]
    .into_iter()
    .map(|kind| normalized_property(&frame_properties, kind))
    .collect(),
  })
}

fn paragraph_applied_properties(paragraph: DocParagraphRef<'_>) -> Result<GrpPrl> {
  let local_cp = paragraph
    .local_cp_range
    .end
    .0
    .checked_sub(1)
    .ok_or_else(|| Error::invalid(0, "DOC paragraph has an empty CP range"))?;
  Ok(
    paragraph
      .document_part
      .file
      .direct_paragraph_formatting_at_cp(paragraph.document_part.part, local_cp)?
      .applied_properties,
  )
}

fn normalized_property(properties: &GrpPrl, kind: KnownSprm) -> Option<SprmOperand> {
  properties
    .properties
    .iter()
    .rev()
    .find(|property| property.sprm.kind() == SprmKind::Known(kind))
    .map(|property| property.operand.clone())
    .filter(|operand| !fixed_operand_is_zero(operand))
}

fn fixed_operand_is_zero(operand: &SprmOperand) -> bool {
  match operand {
    SprmOperand::Byte(value) | SprmOperand::Toggle(value) => *value == 0,
    SprmOperand::Word(value) | SprmOperand::Word4(value) | SprmOperand::Word5(value) => {
      *value == [0; 2]
    }
    SprmOperand::Dword(value) => *value == [0; 4],
    _ => false,
  }
}

fn table_direction(properties: &GrpPrl, kind: KnownSprm) -> Result<bool> {
  let Some(operand) = properties
    .properties
    .iter()
    .rev()
    .find(|property| property.sprm.kind() == SprmKind::Known(kind))
    .map(|property| &property.operand)
  else {
    return Ok(false);
  };
  let SprmOperand::Word(value) = operand else {
    return Err(Error::invalid(0, "table direction operand is not Bool16"));
  };
  let value = u16::from_le_bytes(*value);
  if value > 1 {
    return Err(Error::invalid(0, "table direction Bool16 exceeds one"));
  }
  Ok(value != 0)
}

fn table_from_rows<'a>(
  document_part: DocDocumentPartRef<'a>,
  table_depth: u32,
  rows: Vec<DocTableRowRef<'a>>,
) -> Result<DocTableRef<'a>> {
  let first = rows
    .first()
    .ok_or_else(|| Error::invalid(0, "DOC table has no rows"))?;
  let last = rows
    .last()
    .ok_or_else(|| Error::invalid(0, "DOC table has no rows"))?;
  let global_cp_range = DocCpRange {
    start: first.global_cp_range.start,
    end: last.global_cp_range.end,
  };
  Ok(DocTableRef {
    document_part,
    table_depth,
    global_cp_range,
    local_cp_range: local_range(document_part, global_cp_range)?,
    rows,
  })
}

fn infer_compatible_table_paragraph(state: DocDirectTableState, terminal: u16) -> DocParagraphKind {
  if !state.in_table && state.depth == 0 {
    return DocParagraphKind::NonTable;
  }
  let table_depth = state.depth.max(1);
  if state.table_terminating_paragraph || state.inner_table_terminating_paragraph {
    DocParagraphKind::TableTerminatingParagraph { table_depth }
  } else if state.inner_table_cell || (table_depth == 1 && terminal == 0x0007) {
    DocParagraphKind::CellMark { table_depth }
  } else {
    DocParagraphKind::TableParagraph { table_depth }
  }
}

fn paragraph_range(paragraphs: &[DocParagraphRef<'_>], start: usize, end: usize) -> DocCpRange {
  DocCpRange {
    start: paragraphs[start].global_cp_range.start,
    end: paragraphs[end].global_cp_range.end,
  }
}

fn local_range(document_part: DocDocumentPartRef<'_>, global: DocCpRange) -> Result<DocCpRange> {
  let base = document_part.global_cp_range.start.0;
  Ok(DocCpRange {
    start: DocCp(global.start.0.checked_sub(base).ok_or_else(|| {
      Error::invalid(
        u64::from(global.start.0),
        "DOC relationship range begins before its document part",
      )
    })?),
    end: DocCp(global.end.0.checked_sub(base).ok_or_else(|| {
      Error::invalid(
        u64::from(global.end.0),
        "DOC relationship range ends before its document part",
      )
    })?),
  })
}

impl<'a> DocCharacterRunRef<'a> {
  pub const fn document_part(self) -> DocDocumentPartRef<'a> {
    self.document_part
  }

  pub const fn source(self) -> &'a DocChpxRun {
    self.source
  }

  pub const fn global_cp_range(self) -> DocCpRange {
    self.global_cp_range
  }

  pub const fn local_cp_range(self) -> DocCpRange {
    self.local_cp_range
  }

  pub fn text_pieces(self) -> impl Iterator<Item = DocTextPieceRef<'a>> {
    self.document_part.text_pieces().filter(move |piece| {
      piece.global_cp_range.start.0 < self.global_cp_range.end.0
        && self.global_cp_range.start.0 < piece.global_cp_range.end.0
    })
  }
}

impl<'a> DocFormattedTextRef<'a> {
  pub const fn text(self) -> DocTextPieceRef<'a> {
    self.text
  }

  pub const fn character_run(self) -> DocCharacterRunRef<'a> {
    self.character_run
  }

  pub const fn local_cp_range(self) -> DocCpRange {
    self.text.local_cp_range
  }

  pub const fn global_cp_range(self) -> DocCpRange {
    self.text.global_cp_range
  }
}

impl<'a> DocDirectFormattingRef<'a> {
  pub const fn document_part(self) -> DocDocumentPartRef<'a> {
    self.document_part
  }

  pub const fn local_cp(self) -> DocCp {
    self.local_cp
  }

  pub const fn global_cp(self) -> DocCp {
    self.global_cp
  }

  pub const fn text_piece(self) -> DocTextPieceRef<'a> {
    self.text_piece
  }

  pub fn descriptor(self) -> &'a Pcd {
    match self.text_piece.descriptor {
      Some(value) => value,
      None => unreachable!("formatting refs require a resolved Pcd"),
    }
  }

  pub const fn paragraph(self) -> &'a DocPapxRun {
    self.paragraph
  }

  pub const fn character_run(self) -> &'a DocChpxRun {
    self.character_run
  }

  pub fn materialize(self) -> Result<DocDirectFormatting> {
    self
      .document_part
      .file
      .direct_formatting_at_cp(self.document_part.part, self.local_cp.0)
  }

  pub fn materialize_paragraph(self) -> Result<DocDirectParagraphFormatting> {
    self
      .document_part
      .file
      .direct_paragraph_formatting_at_cp(self.document_part.part, self.local_cp.0)
  }
}

impl<'a> DocFieldRef<'a> {
  pub const fn document_part(self) -> DocDocumentPartRef<'a> {
    self.document_part
  }

  pub const fn source(self) -> &'a super::Field {
    self.source
  }

  pub fn local_cp_range(self) -> Result<DocCpRange> {
    Ok(DocCpRange {
      start: DocCp(self.source.begin.position),
      end: DocCp(
        self
          .source
          .end
          .position
          .checked_add(1)
          .ok_or_else(|| Error::Limit("DOC field CP range overflow".into()))?,
      ),
    })
  }

  pub fn global_cp_range(self) -> Result<DocCpRange> {
    let local = self.local_cp_range()?;
    let base = self.document_part.global_cp_range.start.0;
    Ok(DocCpRange {
      start: DocCp(
        base
          .checked_add(local.start.0)
          .ok_or_else(|| Error::Limit("DOC field aggregate CP overflow".into()))?,
      ),
      end: DocCp(
        base
          .checked_add(local.end.0)
          .ok_or_else(|| Error::Limit("DOC field aggregate CP overflow".into()))?,
      ),
    })
  }

  pub fn instruction_fields(self) -> impl Iterator<Item = DocFieldRef<'a>> {
    self
      .source
      .instruction_fields
      .iter()
      .map(move |source| DocFieldRef {
        document_part: self.document_part,
        source,
      })
  }

  pub fn result_fields(self) -> impl Iterator<Item = DocFieldRef<'a>> {
    self
      .source
      .result_fields
      .iter()
      .map(move |source| DocFieldRef {
        document_part: self.document_part,
        source,
      })
  }
}

fn cp_ranges_overlap(left: DocCpRange, right: DocCpRange) -> bool {
  left.start.0 < right.end.0 && right.start.0 < left.end.0
}

fn make_text_range(
  document_part: DocDocumentPartRef<'_>,
  start: u32,
  end: u32,
) -> Result<DocTextRangeRef<'_>> {
  if start > end || end > document_part.local_cp_range().end.0 {
    return Err(Error::invalid(
      u64::from(start),
      "DOC relationship range is outside its document part",
    ));
  }
  let base = document_part.global_cp_range.start.0;
  Ok(DocTextRangeRef {
    document_part,
    local_cp_range: DocCpRange {
      start: DocCp(start),
      end: DocCp(end),
    },
    global_cp_range: DocCpRange {
      start: DocCp(
        base
          .checked_add(start)
          .ok_or_else(|| Error::Limit("DOC relationship start CP overflow".into()))?,
      ),
      end: DocCp(
        base
          .checked_add(end)
          .ok_or_else(|| Error::Limit("DOC relationship end CP overflow".into()))?,
      ),
    },
  })
}

fn make_aggregate_text_range<'a>(
  parts: &[DocDocumentPartRef<'a>; 7],
  start: u32,
  end: u32,
) -> Result<DocTextRangeRef<'a>> {
  if start > end {
    return Err(Error::invalid(
      start.into(),
      "DOC aggregate range is reversed",
    ));
  }
  let part = parts
    .iter()
    .copied()
    .find(|part| {
      let range = part.global_cp_range;
      if start == end {
        range.start.0 <= start && start < range.end.0
      } else {
        range.start.0 <= start && start < range.end.0 && end <= range.end.0
      }
    })
    .or_else(|| {
      parts
        .last()
        .copied()
        .filter(|part| start == end && start == part.global_cp_range.end.0)
    })
    .ok_or_else(|| {
      Error::invalid(
        start.into(),
        "DOC aggregate range does not lie in one document part",
      )
    })?;
  make_text_range(
    part,
    start - part.global_cp_range.start.0,
    end - part.global_cp_range.start.0,
  )
}

fn relationship_problem(
  preserve_compatibility: bool,
  diagnostics: &mut Vec<DocRelationshipDiagnostic>,
  index: Option<usize>,
  reason: impl Into<String>,
) -> Result<()> {
  let reason = reason.into();
  if preserve_compatibility {
    diagnostics.push(DocRelationshipDiagnostic { index, reason });
    Ok(())
  } else {
    Err(Error::invalid(
      index
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or(0),
      reason,
    ))
  }
}

fn build_bookmarks<'a>(
  file: &'a DocFile,
  parts: &[DocDocumentPartRef<'a>; 7],
  preserve_compatibility: bool,
) -> Result<DocBookmarks<'a>> {
  let mut result = DocBookmarks {
    bookmarks: Vec::new(),
    diagnostics: Vec::new(),
  };
  let Some(located) = &file.table.bookmarks else {
    return Ok(result);
  };
  let source = &located.value;
  let count = source
    .names
    .names
    .len()
    .min(source.starts.bookmarks.len())
    .min(source.starts.positions.len().saturating_sub(1));
  if count != source.names.names.len() || count != source.starts.bookmarks.len() {
    relationship_problem(
      preserve_compatibility,
      &mut result.diagnostics,
      None,
      "parallel standard bookmark tables have different cardinalities",
    )?;
  }
  let end_count = source.ends.positions.len().saturating_sub(1);
  let mut used_end_indices = BTreeSet::new();
  for index in 0..count {
    let properties = &source.starts.bookmarks[index];
    let end_index = usize::from(properties.end_index);
    if end_index >= end_count {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        "standard bookmark FBKF.ibkl is outside PlcfBkl",
      )?;
      continue;
    }
    if !used_end_indices.insert(end_index) {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        "standard bookmark FBKF.ibkl is not unique",
      )?;
    }
    let text = match make_aggregate_text_range(
      parts,
      source.starts.positions[index],
      source.ends.positions[end_index],
    ) {
      Ok(text) => text,
      Err(error) => {
        relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(index),
          format!("standard bookmark range is invalid: {error}"),
        )?;
        continue;
      }
    };
    result.bookmarks.push(DocBookmarkRef {
      index,
      name: &source.names.names[index],
      properties,
      text,
    });
  }
  Ok(result)
}

fn build_notes<'a>(
  file: &'a DocFile,
  main: DocDocumentPartRef<'a>,
  text_part: DocDocumentPartRef<'a>,
  kind: DocNoteKind,
  preserve_compatibility: bool,
) -> Result<DocNotes<'a>> {
  let mut result = DocNotes {
    kind,
    notes: Vec::new(),
    diagnostics: Vec::new(),
  };
  let tables = match kind {
    DocNoteKind::Footnote => file.table.footnotes.as_ref(),
    DocNoteKind::Endnote => file.table.endnotes.as_ref(),
  };
  let Some(tables) = tables else {
    if !text_part.local_cp_range().is_empty() {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        None,
        format!("non-empty {kind:?} Document has no reference/text PLCs"),
      )?;
    }
    return Ok(result);
  };
  let references = &tables.references.value;
  let positions = &tables.text.value.positions;
  let text_count = positions.len().saturating_sub(2);
  if text_count != references.indices.len() {
    relationship_problem(
      preserve_compatibility,
      &mut result.diagnostics,
      None,
      format!("{kind:?} reference/text PLC cardinality differs"),
    )?;
  }
  if positions.len() >= 2
    && positions[positions.len() - 2] != text_part.local_cp_range().end.0.saturating_sub(1)
  {
    relationship_problem(
      preserve_compatibility,
      &mut result.diagnostics,
      None,
      format!("{kind:?} text PLC does not end at ccp - 1"),
    )?;
  }
  let count = text_count.min(references.indices.len());
  let mut previous_reference = None;
  for index in 0..count {
    let reference_cp = references.positions[index];
    if previous_reference.is_some_and(|previous| previous >= reference_cp) {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        format!("{kind:?} reference CPs are not strictly increasing"),
      )?;
      continue;
    }
    previous_reference = Some(reference_cp);
    if reference_cp >= main.local_cp_range().end.0 {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        format!("{kind:?} reference CP is outside the Main Document"),
      )?;
      continue;
    }
    let text = match make_text_range(text_part, positions[index], positions[index + 1]) {
      Ok(text) => text,
      Err(error) => {
        relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(index),
          format!("{kind:?} text range is invalid: {error}"),
        )?;
        continue;
      }
    };
    let numbering_index = &references.indices[index];
    if *numbering_index != 0 && main.character_at(DocCp(reference_cp)) != Some(0x0002) {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        format!("automatically numbered {kind:?} reference is not character 0x02"),
      )?;
    }
    if *numbering_index != 0 {
      match file.effective_cf_spec_at_cp(FieldDocumentPart::Main, reference_cp) {
        Ok(true) => {}
        Ok(false) => relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(index),
          format!("automatically numbered {kind:?} reference lacks sprmCFSpec=1"),
        )?,
        Err(error) => relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(index),
          format!("{kind:?} reference formatting cannot be resolved: {error}"),
        )?,
      }
    }
    let text_len = text.local_cp_range.len();
    if text_len == 0 || text.character_at(DocCp(text_len.saturating_sub(1))) != Some(0x000d) {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        format!("{kind:?} text range does not end with U+000D"),
      )?;
    }
    result.notes.push(DocNoteRef {
      kind,
      index,
      reference_document: main,
      reference_cp: DocCp(reference_cp),
      numbering_index,
      text,
    });
  }
  Ok(result)
}

fn annotation_bookmark_ref<'a>(
  file: &'a DocFile,
  main: DocDocumentPartRef<'a>,
  tag: i32,
) -> Option<DocAnnotationBookmarkRef<'a>> {
  let source = &file.table.annotation_bookmarks.as_ref()?.value;
  let index = source
    .infos
    .entries
    .iter()
    .position(|entry| entry.tag == tag)?;
  let properties = source.starts.bookmarks.get(index)?;
  let start = *source.starts.positions.get(index)?;
  let end = *source
    .ends
    .positions
    .get(usize::from(properties.end_index))?;
  Some(DocAnnotationBookmarkRef {
    index,
    info: &source.infos.entries[index],
    properties,
    text: make_text_range(main, start, end).ok()?,
  })
}

fn comment_parent_index(extended: Option<&AnnotationPost10>, index: usize) -> Option<usize> {
  let extended = extended?;
  if extended.depth == 0 {
    return None;
  }
  i64::try_from(index)
    .ok()?
    .checked_add(i64::from(extended.parent_offset))
    .and_then(|value| usize::try_from(value).ok())
}

fn comment_ref_at<'a>(
  file: &'a DocFile,
  main: DocDocumentPartRef<'a>,
  text_part: DocDocumentPartRef<'a>,
  preserve_compatibility: bool,
  index: usize,
) -> Option<DocCommentRef<'a>> {
  let tables = file.table.annotations.as_ref()?;
  let annotation = tables.references.value.annotations.get(index)?;
  let reference_cp = *tables.references.value.positions.get(index)?;
  if reference_cp >= main.local_cp_range().end.0 {
    return None;
  }
  let start = *tables.text.value.positions.get(index)?;
  let end = *tables.text.value.positions.get(index + 1)?;
  let text = make_text_range(text_part, start, end).ok()?;
  let author = usize::try_from(annotation.author_index)
    .ok()
    .and_then(|author| {
      file
        .table
        .annotation_owners
        .as_ref()?
        .value
        .names
        .get(author)
    })
    .map(Vec::as_slice);
  let extended = file
    .table
    .annotation_extended_data
    .as_ref()
    .and_then(|data| data.value.comments.get(index));
  let annotation_bookmark = (annotation.bookmark_tag != -1)
    .then(|| annotation_bookmark_ref(file, main, annotation.bookmark_tag))
    .flatten();
  Some(DocCommentRef {
    file,
    preserve_compatibility,
    index,
    reference_document: main,
    reference_cp: DocCp(reference_cp),
    annotation,
    author,
    extended,
    annotation_bookmark,
    text,
  })
}

fn build_comments<'a>(
  file: &'a DocFile,
  main: DocDocumentPartRef<'a>,
  text_part: DocDocumentPartRef<'a>,
  preserve_compatibility: bool,
) -> Result<DocComments<'a>> {
  let mut result = DocComments {
    comments: Vec::new(),
    diagnostics: Vec::new(),
  };
  let Some(tables) = &file.table.annotations else {
    if !text_part.local_cp_range().is_empty() {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        None,
        "non-empty Comment Document has no PlcfandRef/PlcfandTxt",
      )?;
    }
    return Ok(result);
  };
  let reference_count = tables.references.value.annotations.len();
  let text_count = tables.text.value.positions.len().saturating_sub(2);
  if reference_count != text_count {
    relationship_problem(
      preserve_compatibility,
      &mut result.diagnostics,
      None,
      "comment reference/text PLC cardinality differs",
    )?;
  }
  if let Some(extended) = &file.table.annotation_extended_data
    && extended.value.comments.len() != reference_count
  {
    relationship_problem(
      preserve_compatibility,
      &mut result.diagnostics,
      None,
      "AtrdExtra/PlcfandRef cardinality differs",
    )?;
  }
  let count = reference_count.min(text_count);
  let mut previous_reference = None;
  for index in 0..count {
    let reference_cp = tables.references.value.positions[index];
    if previous_reference.is_some_and(|previous| previous >= reference_cp) {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        "comment reference CPs are not strictly increasing",
      )?;
      continue;
    }
    previous_reference = Some(reference_cp);
    let Some(comment) = comment_ref_at(file, main, text_part, preserve_compatibility, index) else {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        "comment anchor or text range is outside its document part",
      )?;
      continue;
    };
    if main.character_at(comment.reference_cp) != Some(0x0005) {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        "comment reference is not character 0x05",
      )?;
    }
    match comment.reference_has_effective_cf_spec() {
      Ok(true) => {}
      Ok(false) => relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        "comment reference lacks sprmCFSpec=1",
      )?,
      Err(error) => relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        format!("comment reference formatting cannot be resolved: {error}"),
      )?,
    }
    let text_len = comment.text.local_cp_range.len();
    if text_len == 0 || comment.text.character_at(DocCp(0)) != Some(0x0005) {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        "comment text range does not begin with character 0x05",
      )?;
    }
    if text_len == 0 || comment.text.character_at(DocCp(text_len.saturating_sub(1))) != Some(0x000d)
    {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        "comment text range does not end with U+000D",
      )?;
    }
    match comment.text_marker_has_effective_cf_spec() {
      Ok(true) => {}
      Ok(false) => relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        "comment text marker lacks sprmCFSpec=1",
      )?,
      Err(error) => relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        format!("comment text marker formatting cannot be resolved: {error}"),
      )?,
    }
    if comment.author.is_none() {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        "comment author index is outside GrpXstAtnOwners",
      )?;
    }
    if comment.annotation.bookmark_tag != -1 && comment.annotation_bookmark.is_none() {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        "comment lTagBkmk does not select an annotation bookmark",
      )?;
    }
    if comment
      .annotation_bookmark
      .is_some_and(|bookmark| bookmark.text.local_cp_range.is_empty())
    {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        Some(index),
        "zero-length commented range does not use lTagBkmk == -1",
      )?;
    }
    result.comments.push(comment);
  }
  Ok(result)
}

fn simple_office_art_property(property: Option<&OfficeArtProperty>) -> Option<u32> {
  property.and_then(|property| match &property.value {
    OfficeArtPropertyValue::Simple(value) => Some(*value),
    _ => None,
  })
}

fn last_office_art_property(
  records: &[OfficeArtRecord],
  property_id: u16,
) -> Option<&OfficeArtProperty> {
  records
    .iter()
    .filter_map(|record| match &record.data {
      OfficeArtRecordData::PropertyTable(table) => Some(table.properties.as_slice()),
      _ => None,
    })
    .flat_map(|properties| properties.iter())
    .rfind(|property| property.property_id == property_id)
}

fn collect_office_art_shapes<'a>(
  records: &'a [OfficeArtRecord],
  document_part: TextboxDocumentPart,
  shapes: &mut Vec<DocOfficeArtShapeRef<'a>>,
) {
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
      let client_textbox = children.iter().find_map(|child| match &child.data {
        OfficeArtRecordData::WordClientTextbox(value) => Some(value),
        _ => None,
      });
      if let Some((shape_type, shape)) = shape {
        let z_order = shapes.len();
        shapes.push(DocOfficeArtShapeRef {
          document_part,
          z_order,
          container: record,
          properties: children,
          shape_type,
          shape,
          text_id_property: last_office_art_property(children, 0x0080),
          next_shape_id_property: last_office_art_property(children, 0x008a),
          client_textbox,
        });
      }
    }
    if let Some(children) = children {
      collect_office_art_shapes(children, document_part, shapes);
    }
  }
}

fn build_textboxes<'a>(
  file: &'a DocFile,
  anchor_part: DocDocumentPartRef<'a>,
  text_part: DocDocumentPartRef<'a>,
  document_part: TextboxDocumentPart,
  preserve_compatibility: bool,
) -> Result<DocTextboxes<'a>> {
  let mut result = DocTextboxes {
    document_part,
    stories: Vec::new(),
    breaks: Vec::new(),
    anchors: Vec::new(),
    shapes: Vec::new(),
    diagnostics: Vec::new(),
  };

  if let Some(office_art) = &file.table.office_art
    && let Some(drawing) = office_art
      .value
      .drawings
      .iter()
      .find(|drawing| drawing.document_part == document_part)
  {
    match &drawing.container {
      super::DocOfficeArtRecordTree::Complete(stream) => {
        collect_office_art_shapes(&stream.records, document_part, &mut result.shapes);
      }
      super::DocOfficeArtRecordTree::Partial(_) => relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        None,
        format!("{document_part:?} OfficeArt drawing tree is partial"),
      )?,
    }
  }
  for shape in &result.shapes {
    let property_text_id = simple_office_art_property(shape.text_id_property);
    let client_text_id = shape
      .client_textbox
      .map(|value| (u32::from(value.story_index) << 16) | u32::from(value.chain_index));
    if property_text_id.is_some() && client_text_id.is_some() && property_text_id != client_text_id
    {
      relationship_problem(
        preserve_compatibility,
        &mut result.diagnostics,
        None,
        format!("{document_part:?} OfficeArt lTxid and ClientTextbox identifiers disagree"),
      )?;
    }
  }

  if let Some(breaks) = file.table.textbox_breaks.get(&document_part) {
    for (index, source) in breaks.value.breaks.iter().enumerate() {
      // MS-DOC PlcfTxbxBkd/PlcfTxbxHdrBkd define the final Tbkd as a
      // sentinel which is not associated with any FTXBXS object.
      if index + 1 == breaks.value.breaks.len() {
        continue;
      }
      let text = match make_text_range(
        text_part,
        breaks.value.positions[index],
        breaks.value.positions[index + 1],
      ) {
        Ok(text) => text,
        Err(_) => {
          relationship_problem(
            preserve_compatibility,
            &mut result.diagnostics,
            Some(index),
            format!("{document_part:?} Tbkd text range is outside its textbox document"),
          )?;
          continue;
        }
      };
      let story_index = match usize::try_from(source.story_index) {
        Ok(index) => Some(index),
        Err(_) => {
          relationship_problem(
            preserve_compatibility,
            &mut result.diagnostics,
            Some(index),
            format!("{document_part:?} Tbkd.itxbxs is negative"),
          )?;
          None
        }
      };
      result.breaks.push(DocTextboxBreakRef {
        index,
        source,
        text,
        story_index,
      });
    }
  }

  let stories = file.table.textbox_stories.get(&document_part);
  if stories.is_none() && !text_part.local_cp_range().is_empty() {
    relationship_problem(
      preserve_compatibility,
      &mut result.diagnostics,
      None,
      format!("non-empty {document_part:?} textbox document has no story PLC"),
    )?;
  }
  if let Some(stories) = stories {
    let story_count = stories.value.stories.len();
    for (index, source) in stories.value.stories.iter().enumerate() {
      let reusable = index + 1 == story_count || source.reusable_flags != 0;
      // Reusable FTXBXS entries are allocation placeholders, not live
      // textboxes. The final entry is always reusable and its text is
      // explicitly ignored by MS-DOC.
      if reusable {
        continue;
      }
      let text = match make_text_range(
        text_part,
        stories.value.positions[index],
        stories.value.positions[index + 1],
      ) {
        Ok(text) => text,
        Err(_) => {
          relationship_problem(
            preserve_compatibility,
            &mut result.diagnostics,
            Some(index),
            format!("{document_part:?} FTXBXS text range is outside its textbox document"),
          )?;
          continue;
        }
      };
      if text.local_cp_range.len() <= 1 {
        relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(index),
          format!("{document_part:?} live FTXBXS range has no content"),
        )?;
      }
      if text.character_at(DocCp(text.local_cp_range.len() - 1)) != Some(0x000d) {
        relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(index),
          format!("{document_part:?} FTXBXS text range does not end with U+000D"),
        )?;
      }

      let one_based_story = u16::try_from(index + 1).ok();
      let mut linked_shapes = result
        .shapes
        .iter()
        .copied()
        .filter(|shape| {
          shape
            .textbox_link()
            .is_some_and(|link| Some(link.story_index()) == one_based_story)
        })
        .collect::<Vec<_>>();
      linked_shapes.sort_by_key(|shape| {
        shape
          .textbox_link()
          .map_or(u16::MAX, |link| link.chain_index())
      });
      if linked_shapes.first().map(|shape| shape.shape.shape_id) != Some(source.shape_id) {
        relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(index),
          format!("{document_part:?} FTXBXS.lid does not select chain index zero"),
        )?;
      }
      if let TextboxStoryChain::NonReusable { textbox_count, .. } = source.chain
        && usize::try_from(textbox_count).ok() != Some(linked_shapes.len())
      {
        relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(index),
          format!("{document_part:?} FTXBXS cTxbx does not match shape chain"),
        )?;
      }
      for (chain_index, shape) in linked_shapes.iter().enumerate() {
        if shape
          .textbox_link()
          .is_none_or(|link| usize::from(link.chain_index()) != chain_index)
        {
          relationship_problem(
            preserve_compatibility,
            &mut result.diagnostics,
            Some(index),
            format!("{document_part:?} OfficeArt textbox chain indexes are not contiguous"),
          )?;
          break;
        }
        let declared_next = shape.next_shape_id().filter(|value| *value != 0);
        let actual_next = linked_shapes
          .get(chain_index + 1)
          .map(|next| next.shape.shape_id);
        if declared_next.is_some() && declared_next != actual_next {
          relationship_problem(
            preserve_compatibility,
            &mut result.diagnostics,
            Some(index),
            format!("{document_part:?} OfficeArt hspNext leaves its textbox chain"),
          )?;
        }
      }

      let mut linked_breaks = result
        .breaks
        .iter()
        .copied()
        .filter(|value| value.story_index == Some(index))
        .collect::<Vec<_>>();
      linked_breaks.sort_by_key(|value| value.text.local_cp_range.start);
      for value in &linked_breaks {
        if value.text.local_cp_range.start.0 < text.local_cp_range.start.0
          || value.text.local_cp_range.end.0 > text.local_cp_range.end.0
        {
          relationship_problem(
            preserve_compatibility,
            &mut result.diagnostics,
            Some(value.index),
            format!("{document_part:?} Tbkd range is outside its FTXBXS range"),
          )?;
        }
      }
      result.stories.push(DocTextboxStoryRef {
        document_part,
        index,
        source,
        text,
        reusable: false,
        shapes: linked_shapes,
        breaks: linked_breaks,
      });
    }
    for value in &result.breaks {
      if value.story_index.is_some_and(|index| index >= story_count) {
        relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(value.index),
          format!("{document_part:?} Tbkd.itxbxs is outside the story PLC"),
        )?;
      }
    }
  }

  if let Some(anchors) = file.table.shape_anchors.get(&document_part) {
    for (index, source) in anchors.value.anchors.iter().enumerate() {
      let cp = anchors.value.positions[index];
      if cp >= anchor_part.local_cp_range().end.0 {
        relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(index),
          format!("{document_part:?} shape anchor CP is outside its document part"),
        )?;
        continue;
      }
      let shape = result
        .shapes
        .iter()
        .find(|shape| shape.shape.shape_id == source.shape_id)
        .copied();
      if shape.is_none() {
        relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(index),
          format!("{document_part:?} SPA shape id has no OfficeArtFSP"),
        )?;
      }
      let anchor = DocShapeAnchorRef {
        index,
        anchor_document: anchor_part,
        anchor_cp: DocCp(cp),
        source,
        shape,
      };
      if anchor.anchor_character() != Some(0x0008) {
        relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(index),
          format!("{document_part:?} SPA anchor is not character 0x08"),
        )?;
      }
      match anchor.anchor_has_effective_cf_spec() {
        Ok(true) => {}
        Ok(false) => relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(index),
          format!("{document_part:?} SPA anchor lacks sprmCFSpec=1"),
        )?,
        Err(error) => relationship_problem(
          preserve_compatibility,
          &mut result.diagnostics,
          Some(index),
          format!("{document_part:?} SPA formatting cannot be resolved: {error}"),
        )?,
      }
      result.anchors.push(anchor);
    }
  }
  Ok(result)
}

impl DocFile {
  /// Builds the strict zero-copy relationship view over all seven text
  /// document parts. Missing FKP CP trees or a PlcPcd/Pcd cardinality drift
  /// are rejected rather than hidden behind an empty iterator.
  pub fn content_tree(&self) -> Result<DocContentTree<'_>> {
    self.content_tree_with_policy(false)
  }

  /// Builds every unambiguous relationship for a producer-compatible file.
  /// `paragraphs()` or `character_runs()` is empty when its corresponding
  /// compatible FKP tree could not be formed; physical FKP pages remain
  /// reachable from `DocWordDocumentStream`.
  pub fn content_tree_compatible(&self) -> Result<DocContentTree<'_>> {
    self.content_tree_with_policy(true)
  }

  /// Resolves a one-based OfficeArt BLIP identifier across both embedded
  /// BStore payloads and host-delayed BLIPs in the WordDocument stream.
  /// Raster and uncompressed metafile payloads remain borrowed end to end.
  pub fn office_art_image_link(
    &self,
    blip_identifier: u32,
  ) -> Result<Option<DocOfficeArtImageLink<'_>>> {
    let Some(office_art) = self.table.office_art.as_ref() else {
      return Ok(None);
    };
    let Some(link) = office_art.value.image_link(blip_identifier)? else {
      return Ok(None);
    };
    let DocOfficeArtImageLink::Delayed {
      word_document_offset,
    } = link
    else {
      return Ok(Some(link));
    };
    let offset = usize::try_from(word_document_offset)
      .map_err(|_| Error::Limit("OfficeArt delayed BLIP offset exceeds usize".into()))?;
    let Some(bytes) = self.word_document.physical_bytes.as_slice().get(offset..) else {
      return Ok(Some(DocOfficeArtImageLink::Unsupported));
    };
    Ok(Some(image_ref_from_record_bytes(bytes)?.map_or(
      DocOfficeArtImageLink::Unsupported,
      DocOfficeArtImageLink::Resolved,
    )))
  }

  fn content_tree_with_policy(&self, preserve_compatibility: bool) -> Result<DocContentTree<'_>> {
    if !preserve_compatibility {
      if self.word_document.papx_runs.is_none() {
        return Err(Error::invalid(0, "DOC PAPX CP tree is unavailable"));
      }
      if self.word_document.chpx_runs.is_none() {
        return Err(Error::invalid(0, "DOC CHPX CP tree is unavailable"));
      }
    }
    if self.word_document.text_pieces.len() != self.table.clx.value.piece_table.pieces.len() {
      return Err(Error::invalid(0, "PlcPcd text/Pcd cardinality changed"));
    }
    for (index, piece) in self.word_document.text_pieces.iter().enumerate() {
      if piece.piece_index != index {
        return Err(Error::invalid(
          u64::try_from(index).unwrap_or(u64::MAX),
          "text piece identity no longer selects its Pcd descriptor",
        ));
      }
      let start = u32::try_from(piece.value.cp_start)
        .map_err(|_| Error::invalid(0, "PlcPcd text piece has a negative start CP"))?;
      let end = u32::try_from(piece.value.cp_end)
        .map_err(|_| Error::invalid(0, "PlcPcd text piece has a negative end CP"))?;
      let count = u32::try_from(piece.value.characters.character_count())
        .map_err(|_| Error::Limit("DOC text piece character count exceeds u32".into()))?;
      if end < start || end - start != count {
        return Err(Error::invalid(
          u64::from(start),
          "PlcPcd CP interval does not match its character units",
        ));
      }
    }
    let mut global_start = 0u32;
    let mut parts = Vec::with_capacity(7);
    for (part, length) in document_part_lengths(&self.word_document.fib)? {
      let start = global_start;
      global_start = global_start
        .checked_add(length)
        .ok_or_else(|| Error::Limit("DOC document-part CP range overflow".into()))?;
      parts.push(DocDocumentPartRef {
        file: self,
        part,
        global_cp_range: DocCpRange {
          start: DocCp(start),
          end: DocCp(global_start),
        },
        preserve_compatibility,
      });
    }
    let parts = parts.try_into().map_err(|_| {
      Error::invalid(
        0,
        "DOC document-part inventory does not contain seven parts",
      )
    })?;
    Ok(DocContentTree {
      file: self,
      parts,
      preserve_compatibility,
    })
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

  /// Opens a path in strict mode and returns its owned MS-DOC tree.
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
    let compound = compound_from_path(path.as_ref(), options, BinaryFormat::Doc)?;
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
    let compound = compound_from_bytes(bytes, options, BinaryFormat::Doc)?;
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
    let compound = compound_from_vec(bytes, options, BinaryFormat::Doc)?;
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
    let compound = compound_outcome(compound_file, options, BinaryFormat::Doc)?;
    Self::from_compound_outcome(compound, options)
  }

  fn from_compound_outcome(
    compound: ParseOutcome<CompoundFile>,
    options: ParseOptions,
  ) -> Result<ParseOutcome<Self>> {
    let ParseOutcome {
      value: compound_file,
      mut diagnostics,
    } = compound;
    let limits = options.limits;
    let word_data = required_stream_data(&compound_file, WORD_DOCUMENT_STREAM_PATH)?;
    let word_bytes = word_data.clone();
    ensure_stream_limit("WordDocument", &word_bytes, limits)?;
    let fib = Fib::from_word_document(&word_bytes)?;
    if fib
      .base
      .flags
      .intersects(FibBaseFlags::ENCRYPTED | FibBaseFlags::OBFUSCATED)
    {
      return Err(Error::invalid(
        0,
        "encrypted or obfuscated DOC requires an MS-OFFCRYPTO layer",
      ));
    }
    let table_name = if fib.base.flags.contains(FibBaseFlags::USE_1_TABLE) {
      DocTableStreamName::Table1
    } else {
      DocTableStreamName::Table0
    };
    let table_data = required_stream_data(&compound_file, table_name.path())?;
    let table_bytes = table_data.clone();
    ensure_stream_limit(table_name.path(), &table_bytes, limits)?;

    let clx = parse_required(&table_bytes, fib.clx_location(), "CLX", Clx::from_bytes)?;
    let character_bin_table = parse_required(
      &table_bytes,
      fib.chpx_bte_location(),
      "PlcBteChpx",
      PlcBte::from_bytes,
    )?;
    let paragraph_bin_table = parse_required(
      &table_bytes,
      fib.papx_bte_location(),
      "PlcBtePapx",
      PlcBte::from_bytes,
    )?;
    let sections = parse_required(
      &table_bytes,
      fib.section_table_location(),
      "PlcfSed",
      PlcfSed::from_bytes,
    )?;
    let styles = parse_optional(
      &table_bytes,
      fib.style_sheet_location(),
      "STSH",
      StyleSheet::from_bytes,
    )?;
    let fonts = parse_optional(
      &table_bytes,
      fib.font_table_location(),
      "SttbfFfn",
      FontTable::from_bytes,
    )?;
    let mut fields = BTreeMap::new();
    for (part, location) in fib.field_table_locations() {
      if let Some(field) = parse_optional(&table_bytes, Some(location), "Plcfld", |bytes| {
        FieldTable::from_bytes_with_compatibility(bytes, !options.is_strict())
      })? {
        for position in field.value.separator_flag_mismatches() {
          report_doc_compatibility(
            &mut diagnostics,
            ParseDiagnosticCode::NonconformingRecord,
            u64::from(location.fc),
            "Plcfld",
            format!(
              "field ending at document-part CP {position} has a separator that disagrees with grffldEnd.fHasSep"
            ),
          );
        }
        fields.insert(part, field);
      }
    }
    let bookmarks = parse_bookmarks(&fib, &table_bytes)?;
    let mut compatibility_tables = Vec::new();
    let header_text = parse_optional(
      &table_bytes,
      fib.header_text_location(),
      "PlcfHdd",
      HeaderTextTable::from_bytes,
    )?;
    let footnotes = parse_note_tables(&table_bytes, fib.footnote_locations(), "footnote")?;
    let endnotes = parse_note_tables(&table_bytes, fib.endnote_locations(), "endnote")?;
    let annotations = parse_annotation_tables(&table_bytes, fib.annotation_locations())?;
    let annotation_owners = parse_optional(
      &table_bytes,
      fib.annotation_owner_location(),
      "GrpXstAtnOwners",
      AnnotationOwners::from_bytes,
    )?;
    let annotation_bookmarks =
      parse_annotation_bookmarks(&fib, &table_bytes, options, &mut diagnostics)?;
    let annotation_extended_data = parse_optional_compatible(
      &table_bytes,
      fib.annotation_extended_data_location(),
      "AtrdExtra",
      AnnotationExtendedData::from_bytes,
      options,
      &mut diagnostics,
      &mut compatibility_tables,
    )?;
    let textbox_stories = parse_part_tables(
      &table_bytes,
      fib.textbox_story_locations(),
      "PlcfTxbxTxt",
      TextboxStoryTable::from_bytes,
    )?;
    let textbox_breaks = parse_part_tables(
      &table_bytes,
      fib.textbox_break_locations(),
      "PlcfTxbxBkd",
      TextboxBreakTable::from_bytes,
    )?;
    let shape_anchors = parse_part_tables(
      &table_bytes,
      fib.shape_anchor_locations(),
      "PlcSpa",
      ShapeAnchorTable::from_bytes,
    )?;
    let office_art = parse_optional(
      &table_bytes,
      fib.office_art_content_location(),
      "OfficeArtContent",
      DocOfficeArtContent::from_bytes,
    )?;
    let revision_authors = parse_optional(
      &table_bytes,
      fib.revision_authors_location(),
      "SttbfRMark",
      RevisionAuthors::from_bytes,
    )?;
    let captions = parse_caption_tables(
      &table_bytes,
      fib.caption_locations(),
      options,
      &mut diagnostics,
    )?;
    let subdocuments = parse_optional_compatible(
      &table_bytes,
      fib.subdocuments_location(),
      "PlcfWkb",
      SubdocumentTable::from_bytes,
      options,
      &mut diagnostics,
      &mut compatibility_tables,
    )?;
    let user_variables = parse_optional(
      &table_bytes,
      fib.user_variables_location(),
      "StwUser",
      UserVariables::from_bytes,
    )?;
    let embedded_fonts = parse_optional(
      &table_bytes,
      fib.embedded_fonts_location(),
      "SttbTtmbd",
      EmbeddedFontTable::from_bytes,
    )?;
    let spelling_state = parse_optional(
      &table_bytes,
      fib.spelling_state_location(),
      "PlcfSpl",
      SpellingStateTable::from_bytes,
    )?;
    let grammar_state = parse_optional(
      &table_bytes,
      fib.grammar_state_location(),
      "PlcfGram",
      GrammarStateTable::from_bytes,
    )?;
    let language_detection_state = parse_optional(
      &table_bytes,
      fib.language_detection_state_location(),
      "PlcfLad",
      LanguageDetectionStateTable::from_bytes,
    )?;
    let list_definitions = parse_list_definitions(
      &table_bytes,
      fib.list_definition_location(),
      options,
      &mut diagnostics,
      &mut compatibility_tables,
    )?;
    let list_names = parse_optional(
      &table_bytes,
      fib.list_names_location(),
      "SttbListNames",
      ListNamesTable::from_bytes,
    )?;
    let list_overrides = parse_optional(
      &table_bytes,
      fib.list_override_location(),
      "PlfLfo",
      ListOverrides::from_bytes,
    )?;
    let document_properties = parse_optional(
      &table_bytes,
      fib.document_properties_location(),
      "Dop",
      DocumentProperties::from_bytes,
    )?;
    let associated_strings = parse_optional(
      &table_bytes,
      fib.associated_strings_location(),
      "SttbfAssoc",
      AssociatedStrings::from_bytes,
    )?;
    let external_file_names = parse_optional(
      &table_bytes,
      fib.external_file_names_location(),
      "SttbFnm",
      ExternalFileNameTable::from_bytes,
    )?;
    macro_rules! parse_compatible_table {
      ($location:expr, $label:literal, $parser:path) => {
        parse_optional_compatible(
          &table_bytes,
          $location,
          $label,
          $parser,
          options,
          &mut diagnostics,
          &mut compatibility_tables,
        )?
      };
    }
    let mail_merge_state = parse_compatible_table!(
      fib.mail_merge_state_location(),
      "Pms",
      MailMergeState::from_bytes
    );
    let new_mail_merge_state = parse_compatible_table!(
      fib.new_mail_merge_state_location(),
      "PmsNew",
      MailMergeState::from_bytes
    );
    let office_data_source = parse_compatible_table!(
      fib.office_data_source_location(),
      "Odso",
      OfficeDataSource::from_bytes
    );
    let printer_driver_info = parse_compatible_table!(
      fib.printer_driver_info_location(),
      "PrDrvr",
      PrinterDriverInfo::from_bytes
    );
    let ole_control_infos = parse_compatible_table!(
      fib.ole_control_info_location(),
      "RgxOcxInfo",
      OleControlInfos::from_bytes
    );
    let table_character_cache = parse_compatible_table!(
      fib.table_character_cache_location(),
      "PlcfTch",
      TableCharacterCacheTable::from_bytes
    );
    let revision_message_threading = parse_compatible_table!(
      fib.revision_message_threading_location(),
      "RmdThreading",
      RevisionMessageThreading::from_bytes
    );
    let list_style_templates = parse_compatible_table!(
      fib.list_style_templates_location(),
      "SttbRgtplc",
      ListStyleTemplates::from_bytes
    );
    let frame_and_list_records = parse_compatible_table!(
      fib.frame_and_list_records_location(),
      "RgDofr",
      FrameAndListRecords::from_bytes
    );
    let grammar_option_sets = parse_compatible_table!(
      fib.grammar_option_sets_location(),
      "PlfCosi",
      GrammarOptionSets::from_bytes
    );
    let legacy_grammar_option_sets = parse_compatible_table!(
      fib.legacy_grammar_option_sets_location(),
      "PlfGosl",
      LegacyGrammarOptionSets::from_bytes
    );
    let auto_summary_ranges = parse_compatible_table!(
      fib.auto_summary_ranges_location(),
      "PlcfAsumy",
      AutoSummaryRangeTable::from_bytes
    );
    let smart_tag_recognizer_state = parse_compatible_table!(
      fib.smart_tag_recognizer_state_location(),
      "PlcfFactoid",
      SmartTagRecognizerStateTable::from_bytes
    );
    let xml_schema_references = parse_compatible_table!(
      fib.xml_schema_references_location(),
      "Hplxsdr",
      XmlSchemaReferences::from_bytes
    );
    let xml_transform_path = parse_compatible_table!(
      fib.xml_transform_path_location(),
      "CustomXForm",
      XmlTransformPath::from_bytes
    );
    let paragraph_group_properties = parse_compatible_table!(
      fib.paragraph_group_properties_location(),
      "PlcfPgp",
      ParagraphGroupProperties::from_bytes
    );
    let save_history = parse_compatible_table!(
      fib.save_history_location(),
      "SttbSavedBy",
      SaveHistory::from_bytes
    );
    let grammar_checker_cookies = parse_compatible_table!(
      fib.grammar_checker_cookies_location(),
      "PlcfCookie",
      GrammarCheckerCookieTable::from_bytes
    );
    let legacy_grammar_checker_cookies = parse_compatible_table!(
      fib.legacy_grammar_checker_cookies_location(),
      "PlcfCookieOld",
      LegacyGrammarCheckerCookieTable::from_bytes
    );
    let grammar_cookie_data = parse_compatible_table!(
      fib.grammar_cookie_data_location(),
      "CookieData",
      GrammarCookieStore::from_bytes
    );
    let smart_tag_data = parse_compatible_table!(
      fib.smart_tag_data_location(),
      "FactoidData",
      SmartTagData::from_bytes
    );
    let revision_save_ids = parse_compatible_table!(
      fib.revision_save_ids_location(),
      "Plrsid",
      RevisionSaveIdTable::from_bytes
    );
    let selection_state = parse_compatible_table!(
      fib.selection_state_location(),
      "Wss",
      SelectionState::from_bytes
    );
    let command_customizations = parse_compatible_table!(
      fib.command_customizations_location(),
      "Cmds",
      CommandCustomizations::from_bytes
    );
    let structured_tag_bookmarks = parse_bookmark_set(
      &table_bytes,
      fib.structured_tag_bookmark_locations(),
      DocCompatibilityGroup {
        labels: ["SttbfBkmkSdt", "PlcfBkfSdt", "PlcfBklSdt"],
        structure: "structured-tag bookmarks",
      },
      StructuredTagBookmarks::from_bytes,
      options,
      &mut diagnostics,
      &mut compatibility_tables,
    )?;
    let range_protection = parse_range_protection(
      &table_bytes,
      fib.range_protection_locations(),
      options,
      &mut diagnostics,
      &mut compatibility_tables,
    )?;
    let smart_tag_bookmarks = parse_bookmark_set(
      &table_bytes,
      fib.smart_tag_bookmark_locations(),
      DocCompatibilityGroup {
        labels: ["SttbfBkmkFactoid", "PlcfBkfFactoid", "PlcfBklFactoid"],
        structure: "smart-tag bookmarks",
      },
      SmartTagBookmarks::from_bytes,
      options,
      &mut diagnostics,
      &mut compatibility_tables,
    )?;
    let format_consistency_bookmarks = parse_bookmark_set(
      &table_bytes,
      fib.format_consistency_bookmark_locations(),
      DocCompatibilityGroup {
        labels: ["SttbfBkmkFcc", "PlcfBkfFcc", "PlcfBklFcc"],
        structure: "format-consistency bookmarks",
      },
      FormatConsistencyBookmarks::from_bytes,
      options,
      &mut diagnostics,
      &mut compatibility_tables,
    )?;
    let repair_bookmarks = parse_bookmark_set(
      &table_bytes,
      fib.repair_bookmark_locations(),
      DocCompatibilityGroup {
        labels: ["SttbfBkmkBpRepairs", "PlcfBkfBpRepairs", "PlcfBklBpRepairs"],
        structure: "repair bookmarks",
      },
      RepairBookmarks::from_bytes,
      options,
      &mut diagnostics,
      &mut compatibility_tables,
    )?;
    let user_input_methods = parse_user_input_methods(
      &table_bytes,
      fib.user_input_method_locations(),
      options,
      &mut diagnostics,
      &mut compatibility_tables,
    )?;
    let mso_envelope = parse_optional_compatible(
      &table_bytes,
      fib.mso_envelope_location(),
      "MsoEnvelope",
      MsoEnvelope::from_bytes,
      options,
      &mut diagnostics,
      &mut compatibility_tables,
    )?;
    let deprecated_numbering_field_cache = parse_optional_compatible(
      &table_bytes,
      fib.deprecated_numbering_field_cache_location(),
      "PlcfBteLvc",
      |bytes| Ok::<_, Error>(bytes.to_vec()),
      options,
      &mut diagnostics,
      &mut compatibility_tables,
    )?
    .map(|value| DocDeprecatedNumberingFieldCache {
      location: value.location,
      physical_bytes: value.value,
    });

    let text_pieces = parse_text_pieces(&clx.value, &word_bytes, options, &mut diagnostics)?;
    let character_format_pages =
      parse_fkp_pages(&character_bin_table.value, &word_bytes, ChpxFkp::from_bytes)?;
    let paragraph_format_pages =
      parse_fkp_pages(&paragraph_bin_table.value, &word_bytes, PapxFkp::from_bytes)?;
    validate_fkp_page_order(
      &character_format_pages,
      |page| &page.file_positions,
      "ChpxFkp rgfc",
      options,
      &mut diagnostics,
    )?;
    validate_fkp_page_order(
      &paragraph_format_pages,
      |page| &page.file_positions,
      "PapxFkp rgfc",
      options,
      &mut diagnostics,
    )?;
    let chpx_runs = match source_character_formatting_runs(&character_format_pages, &clx.value) {
      Ok(runs) => Some(runs),
      Err(error) if !options.is_strict() => {
        report_doc_compatibility(
          &mut diagnostics,
          ParseDiagnosticCode::NonconformingRecord,
          u64::from(character_bin_table.location.fc),
          "PlcBteChpx",
          format!("CHPX FKP runs cannot form a CP tree: {error}"),
        );
        None
      }
      Err(error) => return Err(error),
    };
    let papx_runs =
      match source_paragraph_formatting_runs(&paragraph_format_pages, &clx.value, &word_bytes) {
        Ok(runs) => Some(runs),
        Err(error) if !options.is_strict() => {
          report_doc_compatibility(
            &mut diagnostics,
            ParseDiagnosticCode::NonconformingRecord,
            u64::from(paragraph_bin_table.location.fc),
            "PlcBtePapx",
            format!("PAPX FKP runs cannot form a CP tree: {error}"),
          );
          None
        }
        Err(error) => return Err(error),
      };
    let section_properties = sections
      .value
      .sections
      .iter()
      .enumerate()
      .map(|(section_index, sed)| {
        let value = Sepx::from_word_document(&word_bytes, sed.sepx_offset)?;
        let physical_len = value
          .as_ref()
          .map(Sepx::to_bytes)
          .transpose()?
          .map_or(0, |bytes| bytes.len());
        Ok(DocSectionProperties {
          section_index,
          offset: sed.sepx_offset,
          physical_len,
          value,
        })
      })
      .collect::<Result<Vec<_>>>()?;
    ensure_entry_limit("DOC text pieces", text_pieces.len(), limits)?;
    ensure_entry_limit("DOC fields", fields.len(), limits)?;

    let source_fib_len = fib.encoded_len();
    let source_piece_indices = (0..text_pieces.len()).collect();
    let source_chpx_runs = chpx_runs.clone();
    let source_papx_runs = papx_runs.clone();
    let word_document = DocWordDocumentStream {
      fib,
      text_pieces,
      chpx_runs,
      papx_runs,
      character_format_pages,
      paragraph_format_pages,
      section_properties,
      physical_bytes: word_data,
      source_fib_len,
      source_piece_indices,
      source_chpx_runs,
      source_papx_runs,
      pending_text_edits: BTreeMap::new(),
      rebuild_character_formatting: false,
      rebuild_paragraph_formatting: false,
    };
    let table = DocTableStream {
      name: table_name,
      clx,
      character_bin_table,
      paragraph_bin_table,
      sections,
      styles,
      fonts,
      fields,
      bookmarks,
      header_text,
      footnotes,
      endnotes,
      annotations,
      annotation_owners,
      annotation_bookmarks,
      annotation_extended_data,
      textbox_stories,
      textbox_breaks,
      shape_anchors,
      office_art,
      revision_authors,
      captions,
      subdocuments,
      user_variables,
      embedded_fonts,
      spelling_state,
      grammar_state,
      language_detection_state,
      list_definitions,
      list_names,
      list_overrides,
      document_properties,
      associated_strings,
      external_file_names,
      mail_merge_state,
      new_mail_merge_state,
      office_data_source,
      printer_driver_info,
      ole_control_infos,
      table_character_cache,
      revision_message_threading,
      list_style_templates,
      frame_and_list_records,
      grammar_option_sets,
      legacy_grammar_option_sets,
      auto_summary_ranges,
      smart_tag_recognizer_state,
      xml_schema_references,
      xml_transform_path,
      paragraph_group_properties,
      save_history,
      grammar_checker_cookies,
      legacy_grammar_checker_cookies,
      grammar_cookie_data,
      smart_tag_data,
      revision_save_ids,
      selection_state,
      command_customizations,
      structured_tag_bookmarks,
      range_protection,
      smart_tag_bookmarks,
      format_consistency_bookmarks,
      repair_bookmarks,
      user_input_methods,
      mso_envelope,
      deprecated_numbering_field_cache,
      compatibility_tables,
      physical_bytes: table_data,
    };
    let source_data = compound_file
      .entry(DATA_STREAM_PATH)
      .filter(|entry| entry.is_stream())
      .map(|entry| entry.data.clone());
    let source_data_stream_present = source_data.is_some();
    let data = parse_data_stream(
      source_data,
      &word_document,
      &table,
      options,
      &mut diagnostics,
    )?;
    let data_link_baseline = if options.is_strict() {
      DocDataLinkBaseline {
        source_stream_present: source_data_stream_present,
        unresolved_references: BTreeMap::new(),
      }
    } else {
      build_data_link_baseline(
        source_data_stream_present,
        &word_document,
        &table,
        data.as_ref(),
      )?
    };
    let object_pool = parse_object_pool(&compound_file, options, &mut diagnostics)?;
    let shared = OfficeSharedContent::from_compound_file_with_host(
      &compound_file,
      options,
      Some(OfficeHostKind::Doc),
    )?;
    diagnostics.extend(shared.diagnostics);
    Ok(ParseOutcome::new(
      Self {
        compound_file,
        data_link_baseline: Arc::new(data_link_baseline),
        shared: shared.value,
        word_document: word_document.into(),
        table: table.into(),
        data: data.map(Arc::new),
        object_pool: object_pool.map(Arc::new),
      },
      diagnostics,
    ))
  }

  /// Replaces one host VBA module source and removes every host signature
  /// that the mutation invalidates. The complete DOC tree is transactional.
  pub fn replace_vba_module_source(
    &mut self,
    stream_name: &str,
    source: &[u8],
  ) -> Result<OfficeVbaModuleMutation> {
    let mut candidate = self.clone();
    let mut report = candidate
      .shared
      .replace_vba_module_source(stream_name, source)?;
    if let Some(user_variables) = &mut Arc::make_mut(&mut candidate.table).user_variables {
      report.invalidated_host_signatures = user_variables.value.remove_vba_signatures();
      UserVariables::from_bytes(&user_variables.value.to_bytes()?)?;
    }
    candidate.validate_links()?;
    *self = candidate;
    Ok(report)
  }

  /// Transactionally edits one VBA Designer storage and invalidates DOC VBA signatures.
  pub fn edit_vba_designer_storage(
    &mut self,
    index: usize,
    edit: impl FnOnce(&mut ParentControlStorageModel) -> Result<()>,
  ) -> Result<OfficeFormsMutation> {
    let mut candidate = self.clone();
    let mut report = candidate.shared.edit_vba_designer_storage(index, edit)?;
    if let Some(user_variables) = &mut Arc::make_mut(&mut candidate.table).user_variables {
      report.invalidated_host_signatures = user_variables.value.remove_vba_signatures();
      UserVariables::from_bytes(&user_variables.value.to_bytes()?)?;
    }
    candidate.validate_links()?;
    *self = candidate;
    Ok(report)
  }

  /// Rebuilds managed streams from the typed tree and returns a strict CFB.
  pub fn to_compound_file(&self) -> Result<CompoundFile> {
    self.to_compound_file_with_options(SaveOptions::default())
  }

  /// Rebuilds managed streams while explicitly retaining compatibility nodes.
  pub fn to_compound_file_preserving_compatibility(&self) -> Result<CompoundFile> {
    self.to_compound_file_with_options(SaveOptions::preserving_compatibility())
  }

  /// Rebuilds managed streams under the requested compatibility policy.
  pub fn to_compound_file_with_options(&self, options: SaveOptions) -> Result<CompoundFile> {
    let compound = self.to_compound_file_with_current_layout(options)?;
    if !options.preserves_compatibility() {
      // Validate the bytes the native CFB writer actually emits.  The
      // source tree may carry compatibility-only physical CFB state
      // that the writer canonicalizes while serializing.
      let bytes = compound.to_bytes()?;
      drop(Self::validate_emitted_bytes(bytes, options)?);
    }
    Ok(compound)
  }

  fn validate_emitted_bytes(bytes: Vec<u8>, options: SaveOptions) -> Result<Vec<u8>> {
    if !options.preserves_compatibility() {
      let archive = Arc::new(bytes);
      let compound =
        CompoundFile::from_shared_archive_with_limits(Arc::clone(&archive), Limits::default())?;
      drop(Self::from_compound_file(compound)?);
      return Arc::try_unwrap(archive)
        .map_err(|_| Error::invalid(0, "validated DOC retained its emitted CFB archive"));
    }
    Ok(bytes)
  }

  fn to_compound_file_with_current_layout(&self, options: SaveOptions) -> Result<CompoundFile> {
    self
      .compound_write_plan_with_current_layout(options)?
      .into_compound()
  }

  fn compound_write_plan_with_current_layout(
    &self,
    options: SaveOptions,
  ) -> Result<DocCompoundWritePlan<'_>> {
    self.validate_links()?;
    let source_word = self.word_document.physical_bytes.as_slice();
    let mut table = TableLayout::new(&self.table.physical_bytes);
    let mut fib = self.word_document.fib.clone();
    let mut clx_table = self.table.clx.clone();
    let mut character_bin_table = self.table.character_bin_table.clone();
    let mut paragraph_bin_table = self.table.paragraph_bin_table.clone();
    let mut character_format_pages = self.word_document.character_format_pages.clone();
    let mut paragraph_format_pages = self.word_document.paragraph_format_pages.clone();
    let mut section_properties = self.word_document.section_properties.clone();
    let mut sections_table = self.table.sections.clone();
    let mut styles_table = self.table.styles.clone();
    let mut list_definitions = self.table.list_definitions.clone();
    let RebuiltDataStream {
      plan: data_plan,
      relocations: data_relocations,
    } = rebuild_data_stream(self.data.as_deref())?;
    relocate_root_data_references(
      &mut clx_table,
      &mut character_format_pages,
      &mut paragraph_format_pages,
      &mut section_properties,
      styles_table.as_mut(),
      list_definitions.as_mut(),
      &data_relocations,
    )?;
    if let Some(styles) = &styles_table {
      patch_located(&mut table, styles, StyleSheet::to_bytes, "STSH")?;
    }
    if let Some(fonts) = &self.table.fonts {
      patch_located(&mut table, fonts, FontTable::to_bytes, "SttbfFfn")?;
    }
    for field in self.table.fields.values() {
      patch_located(&mut table, field, FieldTable::to_bytes, "Plcfld")?;
    }
    if let Some(bookmarks) = &self.table.bookmarks {
      let (names, starts, ends) = bookmarks.value.to_bytes()?;
      patch_location(&mut table, bookmarks.names_location, names, "SttbfBkmk")?;
      patch_location(&mut table, bookmarks.starts_location, starts, "PlcfBkf")?;
      patch_location(&mut table, bookmarks.ends_location, ends, "PlcfBkl")?;
    }
    patch_optional_located(
      &mut table,
      self.table.header_text.as_ref(),
      HeaderTextTable::to_bytes,
      "PlcfHdd",
    )?;
    if let Some(notes) = &self.table.footnotes {
      patch_located(
        &mut table,
        &notes.references,
        NoteReferenceTable::to_bytes,
        "PlcfFndRef",
      )?;
      patch_located(&mut table, &notes.text, CpOnlyTable::to_bytes, "PlcfFndTxt")?;
    }
    if let Some(notes) = &self.table.endnotes {
      patch_located(
        &mut table,
        &notes.references,
        NoteReferenceTable::to_bytes,
        "PlcfEndRef",
      )?;
      patch_located(&mut table, &notes.text, CpOnlyTable::to_bytes, "PlcfEndTxt")?;
    }
    if let Some(annotations) = &self.table.annotations {
      patch_located(
        &mut table,
        &annotations.references,
        AnnotationReferenceTable::to_bytes,
        "PlcfAndRef",
      )?;
      patch_located(
        &mut table,
        &annotations.text,
        CpOnlyTable::to_bytes,
        "PlcfAndTxt",
      )?;
    }
    patch_optional_located(
      &mut table,
      self.table.annotation_owners.as_ref(),
      AnnotationOwners::to_bytes,
      "GrpXstAtnOwners",
    )?;
    if let Some(bookmarks) = &self.table.annotation_bookmarks {
      let (infos, starts, ends) = bookmarks.value.to_bytes()?;
      patch_location(&mut table, bookmarks.infos_location, infos, "SttbfAtnBkmk")?;
      patch_location(&mut table, bookmarks.starts_location, starts, "PlcfAtnBkf")?;
      patch_location(&mut table, bookmarks.ends_location, ends, "PlcfAtnBkl")?;
    }
    patch_optional_located(
      &mut table,
      self.table.annotation_extended_data.as_ref(),
      AnnotationExtendedData::to_bytes,
      "AtrdExtra",
    )?;
    patch_part_tables(
      &mut table,
      &self.table.textbox_stories,
      TextboxStoryTable::to_bytes,
      "PlcfTxbxTxt",
    )?;
    patch_part_tables(
      &mut table,
      &self.table.textbox_breaks,
      TextboxBreakTable::to_bytes,
      "PlcfTxbxBkd",
    )?;
    patch_part_tables(
      &mut table,
      &self.table.shape_anchors,
      ShapeAnchorTable::to_bytes,
      "PlcSpa",
    )?;
    if let Some(office_art) = &self.table.office_art {
      patch_located(
        &mut table,
        office_art,
        DocOfficeArtContent::to_bytes,
        "OfficeArtContent",
      )?;
    }
    patch_optional_located(
      &mut table,
      self.table.revision_authors.as_ref(),
      RevisionAuthors::to_bytes,
      "SttbfRMark",
    )?;
    if let Some(captions) = &self.table.captions {
      patch_located(
        &mut table,
        &captions.definitions,
        CaptionDefinitions::to_bytes,
        "SttbfCaption",
      )?;
      patch_located(
        &mut table,
        &captions.automatic,
        AutoCaptionDefinitions::to_bytes,
        "SttbfAutoCaption",
      )?;
    }
    patch_optional_located(
      &mut table,
      self.table.subdocuments.as_ref(),
      SubdocumentTable::to_bytes,
      "PlcfWkb",
    )?;
    patch_optional_located(
      &mut table,
      self.table.user_variables.as_ref(),
      UserVariables::to_bytes,
      "StwUser",
    )?;
    patch_optional_located(
      &mut table,
      self.table.embedded_fonts.as_ref(),
      EmbeddedFontTable::to_bytes,
      "SttbTtmbd",
    )?;
    patch_optional_located(
      &mut table,
      self.table.spelling_state.as_ref(),
      SpellingStateTable::to_bytes,
      "PlcfSpl",
    )?;
    patch_optional_located(
      &mut table,
      self.table.grammar_state.as_ref(),
      GrammarStateTable::to_bytes,
      "PlcfGram",
    )?;
    patch_optional_located(
      &mut table,
      self.table.language_detection_state.as_ref(),
      LanguageDetectionStateTable::to_bytes,
      "PlcfLad",
    )?;
    if let Some(definitions) = &list_definitions {
      let (base, levels) = definitions.value.to_bytes()?;
      patch_location(&mut table, definitions.location, base, "PlfLst")?;
      let levels_offset = usize::try_from(definitions.location.fc)
        .ok()
        .and_then(|offset| {
          usize::try_from(definitions.location.lcb)
            .ok()
            .and_then(|length| offset.checked_add(length))
        })
        .ok_or_else(|| Error::Limit("PlfLst levels offset exceeds usize".into()))?;
      patch_at(
        &mut table,
        levels_offset,
        definitions.trailing_levels_len,
        levels,
        "PlfLst levels",
      )?;
    }
    patch_optional_located(
      &mut table,
      self.table.list_names.as_ref(),
      ListNamesTable::to_bytes,
      "SttbListNames",
    )?;
    patch_optional_located(
      &mut table,
      self.table.list_overrides.as_ref(),
      ListOverrides::to_bytes,
      "PlfLfo",
    )?;
    patch_optional_located(
      &mut table,
      self.table.document_properties.as_ref(),
      DocumentProperties::to_bytes,
      "Dop",
    )?;
    patch_optional_located(
      &mut table,
      self.table.associated_strings.as_ref(),
      AssociatedStrings::to_bytes,
      "SttbfAssoc",
    )?;
    patch_optional_located(
      &mut table,
      self.table.external_file_names.as_ref(),
      ExternalFileNameTable::to_bytes,
      "SttbFnm",
    )?;
    macro_rules! patch_table {
      ($field:ident, $type:ty, $label:literal) => {
        patch_optional_located(
          &mut table,
          self.table.$field.as_ref(),
          <$type>::to_bytes,
          $label,
        )?;
      };
    }
    patch_table!(mail_merge_state, MailMergeState, "Pms");
    patch_table!(new_mail_merge_state, MailMergeState, "PmsNew");
    patch_table!(office_data_source, OfficeDataSource, "Odso");
    patch_table!(printer_driver_info, PrinterDriverInfo, "PrDrvr");
    patch_table!(ole_control_infos, OleControlInfos, "RgxOcxInfo");
    patch_table!(table_character_cache, TableCharacterCacheTable, "PlcfTch");
    patch_table!(
      revision_message_threading,
      RevisionMessageThreading,
      "RmdThreading"
    );
    patch_table!(list_style_templates, ListStyleTemplates, "SttbRgtplc");
    patch_table!(frame_and_list_records, FrameAndListRecords, "RgDofr");
    patch_table!(grammar_option_sets, GrammarOptionSets, "PlfCosi");
    patch_table!(
      legacy_grammar_option_sets,
      LegacyGrammarOptionSets,
      "PlfGosl"
    );
    patch_table!(auto_summary_ranges, AutoSummaryRangeTable, "PlcfAsumy");
    patch_table!(
      smart_tag_recognizer_state,
      SmartTagRecognizerStateTable,
      "PlcfFactoid"
    );
    patch_table!(xml_schema_references, XmlSchemaReferences, "Hplxsdr");
    patch_table!(xml_transform_path, XmlTransformPath, "CustomXForm");
    patch_table!(
      paragraph_group_properties,
      ParagraphGroupProperties,
      "PlcfPgp"
    );
    patch_table!(save_history, SaveHistory, "SttbSavedBy");
    patch_table!(
      grammar_checker_cookies,
      GrammarCheckerCookieTable,
      "PlcfCookie"
    );
    patch_table!(
      legacy_grammar_checker_cookies,
      LegacyGrammarCheckerCookieTable,
      "PlcfCookieOld"
    );
    patch_table!(grammar_cookie_data, GrammarCookieStore, "CookieData");
    patch_table!(smart_tag_data, SmartTagData, "FactoidData");
    patch_table!(revision_save_ids, RevisionSaveIdTable, "Plrsid");
    patch_table!(selection_state, SelectionState, "Wss");
    patch_table!(command_customizations, CommandCustomizations, "Cmds");
    if let Some(value) = &self.table.structured_tag_bookmarks {
      let encoded = value.value.to_bytes()?;
      patch_location(
        &mut table,
        value.metadata_location,
        encoded.tags,
        "SttbfBkmkSdt",
      )?;
      patch_location(
        &mut table,
        value.starts_location,
        encoded.starts,
        "PlcfBkfSdt",
      )?;
      patch_location(&mut table, value.ends_location, encoded.ends, "PlcfBklSdt")?;
    }
    if let Some(value) = &self.table.range_protection {
      let encoded = value.value.to_bytes()?;
      patch_location(
        &mut table,
        value.permissions_location,
        encoded.permissions,
        "SttbfBkmkProt",
      )?;
      patch_location(
        &mut table,
        value.starts_location,
        encoded.starts,
        "PlcfBkfProt",
      )?;
      patch_location(&mut table, value.ends_location, encoded.ends, "PlcfBklProt")?;
      patch_location(
        &mut table,
        value.users_location,
        encoded.users,
        "SttbProtUser",
      )?;
    }
    if let Some(value) = &self.table.smart_tag_bookmarks {
      let (metadata, starts, ends) = value.value.to_bytes()?;
      patch_location(
        &mut table,
        value.metadata_location,
        metadata,
        "SttbfBkmkFactoid",
      )?;
      patch_location(&mut table, value.starts_location, starts, "PlcfBkfFactoid")?;
      patch_location(&mut table, value.ends_location, ends, "PlcfBklFactoid")?;
    }
    if let Some(value) = &self.table.format_consistency_bookmarks {
      let encoded = value.value.to_bytes()?;
      patch_location(
        &mut table,
        value.metadata_location,
        encoded.metadata,
        "SttbfBkmkFcc",
      )?;
      patch_location(
        &mut table,
        value.starts_location,
        encoded.starts,
        "PlcfBkfFcc",
      )?;
      patch_location(&mut table, value.ends_location, encoded.ends, "PlcfBklFcc")?;
    }
    if let Some(value) = &self.table.repair_bookmarks {
      let encoded = value.value.to_bytes()?;
      patch_location(
        &mut table,
        value.metadata_location,
        encoded.metadata,
        "SttbfBkmkBpRepairs",
      )?;
      patch_location(
        &mut table,
        value.starts_location,
        encoded.starts,
        "PlcfBkfBpRepairs",
      )?;
      patch_location(
        &mut table,
        value.ends_location,
        encoded.ends,
        "PlcfBklBpRepairs",
      )?;
    }
    if let Some(value) = &self.table.user_input_methods {
      let (methods, guids) = value.value.to_bytes()?;
      patch_location(&mut table, value.methods_location, methods, "PlcfUim")?;
      patch_location(
        &mut table,
        value.service_guids_location,
        guids,
        "PlfGuidUim",
      )?;
    }
    patch_optional_located(
      &mut table,
      self.table.mso_envelope.as_ref(),
      MsoEnvelope::to_bytes,
      "MsoEnvelope",
    )?;
    if let Some(value) = &self.table.deprecated_numbering_field_cache {
      patch_location(
        &mut table,
        value.location,
        value.physical_bytes.clone(),
        "PlcfBteLvc",
      )?;
    }

    let source_clx = Clx::from_bytes(bounded_slice(
      &self.table.physical_bytes,
      self.table.clx.location,
      "source CLX",
    )?)?;
    if self.word_document.source_piece_indices.len() != self.word_document.text_pieces.len() {
      return Err(Error::invalid(
        0,
        "DOC current/source text-piece identity cardinality changed",
      ));
    }
    if self.word_document.chpx_runs.is_none() && self.word_document.source_chpx_runs.is_some() {
      return Err(Error::invalid(0, "DOC CHPX CP tree was removed"));
    }
    if self.word_document.papx_runs.is_none() && self.word_document.source_papx_runs.is_some() {
      return Err(Error::invalid(0, "DOC PAPX CP tree was removed"));
    }
    let rebuild_character_formatting = self.word_document.rebuild_character_formatting
      || self.word_document.chpx_runs != self.word_document.source_chpx_runs;
    let rebuild_paragraph_formatting = self.word_document.rebuild_paragraph_formatting
      || self.word_document.papx_runs != self.word_document.source_papx_runs;
    let mut encoded_pieces = Vec::with_capacity(self.word_document.text_pieces.len());
    let mut text_layout_changed = false;
    for (current_piece_index, piece) in self.word_document.text_pieces.iter().enumerate() {
      let source_piece_index = self.word_document.source_piece_indices[current_piece_index];
      let bytes = piece.value.to_bytes()?;
      let expected_characters = piece
        .value
        .cp_end
        .checked_sub(piece.value.cp_start)
        .and_then(|count| usize::try_from(count).ok())
        .ok_or_else(|| {
          Error::invalid(
            u64::from(piece.value.file_offset),
            "text piece CP range is invalid",
          )
        })?;
      if piece.value.character_count() != expected_characters {
        return Err(Error::invalid(
          u64::from(piece.value.file_offset),
          "text piece character count changed",
        ));
      }
      let descriptor = source_clx
        .piece_table
        .pieces
        .get(source_piece_index)
        .ok_or_else(|| Error::invalid(0, "source text piece index is stale"))?;
      let source_cp_start = *source_clx
        .piece_table
        .character_positions
        .get(source_piece_index)
        .ok_or_else(|| Error::invalid(0, "source text piece CP start is missing"))?;
      let source_cp_end = *source_clx
        .piece_table
        .character_positions
        .get(source_piece_index + 1)
        .ok_or_else(|| Error::invalid(0, "source text piece CP limit is missing"))?;
      let source_characters = descriptor.text_piece(
        &self.word_document.physical_bytes,
        source_cp_start,
        source_cp_end,
      )?;
      let source_character_count = source_characters.character_count();
      let character_replacements = if let Some(edits) = self
        .word_document
        .pending_text_edits
        .get(&source_piece_index)
      {
        let relocated_count = relocate_character_position(
          u32::try_from(source_character_count)
            .map_err(|_| Error::Limit("source text piece character count exceeds u32".into()))?,
          edits,
          "text piece character count",
        )?;
        if usize::try_from(relocated_count).ok() != Some(expected_characters) {
          return Err(Error::invalid(
            u64::from(piece.value.file_offset),
            "text piece has an untracked variable-length edit after replace_text_range",
          ));
        }
        edits.clone()
      } else {
        vec![text_piece_character_replacement(
          &source_characters.characters,
          &piece.value.characters,
        )?]
      };
      let source_width = if descriptor.file_position.compressed {
        1usize
      } else {
        2usize
      };
      let source_len = source_character_count
        .checked_mul(source_width)
        .ok_or_else(|| Error::Limit("text piece byte length overflow".into()))?;
      text_layout_changed |= bytes.len() != source_len;
      encoded_pieces.push(EncodedTextPiece {
        piece_index: current_piece_index,
        source_offset: descriptor.file_position.byte_offset(),
        source_len,
        source_width,
        source_character_count,
        destination_character_count: expected_characters,
        destination_start: None,
        character_replacements,
        compressed: piece.value.characters.encoding() == TextPieceEncoding::Compressed,
        bytes,
      });
    }
    text_layout_changed |= rebuild_character_formatting || rebuild_paragraph_formatting;

    let section_layout_changed =
      section_properties
        .iter()
        .try_fold(false, |changed, section| -> Result<bool> {
          let Some(value) = &section.value else {
            return Ok(changed);
          };
          Ok(changed || value.to_bytes()?.len() != section.physical_len)
        })?;
    let mut word = if text_layout_changed || section_layout_changed {
      MutableWordLayout::Owned(source_word.to_vec())
    } else {
      MutableWordLayout::Overlay(TableLayout::new(source_word))
    };

    if text_layout_changed {
      let meaningful_end = usize::try_from(fib.rg_lw.cb_mac)
        .map_err(|_| Error::Limit("FIB cbMac exceeds usize".into()))?;
      if meaningful_end > word.len() {
        return Err(Error::invalid(
          u64::from(fib.rg_lw.cb_mac),
          "FIB cbMac exceeds WordDocument",
        ));
      }
      let mut source_order = encoded_pieces.iter().collect::<Vec<_>>();
      source_order.sort_by_key(|piece| piece.source_offset);
      for pair in source_order.windows(2) {
        let left_end = u64::from(pair[0].source_offset)
          .checked_add(pair[0].source_len as u64)
          .ok_or_else(|| Error::Limit("text piece source range overflow".into()))?;
        if left_end > u64::from(pair[1].source_offset) {
          return Err(Error::invalid(
            u64::from(pair[1].source_offset),
            "overlapping text pieces cannot be relocated",
          ));
        }
      }

      let appended_len = encoded_pieces.iter().try_fold(0usize, |length, piece| {
        length
          .checked_add(piece.bytes.len())
          .ok_or_else(|| Error::Limit("relocated text length overflow".into()))
      })?;
      let mut appended = Vec::with_capacity(appended_len);
      let mut relocations = Vec::with_capacity(encoded_pieces.len());
      for piece in &mut encoded_pieces {
        let new_offset = meaningful_end
          .checked_add(appended.len())
          .ok_or_else(|| Error::Limit("relocated text offset overflow".into()))?;
        let new_offset_u32 = u32::try_from(new_offset)
          .map_err(|_| Error::Limit("relocated text offset exceeds u32".into()))?;
        let new_width = if piece.compressed { 1usize } else { 2usize };
        let descriptor = clx_table
          .value
          .piece_table
          .pieces
          .get_mut(piece.piece_index)
          .ok_or_else(|| Error::invalid(0, "text piece index is stale"))?;
        descriptor.file_position.fc = if piece.compressed {
          new_offset_u32
            .checked_mul(2)
            .ok_or_else(|| Error::Limit("compressed text FC representation overflow".into()))?
        } else {
          new_offset_u32
        };
        if descriptor.file_position.fc > 0x3fff_ffff {
          return Err(Error::Limit("relocated text FC exceeds 30 bits".into()));
        }
        descriptor.file_position.compressed = piece.compressed;
        piece.destination_start = Some(new_offset_u32);
        relocations.push(TextRelocation {
          source_start: piece.source_offset,
          source_len: piece.source_len,
          source_width: piece.source_width,
          destination_start: new_offset_u32,
          destination_width: new_width,
          source_character_count: piece.source_character_count,
          destination_character_count: piece.destination_character_count,
          character_replacements: piece.character_replacements.clone(),
        });
        appended.extend_from_slice(&piece.bytes);
      }
      if !rebuild_paragraph_formatting {
        relocate_text_file_positions(&mut paragraph_bin_table.value.file_positions, &relocations)?;
        for page in &mut paragraph_format_pages {
          relocate_text_file_positions(&mut page.value.file_positions, &relocations)?;
        }
      }
      word
        .owned_mut()?
        .splice(meaningful_end..meaningful_end, appended);
      fib.rg_lw.cb_mac = fib
        .rg_lw
        .cb_mac
        .checked_add(
          u32::try_from(appended_len)
            .map_err(|_| Error::Limit("relocated text length exceeds u32".into()))?,
        )
        .ok_or_else(|| Error::Limit("FIB cbMac overflow".into()))?;
      if rebuild_character_formatting {
        let rebuilt = rebuild_character_formatting_pages(
          self
            .word_document
            .chpx_runs
            .as_deref()
            .ok_or_else(|| Error::invalid(0, "DOC CHPX CP tree is unavailable for rebuild"))?,
          &self.word_document.text_pieces,
          &encoded_pieces,
          word.owned_mut()?,
          &mut fib,
        )?;
        character_bin_table.value = rebuilt.0;
        character_format_pages = rebuilt.1;
      } else {
        relocate_text_file_positions(&mut character_bin_table.value.file_positions, &relocations)?;
        for page in &mut character_format_pages {
          relocate_text_file_positions(&mut page.value.file_positions, &relocations)?;
        }
      }
      if rebuild_paragraph_formatting {
        let rebuilt = rebuild_paragraph_formatting_pages(
          self
            .word_document
            .papx_runs
            .as_deref()
            .ok_or_else(|| Error::invalid(0, "DOC PAPX CP tree is unavailable for rebuild"))?,
          &self.word_document.text_pieces,
          &encoded_pieces,
          word.owned_mut()?,
          &mut fib,
        )?;
        paragraph_bin_table.value = rebuilt.0;
        paragraph_format_pages = rebuilt.1;
      }
    } else {
      let relocations = encoded_pieces
        .iter()
        .map(|piece| TextRelocation {
          source_start: piece.source_offset,
          source_len: piece.source_len,
          source_width: piece.source_width,
          destination_start: piece.source_offset,
          destination_width: piece.source_width,
          source_character_count: piece.source_character_count,
          destination_character_count: piece.destination_character_count,
          character_replacements: piece.character_replacements.clone(),
        })
        .collect::<Vec<_>>();
      relocate_text_file_positions(&mut character_bin_table.value.file_positions, &relocations)?;
      relocate_text_file_positions(&mut paragraph_bin_table.value.file_positions, &relocations)?;
      for page in &mut character_format_pages {
        relocate_text_file_positions(&mut page.value.file_positions, &relocations)?;
      }
      for page in &mut paragraph_format_pages {
        relocate_text_file_positions(&mut page.value.file_positions, &relocations)?;
      }
      for piece in encoded_pieces {
        patch_at(
          &mut word,
          usize::try_from(piece.source_offset)
            .map_err(|_| Error::Limit("text piece offset exceeds usize".into()))?,
          piece.source_len,
          piece.bytes,
          "text piece",
        )?;
      }
    }
    for page in &character_format_pages {
      patch_at(
        &mut word,
        page.page.byte_offset()?,
        512,
        page.value.to_bytes()?,
        "ChpxFkp",
      )?;
    }
    for page in &paragraph_format_pages {
      patch_at(
        &mut word,
        page.page.byte_offset()?,
        512,
        page.value.to_bytes()?,
        "PapxFkp",
      )?;
    }
    for section in &section_properties {
      if let Some(value) = &section.value {
        let encoded = value.to_bytes()?;
        if encoded.len() == section.physical_len {
          patch_at(
            &mut word,
            usize::try_from(section.offset)
              .map_err(|_| Error::invalid(0, "negative Sepx offset"))?,
            section.physical_len,
            encoded,
            "Sepx",
          )?;
        } else {
          let meaningful_end = usize::try_from(fib.rg_lw.cb_mac)
            .map_err(|_| Error::Limit("FIB cbMac exceeds usize".into()))?;
          if meaningful_end > word.len() {
            return Err(Error::invalid(
              u64::from(fib.rg_lw.cb_mac),
              "FIB cbMac exceeds WordDocument",
            ));
          }
          let new_offset = i32::try_from(meaningful_end)
            .map_err(|_| Error::Limit("Sepx offset exceeds i32".into()))?;
          let encoded_len = u32::try_from(encoded.len())
            .map_err(|_| Error::Limit("Sepx length exceeds u32".into()))?;
          word
            .owned_mut()?
            .splice(meaningful_end..meaningful_end, encoded);
          fib.rg_lw.cb_mac = fib
            .rg_lw
            .cb_mac
            .checked_add(encoded_len)
            .ok_or_else(|| Error::Limit("FIB cbMac overflow".into()))?;
          sections_table
            .value
            .sections
            .get_mut(section.section_index)
            .ok_or_else(|| Error::invalid(0, "section property index is stale"))?
            .sepx_offset = new_offset;
        }
      }
    }

    patch_located(&mut table, &clx_table, Clx::to_bytes, "CLX")?;
    patch_located(
      &mut table,
      &character_bin_table,
      PlcBte::to_bytes,
      "PlcBteChpx",
    )?;
    patch_located(
      &mut table,
      &paragraph_bin_table,
      PlcBte::to_bytes,
      "PlcBtePapx",
    )?;
    patch_located(&mut table, &sections_table, PlcfSed::to_bytes, "PlcfSed")?;
    let (table, relocation) = table.finish_plan()?;
    fib.relocate_table_locations(|location| relocation.relocate(location))?;
    patch_prefix(
      &mut word,
      self.word_document.source_fib_len,
      fib.to_bytes()?,
      "FIB",
    )?;
    let word = word.finish()?;

    let mut compound = self.compound_file.clone();
    match data_plan.as_ref() {
      Some(_) if !compound.is_stream(DATA_STREAM_PATH) => {
        compound.upsert_stream(DATA_STREAM_PATH, Vec::new())?;
      }
      None if compound.is_stream(DATA_STREAM_PATH) => {
        compound.remove_stream(DATA_STREAM_PATH)?;
      }
      Some(_) | None => {}
    }
    if let Some(object_pool) = &self.object_pool {
      for object in &object_pool.objects {
        compound.overwrite_stream(&object.descriptor_stream_path, object.descriptor.to_bytes())?;
      }
    }
    self.shared.write_to_compound_file(&mut compound, options)?;
    Ok(DocCompoundWritePlan {
      compound,
      word,
      table_path: self.table.name.path(),
      table: DocStreamWritePlan::Overlay(table),
      data: data_plan.map(DocStreamWritePlan::Overlay),
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.to_bytes_with_options(SaveOptions::default())
  }

  pub fn to_bytes_preserving_compatibility(&self) -> Result<Vec<u8>> {
    self.to_bytes_with_options(SaveOptions::preserving_compatibility())
  }

  pub fn to_bytes_with_options(&self, options: SaveOptions) -> Result<Vec<u8>> {
    let bytes = self
      .compound_write_plan_with_current_layout(options)?
      .to_bytes()?;
    Self::validate_emitted_bytes(bytes, options)
  }

  pub fn write_to(&self, mut writer: impl Write) -> Result<()> {
    self.write_to_with_options(&mut writer, SaveOptions::default())
  }

  pub fn write_to_preserving_compatibility(&self, writer: impl Write) -> Result<()> {
    self.write_to_with_options(writer, SaveOptions::preserving_compatibility())
  }

  pub fn write_to_with_options(&self, mut writer: impl Write, options: SaveOptions) -> Result<()> {
    if options.preserves_compatibility() {
      return self
        .compound_write_plan_with_current_layout(options)?
        .write_to(writer);
    }
    writer.write_all(&self.to_bytes_with_options(options)?)?;
    Ok(())
  }

  pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
    self.save_with_options(path, SaveOptions::default())
  }

  pub fn save_preserving_compatibility(&self, path: impl AsRef<Path>) -> Result<()> {
    self.save_with_options(path, SaveOptions::preserving_compatibility())
  }

  pub fn save_with_options(&self, path: impl AsRef<Path>, options: SaveOptions) -> Result<()> {
    if options.preserves_compatibility() {
      let plan = self.compound_write_plan_with_current_layout(options)?;
      plan.write_to(std::io::sink())?;
      return plan.write_to(fs::File::create(path)?);
    }
    fs::write(path, self.to_bytes_with_options(options)?)?;
    Ok(())
  }

  /// Replaces a character range in the Main Document and relocates every
  /// currently materialized CP reference whose coordinate space is affected.
  ///
  /// The range is relative to the Main Document, as specified by MS-DOC
  /// section 2.3.1. The replacement is a Rust string; a compressed source
  /// piece is retained when every scalar fits U+00FF and is transactionally
  /// upgraded to UTF-16 otherwise. CHPX and PAPX boundaries are rebuilt from
  /// logical runs; the paragraph/cell/section terminator sequence must stay
  /// unchanged so paragraph formatting inheritance is unambiguous. A
  /// cross-piece edit removes any emptied PlcPcd descriptors and rebuilds
  /// the logical formatting tables before serialization.
  pub fn replace_main_text_range(
    &mut self,
    range: Range<u32>,
    replacement: impl Into<String>,
  ) -> Result<()> {
    self.replace_text_range(FieldDocumentPart::Main, range, replacement)
  }

  /// Replaces a character range whose CPs are relative to an MS-DOC
  /// document part and relocates both global and part-local CP references.
  pub fn replace_text_range(
    &mut self,
    part: FieldDocumentPart,
    range: Range<u32>,
    replacement: impl Into<String>,
  ) -> Result<()> {
    let mut edited = self.clone();
    let replacement = edited.encode_text_replacement(part, &range, replacement.into())?;
    edited.replace_text_range_composed(part, range, replacement, ParagraphMarkEdit::PreserveAll)?;
    edited.realign_papx_runs()?;
    *self = edited;
    Ok(())
  }

  /// Replaces Main Document text while allowing paragraph-mark (0x000D)
  /// insertion or deletion. `papx_runs` is the complete PAPX FKP CP tree in
  /// the post-edit global coordinate space; no formatting inheritance is
  /// inferred. Cell marks (0x0007) and section marks (0x000C) must remain
  /// unchanged because their other owning structures are not supplied here.
  pub fn replace_main_text_range_with_papx_runs(
    &mut self,
    range: Range<u32>,
    replacement: impl Into<String>,
    papx_runs: Vec<DocPapxRun>,
  ) -> Result<()> {
    self.replace_text_range_with_papx_runs(FieldDocumentPart::Main, range, replacement, papx_runs)
  }

  /// Replaces text in an MS-DOC document part while allowing paragraph-mark
  /// (0x000D) insertion or deletion. `papx_runs` is the complete PAPX FKP CP
  /// tree in the post-edit global coordinate space; no formatting
  /// inheritance is inferred. The target part's official story/note/comment/
  /// textbox boundaries and guard characters are validated transactionally.
  /// Cell marks (0x0007) and section marks (0x000C) must remain unchanged.
  pub fn replace_text_range_with_papx_runs(
    &mut self,
    part: FieldDocumentPart,
    range: Range<u32>,
    replacement: impl Into<String>,
    papx_runs: Vec<DocPapxRun>,
  ) -> Result<()> {
    let mut edited = self.clone();
    let replacement = edited.encode_text_replacement(part, &range, replacement.into())?;
    edited.replace_text_range_composed(
      part,
      range,
      replacement,
      ParagraphMarkEdit::ExplicitPapx,
    )?;
    Arc::make_mut(&mut edited.word_document).papx_runs = Some(papx_runs);
    edited.validate_current_papx_runs()?;
    edited.validate_document_part_structure(part)?;
    *self = edited;
    Ok(())
  }

  fn encode_text_replacement(
    &self,
    part: FieldDocumentPart,
    range: &Range<u32>,
    replacement: String,
  ) -> Result<TextPieceCharacters> {
    if range.start > range.end {
      return Err(Error::invalid(0, "DOC text replacement range is reversed"));
    }
    let (part_start, part_len) = document_part_range(&self.word_document.fib, part)?;
    if range.end > part_len {
      return Err(Error::invalid(
        u64::from(range.end),
        "DOC text replacement exceeds its document part",
      ));
    }
    let global_start = part_start
      .checked_add(range.start)
      .ok_or_else(|| Error::Limit("DOC global text edit start overflow".into()))?;
    let source = self
      .word_document
      .text_pieces
      .iter()
      .find(|piece| {
        let Ok(start) = u32::try_from(piece.value.cp_start) else {
          return false;
        };
        let Ok(end) = u32::try_from(piece.value.cp_end) else {
          return false;
        };
        start <= global_start
          && (global_start < end || (range.start == range.end && global_start == end))
      })
      .ok_or_else(|| {
        Error::invalid(
          u64::from(global_start),
          "DOC text replacement has no containing text piece",
        )
      })?;
    match source.value.characters.encoding() {
      TextPieceEncoding::Utf16 => Ok(TextPieceCharacters::utf16(replacement)),
      TextPieceEncoding::Compressed => TextPieceCharacters::compressed(replacement.clone())
        .or_else(|_| Ok(TextPieceCharacters::utf16(replacement))),
    }
  }

  /// Resolves only the direct paragraph-formatting layers at a part-local
  /// CP. This path deliberately does not require a CHPX tree, which keeps
  /// paragraph/table navigation independent from character formatting.
  pub fn direct_paragraph_formatting_at_cp(
    &self,
    part: FieldDocumentPart,
    local_cp: u32,
  ) -> Result<DocDirectParagraphFormatting> {
    self
      .direct_paragraph_formatting_at_cp_with_piece(part, local_cp)
      .map(|(_, _, _, paragraph)| paragraph)
  }

  fn direct_paragraph_formatting_at_cp_with_piece(
    &self,
    part: FieldDocumentPart,
    local_cp: u32,
  ) -> Result<(u32, usize, GrpPrl, DocDirectParagraphFormatting)> {
    let (part_start, part_len) = document_part_range(&self.word_document.fib, part)?;
    if local_cp >= part_len {
      return Err(Error::invalid(
        u64::from(local_cp),
        "direct-formatting CP exceeds its MS-DOC document part",
      ));
    }
    let global_cp = part_start
      .checked_add(local_cp)
      .ok_or_else(|| Error::Limit("DOC direct-formatting CP overflow".into()))?;
    let (piece_index, _) = self
      .word_document
      .text_pieces
      .iter()
      .enumerate()
      .find(|(_, piece)| {
        u32::try_from(piece.value.cp_start).is_ok_and(|start| start <= global_cp)
          && u32::try_from(piece.value.cp_end).is_ok_and(|end| global_cp < end)
      })
      .ok_or_else(|| {
        Error::invalid(
          u64::from(global_cp),
          "direct-formatting CP has no containing PlcPcd text piece",
        )
      })?;
    let descriptor = self
      .table
      .clx
      .value
      .piece_table
      .pieces
      .get(piece_index)
      .ok_or_else(|| {
        Error::invalid(
          u64::try_from(piece_index).unwrap_or(u64::MAX),
          "direct-formatting text piece has no Pcd descriptor",
        )
      })?;
    let piece_properties = descriptor
      .property_modifier
      .property_modifications(&self.table.clx.value)?;

    let papx = self
      .word_document
      .papx_runs
      .as_ref()
      .ok_or_else(|| Error::invalid(0, "DOC PAPX CP tree is unavailable"))?
      .iter()
      .find(|run| run.cp_start <= global_cp && global_cp < run.cp_end)
      .ok_or_else(|| {
        Error::invalid(
          u64::from(global_cp),
          "direct-formatting CP has no containing PAPX run",
        )
      })?;
    let (style_index, papx_properties) = papx
      .properties
      .as_ref()
      .map(|properties| (properties.style_index, properties.properties.clone()))
      .unwrap_or_else(|| {
        (
          0,
          GrpPrl {
            properties: Vec::new(),
          },
        )
      });
    let piece_paragraph_properties = grpprl_for_group(&piece_properties, SprmGroup::Paragraph);
    let mut applied_paragraph_properties = expand_direct_paragraph_properties(
      &papx_properties,
      self.data.as_deref(),
      Some(style_index),
    )?;
    applied_paragraph_properties.properties.extend(
      expand_direct_paragraph_properties(&piece_paragraph_properties, self.data.as_deref(), None)?
        .properties,
    );

    Ok((
      global_cp,
      piece_index,
      piece_properties,
      DocDirectParagraphFormatting {
        style_index,
        papx_properties,
        piece_properties: piece_paragraph_properties,
        applied_properties: applied_paragraph_properties,
      },
    ))
  }

  /// Resolves the direct paragraph and character formatting at a CP that is
  /// local to an MS-DOC document part.
  ///
  /// The result follows MS-DOC 2.4.6.1: PAPX/CHPX properties come first and
  /// paragraph/character properties selected by the containing Pcd.Prm come
  /// second. It deliberately stops before style, list, and table-style
  /// evaluation so every returned Rust node still has one physical owner.
  pub fn direct_formatting_at_cp(
    &self,
    part: FieldDocumentPart,
    local_cp: u32,
  ) -> Result<DocDirectFormatting> {
    let (global_cp, piece_index, piece_properties, paragraph) =
      self.direct_paragraph_formatting_at_cp_with_piece(part, local_cp)?;
    let chpx = self
      .word_document
      .chpx_runs
      .as_ref()
      .ok_or_else(|| Error::invalid(0, "DOC CHPX CP tree is unavailable"))?
      .iter()
      .find(|run| run.cp_start <= global_cp && global_cp < run.cp_end)
      .ok_or_else(|| {
        Error::invalid(
          u64::from(global_cp),
          "direct-formatting CP has no containing CHPX run",
        )
      })?;

    Ok(DocDirectFormatting {
      part,
      local_cp,
      global_cp,
      piece_index,
      paragraph,
      character: DocDirectCharacterFormatting {
        chpx_properties: chpx
          .properties
          .as_deref()
          .cloned()
          .unwrap_or_else(|| GrpPrl {
            properties: Vec::new(),
          }),
        piece_properties: grpprl_for_group(&piece_properties, SprmGroup::Character),
      },
    })
  }

  /// Resolves an STSH style hierarchy into base-first property arrays as
  /// specified by MS-DOC 2.4.6.5.
  pub fn style_properties(&self, style_index: u16) -> Result<DocStyleProperties> {
    let styles = self
      .table
      .styles
      .as_ref()
      .ok_or_else(|| Error::invalid(0, "DOC has no STSH style sheet"))?;
    let mut active = BTreeSet::new();
    let mut lineage = Vec::new();
    let mut paragraph_properties = Vec::new();
    let mut character_properties = Vec::new();
    let mut table_properties = Vec::new();
    collect_style_properties(
      &styles.value,
      style_index,
      &mut StylePropertyAccumulator {
        active: &mut active,
        lineage: &mut lineage,
        paragraph: &mut paragraph_properties,
        character: &mut character_properties,
        table: &mut table_properties,
      },
    )?;
    let definition = styles
      .value
      .styles
      .get(usize::from(style_index))
      .and_then(|style| style.definition.as_ref())
      .ok_or_else(|| {
        Error::invalid(
          u64::from(style_index),
          "requested STSH style is unavailable",
        )
      })?;
    Ok(DocStyleProperties {
      style_index,
      style_kind: definition.base.style_kind,
      lineage,
      paragraph_properties: GrpPrl {
        properties: paragraph_properties,
      },
      character_properties: GrpPrl {
        properties: character_properties,
      },
      table_properties: GrpPrl {
        properties: table_properties,
      },
    })
  }

  /// Determines the effective sprmCFSpec Boolean at a document-part CP.
  /// The paragraph style hierarchy is applied first, followed by CHPX and
  /// Pcd.Prm direct character formatting. Toggle operands 0x80/0x81 retain
  /// their normative relationship to the current style value.
  pub fn effective_cf_spec_at_cp(&self, part: FieldDocumentPart, local_cp: u32) -> Result<bool> {
    let direct = self.direct_formatting_at_cp(part, local_cp)?;
    let style_index = effective_paragraph_style_index(&direct.paragraph)?;
    let style = self.style_properties(style_index)?;
    if style.style_kind != super::StyleKind::Paragraph {
      return Err(Error::invalid(
        u64::from(style_index),
        "paragraph formatting references a non-paragraph style",
      ));
    }
    let style_value = apply_cf_spec_toggles(
      &style.character_properties,
      false,
      false,
      "style character properties",
    )?;
    let value = apply_cf_spec_toggles(
      &direct.character.chpx_properties,
      style_value,
      style_value,
      "CHPX character properties",
    )?;
    apply_cf_spec_toggles(
      &direct.character.piece_properties,
      value,
      style_value,
      "Pcd.Prm character properties",
    )
  }

  /// Validates the text and boundary invariants owned by one MS-DOC
  /// document part. CPs stored in the part's PLCs are interpreted relative
  /// to that part, while text lookup is performed in the aggregate global CP
  /// space. Direct paragraph formatting is assembled from PAPX, Pcd.Prm, and
  /// referenced PrcData so comment-ending table depth can be validated. STSH
  /// inheritance and direct character formatting are also evaluated for the
  /// comment marker's effective sprmCFSpec. This method does not claim to
  /// produce the complete list/table-style/conditional formatting state for
  /// arbitrary content.
  pub fn validate_document_part_structure(&self, part: FieldDocumentPart) -> Result<()> {
    self.validate_main_document_final_paragraph_mark()?;
    if part == FieldDocumentPart::Main {
      return Ok(());
    }
    if part == FieldDocumentPart::Macro {
      return Err(Error::invalid(
        0,
        "the macro field table is not an MS-DOC document part",
      ));
    }
    self.validate_additional_document_paragraph_mark()?;
    let (part_start, part_len) = document_part_range(&self.word_document.fib, part)?;
    match part {
      FieldDocumentPart::Header => self.validate_header_document(part_start, part_len),
      FieldDocumentPart::Footnote | FieldDocumentPart::Endnote => {
        self.validate_note_document(part, part_start, part_len)
      }
      FieldDocumentPart::Comment => self.validate_comment_document(part_start, part_len),
      FieldDocumentPart::Textbox | FieldDocumentPart::HeaderTextbox => {
        self.validate_textbox_document(part, part_start, part_len)
      }
      FieldDocumentPart::Main | FieldDocumentPart::Macro => unreachable!(),
    }
  }

  fn realign_papx_runs(&mut self) -> Result<()> {
    if self.word_document.papx_runs.is_none() {
      if self.word_document.rebuild_paragraph_formatting {
        return Err(Error::invalid(
          0,
          "DOC PAPX CP tree is unavailable for a structural text edit",
        ));
      }
      return Ok(());
    }
    let word_document = Arc::make_mut(&mut self.word_document);
    let ranges = current_paragraph_ranges(&word_document.text_pieces)?;
    let runs = word_document
      .papx_runs
      .as_mut()
      .expect("PAPX runs were checked above");
    if runs.len() != ranges.len() {
      return Err(Error::invalid(
        0,
        "paragraph terminator cardinality changed during PAPX realignment",
      ));
    }
    for (run, (cp_start, cp_end)) in runs.iter_mut().zip(ranges) {
      run.cp_start = cp_start;
      run.cp_end = cp_end;
    }
    Ok(())
  }

  fn validate_current_papx_runs(&self) -> Result<()> {
    let runs = self
      .word_document
      .papx_runs
      .as_ref()
      .ok_or_else(|| Error::invalid(0, "DOC PAPX CP tree is unavailable"))?;
    let ranges = current_paragraph_ranges(&self.word_document.text_pieces)?;
    if runs.len() != ranges.len()
      || runs
        .iter()
        .zip(ranges)
        .any(|(run, range)| (run.cp_start, run.cp_end) != range)
    {
      return Err(Error::invalid(
        0,
        "supplied PAPX CP tree does not match post-edit paragraph ranges",
      ));
    }
    Ok(())
  }

  fn validate_main_document_final_paragraph_mark(&self) -> Result<()> {
    let main_len = u32::try_from(self.word_document.fib.rg_lw.ccp_text)
      .map_err(|_| Error::invalid(0, "Main Document character count is negative"))?;
    if main_len == 0 || text_value_at_cp(&self.word_document.text_pieces, main_len - 1)? != 0x000d {
      return Err(Error::invalid(
        u64::from(main_len),
        "Main Document must end with a paragraph mark",
      ));
    }
    Ok(())
  }

  fn validate_additional_document_paragraph_mark(&self) -> Result<()> {
    let parts = document_part_lengths(&self.word_document.fib)?;
    if !parts.iter().skip(1).any(|(_, length)| *length != 0) {
      return Ok(());
    }
    let end = parts.iter().try_fold(0u32, |total, (_, length)| {
      total
        .checked_add(*length)
        .ok_or_else(|| Error::Limit("DOC document-part CP limit overflow".into()))
    })?;
    if text_value_at_cp(&self.word_document.text_pieces, end)? != 0x000d {
      return Err(Error::invalid(
        u64::from(end),
        "non-empty secondary document parts require an additional paragraph mark",
      ));
    }
    Ok(())
  }

  fn validate_header_document(&self, part_start: u32, part_len: u32) -> Result<()> {
    let Some(table) = &self.table.header_text else {
      return if part_len == 0 {
        Ok(())
      } else {
        Err(Error::invalid(
          0,
          "non-empty Header Document has no PlcfHdd",
        ))
      };
    };
    if part_len == 0 {
      return Err(Error::invalid(0, "empty Header Document has a PlcfHdd"));
    }
    if table.value.boundaries.len() < 2 {
      return Err(Error::invalid(0, "PlcfHdd has fewer than two terminal CPs"));
    }
    let story_count = table.value.boundaries.len() - 2;
    let expected_story_count = self
      .table
      .sections
      .value
      .sections
      .len()
      .checked_mul(6)
      .and_then(|count| count.checked_add(6))
      .ok_or_else(|| Error::Limit("Header Document story count overflow".into()))?;
    if story_count != expected_story_count {
      return Err(Error::invalid(
        0,
        format!(
          "PlcfHdd contains {story_count} stories; {expected_story_count} are required for the section table"
        ),
      ));
    }
    let positions = table.value.boundaries[..table.value.boundaries.len() - 1]
      .iter()
      .map(|boundary| match boundary {
        super::HeaderStoryBoundary::Position(value) => Ok(*value),
        super::HeaderStoryBoundary::Missing => Err(Error::invalid(
          0,
          "PlcfHdd has an undefined CP before its final ignored CP",
        )),
      })
      .collect::<Result<Vec<_>>>()?;
    validate_nondecreasing_part_positions(&positions, part_len, "PlcfHdd CP")?;
    if positions.last().copied() != Some(part_len - 1) {
      return Err(Error::invalid(
        0,
        "PlcfHdd second-to-last CP is not ccpHdd - 1",
      ));
    }
    for (index, range) in positions.windows(2).enumerate() {
      let [start, end] = [range[0], range[1]];
      if start == end {
        continue;
      }
      require_part_character(
        &self.word_document.text_pieces,
        part_start,
        end - 1,
        0x000d,
        "Header Document story guard",
      )?;
      if index >= 6 {
        if end - start < 2 {
          return Err(Error::invalid(
            u64::from(part_start + start),
            "non-empty header/footer story lacks a content paragraph mark and guard",
          ));
        }
        require_part_character(
          &self.word_document.text_pieces,
          part_start,
          end - 2,
          0x000d,
          "header/footer content paragraph mark",
        )?;
      }
    }
    Ok(())
  }

  fn validate_note_document(
    &self,
    part: FieldDocumentPart,
    part_start: u32,
    part_len: u32,
  ) -> Result<()> {
    let tables = match part {
      FieldDocumentPart::Footnote => self.table.footnotes.as_ref(),
      FieldDocumentPart::Endnote => self.table.endnotes.as_ref(),
      _ => unreachable!(),
    };
    let Some(tables) = tables else {
      return if part_len == 0 {
        Ok(())
      } else {
        Err(Error::invalid(
          0,
          format!("non-empty {part:?} Document has no text/reference PLCs"),
        ))
      };
    };
    if part_len == 0 {
      return Err(Error::invalid(
        0,
        format!("empty {part:?} Document has text/reference PLCs"),
      ));
    }
    let positions = &tables.text.value.positions;
    if positions.len() < 2 || positions.len() - 2 != tables.references.value.indices.len() {
      return Err(Error::invalid(
        0,
        format!("{part:?} text/reference cardinality differs"),
      ));
    }
    validate_strict_part_positions(
      &positions[..positions.len() - 1],
      part_len,
      &format!("{part:?} text CP"),
    )?;
    if positions[positions.len() - 2] != part_len - 1 {
      return Err(Error::invalid(
        0,
        format!("{part:?} second-to-last text CP is not ccp - 1"),
      ));
    }
    for range in positions[..positions.len() - 1].windows(2) {
      require_part_character(
        &self.word_document.text_pieces,
        part_start,
        range[1] - 1,
        0x000d,
        &format!("{part:?} range paragraph mark"),
      )?;
    }
    Ok(())
  }

  fn validate_comment_document(&self, part_start: u32, part_len: u32) -> Result<()> {
    let Some(tables) = &self.table.annotations else {
      return if part_len == 0 {
        Ok(())
      } else {
        Err(Error::invalid(
          0,
          "non-empty Comment Document has no PlcfandTxt/PlcfandRef",
        ))
      };
    };
    if part_len == 0 {
      return Err(Error::invalid(
        0,
        "empty Comment Document has PlcfandTxt/PlcfandRef",
      ));
    }
    let positions = &tables.text.value.positions;
    if positions.len() < 2 || positions.len() - 2 != tables.references.value.annotations.len() {
      return Err(Error::invalid(
        0,
        "comment text/reference cardinality differs",
      ));
    }
    validate_strict_part_positions(&positions[..positions.len() - 1], part_len, "PlcfandTxt CP")?;
    if positions[positions.len() - 2] != part_len - 1 {
      return Err(Error::invalid(
        0,
        "PlcfandTxt second-to-last CP is not ccpAtn - 1",
      ));
    }
    for range in positions[..positions.len() - 1].windows(2) {
      require_part_character(
        &self.word_document.text_pieces,
        part_start,
        range[0],
        0x0005,
        "comment range marker",
      )?;
      if !self.effective_cf_spec_at_cp(FieldDocumentPart::Comment, range[0])? {
        return Err(Error::invalid(
          u64::from(part_start + range[0]),
          "comment range marker does not have effective sprmCFSpec=1",
        ));
      }
      require_part_character(
        &self.word_document.text_pieces,
        part_start,
        range[1] - 1,
        0x000d,
        "comment range paragraph mark",
      )?;
      let table_state = self
        .direct_formatting_at_cp(FieldDocumentPart::Comment, range[1] - 1)?
        .paragraph
        .table_state()?;
      if table_state.in_table || table_state.depth != 0 {
        return Err(Error::invalid(
          u64::from(part_start + range[1] - 1),
          "comment range does not end at table depth zero",
        ));
      }
    }
    Ok(())
  }

  fn validate_textbox_document(
    &self,
    part: FieldDocumentPart,
    part_start: u32,
    part_len: u32,
  ) -> Result<()> {
    let key = match part {
      FieldDocumentPart::Textbox => TextboxDocumentPart::Main,
      FieldDocumentPart::HeaderTextbox => TextboxDocumentPart::Header,
      _ => unreachable!(),
    };
    let Some(table) = self.table.textbox_stories.get(&key) else {
      return if part_len == 0 {
        Ok(())
      } else {
        Err(Error::invalid(
          0,
          format!("non-empty {part:?} Document has no textbox story PLC"),
        ))
      };
    };
    if part_len == 0 {
      return Err(Error::invalid(
        0,
        format!("empty {part:?} Document has a textbox story PLC"),
      ));
    }
    let positions = &table.value.positions;
    if positions.len() != table.value.stories.len().saturating_add(1)
      || table.value.stories.is_empty()
    {
      return Err(Error::invalid(
        0,
        format!("{part:?} textbox CP/FTXBXS cardinality differs"),
      ));
    }
    validate_strict_textbox_positions(positions, part_len, &format!("{part:?} textbox CP"))?;
    for range in positions.windows(2).take(table.value.stories.len() - 1) {
      require_part_character(
        &self.word_document.text_pieces,
        part_start,
        range[1] - 1,
        0x000d,
        &format!("{part:?} textbox range paragraph mark"),
      )?;
    }
    Ok(())
  }

  fn replace_text_range_composed(
    &mut self,
    part: FieldDocumentPart,
    range: Range<u32>,
    replacement: TextPieceCharacters,
    paragraph_marks: ParagraphMarkEdit,
  ) -> Result<()> {
    if range.start > range.end {
      return Err(Error::invalid(0, "DOC text replacement range is reversed"));
    }
    let (part_start, part_len) = document_part_range(&self.word_document.fib, part)?;
    if range.end > part_len {
      return Err(Error::invalid(
        u64::from(range.end),
        "DOC text replacement exceeds its document part",
      ));
    }
    let global_start = part_start
      .checked_add(range.start)
      .ok_or_else(|| Error::Limit("DOC global text edit start overflow".into()))?;
    let global_end = part_start
      .checked_add(range.end)
      .ok_or_else(|| Error::Limit("DOC global text edit limit overflow".into()))?;
    let removed_terminators = paragraph_terminators_in_piece_range(
      &self.word_document.text_pieces,
      global_start,
      global_end,
    )?;
    let replacement_terminators = paragraph_terminators(&replacement);
    let terminators_match = match paragraph_marks {
      ParagraphMarkEdit::PreserveAll => removed_terminators == replacement_terminators,
      ParagraphMarkEdit::ExplicitPapx => {
        non_paragraph_terminators(&removed_terminators)
          == non_paragraph_terminators(&replacement_terminators)
      }
    };
    if !terminators_match {
      return Err(Error::invalid(
        u64::from(global_start),
        match paragraph_marks {
          ParagraphMarkEdit::PreserveAll => {
            "DOC text replacement changes the paragraph/cell/section terminator sequence"
          }
          ParagraphMarkEdit::ExplicitPapx => {
            "DOC explicit PAPX replacement changes a cell or section mark"
          }
        },
      ));
    }
    let segments = self
      .word_document
      .text_pieces
      .iter()
      .filter_map(|piece| {
        let start = u32::try_from(piece.value.cp_start).ok()?;
        let end = u32::try_from(piece.value.cp_end).ok()?;
        let overlap_start = global_start.max(start);
        let overlap_end = global_end.min(end);
        (overlap_start < overlap_end).then_some((
          overlap_start,
          overlap_end,
          piece.value.characters.encoding() == TextPieceEncoding::Compressed,
        ))
      })
      .collect::<Vec<_>>();
    if segments.len() <= 1 {
      return self.replace_text_range_inner(part, range, replacement);
    }
    for (index, (start, end, compressed)) in segments.iter().copied().enumerate().rev() {
      let piece_replacement = if index == 0 {
        replacement.clone()
      } else if compressed {
        TextPieceCharacters::compressed(String::new())?
      } else {
        TextPieceCharacters::utf16(String::new())
      };
      self.replace_text_range_inner(
        part,
        (start - part_start)..(end - part_start),
        piece_replacement,
      )?;
    }
    Ok(())
  }

  fn replace_text_range_inner(
    &mut self,
    part: FieldDocumentPart,
    range: Range<u32>,
    replacement: TextPieceCharacters,
  ) -> Result<()> {
    if range.start > range.end {
      return Err(Error::invalid(0, "DOC text replacement range is reversed"));
    }
    if !self.table.compatibility_tables.is_empty() {
      return Err(Error::invalid(
        0,
        "DOC text relocation cannot preserve opaque compatibility tables",
      ));
    }
    if matches!(
      self.word_document.fib.version(),
      super::FibVersion::Compatibility(_)
    ) {
      return Err(Error::invalid(
        0,
        "DOC text relocation requires a documented FIB version",
      ));
    }
    let (part_start, part_len) = document_part_range(&self.word_document.fib, part)?;
    if range.end > part_len {
      return Err(Error::invalid(
        u64::from(range.end),
        "DOC text replacement exceeds its document part",
      ));
    }
    let global_start = part_start
      .checked_add(range.start)
      .ok_or_else(|| Error::Limit("DOC global text edit start overflow".into()))?;
    let global_end = part_start
      .checked_add(range.end)
      .ok_or_else(|| Error::Limit("DOC global text edit limit overflow".into()))?;
    let source_clx = Clx::from_bytes(bounded_slice(
      &self.table.physical_bytes,
      self.table.clx.location,
      "source CLX",
    )?)?;
    let piece_index = self
      .word_document
      .text_pieces
      .iter()
      .position(|piece| {
        let start = u32::try_from(piece.value.cp_start).ok();
        let end = u32::try_from(piece.value.cp_end).ok();
        start.is_some_and(|start| start <= global_start) && end.is_some_and(|end| global_end <= end)
      })
      .ok_or_else(|| {
        Error::invalid(
          u64::from(global_start),
          "DOC text replacement crosses a text-piece boundary",
        )
      })?;
    let source_piece_index = *self
      .word_document
      .source_piece_indices
      .get(piece_index)
      .ok_or_else(|| Error::invalid(0, "source text piece identity is missing"))?;
    let piece = &self.word_document.text_pieces[piece_index].value;
    let source_piece_start = *source_clx
      .piece_table
      .character_positions
      .get(source_piece_index)
      .ok_or_else(|| Error::invalid(0, "source text piece CP start is missing"))?;
    let source_piece_end = *source_clx
      .piece_table
      .character_positions
      .get(source_piece_index + 1)
      .ok_or_else(|| Error::invalid(0, "source text piece CP limit is missing"))?;
    let source_piece_count = source_piece_end
      .checked_sub(source_piece_start)
      .and_then(|value| usize::try_from(value).ok())
      .ok_or_else(|| Error::invalid(0, "source text piece CP range is invalid"))?;
    let prior_edits = self
      .word_document
      .pending_text_edits
      .get(&source_piece_index)
      .cloned()
      .unwrap_or_default();
    let current_piece_count = relocate_character_position(
      u32::try_from(source_piece_count)
        .map_err(|_| Error::Limit("source text piece character count exceeds u32".into()))?,
      &prior_edits,
      "text piece character count",
    )?;
    if usize::try_from(current_piece_count).ok() != Some(piece.character_count()) {
      return Err(Error::invalid(
        u64::from(global_start),
        "text piece has an untracked variable-length edit after replace_text_range",
      ));
    }
    let piece_start = u32::try_from(piece.cp_start)
      .map_err(|_| Error::invalid(0, "text piece begins at a negative CP"))?;
    let local_start = usize::try_from(global_start - piece_start)
      .map_err(|_| Error::Limit("text replacement start exceeds usize".into()))?;
    let local_end = usize::try_from(global_end - piece_start)
      .map_err(|_| Error::Limit("text replacement end exceeds usize".into()))?;
    let replacement_len = replacement.character_count();
    let local_edit = CpReplacement::new(
      u32::try_from(local_start)
        .map_err(|_| Error::Limit("text replacement start exceeds u32".into()))?,
      u32::try_from(local_end)
        .map_err(|_| Error::Limit("text replacement end exceeds u32".into()))?,
      u32::try_from(replacement_len)
        .map_err(|_| Error::Limit("replacement character count exceeds u32".into()))?,
    )?;
    let source_descriptor = source_clx
      .piece_table
      .pieces
      .get(source_piece_index)
      .ok_or_else(|| Error::invalid(0, "source text piece descriptor is missing"))?;
    let rebuild_formatting = validate_formatting_edit(
      self,
      source_descriptor.file_position.byte_offset(),
      if source_descriptor.file_position.compressed {
        1
      } else {
        2
      },
      source_piece_count,
      &prior_edits,
      &local_edit,
    )?;
    let word_document = Arc::make_mut(&mut self.word_document);
    let table = Arc::make_mut(&mut self.table);
    let piece = &mut word_document.text_pieces[piece_index].value;
    piece
      .characters
      .replace_code_unit_range(local_start..local_end, &replacement)
      .map_err(|error| {
        Error::invalid(
          u64::from(range.start),
          format!("DOC text replacement failed: {error}"),
        )
      })?;
    let remove_piece = piece.character_count() == 0;
    if remove_piece && word_document.text_pieces.len() == 1 {
      return Err(Error::invalid(
        u64::from(global_start),
        "DOC text replacement would remove the final PlcPcd text piece",
      ));
    }
    let replacement_len = u32::try_from(replacement_len)
      .map_err(|_| Error::Limit("DOC replacement character count exceeds u32".into()))?;
    let global_edit = CpReplacement::new(global_start, global_end, replacement_len)?;
    let part_edit = CpReplacement::new(range.start, range.end, replacement_len)?;

    for position in table
      .clx
      .value
      .piece_table
      .character_positions
      .iter_mut()
      .skip(piece_index + 1)
    {
      *position = global_edit.relocate_i32(*position, "PlcPcd CP")?;
    }
    for text_piece in word_document.text_pieces.iter_mut().skip(piece_index) {
      if text_piece.piece_index == piece_index {
        text_piece.value.cp_end =
          global_edit.relocate_i32(text_piece.value.cp_end, "text piece CP limit")?;
      } else {
        text_piece.value.cp_start =
          global_edit.relocate_i32(text_piece.value.cp_start, "text piece CP start")?;
        text_piece.value.cp_end =
          global_edit.relocate_i32(text_piece.value.cp_end, "text piece CP limit")?;
      }
    }
    if remove_piece {
      table.clx.value.piece_table.pieces.remove(piece_index);
      table
        .clx
        .value
        .piece_table
        .character_positions
        .remove(piece_index + 1);
      word_document.text_pieces.remove(piece_index);
      word_document.source_piece_indices.remove(piece_index);
      for (index, piece) in word_document
        .text_pieces
        .iter_mut()
        .enumerate()
        .skip(piece_index)
      {
        piece.piece_index = index;
      }
    }
    set_document_part_length(
      &mut word_document.fib,
      part,
      part_edit.relocate_u32(part_len, "FIB document-part character count")?,
    )?;
    if part == FieldDocumentPart::Main {
      relocate_main_document_cps(table, &part_edit)?;
    } else {
      relocate_global_document_cps(table, &global_edit)?;
      relocate_non_main_document_part_cps(table, part, &part_edit)?;
    }
    if remove_piece {
      word_document.pending_text_edits.remove(&source_piece_index);
    } else {
      word_document
        .pending_text_edits
        .entry(source_piece_index)
        .or_default()
        .push(local_edit);
    }
    if let Some(runs) = &mut word_document.chpx_runs {
      apply_character_run_edit(runs, &global_edit)?;
    } else if rebuild_formatting.character || remove_piece {
      return Err(Error::invalid(
        u64::from(global_start),
        "DOC CHPX CP tree is unavailable for a structural text edit",
      ));
    }
    word_document.rebuild_character_formatting |= rebuild_formatting.character || remove_piece;
    word_document.rebuild_paragraph_formatting |= rebuild_formatting.paragraph || remove_piece;
    Ok(())
  }

  fn validate_links(&self) -> Result<()> {
    let fib = &self.word_document.fib;
    validate_compatibility_tables(&self.table.physical_bytes, &self.table.compatibility_tables)?;
    validate_object_pool_links(&self.compound_file, self.object_pool.as_deref())?;
    validate_data_links(
      &self.word_document,
      &self.table,
      self.data.as_deref(),
      &self.data_link_baseline,
    )?;
    let expected_name = if fib.base.flags.contains(FibBaseFlags::USE_1_TABLE) {
      DocTableStreamName::Table1
    } else {
      DocTableStreamName::Table0
    };
    if self.table.name != expected_name
      || fib.clx_location() != Some(self.table.clx.location)
      || fib.chpx_bte_location() != Some(self.table.character_bin_table.location)
      || fib.papx_bte_location() != Some(self.table.paragraph_bin_table.location)
      || fib.section_table_location() != Some(self.table.sections.location)
      || fib.style_sheet_location().filter(|v| v.lcb != 0)
        != self.table.styles.as_ref().map(|v| v.location)
      || fib.font_table_location().filter(|v| v.lcb != 0)
        != self.table.fonts.as_ref().map(|v| v.location)
      || fib.office_art_content_location().filter(|v| v.lcb != 0)
        != self.table.office_art.as_ref().map(|v| v.location)
    {
      return Err(Error::invalid(0, "DOC FIB/content-tree links changed"));
    }
    let expected_fields = fib
      .field_table_locations()
      .into_iter()
      .filter(|(_, location)| location.lcb != 0)
      .collect::<BTreeMap<_, _>>();
    let actual_fields = self
      .table
      .fields
      .iter()
      .map(|(part, field)| (*part, field.location))
      .collect::<BTreeMap<_, _>>();
    if actual_fields != expected_fields {
      return Err(Error::invalid(0, "DOC FIB/field-table links changed"));
    }
    let expected_bookmarks = match fib.bookmark_locations() {
      None => None,
      Some(locations) => {
        let locations = [locations.0, locations.1, locations.2];
        if locations.iter().all(|location| location.lcb == 0) {
          None
        } else if locations.iter().all(|location| location.lcb != 0) {
          Some(locations)
        } else {
          return Err(Error::invalid(0, "DOC bookmark locations are incomplete"));
        }
      }
    };
    let actual_bookmarks = self.table.bookmarks.as_ref().map(|bookmarks| {
      [
        bookmarks.names_location,
        bookmarks.starts_location,
        bookmarks.ends_location,
      ]
    });
    if expected_bookmarks != actual_bookmarks {
      return Err(Error::invalid(0, "DOC FIB/bookmark links changed"));
    }
    validate_optional_location(
      fib.header_text_location(),
      self.table.header_text.as_ref(),
      "header text",
    )?;
    validate_note_locations(
      fib.footnote_locations(),
      self.table.footnotes.as_ref(),
      "footnote",
    )?;
    validate_note_locations(
      fib.endnote_locations(),
      self.table.endnotes.as_ref(),
      "endnote",
    )?;
    validate_annotation_locations(fib.annotation_locations(), self.table.annotations.as_ref())?;
    validate_optional_location(
      fib.annotation_owner_location(),
      self.table.annotation_owners.as_ref(),
      "annotation owners",
    )?;
    let expected_annotation_bookmarks =
      fib
        .annotation_bookmark_locations()
        .and_then(|(infos, starts, ends)| {
          [infos, starts, ends]
            .iter()
            .all(|location| location.lcb != 0)
            .then_some([infos, starts, ends])
        });
    let actual_annotation_bookmarks = self.table.annotation_bookmarks.as_ref().map(|value| {
      [
        value.infos_location,
        value.starts_location,
        value.ends_location,
      ]
    });
    if expected_annotation_bookmarks != actual_annotation_bookmarks {
      return Err(Error::invalid(
        0,
        "DOC FIB/annotation-bookmark links changed",
      ));
    }
    validate_optional_location(
      managed_expected_location(
        fib.annotation_extended_data_location(),
        &self.table.compatibility_tables,
        "AtrdExtra",
      ),
      self.table.annotation_extended_data.as_ref(),
      "annotation extended data",
    )?;
    validate_part_locations(
      fib.textbox_story_locations(),
      &self.table.textbox_stories,
      "textbox stories",
    )?;
    validate_part_locations(
      fib.textbox_break_locations(),
      &self.table.textbox_breaks,
      "textbox breaks",
    )?;
    validate_part_locations(
      fib.shape_anchor_locations(),
      &self.table.shape_anchors,
      "shape anchors",
    )?;
    validate_optional_location(
      fib.revision_authors_location(),
      self.table.revision_authors.as_ref(),
      "revision authors",
    )?;
    validate_caption_locations(fib.caption_locations(), self.table.captions.as_ref())?;
    validate_optional_location(
      managed_expected_location(
        fib.subdocuments_location(),
        &self.table.compatibility_tables,
        "PlcfWkb",
      ),
      self.table.subdocuments.as_ref(),
      "subdocuments",
    )?;
    validate_optional_location(
      fib.user_variables_location(),
      self.table.user_variables.as_ref(),
      "user variables",
    )?;
    validate_optional_location(
      fib.embedded_fonts_location(),
      self.table.embedded_fonts.as_ref(),
      "embedded fonts",
    )?;
    validate_optional_location(
      fib.spelling_state_location(),
      self.table.spelling_state.as_ref(),
      "spelling state",
    )?;
    validate_optional_location(
      fib.grammar_state_location(),
      self.table.grammar_state.as_ref(),
      "grammar state",
    )?;
    validate_optional_location(
      fib.language_detection_state_location(),
      self.table.language_detection_state.as_ref(),
      "language detection state",
    )?;
    if managed_expected_location(
      fib.list_definition_location(),
      &self.table.compatibility_tables,
      "PlfLst",
    ) != self
      .table
      .list_definitions
      .as_ref()
      .map(|value| value.location)
    {
      return Err(Error::invalid(0, "DOC FIB/list-definition link changed"));
    }
    validate_optional_location(
      fib.list_names_location(),
      self.table.list_names.as_ref(),
      "list names",
    )?;
    validate_optional_location(
      fib.list_override_location(),
      self.table.list_overrides.as_ref(),
      "list overrides",
    )?;
    validate_optional_location(
      fib.document_properties_location(),
      self.table.document_properties.as_ref(),
      "document properties",
    )?;
    validate_optional_location(
      fib.associated_strings_location(),
      self.table.associated_strings.as_ref(),
      "associated strings",
    )?;
    validate_optional_location(
      fib.external_file_names_location(),
      self.table.external_file_names.as_ref(),
      "external file names",
    )?;
    macro_rules! validate_table {
      ($location:expr, $field:ident, $physical:literal, $link:literal) => {
        validate_compatible_location(
          $location,
          self.table.$field.as_ref(),
          &self.table.compatibility_tables,
          $physical,
          $link,
        )?;
      };
    }
    validate_table!(
      fib.mail_merge_state_location(),
      mail_merge_state,
      "Pms",
      "mail merge state"
    );
    validate_table!(
      fib.new_mail_merge_state_location(),
      new_mail_merge_state,
      "PmsNew",
      "new mail merge state"
    );
    validate_table!(
      fib.office_data_source_location(),
      office_data_source,
      "Odso",
      "office data source"
    );
    validate_table!(
      fib.printer_driver_info_location(),
      printer_driver_info,
      "PrDrvr",
      "printer driver info"
    );
    validate_table!(
      fib.ole_control_info_location(),
      ole_control_infos,
      "RgxOcxInfo",
      "OLE control infos"
    );
    validate_table!(
      fib.table_character_cache_location(),
      table_character_cache,
      "PlcfTch",
      "table character cache"
    );
    validate_table!(
      fib.revision_message_threading_location(),
      revision_message_threading,
      "RmdThreading",
      "revision message threading"
    );
    validate_table!(
      fib.list_style_templates_location(),
      list_style_templates,
      "SttbRgtplc",
      "list style templates"
    );
    validate_table!(
      fib.frame_and_list_records_location(),
      frame_and_list_records,
      "RgDofr",
      "frame and list records"
    );
    validate_table!(
      fib.grammar_option_sets_location(),
      grammar_option_sets,
      "PlfCosi",
      "grammar option sets"
    );
    validate_table!(
      fib.legacy_grammar_option_sets_location(),
      legacy_grammar_option_sets,
      "PlfGosl",
      "legacy grammar option sets"
    );
    validate_table!(
      fib.auto_summary_ranges_location(),
      auto_summary_ranges,
      "PlcfAsumy",
      "auto summary ranges"
    );
    validate_table!(
      fib.smart_tag_recognizer_state_location(),
      smart_tag_recognizer_state,
      "PlcfFactoid",
      "smart-tag recognizer state"
    );
    validate_table!(
      fib.xml_schema_references_location(),
      xml_schema_references,
      "Hplxsdr",
      "XML schema references"
    );
    validate_table!(
      fib.xml_transform_path_location(),
      xml_transform_path,
      "CustomXForm",
      "XML transform path"
    );
    validate_table!(
      fib.paragraph_group_properties_location(),
      paragraph_group_properties,
      "PlcfPgp",
      "paragraph group properties"
    );
    validate_table!(
      fib.save_history_location(),
      save_history,
      "SttbSavedBy",
      "save history"
    );
    validate_table!(
      fib.grammar_checker_cookies_location(),
      grammar_checker_cookies,
      "PlcfCookie",
      "grammar checker cookies"
    );
    validate_table!(
      fib.legacy_grammar_checker_cookies_location(),
      legacy_grammar_checker_cookies,
      "PlcfCookieOld",
      "legacy grammar checker cookies"
    );
    validate_table!(
      fib.grammar_cookie_data_location(),
      grammar_cookie_data,
      "CookieData",
      "grammar cookie data"
    );
    validate_table!(
      fib.smart_tag_data_location(),
      smart_tag_data,
      "FactoidData",
      "smart-tag data"
    );
    validate_table!(
      fib.revision_save_ids_location(),
      revision_save_ids,
      "Plrsid",
      "revision save IDs"
    );
    validate_table!(
      fib.selection_state_location(),
      selection_state,
      "Wss",
      "selection state"
    );
    validate_table!(
      fib.command_customizations_location(),
      command_customizations,
      "Cmds",
      "command customizations"
    );
    let expected = managed_expected_locations(
      fib.structured_tag_bookmark_locations(),
      &self.table.compatibility_tables,
      ["SttbfBkmkSdt", "PlcfBkfSdt", "PlcfBklSdt"],
    );
    let actual = self.table.structured_tag_bookmarks.as_ref().map(|value| {
      [
        value.metadata_location,
        value.starts_location,
        value.ends_location,
      ]
    });
    if expected != actual {
      return Err(Error::invalid(
        0,
        "DOC FIB/structured-tag bookmark links changed",
      ));
    }
    let expected = managed_expected_locations(
      fib.range_protection_locations(),
      &self.table.compatibility_tables,
      [
        "SttbfBkmkProt",
        "PlcfBkfProt",
        "PlcfBklProt",
        "SttbProtUser",
      ],
    );
    let actual = self.table.range_protection.as_ref().map(|value| {
      [
        value.permissions_location,
        value.starts_location,
        value.ends_location,
        value.users_location,
      ]
    });
    if expected != actual {
      return Err(Error::invalid(0, "DOC FIB/range-protection links changed"));
    }
    let expected = managed_expected_locations(
      fib.smart_tag_bookmark_locations(),
      &self.table.compatibility_tables,
      ["SttbfBkmkFactoid", "PlcfBkfFactoid", "PlcfBklFactoid"],
    );
    let actual = self.table.smart_tag_bookmarks.as_ref().map(|value| {
      [
        value.metadata_location,
        value.starts_location,
        value.ends_location,
      ]
    });
    if expected != actual {
      return Err(Error::invalid(
        0,
        "DOC FIB/smart-tag bookmark links changed",
      ));
    }
    let expected = managed_expected_locations(
      fib.format_consistency_bookmark_locations(),
      &self.table.compatibility_tables,
      ["SttbfBkmkFcc", "PlcfBkfFcc", "PlcfBklFcc"],
    );
    let actual = self
      .table
      .format_consistency_bookmarks
      .as_ref()
      .map(|value| {
        [
          value.metadata_location,
          value.starts_location,
          value.ends_location,
        ]
      });
    if expected != actual {
      return Err(Error::invalid(
        0,
        "DOC FIB/format-consistency bookmark links changed",
      ));
    }
    let expected = managed_expected_locations(
      fib.repair_bookmark_locations(),
      &self.table.compatibility_tables,
      ["SttbfBkmkBpRepairs", "PlcfBkfBpRepairs", "PlcfBklBpRepairs"],
    );
    let actual = self.table.repair_bookmarks.as_ref().map(|value| {
      [
        value.metadata_location,
        value.starts_location,
        value.ends_location,
      ]
    });
    if expected != actual {
      return Err(Error::invalid(0, "DOC FIB/repair bookmark links changed"));
    }
    let expected = managed_expected_locations(
      fib.user_input_method_locations(),
      &self.table.compatibility_tables,
      ["PlcfUim", "PlfGuidUim"],
    );
    let actual = self
      .table
      .user_input_methods
      .as_ref()
      .map(|value| [value.methods_location, value.service_guids_location]);
    if expected != actual {
      return Err(Error::invalid(0, "DOC FIB/user-input-method links changed"));
    }
    validate_compatible_location(
      fib.mso_envelope_location(),
      self.table.mso_envelope.as_ref(),
      &self.table.compatibility_tables,
      "MsoEnvelope",
      "MsoEnvelope",
    )?;
    if managed_expected_location(
      fib.deprecated_numbering_field_cache_location(),
      &self.table.compatibility_tables,
      "PlcfBteLvc",
    ) != self
      .table
      .deprecated_numbering_field_cache
      .as_ref()
      .map(|value| value.location)
    {
      return Err(Error::invalid(
        0,
        "DOC FIB/deprecated numbering field cache link changed",
      ));
    }

    let pieces = &self.table.clx.value.piece_table;
    if pieces.pieces.len() != self.word_document.text_pieces.len()
      || pieces.character_positions.len() != pieces.pieces.len() + 1
      || self.word_document.source_piece_indices.len() != self.word_document.text_pieces.len()
      || self
        .word_document
        .source_piece_indices
        .windows(2)
        .any(|indices| indices[0] >= indices[1])
    {
      return Err(Error::invalid(0, "CLX/text-piece tree cardinality changed"));
    }
    for (index, (descriptor, piece)) in pieces
      .pieces
      .iter()
      .zip(&self.word_document.text_pieces)
      .enumerate()
    {
      if piece.piece_index != index
        || piece.value.cp_start != pieces.character_positions[index]
        || piece.value.cp_end != pieces.character_positions[index + 1]
        || piece.value.file_offset != descriptor.file_position.byte_offset()
      {
        return Err(Error::invalid(0, "CLX/text-piece link changed"));
      }
    }
    if let Some(runs) = &self.word_document.chpx_runs {
      let normalized = normalize_logical_character_runs(runs.clone())?;
      if normalized != *runs {
        return Err(Error::invalid(0, "CHPX CP tree is not canonical"));
      }
    }
    if let Some(runs) = &self.word_document.papx_runs {
      let ranges = current_paragraph_ranges(&self.word_document.text_pieces)?;
      if runs.len() != ranges.len()
        || runs
          .iter()
          .zip(ranges)
          .any(|(run, range)| (run.cp_start, run.cp_end) != range)
      {
        return Err(Error::invalid(
          0,
          "PAPX CP tree does not match the document paragraphs",
        ));
      }
    }
    if self
      .word_document
      .character_format_pages
      .iter()
      .map(|page| page.page)
      .ne(self.table.character_bin_table.value.pages.iter().copied())
      || self
        .word_document
        .paragraph_format_pages
        .iter()
        .map(|page| page.page)
        .ne(self.table.paragraph_bin_table.value.pages.iter().copied())
    {
      return Err(Error::invalid(0, "PlcBte/FKP page links changed"));
    }
    if self.word_document.section_properties.len() != self.table.sections.value.sections.len() {
      return Err(Error::invalid(0, "SED/Sepx tree cardinality changed"));
    }
    for section in &self.word_document.section_properties {
      let sed = self
        .table
        .sections
        .value
        .sections
        .get(section.section_index)
        .ok_or_else(|| Error::invalid(0, "section property index is stale"))?;
      if sed.sepx_offset != section.offset || (section.offset == -1) != section.value.is_none() {
        return Err(Error::invalid(0, "SED/Sepx link changed"));
      }
    }
    Ok(())
  }
}

fn document_part_lengths(fib: &Fib) -> Result<[(FieldDocumentPart, u32); 7]> {
  let values = [
    (FieldDocumentPart::Main, fib.rg_lw.ccp_text),
    (FieldDocumentPart::Footnote, fib.rg_lw.ccp_footnote),
    (FieldDocumentPart::Header, fib.rg_lw.ccp_header),
    (FieldDocumentPart::Comment, fib.rg_lw.ccp_comment),
    (FieldDocumentPart::Endnote, fib.rg_lw.ccp_endnote),
    (FieldDocumentPart::Textbox, fib.rg_lw.ccp_textbox),
    (
      FieldDocumentPart::HeaderTextbox,
      fib.rg_lw.ccp_header_textbox,
    ),
  ];
  let mut lengths = [(FieldDocumentPart::Main, 0); 7];
  for (index, (part, value)) in values.into_iter().enumerate() {
    lengths[index] = (
      part,
      u32::try_from(value)
        .map_err(|_| Error::invalid(0, format!("FIB character count for {part:?} is negative")))?,
    );
  }
  Ok(lengths)
}

fn document_part_range(fib: &Fib, target: FieldDocumentPart) -> Result<(u32, u32)> {
  if target == FieldDocumentPart::Macro {
    return Err(Error::invalid(
      0,
      "the macro field table has no MS-DOC document-part text range",
    ));
  }
  let mut start = 0u32;
  for (part, length) in document_part_lengths(fib)? {
    if part == target {
      return Ok((start, length));
    }
    start = start
      .checked_add(length)
      .ok_or_else(|| Error::Limit("DOC document-part CP range overflow".into()))?;
  }
  Err(Error::invalid(0, "unknown MS-DOC document part"))
}

fn grpprl_for_group(properties: &GrpPrl, group: SprmGroup) -> GrpPrl {
  GrpPrl {
    properties: properties
      .properties
      .iter()
      .filter(|property| property.sprm.group == group)
      .cloned()
      .collect(),
  }
}

fn effective_paragraph_style_index(formatting: &DocDirectParagraphFormatting) -> Result<u16> {
  let mut style_index = formatting.style_index;
  for property in &formatting.applied_properties.properties {
    match property.sprm.kind() {
      SprmKind::Known(KnownSprm::PIstd) => {
        let SprmOperand::Word(value) = &property.operand else {
          return Err(Error::invalid(0, "sprmPIstd operand is not an istd"));
        };
        style_index = u16::from_le_bytes(*value);
      }
      SprmKind::Known(KnownSprm::PIstdPermute) => {
        let SprmOperand::StylePermutation(permutation) = &property.operand else {
          return Err(Error::invalid(
            0,
            "sprmPIstdPermute operand is not SPPOperand",
          ));
        };
        if let Some(remapped) = permutation.remap(style_index) {
          style_index = remapped;
        }
      }
      _ => {}
    }
  }
  Ok(style_index)
}

fn apply_outline_properties(properties: &GrpPrl, level: &mut u8) -> Result<()> {
  for property in &properties.properties {
    match property.sprm.kind() {
      SprmKind::Known(KnownSprm::POutLvl) => {
        let SprmOperand::Byte(value) = &property.operand else {
          return Err(Error::invalid(
            0,
            "sprmPOutLvl operand is not an unsigned byte",
          ));
        };
        if *value > 9 {
          return Err(Error::invalid(
            u64::from(*value),
            "sprmPOutLvl exceeds 0x09",
          ));
        }
        *level = *value;
      }
      SprmKind::Known(KnownSprm::PIncLvl) if *level != 9 => {
        let SprmOperand::Byte(value) = &property.operand else {
          return Err(Error::invalid(
            0,
            "sprmPIncLvl operand is not a signed byte",
          ));
        };
        *level = i16::from(*level)
          .saturating_add(i16::from(i8::from_le_bytes([*value])))
          .clamp(0, 9) as u8;
      }
      _ => {}
    }
  }
  Ok(())
}

struct StylePropertyAccumulator<'a> {
  active: &'a mut BTreeSet<u16>,
  lineage: &'a mut Vec<u16>,
  paragraph: &'a mut Vec<super::Prl>,
  character: &'a mut Vec<super::Prl>,
  table: &'a mut Vec<super::Prl>,
}

fn collect_style_properties(
  styles: &StyleSheet,
  style_index: u16,
  accumulator: &mut StylePropertyAccumulator<'_>,
) -> Result<()> {
  if !accumulator.active.insert(style_index) {
    return Err(Error::invalid(
      u64::from(style_index),
      "STSH base-style references contain a cycle",
    ));
  }
  let definition = styles
    .styles
    .get(usize::from(style_index))
    .and_then(|style| style.definition.as_ref())
    .ok_or_else(|| {
      Error::invalid(
        u64::from(style_index),
        "STSH style hierarchy references an unavailable style",
      )
    })?;
  if definition.base.base_style_index != 0x0fff {
    collect_style_properties(styles, definition.base.base_style_index, accumulator)?;
  }
  accumulator.lineage.push(style_index);
  match &definition.formatting {
    StyleFormatting::Paragraph {
      paragraph: value,
      character: value_character,
    } => {
      accumulator
        .paragraph
        .extend(value.properties.properties.iter().cloned());
      accumulator
        .character
        .extend(value_character.properties.properties.iter().cloned());
    }
    StyleFormatting::Character { character: value } => {
      accumulator
        .character
        .extend(value.properties.properties.iter().cloned());
    }
    StyleFormatting::RevisionParagraph {
      paragraph: value,
      character: value_character,
      ..
    } => {
      accumulator
        .paragraph
        .extend(value.properties.properties.iter().cloned());
      accumulator
        .character
        .extend(value_character.properties.properties.iter().cloned());
    }
    StyleFormatting::RevisionCharacter {
      character: value, ..
    } => {
      accumulator
        .character
        .extend(value.properties.properties.iter().cloned());
    }
    StyleFormatting::Table {
      table: value_table,
      paragraph: value_paragraph,
      character: value_character,
    } => {
      accumulator
        .table
        .extend(value_table.properties.properties.iter().cloned());
      accumulator
        .paragraph
        .extend(value_paragraph.properties.properties.iter().cloned());
      accumulator
        .character
        .extend(value_character.properties.properties.iter().cloned());
    }
    StyleFormatting::Numbering { paragraph: value } => {
      accumulator
        .paragraph
        .extend(value.properties.properties.iter().cloned());
    }
  }
  accumulator.active.remove(&style_index);
  Ok(())
}

fn apply_cf_spec_toggles(
  properties: &GrpPrl,
  value: bool,
  style_value: bool,
  source: &str,
) -> Result<bool> {
  apply_character_toggles(properties, KnownSprm::CFSpec, value, style_value, source)
}

fn apply_character_toggles(
  properties: &GrpPrl,
  target: KnownSprm,
  mut value: bool,
  style_value: bool,
  source: &str,
) -> Result<bool> {
  for property in &properties.properties {
    if property.sprm.kind() != SprmKind::Known(target) {
      continue;
    }
    let SprmOperand::Toggle(operand) = &property.operand else {
      return Err(Error::invalid(
        0,
        format!("{target:?} in {source} does not have a ToggleOperand"),
      ));
    };
    value = match *operand {
      0x00 => false,
      0x01 => true,
      0x80 => style_value,
      0x81 => !style_value,
      _ => {
        return Err(Error::invalid(
          0,
          format!("{target:?} in {source} has an invalid ToggleOperand"),
        ));
      }
    };
  }
  Ok(value)
}

fn effective_character_toggle_from_ref(
  file: &DocFile,
  formatting: DocDirectFormattingRef<'_>,
  target: KnownSprm,
) -> Result<bool> {
  let direct = direct_character_toggle_operand(formatting, target)?;
  if matches!(direct, Some(0x00 | 0x01)) {
    return Ok(direct == Some(0x01));
  }
  let style_value = style_character_toggle(file, formatting, target)?;
  match direct {
    None | Some(0x80) => Ok(style_value),
    Some(0x81) => Ok(!style_value),
    Some(value) => Err(Error::invalid(
      u64::from(value),
      format!("{target:?} has an invalid ToggleOperand"),
    )),
  }
}

fn direct_character_toggle_operand(
  formatting: DocDirectFormattingRef<'_>,
  target: KnownSprm,
) -> Result<Option<u8>> {
  let chpx_operand = formatting
    .character_run
    .properties
    .as_deref()
    .map(|properties| last_toggle_operand(properties, target, "CHPX character properties"))
    .transpose()?
    .flatten();
  let piece = formatting
    .descriptor()
    .property_modifier
    .property_modifications_ref(&formatting.document_part.file.table.clx.value)?;
  let piece_operand = match piece {
    PrmPropertiesRef::Empty => None,
    PrmPropertiesRef::Simple { sprm, value } if sprm == target => {
      validate_toggle_operand(value, target, "Pcd.Prm character properties")?;
      Some(value)
    }
    PrmPropertiesRef::Simple { .. } => None,
    PrmPropertiesRef::Complex(properties) => {
      last_toggle_operand(properties, target, "Pcd.Prm character properties")?
    }
  };
  Ok(piece_operand.or(chpx_operand))
}

fn last_toggle_operand(properties: &GrpPrl, target: KnownSprm, source: &str) -> Result<Option<u8>> {
  let mut last = None;
  for property in &properties.properties {
    if property.sprm.kind() != SprmKind::Known(target) {
      continue;
    }
    let SprmOperand::Toggle(value) = property.operand else {
      return Err(Error::invalid(
        0,
        format!("{target:?} in {source} does not have a ToggleOperand"),
      ));
    };
    validate_toggle_operand(value, target, source)?;
    last = Some(value);
  }
  Ok(last)
}

fn validate_toggle_operand(value: u8, target: KnownSprm, source: &str) -> Result<()> {
  if matches!(value, 0x00 | 0x01 | 0x80 | 0x81) {
    Ok(())
  } else {
    Err(Error::invalid(
      u64::from(value),
      format!("{target:?} in {source} has an invalid ToggleOperand"),
    ))
  }
}

fn style_character_toggle(
  file: &DocFile,
  formatting: DocDirectFormattingRef<'_>,
  target: KnownSprm,
) -> Result<bool> {
  let paragraph = formatting.materialize_paragraph()?;
  let style_index = effective_paragraph_style_index(&paragraph)?;
  let style = file.style_properties(style_index)?;
  if style.style_kind != super::StyleKind::Paragraph {
    return Err(Error::invalid(
      u64::from(style_index),
      "paragraph formatting references a non-paragraph style",
    ));
  }
  apply_character_toggles(
    &style.character_properties,
    target,
    false,
    false,
    "style character properties",
  )
}

fn direct_picture_location_property(
  formatting: DocDirectFormattingRef<'_>,
) -> Result<Option<&super::Prl>> {
  let mut location = formatting
    .character_run
    .properties
    .as_ref()
    .and_then(|properties| {
      properties
        .properties
        .iter()
        .rev()
        .find(|property| property.sprm.kind() == SprmKind::Known(KnownSprm::CPicLocation))
    });
  if let Prm::Complex { property_run_index } = formatting.descriptor().property_modifier {
    let properties = &formatting
      .document_part
      .file
      .table
      .clx
      .value
      .property_runs
      .get(usize::from(property_run_index))
      .ok_or_else(|| {
        Error::invalid(
          u64::from(property_run_index),
          "Prm1 property-run index exceeds the CLX Prc array",
        )
      })?
      .properties;
    if let Some(piece_location) = properties
      .properties
      .iter()
      .rev()
      .find(|property| property.sprm.kind() == SprmKind::Known(KnownSprm::CPicLocation))
    {
      location = Some(piece_location);
    }
  }
  Ok(location)
}

fn find_field_separator<'a>(field: DocFieldRef<'a>, local_cp: u32) -> Option<DocFieldRef<'a>> {
  if field
    .source
    .separator
    .is_some_and(|separator| separator.position == local_cp)
  {
    return Some(field);
  }
  field
    .instruction_fields()
    .chain(field.result_fields())
    .find_map(|nested| find_field_separator(nested, local_cp))
}

fn expand_direct_paragraph_properties(
  properties: &GrpPrl,
  data: Option<&DocDataStream>,
  papx_style_index: Option<u16>,
) -> Result<GrpPrl> {
  let mut active_offsets = BTreeSet::new();
  Ok(GrpPrl {
    properties: expand_direct_paragraph_property_array(
      properties,
      data,
      papx_style_index,
      &mut active_offsets,
    )?,
  })
}

fn expand_direct_paragraph_property_array(
  properties: &GrpPrl,
  data: Option<&DocDataStream>,
  papx_style_index: Option<u16>,
  active_offsets: &mut BTreeSet<u32>,
) -> Result<Vec<super::Prl>> {
  let mut applied = Vec::new();
  for (index, property) in properties.properties.iter().enumerate() {
    let reference_kind = match property.sprm.kind() {
      SprmKind::Known(KnownSprm::PHugePapx) if index != 0 => continue,
      SprmKind::Known(KnownSprm::PHugePapx) => {
        if papx_style_index.is_some_and(|style_index| style_index != 0) {
          return Err(Error::invalid(
            0,
            "sprmPHugePapx in PapxInFkp requires style index zero",
          ));
        }
        "sprmPHugePapx"
      }
      SprmKind::Known(KnownSprm::PTableProps) => "sprmPTableProps",
      _ => {
        applied.push(property.clone());
        continue;
      }
    };
    let SprmOperand::Dword(raw_offset) = &property.operand else {
      return Err(Error::invalid(
        0,
        format!("{reference_kind} operand is not a Data-stream offset"),
      ));
    };
    let offset = u32::from_le_bytes(*raw_offset);
    if !active_offsets.insert(offset) {
      return Err(Error::invalid(
        u64::from(offset),
        "paragraph-property Data references contain a cycle",
      ));
    }
    let node = data
      .and_then(|data| data.nodes.iter().find(|node| node.offset == offset))
      .ok_or_else(|| {
        Error::invalid(
          u64::from(offset),
          format!("{reference_kind} target PrcData is unavailable"),
        )
      })?;
    let DocDataNodeValue::ParagraphProperties(referenced) = &node.value else {
      return Err(Error::invalid(
        u64::from(offset),
        format!("{reference_kind} target is not a PrcData node"),
      ));
    };
    if node.physical_len < 12 {
      return Err(Error::invalid(
        u64::from(offset),
        format!("{reference_kind} target PrcData cbGrpprl is less than 10"),
      ));
    }
    applied.extend(expand_direct_paragraph_property_array(
      &referenced.properties,
      data,
      None,
      active_offsets,
    )?);
    active_offsets.remove(&offset);
    // Both reference SPRMs terminate processing of their containing array
    // when the referenced PrcData is processed.
    break;
  }
  Ok(applied)
}

fn set_document_part_length(fib: &mut Fib, part: FieldDocumentPart, length: u32) -> Result<()> {
  let length = i32::try_from(length)
    .map_err(|_| Error::Limit("document-part character count exceeds i32".into()))?;
  match part {
    FieldDocumentPart::Main => fib.rg_lw.ccp_text = length,
    FieldDocumentPart::Footnote => fib.rg_lw.ccp_footnote = length,
    FieldDocumentPart::Header => fib.rg_lw.ccp_header = length,
    FieldDocumentPart::Comment => fib.rg_lw.ccp_comment = length,
    FieldDocumentPart::Endnote => fib.rg_lw.ccp_endnote = length,
    FieldDocumentPart::Textbox => fib.rg_lw.ccp_textbox = length,
    FieldDocumentPart::HeaderTextbox => fib.rg_lw.ccp_header_textbox = length,
    FieldDocumentPart::Macro => {
      return Err(Error::invalid(
        0,
        "the macro field table has no document-part character count",
      ));
    }
  }
  Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CpReplacement {
  old_start: u32,
  old_end: u32,
  new_end: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct FormattingRebuild {
  character: bool,
  paragraph: bool,
}

impl CpReplacement {
  fn new(old_start: u32, old_end: u32, replacement_len: u32) -> Result<Self> {
    let new_end = old_start
      .checked_add(replacement_len)
      .ok_or_else(|| Error::Limit("DOC replacement CP limit overflow".into()))?;
    Ok(Self {
      old_start,
      old_end,
      new_end,
    })
  }

  fn relocate_u32(self, value: u32, label: &str) -> Result<u32> {
    if value < self.old_start || (value == self.old_start && self.old_start != self.old_end) {
      return Ok(value);
    }
    if value < self.old_end {
      return Err(Error::invalid(
        u64::from(value),
        format!("{label} falls inside the replaced text range"),
      ));
    }
    if self.new_end >= self.old_end {
      value
        .checked_add(self.new_end - self.old_end)
        .ok_or_else(|| Error::Limit(format!("{label} CP overflow")))
    } else {
      value
        .checked_sub(self.old_end - self.new_end)
        .ok_or_else(|| Error::Limit(format!("{label} CP underflow")))
    }
  }

  fn relocate_i32(self, value: i32, label: &str) -> Result<i32> {
    let value =
      u32::try_from(value).map_err(|_| Error::invalid(0, format!("{label} is negative")))?;
    i32::try_from(self.relocate_u32(value, label)?)
      .map_err(|_| Error::Limit(format!("{label} exceeds i32")))
  }
}

fn relocate_cp_positions(positions: &mut [u32], edit: &CpReplacement, label: &str) -> Result<()> {
  for position in positions {
    *position = edit.relocate_u32(*position, label)?;
  }
  Ok(())
}

fn relocate_bookmark_positions(
  starts: &mut super::BookmarkStartTable,
  ends: &mut super::BookmarkEndTable,
  edit: &CpReplacement,
  label: &str,
) -> Result<()> {
  relocate_cp_positions(&mut starts.positions, edit, label)?;
  relocate_cp_positions(&mut ends.positions, edit, label)
}

fn relocate_field(field: &mut super::Field, edit: &CpReplacement) -> Result<()> {
  field.begin.position = edit.relocate_u32(field.begin.position, "Plcfld begin CP")?;
  for nested in &mut field.instruction_fields {
    relocate_field(nested, edit)?;
  }
  if let Some(separator) = &mut field.separator {
    separator.position = edit.relocate_u32(separator.position, "Plcfld separator CP")?;
  }
  for nested in &mut field.result_fields {
    relocate_field(nested, edit)?;
  }
  field.end.position = edit.relocate_u32(field.end.position, "Plcfld end CP")?;
  Ok(())
}

fn validate_formatting_edit(
  file: &DocFile,
  source_start: u32,
  source_width: usize,
  source_character_count: usize,
  prior_edits: &[CpReplacement],
  edit: &CpReplacement,
) -> Result<FormattingRebuild> {
  let source_len = source_character_count
    .checked_mul(source_width)
    .ok_or_else(|| Error::Limit("text piece source length overflow".into()))?;
  let mut rebuild = FormattingRebuild::default();
  let mut validate_positions = |positions: &[u32], label: &str, kind: &str| -> Result<()> {
    let source_end = u64::from(source_start)
      .checked_add(source_len as u64)
      .ok_or_else(|| Error::Limit("text piece source limit overflow".into()))?;
    for position in positions {
      let position = u64::from(*position);
      if position < u64::from(source_start) || position > source_end {
        continue;
      }
      let byte_offset = usize::try_from(position - u64::from(source_start))
        .map_err(|_| Error::Limit("formatting boundary offset exceeds usize".into()))?;
      if !byte_offset.is_multiple_of(source_width) {
        return Err(Error::invalid(
          position,
          format!("{label} is not aligned to the text-piece encoding"),
        ));
      }
      let current_position = relocate_character_position(
        u32::try_from(byte_offset / source_width)
          .map_err(|_| Error::Limit("formatting boundary character offset exceeds u32".into()))?,
        prior_edits,
        label,
      )?;
      if let Err(error) = edit.relocate_u32(current_position, label) {
        match kind {
          "character" => rebuild.character = true,
          "paragraph" => rebuild.paragraph = true,
          _ => return Err(error),
        }
      }
    }
    Ok(())
  };
  validate_positions(
    &file.table.character_bin_table.value.file_positions,
    "PlcBteChpx FC boundary",
    "character",
  )?;
  validate_positions(
    &file.table.paragraph_bin_table.value.file_positions,
    "PlcBtePapx FC boundary",
    "paragraph",
  )?;
  for page in &file.word_document.character_format_pages {
    validate_positions(
      &page.value.file_positions,
      "ChpxFkp FC boundary",
      "character",
    )?;
  }
  for page in &file.word_document.paragraph_format_pages {
    validate_positions(
      &page.value.file_positions,
      "PapxFkp FC boundary",
      "paragraph",
    )?;
  }
  Ok(rebuild)
}

fn relocate_document_properties_cp(
  properties: &mut DocumentProperties,
  part: FieldDocumentPart,
  edit: &CpReplacement,
) -> Result<()> {
  use super::DocumentPropertiesExtension;

  properties.word97.base.document_flags.exact_statistics = false;
  properties.word97.display_flags.list_cache_invalid = true;
  if part == FieldDocumentPart::Main && properties.word97.maximum_list_cache_position >= 0 {
    properties.word97.maximum_list_cache_position = edit.relocate_i32(
      properties.word97.maximum_list_cache_position,
      "DOP cpMaxListCacheMainDoc",
    )?;
  }
  let word2002 = match &mut properties.extension {
    DocumentPropertiesExtension::None | DocumentPropertiesExtension::Word2000(_) => None,
    DocumentPropertiesExtension::Word2002(value)
    | DocumentPropertiesExtension::Compatibility600 {
      word2002: value, ..
    }
    | DocumentPropertiesExtension::Compatibility610 {
      word2002: value, ..
    } => Some(value),
    DocumentPropertiesExtension::Word2003(value)
    | DocumentPropertiesExtension::Word2003WithTrailingByte {
      word2003: value, ..
    } => Some(&mut value.word2002),
    DocumentPropertiesExtension::Word2007(value) => Some(&mut value.word2003.word2002),
    DocumentPropertiesExtension::Word2010(value) => Some(&mut value.word2007.word2003.word2002),
    DocumentPropertiesExtension::Word2013(value) => {
      Some(&mut value.word2010.word2007.word2003.word2002)
    }
  };
  if let Some(value) = word2002 {
    let (position, label) = match part {
      FieldDocumentPart::Main => (
        &mut value.minimum_revision_positions.main,
        "DOP cpMinRMText",
      ),
      FieldDocumentPart::Footnote => (
        &mut value.minimum_revision_positions.footnote,
        "DOP cpMinRMFtn",
      ),
      FieldDocumentPart::Header => (
        &mut value.minimum_revision_positions.header,
        "DOP cpMinRMHdd",
      ),
      FieldDocumentPart::Comment => (
        &mut value.minimum_revision_positions.comment,
        "DOP cpMinRMAtn",
      ),
      FieldDocumentPart::Endnote => (
        &mut value.minimum_revision_positions.endnote,
        "DOP cpMinRMEdn",
      ),
      FieldDocumentPart::Textbox => (
        &mut value.minimum_revision_positions.textbox,
        "DOP cpMinRMTxbx",
      ),
      FieldDocumentPart::HeaderTextbox => (
        &mut value.minimum_revision_positions.header_textbox,
        "DOP cpMinRMHdrTxbx",
      ),
      FieldDocumentPart::Macro => return Ok(()),
    };
    *position = edit.relocate_u32(*position, label)?;
  }
  Ok(())
}

fn relocate_main_document_cps(table: &mut DocTableStream, edit: &CpReplacement) -> Result<()> {
  for position in &mut table.sections.value.character_positions {
    *position = edit.relocate_i32(*position, "PlcfSed CP")?;
  }
  if let Some(fields) = table.fields.get_mut(&FieldDocumentPart::Main) {
    for field in &mut fields.value.fields {
      relocate_field(field, edit)?;
    }
    fields.value.terminal_position =
      edit.relocate_u32(fields.value.terminal_position, "Plcfld terminal CP")?;
  }
  if let Some(bookmarks) = &mut table.bookmarks {
    relocate_bookmark_positions(
      &mut bookmarks.value.starts,
      &mut bookmarks.value.ends,
      edit,
      "main bookmark CP",
    )?;
  }
  if let Some(notes) = &mut table.footnotes {
    relocate_cp_positions(
      &mut notes.references.value.positions,
      edit,
      "footnote reference CP",
    )?;
  }
  if let Some(notes) = &mut table.endnotes {
    relocate_cp_positions(
      &mut notes.references.value.positions,
      edit,
      "endnote reference CP",
    )?;
  }
  if let Some(annotations) = &mut table.annotations {
    relocate_cp_positions(
      &mut annotations.references.value.positions,
      edit,
      "comment reference CP",
    )?;
  }
  if let Some(bookmarks) = &mut table.annotation_bookmarks {
    relocate_bookmark_positions(
      &mut bookmarks.value.starts,
      &mut bookmarks.value.ends,
      edit,
      "annotation bookmark CP",
    )?;
  }
  if let Some(anchors) = table.shape_anchors.get_mut(&TextboxDocumentPart::Main) {
    relocate_cp_positions(&mut anchors.value.positions, edit, "main shape anchor CP")?;
  }
  if let Some(subdocuments) = &mut table.subdocuments {
    relocate_cp_positions(&mut subdocuments.value.positions, edit, "subdocument CP")?;
  }
  if let Some(overrides) = &mut table.list_overrides {
    for value in &mut overrides.value.overrides {
      value.data.first_paragraph_position = edit.relocate_u32(
        value.data.first_paragraph_position,
        "LFO first-paragraph CP",
      )?;
    }
  }
  if let Some(properties) = &mut table.document_properties {
    relocate_document_properties_cp(&mut properties.value, FieldDocumentPart::Main, edit)?;
  }
  if let Some(cache) = &mut table.table_character_cache {
    relocate_cp_positions(&mut cache.value.positions, edit, "table-character cache CP")?;
  }
  if let Some(selection) = &mut table.selection_state {
    selection.value.first_character =
      edit.relocate_i32(selection.value.first_character, "selection cpFirst")?;
    selection.value.character_limit =
      edit.relocate_i32(selection.value.character_limit, "selection cpLim")?;
    selection.value.anchor_character =
      edit.relocate_i32(selection.value.anchor_character, "selection cpAnchor")?;
    if selection.value.flags.block && !selection.value.flags.table {
      selection.value.shrink_anchor_character = edit.relocate_i32(
        selection.value.shrink_anchor_character,
        "selection cpAnchorShrink",
      )?;
    }
  }
  relocate_global_document_cps(table, edit)
}

fn relocate_global_document_cps(table: &mut DocTableStream, edit: &CpReplacement) -> Result<()> {
  if let Some(state) = &mut table.spelling_state {
    relocate_cp_positions(&mut state.value.positions, edit, "spelling state CP")?;
  }
  if let Some(state) = &mut table.grammar_state {
    relocate_cp_positions(&mut state.value.positions, edit, "grammar state CP")?;
  }
  if let Some(state) = &mut table.language_detection_state {
    relocate_cp_positions(
      &mut state.value.positions,
      edit,
      "language-detection state CP",
    )?;
  }
  if let Some(summary) = &mut table.auto_summary_ranges {
    relocate_cp_positions(&mut summary.value.positions, edit, "AutoSummary CP")?;
  }
  if let Some(state) = &mut table.smart_tag_recognizer_state {
    relocate_cp_positions(&mut state.value.positions, edit, "smart-tag state CP")?;
  }
  if let Some(cookies) = &mut table.grammar_checker_cookies {
    relocate_cp_positions(&mut cookies.value.positions, edit, "grammar-cookie CP")?;
  }
  if let Some(cookies) = &mut table.legacy_grammar_checker_cookies {
    relocate_cp_positions(
      &mut cookies.value.positions,
      edit,
      "legacy grammar-cookie CP",
    )?;
  }
  if let Some(bookmarks) = &mut table.structured_tag_bookmarks {
    relocate_bookmark_positions(
      &mut bookmarks.value.starts,
      &mut bookmarks.value.ends,
      edit,
      "structured-tag bookmark CP",
    )?;
  }
  if let Some(bookmarks) = &mut table.range_protection {
    relocate_bookmark_positions(
      &mut bookmarks.value.starts,
      &mut bookmarks.value.ends,
      edit,
      "range-protection bookmark CP",
    )?;
  }
  if let Some(bookmarks) = &mut table.smart_tag_bookmarks {
    relocate_cp_positions(
      &mut bookmarks.value.starts.positions,
      edit,
      "smart-tag bookmark start CP",
    )?;
    relocate_cp_positions(
      &mut bookmarks.value.ends.positions,
      edit,
      "smart-tag bookmark end CP",
    )?;
  }
  if let Some(bookmarks) = &mut table.format_consistency_bookmarks {
    relocate_bookmark_positions(
      &mut bookmarks.value.starts,
      &mut bookmarks.value.ends,
      edit,
      "format-consistency bookmark CP",
    )?;
  }
  if let Some(bookmarks) = &mut table.repair_bookmarks {
    relocate_bookmark_positions(
      &mut bookmarks.value.starts,
      &mut bookmarks.value.ends,
      edit,
      "repair bookmark CP",
    )?;
  }
  if let Some(methods) = &mut table.user_input_methods {
    relocate_cp_positions(&mut methods.value.positions, edit, "user-input-method CP")?;
  }
  Ok(())
}

fn relocate_non_main_document_part_cps(
  table: &mut DocTableStream,
  part: FieldDocumentPart,
  edit: &CpReplacement,
) -> Result<()> {
  if let Some(fields) = table.fields.get_mut(&part) {
    for field in &mut fields.value.fields {
      relocate_field(field, edit)?;
    }
    fields.value.terminal_position =
      edit.relocate_u32(fields.value.terminal_position, "Plcfld terminal CP")?;
  }
  match part {
    FieldDocumentPart::Footnote => {
      if let Some(notes) = &mut table.footnotes {
        relocate_cp_positions(&mut notes.text.value.positions, edit, "footnote text CP")?;
      }
    }
    FieldDocumentPart::Header => {
      if let Some(headers) = &mut table.header_text {
        for boundary in &mut headers.value.boundaries {
          if let super::HeaderStoryBoundary::Position(position) = boundary {
            *position = edit.relocate_u32(*position, "header story CP")?;
          }
        }
      }
      if let Some(anchors) = table.shape_anchors.get_mut(&TextboxDocumentPart::Header) {
        relocate_cp_positions(&mut anchors.value.positions, edit, "header shape anchor CP")?;
      }
    }
    FieldDocumentPart::Comment => {
      if let Some(annotations) = &mut table.annotations {
        relocate_cp_positions(
          &mut annotations.text.value.positions,
          edit,
          "comment text CP",
        )?;
      }
    }
    FieldDocumentPart::Endnote => {
      if let Some(notes) = &mut table.endnotes {
        relocate_cp_positions(&mut notes.text.value.positions, edit, "endnote text CP")?;
      }
    }
    FieldDocumentPart::Textbox => {
      if let Some(stories) = table.textbox_stories.get_mut(&TextboxDocumentPart::Main) {
        relocate_cp_positions(&mut stories.value.positions, edit, "textbox story CP")?;
      }
      if let Some(breaks) = table.textbox_breaks.get_mut(&TextboxDocumentPart::Main) {
        relocate_cp_positions(&mut breaks.value.positions, edit, "textbox break CP")?;
      }
    }
    FieldDocumentPart::HeaderTextbox => {
      if let Some(stories) = table.textbox_stories.get_mut(&TextboxDocumentPart::Header) {
        relocate_cp_positions(
          &mut stories.value.positions,
          edit,
          "header-textbox story CP",
        )?;
      }
      if let Some(breaks) = table.textbox_breaks.get_mut(&TextboxDocumentPart::Header) {
        relocate_cp_positions(&mut breaks.value.positions, edit, "header-textbox break CP")?;
      }
    }
    FieldDocumentPart::Main | FieldDocumentPart::Macro => {
      return Err(Error::invalid(
        0,
        "invalid non-main document-part CP relocation",
      ));
    }
  }
  if let Some(properties) = &mut table.document_properties {
    relocate_document_properties_cp(&mut properties.value, part, edit)?;
  }
  Ok(())
}

fn parse_text_pieces(
  clx: &Clx,
  word: &[u8],
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<Vec<DocTextPiece>> {
  if clx.piece_table.character_positions.len() != clx.piece_table.pieces.len() + 1 {
    return Err(Error::invalid(0, "PlcPcd CP/Pcd cardinality mismatch"));
  }
  ensure_entry_limit(
    "DOC text pieces",
    clx.piece_table.pieces.len(),
    options.limits,
  )?;
  let mut pieces = Vec::with_capacity(clx.piece_table.pieces.len());
  for (piece_index, descriptor) in clx.piece_table.pieces.iter().enumerate() {
    let value = descriptor.text_piece(
      word,
      clx.piece_table.character_positions[piece_index],
      clx.piece_table.character_positions[piece_index + 1],
    )?;
    if value.characters.compatibility_code_units().is_some() {
      let error = Error::invalid(
        u64::from(value.file_offset),
        "DOC text piece contains an unpaired UTF-16 surrogate",
      );
      if options.is_strict() {
        return Err(error);
      }
      diagnostics.push(ParseDiagnostic::warning(
        ParseDiagnosticCode::NonconformingRecord,
        BinaryFormat::Doc,
        Some(WORD_DOCUMENT_STREAM_PATH),
        Some(u64::from(value.file_offset)),
        "PlcPcd",
        SpecificationReference {
          document: "MS-DOC",
          section: "2.8.1",
        },
        error.to_string(),
      ));
    }
    pieces.push(DocTextPiece { piece_index, value });
  }
  Ok(pieces)
}

fn parse_object_pool(
  compound: &CompoundFile,
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<Option<DocObjectPoolStorage>> {
  let Some(pool) = compound.entry(OBJECT_POOL_STORAGE_PATH) else {
    return Ok(None);
  };
  if !pool.is_storage() {
    let error = Error::invalid(0, "ObjectPool is not a CFB storage");
    if options.is_strict() {
      return Err(error);
    }
    report_object_pool_compatibility(diagnostics, &pool.path, error.to_string());
    return Ok(None);
  }

  let mut objects = Vec::new();
  let mut compatibility_objects = Vec::new();
  for entry in compound.children(&pool.path)? {
    if !entry.is_storage() {
      let error = Error::invalid(0, "ObjectPool contains a direct stream");
      if options.is_strict() {
        return Err(error);
      }
      report_object_pool_compatibility(diagnostics, &entry.path, error.to_string());
      continue;
    }
    let mut entry_paths = compound
      .entries()
      .iter()
      .filter(|candidate| candidate.path.starts_with(&entry.path))
      .map(|candidate| candidate.path.clone())
      .collect::<Vec<_>>();
    entry_paths.sort();
    let descriptor_entry = compound
      .children(&entry.path)?
      .into_iter()
      .find(|child| child.is_stream() && child.name.eq_ignore_ascii_case(OBJECT_INFO_STREAM_NAME));
    let Some(descriptor_entry) = descriptor_entry else {
      let error = Error::invalid(0, "embedded object storage has no ObjInfo stream");
      if options.is_strict() {
        return Err(error);
      }
      report_object_pool_compatibility(diagnostics, &entry.path, error.to_string());
      compatibility_objects.push(DocCompatibilityObjectStorage {
        path: entry.path.clone(),
        descriptor_stream_path: None,
        entry_paths,
        reason: error.to_string(),
      });
      continue;
    };
    let descriptor = match OleObjectDescriptor::from_bytes(&descriptor_entry.data) {
      Ok(value) => value,
      Err(error) if options.is_strict() => return Err(error),
      Err(error) => {
        report_object_pool_compatibility(diagnostics, &descriptor_entry.path, error.to_string());
        compatibility_objects.push(DocCompatibilityObjectStorage {
          path: entry.path.clone(),
          descriptor_stream_path: Some(descriptor_entry.path.clone()),
          entry_paths,
          reason: error.to_string(),
        });
        continue;
      }
    };
    objects.push(DocEmbeddedObjectStorage {
      path: entry.path.clone(),
      descriptor_stream_path: descriptor_entry.path.clone(),
      descriptor,
      entry_paths,
    });
  }
  ensure_entry_limit(
    "DOC ObjectPool objects",
    objects.len().saturating_add(compatibility_objects.len()),
    options.limits,
  )?;
  objects.sort_by(|left, right| left.path.cmp(&right.path));
  compatibility_objects.sort_by(|left, right| left.path.cmp(&right.path));
  Ok(Some(DocObjectPoolStorage {
    path: pool.path.clone(),
    objects,
    compatibility_objects,
  }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DataReferenceKind {
  PictureOrBinary,
  ParagraphProperties,
  Ambiguous,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DocDataLinkBaseline {
  source_stream_present: bool,
  unresolved_references: BTreeMap<u32, DataReferenceKind>,
}

fn rebuild_data_stream<'a>(data: Option<&'a DocDataStream>) -> Result<RebuiltDataStream<'a>> {
  let Some(data) = data else {
    return Ok(RebuiltDataStream {
      plan: None,
      relocations: BTreeMap::new(),
    });
  };
  let mut measured = Vec::with_capacity(data.nodes.len());
  let mut prepared = Vec::with_capacity(data.nodes.len());
  for node in &data.nodes {
    let encoded = encode_data_node_value(&node.value)?;
    measured.push((
      usize::try_from(node.offset)
        .map_err(|_| Error::Limit("Data node offset exceeds usize".into()))?,
      node.physical_len,
      encoded.len(),
    ));
    prepared
      .push((!matches!(node.value, DocDataNodeValue::ParagraphProperties(_))).then_some(encoded));
  }
  let measured_relocation =
    plan_table_relocation(data.physical_bytes.len(), measured, "Data node")?;
  let mut relocations = BTreeMap::new();
  for node in &data.nodes {
    let location = FibFcLcb {
      fc: node.offset,
      lcb: u32::try_from(node.physical_len)
        .map_err(|_| Error::Limit("Data node length exceeds u32".into()))?,
    };
    let relocated = measured_relocation
      .relocate(location)?
      .ok_or_else(|| Error::invalid(u64::from(node.offset), "Data node exceeds stream"))?;
    relocations.insert(node.offset, relocated.fc);
  }

  let mut emitted = TableLayout::new(&data.physical_bytes);
  for (node, prepared) in data.nodes.iter().zip(prepared) {
    let encoded = match (&node.value, prepared) {
      (DocDataNodeValue::ParagraphProperties(properties), None) => {
        let mut properties = properties.clone();
        relocate_grpprl_data_references(&mut properties.properties, &relocations)?;
        properties.to_bytes()?
      }
      (_, Some(encoded)) => encoded,
      (_, None) => unreachable!("only paragraph properties require relocation"),
    };
    emitted.replace(
      usize::try_from(node.offset)
        .map_err(|_| Error::Limit("Data node offset exceeds usize".into()))?,
      node.physical_len,
      encoded,
      "Data node",
    )?;
  }
  let (plan, emitted_relocation) = emitted.finish_plan()?;
  for node in &data.nodes {
    let location = FibFcLcb {
      fc: node.offset,
      lcb: u32::try_from(node.physical_len)
        .map_err(|_| Error::Limit("Data node length exceeds u32".into()))?,
    };
    let emitted = emitted_relocation
      .relocate(location)?
      .ok_or_else(|| Error::invalid(u64::from(node.offset), "Data node exceeds stream"))?;
    if emitted.fc != relocations[&node.offset] {
      return Err(Error::invalid(
        0,
        "Data layout changed between measure and emit",
      ));
    }
  }
  Ok(RebuiltDataStream {
    plan: Some(plan),
    relocations,
  })
}

fn encode_data_node_value(value: &DocDataNodeValue) -> Result<Vec<u8>> {
  match value {
    DocDataNodeValue::Picture(value) => value.to_bytes_with_computed_length(),
    DocDataNodeValue::Binary(value) => value.to_bytes_with_computed_length(),
    DocDataNodeValue::ParagraphProperties(value) => value.to_bytes(),
  }
}

fn relocate_root_data_references(
  clx: &mut DocLocated<Clx>,
  character_pages: &mut [DocFkpPage<ChpxFkp>],
  paragraph_pages: &mut [DocFkpPage<PapxFkp>],
  sections: &mut [DocSectionProperties],
  styles: Option<&mut DocLocated<StyleSheet>>,
  lists: Option<&mut DocListDefinitions>,
  relocations: &BTreeMap<u32, u32>,
) -> Result<()> {
  if relocations.is_empty() {
    return Ok(());
  }
  for property_run in &mut clx.value.property_runs {
    relocate_grpprl_data_references(&mut property_run.properties, relocations)?;
  }
  for page in character_pages {
    for run in &mut page.value.runs {
      if let Some(properties) = &mut run.properties {
        relocate_grpprl_data_references(Arc::make_mut(properties), relocations)?;
      }
    }
  }
  for page in paragraph_pages {
    for run in &mut page.value.runs {
      if let Some(properties) = &mut run.properties {
        relocate_grpprl_data_references(&mut Arc::make_mut(properties).properties, relocations)?;
      }
    }
  }
  for section in sections {
    if let Some(value) = &mut section.value {
      relocate_grpprl_data_references(&mut value.properties, relocations)?;
    }
  }
  if let Some(styles) = styles {
    if let Some(properties) = &mut styles.value.info.standard_character_properties {
      relocate_grpprl_data_references(properties, relocations)?;
    }
    if let Some(properties) = &mut styles.value.info.standard_paragraph_properties {
      relocate_grpprl_data_references(properties, relocations)?;
    }
    for style in &mut styles.value.styles {
      if let Some(definition) = &mut style.definition {
        relocate_style_data_references(&mut definition.formatting, relocations)?;
      }
    }
  }
  if let Some(lists) = lists {
    for definition in &mut lists.value.definitions {
      for level in &mut definition.levels {
        relocate_grpprl_data_references(&mut level.paragraph_properties, relocations)?;
        relocate_grpprl_data_references(&mut level.number_properties, relocations)?;
      }
    }
  }
  Ok(())
}

fn relocate_style_data_references(
  formatting: &mut StyleFormatting,
  relocations: &BTreeMap<u32, u32>,
) -> Result<()> {
  let visit = |properties: &mut GrpPrl| relocate_grpprl_data_references(properties, relocations);
  match formatting {
    StyleFormatting::Paragraph {
      paragraph,
      character,
    } => {
      visit(&mut paragraph.properties)?;
      visit(&mut character.properties)?;
    }
    StyleFormatting::Character { character } => visit(&mut character.properties)?,
    StyleFormatting::RevisionParagraph {
      paragraph,
      character,
      original_paragraph,
      original_character,
      ..
    } => {
      visit(&mut paragraph.properties)?;
      visit(&mut character.properties)?;
      visit(&mut original_paragraph.properties)?;
      visit(&mut original_character.properties)?;
    }
    StyleFormatting::RevisionCharacter {
      character,
      original_character,
      ..
    } => {
      visit(&mut character.properties)?;
      visit(&mut original_character.properties)?;
    }
    StyleFormatting::Table {
      table,
      paragraph,
      character,
    } => {
      visit(&mut table.properties)?;
      visit(&mut paragraph.properties)?;
      visit(&mut character.properties)?;
    }
    StyleFormatting::Numbering { paragraph } => visit(&mut paragraph.properties)?,
  }
  Ok(())
}

fn relocate_grpprl_data_references(
  properties: &mut GrpPrl,
  relocations: &BTreeMap<u32, u32>,
) -> Result<()> {
  let special_character = properties.properties.iter().rev().find_map(|property| {
    (property.sprm.opcode().ok() == Some(0x0855)).then_some(match property.operand {
      SprmOperand::Toggle(value) => value & 1 != 0,
      _ => false,
    })
  });
  let ole_object = properties.properties.iter().rev().find_map(|property| {
    (property.sprm.opcode().ok() == Some(0x080a)).then_some(match property.operand {
      SprmOperand::Toggle(value) => value & 1 != 0,
      _ => false,
    })
  });
  for property in &mut properties.properties {
    let opcode = property.sprm.opcode()?;
    let is_reference = matches!(opcode, 0x646b | 0x6646)
      || (opcode == 0x6a03 && special_character == Some(true) && ole_object != Some(true));
    if is_reference {
      let SprmOperand::Dword(raw_offset) = &mut property.operand else {
        return Err(Error::invalid(
          0,
          "Data-reference SPRM operand is not a dword",
        ));
      };
      let offset = u32::from_le_bytes(*raw_offset);
      if let Some(relocated) = relocations.get(&offset) {
        *raw_offset = relocated.to_le_bytes();
      }
    }
    if let SprmOperand::CharacterMajority(nested) = &mut property.operand {
      relocate_grpprl_data_references(nested, relocations)?;
    }
  }
  Ok(())
}

#[derive(Clone, Copy, Debug, Default)]
struct CharacterDataReferenceState {
  picture_offset: Option<u32>,
  ole_object: Option<bool>,
}

impl CharacterDataReferenceState {
  fn apply(&mut self, properties: &GrpPrl) {
    for property in &properties.properties {
      match (property.sprm.opcode().ok(), &property.operand) {
        (Some(0x6a03), SprmOperand::Dword(bytes)) => {
          self.picture_offset = Some(u32::from_le_bytes(*bytes));
        }
        (Some(0x080a), SprmOperand::Toggle(value)) => {
          self.ole_object = Some(value & 1 != 0);
        }
        (_, SprmOperand::CharacterMajority(nested)) => self.apply(nested),
        _ => {}
      }
    }
  }
}

fn collect_nil_picf_field_types(
  word: &DocWordDocumentStream,
  table: &DocTableStream,
) -> BTreeMap<u32, Option<NilPicfFieldType>> {
  let mut references = BTreeMap::new();
  let mut picture_characters = Vec::new();
  for (descriptor, piece) in table
    .clx
    .value
    .piece_table
    .pieces
    .iter()
    .zip(&word.text_pieces)
  {
    let mut base = CharacterDataReferenceState::default();
    if let Prm::Complex { property_run_index } = descriptor.property_modifier
      && let Some(properties) = table
        .clx
        .value
        .property_runs
        .get(usize::from(property_run_index))
    {
      base.apply(&properties.properties);
    }
    picture_characters.extend(
      picture_characters_in_piece(&piece.value)
        .into_iter()
        .map(|(cp, fc)| (cp, fc, base)),
    );
  }
  for page in &word.character_format_pages {
    for (index, run) in page.value.runs.iter().enumerate() {
      let Some((&fc_start, &fc_end)) = page
        .value
        .file_positions
        .get(index)
        .zip(page.value.file_positions.get(index + 1))
      else {
        continue;
      };
      for &(cp, fc, base) in &picture_characters {
        if !(fc_start..fc_end).contains(&fc) {
          continue;
        }
        let mut state = base;
        if let Some(properties) = &run.properties {
          state.apply(properties);
        }
        let Some(offset) = state
          .picture_offset
          .filter(|_| state.ole_object != Some(true))
        else {
          continue;
        };
        let Some(field_type) = nil_picf_field_type_at_cp(&word.fib, table, cp)
          .or_else(|| private_field_type_at_cp(word, cp))
        else {
          continue;
        };
        references
          .entry(offset)
          .and_modify(|existing| {
            if *existing != Some(field_type) {
              *existing = None;
            }
          })
          .or_insert(Some(field_type));
      }
    }
  }
  references
}

fn picture_characters_in_piece(piece: &TextPiece) -> Vec<(u32, u32)> {
  let width = match piece.characters.encoding() {
    TextPieceEncoding::Compressed => 1,
    TextPieceEncoding::Utf16 => 2,
  };
  piece
    .characters
    .code_units_iter()
    .map(|value| value == 1)
    .enumerate()
    .filter_map(|(index, is_picture)| {
      if !is_picture {
        return None;
      }
      let fc = u64::from(piece.file_offset).checked_add(index as u64 * width)?;
      let cp = i64::from(piece.cp_start).checked_add(index as i64)?;
      Some((u32::try_from(cp).ok()?, u32::try_from(fc).ok()?))
    })
    .collect()
}

fn nil_picf_field_type_at_cp(
  fib: &Fib,
  table: &DocTableStream,
  global_cp: u32,
) -> Option<NilPicfFieldType> {
  let counts = [
    (FieldDocumentPart::Main, fib.rg_lw.ccp_text),
    (FieldDocumentPart::Footnote, fib.rg_lw.ccp_footnote),
    (FieldDocumentPart::Header, fib.rg_lw.ccp_header),
    (FieldDocumentPart::Comment, fib.rg_lw.ccp_comment),
    (FieldDocumentPart::Endnote, fib.rg_lw.ccp_endnote),
    (FieldDocumentPart::Textbox, fib.rg_lw.ccp_textbox),
    (
      FieldDocumentPart::HeaderTextbox,
      fib.rg_lw.ccp_header_textbox,
    ),
  ];
  let mut start = 0u32;
  for (part, count) in counts {
    let count = u32::try_from(count).ok()?;
    let end = start.checked_add(count)?;
    if (start..end).contains(&global_cp) {
      let relative_cp = global_cp - start;
      let field = table.fields.get(&part)?.value.innermost_at(relative_cp)?;
      return NilPicfFieldType::from_field_type(field.begin.field_type);
    }
    start = end;
  }
  None
}

fn private_field_type_at_cp(
  word: &DocWordDocumentStream,
  picture_cp: u32,
) -> Option<NilPicfFieldType> {
  let mut nesting = 0usize;
  let mut begin = None;
  for cp in (0..picture_cp).rev() {
    match text_character_at_cp(word, cp)? {
      0x0015 => nesting = nesting.saturating_add(1),
      0x0013 if nesting == 0 => {
        begin = Some(cp);
        break;
      }
      0x0013 => nesting = nesting.saturating_sub(1),
      _ => {}
    }
  }
  let begin = begin?;
  let mut instruction = String::new();
  let mut nested = 0usize;
  for cp in begin + 1..picture_cp {
    match text_character_at_cp(word, cp)? {
      0x0013 => nested = nested.saturating_add(1),
      0x0015 if nested != 0 => nested = nested.saturating_sub(1),
      0x0014 | 0x0015 if nested == 0 => break,
      value if nested == 0 && value <= 0x007f => {
        instruction.push(char::from(value as u8));
      }
      _ => {}
    }
  }
  instruction
    .trim_start()
    .to_ascii_uppercase()
    .starts_with("PRIVATE")
    .then_some(NilPicfFieldType::Private(PrivateFieldType::Private))
}

fn text_character_at_cp(word: &DocWordDocumentStream, cp: u32) -> Option<u16> {
  let cp = i32::try_from(cp).ok()?;
  let piece = word
    .text_pieces
    .iter()
    .find(|piece| piece.value.cp_start <= cp && cp < piece.value.cp_end)?;
  let index = usize::try_from(cp - piece.value.cp_start).ok()?;
  piece.value.characters.code_units().get(index).copied()
}

fn parse_data_stream(
  bytes: Option<CfbStreamData>,
  word: &DocWordDocumentStream,
  table: &DocTableStream,
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<Option<DocDataStream>> {
  let mut references = collect_data_references(word, table)?;
  let binary_field_types = collect_nil_picf_field_types(word, table);
  let Some(physical_bytes) = bytes else {
    if references.is_empty() {
      return Ok(None);
    }
    let error = Error::invalid(0, "SPRM references exist but the Data stream is missing");
    if options.is_strict() {
      return Err(error);
    }
    report_data_compatibility(diagnostics, 0, error.to_string());
    return Ok(None);
  };
  let bytes = physical_bytes.as_slice();
  ensure_stream_limit("Data", bytes, options.limits)?;

  let mut pending = references
    .iter()
    .map(|(offset, kind)| (*offset, *kind))
    .collect::<Vec<_>>();
  let mut index = 0usize;
  let mut nodes = Vec::new();
  while index < pending.len() {
    let (offset, kind) = pending[index];
    index += 1;
    let parsed = parse_data_node(bytes, offset, kind);
    let mut node = match parsed {
      Ok(value) => value,
      Err(error) if options.is_strict() => return Err(error),
      Err(error) => {
        report_data_compatibility(diagnostics, u64::from(offset), error.to_string());
        continue;
      }
    };
    if let DocDataNodeValue::Binary(value) = &mut node.value {
      match binary_field_types.get(&offset) {
        Some(Some(field_type)) => value.interpret(*field_type),
        Some(None) | None if options.is_strict() => {
          return Err(Error::invalid(
            u64::from(offset),
            "NilPICF picture character is not inside one permitted field type",
          ));
        }
        Some(None) | None => {
          value.mark_invalid_context();
          report_data_compatibility(
            diagnostics,
            u64::from(offset),
            "NilPICF picture character is not inside one permitted field type".into(),
          );
        }
      }
    }
    if let DocDataNodeValue::ParagraphProperties(value) = &node.value {
      let mut nested = BTreeMap::new();
      collect_grpprl_data_references(&value.properties, &mut nested)?;
      for (nested_offset, nested_kind) in nested {
        match references.get(&nested_offset) {
          Some(existing) if *existing != nested_kind => {
            references.insert(nested_offset, DataReferenceKind::Ambiguous);
            if let Some(pending_entry) = pending
              .iter_mut()
              .find(|(pending_offset, _)| *pending_offset == nested_offset)
            {
              pending_entry.1 = DataReferenceKind::Ambiguous;
            }
          }
          Some(_) => {}
          None => {
            references.insert(nested_offset, nested_kind);
            pending.push((nested_offset, nested_kind));
          }
        }
      }
    }
    nodes.push(node);
  }
  nodes.sort_by_key(|node| node.offset);
  let mut retained = Vec::<DocDataNode>::with_capacity(nodes.len());
  for node in nodes {
    let overlaps = retained.last().is_some_and(|previous| {
      u64::from(previous.offset) + previous.physical_len as u64 > u64::from(node.offset)
    });
    if overlaps {
      let error = Error::invalid(
        u64::from(node.offset),
        "referenced Data stream structures overlap",
      );
      if options.is_strict() {
        return Err(error);
      }
      report_data_compatibility(diagnostics, u64::from(node.offset), error.to_string());
    } else {
      retained.push(node);
    }
  }
  ensure_entry_limit("DOC Data nodes", retained.len(), options.limits)?;
  Ok(Some(DocDataStream {
    physical_bytes,
    nodes: retained,
  }))
}

fn parse_data_node(bytes: &[u8], offset: u32, kind: DataReferenceKind) -> Result<DocDataNode> {
  if kind == DataReferenceKind::Ambiguous {
    let picture = parse_data_node(bytes, offset, DataReferenceKind::PictureOrBinary);
    let paragraph = parse_data_node(bytes, offset, DataReferenceKind::ParagraphProperties);
    return match (picture, paragraph) {
      (Ok(value), Err(_)) | (Err(_), Ok(value)) => Ok(value),
      (Ok(_), Ok(_)) => Err(Error::invalid(
        u64::from(offset),
        "Data offset is structurally ambiguous between picture and PrcData",
      )),
      (Err(picture_error), Err(paragraph_error)) => Err(Error::invalid(
        u64::from(offset),
        format!(
          "Data offset matches neither picture nor PrcData ({picture_error}; {paragraph_error})"
        ),
      )),
    };
  }
  let start =
    usize::try_from(offset).map_err(|_| Error::Limit("Data node offset exceeds usize".into()))?;
  let remaining = bytes
    .get(start..)
    .ok_or_else(|| Error::invalid(u64::from(offset), "Data node offset exceeds stream"))?;
  let (physical_len, value) = match kind {
    DataReferenceKind::PictureOrBinary => {
      let length_bytes = remaining
        .get(..4)
        .ok_or_else(|| Error::invalid(u64::from(offset), "Data picture length is missing"))?;
      let length = i32::from_le_bytes(length_bytes.try_into().expect("four bytes checked"));
      let length = usize::try_from(length)
        .map_err(|_| Error::invalid(u64::from(offset), "Data picture length is negative"))?;
      let encoded = remaining
        .get(..length)
        .ok_or_else(|| Error::invalid(u64::from(offset), "Data picture exceeds the stream"))?;
      let mapping_mode = encoded
        .get(6..8)
        .map(|value| i16::from_le_bytes(value.try_into().expect("two bytes checked")));
      let value = if matches!(mapping_mode, Some(0x0064 | 0x0066)) {
        DocDataNodeValue::Picture(PicfAndOfficeArtData::from_bytes(encoded)?)
      } else {
        DocDataNodeValue::Binary(Box::new(NilPicfAndBinData::from_bytes(encoded)?))
      };
      (length, value)
    }
    DataReferenceKind::ParagraphProperties => {
      let length_bytes = remaining
        .get(..2)
        .ok_or_else(|| Error::invalid(u64::from(offset), "PrcData length is missing"))?;
      let length = i16::from_le_bytes(length_bytes.try_into().expect("two bytes checked"));
      if !(0..=0x3fa2).contains(&length) {
        return Err(Error::invalid(
          u64::from(offset),
          "PrcData cbGrpprl is outside 0..=0x3FA2",
        ));
      }
      let physical_len = length as usize + 2;
      let encoded = remaining
        .get(..physical_len)
        .ok_or_else(|| Error::invalid(u64::from(offset), "PrcData exceeds the Data stream"))?;
      (
        physical_len,
        DocDataNodeValue::ParagraphProperties(PrcData::from_bytes(encoded)?),
      )
    }
    DataReferenceKind::Ambiguous => unreachable!("handled above"),
  };
  Ok(DocDataNode {
    offset,
    physical_len,
    value,
  })
}

fn collect_data_references(
  word: &DocWordDocumentStream,
  table: &DocTableStream,
) -> Result<BTreeMap<u32, DataReferenceKind>> {
  let mut references = BTreeMap::new();
  for property_run in &table.clx.value.property_runs {
    collect_grpprl_data_references(&property_run.properties, &mut references)?;
  }
  for page in &word.character_format_pages {
    for run in &page.value.runs {
      if let Some(properties) = &run.properties {
        collect_grpprl_data_references(properties, &mut references)?;
      }
    }
  }
  for page in &word.paragraph_format_pages {
    for run in &page.value.runs {
      if let Some(properties) = &run.properties {
        collect_grpprl_data_references(&properties.properties, &mut references)?;
      }
    }
  }
  for section in &word.section_properties {
    if let Some(value) = &section.value {
      collect_grpprl_data_references(&value.properties, &mut references)?;
    }
  }
  if let Some(styles) = &table.styles {
    if let Some(properties) = &styles.value.info.standard_character_properties {
      collect_grpprl_data_references(properties, &mut references)?;
    }
    if let Some(properties) = &styles.value.info.standard_paragraph_properties {
      collect_grpprl_data_references(properties, &mut references)?;
    }
    for style in &styles.value.styles {
      if let Some(definition) = &style.definition {
        collect_style_data_references(&definition.formatting, &mut references)?;
      }
    }
  }
  if let Some(lists) = &table.list_definitions {
    for definition in &lists.value.definitions {
      for level in &definition.levels {
        collect_grpprl_data_references(&level.paragraph_properties, &mut references)?;
        collect_grpprl_data_references(&level.number_properties, &mut references)?;
      }
    }
  }
  Ok(references)
}

fn collect_style_data_references(
  formatting: &StyleFormatting,
  references: &mut BTreeMap<u32, DataReferenceKind>,
) -> Result<()> {
  let mut visit = |properties: &GrpPrl| collect_grpprl_data_references(properties, references);
  match formatting {
    StyleFormatting::Paragraph {
      paragraph,
      character,
    } => {
      visit(&paragraph.properties)?;
      visit(&character.properties)?;
    }
    StyleFormatting::Character { character } => visit(&character.properties)?,
    StyleFormatting::RevisionParagraph {
      paragraph,
      character,
      original_paragraph,
      original_character,
      ..
    } => {
      visit(&paragraph.properties)?;
      visit(&character.properties)?;
      visit(&original_paragraph.properties)?;
      visit(&original_character.properties)?;
    }
    StyleFormatting::RevisionCharacter {
      character,
      original_character,
      ..
    } => {
      visit(&character.properties)?;
      visit(&original_character.properties)?;
    }
    StyleFormatting::Table {
      table,
      paragraph,
      character,
    } => {
      visit(&table.properties)?;
      visit(&paragraph.properties)?;
      visit(&character.properties)?;
    }
    StyleFormatting::Numbering { paragraph } => visit(&paragraph.properties)?,
  }
  Ok(())
}

fn collect_grpprl_data_references(
  properties: &GrpPrl,
  references: &mut BTreeMap<u32, DataReferenceKind>,
) -> Result<()> {
  let special_character = properties.properties.iter().rev().find_map(|property| {
    (property.sprm.opcode().ok() == Some(0x0855)).then_some(match property.operand {
      SprmOperand::Toggle(value) => value & 1 != 0,
      _ => false,
    })
  });
  let ole_object = properties.properties.iter().rev().find_map(|property| {
    (property.sprm.opcode().ok() == Some(0x080a)).then_some(match property.operand {
      SprmOperand::Toggle(value) => value & 1 != 0,
      _ => false,
    })
  });
  for property in &properties.properties {
    let opcode = property.sprm.opcode()?;
    let kind = match opcode {
      0x6a03 if special_character == Some(true) && ole_object != Some(true) => {
        Some(DataReferenceKind::PictureOrBinary)
      }
      0x646b | 0x6646 => Some(DataReferenceKind::ParagraphProperties),
      _ => None,
    };
    if let Some(kind) = kind {
      let SprmOperand::Dword(raw_offset) = property.operand else {
        return Err(Error::invalid(
          0,
          "Data-reference SPRM operand is not a dword",
        ));
      };
      let offset = u32::from_le_bytes(raw_offset);
      references
        .entry(offset)
        .and_modify(|existing| {
          if *existing != kind {
            *existing = DataReferenceKind::Ambiguous;
          }
        })
        .or_insert(kind);
    }
    if let SprmOperand::CharacterMajority(nested) = &property.operand {
      collect_grpprl_data_references(nested, references)?;
    }
  }
  Ok(())
}

fn report_data_compatibility(diagnostics: &mut Vec<ParseDiagnostic>, offset: u64, message: String) {
  diagnostics.push(ParseDiagnostic::warning(
    ParseDiagnosticCode::InvalidReference,
    BinaryFormat::Doc,
    Some(DATA_STREAM_PATH),
    Some(offset),
    "Data Stream",
    SpecificationReference {
      document: "MS-DOC",
      section: "2.1.3",
    },
    message,
  ));
}

fn report_object_pool_compatibility(
  diagnostics: &mut Vec<ParseDiagnostic>,
  path: &Path,
  message: String,
) {
  let path = path.display().to_string();
  diagnostics.push(ParseDiagnostic::warning(
    ParseDiagnosticCode::InvalidStreamPreserved,
    BinaryFormat::Doc,
    Some(&path),
    None,
    "ObjectPool Storage",
    SpecificationReference {
      document: "MS-DOC",
      section: "2.1.4",
    },
    message,
  ));
}

fn validate_object_pool_links(
  compound: &CompoundFile,
  object_pool: Option<&DocObjectPoolStorage>,
) -> Result<()> {
  let physical_pool = compound
    .entry(OBJECT_POOL_STORAGE_PATH)
    .filter(|entry| entry.is_storage());
  if physical_pool.map(|entry| &entry.path) != object_pool.map(|pool| &pool.path) {
    return Err(Error::invalid(0, "DOC ObjectPool root link changed"));
  }
  let Some(object_pool) = object_pool else {
    return Ok(());
  };

  let mut physical_objects = Vec::new();
  let mut physical_storage_paths = Vec::new();
  for entry in compound.children(&object_pool.path)? {
    if !entry.is_storage() {
      continue;
    }
    physical_storage_paths.push(entry.path.clone());
    let Some(descriptor_entry) = compound.children(&entry.path)?.into_iter().find(|child| {
      child.is_stream()
        && child.name.eq_ignore_ascii_case(OBJECT_INFO_STREAM_NAME)
        && OleObjectDescriptor::from_bytes(&child.data).is_ok()
    }) else {
      continue;
    };
    physical_objects.push((entry.path.clone(), descriptor_entry.path.clone()));
  }
  physical_objects.sort();
  physical_storage_paths.sort();
  let managed_objects = object_pool
    .objects
    .iter()
    .map(|object| (object.path.clone(), object.descriptor_stream_path.clone()))
    .collect::<Vec<_>>();
  if physical_objects != managed_objects {
    return Err(Error::invalid(0, "DOC ObjectPool object links changed"));
  }
  let mut modeled_storage_paths = object_pool
    .objects
    .iter()
    .map(|object| object.path.clone())
    .chain(
      object_pool
        .compatibility_objects
        .iter()
        .map(|object| object.path.clone()),
    )
    .collect::<Vec<_>>();
  modeled_storage_paths.sort();
  if physical_storage_paths != modeled_storage_paths {
    return Err(Error::invalid(
      0,
      "DOC ObjectPool compatibility storage links changed",
    ));
  }
  for object in &object_pool.objects {
    let mut physical_paths = compound
      .entries()
      .iter()
      .filter(|entry| entry.path.starts_with(&object.path))
      .map(|entry| entry.path.clone())
      .collect::<Vec<_>>();
    physical_paths.sort();
    if physical_paths != object.entry_paths {
      return Err(Error::invalid(
        0,
        "DOC ObjectPool embedded entry links changed",
      ));
    }
  }
  for object in &object_pool.compatibility_objects {
    let mut physical_paths = compound
      .entries()
      .iter()
      .filter(|entry| entry.path.starts_with(&object.path))
      .map(|entry| entry.path.clone())
      .collect::<Vec<_>>();
    physical_paths.sort();
    if physical_paths != object.entry_paths {
      return Err(Error::invalid(
        0,
        "DOC ObjectPool compatibility entry links changed",
      ));
    }
  }
  Ok(())
}

fn build_data_link_baseline(
  source_stream_present: bool,
  word: &DocWordDocumentStream,
  table: &DocTableStream,
  data: Option<&DocDataStream>,
) -> Result<DocDataLinkBaseline> {
  let nodes = data
    .into_iter()
    .flat_map(|data| &data.nodes)
    .map(|node| (node.offset, node))
    .collect::<BTreeMap<_, _>>();
  let mut unresolved_references = collect_complete_data_references(word, table, &nodes)?;
  unresolved_references.retain(|offset, _| !nodes.contains_key(offset));
  Ok(DocDataLinkBaseline {
    source_stream_present,
    unresolved_references,
  })
}

fn collect_complete_data_references(
  word: &DocWordDocumentStream,
  table: &DocTableStream,
  nodes: &BTreeMap<u32, &DocDataNode>,
) -> Result<BTreeMap<u32, DataReferenceKind>> {
  let mut references = collect_data_references(word, table)?;
  let mut pending = references
    .iter()
    .map(|(offset, kind)| (*offset, *kind))
    .collect::<Vec<_>>();
  let mut index = 0usize;
  while index < pending.len() {
    let (offset, _) = pending[index];
    index += 1;
    let Some(DocDataNode {
      value: DocDataNodeValue::ParagraphProperties(properties),
      ..
    }) = nodes.get(&offset).copied()
    else {
      continue;
    };
    let mut nested = BTreeMap::new();
    collect_grpprl_data_references(&properties.properties, &mut nested)?;
    for (nested_offset, nested_kind) in nested {
      match references.get_mut(&nested_offset) {
        Some(existing) if *existing != nested_kind => {
          *existing = DataReferenceKind::Ambiguous;
          if let Some(pending_entry) = pending
            .iter_mut()
            .find(|(pending_offset, _)| *pending_offset == nested_offset)
          {
            pending_entry.1 = DataReferenceKind::Ambiguous;
          }
        }
        Some(_) => {}
        None => {
          references.insert(nested_offset, nested_kind);
          pending.push((nested_offset, nested_kind));
        }
      }
    }
  }
  Ok(references)
}

fn reference_kinds_match(left: DataReferenceKind, right: DataReferenceKind) -> bool {
  left == right
    || matches!(left, DataReferenceKind::Ambiguous)
    || matches!(right, DataReferenceKind::Ambiguous)
}

fn data_node_matches_reference(node: &DocDataNode, kind: DataReferenceKind) -> bool {
  match kind {
    DataReferenceKind::PictureOrBinary => matches!(
      node.value,
      DocDataNodeValue::Picture(_) | DocDataNodeValue::Binary(_)
    ),
    DataReferenceKind::ParagraphProperties => {
      matches!(node.value, DocDataNodeValue::ParagraphProperties(_))
    }
    DataReferenceKind::Ambiguous => true,
  }
}

fn validate_data_links(
  word: &DocWordDocumentStream,
  table: &DocTableStream,
  data: Option<&DocDataStream>,
  baseline: &DocDataLinkBaseline,
) -> Result<()> {
  let Some(data) = data else {
    let nodes = BTreeMap::new();
    let references = collect_complete_data_references(word, table, &nodes)?;
    if references.is_empty()
      || (!baseline.source_stream_present
        && references.iter().all(|(offset, kind)| {
          baseline
            .unresolved_references
            .get(offset)
            .is_some_and(|baseline_kind| reference_kinds_match(*kind, *baseline_kind))
        }))
    {
      return Ok(());
    }
    return Err(Error::invalid(
      0,
      "DOC SPRM references exist but the Data stream was removed",
    ));
  };

  let mut nodes = BTreeMap::new();
  let mut previous_end = 0usize;
  for node in &data.nodes {
    let offset = usize::try_from(node.offset)
      .map_err(|_| Error::Limit("Data node offset exceeds usize".into()))?;
    let end = offset
      .checked_add(node.physical_len)
      .ok_or_else(|| Error::Limit("Data node physical range overflow".into()))?;
    if end > data.physical_bytes.len() {
      return Err(Error::invalid(
        u64::from(node.offset),
        "DOC Data node physical range exceeds the stream",
      ));
    }
    if offset < previous_end {
      return Err(Error::invalid(
        u64::from(node.offset),
        "DOC Data nodes overlap or are not in physical order",
      ));
    }
    previous_end = end;
    if nodes.insert(node.offset, node).is_some() {
      return Err(Error::invalid(
        u64::from(node.offset),
        "DOC Data node offset is duplicated",
      ));
    }
  }

  let references = collect_complete_data_references(word, table, &nodes)?;
  for (offset, kind) in &references {
    let Some(node) = nodes.get(offset) else {
      if baseline
        .unresolved_references
        .get(offset)
        .is_some_and(|baseline_kind| reference_kinds_match(*kind, *baseline_kind))
      {
        continue;
      }
      return Err(Error::invalid(
        u64::from(*offset),
        "DOC SPRM references a missing typed Data node",
      ));
    };
    if !data_node_matches_reference(node, *kind) {
      return Err(Error::invalid(
        u64::from(*offset),
        "DOC SPRM/Data node type link changed",
      ));
    }
  }
  if nodes.keys().any(|offset| !references.contains_key(offset)) {
    return Err(Error::invalid(0, "DOC SPRM/Data node links changed"));
  }
  Ok(())
}

fn parse_fkp_pages<T>(
  table: &PlcBte,
  word: &[u8],
  parse: impl Fn(&[u8]) -> Result<T>,
) -> Result<Vec<DocFkpPage<T>>> {
  table
    .pages
    .iter()
    .copied()
    .map(|page| {
      let offset = page.byte_offset()?;
      let bytes = word
        .get(offset..offset + 512)
        .ok_or_else(|| Error::invalid(offset as u64, "FKP page exceeds WordDocument"))?;
      Ok(DocFkpPage {
        page,
        value: parse(bytes)?,
      })
    })
    .collect()
}

fn validate_fkp_page_order<T>(
  pages: &[DocFkpPage<T>],
  positions: impl Fn(&T) -> &[u32],
  structure: &'static str,
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<()> {
  for page in pages {
    let Some((index, pair)) = positions(&page.value)
      .windows(2)
      .enumerate()
      .find(|(_, pair)| pair[0] >= pair[1])
    else {
      continue;
    };
    let offset = page
      .page
      .byte_offset()?
      .checked_add((index + 1) * 4)
      .ok_or_else(|| Error::Limit("FKP boundary offset overflow".into()))?;
    let error = Error::invalid(
      offset as u64,
      format!(
        "{structure} values are not strictly increasing: {} then {}",
        pair[0], pair[1]
      ),
    );
    if options.is_strict() {
      return Err(error);
    }
    diagnostics.push(ParseDiagnostic::warning(
      ParseDiagnosticCode::NonconformingRecord,
      BinaryFormat::Doc,
      Some("WordDocument Stream"),
      Some(offset as u64),
      structure,
      SpecificationReference {
        document: "MS-DOC",
        section: "2.9",
      },
      error.to_string(),
    ));
  }
  Ok(())
}

fn parse_bookmarks(fib: &Fib, table: &[u8]) -> Result<Option<DocLocatedBookmarks>> {
  let Some((names, starts, ends)) = fib.bookmark_locations() else {
    return Ok(None);
  };
  if names.lcb == 0 && starts.lcb == 0 && ends.lcb == 0 {
    return Ok(None);
  }
  if names.lcb == 0 || starts.lcb == 0 || ends.lcb == 0 {
    return Err(Error::invalid(0, "bookmark table locations are incomplete"));
  }
  Ok(Some(DocLocatedBookmarks {
    names_location: names,
    starts_location: starts,
    ends_location: ends,
    value: Bookmarks::from_bytes(
      bounded_slice(table, names, "SttbfBkmk")?,
      bounded_slice(table, starts, "PlcfBkf")?,
      bounded_slice(table, ends, "PlcfBkl")?,
    )?,
  }))
}

fn parse_note_tables(
  table: &[u8],
  locations: Option<(FibFcLcb, FibFcLcb)>,
  label: &'static str,
) -> Result<Option<DocNoteTables>> {
  let Some((references, text)) = complete_locations(locations, label)? else {
    return Ok(None);
  };
  Ok(Some(DocNoteTables {
    references: DocLocated {
      location: references,
      value: NoteReferenceTable::from_bytes(bounded_slice(
        table,
        references,
        &format!("{label} reference table"),
      )?)?,
    },
    text: DocLocated {
      location: text,
      value: CpOnlyTable::from_bytes(bounded_slice(table, text, &format!("{label} text table"))?)?,
    },
  }))
}

fn parse_annotation_tables(
  table: &[u8],
  locations: Option<(FibFcLcb, FibFcLcb)>,
) -> Result<Option<DocAnnotationTables>> {
  let Some((references, text)) = complete_locations(locations, "annotation")? else {
    return Ok(None);
  };
  Ok(Some(DocAnnotationTables {
    references: DocLocated {
      location: references,
      value: AnnotationReferenceTable::from_bytes(bounded_slice(
        table,
        references,
        "annotation reference table",
      )?)?,
    },
    text: DocLocated {
      location: text,
      value: CpOnlyTable::from_bytes(bounded_slice(table, text, "annotation text table")?)?,
    },
  }))
}

fn parse_annotation_bookmarks(
  fib: &Fib,
  table: &[u8],
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<Option<DocLocatedAnnotationBookmarks>> {
  let Some((infos, starts, ends)) = fib.annotation_bookmark_locations() else {
    return Ok(None);
  };
  let [infos, starts, ends] =
    match complete_location_array([infos, starts, ends], "annotation bookmark tables") {
      Ok(Some(locations)) => locations,
      Ok(None) => return Ok(None),
      Err(error) if options.is_strict() => return Err(error),
      Err(error) => {
        report_doc_compatibility(
          diagnostics,
          ParseDiagnosticCode::InvalidReference,
          0,
          "annotation bookmark tables",
          format!("preserved incomplete annotation bookmark tables: {error}"),
        );
        return Ok(None);
      }
    };
  Ok(Some(DocLocatedAnnotationBookmarks {
    infos_location: infos,
    starts_location: starts,
    ends_location: ends,
    value: AnnotationBookmarks::from_bytes(
      bounded_slice(table, infos, "SttbfAtnBkmk")?,
      bounded_slice(table, starts, "PlcfAtnBkf")?,
      bounded_slice(table, ends, "PlcfAtnBkl")?,
    )?,
  }))
}

fn parse_optional_compatible<T>(
  bytes: &[u8],
  location: Option<FibFcLcb>,
  label: &'static str,
  parse: impl Fn(&[u8]) -> Result<T>,
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
  compatibility_tables: &mut Vec<DocCompatibilityTable>,
) -> Result<Option<DocLocated<T>>> {
  match parse_optional(bytes, location, label, parse) {
    Ok(value) => Ok(value),
    Err(error) if options.is_strict() => Err(error),
    Err(error) => {
      let reason = error.to_string();
      report_doc_compatibility(
        diagnostics,
        ParseDiagnosticCode::InvalidReference,
        location.map_or(0, |value| u64::from(value.fc)),
        label,
        format!("preserved an invalid {label} reference: {reason}"),
      );
      if let Some(location) = location.filter(|value| value.lcb != 0) {
        compatibility_tables.push(DocCompatibilityTable {
          label: label.to_owned(),
          location,
          physical_bytes: bounded_slice(bytes, location, label)
            .ok()
            .map(<[u8]>::to_vec),
          reason,
        });
      }
      Ok(None)
    }
  }
}

fn parse_list_definitions(
  table: &[u8],
  location: Option<FibFcLcb>,
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
  compatibility_tables: &mut Vec<DocCompatibilityTable>,
) -> Result<Option<DocListDefinitions>> {
  let Some(location) = location.filter(|value| value.lcb != 0) else {
    return Ok(None);
  };
  match ListDefinitions::from_table_stream(table, location) {
    Ok(value) => {
      let (base, levels) = value.to_bytes()?;
      if base.len()
        != usize::try_from(location.lcb)
          .map_err(|_| Error::Limit("PlfLst length exceeds usize".into()))?
      {
        return Err(Error::invalid(
          u64::from(location.fc),
          "PlfLst static base does not match its FIB length",
        ));
      }
      Ok(Some(DocListDefinitions {
        location,
        value,
        trailing_levels_len: levels.len(),
      }))
    }
    Err(error) if options.is_strict() => Err(error),
    Err(error) => {
      let reason = error.to_string();
      report_doc_compatibility(
        diagnostics,
        ParseDiagnosticCode::NonconformingRecord,
        u64::from(location.fc),
        "PlfLst",
        format!("preserved an invalid PlfLst: {reason}"),
      );
      compatibility_tables.push(DocCompatibilityTable {
        label: "PlfLst".to_owned(),
        location,
        physical_bytes: bounded_slice(table, location, "PlfLst")
          .ok()
          .map(<[u8]>::to_vec),
        reason,
      });
      Ok(None)
    }
  }
}

#[derive(Clone, Copy)]
struct DocCompatibilityGroup<const N: usize> {
  labels: [&'static str; N],
  structure: &'static str,
}

fn parse_bookmark_set<T>(
  table: &[u8],
  locations: Option<[FibFcLcb; 3]>,
  group: DocCompatibilityGroup<3>,
  parse: impl Fn(&[u8], &[u8], &[u8]) -> Result<T>,
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
  compatibility_tables: &mut Vec<DocCompatibilityTable>,
) -> Result<Option<DocBookmarkSet<T>>> {
  let Some(locations) = locations else {
    return Ok(None);
  };
  let locations = match complete_location_array(locations, group.structure) {
    Ok(Some(value)) => value,
    Ok(None) => return Ok(None),
    Err(error) => {
      return preserve_compatibility_group(
        table,
        locations,
        group,
        error,
        options,
        diagnostics,
        compatibility_tables,
      );
    }
  };
  let parsed = (|| {
    parse(
      bounded_slice(table, locations[0], group.labels[0])?,
      bounded_slice(table, locations[1], group.labels[1])?,
      bounded_slice(table, locations[2], group.labels[2])?,
    )
  })();
  match parsed {
    Ok(value) => Ok(Some(DocBookmarkSet {
      metadata_location: locations[0],
      starts_location: locations[1],
      ends_location: locations[2],
      value,
    })),
    Err(error) => preserve_compatibility_group(
      table,
      locations,
      group,
      error,
      options,
      diagnostics,
      compatibility_tables,
    ),
  }
}

fn preserve_compatibility_group<T, const N: usize>(
  table: &[u8],
  locations: [FibFcLcb; N],
  group: DocCompatibilityGroup<N>,
  error: Error,
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
  compatibility_tables: &mut Vec<DocCompatibilityTable>,
) -> Result<Option<T>> {
  if options.is_strict() {
    return Err(error);
  }
  let reason = error.to_string();
  report_doc_compatibility(
    diagnostics,
    ParseDiagnosticCode::NonconformingRecord,
    locations
      .iter()
      .find(|location| location.lcb != 0)
      .map_or(0, |location| u64::from(location.fc)),
    group.structure,
    format!("preserved invalid {}: {reason}", group.structure),
  );
  for (location, label) in locations.into_iter().zip(group.labels) {
    if location.lcb != 0 {
      compatibility_tables.push(DocCompatibilityTable {
        label: label.to_owned(),
        location,
        physical_bytes: bounded_slice(table, location, label)
          .ok()
          .map(<[u8]>::to_vec),
        reason: reason.clone(),
      });
    }
  }
  Ok(None)
}

fn parse_range_protection(
  table: &[u8],
  locations: Option<[FibFcLcb; 4]>,
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
  compatibility_tables: &mut Vec<DocCompatibilityTable>,
) -> Result<Option<DocRangeProtectionTables>> {
  let Some(locations) = locations else {
    return Ok(None);
  };
  let labels = [
    "SttbfBkmkProt",
    "PlcfBkfProt",
    "PlcfBklProt",
    "SttbProtUser",
  ];
  let locations = match complete_location_array(locations, "range-protection tables") {
    Ok(Some(value)) => value,
    Ok(None) => return Ok(None),
    Err(error) => {
      return preserve_compatibility_group(
        table,
        locations,
        DocCompatibilityGroup {
          labels,
          structure: "range-protection tables",
        },
        error,
        options,
        diagnostics,
        compatibility_tables,
      );
    }
  };
  let parsed = (|| {
    RangeProtection::from_bytes(
      bounded_slice(table, locations[0], labels[0])?,
      bounded_slice(table, locations[1], labels[1])?,
      bounded_slice(table, locations[2], labels[2])?,
      bounded_slice(table, locations[3], labels[3])?,
    )
  })();
  match parsed {
    Ok(value) => Ok(Some(DocRangeProtectionTables {
      permissions_location: locations[0],
      starts_location: locations[1],
      ends_location: locations[2],
      users_location: locations[3],
      value,
    })),
    Err(error) => preserve_compatibility_group(
      table,
      locations,
      DocCompatibilityGroup {
        labels,
        structure: "range-protection tables",
      },
      error,
      options,
      diagnostics,
      compatibility_tables,
    ),
  }
}

fn parse_user_input_methods(
  table: &[u8],
  locations: Option<[FibFcLcb; 2]>,
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
  compatibility_tables: &mut Vec<DocCompatibilityTable>,
) -> Result<Option<DocUserInputMethodTables>> {
  let Some(locations) = locations else {
    return Ok(None);
  };
  let labels = ["PlcfUim", "PlfGuidUim"];
  let locations = match complete_location_array(locations, "user-input-method tables") {
    Ok(Some(value)) => value,
    Ok(None) => return Ok(None),
    Err(error) => {
      return preserve_compatibility_group(
        table,
        locations,
        DocCompatibilityGroup {
          labels,
          structure: "user-input-method tables",
        },
        error,
        options,
        diagnostics,
        compatibility_tables,
      );
    }
  };
  let parsed = (|| {
    UserInputMethods::from_bytes(
      bounded_slice(table, locations[0], labels[0])?,
      bounded_slice(table, locations[1], labels[1])?,
    )
  })();
  match parsed {
    Ok(value) => Ok(Some(DocUserInputMethodTables {
      methods_location: locations[0],
      service_guids_location: locations[1],
      value,
    })),
    Err(error) => preserve_compatibility_group(
      table,
      locations,
      DocCompatibilityGroup {
        labels,
        structure: "user-input-method tables",
      },
      error,
      options,
      diagnostics,
      compatibility_tables,
    ),
  }
}

fn report_doc_compatibility(
  diagnostics: &mut Vec<ParseDiagnostic>,
  code: ParseDiagnosticCode,
  offset: u64,
  structure: &'static str,
  message: String,
) {
  diagnostics.push(ParseDiagnostic::warning(
    code,
    BinaryFormat::Doc,
    Some("Table Stream"),
    Some(offset),
    structure,
    SpecificationReference {
      document: "MS-DOC",
      section: "2.8",
    },
    message,
  ));
}

fn parse_part_tables<T>(
  table: &[u8],
  locations: Vec<(TextboxDocumentPart, FibFcLcb)>,
  label: &str,
  parse: impl Fn(&[u8]) -> Result<T>,
) -> Result<BTreeMap<TextboxDocumentPart, DocLocated<T>>> {
  locations
    .into_iter()
    .filter(|(_, location)| location.lcb != 0)
    .map(|(part, location)| {
      Ok((
        part,
        DocLocated {
          location,
          value: parse(bounded_slice(table, location, label)?)?,
        },
      ))
    })
    .collect()
}

fn parse_caption_tables(
  table: &[u8],
  locations: Option<(FibFcLcb, FibFcLcb)>,
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<Option<DocCaptionTables>> {
  let (definitions, automatic) = match complete_locations(locations, "caption") {
    Ok(Some(locations)) => locations,
    Ok(None) => return Ok(None),
    Err(error) if options.is_strict() => return Err(error),
    Err(error) => {
      report_doc_compatibility(
        diagnostics,
        ParseDiagnosticCode::InvalidReference,
        0,
        "caption tables",
        format!("preserved incomplete caption tables: {error}"),
      );
      return Ok(None);
    }
  };
  Ok(Some(DocCaptionTables {
    definitions: DocLocated {
      location: definitions,
      value: CaptionDefinitions::from_bytes(bounded_slice(table, definitions, "SttbfCaption")?)?,
    },
    automatic: DocLocated {
      location: automatic,
      value: AutoCaptionDefinitions::from_bytes(bounded_slice(
        table,
        automatic,
        "SttbfAutoCaption",
      )?)?,
    },
  }))
}

fn complete_locations(
  locations: Option<(FibFcLcb, FibFcLcb)>,
  label: &str,
) -> Result<Option<(FibFcLcb, FibFcLcb)>> {
  let Some(locations) = locations else {
    return Ok(None);
  };
  match (locations.0.lcb == 0, locations.1.lcb == 0) {
    (true, true) => Ok(None),
    (false, false) => Ok(Some(locations)),
    _ => Err(Error::invalid(
      0,
      format!("{label} table locations are incomplete"),
    )),
  }
}

fn complete_location_array<const N: usize>(
  locations: [FibFcLcb; N],
  label: &str,
) -> Result<Option<[FibFcLcb; N]>> {
  if locations.iter().all(|location| location.lcb == 0) {
    Ok(None)
  } else if locations.iter().all(|location| location.lcb != 0) {
    Ok(Some(locations))
  } else {
    Err(Error::invalid(0, format!("{label} are incomplete")))
  }
}

fn validate_optional_location<T>(
  expected: Option<FibFcLcb>,
  actual: Option<&DocLocated<T>>,
  label: &str,
) -> Result<()> {
  if expected.filter(|location| location.lcb != 0) != actual.map(|value| value.location) {
    return Err(Error::invalid(0, format!("DOC FIB/{label} link changed")));
  }
  Ok(())
}

fn validate_compatible_location<T>(
  expected: Option<FibFcLcb>,
  actual: Option<&DocLocated<T>>,
  compatibility: &[DocCompatibilityTable],
  physical_label: &str,
  link_label: &str,
) -> Result<()> {
  validate_optional_location(
    managed_expected_location(expected, compatibility, physical_label),
    actual,
    link_label,
  )
}

fn managed_expected_location(
  expected: Option<FibFcLcb>,
  compatibility: &[DocCompatibilityTable],
  label: &str,
) -> Option<FibFcLcb> {
  expected.filter(|location| {
    location.lcb != 0
      && !compatibility
        .iter()
        .any(|value| value.label == label && value.location == *location)
  })
}

fn managed_expected_locations<const N: usize>(
  expected: Option<[FibFcLcb; N]>,
  compatibility: &[DocCompatibilityTable],
  labels: [&str; N],
) -> Option<[FibFcLcb; N]> {
  expected.filter(|locations| {
    locations.iter().all(|location| location.lcb != 0)
      && !locations.iter().zip(labels).any(|(location, label)| {
        compatibility
          .iter()
          .any(|value| value.label == label && value.location == *location)
      })
  })
}

fn validate_compatibility_tables(
  table: &[u8],
  compatibility: &[DocCompatibilityTable],
) -> Result<()> {
  for value in compatibility {
    let physical = bounded_slice(table, value.location, &value.label).ok();
    if physical.map(<[u8]>::to_vec) != value.physical_bytes {
      return Err(Error::invalid(
        u64::from(value.location.fc),
        format!("DOC compatibility table {} link changed", value.label),
      ));
    }
  }
  Ok(())
}

fn validate_note_locations(
  expected: Option<(FibFcLcb, FibFcLcb)>,
  actual: Option<&DocNoteTables>,
  label: &str,
) -> Result<()> {
  let expected = expected.and_then(|(references, text)| {
    (references.lcb != 0 && text.lcb != 0).then_some((references, text))
  });
  let actual = actual.map(|value| (value.references.location, value.text.location));
  if expected != actual {
    return Err(Error::invalid(0, format!("DOC FIB/{label} links changed")));
  }
  Ok(())
}

fn validate_annotation_locations(
  expected: Option<(FibFcLcb, FibFcLcb)>,
  actual: Option<&DocAnnotationTables>,
) -> Result<()> {
  let expected = expected.and_then(|(references, text)| {
    (references.lcb != 0 && text.lcb != 0).then_some((references, text))
  });
  let actual = actual.map(|value| (value.references.location, value.text.location));
  if expected != actual {
    return Err(Error::invalid(0, "DOC FIB/annotation links changed"));
  }
  Ok(())
}

fn validate_caption_locations(
  expected: Option<(FibFcLcb, FibFcLcb)>,
  actual: Option<&DocCaptionTables>,
) -> Result<()> {
  let expected = expected.and_then(|(definitions, automatic)| {
    (definitions.lcb != 0 && automatic.lcb != 0).then_some((definitions, automatic))
  });
  let actual = actual.map(|value| (value.definitions.location, value.automatic.location));
  if expected != actual {
    return Err(Error::invalid(0, "DOC FIB/caption links changed"));
  }
  Ok(())
}

fn validate_part_locations<T>(
  expected: Vec<(TextboxDocumentPart, FibFcLcb)>,
  actual: &BTreeMap<TextboxDocumentPart, DocLocated<T>>,
  label: &str,
) -> Result<()> {
  let expected = expected
    .into_iter()
    .filter(|(_, location)| location.lcb != 0)
    .collect::<BTreeMap<_, _>>();
  let actual = actual
    .iter()
    .map(|(part, value)| (*part, value.location))
    .collect::<BTreeMap<_, _>>();
  if expected != actual {
    return Err(Error::invalid(0, format!("DOC FIB/{label} links changed")));
  }
  Ok(())
}

fn parse_required<T>(
  bytes: &[u8],
  location: Option<FibFcLcb>,
  label: &str,
  parse: impl Fn(&[u8]) -> Result<T>,
) -> Result<DocLocated<T>> {
  let location = location
    .filter(|value| value.lcb != 0)
    .ok_or_else(|| Error::invalid(0, format!("{label} location is missing")))?;
  Ok(DocLocated {
    location,
    value: parse(bounded_slice(bytes, location, label)?)?,
  })
}

fn parse_optional<T>(
  bytes: &[u8],
  location: Option<FibFcLcb>,
  label: &str,
  parse: impl Fn(&[u8]) -> Result<T>,
) -> Result<Option<DocLocated<T>>> {
  location
    .filter(|value| value.lcb != 0)
    .map(|location| {
      Ok(DocLocated {
        location,
        value: parse(bounded_slice(bytes, location, label)?)?,
      })
    })
    .transpose()
}

fn required_stream_data(compound: &CompoundFile, path: &str) -> Result<CfbStreamData> {
  compound
    .entry(path)
    .filter(|entry| entry.is_stream())
    .map(|entry| entry.data.clone())
    .ok_or_else(|| Error::invalid(0, format!("required CFB stream {path} is missing")))
}

fn bounded_slice<'a>(bytes: &'a [u8], location: FibFcLcb, label: &str) -> Result<&'a [u8]> {
  let start = usize::try_from(location.fc)
    .map_err(|_| Error::Limit(format!("{label} offset exceeds usize")))?;
  let len = usize::try_from(location.lcb)
    .map_err(|_| Error::Limit(format!("{label} length exceeds usize")))?;
  bytes
    .get(start..start.saturating_add(len))
    .ok_or_else(|| Error::invalid(u64::from(location.fc), format!("{label} exceeds stream")))
}

#[derive(Debug)]
struct EncodedTextPiece {
  piece_index: usize,
  source_offset: u32,
  source_len: usize,
  source_width: usize,
  source_character_count: usize,
  destination_character_count: usize,
  destination_start: Option<u32>,
  character_replacements: Vec<CpReplacement>,
  compressed: bool,
  bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
struct TextRelocation {
  source_start: u32,
  source_len: usize,
  source_width: usize,
  destination_start: u32,
  destination_width: usize,
  source_character_count: usize,
  destination_character_count: usize,
  character_replacements: Vec<CpReplacement>,
}

#[derive(Clone, Debug)]
struct PhysicalCharacterRun {
  start: u32,
  end: u32,
  properties: Option<Arc<GrpPrl>>,
}

#[derive(Clone, Debug)]
struct PhysicalParagraphRun {
  start: u32,
  end: u32,
  formatting: PapxFkpRun,
}

fn rebuild_paragraph_formatting_pages(
  logical_runs: &[DocPapxRun],
  current_pieces: &[DocTextPiece],
  encoded_pieces: &[EncodedTextPiece],
  word: &mut Vec<u8>,
  fib: &mut Fib,
) -> Result<(PlcBte, Vec<DocFkpPage<PapxFkp>>)> {
  let current_ranges = current_paragraph_ranges(current_pieces)?;
  if logical_runs.len() != current_ranges.len()
    || logical_runs
      .iter()
      .zip(&current_ranges)
      .any(|(run, range)| (run.cp_start, run.cp_end) != *range)
  {
    return Err(Error::invalid(
      0,
      "PAPX CP tree does not match the current paragraph ranges",
    ));
  }
  let physical_runs =
    destination_paragraph_formatting_runs(logical_runs, current_pieces, encoded_pieces)?;
  let pages = paginate_paragraph_formatting_runs(&physical_runs)?;
  append_paragraph_formatting_pages(pages, word, fib)
}

fn source_paragraph_formatting_runs(
  pages: &[DocFkpPage<PapxFkp>],
  clx: &Clx,
  source_word: &[u8],
) -> Result<Vec<DocPapxRun>> {
  let mut fragments = Vec::<(u32, u32, &PapxFkpRun)>::new();
  for page in pages {
    for (range, run) in page.value.file_positions.windows(2).zip(&page.value.runs) {
      if range[0] >= range[1] {
        return Err(Error::invalid(
          u64::from(range[1]),
          "cannot rebuild a PapxFkp with non-increasing rgfc",
        ));
      }
      for (piece_index, descriptor) in clx.piece_table.pieces.iter().enumerate() {
        let cp_start = u32::try_from(clx.piece_table.character_positions[piece_index])
          .map_err(|_| Error::invalid(0, "source text piece CP is negative"))?;
        let cp_end = u32::try_from(clx.piece_table.character_positions[piece_index + 1])
          .map_err(|_| Error::invalid(0, "source text piece CP is negative"))?;
        let character_count = cp_end
          .checked_sub(cp_start)
          .ok_or_else(|| Error::invalid(0, "source text piece CP range is not increasing"))?;
        let width = if descriptor.file_position.compressed {
          1u64
        } else {
          2u64
        };
        let piece_start = u64::from(descriptor.file_position.byte_offset());
        let piece_end = piece_start
          .checked_add(u64::from(character_count) * width)
          .ok_or_else(|| Error::Limit("source text piece FC limit overflow".into()))?;
        let start = u64::from(range[0]).max(piece_start);
        let end = u64::from(range[1]).min(piece_end);
        if start >= end {
          continue;
        }
        let start_delta = start - piece_start;
        let end_delta = end - piece_start;
        if start_delta % width != 0 || end_delta % width != 0 {
          return Err(Error::invalid(
            start,
            "PapxFkp boundary is not aligned to its text-piece encoding",
          ));
        }
        let fragment_start = cp_start
          .checked_add(
            u32::try_from(start_delta / width)
              .map_err(|_| Error::Limit("logical paragraph run start exceeds u32".into()))?,
          )
          .ok_or_else(|| Error::Limit("logical paragraph run start overflow".into()))?;
        let fragment_end = cp_start
          .checked_add(
            u32::try_from(end_delta / width)
              .map_err(|_| Error::Limit("logical paragraph run limit exceeds u32".into()))?,
          )
          .ok_or_else(|| Error::Limit("logical paragraph run limit overflow".into()))?;
        fragments.push((fragment_start, fragment_end, run));
      }
    }
  }
  fragments.sort_by_key(|(cp_start, cp_end, _)| (*cp_start, *cp_end));
  let source_ranges = source_paragraph_ranges(clx, source_word)?;
  source_ranges
    .into_iter()
    .map(|(start, end)| {
      let marker = end.saturating_sub(1);
      let formatting = fragments
        .iter()
        .find(|(cp_start, cp_end, _)| *cp_start <= marker && marker < *cp_end)
        .or_else(|| fragments.iter().find(|(_, cp_end, _)| *cp_end == end))
        .ok_or_else(|| Error::invalid(u64::from(start), "paragraph has no PapxFkp formatting run"))?
        .2;
      Ok(DocPapxRun {
        cp_start: start,
        cp_end: end,
        paragraph_height_info: formatting.paragraph_height_info,
        properties: formatting.properties.clone(),
      })
    })
    .collect()
}

fn source_paragraph_ranges(clx: &Clx, word: &[u8]) -> Result<Vec<(u32, u32)>> {
  let mut ranges = Vec::new();
  let mut paragraph_start = u32::try_from(
    *clx
      .piece_table
      .character_positions
      .first()
      .ok_or_else(|| Error::invalid(0, "PlcPcd has no CP positions"))?,
  )
  .map_err(|_| Error::invalid(0, "first PlcPcd CP is negative"))?;
  let mut document_end = paragraph_start;
  for (index, descriptor) in clx.piece_table.pieces.iter().enumerate() {
    let cp_start = clx.piece_table.character_positions[index];
    let cp_end = clx.piece_table.character_positions[index + 1];
    let piece = descriptor.text_piece(word, cp_start, cp_end)?;
    let cp_start =
      u32::try_from(cp_start).map_err(|_| Error::invalid(0, "source text piece CP is negative"))?;
    for (offset, value) in text_piece_u16_values(&piece.characters).enumerate() {
      let cp = cp_start
        .checked_add(
          u32::try_from(offset).map_err(|_| Error::Limit("paragraph CP exceeds u32".into()))?,
        )
        .ok_or_else(|| Error::Limit("paragraph CP overflow".into()))?;
      if is_paragraph_terminator(value) {
        let end = cp
          .checked_add(1)
          .ok_or_else(|| Error::Limit("paragraph limit overflow".into()))?;
        ranges.push((paragraph_start, end));
        paragraph_start = end;
      }
    }
    document_end =
      u32::try_from(cp_end).map_err(|_| Error::invalid(0, "source text piece CP is negative"))?;
  }
  if paragraph_start < document_end {
    ranges.push((paragraph_start, document_end));
  }
  Ok(ranges)
}

fn current_paragraph_ranges(pieces: &[DocTextPiece]) -> Result<Vec<(u32, u32)>> {
  let mut ranges = Vec::new();
  let mut paragraph_start = pieces
    .first()
    .map(|piece| u32::try_from(piece.value.cp_start))
    .transpose()
    .map_err(|_| Error::invalid(0, "first text piece CP is negative"))?
    .unwrap_or(0);
  let mut document_end = paragraph_start;
  for piece in pieces {
    let cp_start = u32::try_from(piece.value.cp_start)
      .map_err(|_| Error::invalid(0, "text piece CP is negative"))?;
    for (offset, value) in text_piece_u16_values(&piece.value.characters).enumerate() {
      let cp = cp_start
        .checked_add(
          u32::try_from(offset).map_err(|_| Error::Limit("paragraph CP exceeds u32".into()))?,
        )
        .ok_or_else(|| Error::Limit("paragraph CP overflow".into()))?;
      if is_paragraph_terminator(value) {
        let end = cp
          .checked_add(1)
          .ok_or_else(|| Error::Limit("paragraph limit overflow".into()))?;
        ranges.push((paragraph_start, end));
        paragraph_start = end;
      }
    }
    document_end = u32::try_from(piece.value.cp_end)
      .map_err(|_| Error::invalid(0, "text piece CP is negative"))?;
  }
  if paragraph_start < document_end {
    ranges.push((paragraph_start, document_end));
  }
  Ok(ranges)
}

fn text_piece_u16_values(characters: &TextPieceCharacters) -> impl Iterator<Item = u16> + '_ {
  characters.code_units_iter()
}

fn destination_paragraph_formatting_runs(
  logical_runs: &[DocPapxRun],
  pieces: &[DocTextPiece],
  encoded_pieces: &[EncodedTextPiece],
) -> Result<Vec<PhysicalParagraphRun>> {
  logical_runs
    .iter()
    .map(|run| {
      Ok(PhysicalParagraphRun {
        start: destination_fc_for_cp(run.cp_start, pieces, encoded_pieces)?,
        end: destination_fc_for_cp(run.cp_end, pieces, encoded_pieces)?,
        formatting: PapxFkpRun {
          property_offset: None,
          paragraph_height_info: run.paragraph_height_info,
          properties: run.properties.clone(),
        },
      })
    })
    .collect()
}

fn destination_fc_for_cp(
  cp: u32,
  pieces: &[DocTextPiece],
  encoded_pieces: &[EncodedTextPiece],
) -> Result<u32> {
  for piece in pieces {
    let start = u32::try_from(piece.value.cp_start)
      .map_err(|_| Error::invalid(0, "destination text piece CP is negative"))?;
    let end = u32::try_from(piece.value.cp_end)
      .map_err(|_| Error::invalid(0, "destination text piece CP is negative"))?;
    if cp < start || cp > end {
      continue;
    }
    let encoded = encoded_pieces
      .iter()
      .find(|encoded| encoded.piece_index == piece.piece_index)
      .ok_or_else(|| Error::invalid(0, "destination text piece layout is missing"))?;
    let destination_start = encoded
      .destination_start
      .ok_or_else(|| Error::invalid(0, "destination text piece has no FC"))?;
    let width = if encoded.compressed { 1u32 } else { 2u32 };
    return destination_start
      .checked_add(
        (cp - start)
          .checked_mul(width)
          .ok_or_else(|| Error::Limit("destination paragraph FC overflow".into()))?,
      )
      .ok_or_else(|| Error::Limit("destination paragraph FC overflow".into()));
  }
  Err(Error::invalid(
    u64::from(cp),
    "paragraph CP is outside destination text pieces",
  ))
}

fn paginate_paragraph_formatting_runs(runs: &[PhysicalParagraphRun]) -> Result<Vec<PapxFkp>> {
  let mut pages = Vec::new();
  let mut current = Vec::<PhysicalParagraphRun>::new();
  for run in runs {
    let mut candidate = current.clone();
    candidate.push(run.clone());
    let fits = candidate.len() <= 0x1d && build_paragraph_formatting_page(&candidate).is_ok();
    if !fits && !current.is_empty() {
      pages.push(build_paragraph_formatting_page(&current)?);
      current.clear();
    }
    current.push(run.clone());
    build_paragraph_formatting_page(&current)?;
  }
  if !current.is_empty() {
    pages.push(build_paragraph_formatting_page(&current)?);
  }
  Ok(pages)
}

fn build_paragraph_formatting_page(runs: &[PhysicalParagraphRun]) -> Result<PapxFkp> {
  let mut positions = Vec::with_capacity(runs.len() + 1);
  let mut page_runs = Vec::with_capacity(runs.len());
  for (index, run) in runs.iter().enumerate() {
    if index == 0 {
      positions.push(run.start);
    } else if positions.last().copied() != Some(run.start) {
      return Err(Error::invalid(
        u64::from(run.start),
        "paragraph formatting page runs are not adjacent",
      ));
    }
    positions.push(run.end);
    let mut formatting = run.formatting.clone();
    formatting.property_offset = None;
    page_runs.push(formatting);
  }
  PapxFkp::with_canonical_layout(positions, page_runs)
}

fn append_paragraph_formatting_pages(
  pages: Vec<PapxFkp>,
  word: &mut Vec<u8>,
  fib: &mut Fib,
) -> Result<(PlcBte, Vec<DocFkpPage<PapxFkp>>)> {
  if pages.is_empty() {
    return Err(Error::invalid(0, "paragraph formatting has no pages"));
  }
  let meaningful_end = usize::try_from(fib.rg_lw.cb_mac)
    .map_err(|_| Error::Limit("FIB cbMac exceeds usize".into()))?;
  if meaningful_end > word.len() {
    return Err(Error::invalid(
      u64::from(fib.rg_lw.cb_mac),
      "FIB cbMac exceeds WordDocument",
    ));
  }
  let page_start = meaningful_end
    .checked_add(511)
    .map(|value| value & !511)
    .ok_or_else(|| Error::Limit("PapxFkp alignment overflow".into()))?;
  let page_bytes = pages
    .len()
    .checked_mul(512)
    .ok_or_else(|| Error::Limit("PapxFkp page bytes overflow".into()))?;
  let page_end = page_start
    .checked_add(page_bytes)
    .ok_or_else(|| Error::Limit("PapxFkp page end overflow".into()))?;
  word.splice(
    meaningful_end..meaningful_end,
    vec![0; page_end - meaningful_end],
  );
  let first_page_number = page_start / 512;
  let mut located_pages = Vec::with_capacity(pages.len());
  let mut bin_positions = Vec::with_capacity(pages.len() + 1);
  let mut page_numbers = Vec::with_capacity(pages.len());
  for (index, page) in pages.into_iter().enumerate() {
    let offset = page_start
      .checked_add(
        index
          .checked_mul(512)
          .ok_or_else(|| Error::Limit("PapxFkp page offset overflow".into()))?,
      )
      .ok_or_else(|| Error::Limit("PapxFkp page offset overflow".into()))?;
    word[offset..offset + 512].copy_from_slice(&page.to_bytes()?);
    bin_positions.push(page.file_positions[0]);
    let page_ref = FkpPageNumber {
      page_number: u32::try_from(
        first_page_number
          .checked_add(index)
          .ok_or_else(|| Error::Limit("PapxFkp page number overflow".into()))?,
      )
      .map_err(|_| Error::Limit("PapxFkp page number exceeds u32".into()))?,
      unused: 0,
    };
    page_numbers.push(page_ref);
    located_pages.push(DocFkpPage {
      page: page_ref,
      value: page,
    });
  }
  bin_positions.push(
    located_pages
      .last()
      .and_then(|page| page.value.file_positions.last())
      .copied()
      .ok_or_else(|| Error::invalid(0, "paragraph formatting has no physical runs"))?,
  );
  fib.rg_lw.cb_mac =
    u32::try_from(page_end).map_err(|_| Error::Limit("FIB cbMac exceeds u32".into()))?;
  Ok((
    PlcBte {
      file_positions: bin_positions,
      pages: page_numbers,
    },
    located_pages,
  ))
}

fn rebuild_character_formatting_pages(
  logical_runs: &[DocChpxRun],
  current_pieces: &[DocTextPiece],
  encoded_pieces: &[EncodedTextPiece],
  word: &mut Vec<u8>,
  fib: &mut Fib,
) -> Result<(PlcBte, Vec<DocFkpPage<ChpxFkp>>)> {
  for run in logical_runs {
    if run.cp_start >= run.cp_end {
      return Err(Error::invalid(
        u64::from(run.cp_start),
        "logical character formatting run is empty",
      ));
    }
  }
  let physical_runs =
    destination_character_formatting_runs(logical_runs, current_pieces, encoded_pieces)?;
  let pages = paginate_character_formatting_runs(&physical_runs)?;
  let meaningful_end = usize::try_from(fib.rg_lw.cb_mac)
    .map_err(|_| Error::Limit("FIB cbMac exceeds usize".into()))?;
  if meaningful_end > word.len() {
    return Err(Error::invalid(
      u64::from(fib.rg_lw.cb_mac),
      "FIB cbMac exceeds WordDocument",
    ));
  }
  let page_start = meaningful_end
    .checked_add(511)
    .map(|value| value & !511)
    .ok_or_else(|| Error::Limit("ChpxFkp alignment overflow".into()))?;
  let page_bytes = pages
    .len()
    .checked_mul(512)
    .ok_or_else(|| Error::Limit("ChpxFkp page bytes overflow".into()))?;
  let page_end = page_start
    .checked_add(page_bytes)
    .ok_or_else(|| Error::Limit("ChpxFkp page end overflow".into()))?;
  word.splice(
    meaningful_end..meaningful_end,
    vec![0; page_end - meaningful_end],
  );

  let first_page_number = page_start / 512;
  let mut located_pages = Vec::with_capacity(pages.len());
  let mut bin_positions = Vec::with_capacity(pages.len() + 1);
  let mut page_numbers = Vec::with_capacity(pages.len());
  for (index, page) in pages.into_iter().enumerate() {
    let offset = page_start
      .checked_add(
        index
          .checked_mul(512)
          .ok_or_else(|| Error::Limit("ChpxFkp page offset overflow".into()))?,
      )
      .ok_or_else(|| Error::Limit("ChpxFkp page offset overflow".into()))?;
    let bytes = page.to_bytes()?;
    word[offset..offset + 512].copy_from_slice(&bytes);
    bin_positions.push(page.file_positions[0]);
    let page_number = u32::try_from(
      first_page_number
        .checked_add(index)
        .ok_or_else(|| Error::Limit("ChpxFkp page number overflow".into()))?,
    )
    .map_err(|_| Error::Limit("ChpxFkp page number exceeds u32".into()))?;
    let page_ref = FkpPageNumber {
      page_number,
      unused: 0,
    };
    page_numbers.push(page_ref);
    located_pages.push(DocFkpPage {
      page: page_ref,
      value: page,
    });
  }
  let last_position = located_pages
    .last()
    .and_then(|page| page.value.file_positions.last())
    .copied()
    .ok_or_else(|| Error::invalid(0, "character formatting has no physical runs"))?;
  bin_positions.push(last_position);
  fib.rg_lw.cb_mac =
    u32::try_from(page_end).map_err(|_| Error::Limit("FIB cbMac exceeds u32".into()))?;
  Ok((
    PlcBte {
      file_positions: bin_positions,
      pages: page_numbers,
    },
    located_pages,
  ))
}

fn source_character_formatting_runs(
  pages: &[DocFkpPage<ChpxFkp>],
  clx: &Clx,
) -> Result<Vec<DocChpxRun>> {
  let mut runs = Vec::new();
  for page in pages {
    for (range, run) in page.value.file_positions.windows(2).zip(&page.value.runs) {
      if range[0] >= range[1] {
        return Err(Error::invalid(
          u64::from(range[1]),
          "cannot rebuild a ChpxFkp with non-increasing rgfc",
        ));
      }
      for (piece_index, descriptor) in clx.piece_table.pieces.iter().enumerate() {
        let cp_start = u32::try_from(clx.piece_table.character_positions[piece_index])
          .map_err(|_| Error::invalid(0, "source text piece CP is negative"))?;
        let cp_end = u32::try_from(clx.piece_table.character_positions[piece_index + 1])
          .map_err(|_| Error::invalid(0, "source text piece CP is negative"))?;
        let character_count = cp_end
          .checked_sub(cp_start)
          .ok_or_else(|| Error::invalid(0, "source text piece CP range is not increasing"))?;
        let width = if descriptor.file_position.compressed {
          1u64
        } else {
          2u64
        };
        let piece_start = u64::from(descriptor.file_position.byte_offset());
        let piece_end = piece_start
          .checked_add(u64::from(character_count) * width)
          .ok_or_else(|| Error::Limit("source text piece FC limit overflow".into()))?;
        let start = u64::from(range[0]).max(piece_start);
        let end = u64::from(range[1]).min(piece_end);
        if start >= end {
          continue;
        }
        let start_delta = start - piece_start;
        let end_delta = end - piece_start;
        if start_delta % width != 0 || end_delta % width != 0 {
          return Err(Error::invalid(
            start,
            "ChpxFkp boundary is not aligned to its text-piece encoding",
          ));
        }
        runs.push(DocChpxRun {
          cp_start: cp_start
            .checked_add(
              u32::try_from(start_delta / width)
                .map_err(|_| Error::Limit("logical character run start exceeds u32".into()))?,
            )
            .ok_or_else(|| Error::Limit("logical character run start overflow".into()))?,
          cp_end: cp_start
            .checked_add(
              u32::try_from(end_delta / width)
                .map_err(|_| Error::Limit("logical character run limit exceeds u32".into()))?,
            )
            .ok_or_else(|| Error::Limit("logical character run limit overflow".into()))?,
          properties: run.properties.clone(),
        });
      }
    }
  }
  normalize_logical_character_runs(runs)
}

fn normalize_logical_character_runs(mut runs: Vec<DocChpxRun>) -> Result<Vec<DocChpxRun>> {
  runs.sort_by_key(|run| (run.cp_start, run.cp_end));
  let mut normalized: Vec<DocChpxRun> = Vec::with_capacity(runs.len());
  for run in runs {
    if run.cp_start >= run.cp_end {
      return Err(Error::invalid(
        u64::from(run.cp_start),
        "logical character formatting run is empty",
      ));
    }
    if let Some(previous) = normalized.last_mut() {
      if run.cp_start < previous.cp_end {
        return Err(Error::invalid(
          u64::from(run.cp_start),
          "logical character formatting runs overlap",
        ));
      }
      if run.cp_start == previous.cp_end && run.properties == previous.properties {
        previous.cp_end = run.cp_end;
        continue;
      }
    }
    normalized.push(run);
  }
  Ok(normalized)
}

fn apply_character_run_edit(runs: &mut Vec<DocChpxRun>, edit: &CpReplacement) -> Result<()> {
  let replacement_properties = runs
    .iter()
    .find(|run| run.cp_start <= edit.old_start && edit.old_start < run.cp_end)
    .or_else(|| runs.last().filter(|run| run.cp_end == edit.old_start))
    .and_then(|run| run.properties.clone());
  let mut edited = Vec::with_capacity(runs.len() + 1);
  for run in runs.iter() {
    if run.cp_end <= edit.old_start {
      edited.push(run.clone());
      continue;
    }
    if run.cp_start >= edit.old_end {
      edited.push(DocChpxRun {
        cp_start: edit.relocate_u32(run.cp_start, "character formatting run start")?,
        cp_end: edit.relocate_u32(run.cp_end, "character formatting run limit")?,
        properties: run.properties.clone(),
      });
      continue;
    }
    if run.cp_start < edit.old_start {
      edited.push(DocChpxRun {
        cp_start: run.cp_start,
        cp_end: edit.old_start,
        properties: run.properties.clone(),
      });
    }
    if run.cp_end > edit.old_end {
      edited.push(DocChpxRun {
        cp_start: edit.new_end,
        cp_end: edit.relocate_u32(run.cp_end, "character formatting run limit")?,
        properties: run.properties.clone(),
      });
    }
  }
  if edit.new_end > edit.old_start {
    edited.push(DocChpxRun {
      cp_start: edit.old_start,
      cp_end: edit.new_end,
      properties: replacement_properties,
    });
  }
  *runs = normalize_logical_character_runs(edited)?;
  Ok(())
}

fn destination_character_formatting_runs(
  logical_runs: &[DocChpxRun],
  current_pieces: &[DocTextPiece],
  encoded_pieces: &[EncodedTextPiece],
) -> Result<Vec<PhysicalCharacterRun>> {
  let mut runs = Vec::new();
  for logical in logical_runs {
    for piece in current_pieces {
      let piece_start = u32::try_from(piece.value.cp_start)
        .map_err(|_| Error::invalid(0, "destination text piece CP is negative"))?;
      let piece_end = u32::try_from(piece.value.cp_end)
        .map_err(|_| Error::invalid(0, "destination text piece CP is negative"))?;
      let start = logical.cp_start.max(piece_start);
      let end = logical.cp_end.min(piece_end);
      if start >= end {
        continue;
      }
      let encoded = encoded_pieces
        .iter()
        .find(|encoded| encoded.piece_index == piece.piece_index)
        .ok_or_else(|| Error::invalid(0, "destination text piece layout is missing"))?;
      let destination_start = encoded
        .destination_start
        .ok_or_else(|| Error::invalid(0, "destination text piece was not assigned an FC"))?;
      let width = if encoded.compressed { 1u32 } else { 2u32 };
      let physical_start = destination_start
        .checked_add(
          (start - piece_start)
            .checked_mul(width)
            .ok_or_else(|| Error::Limit("destination character run FC overflow".into()))?,
        )
        .ok_or_else(|| Error::Limit("destination character run FC overflow".into()))?;
      let physical_end = destination_start
        .checked_add(
          (end - piece_start)
            .checked_mul(width)
            .ok_or_else(|| Error::Limit("destination character run FC overflow".into()))?,
        )
        .ok_or_else(|| Error::Limit("destination character run FC overflow".into()))?;
      runs.push(PhysicalCharacterRun {
        start: physical_start,
        end: physical_end,
        properties: logical.properties.clone(),
      });
    }
  }
  runs.sort_by_key(|run| (run.start, run.end));
  let mut normalized: Vec<PhysicalCharacterRun> = Vec::with_capacity(runs.len());
  for run in runs {
    if let Some(previous) = normalized.last() {
      let previous_end = previous.end;
      let same_properties = run.properties == previous.properties;
      if run.start < previous_end {
        return Err(Error::invalid(
          u64::from(run.start),
          "destination character formatting runs overlap",
        ));
      }
      if run.start > previous_end {
        normalized.push(PhysicalCharacterRun {
          start: previous_end,
          end: run.start,
          properties: None,
        });
      } else if same_properties {
        normalized.last_mut().expect("previous run exists").end = run.end;
        continue;
      }
    }
    normalized.push(run);
  }
  if normalized.is_empty() {
    return Err(Error::invalid(
      0,
      "character formatting has no destination runs",
    ));
  }
  Ok(normalized)
}

fn paginate_character_formatting_runs(runs: &[PhysicalCharacterRun]) -> Result<Vec<ChpxFkp>> {
  let mut pages = Vec::new();
  let mut current = Vec::<PhysicalCharacterRun>::new();
  for run in runs {
    let mut candidate = current.clone();
    candidate.push(run.clone());
    let fits = candidate.len() <= 0x65 && build_character_formatting_page(&candidate).is_ok();
    if !fits && !current.is_empty() {
      pages.push(build_character_formatting_page(&current)?);
      current.clear();
    }
    current.push(run.clone());
    build_character_formatting_page(&current)?;
  }
  if !current.is_empty() {
    pages.push(build_character_formatting_page(&current)?);
  }
  Ok(pages)
}

fn build_character_formatting_page(runs: &[PhysicalCharacterRun]) -> Result<ChpxFkp> {
  let mut positions = Vec::with_capacity(runs.len() + 1);
  let mut page_runs = Vec::with_capacity(runs.len());
  for (index, run) in runs.iter().enumerate() {
    if index == 0 {
      positions.push(run.start);
    } else if positions.last().copied() != Some(run.start) {
      return Err(Error::invalid(
        u64::from(run.start),
        "character formatting page runs are not adjacent",
      ));
    }
    positions.push(run.end);
    page_runs.push(ChpxFkpRun {
      property_offset: None,
      properties: run.properties.clone(),
    });
  }
  ChpxFkp::with_canonical_layout(positions, page_runs)
}

fn relocate_character_position(
  mut position: u32,
  edits: &[CpReplacement],
  label: &str,
) -> Result<u32> {
  for edit in edits {
    position = edit.relocate_u32(position, label)?;
  }
  Ok(position)
}

fn is_paragraph_terminator(value: u16) -> bool {
  matches!(value, 0x0007 | 0x000c | 0x000d)
}

fn paragraph_terminators(characters: &TextPieceCharacters) -> Vec<u16> {
  characters
    .code_units_iter()
    .filter(|value| is_paragraph_terminator(*value))
    .collect()
}

fn non_paragraph_terminators(terminators: &[u16]) -> Vec<u16> {
  terminators
    .iter()
    .copied()
    .filter(|value| *value != 0x000d)
    .collect()
}

fn text_value_at_cp(pieces: &[DocTextPiece], cp: u32) -> Result<u16> {
  for piece in pieces {
    let start = u32::try_from(piece.value.cp_start)
      .map_err(|_| Error::invalid(0, "text piece begins at a negative CP"))?;
    let end = u32::try_from(piece.value.cp_end)
      .map_err(|_| Error::invalid(0, "text piece ends at a negative CP"))?;
    if cp < start || cp >= end {
      continue;
    }
    let index =
      usize::try_from(cp - start).map_err(|_| Error::Limit("text CP exceeds usize".into()))?;
    return piece
      .value
      .characters
      .code_units_iter()
      .nth(index)
      .ok_or_else(|| Error::invalid(u64::from(cp), "text CP exceeds its piece"));
  }
  Err(Error::invalid(
    u64::from(cp),
    "text CP is outside PlcPcd pieces",
  ))
}

fn validate_nondecreasing_part_positions(
  positions: &[u32],
  part_len: u32,
  label: &str,
) -> Result<()> {
  if positions.is_empty()
    || positions.iter().any(|position| *position >= part_len)
    || positions.windows(2).any(|pair| pair[0] > pair[1])
  {
    return Err(Error::invalid(
      0,
      format!("{label} values are outside the document part or not sorted"),
    ));
  }
  Ok(())
}

fn validate_strict_part_positions(positions: &[u32], part_len: u32, label: &str) -> Result<()> {
  if positions.is_empty()
    || positions.iter().any(|position| *position >= part_len)
    || positions.windows(2).any(|pair| pair[0] >= pair[1])
  {
    return Err(Error::invalid(
      0,
      format!("{label} values are outside the document part or not strictly increasing"),
    ));
  }
  Ok(())
}

fn validate_strict_textbox_positions(positions: &[u32], part_len: u32, label: &str) -> Result<()> {
  if positions.is_empty()
    || positions.iter().enumerate().any(|(index, position)| {
      if index + 1 == positions.len() {
        *position > part_len
      } else {
        *position >= part_len
      }
    })
    || positions.windows(2).any(|pair| pair[0] >= pair[1])
  {
    return Err(Error::invalid(
      0,
      format!("{label} values are outside the document part or not strictly increasing"),
    ));
  }
  Ok(())
}

fn require_part_character(
  pieces: &[DocTextPiece],
  part_start: u32,
  local_cp: u32,
  expected: u16,
  label: &str,
) -> Result<()> {
  let global_cp = part_start
    .checked_add(local_cp)
    .ok_or_else(|| Error::Limit(format!("{label} CP overflow")))?;
  if text_value_at_cp(pieces, global_cp)? != expected {
    return Err(Error::invalid(
      u64::from(global_cp),
      format!("{label} is not U+{expected:04X}"),
    ));
  }
  Ok(())
}

fn paragraph_terminators_in_range(
  characters: &TextPieceCharacters,
  start: usize,
  end: usize,
) -> Result<Vec<u16>> {
  if start > end || end > characters.character_count() {
    return Err(Error::invalid(
      start as u64,
      "text replacement range exceeds piece",
    ));
  }
  Ok(
    characters
      .code_units_iter()
      .skip(start)
      .take(end - start)
      .filter(|value| is_paragraph_terminator(*value))
      .collect(),
  )
}

fn paragraph_terminators_in_piece_range(
  pieces: &[DocTextPiece],
  start: u32,
  end: u32,
) -> Result<Vec<u16>> {
  let mut terminators = Vec::new();
  for piece in pieces {
    let piece_start = u32::try_from(piece.value.cp_start)
      .map_err(|_| Error::invalid(0, "text piece begins at a negative CP"))?;
    let piece_end = u32::try_from(piece.value.cp_end)
      .map_err(|_| Error::invalid(0, "text piece ends at a negative CP"))?;
    let overlap_start = start.max(piece_start);
    let overlap_end = end.min(piece_end);
    if overlap_start >= overlap_end {
      continue;
    }
    terminators.extend(paragraph_terminators_in_range(
      &piece.value.characters,
      usize::try_from(overlap_start - piece_start)
        .map_err(|_| Error::Limit("text replacement start exceeds usize".into()))?,
      usize::try_from(overlap_end - piece_start)
        .map_err(|_| Error::Limit("text replacement limit exceeds usize".into()))?,
    )?);
  }
  Ok(terminators)
}

fn text_piece_character_replacement(
  source: &TextPieceCharacters,
  destination: &TextPieceCharacters,
) -> Result<CpReplacement> {
  let source_len = source.character_count();
  let destination_len = destination.character_count();
  if source_len == destination_len {
    let end = u32::try_from(source_len)
      .map_err(|_| Error::Limit("text piece character count exceeds u32".into()))?;
    return CpReplacement::new(end, end, 0);
  }
  if source.encoding() != destination.encoding()
    || source.compatibility_code_units().is_some()
    || destination.compatibility_code_units().is_some()
  {
    return Err(Error::invalid(
      0,
      "text piece encoding and character count changed together",
    ));
  }
  let source_units = source.code_units();
  let destination_units = destination.code_units();
  let (prefix, suffix) = common_prefix_and_suffix(source_units, destination_units);
  let old_start =
    u32::try_from(prefix).map_err(|_| Error::Limit("text edit start exceeds u32".into()))?;
  let old_end = u32::try_from(source_len - suffix)
    .map_err(|_| Error::Limit("text edit limit exceeds u32".into()))?;
  let replacement_len = u32::try_from(destination_len - prefix - suffix)
    .map_err(|_| Error::Limit("replacement character count exceeds u32".into()))?;
  CpReplacement::new(old_start, old_end, replacement_len)
}

fn common_prefix_and_suffix<T: PartialEq>(source: &[T], destination: &[T]) -> (usize, usize) {
  let prefix = source
    .iter()
    .zip(destination)
    .take_while(|(left, right)| left == right)
    .count();
  let suffix_limit = source
    .len()
    .saturating_sub(prefix)
    .min(destination.len().saturating_sub(prefix));
  let suffix = source
    .iter()
    .rev()
    .zip(destination.iter().rev())
    .take(suffix_limit)
    .take_while(|(left, right)| left == right)
    .count();
  (prefix, suffix)
}

fn relocate_text_file_positions(
  positions: &mut [u32],
  relocations: &[TextRelocation],
) -> Result<()> {
  for position in positions {
    for relocation in relocations {
      let source_end = u64::from(relocation.source_start)
        .checked_add(relocation.source_len as u64)
        .ok_or_else(|| Error::Limit("text relocation source range overflow".into()))?;
      let raw_position = u64::from(*position);
      if raw_position < u64::from(relocation.source_start) || raw_position > source_end {
        continue;
      }
      let delta = usize::try_from(raw_position - u64::from(relocation.source_start))
        .map_err(|_| Error::Limit("text relocation delta exceeds usize".into()))?;
      if !delta.is_multiple_of(relocation.source_width) {
        return Err(Error::invalid(
          raw_position,
          "formatting FC is not aligned to its text-piece encoding",
        ));
      }
      let character_offset = delta / relocation.source_width;
      if character_offset > relocation.source_character_count {
        return Err(Error::invalid(
          raw_position,
          "formatting FC exceeds its source text piece",
        ));
      }
      let relocated_character_offset = usize::try_from(relocate_character_position(
        u32::try_from(character_offset)
          .map_err(|_| Error::Limit("formatting character offset exceeds u32".into()))?,
        &relocation.character_replacements,
        "formatting boundary",
      )?)
      .map_err(|_| Error::Limit("relocated character offset exceeds usize".into()))?;
      if relocated_character_offset > relocation.destination_character_count {
        return Err(Error::invalid(
          raw_position,
          "relocated formatting FC exceeds its destination text piece",
        ));
      }
      let relocated_delta = relocated_character_offset
        .checked_mul(relocation.destination_width)
        .ok_or_else(|| Error::Limit("relocated formatting FC overflow".into()))?;
      *position = relocation
        .destination_start
        .checked_add(
          u32::try_from(relocated_delta)
            .map_err(|_| Error::Limit("relocated formatting FC exceeds u32".into()))?,
        )
        .ok_or_else(|| Error::Limit("relocated formatting FC overflow".into()))?;
      break;
    }
  }
  Ok(())
}

trait PatchSink {
  fn replace(
    &mut self,
    offset: usize,
    expected: usize,
    encoded: Vec<u8>,
    label: &str,
  ) -> Result<()>;
}

impl PatchSink for Vec<u8> {
  fn replace(
    &mut self,
    offset: usize,
    expected: usize,
    encoded: Vec<u8>,
    label: &str,
  ) -> Result<()> {
    if encoded.len() != expected {
      return Err(Error::invalid(
        offset as u64,
        format!("{label} size changed; WordDocument relocation is not implemented"),
      ));
    }
    self
      .get_mut(offset..offset.saturating_add(expected))
      .ok_or_else(|| Error::invalid(offset as u64, format!("{label} exceeds stream")))?
      .copy_from_slice(&encoded);
    Ok(())
  }
}

#[derive(Debug)]
struct PendingTableReplacement {
  offset: usize,
  expected: usize,
  encoded: Vec<u8>,
  label: String,
}

#[derive(Debug)]
struct TableLayout<'a> {
  original: &'a [u8],
  replacements: Vec<PendingTableReplacement>,
}

#[derive(Debug)]
struct TableWritePlan<'a> {
  original: &'a [u8],
  replacements: Vec<PendingTableReplacement>,
  output_len: usize,
}

enum MutableWordLayout<'a> {
  Owned(Vec<u8>),
  Overlay(TableLayout<'a>),
}

enum DocStreamWritePlan<'a> {
  Owned(Vec<u8>),
  Overlay(TableWritePlan<'a>),
}

struct DocCompoundWritePlan<'a> {
  compound: CompoundFile,
  word: DocStreamWritePlan<'a>,
  table_path: &'static str,
  table: DocStreamWritePlan<'a>,
  data: Option<DocStreamWritePlan<'a>>,
}

impl CfbStreamWriter for DocStreamWritePlan<'_> {
  fn write_to(&self, writer: &mut dyn Write) -> Result<()> {
    DocStreamWritePlan::write_to(self, writer)
  }
}

impl DocCompoundWritePlan<'_> {
  fn stream_overrides(&self) -> Vec<CfbStreamOverride<'_>> {
    let mut overrides = Vec::with_capacity(3);
    overrides.push(CfbStreamOverride::new(
      Path::new(WORD_DOCUMENT_STREAM_PATH),
      self.word.output_len(),
      &self.word,
    ));
    overrides.push(CfbStreamOverride::new(
      Path::new(self.table_path),
      self.table.output_len(),
      &self.table,
    ));
    if let Some(data) = &self.data {
      overrides.push(CfbStreamOverride::new(
        Path::new(DATA_STREAM_PATH),
        data.output_len(),
        data,
      ));
    }
    overrides
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    self
      .compound
      .to_bytes_with_stream_overrides(&self.stream_overrides())
  }

  fn write_to(&self, writer: impl Write) -> Result<()> {
    self
      .compound
      .write_to_with_stream_overrides(&self.stream_overrides(), writer)
  }

  fn into_compound(self) -> Result<CompoundFile> {
    let Self {
      mut compound,
      word,
      table_path,
      table,
      data,
    } = self;
    compound.overwrite_stream(WORD_DOCUMENT_STREAM_PATH, word.into_bytes()?)?;
    compound.overwrite_stream(table_path, table.into_bytes()?)?;
    if let Some(data) = data {
      compound.upsert_stream(DATA_STREAM_PATH, data.into_bytes()?)?;
    }
    Ok(compound)
  }
}

#[derive(Clone, Debug)]
struct AppliedTableReplacement {
  old_offset: usize,
  old_len: usize,
  new_offset: usize,
  new_len: usize,
}

#[derive(Clone, Debug)]
struct TableRelocation {
  original_len: usize,
  changed_layout: bool,
  replacements: Vec<AppliedTableReplacement>,
}

impl<'a> TableLayout<'a> {
  fn new(original: &'a [u8]) -> Self {
    Self {
      original,
      replacements: Vec::new(),
    }
  }

  #[cfg(test)]
  fn finish(self) -> Result<(Vec<u8>, TableRelocation)> {
    let (plan, relocation) = self.finish_plan()?;
    Ok((plan.to_bytes()?, relocation))
  }

  fn finish_plan(mut self) -> Result<(TableWritePlan<'a>, TableRelocation)> {
    self.replacements.sort_by_key(|value| value.offset);
    for pair in self.replacements.windows(2) {
      let previous_end = pair[0]
        .offset
        .checked_add(pair[0].expected)
        .ok_or_else(|| Error::Limit("Table Stream replacement end overflow".into()))?;
      if previous_end > pair[1].offset {
        return Err(Error::invalid(
          pair[1].offset as u64,
          format!(
            "Table Stream replacements {} and {} overlap",
            pair[0].label, pair[1].label
          ),
        ));
      }
    }

    let original_len = self.original.len();
    let output_len = self
      .replacements
      .iter()
      .try_fold(original_len, |length, replacement| {
        length
          .checked_sub(replacement.expected)
          .and_then(|length| length.checked_add(replacement.encoded.len()))
          .ok_or_else(|| Error::Limit("Table Stream output length overflow".into()))
      })?;
    let mut cursor = 0usize;
    let mut emitted = 0usize;
    let mut applied = Vec::with_capacity(self.replacements.len());
    for replacement in &self.replacements {
      let old_end = replacement
        .offset
        .checked_add(replacement.expected)
        .ok_or_else(|| Error::Limit("Table Stream replacement end overflow".into()))?;
      let unchanged = self
        .original
        .get(cursor..replacement.offset)
        .ok_or_else(|| {
          Error::invalid(
            replacement.offset as u64,
            format!("{} exceeds Table Stream", replacement.label),
          )
        })?;
      if old_end > original_len {
        return Err(Error::invalid(
          replacement.offset as u64,
          format!("{} exceeds Table Stream", replacement.label),
        ));
      }
      emitted = emitted
        .checked_add(unchanged.len())
        .ok_or_else(|| Error::Limit("Table Stream output length overflow".into()))?;
      let new_offset = emitted;
      let new_len = replacement.encoded.len();
      emitted = emitted
        .checked_add(new_len)
        .ok_or_else(|| Error::Limit("Table Stream output length overflow".into()))?;
      applied.push(AppliedTableReplacement {
        old_offset: replacement.offset,
        old_len: replacement.expected,
        new_offset,
        new_len,
      });
      cursor = old_end;
    }
    emitted = emitted
      .checked_add(self.original.len() - cursor)
      .ok_or_else(|| Error::Limit("Table Stream output length overflow".into()))?;
    debug_assert_eq!(emitted, output_len);
    Ok((
      TableWritePlan {
        original: self.original,
        replacements: self.replacements,
        output_len,
      },
      TableRelocation {
        original_len,
        changed_layout: applied.iter().any(|value| value.old_len != value.new_len),
        replacements: applied,
      },
    ))
  }
}

impl TableWritePlan<'_> {
  fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(self.output_len);
    self.write_to(&mut bytes)?;
    debug_assert_eq!(bytes.len(), self.output_len);
    Ok(bytes)
  }

  fn write_to<W: Write + ?Sized>(&self, writer: &mut W) -> Result<()> {
    let mut cursor = 0usize;
    for replacement in &self.replacements {
      writer.write_all(&self.original[cursor..replacement.offset])?;
      writer.write_all(&replacement.encoded)?;
      cursor = replacement.offset + replacement.expected;
    }
    writer.write_all(&self.original[cursor..])?;
    Ok(())
  }
}

impl<'a> MutableWordLayout<'a> {
  fn len(&self) -> usize {
    match self {
      Self::Owned(bytes) => bytes.len(),
      Self::Overlay(layout) => layout.original.len(),
    }
  }

  fn owned_mut(&mut self) -> Result<&mut Vec<u8>> {
    match self {
      Self::Owned(bytes) => Ok(bytes),
      Self::Overlay(_) => Err(Error::invalid(
        0,
        "WordDocument variable layout was not selected before mutation",
      )),
    }
  }

  fn finish(self) -> Result<DocStreamWritePlan<'a>> {
    match self {
      Self::Owned(bytes) => Ok(DocStreamWritePlan::Owned(bytes)),
      Self::Overlay(layout) => {
        let (plan, relocation) = layout.finish_plan()?;
        if relocation.changed_layout {
          return Err(Error::invalid(
            0,
            "WordDocument overlay changed physical layout without relocation",
          ));
        }
        Ok(DocStreamWritePlan::Overlay(plan))
      }
    }
  }
}

impl PatchSink for MutableWordLayout<'_> {
  fn replace(
    &mut self,
    offset: usize,
    expected: usize,
    encoded: Vec<u8>,
    label: &str,
  ) -> Result<()> {
    match self {
      Self::Owned(bytes) => bytes.replace(offset, expected, encoded, label),
      Self::Overlay(layout) => layout.replace(offset, expected, encoded, label),
    }
  }
}

impl DocStreamWritePlan<'_> {
  fn output_len(&self) -> usize {
    match self {
      Self::Owned(bytes) => bytes.len(),
      Self::Overlay(plan) => plan.output_len,
    }
  }

  fn write_to<W: Write + ?Sized>(&self, writer: &mut W) -> Result<()> {
    match self {
      Self::Owned(bytes) => writer.write_all(bytes)?,
      Self::Overlay(plan) => plan.write_to(writer)?,
    }
    Ok(())
  }

  fn into_bytes(self) -> Result<Vec<u8>> {
    match self {
      Self::Owned(bytes) => Ok(bytes),
      Self::Overlay(plan) => plan.to_bytes(),
    }
  }
}

impl PatchSink for TableLayout<'_> {
  fn replace(
    &mut self,
    offset: usize,
    expected: usize,
    encoded: Vec<u8>,
    label: &str,
  ) -> Result<()> {
    if expected == 0 && encoded.is_empty() {
      return Ok(());
    }
    let end = offset
      .checked_add(expected)
      .ok_or_else(|| Error::Limit(format!("{label} replacement end overflow")))?;
    if end > self.original.len() {
      return Err(Error::invalid(
        offset as u64,
        format!("{label} exceeds Table Stream"),
      ));
    }
    if encoded.len() == expected && self.original.get(offset..end) == Some(encoded.as_slice()) {
      return Ok(());
    }
    self.replacements.push(PendingTableReplacement {
      offset,
      expected,
      encoded,
      label: label.to_owned(),
    });
    Ok(())
  }
}

impl TableRelocation {
  fn relocate(&self, location: FibFcLcb) -> Result<Option<FibFcLcb>> {
    let old_offset = usize::try_from(location.fc)
      .map_err(|_| Error::Limit("Table Stream location exceeds usize".into()))?;
    let old_len = usize::try_from(location.lcb)
      .map_err(|_| Error::Limit("Table Stream length exceeds usize".into()))?;
    let old_end = old_offset
      .checked_add(old_len)
      .ok_or_else(|| Error::Limit("Table Stream location end overflow".into()))?;
    if old_end > self.original_len {
      return Ok(None);
    }
    if !self.changed_layout {
      return Ok(Some(location));
    }
    if let Some(replacement) = self
      .replacements
      .iter()
      .find(|value| value.old_offset == old_offset && value.old_len == old_len)
    {
      return Ok(Some(FibFcLcb {
        fc: u32::try_from(replacement.new_offset)
          .map_err(|_| Error::Limit("relocated Table offset exceeds u32".into()))?,
        lcb: u32::try_from(replacement.new_len)
          .map_err(|_| Error::Limit("relocated Table length exceeds u32".into()))?,
      }));
    }
    for replacement in &self.replacements {
      let replacement_end = replacement.old_offset + replacement.old_len;
      if old_offset < replacement_end && replacement.old_offset < old_end {
        return Err(Error::invalid(
          old_offset as u64,
          "an FIB Table range partially overlaps a relocated structure",
        ));
      }
    }
    let delta = self
      .replacements
      .iter()
      .take_while(|value| value.old_offset + value.old_len <= old_offset)
      .fold(0i64, |delta, value| {
        delta + value.new_len as i64 - value.old_len as i64
      });
    let new_offset = i64::try_from(old_offset)
      .ok()
      .and_then(|offset| offset.checked_add(delta))
      .and_then(|offset| u32::try_from(offset).ok())
      .ok_or_else(|| Error::Limit("relocated Table offset exceeds u32".into()))?;
    Ok(Some(FibFcLcb {
      fc: new_offset,
      lcb: location.lcb,
    }))
  }
}

fn plan_table_relocation(
  original_len: usize,
  mut replacements: Vec<(usize, usize, usize)>,
  label: &str,
) -> Result<TableRelocation> {
  replacements.sort_by_key(|(offset, _, _)| *offset);
  let mut old_cursor = 0usize;
  let mut new_cursor = 0usize;
  let mut applied = Vec::with_capacity(replacements.len());
  for (old_offset, old_len, new_len) in replacements {
    if old_offset < old_cursor {
      return Err(Error::invalid(
        old_offset as u64,
        format!("{label} replacements overlap"),
      ));
    }
    let old_end = old_offset
      .checked_add(old_len)
      .ok_or_else(|| Error::Limit(format!("{label} replacement end overflow")))?;
    if old_end > original_len {
      return Err(Error::invalid(
        old_offset as u64,
        format!("{label} exceeds its stream"),
      ));
    }
    let unchanged_len = old_offset - old_cursor;
    let new_offset = new_cursor
      .checked_add(unchanged_len)
      .ok_or_else(|| Error::Limit(format!("{label} output offset overflow")))?;
    new_cursor = new_offset
      .checked_add(new_len)
      .ok_or_else(|| Error::Limit(format!("{label} output end overflow")))?;
    applied.push(AppliedTableReplacement {
      old_offset,
      old_len,
      new_offset,
      new_len,
    });
    old_cursor = old_end;
  }
  Ok(TableRelocation {
    original_len,
    changed_layout: applied.iter().any(|value| value.old_len != value.new_len),
    replacements: applied,
  })
}

fn patch_located<T, S: PatchSink + ?Sized>(
  target: &mut S,
  located: &DocLocated<T>,
  encode: impl Fn(&T) -> Result<Vec<u8>>,
  label: &str,
) -> Result<()> {
  patch_location(target, located.location, encode(&located.value)?, label)
}

fn patch_optional_located<T, S: PatchSink + ?Sized>(
  target: &mut S,
  located: Option<&DocLocated<T>>,
  encode: impl Fn(&T) -> Result<Vec<u8>>,
  label: &str,
) -> Result<()> {
  if let Some(located) = located {
    patch_located(target, located, encode, label)?;
  }
  Ok(())
}

fn patch_part_tables<T, S: PatchSink + ?Sized>(
  target: &mut S,
  tables: &BTreeMap<TextboxDocumentPart, DocLocated<T>>,
  encode: impl Fn(&T) -> Result<Vec<u8>>,
  label: &str,
) -> Result<()> {
  for table in tables.values() {
    patch_located(target, table, &encode, label)?;
  }
  Ok(())
}

fn patch_location<S: PatchSink + ?Sized>(
  target: &mut S,
  location: FibFcLcb,
  encoded: Vec<u8>,
  label: &str,
) -> Result<()> {
  patch_at(
    target,
    usize::try_from(location.fc)
      .map_err(|_| Error::Limit(format!("{label} offset exceeds usize")))?,
    usize::try_from(location.lcb)
      .map_err(|_| Error::Limit(format!("{label} length exceeds usize")))?,
    encoded,
    label,
  )
}

fn patch_prefix<S: PatchSink + ?Sized>(
  target: &mut S,
  expected: usize,
  encoded: Vec<u8>,
  label: &str,
) -> Result<()> {
  patch_at(target, 0, expected, encoded, label)
}

fn patch_at<S: PatchSink + ?Sized>(
  target: &mut S,
  offset: usize,
  expected: usize,
  encoded: Vec<u8>,
  label: &str,
) -> Result<()> {
  target.replace(offset, expected, encoded, label)
}

fn ensure_stream_limit(label: &str, bytes: &[u8], limits: Limits) -> Result<()> {
  if bytes.len() as u64 > limits.max_stream_size {
    return Err(Error::Limit(format!(
      "{label} stream length {} exceeds {}",
      bytes.len(),
      limits.max_stream_size
    )));
  }
  Ok(())
}

fn ensure_entry_limit(label: &str, count: usize, limits: Limits) -> Result<()> {
  if count > limits.max_entries {
    return Err(Error::Limit(format!(
      "{label} count {count} exceeds {}",
      limits.max_entries
    )));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn table_layout_rebuilds_ranges_and_relocates_fib_coordinates() {
    let mut layout = TableLayout::new(b"abcdefghij");
    layout.replace(2, 2, b"XYZ".to_vec(), "grow").unwrap();
    layout.replace(7, 1, Vec::new(), "remove").unwrap();
    let (plan, relocation) = layout.finish_plan().unwrap();
    let bytes = plan.to_bytes().unwrap();
    let mut streamed = Vec::new();
    plan.write_to(&mut streamed).unwrap();
    let planned =
      plan_table_relocation(10, vec![(2, 2, 3), (7, 1, 0)], "test replacement").unwrap();
    assert_eq!(bytes, b"abXYZefgij");
    assert_eq!(streamed, bytes);
    assert_eq!(
      relocation.relocate(FibFcLcb { fc: 2, lcb: 2 }).unwrap(),
      Some(FibFcLcb { fc: 2, lcb: 3 })
    );
    assert_eq!(
      relocation.relocate(FibFcLcb { fc: 4, lcb: 2 }).unwrap(),
      Some(FibFcLcb { fc: 5, lcb: 2 })
    );
    assert_eq!(
      relocation.relocate(FibFcLcb { fc: 7, lcb: 1 }).unwrap(),
      Some(FibFcLcb { fc: 8, lcb: 0 })
    );
    assert!(relocation.relocate(FibFcLcb { fc: 1, lcb: 2 }).is_err());
    for location in [
      FibFcLcb { fc: 2, lcb: 2 },
      FibFcLcb { fc: 4, lcb: 2 },
      FibFcLcb { fc: 7, lcb: 1 },
      FibFcLcb { fc: 1, lcb: 2 },
    ] {
      assert_eq!(
        planned
          .relocate(location)
          .map_err(|error| error.to_string()),
        relocation
          .relocate(location)
          .map_err(|error| error.to_string())
      );
    }
  }

  #[test]
  fn same_size_table_layout_preserves_compatibility_overlaps() {
    let mut layout = TableLayout::new(b"abcdef");
    layout.replace(2, 2, b"XY".to_vec(), "same size").unwrap();
    let (bytes, relocation) = layout.finish().unwrap();
    assert_eq!(bytes, b"abXYef");
    let overlapping = FibFcLcb { fc: 1, lcb: 4 };
    assert_eq!(relocation.relocate(overlapping).unwrap(), Some(overlapping));
  }

  #[test]
  fn table_layout_drops_byte_identical_owned_replacements() {
    let mut layout = TableLayout::new(b"abcdef");
    layout.replace(2, 2, b"cd".to_vec(), "unchanged").unwrap();
    assert!(layout.replacements.is_empty());
    let (bytes, relocation) = layout.finish().unwrap();
    assert_eq!(bytes, b"abcdef");
    assert!(!relocation.changed_layout);
    assert!(relocation.replacements.is_empty());
  }

  #[test]
  fn cp_replacement_relocates_boundaries_and_rejects_interior_references() {
    let replacement = CpReplacement::new(3, 6, 1).unwrap();
    assert_eq!(replacement.relocate_u32(2, "test").unwrap(), 2);
    assert_eq!(replacement.relocate_u32(3, "test").unwrap(), 3);
    assert!(replacement.relocate_u32(4, "test").is_err());
    assert_eq!(replacement.relocate_u32(6, "test").unwrap(), 4);
    assert_eq!(replacement.relocate_u32(10, "test").unwrap(), 8);

    let insertion = CpReplacement::new(3, 3, 2).unwrap();
    assert_eq!(insertion.relocate_u32(2, "test").unwrap(), 2);
    assert_eq!(insertion.relocate_u32(3, "test").unwrap(), 5);
  }

  #[test]
  fn malformed_fkp_order_is_strictly_rejected_and_compatibly_diagnosed() {
    let pages = vec![DocFkpPage {
      page: FkpPageNumber {
        page_number: 2,
        unused: 0,
      },
      value: vec![100, 100],
    }];
    let mut diagnostics = Vec::new();
    assert!(
      validate_fkp_page_order(
        &pages,
        Vec::as_slice,
        "test FKP rgfc",
        ParseOptions::default(),
        &mut diagnostics,
      )
      .is_err()
    );
    assert!(diagnostics.is_empty());

    validate_fkp_page_order(
      &pages,
      Vec::as_slice,
      "test FKP rgfc",
      ParseOptions::compatible(Limits::default()),
      &mut diagnostics,
    )
    .unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
      diagnostics[0].code,
      ParseDiagnosticCode::NonconformingRecord
    );
  }

  #[test]
  fn text_relocation_maps_source_fc_through_character_edit() {
    let relocation = TextRelocation {
      source_start: 100,
      source_len: 10,
      source_width: 1,
      destination_start: 200,
      destination_width: 2,
      source_character_count: 10,
      destination_character_count: 11,
      character_replacements: vec![CpReplacement::new(3, 4, 2).unwrap()],
    };
    let mut positions = [100, 103, 104, 110];
    relocate_text_file_positions(&mut positions, &[relocation]).unwrap();
    assert_eq!(positions, [200, 206, 210, 222]);
  }

  #[test]
  fn text_relocation_composes_multiple_character_edits() {
    let relocation = TextRelocation {
      source_start: 100,
      source_len: 10,
      source_width: 1,
      destination_start: 200,
      destination_width: 1,
      source_character_count: 10,
      destination_character_count: 11,
      character_replacements: vec![
        CpReplacement::new(2, 3, 3).unwrap(),
        CpReplacement::new(8, 10, 1).unwrap(),
      ],
    };
    let mut positions = [100, 102, 103, 106, 108, 110];
    relocate_text_file_positions(&mut positions, &[relocation]).unwrap();
    assert_eq!(positions, [200, 202, 205, 208, 209, 211]);
  }

  #[test]
  fn character_run_edits_inherit_the_start_run_and_remove_inner_boundaries() {
    let property = |value| {
      Some(Arc::new(GrpPrl {
        properties: vec![super::super::Prl {
          sprm: super::super::Sprm::from_opcode(0x0835),
          operand: SprmOperand::Toggle(value),
        }],
      }))
    };
    let mut runs = vec![
      DocChpxRun {
        cp_start: 0,
        cp_end: 3,
        properties: property(1),
      },
      DocChpxRun {
        cp_start: 3,
        cp_end: 6,
        properties: property(0),
      },
      DocChpxRun {
        cp_start: 6,
        cp_end: 10,
        properties: None,
      },
    ];
    apply_character_run_edit(&mut runs, &CpReplacement::new(2, 8, 2).unwrap()).unwrap();
    assert_eq!(
      runs
        .iter()
        .map(|run| (run.cp_start, run.cp_end, run.properties.clone()))
        .collect::<Vec<_>>(),
      vec![(0, 4, property(1)), (4, 6, None)]
    );

    let mut insertion_runs = vec![
      DocChpxRun {
        cp_start: 0,
        cp_end: 3,
        properties: property(1),
      },
      DocChpxRun {
        cp_start: 3,
        cp_end: 6,
        properties: property(0),
      },
    ];
    apply_character_run_edit(&mut insertion_runs, &CpReplacement::new(3, 3, 2).unwrap()).unwrap();
    assert_eq!(
      insertion_runs
        .iter()
        .map(|run| (run.cp_start, run.cp_end, run.properties.clone()))
        .collect::<Vec<_>>(),
      vec![(0, 3, property(1)), (3, 8, property(0))]
    );
  }

  #[test]
  fn paragraph_terminator_inventory_distinguishes_text_from_structure() {
    let compressed = TextPieceCharacters::compressed("A\r\u{7}B").unwrap();
    assert_eq!(paragraph_terminators(&compressed), vec![0x000d, 0x0007]);
    assert_eq!(
      non_paragraph_terminators(&paragraph_terminators(&compressed)),
      vec![0x0007]
    );
    assert_eq!(
      paragraph_terminators_in_range(&compressed, 1, 3).unwrap(),
      vec![0x000d, 0x0007]
    );
    let utf16 = TextPieceCharacters::utf16("中\u{c}文");
    assert_eq!(paragraph_terminators(&utf16), vec![0x000c]);
    assert!(paragraph_terminators_in_range(&utf16, 0, 4).is_err());
  }

  #[test]
  fn direct_paragraph_properties_follow_prcdata_and_stop_the_source_array() {
    let byte = |sprm: KnownSprm, value: u8| super::super::Prl {
      sprm: super::super::Sprm::from_opcode(sprm.opcode()),
      operand: SprmOperand::Byte(value),
    };
    let dword = |sprm: KnownSprm, value: u32| super::super::Prl {
      sprm: super::super::Sprm::from_opcode(sprm.opcode()),
      operand: SprmOperand::Dword(value.to_le_bytes()),
    };
    let inner = GrpPrl {
      properties: vec![
        dword(KnownSprm::PItap, 1),
        byte(KnownSprm::PFKeep, 1),
        byte(KnownSprm::PFKeepFollow, 1),
      ],
    };
    let outer = GrpPrl {
      properties: vec![
        byte(KnownSprm::PFInTable, 1),
        dword(KnownSprm::PTableProps, 20),
        byte(KnownSprm::PFPageBreakBefore, 1),
      ],
    };
    let data = DocDataStream {
      physical_bytes: Vec::new().into(),
      nodes: vec![
        DocDataNode {
          offset: 4,
          physical_len: outer.to_bytes().unwrap().len() + 2,
          value: DocDataNodeValue::ParagraphProperties(PrcData { properties: outer }),
        },
        DocDataNode {
          offset: 20,
          physical_len: inner.to_bytes().unwrap().len() + 2,
          value: DocDataNodeValue::ParagraphProperties(PrcData { properties: inner }),
        },
      ],
    };
    let root = GrpPrl {
      properties: vec![dword(KnownSprm::PHugePapx, 4)],
    };

    let applied = expand_direct_paragraph_properties(&root, Some(&data), Some(0)).unwrap();
    assert_eq!(
      applied
        .properties
        .iter()
        .map(|property| property.sprm.kind())
        .collect::<Vec<_>>(),
      vec![
        SprmKind::Known(KnownSprm::PFInTable),
        SprmKind::Known(KnownSprm::PItap),
        SprmKind::Known(KnownSprm::PFKeep),
        SprmKind::Known(KnownSprm::PFKeepFollow),
      ]
    );
    assert_eq!(
      DocDirectParagraphFormatting {
        style_index: 0,
        papx_properties: root.clone(),
        piece_properties: GrpPrl {
          properties: Vec::new(),
        },
        applied_properties: applied,
      }
      .table_state()
      .unwrap(),
      DocDirectTableState {
        in_table: true,
        depth: 1,
        depth_is_explicit: true,
        table_terminating_paragraph: false,
        inner_table_cell: false,
        inner_table_terminating_paragraph: false,
      }
    );
    assert!(expand_direct_paragraph_properties(&root, Some(&data), Some(1)).is_err());
    assert!(expand_direct_paragraph_properties(&root, None, Some(0)).is_err());
  }
}
