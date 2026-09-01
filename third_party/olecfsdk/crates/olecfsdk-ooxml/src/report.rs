use olecfsdk::doc::{DocNoteKind, FieldDocumentPart, TextboxDocumentPart};
use olecfsdk::shared_content::OfficePropertySetKind;

/// Policy for a source feature that has no implemented target mapping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LossPolicy {
  /// Stop before returning an OOXML package.
  #[default]
  Reject,
  /// Record the loss and continue with the explicitly degraded package.
  Report,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConversionOptions {
  pub unsupported: LossPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
  Mapped,
  PreservedAsPayload,
  Unsupported,
  NotApplicable,
  Rejected,
}

impl Disposition {
  const fn index(self) -> usize {
    match self {
      Self::Mapped => 0,
      Self::PreservedAsPayload => 1,
      Self::Unsupported => 2,
      Self::NotApplicable => 3,
      Self::Rejected => 4,
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DispositionCounts([usize; 5]);

impl DispositionCounts {
  pub const fn mapped(self) -> usize {
    self.0[Disposition::Mapped.index()]
  }

  pub const fn preserved_as_payload(self) -> usize {
    self.0[Disposition::PreservedAsPayload.index()]
  }

  pub const fn unsupported(self) -> usize {
    self.0[Disposition::Unsupported.index()]
  }

  pub const fn not_applicable(self) -> usize {
    self.0[Disposition::NotApplicable.index()]
  }

  pub const fn rejected(self) -> usize {
    self.0[Disposition::Rejected.index()]
  }

  pub const fn total(self) -> usize {
    self.0[0] + self.0[1] + self.0[2] + self.0[3] + self.0[4]
  }

  fn record(&mut self, disposition: Disposition) {
    self.0[disposition.index()] += 1;
  }
}

/// Compact source identity retained by a conversion issue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceLocation {
  Document,
  OfficeProperty {
    kind: OfficePropertySetKind,
    property_set_index: usize,
    property_identifier: u32,
  },
  OfficePropertySet {
    kind: OfficePropertySetKind,
  },
  OfficeVbaProject,
  DocStyle {
    style_index: u16,
  },
  DocSection {
    section_index: usize,
    start_cp: u32,
    end_cp: u32,
  },
  DocBookmark {
    bookmark_index: usize,
    part: FieldDocumentPart,
    start_cp: u32,
    end_cp: u32,
  },
  DocNote {
    kind: DocNoteKind,
    note_index: usize,
    reference_cp: u32,
    start_cp: u32,
    end_cp: u32,
  },
  DocComment {
    comment_index: usize,
    reference_cp: u32,
    start_cp: u32,
    end_cp: u32,
    selection_start_cp: u32,
    selection_end_cp: u32,
  },
  DocTextbox {
    document_part: TextboxDocumentPart,
    story_index: Option<usize>,
    shape_id: u32,
    anchor_cp: u32,
    start_cp: Option<u32>,
    end_cp: Option<u32>,
  },
  XlsWorkbook {
    workbook_index: usize,
  },
  XlsSheet {
    workbook_index: usize,
    sheet_index: usize,
  },
  XlsSharedString {
    workbook_index: usize,
    string_index: u32,
  },
  XlsCell {
    workbook_index: usize,
    sheet_index: usize,
    row: u16,
    column: u16,
  },
  XlsRow {
    workbook_index: usize,
    sheet_index: usize,
    row: u16,
  },
  XlsColumns {
    workbook_index: usize,
    sheet_index: usize,
    first_column: u16,
    last_column: u16,
  },
  XlsDrawing {
    workbook_index: usize,
    sheet_index: usize,
    shape_id: u32,
  },
  PptPresentation,
  PptMaster {
    master_index: usize,
    slide_id: u32,
  },
  PptSlide {
    slide_index: usize,
    slide_id: u32,
  },
  PptNotesMaster {
    persist_id: u32,
  },
  PptNotesSlide {
    slide_index: usize,
    notes_id: u32,
  },
  PptMasterShape {
    master_index: usize,
    shape_id: u32,
  },
  PptShape {
    slide_index: usize,
    shape_id: u32,
  },
  PptNotesMasterShape {
    shape_id: u32,
  },
  PptNotesShape {
    slide_index: usize,
    shape_id: u32,
  },
  DocRange {
    part: FieldDocumentPart,
    start_cp: u32,
    end_cp: u32,
  },
}

/// Stable typed reason code; human-readable text is formatted only on demand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversionCode {
  DocumentCreated,
  CorePropertyNotMapped,
  PropertySetNotMapped,
  VbaProjectNotMapped,
  ParagraphMapped,
  TextMapped,
  StyleFormattingNotMapped,
  StyleKindNotMapped,
  StyleNameCompatibilityUtf16,
  BookmarkNameCompatibilityUtf16,
  BookmarkNameNotMapped,
  BookmarkColumnRangeNotMapped,
  BookmarkStoryNotMapped,
  BookmarkBoundaryNotMapped,
  NoteCustomMarkNotMapped,
  NoteBoundaryNotMapped,
  CommentRelationshipNotMapped,
  CommentMetadataNotMapped,
  CommentThreadNotMapped,
  CommentInkNotMapped,
  CommentBoundaryNotMapped,
  TextboxRelationshipNotMapped,
  TextboxBoundaryNotMapped,
  TextboxFlowNotMapped,
  FloatingShapeNotMapped,
  FloatingPictureNotMapped,
  FloatingShapeGeometryNotMapped,
  FloatingShapeFormattingNotMapped,
  ParagraphFormattingNotMapped,
  CharacterFormattingNotMapped,
  SectionFormattingNotMapped,
  SectionBoundaryNotMapped,
  TableFormattingNotMapped,
  NonMainStoryNotMapped,
  CompatibilityUtf16,
  ControlCharacterNotMapped,
  InlinePictureNotMapped,
  InlineBinaryNotMapped,
  OleObjectNotMapped,
  AdditionalWorkbookStreamNotMapped,
  WorkbookFeatureNotMapped,
  WorkbookPropertiesNotMapped,
  WorkbookProtectionNotMapped,
  WorkbookCalculationNotMapped,
  WorkbookViewNotMapped,
  WorksheetFeatureNotMapped,
  WorksheetPropertiesNotMapped,
  WorksheetDimensionNotMapped,
  WorksheetPhoneticInformationNotMapped,
  WorksheetProtectionNotMapped,
  WorksheetProtectedRangeNotMapped,
  WorksheetAutoFilterNotMapped,
  WorksheetSortStateNotMapped,
  WorksheetCalculationNotMapped,
  WorksheetDefaultFormattingNotMapped,
  WorksheetViewNotMapped,
  WorksheetPaneNotMapped,
  WorksheetSelectionNotMapped,
  WorksheetPrintOptionsNotMapped,
  WorksheetPageMarginsNotMapped,
  WorksheetPageSetupNotMapped,
  WorksheetHeaderFooterNotMapped,
  WorksheetPageBreaksNotMapped,
  SheetKindNotMapped,
  SheetStateNotMapped,
  SharedStringRichTextNotMapped,
  SharedStringPhoneticTextNotMapped,
  SharedStringPhoneticCompatibilityNotMapped,
  CommentFormattingNotMapped,
  CellFormattingNotMapped,
  RowFormattingNotMapped,
  ColumnFormattingNotMapped,
  FormulaNotMapped,
  CompatibilityCellValue,
  HyperlinkTargetNotMapped,
  HyperlinkFrameNotMapped,
  SpreadsheetPictureNotMapped,
  SpreadsheetPictureAnchorNotMapped,
  SpreadsheetPictureFormattingNotMapped,
  PresentationFeatureNotMapped,
  MasterFeatureNotMapped,
  MasterRelationshipNotMapped,
  SlideFeatureNotMapped,
  SlideTransitionNotMapped,
  SlideTransitionFeatureNotMapped,
  ShapeIdentityNotMapped,
  ShapeGeometryNotMapped,
  PictureNotMapped,
  PictureTextNotMapped,
  TextFormattingNotMapped,
  MultipleTextBodiesNotMapped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConversionIssue {
  pub disposition: Disposition,
  pub code: ConversionCode,
  pub source: SourceLocation,
}

/// Conservation report. Successful mappings update counters without storing
/// per-node events; only exceptional dispositions allocate issue entries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConversionReport {
  counts: DispositionCounts,
  issues: Vec<ConversionIssue>,
}

impl ConversionReport {
  pub const fn counts(&self) -> DispositionCounts {
    self.counts
  }

  pub fn issues(&self) -> &[ConversionIssue] {
    &self.issues
  }

  pub(crate) fn record(&mut self, disposition: Disposition) {
    self.counts.record(disposition);
  }

  pub(crate) fn issue(
    &mut self,
    disposition: Disposition,
    code: ConversionCode,
    source: SourceLocation,
  ) {
    self.counts.record(disposition);
    self.issues.push(ConversionIssue {
      disposition,
      code,
      source,
    });
  }
}

#[derive(Debug)]
pub struct ConversionOutput<T> {
  pub document: T,
  pub report: ConversionReport,
}
