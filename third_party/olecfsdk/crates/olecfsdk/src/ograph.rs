//! Microsoft Graph binary chart documents (`[MS-OGRAPH]`).
//!
//! Graph objects use a CFB `/Workbook` stream whose record vocabulary shares
//! the chart records from BIFF8, but it is not an MS-XLS workbook. In
//! particular, its BOF version is `0x0680`, its data-sheet cell records have
//! Graph-specific layouts, and BRAI stores a row/column selector rather than
//! an MS-XLS formula token stream. This module keeps that dialect boundary
//! explicit while reusing the identical typed chart records from [`crate::xls`].

use std::{collections::BTreeMap, fs::File, io::Write, path::Path, sync::Arc};

use crate::{
  Error, Result,
  cfb::CompoundFile,
  io::BinaryFormat,
  limits::Limits,
  parse::{
    ParseDiagnostic, ParseDiagnosticCode, ParseOptions, ParseOutcome, SpecificationReference,
    compound_from_bytes, compound_from_path, compound_from_vec, compound_outcome,
  },
  save::SaveOptions,
  xls::{
    BiffRecord, BiffRecordData, Chart3DBarShapeRecord, Chart3DRecord, ChartAreaFormatRecord,
    ChartAreaRecord, ChartAttachedLabelRecord, ChartAxisOptionsRecord, ChartAxisRecord,
    ChartBarRecord, ChartBopPopCustomRecord, ChartBopPopRecord, ChartDataFormatRecord,
    ChartFormatRecord, ChartFrameRecord, ChartLabelRangeRecord, ChartLegendRecord,
    ChartLineFormatRecord, ChartLineRecord, ChartMarkerFormatRecord, ChartPieFormatRecord,
    ChartPieRecord, ChartPositionRecord, ChartRadarRecord, ChartRecord, ChartScatterRecord,
    ChartSeriesFormatRecord, ChartSeriesRecord, ChartSurfRecord, ChartTickRecord,
    ChartValueRangeRecord, EmptyRecordKind, FixedU16RecordKind, FrtFlags, FrtHeaderOld,
    MAX_BIFF_RECORD_DATA, XlStringCharacters, stitch_continued_records,
  },
};

pub const WORKBOOK_STREAM_PATH: &str = "/Workbook";
pub const COMPONENT_OBJECT_STREAM_PATH: &str = "/\u{1}CompObj";
pub const OLE_STREAM_PATH: &str = "/\u{1}Ole";

const GRAPH_BLANK: u16 = 0x0001;
const GRAPH_NUMBER: u16 = 0x0003;
const GRAPH_LABEL_COMPATIBILITY: u16 = 0x0004;
const GRAPH_SELECTION: u16 = 0x001d;
const GRAPH_COLUMN_WIDTH: u16 = 0x0024;
const GRAPH_WINDOW1: u16 = 0x003d;
const GRAPH_WINDOW2: u16 = 0x003e;
const GRAPH_LABEL: u16 = 0x0204;
const GRAPH_DIMENSIONS: u16 = 0x0200;
const GRAPH_CHART_COLORS: u16 = 0x02ac;
const GRAPH_FRT_WRAPPER: u16 = 0x0851;
const GRAPH_BRAI: u16 = 0x1051;
const GRAPH_BOF_DATASHEET: u16 = 0x1052;
const GRAPH_EXCLUDE_ROWS: u16 = 0x1053;
const GRAPH_EXCLUDE_COLUMNS: u16 = 0x1054;
const GRAPH_ORIENT: u16 = 0x1055;
const GRAPH_WIN_DOC: u16 = 0x1057;
const GRAPH_MAX_STATUS: u16 = 0x1058;
const GRAPH_MAIN_WINDOW: u16 = 0x1059;
const GRAPH_LINKED_SELECTION: u16 = 0x105e;

/// Record identifiers whose payload layout is identical in BIFF8 and
/// MS-OGRAPH. Graph-specific identifiers and the `Window1`/`Window1_10`
/// collision are handled separately below. This allow-list is the record
/// enumeration in MS-OGRAPH section 2.3; it prevents an arbitrary XLS record
/// from silently entering a Graph tree merely because the BIFF decoder knows
/// how to read it.
const OGRAPH_COMMON_RECORD_TYPES: &[u16] = &[
  0x000a, 0x0022, 0x0031, 0x003c, 0x0042, 0x0085, 0x008c, 0x0092, 0x00a0, 0x00eb, 0x00ec, 0x00ed,
  0x01b6, 0x041e, 0x0809, 0x0850, 0x0852, 0x0853, 0x0854, 0x0855, 0x0856, 0x0857, 0x085a, 0x086a,
  0x086b, 0x1001, 0x1002, 0x1003, 0x1006, 0x1007, 0x1009, 0x100a, 0x100b, 0x100c, 0x100d, 0x1014,
  0x1015, 0x1016, 0x1017, 0x1018, 0x1019, 0x101a, 0x101b, 0x101c, 0x101d, 0x101e, 0x101f, 0x1020,
  0x1021, 0x1022, 0x1024, 0x1025, 0x1026, 0x1027, 0x1032, 0x1033, 0x1034, 0x1035, 0x103a, 0x103c,
  0x103d, 0x103e, 0x103f, 0x1040, 0x1041, 0x1043, 0x1044, 0x1045, 0x1046, 0x1048, 0x104a, 0x104b,
  0x104e, 0x104f, 0x1050, 0x105b, 0x105c, 0x105d, 0x105f, 0x1060, 0x1061, 0x1062, 0x1063, 0x1064,
  0x1066, 0x1067, 0x1068,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OgraphShortString {
  /// Bit 0 is `fHighByte`; bits 1..=7 are retained for exact diagnostics and
  /// round trips even though MS-OGRAPH requires them to be zero.
  pub flags: u8,
  pub characters: XlStringCharacters,
}

impl OgraphShortString {
  pub fn text(&self) -> String {
    match &self.characters {
      XlStringCharacters::Compressed(bytes) => bytes
        .iter()
        .map(|byte| char::from_u32(u32::from(*byte)).unwrap_or('\u{fffd}'))
        .collect(),
      XlStringCharacters::Unicode(units) => String::from_utf16_lossy(units),
    }
  }

  fn parse(bytes: &[u8], position: u64) -> Result<Self> {
    if bytes.len() < 2 {
      return Err(Error::invalid(
        position,
        "ShortXLUnicodeString is truncated",
      ));
    }
    let count = usize::from(bytes[0]);
    let flags = bytes[1];
    let width = if flags & 1 == 0 { 1 } else { 2 };
    let expected = 2usize
      .checked_add(
        count
          .checked_mul(width)
          .ok_or_else(|| Error::Limit("ShortXLUnicodeString length overflow".into()))?,
      )
      .ok_or_else(|| Error::Limit("ShortXLUnicodeString length overflow".into()))?;
    if bytes.len() != expected {
      return Err(Error::invalid(
        position,
        "ShortXLUnicodeString length does not match cch/fHighByte",
      ));
    }
    let characters = if width == 1 {
      XlStringCharacters::Compressed(bytes[2..].to_vec())
    } else {
      XlStringCharacters::Unicode(
        bytes[2..]
          .chunks_exact(2)
          .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
          .collect(),
      )
    };
    Ok(Self { flags, characters })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let count = match &self.characters {
      XlStringCharacters::Compressed(bytes) => bytes.len(),
      XlStringCharacters::Unicode(units) => units.len(),
    };
    let mut bytes = Vec::with_capacity(2 + count * if self.flags & 1 == 0 { 1 } else { 2 });
    bytes.push(
      u8::try_from(count)
        .map_err(|_| Error::Limit("ShortXLUnicodeString exceeds 255 characters".into()))?,
    );
    bytes.push(self.flags);
    match (&self.characters, self.flags & 1) {
      (XlStringCharacters::Compressed(values), 0) => bytes.extend_from_slice(values),
      (XlStringCharacters::Unicode(values), 1) => {
        for value in values {
          bytes.extend_from_slice(&value.to_le_bytes());
        }
      }
      _ => {
        return Err(Error::invalid(
          0,
          "ShortXLUnicodeString characters do not match fHighByte",
        ));
      }
    }
    Ok(bytes)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphCellHeader {
  pub row: u16,
  pub column: u16,
  pub reserved: u8,
  pub format_index: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphBlankRecord {
  pub cell: OgraphCellHeader,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphNumberRecord {
  pub cell: OgraphCellHeader,
  pub value_bits: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OgraphLabelRecord {
  pub cell: OgraphCellHeader,
  pub text: OgraphShortString,
  /// `true` preserves the historical record identifier `0x0004` emitted by
  /// Microsoft Graph 8. The current MS-OGRAPH record enumeration requires
  /// `0x0204`, so strict open/save rejects this representation.
  pub legacy_record_id: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphDimensionsRecord {
  pub reserved1: u32,
  /// `rwMac`: number of non-empty cells in the longest datasheet row.
  pub longest_row_cell_count: u32,
  pub reserved2: u16,
  /// `colMac`: number of non-empty rows in the datasheet.
  pub non_empty_row_count: u16,
  pub reserved3: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphBofDatasheetRecord {
  pub unused1: u16,
  pub unused2: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphChartColorsRecord {
  pub color_count: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphColumnWidthRecord {
  pub first_column: u16,
  pub last_column: u16,
  pub width: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OgraphExclusionRecord {
  /// Alternating inclusive/exclusive transition indices. The first item is
  /// the first included row/column and the second is the following excluded
  /// row/column, continuing in pairs.
  pub transitions: Vec<u16>,
}

impl OgraphExclusionRecord {
  pub fn includes(&self, index: u16) -> bool {
    self
      .transitions
      .partition_point(|transition| *transition <= index)
      % 2
      == 1
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphOrientRecord {
  pub series_in_rows: u8,
  pub horizontal_series_row: u16,
  pub horizontal_series_column: u16,
  pub reserved: u8,
}

impl OgraphOrientRecord {
  pub const fn series_are_rows(self) -> bool {
    self.series_in_rows == 1
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphBraiRecord {
  pub data_role: u8,
  pub reference_type: u8,
  /// Bit 0 is `fUnlinkedIfmt`, bit 1 is the specified constant one bit, and
  /// the remaining bits are reserved.
  pub flags: u16,
  pub format_index: u16,
  pub row_or_column: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphWinDocRecord {
  pub chart_selected: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphMaxStatusRecord {
  pub unused1: u8,
  pub unused2: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphMainWindowRecord {
  pub left: i16,
  pub top: i16,
  pub width: i16,
  pub height: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphWindow1_10Record {
  pub x: u16,
  pub y: u16,
  pub width: u16,
  pub height: u16,
  pub reserved: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphWindow2Record {
  pub reserved1: u8,
  pub reserved2: u8,
  pub reserved3: u8,
  pub reserved4: u8,
  pub reserved5: u8,
  pub first_row: u16,
  pub first_column: u16,
  pub reserved6: u8,
  pub reserved7: u16,
  pub reserved8: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphSelectionRecord {
  pub pane: u8,
  pub active_row: u16,
  pub active_column: u16,
  pub reserved: u16,
  pub unused: u16,
  pub first_row: u16,
  pub last_row: u16,
  pub first_column: u16,
  pub last_column: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphLinkedSelectionRecord {
  pub first_row: u16,
  pub last_row: u16,
  pub first_column: u16,
  pub last_column: u16,
}

/// A complete logical Graph record carried inside an MS-OGRAPH FrtWrapper.
/// The nested value deliberately uses [`OgraphRecordData`], not the BIFF8
/// `FrtWrapperRecord`, because wrapped BRAI records retain Graph's row/column
/// selector layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OgraphFrtWrapperRecord {
  pub header: FrtHeaderOld,
  pub wrapped: Box<OgraphRecordData>,
  pub padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OgraphRecordData {
  /// A record whose physical and semantic layout is shared with BIFF8.
  Common(BiffRecordData),
  FrtWrapper(OgraphFrtWrapperRecord),
  BofDatasheet(OgraphBofDatasheetRecord),
  Blank(OgraphBlankRecord),
  Number(OgraphNumberRecord),
  Label(OgraphLabelRecord),
  Dimensions(OgraphDimensionsRecord),
  ChartColors(OgraphChartColorsRecord),
  ColumnWidth(OgraphColumnWidthRecord),
  ExcludeRows(OgraphExclusionRecord),
  ExcludeColumns(OgraphExclusionRecord),
  Orient(OgraphOrientRecord),
  Brai(OgraphBraiRecord),
  WinDoc(OgraphWinDocRecord),
  MaxStatus(OgraphMaxStatusRecord),
  MainWindow(OgraphMainWindowRecord),
  Window1_10(OgraphWindow1_10Record),
  Window2(OgraphWindow2Record),
  Selection(OgraphSelectionRecord),
  LinkedSelection(OgraphLinkedSelectionRecord),
  /// An unsupported vendor/future record retained only by compatible mode.
  Compatibility {
    record_type: u16,
    payload: Vec<u8>,
  },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OgraphRecord {
  pub offset: u32,
  pub data: OgraphRecordData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OgraphWorkbookStream {
  pub records: Vec<OgraphRecord>,
  pub trailing_padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OgraphRecordGroup {
  pub header_index: usize,
  pub end_index: usize,
  pub parent: Option<usize>,
  pub children: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OgraphRecordGroupTree {
  pub groups: Vec<OgraphRecordGroup>,
  pub top_level: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OgraphFile {
  compound_file: CompoundFile,
  /// Clone-shared Graph workbook record tree. Call [`Arc::make_mut`] before
  /// direct edits; [`Self::relayout`] detaches and rebuilds it transactionally.
  pub workbook: Arc<OgraphWorkbookStream>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OgraphCellValue {
  Blank,
  Number(f64),
  Text(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct OgraphCell {
  pub row: u16,
  pub column: u16,
  pub format_index: u16,
  pub value: OgraphCellValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OgraphSeries {
  pub index: usize,
  pub group_index: usize,
  pub included: bool,
  pub name: String,
  pub categories: Vec<Option<OgraphCellValue>>,
  pub values: Vec<Option<f64>>,
  pub bubble_sizes: Vec<Option<f64>>,
  pub source: ChartSeriesRecord,
  pub name_reference: OgraphBraiRecord,
  pub value_reference: OgraphBraiRecord,
  pub category_reference: OgraphBraiRecord,
  pub bubble_reference: OgraphBraiRecord,
  /// Series-wide and point-specific `DataFormat` groups in source order.
  pub data_formats: Vec<OgraphDataFormat>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OgraphDataFormat {
  pub source: ChartDataFormatRecord,
  pub area: Option<ChartAreaFormatRecord>,
  pub line: Option<ChartLineFormatRecord>,
  pub marker: Option<ChartMarkerFormatRecord>,
  pub pie: Option<ChartPieFormatRecord>,
  pub bar_shape: Option<Chart3DBarShapeRecord>,
  pub series: Option<ChartSeriesFormatRecord>,
  pub attached_label: Option<ChartAttachedLabelRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OgraphChartGroupKind {
  Bar(ChartBarRecord),
  Line(ChartLineRecord),
  Pie(ChartPieRecord),
  Area(ChartAreaRecord),
  Scatter(ChartScatterRecord),
  Radar(ChartRadarRecord),
  FilledRadar(ChartRadarRecord),
  Surface(ChartSurfRecord),
  BopPop(ChartBopPopRecord),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OgraphChartGroup {
  pub index: usize,
  pub axis_group: u16,
  pub format: ChartFormatRecord,
  pub kind: OgraphChartGroupKind,
  pub view_3d: Option<Chart3DRecord>,
  pub legend: Option<OgraphLegend>,
  pub bop_pop_custom: Option<ChartBopPopCustomRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OgraphFrame {
  pub source: ChartFrameRecord,
  pub line: Option<ChartLineFormatRecord>,
  pub area: Option<ChartAreaFormatRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OgraphLegend {
  pub source: ChartLegendRecord,
  pub position: OgraphPosition,
  pub frame: Option<OgraphFrame>,
}

/// MS-OGRAPH semantic view of a `Pos` record. The shared BIFF wire record
/// retains each unused high word for lossless saves; Graph consumers must use
/// only the signed low words defined by section 2.4.80.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgraphPosition {
  pub source: ChartPositionRecord,
  pub x1: i16,
  pub y1: i16,
  pub x2: i16,
  pub y2: i16,
}

impl OgraphPosition {
  fn from_source(source: ChartPositionRecord) -> Self {
    Self {
      x1: source.x1 as i16,
      y1: source.y1 as i16,
      x2: source.x2 as i16,
      y2: source.y2 as i16,
      source,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OgraphAxis {
  pub axis_group: u16,
  pub source: ChartAxisRecord,
  pub value_range: Option<ChartValueRangeRecord>,
  pub label_range: Option<ChartLabelRangeRecord>,
  pub options: Option<ChartAxisOptionsRecord>,
  pub tick: Option<ChartTickRecord>,
  /// Axis line, major gridlines, minor gridlines, and wall/floor line format,
  /// indexed by the MS-OGRAPH `AxisLine.id` value.
  pub line_formats: [Option<ChartLineFormatRecord>; 4],
  /// Fill paired with the same `AxisLine.id`, used for 3-D walls and floor.
  pub area_formats: [Option<ChartAreaFormatRecord>; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct OgraphChart {
  /// Chart-area bounds in 16.16 fixed-point points.
  pub source: ChartRecord,
  pub date_1904: bool,
  pub cells: Vec<OgraphCell>,
  pub orient: OgraphOrientRecord,
  pub excluded_rows: OgraphExclusionRecord,
  pub excluded_columns: OgraphExclusionRecord,
  pub groups: Vec<OgraphChartGroup>,
  pub axes: Vec<OgraphAxis>,
  pub series: Vec<OgraphSeries>,
  pub palette: Vec<u32>,
}

impl OgraphChart {
  pub fn width_points(&self) -> f64 {
    f64::from(self.source.width) / 65_536.0
  }

  pub fn height_points(&self) -> f64 {
    f64::from(self.source.height) / 65_536.0
  }
}

fn exact_length(payload: &[u8], expected: usize, offset: u32, name: &str) -> Result<()> {
  if payload.len() != expected {
    return Err(Error::invalid(
      u64::from(offset),
      format!(
        "MS-OGRAPH {name} record must contain {expected} bytes, found {}",
        payload.len()
      ),
    ));
  }
  Ok(())
}

fn read_u16(payload: &[u8], offset: usize) -> u16 {
  u16::from_le_bytes([payload[offset], payload[offset + 1]])
}

fn read_i16(payload: &[u8], offset: usize) -> i16 {
  i16::from_le_bytes([payload[offset], payload[offset + 1]])
}

fn read_u32(payload: &[u8], offset: usize) -> u32 {
  u32::from_le_bytes(payload[offset..offset + 4].try_into().expect("four bytes"))
}

fn read_u64(payload: &[u8], offset: usize) -> u64 {
  u64::from_le_bytes(payload[offset..offset + 8].try_into().expect("eight bytes"))
}

fn cell_header(payload: &[u8]) -> OgraphCellHeader {
  OgraphCellHeader {
    row: read_u16(payload, 0),
    column: read_u16(payload, 2),
    reserved: payload[4],
    format_index: read_u16(payload, 5),
  }
}

fn decode_special_record(
  record_type: u16,
  payload: &[u8],
  offset: u32,
) -> Result<Option<OgraphRecordData>> {
  let value = match record_type {
    GRAPH_BLANK => {
      exact_length(payload, 7, offset, "Blank")?;
      OgraphRecordData::Blank(OgraphBlankRecord {
        cell: cell_header(payload),
      })
    }
    GRAPH_NUMBER => {
      exact_length(payload, 15, offset, "Number")?;
      OgraphRecordData::Number(OgraphNumberRecord {
        cell: cell_header(payload),
        value_bits: read_u64(payload, 7),
      })
    }
    GRAPH_LABEL | GRAPH_LABEL_COMPATIBILITY => {
      if payload.len() < 9 {
        return Err(Error::invalid(
          u64::from(offset),
          "MS-OGRAPH Label record is truncated",
        ));
      }
      OgraphRecordData::Label(OgraphLabelRecord {
        cell: cell_header(payload),
        text: OgraphShortString::parse(&payload[7..], u64::from(offset) + 11)?,
        legacy_record_id: record_type == GRAPH_LABEL_COMPATIBILITY,
      })
    }
    GRAPH_DIMENSIONS => {
      exact_length(payload, 14, offset, "Dimensions")?;
      OgraphRecordData::Dimensions(OgraphDimensionsRecord {
        reserved1: read_u32(payload, 0),
        longest_row_cell_count: read_u32(payload, 4),
        reserved2: read_u16(payload, 8),
        non_empty_row_count: read_u16(payload, 10),
        reserved3: read_u16(payload, 12),
      })
    }
    GRAPH_CHART_COLORS => {
      exact_length(payload, 2, offset, "ChartColors")?;
      OgraphRecordData::ChartColors(OgraphChartColorsRecord {
        color_count: read_i16(payload, 0),
      })
    }
    GRAPH_COLUMN_WIDTH => {
      exact_length(payload, 6, offset, "ColumnWidth")?;
      OgraphRecordData::ColumnWidth(OgraphColumnWidthRecord {
        first_column: read_u16(payload, 0),
        last_column: read_u16(payload, 2),
        width: read_u16(payload, 4),
      })
    }
    GRAPH_EXCLUDE_ROWS | GRAPH_EXCLUDE_COLUMNS => {
      if payload.len() % 2 != 0 {
        return Err(Error::invalid(
          u64::from(offset),
          "MS-OGRAPH exclusion record must contain an even number of bytes",
        ));
      }
      let value = OgraphExclusionRecord {
        transitions: payload
          .chunks_exact(2)
          .map(|pair| read_u16(pair, 0))
          .collect(),
      };
      if record_type == GRAPH_EXCLUDE_ROWS {
        OgraphRecordData::ExcludeRows(value)
      } else {
        OgraphRecordData::ExcludeColumns(value)
      }
    }
    GRAPH_ORIENT => {
      exact_length(payload, 6, offset, "Orient")?;
      OgraphRecordData::Orient(OgraphOrientRecord {
        series_in_rows: payload[0],
        horizontal_series_row: read_u16(payload, 1),
        horizontal_series_column: read_u16(payload, 3),
        reserved: payload[5],
      })
    }
    GRAPH_BRAI => {
      exact_length(payload, 8, offset, "BRAI")?;
      OgraphRecordData::Brai(OgraphBraiRecord {
        data_role: payload[0],
        reference_type: payload[1],
        flags: read_u16(payload, 2),
        format_index: read_u16(payload, 4),
        row_or_column: read_u16(payload, 6),
      })
    }
    GRAPH_BOF_DATASHEET => {
      exact_length(payload, 4, offset, "BOFDatasheet")?;
      OgraphRecordData::BofDatasheet(OgraphBofDatasheetRecord {
        unused1: read_u16(payload, 0),
        unused2: read_u16(payload, 2),
      })
    }
    GRAPH_WIN_DOC => {
      exact_length(payload, 1, offset, "WinDoc")?;
      OgraphRecordData::WinDoc(OgraphWinDocRecord {
        chart_selected: payload[0],
      })
    }
    GRAPH_MAX_STATUS => {
      exact_length(payload, 2, offset, "MaxStatus")?;
      OgraphRecordData::MaxStatus(OgraphMaxStatusRecord {
        unused1: payload[0],
        unused2: payload[1],
      })
    }
    GRAPH_MAIN_WINDOW => {
      exact_length(payload, 8, offset, "MainWindow")?;
      OgraphRecordData::MainWindow(OgraphMainWindowRecord {
        left: read_i16(payload, 0),
        top: read_i16(payload, 2),
        width: read_i16(payload, 4),
        height: read_i16(payload, 6),
      })
    }
    GRAPH_WINDOW1 if payload.len() == 10 => OgraphRecordData::Window1_10(OgraphWindow1_10Record {
      x: read_u16(payload, 0),
      y: read_u16(payload, 2),
      width: read_u16(payload, 4),
      height: read_u16(payload, 6),
      reserved: read_u16(payload, 8),
    }),
    GRAPH_WINDOW2 => {
      exact_length(payload, 14, offset, "Window2Graph")?;
      OgraphRecordData::Window2(OgraphWindow2Record {
        reserved1: payload[0],
        reserved2: payload[1],
        reserved3: payload[2],
        reserved4: payload[3],
        reserved5: payload[4],
        first_row: read_u16(payload, 5),
        first_column: read_u16(payload, 7),
        reserved6: payload[9],
        reserved7: read_u16(payload, 10),
        reserved8: read_u16(payload, 12),
      })
    }
    GRAPH_SELECTION => {
      exact_length(payload, 17, offset, "Selection")?;
      OgraphRecordData::Selection(OgraphSelectionRecord {
        pane: payload[0],
        active_row: read_u16(payload, 1),
        active_column: read_u16(payload, 3),
        reserved: read_u16(payload, 5),
        unused: read_u16(payload, 7),
        first_row: read_u16(payload, 9),
        last_row: read_u16(payload, 11),
        first_column: read_u16(payload, 13),
        last_column: read_u16(payload, 15),
      })
    }
    GRAPH_LINKED_SELECTION => {
      exact_length(payload, 8, offset, "LinkedSelection")?;
      OgraphRecordData::LinkedSelection(OgraphLinkedSelectionRecord {
        first_row: read_u16(payload, 0),
        last_row: read_u16(payload, 2),
        first_column: read_u16(payload, 4),
        last_column: read_u16(payload, 6),
      })
    }
    _ => return Ok(None),
  };
  Ok(Some(value))
}

fn validate_cell_header(cell: OgraphCellHeader, offset: u32, name: &str) -> Result<()> {
  if cell.row > 0x0f9f || cell.column > 0x00ff {
    return Err(Error::invalid(
      u64::from(offset),
      format!("MS-OGRAPH {name} cell coordinate is out of range"),
    ));
  }
  if cell.reserved != 0 {
    return Err(Error::invalid(
      u64::from(offset),
      format!("MS-OGRAPH {name}.reserved must be zero"),
    ));
  }
  Ok(())
}

fn validate_special_record(data: &OgraphRecordData, offset: u32) -> Result<()> {
  match data {
    OgraphRecordData::FrtWrapper(value) => {
      if value.header.record_type != GRAPH_FRT_WRAPPER
        || !value.header.flags.is_empty()
        || value.padding.iter().any(|byte| *byte != 0)
      {
        return Err(Error::invalid(
          u64::from(offset),
          "MS-OGRAPH FrtWrapper header/padding is invalid",
        ));
      }
      validate_special_record(&value.wrapped, offset)
    }
    OgraphRecordData::Blank(value) => validate_cell_header(value.cell, offset, "Blank"),
    OgraphRecordData::Number(value) => validate_cell_header(value.cell, offset, "Number"),
    OgraphRecordData::Label(value) => {
      validate_cell_header(value.cell, offset, "Label")?;
      if value.text.flags & !1 != 0 {
        return Err(Error::invalid(
          u64::from(offset),
          "MS-OGRAPH Label string reserved flags must be zero",
        ));
      }
      Ok(())
    }
    OgraphRecordData::Dimensions(value) => {
      if value.reserved1 != 0 || value.reserved2 != 0 || value.reserved3 != 0 {
        return Err(Error::invalid(
          u64::from(offset),
          "MS-OGRAPH Dimensions reserved fields must be zero",
        ));
      }
      if value.longest_row_cell_count > 0x0f9f || value.non_empty_row_count > 0x00ff {
        return Err(Error::invalid(
          u64::from(offset),
          "MS-OGRAPH Dimensions extent is out of range",
        ));
      }
      Ok(())
    }
    OgraphRecordData::ChartColors(value) if value.color_count != 0x0038 => Err(Error::invalid(
      u64::from(offset),
      "MS-OGRAPH ChartColors.icvMac must be 0x0038",
    )),
    OgraphRecordData::ColumnWidth(value) => {
      if value.first_column > 0x00ff
        || value.last_column > 0x00ff
        || value.last_column < value.first_column
      {
        return Err(Error::invalid(
          u64::from(offset),
          "MS-OGRAPH ColumnWidth range is invalid",
        ));
      }
      Ok(())
    }
    OgraphRecordData::ExcludeRows(value) | OgraphRecordData::ExcludeColumns(value) => {
      if value.transitions.len() % 2 != 0
        || value.transitions.windows(2).any(|pair| pair[0] >= pair[1])
      {
        return Err(Error::invalid(
          u64::from(offset),
          "MS-OGRAPH exclusion transitions must be an increasing even-sized collection",
        ));
      }
      let maximum = if matches!(data, OgraphRecordData::ExcludeRows(_)) {
        0x0f9f
      } else {
        0x00ff
      };
      if value
        .transitions
        .iter()
        .any(|transition| *transition > maximum)
      {
        return Err(Error::invalid(
          u64::from(offset),
          "MS-OGRAPH exclusion transition is out of range",
        ));
      }
      Ok(())
    }
    OgraphRecordData::Orient(value) => {
      if value.series_in_rows > 1
        || value.horizontal_series_row > 0x0f9f
        || value.horizontal_series_column > 0x00ff
        || value.reserved != 1
      {
        return Err(Error::invalid(
          u64::from(offset),
          "MS-OGRAPH Orient fields are invalid",
        ));
      }
      Ok(())
    }
    OgraphRecordData::Brai(value) => {
      if value.data_role > 3
        || value.reference_type > 2
        || value.flags & 0xfffc != 0
        || value.flags & 0x0002 == 0
        || value.row_or_column > 0x0f9f
      {
        return Err(Error::invalid(
          u64::from(offset),
          "MS-OGRAPH BRAI fields are invalid",
        ));
      }
      Ok(())
    }
    OgraphRecordData::WinDoc(value) if value.chart_selected > 1 => Err(Error::invalid(
      u64::from(offset),
      "MS-OGRAPH WinDoc.fChartSelected must be Boolean",
    )),
    OgraphRecordData::MainWindow(value) if value.width < 0 || value.height < 0 => {
      Err(Error::invalid(
        u64::from(offset),
        "MS-OGRAPH MainWindow dimensions must be nonnegative",
      ))
    }
    OgraphRecordData::Window1_10(value)
      if value.width == 0 || value.height == 0 || value.reserved != 0 =>
    {
      Err(Error::invalid(
        u64::from(offset),
        "MS-OGRAPH Window1_10 dimensions/reserved field are invalid",
      ))
    }
    OgraphRecordData::Window2(value)
      if value.reserved1 != 1
        || value.reserved2 != 1
        || value.reserved3 != 1
        || value.reserved4 != 0
        || value.reserved5 != 1
        || value.first_row == 0
        || value.first_column == 0
        || value.reserved6 != 1
        || value.reserved7 != 0
        || value.reserved8 != 0 =>
    {
      Err(Error::invalid(
        u64::from(offset),
        "MS-OGRAPH Window2Graph reserved/range fields are invalid",
      ))
    }
    OgraphRecordData::Selection(value)
      if value.pane != 3
        || value.reserved != 0
        || value.first_row > value.last_row
        || value.first_column > value.last_column
        || !(value.first_row..=value.last_row).contains(&value.active_row)
        || !(value.first_column..=value.last_column).contains(&value.active_column)
        || value.last_row > 0x0f9f
        || value.last_column > 0x00ff =>
    {
      Err(Error::invalid(
        u64::from(offset),
        "MS-OGRAPH Selection fields are invalid",
      ))
    }
    OgraphRecordData::LinkedSelection(value)
      if value.first_row != value.last_row
        || value.first_column != value.last_column
        || value.first_row > 1
        || value.first_column > 1 =>
    {
      Err(Error::invalid(
        u64::from(offset),
        "MS-OGRAPH LinkedSelection fields are invalid",
      ))
    }
    _ => Ok(()),
  }
}

fn record_warning(
  diagnostics: &mut Vec<ParseDiagnostic>,
  offset: u32,
  structure: &'static str,
  section: &'static str,
  message: impl Into<String>,
) {
  diagnostics.push(ParseDiagnostic::warning(
    ParseDiagnosticCode::NonconformingRecord,
    BinaryFormat::Ograph,
    Some(WORKBOOK_STREAM_PATH),
    Some(u64::from(offset)),
    structure,
    SpecificationReference {
      document: "MS-OGRAPH",
      section,
    },
    message,
  ));
}

fn is_ograph_wrapped_record(record_type: u16) -> bool {
  matches!(
    record_type,
    0x0031
      | 0x003c
      | 0x1007
      | 0x100a
      | 0x100d
      | 0x1024
      | 0x1025
      | 0x1026
      | 0x1027
      | 0x1032
      | 0x1033
      | 0x1034
      | 0x103c
      | 0x104f
      | 0x1050
      | 0x1051
      | 0x1060
      | 0x1066
  )
}

fn decode_ograph_frt_wrapper(
  payload: &[u8],
  offset: u32,
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<OgraphRecordData> {
  if payload.len() < 8 {
    return Err(Error::invalid(
      u64::from(offset),
      "MS-OGRAPH FrtWrapper is truncated",
    ));
  }
  let header = FrtHeaderOld {
    record_type: read_u16(payload, 0),
    flags: FrtFlags::from_bits_retain(read_u16(payload, 2)),
  };
  if header.record_type != GRAPH_FRT_WRAPPER || !header.flags.is_empty() {
    return Err(Error::invalid(
      u64::from(offset),
      "MS-OGRAPH FrtWrapper future-record header is invalid",
    ));
  }
  let wrapped_type = read_u16(payload, 4);
  let wrapped_size = usize::from(read_u16(payload, 6));
  if !is_ograph_wrapped_record(wrapped_type) || wrapped_size > MAX_BIFF_RECORD_DATA {
    return Err(Error::invalid(
      u64::from(offset),
      "MS-OGRAPH FrtWrapper wrapped record type/size is invalid",
    ));
  }
  let wrapped_end = 8usize
    .checked_add(wrapped_size)
    .ok_or_else(|| Error::Limit("MS-OGRAPH FrtWrapper size overflow".into()))?;
  let padding_size = 8usize.saturating_sub(wrapped_size + 4);
  let expected = wrapped_end
    .checked_add(padding_size)
    .ok_or_else(|| Error::Limit("MS-OGRAPH FrtWrapper size overflow".into()))?;
  if payload.len() != expected || payload[wrapped_end..].iter().any(|value| *value != 0) {
    return Err(Error::invalid(
      u64::from(offset),
      "MS-OGRAPH FrtWrapper padding is invalid",
    ));
  }
  let wrapped = decode_ograph_record(
    wrapped_type,
    &payload[8..wrapped_end],
    offset
      .checked_add(8)
      .ok_or_else(|| Error::Limit("MS-OGRAPH wrapped-record offset overflow".into()))?,
    options,
    diagnostics,
  )?;
  Ok(OgraphRecordData::FrtWrapper(OgraphFrtWrapperRecord {
    header,
    wrapped: Box::new(wrapped),
    padding: payload[wrapped_end..].to_vec(),
  }))
}

fn is_ograph_common_record(record_type: u16, payload: &[u8]) -> bool {
  (record_type == GRAPH_WINDOW1 && payload.len() != 10)
    || OGRAPH_COMMON_RECORD_TYPES.contains(&record_type)
}

fn decode_ograph_record(
  record_type: u16,
  payload: &[u8],
  offset: u32,
  options: ParseOptions,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<OgraphRecordData> {
  if record_type == GRAPH_FRT_WRAPPER {
    return decode_ograph_frt_wrapper(payload, offset, options, diagnostics);
  }
  if record_type == GRAPH_LABEL_COMPATIBILITY && options.is_strict() {
    return Err(Error::invalid(
      u64::from(offset),
      "MS-OGRAPH Label must use record type 0x0204, not historical 0x0004",
    ));
  }

  let decoded = if let Some(value) = decode_special_record(record_type, payload, offset)? {
    value
  } else if is_ograph_common_record(record_type, payload) {
    match BiffRecordData::decode_ograph_common(record_type, payload, offset as usize)? {
      BiffRecordData::Unknown { .. } => {
        return Err(Error::invalid(
          u64::from(offset),
          format!(
            "MS-OGRAPH record 0x{record_type:04x} is declared by section 2.3 but lacks a typed implementation"
          ),
        ));
      }
      value => OgraphRecordData::Common(value),
    }
  } else if options.is_strict() {
    return Err(Error::invalid(
      u64::from(offset),
      format!("record type 0x{record_type:04x} is not declared by MS-OGRAPH section 2.3"),
    ));
  } else {
    record_warning(
      diagnostics,
      offset,
      "Record",
      "2.3",
      format!("preserved undeclared Graph record type 0x{record_type:04x}"),
    );
    OgraphRecordData::Compatibility {
      record_type,
      payload: payload.to_vec(),
    }
  };

  if let Err(error) = validate_special_record(&decoded, offset) {
    if options.is_strict() {
      return Err(error);
    }
    record_warning(
      diagnostics,
      offset,
      "Record",
      "2.4",
      format!("preserved typed nonconforming record: {error}"),
    );
  }
  if record_type == GRAPH_LABEL_COMPATIBILITY {
    record_warning(
      diagnostics,
      offset,
      "Label",
      "2.4.58",
      "preserved historical Microsoft Graph 8 Label record type 0x0004; MS-OGRAPH declares 0x0204",
    );
  }
  Ok(decoded)
}

fn physical_record_for_stitching(
  record_type: u16,
  payload: &[u8],
  offset: u32,
  options: ParseOptions,
) -> Result<BiffRecordData> {
  if record_type == GRAPH_FRT_WRAPPER
    || decode_special_record(record_type, payload, offset)?.is_some()
    || !is_ograph_common_record(record_type, payload)
  {
    return Ok(BiffRecordData::Unknown {
      record_type,
      payload: payload.to_vec(),
    });
  }
  match BiffRecordData::decode_ograph_common(record_type, payload, offset as usize) {
    Ok(value) => Ok(value),
    Err(error) if options.is_strict() => Err(error),
    Err(_) => Ok(BiffRecordData::Unknown {
      record_type,
      payload: payload.to_vec(),
    }),
  }
}

impl OgraphWorkbookStream {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Ok(Self::from_bytes_with_options(bytes, ParseOptions::default())?.into_value())
  }

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
    if bytes.len() as u64 > options.limits.max_stream_size {
      return Err(Error::Limit(format!(
        "MS-OGRAPH Workbook stream length {} exceeds {}",
        bytes.len(),
        options.limits.max_stream_size
      )));
    }

    let mut physical = Vec::new();
    let mut cursor = 0usize;
    let mut trailing_padding = Vec::new();
    while cursor < bytes.len() {
      if bytes[cursor..].iter().all(|value| *value == 0) {
        trailing_padding.extend_from_slice(&bytes[cursor..]);
        break;
      }
      let offset = cursor;
      if bytes.len() - cursor < 4 {
        return Err(Error::invalid(
          cursor as u64,
          "truncated MS-OGRAPH record header",
        ));
      }
      let record_type = read_u16(bytes, cursor);
      let size = usize::from(read_u16(bytes, cursor + 2));
      cursor += 4;
      if size > MAX_BIFF_RECORD_DATA {
        return Err(Error::invalid(
          offset as u64 + 2,
          "MS-OGRAPH record data exceeds 8224 bytes",
        ));
      }
      let end = cursor
        .checked_add(size)
        .ok_or_else(|| Error::Limit("MS-OGRAPH record end overflow".into()))?;
      if end > bytes.len() {
        return Err(Error::invalid(
          offset as u64,
          "truncated MS-OGRAPH record data",
        ));
      }
      let payload = &bytes[cursor..end];
      let offset = u32::try_from(offset)
        .map_err(|_| Error::Limit("MS-OGRAPH record offset exceeds u32".into()))?;
      physical.push(BiffRecord {
        offset,
        data: physical_record_for_stitching(record_type, payload, offset, options)?,
      });
      if physical.len() > options.limits.max_entries {
        return Err(Error::Limit(format!(
          "MS-OGRAPH record count exceeds {}",
          options.limits.max_entries
        )));
      }
      cursor = end;
    }

    stitch_continued_records(&mut physical, options.limits)?;
    let mut diagnostics = Vec::new();
    let records = physical
      .into_iter()
      .map(|record| {
        let offset = record.offset;
        let data = match record.data {
          BiffRecordData::Unknown {
            record_type,
            payload,
          } => decode_ograph_record(record_type, &payload, offset, options, &mut diagnostics)?,
          data => OgraphRecordData::Common(data),
        };
        Ok(OgraphRecord { offset, data })
      })
      .collect::<Result<Vec<_>>>()?;
    let stream = Self {
      records,
      trailing_padding,
    };
    stream.validate(options.is_strict())?;
    Ok(ParseOutcome::new(stream, diagnostics))
  }

  pub fn groups(&self) -> Result<OgraphRecordGroupTree> {
    OgraphRecordGroupTree::from_workbook(self)
  }

  fn validate(&self, strict_records: bool) -> Result<()> {
    if strict_records {
      for record in &self.records {
        validate_special_record(&record.data, record.offset)?;
      }
    }
    let bof_indices = self
      .records
      .iter()
      .enumerate()
      .filter_map(|(index, record)| match &record.data {
        OgraphRecordData::Common(BiffRecordData::Bof(_)) => Some(index),
        _ => None,
      })
      .collect::<Vec<_>>();
    let eof_indices = self
      .records
      .iter()
      .enumerate()
      .filter_map(|(index, record)| match &record.data {
        OgraphRecordData::Common(BiffRecordData::Eof) => Some(index),
        _ => None,
      })
      .collect::<Vec<_>>();
    if bof_indices.len() != 2 || eof_indices.len() != 2 {
      return Err(Error::invalid(
        0,
        "MS-OGRAPH Workbook must contain exactly one globals and one chart-sheet substream",
      ));
    }
    if bof_indices[0] != 0
      || eof_indices[0] + 1 != bof_indices[1]
      || eof_indices[1] + 1 != self.records.len()
    {
      return Err(Error::invalid(
        0,
        "MS-OGRAPH Workbook substreams are out of order or contain records outside BOF/EOF",
      ));
    }
    let bof = |index: usize| match &self.records[index].data {
      OgraphRecordData::Common(BiffRecordData::Bof(value)) => value,
      _ => unreachable!(),
    };
    if bof(bof_indices[0]).version != 0x0680 || bof(bof_indices[0]).document_type != 0x0005 {
      return Err(Error::invalid(
        u64::from(self.records[bof_indices[0]].offset),
        "MS-OGRAPH globals BOF must have vers=0x0680 and dt=0x0005",
      ));
    }
    if bof(bof_indices[1]).version != 0x0680 || bof(bof_indices[1]).document_type != 0x8000 {
      return Err(Error::invalid(
        u64::from(self.records[bof_indices[1]].offset),
        "MS-OGRAPH chart-sheet BOF must have vers=0x0680 and dt=0x8000",
      ));
    }

    let globals = &self.records[1..eof_indices[0]];
    let bound_sheets = globals
      .iter()
      .filter_map(|record| match &record.data {
        OgraphRecordData::Common(BiffRecordData::BoundSheet8(value)) => Some((record, value)),
        _ => None,
      })
      .collect::<Vec<_>>();
    if bound_sheets.len() != 1 {
      return Err(Error::invalid(
        0,
        "MS-OGRAPH globals must contain exactly one BoundSheet8",
      ));
    }
    let (record, sheet) = bound_sheets[0];
    if sheet.state & 0x03 != 0
      || sheet.sheet_type != 0x02
      || !sheet.name.value.is_empty()
      || sheet.sheet_bof_offset != self.records[bof_indices[1]].offset
    {
      return Err(Error::invalid(
        u64::from(record.offset),
        "MS-OGRAPH BoundSheet8 does not identify the visible unnamed chart sheet",
      ));
    }

    let chart = &self.records[bof_indices[1] + 1..eof_indices[1]];
    let datasheets = chart
      .iter()
      .enumerate()
      .filter(|(_, record)| matches!(&record.data, OgraphRecordData::BofDatasheet(_)))
      .collect::<Vec<_>>();
    if datasheets.len() != 1 {
      return Err(Error::invalid(
        0,
        "MS-OGRAPH chart sheet must contain exactly one BOFDatasheet",
      ));
    }
    let datasheet_absolute = bof_indices[1] + 1 + datasheets[0].0;
    if !matches!(
      self
        .records
        .get(datasheet_absolute + 1)
        .map(|record| &record.data),
      Some(OgraphRecordData::Common(BiffRecordData::Empty {
        kind: EmptyRecordKind::ChartBegin,
        ..
      }))
    ) {
      return Err(Error::invalid(
        u64::from(self.records[datasheet_absolute].offset),
        "MS-OGRAPH BOFDatasheet must begin its collection with Begin",
      ));
    }
    for (label, count) in [
      (
        "Chart",
        chart
          .iter()
          .filter(|record| {
            matches!(
              &record.data,
              OgraphRecordData::Common(BiffRecordData::Chart(_))
            )
          })
          .count(),
      ),
      (
        "Dimensions",
        chart
          .iter()
          .filter(|record| matches!(&record.data, OgraphRecordData::Dimensions(_)))
          .count(),
      ),
      (
        "Orient",
        chart
          .iter()
          .filter(|record| matches!(&record.data, OgraphRecordData::Orient(_)))
          .count(),
      ),
      (
        "ExcludeRows",
        chart
          .iter()
          .filter(|record| matches!(&record.data, OgraphRecordData::ExcludeRows(_)))
          .count(),
      ),
      (
        "ExcludeColumns",
        chart
          .iter()
          .filter(|record| matches!(&record.data, OgraphRecordData::ExcludeColumns(_)))
          .count(),
      ),
    ] {
      if count != 1 {
        return Err(Error::invalid(
          0,
          format!("MS-OGRAPH chart sheet must contain exactly one {label} record"),
        ));
      }
    }

    for (index, record) in chart.iter().enumerate() {
      if let OgraphRecordData::ChartColors(colors) = &record.data {
        let Some(OgraphRecord {
          data: OgraphRecordData::Common(BiffRecordData::Palette(palette)),
          ..
        }) = chart.get(index + 1)
        else {
          return Err(Error::invalid(
            u64::from(record.offset),
            "MS-OGRAPH ChartColors must be immediately followed by Palette",
          ));
        };
        if usize::try_from(colors.color_count).ok() != Some(palette.colors.len()) {
          return Err(Error::invalid(
            u64::from(record.offset),
            "MS-OGRAPH ChartColors count does not match Palette",
          ));
        }
      }
    }

    self.groups()?;
    if strict_records
      && self
        .records
        .iter()
        .any(|record| contains_ograph_compatibility(&record.data))
    {
      return Err(Error::invalid(
        0,
        "strict MS-OGRAPH tree contains a compatibility record",
      ));
    }
    Ok(())
  }
}

impl OgraphFile {
  /// Returns the retained source CFB snapshot. Managed `/Workbook` edits are
  /// materialized by [`Self::to_compound_file`].
  pub fn source_compound_file(&self) -> &CompoundFile {
    &self.compound_file
  }

  /// Opens a complete Graph CFB in strict mode.
  pub fn open(path: impl AsRef<Path>) -> Result<Self> {
    Ok(Self::open_with_options(path, ParseOptions::default())?.into_value())
  }

  /// Opens a complete Graph CFB in compatible mode and returns structured
  /// diagnostics for every retained producer deviation.
  pub fn open_compatible(path: impl AsRef<Path>) -> Result<ParseOutcome<Self>> {
    Self::open_with_options(path, ParseOptions::compatible(Limits::default()))
  }

  pub fn open_with_options(
    path: impl AsRef<Path>,
    options: ParseOptions,
  ) -> Result<ParseOutcome<Self>> {
    let compound = compound_from_path(path.as_ref(), options, BinaryFormat::Ograph)?;
    Self::from_compound_outcome(compound, options)
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Ok(Self::from_bytes_with_options(bytes, ParseOptions::default())?.into_value())
  }

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
    let compound = compound_from_bytes(bytes, options, BinaryFormat::Ograph)?;
    Self::from_compound_outcome(compound, options)
  }

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
    let compound = compound_from_vec(bytes, options, BinaryFormat::Ograph)?;
    Self::from_compound_outcome(compound, options)
  }

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
    let compound = compound_outcome(compound_file, options, BinaryFormat::Ograph)?;
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
    let entry = compound_file
      .entry(WORKBOOK_STREAM_PATH)
      .ok_or_else(|| Error::invalid(0, "MS-OGRAPH /Workbook stream is missing"))?;
    if !entry.is_stream() || entry.path.parent() != Some(Path::new("/")) {
      return Err(Error::invalid(
        0,
        "MS-OGRAPH Workbook must be a stream directly beneath the CFB root",
      ));
    }
    if entry.name != "Workbook" {
      if options.is_strict() {
        return Err(Error::invalid(
          0,
          "MS-OGRAPH Workbook stream name must have exact casing",
        ));
      }
      diagnostics.push(ParseDiagnostic::warning(
        ParseDiagnosticCode::NonconformingRecord,
        BinaryFormat::Ograph,
        entry.path.to_str(),
        None,
        "Workbook Stream",
        SpecificationReference {
          document: "MS-OGRAPH",
          section: "2.1.3",
        },
        "preserved a case-variant Workbook stream name",
      ));
    }
    let workbook = OgraphWorkbookStream::from_bytes_with_options(&entry.data, options)?;
    diagnostics.extend(workbook.diagnostics);
    Ok(ParseOutcome::new(
      Self {
        compound_file,
        workbook: Arc::new(workbook.value),
      },
      diagnostics,
    ))
  }

  /// Recomputes Graph record offsets and BoundSheet8.lbPlyPos transactionally.
  pub fn relayout(&mut self) -> Result<()> {
    let mut rebuilt = self.clone();
    Arc::make_mut(&mut rebuilt.workbook).relayout()?;
    *self = rebuilt;
    Ok(())
  }

  pub fn relayout_preserving_compatibility(&mut self) -> Result<()> {
    let mut rebuilt = self.clone();
    Arc::make_mut(&mut rebuilt.workbook).relayout_preserving_compatibility()?;
    *self = rebuilt;
    Ok(())
  }

  pub fn to_compound_file(&self) -> Result<CompoundFile> {
    self.to_compound_file_with_options(SaveOptions::default())
  }

  pub fn to_compound_file_preserving_compatibility(&self) -> Result<CompoundFile> {
    self.to_compound_file_with_options(SaveOptions::preserving_compatibility())
  }

  pub fn to_compound_file_with_options(&self, options: SaveOptions) -> Result<CompoundFile> {
    if !options.preserves_compatibility()
      && self
        .compound_file
        .entry(WORKBOOK_STREAM_PATH)
        .is_some_and(|entry| entry.name != "Workbook")
    {
      return Err(Error::invalid(
        0,
        "strict MS-OGRAPH save rejects a case-variant Workbook stream name",
      ));
    }
    let workbook = self.workbook.to_bytes_with_options(options)?;
    let mut compound = self.compound_file.clone();
    compound.upsert_stream(WORKBOOK_STREAM_PATH, workbook)?;
    Ok(compound)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.to_bytes_with_options(SaveOptions::default())
  }

  pub fn to_bytes_preserving_compatibility(&self) -> Result<Vec<u8>> {
    self.to_bytes_with_options(SaveOptions::preserving_compatibility())
  }

  pub fn to_bytes_with_options(&self, options: SaveOptions) -> Result<Vec<u8>> {
    self.to_compound_file_with_options(options)?.to_bytes()
  }

  pub fn write_to(&self, writer: impl Write) -> Result<()> {
    self.write_to_with_options(writer, SaveOptions::default())
  }

  pub fn write_to_preserving_compatibility(&self, writer: impl Write) -> Result<()> {
    self.write_to_with_options(writer, SaveOptions::preserving_compatibility())
  }

  pub fn write_to_with_options(&self, writer: impl Write, options: SaveOptions) -> Result<()> {
    self
      .to_compound_file_with_options(options)?
      .write_to(writer)
  }

  pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
    self.save_with_options(path, SaveOptions::default())
  }

  pub fn save_preserving_compatibility(&self, path: impl AsRef<Path>) -> Result<()> {
    self.save_with_options(path, SaveOptions::preserving_compatibility())
  }

  pub fn save_with_options(&self, path: impl AsRef<Path>, options: SaveOptions) -> Result<()> {
    self.write_to_with_options(std::io::sink(), options)?;
    self.write_to_with_options(File::create(path)?, options)
  }
}

fn contains_ograph_compatibility(data: &OgraphRecordData) -> bool {
  match data {
    OgraphRecordData::Compatibility { .. }
    | OgraphRecordData::Label(OgraphLabelRecord {
      legacy_record_id: true,
      ..
    }) => true,
    OgraphRecordData::FrtWrapper(value) => contains_ograph_compatibility(&value.wrapped),
    _ => false,
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OgraphGroupMarker {
  Begin,
  End,
}

fn ograph_group_marker(data: &OgraphRecordData) -> Option<OgraphGroupMarker> {
  match data {
    OgraphRecordData::Common(BiffRecordData::Empty {
      kind: EmptyRecordKind::ChartBegin,
      ..
    }) => Some(OgraphGroupMarker::Begin),
    OgraphRecordData::Common(BiffRecordData::Empty {
      kind: EmptyRecordKind::ChartEnd,
      ..
    }) => Some(OgraphGroupMarker::End),
    OgraphRecordData::FrtWrapper(value) => ograph_group_marker(&value.wrapped),
    _ => None,
  }
}

impl OgraphRecordGroupTree {
  pub fn from_workbook(workbook: &OgraphWorkbookStream) -> Result<Self> {
    let mut tree = Self::default();
    let mut stack = Vec::<usize>::new();
    for (index, record) in workbook.records.iter().enumerate() {
      match ograph_group_marker(&record.data) {
        Some(OgraphGroupMarker::Begin) => {
          if stack.len() >= 100 {
            return Err(Error::Limit(
              "MS-OGRAPH Begin/End nesting exceeds 100 levels".into(),
            ));
          }
          let header_index = index.checked_sub(1).ok_or_else(|| {
            Error::invalid(
              u64::from(record.offset),
              "MS-OGRAPH Begin has no group header",
            )
          })?;
          if ograph_group_marker(&workbook.records[header_index].data).is_some() {
            return Err(Error::invalid(
              u64::from(record.offset),
              "MS-OGRAPH Begin does not follow a group header record",
            ));
          }
          let parent = stack.last().copied();
          let group_index = tree.groups.len();
          tree.groups.push(OgraphRecordGroup {
            header_index,
            end_index: usize::MAX,
            parent,
            children: Vec::new(),
          });
          if let Some(parent) = parent {
            tree.groups[parent].children.push(group_index);
          } else {
            tree.top_level.push(group_index);
          }
          stack.push(group_index);
        }
        Some(OgraphGroupMarker::End) => {
          let group_index = stack.pop().ok_or_else(|| {
            Error::invalid(
              u64::from(record.offset),
              "MS-OGRAPH End has no matching Begin",
            )
          })?;
          tree.groups[group_index].end_index = index;
        }
        None => {}
      }
    }
    if let Some(group_index) = stack.last() {
      return Err(Error::invalid(
        u64::from(workbook.records[tree.groups[*group_index].header_index].offset),
        "MS-OGRAPH Begin has no matching End",
      ));
    }
    Ok(tree)
  }
}

fn logical_record_data(data: &OgraphRecordData) -> &OgraphRecordData {
  match data {
    OgraphRecordData::FrtWrapper(value) => logical_record_data(&value.wrapped),
    _ => data,
  }
}

fn direct_group_record_indices(tree: &OgraphRecordGroupTree, group_index: usize) -> Vec<usize> {
  let group = &tree.groups[group_index];
  let mut children = group
    .children
    .iter()
    .map(|child| {
      let child = &tree.groups[*child];
      (child.header_index, child.end_index)
    })
    .collect::<Vec<_>>();
  children.sort_unstable();
  let mut records = Vec::new();
  let mut index = group.header_index + 2;
  let mut child_index = 0usize;
  while index < group.end_index {
    if let Some((start, end)) = children.get(child_index).copied()
      && index == start
    {
      index = end + 1;
      child_index += 1;
      continue;
    }
    records.push(index);
    index += 1;
  }
  records
}

fn validate_line_format(value: ChartLineFormatRecord, offset: u32) -> Result<()> {
  if value.line_style > 8 {
    return Err(Error::invalid(
      u64::from(offset),
      "MS-OGRAPH LineFormat.lns must be in 0..=8",
    ));
  }
  if !(-1..=2).contains(&value.weight) {
    return Err(Error::invalid(
      u64::from(offset),
      "MS-OGRAPH LineFormat.we must be hairline, narrow, medium, or wide",
    ));
  }
  if value.line_style == 5 && (value.weight != 0 || value.color_index != 0x004d) {
    return Err(Error::invalid(
      u64::from(offset),
      "MS-OGRAPH LineFormat with lns=None must use narrow weight and automatic color",
    ));
  }
  Ok(())
}

fn validate_area_format(value: ChartAreaFormatRecord, offset: u32) -> Result<()> {
  if value.fill_pattern > 0x0012 {
    return Err(Error::invalid(
      u64::from(offset),
      "MS-OGRAPH AreaFormat.fls must be in 0x0000..=0x0012",
    ));
  }
  Ok(())
}

fn ograph_data_format(
  workbook: &OgraphWorkbookStream,
  tree: &OgraphRecordGroupTree,
  group_index: usize,
) -> Result<OgraphDataFormat> {
  let header_index = tree.groups[group_index].header_index;
  let OgraphRecordData::Common(BiffRecordData::ChartDataFormat(source)) =
    logical_record_data(&workbook.records[header_index].data)
  else {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH data-format group does not start with DataFormat",
    ));
  };
  let mut result = OgraphDataFormat {
    source: *source,
    area: None,
    line: None,
    marker: None,
    pie: None,
    bar_shape: None,
    series: None,
    attached_label: None,
  };
  for record_index in direct_group_record_indices(tree, group_index) {
    let record = &workbook.records[record_index];
    macro_rules! set_once {
      ($field:ident, $value:expr, $name:literal) => {
        if result.$field.replace($value).is_some() {
          return Err(Error::invalid(
            u64::from(record.offset),
            concat!(
              "MS-OGRAPH DataFormat contains duplicate ",
              $name,
              " records"
            ),
          ));
        }
      };
    }
    match logical_record_data(&record.data) {
      OgraphRecordData::Common(BiffRecordData::ChartAreaFormat(value)) => {
        validate_area_format(*value, record.offset)?;
        set_once!(area, *value, "AreaFormat");
      }
      OgraphRecordData::Common(BiffRecordData::ChartLineFormat(value)) => {
        validate_line_format(*value, record.offset)?;
        set_once!(line, *value, "LineFormat");
      }
      OgraphRecordData::Common(BiffRecordData::ChartMarkerFormat(value)) => {
        set_once!(marker, *value, "MarkerFormat");
      }
      OgraphRecordData::Common(BiffRecordData::ChartPieFormat(value)) => {
        set_once!(pie, *value, "PieFormat");
      }
      OgraphRecordData::Common(BiffRecordData::Chart3DBarShape(value)) => {
        set_once!(bar_shape, *value, "Chart3DBarShape");
      }
      OgraphRecordData::Common(BiffRecordData::ChartSeriesFormat(value)) => {
        set_once!(series, *value, "SeriesFormat");
      }
      OgraphRecordData::Common(BiffRecordData::ChartAttachedLabel(value)) => {
        set_once!(attached_label, *value, "AttachedLabel");
      }
      _ => {}
    }
  }
  Ok(result)
}

fn ograph_frame(
  workbook: &OgraphWorkbookStream,
  tree: &OgraphRecordGroupTree,
  group_index: usize,
) -> Result<OgraphFrame> {
  let header_index = tree.groups[group_index].header_index;
  let OgraphRecordData::Common(BiffRecordData::ChartFrame(source)) =
    logical_record_data(&workbook.records[header_index].data)
  else {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH frame group does not start with Frame",
    ));
  };
  let mut result = OgraphFrame {
    source: *source,
    line: None,
    area: None,
  };
  if !matches!(source.border_type, 0 | 4) {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH Frame.frt must be plain or shadowed",
    ));
  }
  for record_index in direct_group_record_indices(tree, group_index) {
    let record = &workbook.records[record_index];
    match logical_record_data(&record.data) {
      OgraphRecordData::Common(BiffRecordData::ChartLineFormat(value)) => {
        validate_line_format(*value, record.offset)?;
        if result.line.replace(*value).is_some() {
          return Err(Error::invalid(
            u64::from(record.offset),
            "MS-OGRAPH Frame contains duplicate LineFormat records",
          ));
        }
      }
      OgraphRecordData::Common(BiffRecordData::ChartAreaFormat(value)) => {
        validate_area_format(*value, record.offset)?;
        if result.area.replace(*value).is_some() {
          return Err(Error::invalid(
            u64::from(record.offset),
            "MS-OGRAPH Frame contains duplicate AreaFormat records",
          ));
        }
      }
      _ => {}
    }
  }
  Ok(result)
}

fn ograph_legend(
  workbook: &OgraphWorkbookStream,
  tree: &OgraphRecordGroupTree,
  group_index: usize,
) -> Result<OgraphLegend> {
  let header_index = tree.groups[group_index].header_index;
  let OgraphRecordData::Common(BiffRecordData::ChartLegend(source)) =
    logical_record_data(&workbook.records[header_index].data)
  else {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH legend group does not start with Legend",
    ));
  };
  if source.spacing != 1 {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH Legend.wSpace must equal one",
    ));
  }
  if !source
    .flags
    .contains(crate::xls::ChartLegendFlags::AUTO_SERIES)
  {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH Legend reserved1 bit must be set",
    ));
  }
  if source
    .flags
    .contains(crate::xls::ChartLegendFlags::AUTO_POSITION)
    && !source
      .flags
      .contains(crate::xls::ChartLegendFlags::AUTO_X | crate::xls::ChartLegendFlags::AUTO_Y)
  {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH automatic Legend must use automatic X and Y positioning",
    ));
  }
  if source
    .flags
    .contains(crate::xls::ChartLegendFlags::DATA_TABLE)
    && !source
      .flags
      .contains(crate::xls::ChartLegendFlags::VERTICAL)
  {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH data-table Legend must use a vertical entry layout",
    ));
  }
  let mut position = None;
  for record_index in direct_group_record_indices(tree, group_index) {
    let record = &workbook.records[record_index];
    if let OgraphRecordData::Common(BiffRecordData::ChartPosition(value)) =
      logical_record_data(&record.data)
      && position
        .replace(OgraphPosition::from_source(*value))
        .is_some()
    {
      return Err(Error::invalid(
        u64::from(record.offset),
        "MS-OGRAPH Legend contains duplicate Pos records",
      ));
    }
  }
  let mut frame_groups = tree.groups[group_index]
    .children
    .iter()
    .copied()
    .filter(|child_index| {
      matches!(
        logical_record_data(&workbook.records[tree.groups[*child_index].header_index].data),
        OgraphRecordData::Common(BiffRecordData::ChartFrame(_))
      )
    });
  let frame = frame_groups
    .next()
    .map(|child_index| ograph_frame(workbook, tree, child_index))
    .transpose()?;
  if frame_groups.next().is_some() {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH Legend contains duplicate Frame groups",
    ));
  }
  let position = position.ok_or_else(|| {
    Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH Legend must contain one Pos record",
    )
  })?;
  if !matches!(
    (
      position.source.top_left_mode,
      position.source.bottom_right_mode
    ),
    (5, 1) | (5, 2) | (3, 2)
  ) {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH Legend Pos mode combination is invalid",
    ));
  }
  Ok(OgraphLegend {
    source: *source,
    position,
    frame,
  })
}

fn ograph_axis(
  workbook: &OgraphWorkbookStream,
  tree: &OgraphRecordGroupTree,
  group_index: usize,
  axis_group: u16,
) -> Result<OgraphAxis> {
  let header_index = tree.groups[group_index].header_index;
  let OgraphRecordData::Common(BiffRecordData::ChartAxis(source)) =
    logical_record_data(&workbook.records[header_index].data)
  else {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH axis group does not start with Axis",
    ));
  };
  if source.axis_type > 2 {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH Axis.wType must be 0, 1, or 2",
    ));
  }

  let mut result = OgraphAxis {
    axis_group,
    source: *source,
    value_range: None,
    label_range: None,
    options: None,
    tick: None,
    line_formats: [None; 4],
    area_formats: [None; 4],
  };
  let direct = direct_group_record_indices(tree, group_index);
  let mut index = 0usize;
  while index < direct.len() {
    let record_index = direct[index];
    let record = &workbook.records[record_index];
    macro_rules! set_once {
      ($field:ident, $value:expr, $name:literal) => {
        if result.$field.replace($value).is_some() {
          return Err(Error::invalid(
            u64::from(record.offset),
            concat!("MS-OGRAPH Axis contains duplicate ", $name, " records"),
          ));
        }
      };
    }
    match logical_record_data(&record.data) {
      OgraphRecordData::Common(BiffRecordData::ChartValueRange(value)) => {
        set_once!(value_range, *value, "ValueRange");
      }
      OgraphRecordData::Common(BiffRecordData::ChartLabelRange(value)) => {
        set_once!(label_range, *value, "LabelRange");
      }
      OgraphRecordData::Common(BiffRecordData::ChartAxisOptions(value)) => {
        set_once!(options, *value, "AxcExt");
      }
      OgraphRecordData::Common(BiffRecordData::ChartTick(value)) => {
        set_once!(tick, *value, "Tick");
      }
      OgraphRecordData::Common(BiffRecordData::ChartAxisLine(value)) => {
        let line_kind = usize::from(value.line_kind);
        if line_kind >= result.line_formats.len() {
          return Err(Error::invalid(
            u64::from(record.offset),
            "MS-OGRAPH AxisLine.id must be in 0..=3",
          ));
        }
        let Some(next_record_index) = direct.get(index + 1).copied() else {
          return Err(Error::invalid(
            u64::from(record.offset),
            "MS-OGRAPH AxisLine is not followed by LineFormat",
          ));
        };
        let OgraphRecordData::Common(BiffRecordData::ChartLineFormat(line_format)) =
          logical_record_data(&workbook.records[next_record_index].data)
        else {
          return Err(Error::invalid(
            u64::from(workbook.records[next_record_index].offset),
            "MS-OGRAPH AxisLine is not followed by LineFormat",
          ));
        };
        validate_line_format(*line_format, workbook.records[next_record_index].offset)?;
        if result.line_formats[line_kind]
          .replace(*line_format)
          .is_some()
        {
          return Err(Error::invalid(
            u64::from(record.offset),
            "MS-OGRAPH AxisLine.id is duplicated",
          ));
        }
        index += 1;
        if let Some(area_record_index) = direct.get(index + 1).copied()
          && let OgraphRecordData::Common(BiffRecordData::ChartAreaFormat(area_format)) =
            logical_record_data(&workbook.records[area_record_index].data)
        {
          validate_area_format(*area_format, workbook.records[area_record_index].offset)?;
          result.area_formats[line_kind] = Some(*area_format);
          index += 1;
        }
      }
      _ => {}
    }
    index += 1;
  }
  if result.tick.is_some_and(|tick| {
    tick.major_tick_type > 3
      || tick.minor_tick_type > 3
      || tick.label_position > 3
      || !(1..=2).contains(&tick.background_mode)
  }) {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH Tick contains an out-of-range tick or label position",
    ));
  }
  if result.value_range.is_some() == result.label_range.is_some() {
    return Err(Error::invalid(
      u64::from(workbook.records[header_index].offset),
      "MS-OGRAPH Axis must contain exactly one ValueRange or LabelRange",
    ));
  }
  Ok(result)
}

fn biff_string_text(characters: &XlStringCharacters) -> String {
  match characters {
    XlStringCharacters::Compressed(bytes) => bytes
      .iter()
      .map(|byte| char::from_u32(u32::from(*byte)).unwrap_or('\u{fffd}'))
      .collect(),
    XlStringCharacters::Unicode(units) => String::from_utf16_lossy(units),
  }
}

fn reference_values(
  reference: OgraphBraiRecord,
  orient: OgraphOrientRecord,
  dimensions: OgraphDimensionsRecord,
  included_rows: &OgraphExclusionRecord,
  included_columns: &OgraphExclusionRecord,
  cells: &BTreeMap<(u16, u16), OgraphCell>,
) -> Result<Vec<Option<OgraphCellValue>>> {
  if reference.reference_type == 0 {
    return Ok(Vec::new());
  }
  let mut values = Vec::new();
  if orient.series_are_rows() {
    if reference.row_or_column > dimensions.non_empty_row_count {
      return Err(Error::invalid(
        0,
        "MS-OGRAPH BRAI row does not fit Dimensions.colMac",
      ));
    }
    let column_count = u16::try_from(dimensions.longest_row_cell_count)
      .map_err(|_| Error::Limit("MS-OGRAPH Dimensions.rwMac exceeds u16".into()))?;
    for column in 1..=column_count {
      if included_columns.includes(column) {
        values.push(
          cells
            .get(&(reference.row_or_column, column))
            .map(|cell| cell.value.clone()),
        );
      }
    }
  } else {
    if u32::from(reference.row_or_column) > dimensions.longest_row_cell_count {
      return Err(Error::invalid(
        0,
        "MS-OGRAPH BRAI column does not fit Dimensions.rwMac",
      ));
    }
    for row in 1..=dimensions.non_empty_row_count {
      if included_rows.includes(row) {
        values.push(
          cells
            .get(&(row, reference.row_or_column))
            .map(|cell| cell.value.clone()),
        );
      }
    }
  }
  Ok(values)
}

fn reference_first_cell<'a>(
  reference: OgraphBraiRecord,
  orient: OgraphOrientRecord,
  cells: &'a BTreeMap<(u16, u16), OgraphCell>,
) -> Option<&'a OgraphCellValue> {
  if reference.reference_type == 0 {
    return None;
  }
  let coordinate = if orient.series_are_rows() {
    (reference.row_or_column, 0)
  } else {
    (0, reference.row_or_column)
  };
  cells.get(&coordinate).map(|cell| &cell.value)
}

fn series_name(value: Option<&OgraphCellValue>, cached: Option<String>) -> String {
  match value {
    Some(OgraphCellValue::Text(value)) => value.clone(),
    Some(OgraphCellValue::Number(value)) => value.to_string(),
    Some(OgraphCellValue::Blank) | None => cached.unwrap_or_default(),
  }
}

impl OgraphWorkbookStream {
  /// Projects the native Graph record tree into chart/data relationships used
  /// by renderers. The record tree remains the write authority; this view is
  /// rebuilt from BRAI, Orient, exclusion, Series, SerToCrt, and ChartFormat
  /// records and never replaces their lossless representation.
  pub fn chart(&self) -> Result<OgraphChart> {
    self.validate(false)?;
    let tree = self.groups()?;
    let mut cells_by_coordinate = BTreeMap::new();
    let mut dimensions = None;
    let mut orient = None;
    let mut included_rows = None;
    let mut included_columns = None;
    let mut chart_source = None;
    let mut date_1904 = false;
    let mut palette = Vec::new();
    for record in &self.records {
      match logical_record_data(&record.data) {
        OgraphRecordData::Blank(value) => {
          let cell = OgraphCell {
            row: value.cell.row,
            column: value.cell.column,
            format_index: value.cell.format_index,
            value: OgraphCellValue::Blank,
          };
          if cells_by_coordinate
            .insert((cell.row, cell.column), cell)
            .is_some()
          {
            return Err(Error::invalid(
              u64::from(record.offset),
              "MS-OGRAPH datasheet contains a duplicate cell coordinate",
            ));
          }
        }
        OgraphRecordData::Number(value) => {
          let cell = OgraphCell {
            row: value.cell.row,
            column: value.cell.column,
            format_index: value.cell.format_index,
            value: OgraphCellValue::Number(f64::from_bits(value.value_bits)),
          };
          if cells_by_coordinate
            .insert((cell.row, cell.column), cell)
            .is_some()
          {
            return Err(Error::invalid(
              u64::from(record.offset),
              "MS-OGRAPH datasheet contains a duplicate cell coordinate",
            ));
          }
        }
        OgraphRecordData::Label(value) => {
          let cell = OgraphCell {
            row: value.cell.row,
            column: value.cell.column,
            format_index: value.cell.format_index,
            value: OgraphCellValue::Text(value.text.text()),
          };
          if cells_by_coordinate
            .insert((cell.row, cell.column), cell)
            .is_some()
          {
            return Err(Error::invalid(
              u64::from(record.offset),
              "MS-OGRAPH datasheet contains a duplicate cell coordinate",
            ));
          }
        }
        OgraphRecordData::Dimensions(value) => dimensions = Some(*value),
        OgraphRecordData::Common(BiffRecordData::Chart(value)) => chart_source = Some(*value),
        OgraphRecordData::Orient(value) => orient = Some(*value),
        OgraphRecordData::ExcludeRows(value) => included_rows = Some(value.clone()),
        OgraphRecordData::ExcludeColumns(value) => included_columns = Some(value.clone()),
        OgraphRecordData::Common(BiffRecordData::FixedU16 {
          kind: FixedU16RecordKind::Date1904,
          value,
        }) => date_1904 = *value != 0,
        OgraphRecordData::Common(BiffRecordData::Palette(value)) => {
          palette = value.colors.clone();
        }
        _ => {}
      }
    }
    let dimensions = dimensions.ok_or_else(|| Error::invalid(0, "missing Dimensions"))?;
    let chart_source = chart_source.ok_or_else(|| Error::invalid(0, "missing Chart"))?;
    if chart_source.x != 0
      || chart_source.y != 0
      || chart_source.width < 0
      || chart_source.height < 0
    {
      return Err(Error::invalid(
        0,
        "MS-OGRAPH Chart must start at zero and have nonnegative 16.16 dimensions",
      ));
    }
    let orient = orient.ok_or_else(|| Error::invalid(0, "missing Orient"))?;
    let included_rows = included_rows.ok_or_else(|| Error::invalid(0, "missing ExcludeRows"))?;
    let included_columns =
      included_columns.ok_or_else(|| Error::invalid(0, "missing ExcludeColumns"))?;

    let mut format_groups = tree
      .groups
      .iter()
      .enumerate()
      .filter_map(|(group_index, group)| {
        match logical_record_data(&self.records[group.header_index].data) {
          OgraphRecordData::Common(BiffRecordData::ChartFormat(value)) => {
            Some((group_index, *value))
          }
          _ => None,
        }
      })
      .collect::<Vec<_>>();
    format_groups.sort_by_key(|(group_index, _)| tree.groups[*group_index].header_index);
    let mut groups = Vec::with_capacity(format_groups.len());
    for (index, (tree_group_index, format)) in format_groups.into_iter().enumerate() {
      let direct = direct_group_record_indices(&tree, tree_group_index);
      let mut kinds = direct
        .iter()
        .filter_map(
          |record_index| match logical_record_data(&self.records[*record_index].data) {
            OgraphRecordData::Common(BiffRecordData::ChartBar(value)) => {
              Some(OgraphChartGroupKind::Bar(*value))
            }
            OgraphRecordData::Common(BiffRecordData::ChartLine(value)) => {
              Some(OgraphChartGroupKind::Line(*value))
            }
            OgraphRecordData::Common(BiffRecordData::ChartPie(value)) => {
              Some(OgraphChartGroupKind::Pie(*value))
            }
            OgraphRecordData::Common(BiffRecordData::ChartArea(value)) => {
              Some(OgraphChartGroupKind::Area(*value))
            }
            OgraphRecordData::Common(BiffRecordData::ChartScatter(value)) => {
              Some(OgraphChartGroupKind::Scatter(*value))
            }
            OgraphRecordData::Common(BiffRecordData::ChartRadar(value)) => {
              Some(OgraphChartGroupKind::Radar(*value))
            }
            OgraphRecordData::Common(BiffRecordData::ChartRadarArea(value)) => {
              Some(OgraphChartGroupKind::FilledRadar(*value))
            }
            OgraphRecordData::Common(BiffRecordData::ChartSurf(value)) => {
              Some(OgraphChartGroupKind::Surface(*value))
            }
            OgraphRecordData::Common(BiffRecordData::ChartBopPop(value)) => {
              Some(OgraphChartGroupKind::BopPop(*value))
            }
            _ => None,
          },
        )
        .collect::<Vec<_>>();
      if kinds.len() != 1 {
        return Err(Error::invalid(
          u64::from(self.records[tree.groups[tree_group_index].header_index].offset),
          "MS-OGRAPH ChartFormat must contain exactly one chart-group type record",
        ));
      }
      let mut axis_group = None;
      let mut parent = tree.groups[tree_group_index].parent;
      while let Some(parent_index) = parent {
        if let OgraphRecordData::Common(BiffRecordData::ChartAxisParent(value)) =
          logical_record_data(&self.records[tree.groups[parent_index].header_index].data)
        {
          axis_group = Some(value.axis_group);
          break;
        }
        parent = tree.groups[parent_index].parent;
      }
      let range =
        tree.groups[tree_group_index].header_index..=tree.groups[tree_group_index].end_index;
      let view_3d = range.clone().find_map(|record_index| {
        match logical_record_data(&self.records[record_index].data) {
          OgraphRecordData::Common(BiffRecordData::Chart3D(value)) => Some(*value),
          _ => None,
        }
      });
      let mut legend_groups = tree
        .groups
        .iter()
        .enumerate()
        .filter_map(|(group_index, group)| {
          (range.contains(&group.header_index)
            && matches!(
              logical_record_data(&self.records[group.header_index].data),
              OgraphRecordData::Common(BiffRecordData::ChartLegend(_))
            ))
          .then_some(group_index)
        });
      let legend = legend_groups
        .next()
        .map(|group_index| ograph_legend(self, &tree, group_index))
        .transpose()?;
      if legend_groups.next().is_some() {
        return Err(Error::invalid(
          u64::from(self.records[tree.groups[tree_group_index].header_index].offset),
          "MS-OGRAPH ChartFormat contains duplicate Legend groups",
        ));
      }
      let bop_pop_custom = range.clone().find_map(|record_index| {
        match logical_record_data(&self.records[record_index].data) {
          OgraphRecordData::Common(BiffRecordData::ChartBopPopCustom(value)) => Some(value.clone()),
          _ => None,
        }
      });
      let kind = kinds.pop().unwrap();
      if let OgraphChartGroupKind::BopPop(value) = &kind {
        let requires_custom = value.automatic_split == 0 && value.split_kind == 3;
        if requires_custom != bop_pop_custom.is_some() {
          return Err(Error::invalid(
            u64::from(self.records[tree.groups[tree_group_index].header_index].offset),
            "MS-OGRAPH BopPopCustom presence does not match the BopPop split mode",
          ));
        }
      }
      groups.push(OgraphChartGroup {
        index,
        axis_group: axis_group
          .ok_or_else(|| Error::invalid(0, "MS-OGRAPH ChartFormat has no containing AxisParent"))?,
        format,
        kind,
        view_3d,
        legend,
        bop_pop_custom,
      });
    }
    if groups.is_empty() {
      return Err(Error::invalid(
        0,
        "MS-OGRAPH chart has no ChartFormat group",
      ));
    }

    let mut axis_tree_groups = tree
      .groups
      .iter()
      .enumerate()
      .filter_map(|(group_index, group)| {
        matches!(
          logical_record_data(&self.records[group.header_index].data),
          OgraphRecordData::Common(BiffRecordData::ChartAxis(_))
        )
        .then_some(group_index)
      })
      .collect::<Vec<_>>();
    axis_tree_groups.sort_by_key(|group_index| tree.groups[*group_index].header_index);
    let mut axes = Vec::with_capacity(axis_tree_groups.len());
    let mut next_axis_type_by_group = BTreeMap::<u16, u16>::new();
    for tree_group_index in axis_tree_groups {
      let mut parent = tree.groups[tree_group_index].parent;
      let axis_group = loop {
        let Some(parent_index) = parent else {
          return Err(Error::invalid(
            u64::from(self.records[tree.groups[tree_group_index].header_index].offset),
            "MS-OGRAPH Axis has no containing AxisParent",
          ));
        };
        if let OgraphRecordData::Common(BiffRecordData::ChartAxisParent(value)) =
          logical_record_data(&self.records[tree.groups[parent_index].header_index].data)
        {
          break value.axis_group;
        }
        parent = tree.groups[parent_index].parent;
      };
      let axis = ograph_axis(self, &tree, tree_group_index, axis_group)?;
      let expected_axis_type = next_axis_type_by_group.entry(axis_group).or_default();
      if axis.source.axis_type != *expected_axis_type {
        return Err(Error::invalid(
          u64::from(self.records[tree.groups[tree_group_index].header_index].offset),
          "MS-OGRAPH Axis.wType does not match its order in AxisParent",
        ));
      }
      *expected_axis_type += 1;
      axes.push(axis);
    }
    for group in &groups {
      let axes_for_group = axes
        .iter()
        .filter(|axis| axis.axis_group == group.axis_group)
        .collect::<Vec<_>>();
      let axisless = matches!(
        group.kind,
        OgraphChartGroupKind::Pie(_) | OgraphChartGroupKind::BopPop(_)
      );
      if axisless {
        if !axes_for_group.is_empty() {
          return Err(Error::invalid(
            0,
            "MS-OGRAPH pie, doughnut, pie-of-pie, or bar-of-pie group must not contain axes",
          ));
        }
        continue;
      }
      if axes_for_group.len() < 2 {
        return Err(Error::invalid(
          0,
          "MS-OGRAPH axis-based chart group must contain horizontal/category and value axes",
        ));
      }
      let scatter_or_bubble = matches!(group.kind, OgraphChartGroupKind::Scatter(_));
      for axis in axes_for_group {
        let requires_value_range =
          axis.source.axis_type == 1 || (axis.source.axis_type == 0 && scatter_or_bubble);
        if requires_value_range != axis.value_range.is_some() {
          return Err(Error::invalid(
            0,
            "MS-OGRAPH Axis range record does not match its chart-group role",
          ));
        }
      }
    }

    let mut series_groups = tree
      .groups
      .iter()
      .enumerate()
      .filter_map(|(group_index, group)| {
        match logical_record_data(&self.records[group.header_index].data) {
          OgraphRecordData::Common(BiffRecordData::ChartSeries(value)) => {
            Some((group_index, *value))
          }
          _ => None,
        }
      })
      .collect::<Vec<_>>();
    series_groups.sort_by_key(|(group_index, _)| tree.groups[*group_index].header_index);
    let mut series = Vec::with_capacity(series_groups.len());
    let mut series_per_group = BTreeMap::<usize, usize>::new();
    for (index, (tree_group_index, source)) in series_groups.into_iter().enumerate() {
      let direct = direct_group_record_indices(&tree, tree_group_index);
      let mut references = [None; 4];
      let mut group_index = None;
      let mut cached_name = None;
      for record_index in direct {
        match logical_record_data(&self.records[record_index].data) {
          OgraphRecordData::Brai(value) => {
            let slot = &mut references[usize::from(value.data_role)];
            if slot.replace(*value).is_some() {
              return Err(Error::invalid(
                u64::from(self.records[record_index].offset),
                "MS-OGRAPH Series contains duplicate BRAI roles",
              ));
            }
          }
          OgraphRecordData::Common(BiffRecordData::ChartSeriesGroupIndex(value)) => {
            group_index = Some(usize::from(value.chart_group_index));
          }
          OgraphRecordData::Common(BiffRecordData::ChartSeriesText(value)) => {
            cached_name = Some(biff_string_text(&value.text.characters));
          }
          _ => {}
        }
      }
      let Some(group_index) = group_index else {
        // Trendline and error-bar Series groups use SerParent rather than
        // SerToCrt. They remain fully represented in the native record tree
        // but are not primary plotted series in this projection.
        continue;
      };
      let [
        Some(name_reference),
        Some(value_reference),
        Some(category_reference),
        Some(bubble_reference),
      ] = references
      else {
        return Err(Error::invalid(
          u64::from(self.records[tree.groups[tree_group_index].header_index].offset),
          "MS-OGRAPH Series must contain one BRAI record for each of the four roles",
        ));
      };
      let group = groups
        .get(group_index)
        .ok_or_else(|| Error::invalid(0, "MS-OGRAPH SerToCrt references a missing ChartFormat"))?;
      let mut categories = if category_reference.reference_type == 0 {
        (1..=source.category_count)
          .map(|index| Some(OgraphCellValue::Number(f64::from(index))))
          .collect::<Vec<_>>()
      } else {
        reference_values(
          category_reference,
          orient,
          dimensions,
          &included_rows,
          &included_columns,
          &cells_by_coordinate,
        )?
      };
      categories.truncate(usize::from(source.category_count));
      let raw_values = reference_values(
        value_reference,
        orient,
        dimensions,
        &included_rows,
        &included_columns,
        &cells_by_coordinate,
      )?;
      let values = raw_values
        .into_iter()
        .take(usize::from(source.value_count))
        .map(|value| match value {
          Some(OgraphCellValue::Number(value)) => Some(value),
          _ => None,
        })
        .collect::<Vec<_>>();
      let bubble_sizes = reference_values(
        bubble_reference,
        orient,
        dimensions,
        &included_rows,
        &included_columns,
        &cells_by_coordinate,
      )?
      .into_iter()
      .take(usize::from(source.bubble_count))
      .map(|value| match value {
        Some(OgraphCellValue::Number(value)) => Some(value),
        _ => None,
      })
      .collect::<Vec<_>>();
      let fixed_dimension_included = if orient.series_are_rows() {
        included_rows.includes(value_reference.row_or_column)
      } else {
        included_columns.includes(value_reference.row_or_column)
      };
      let scatter_placeholder = orient.horizontal_series_column > 0
        && usize::from(orient.horizontal_series_column) == index + 1;
      let pie_single_series = matches!(
        &group.kind,
        OgraphChartGroupKind::Pie(ChartPieRecord {
          doughnut_hole_percent: 0,
          ..
        }) | OgraphChartGroupKind::BopPop(_)
      );
      let previous_in_group = *series_per_group.get(&group_index).unwrap_or(&0);
      let included = fixed_dimension_included
        && !scatter_placeholder
        && (!pie_single_series || previous_in_group == 0);
      *series_per_group.entry(group_index).or_default() += 1;
      let mut data_format_groups = tree.groups[tree_group_index]
        .children
        .iter()
        .copied()
        .filter(|child_index| {
          matches!(
            logical_record_data(&self.records[tree.groups[*child_index].header_index].data),
            OgraphRecordData::Common(BiffRecordData::ChartDataFormat(_))
          )
        })
        .collect::<Vec<_>>();
      data_format_groups.sort_by_key(|child_index| tree.groups[*child_index].header_index);
      let data_formats = data_format_groups
        .into_iter()
        .map(|child_index| ograph_data_format(self, &tree, child_index))
        .collect::<Result<Vec<_>>>()?;
      series.push(OgraphSeries {
        index,
        group_index,
        included,
        name: series_name(
          reference_first_cell(name_reference, orient, &cells_by_coordinate),
          cached_name,
        ),
        categories,
        values,
        bubble_sizes,
        source,
        name_reference,
        value_reference,
        category_reference,
        bubble_reference,
        data_formats,
      });
    }

    Ok(OgraphChart {
      source: chart_source,
      date_1904,
      cells: cells_by_coordinate.into_values().collect(),
      orient,
      excluded_rows: included_rows,
      excluded_columns: included_columns,
      groups,
      axes,
      series,
      palette,
    })
  }
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
  bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_i16(bytes: &mut Vec<u8>, value: i16) {
  bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
  bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
  bytes.extend_from_slice(&value.to_le_bytes());
}

fn encode_cell_header(bytes: &mut Vec<u8>, cell: OgraphCellHeader) {
  push_u16(bytes, cell.row);
  push_u16(bytes, cell.column);
  bytes.push(cell.reserved);
  push_u16(bytes, cell.format_index);
}

fn encode_ograph_record(
  data: &OgraphRecordData,
  options: SaveOptions,
) -> Result<Vec<(u16, Vec<u8>)>> {
  let one = |record_type, payload| Ok(vec![(record_type, payload)]);
  match data {
    OgraphRecordData::Common(value) => {
      let encoded = value.encode_ograph_common()?;
      for (record_type, payload) in &encoded {
        if !is_ograph_common_record(*record_type, payload) {
          return Err(Error::invalid(
            0,
            format!(
              "BIFF record 0x{record_type:04x} is not valid in an MS-OGRAPH common-record node"
            ),
          ));
        }
      }
      Ok(encoded)
    }
    OgraphRecordData::FrtWrapper(value) => {
      if value.header.record_type != GRAPH_FRT_WRAPPER || !value.header.flags.is_empty() {
        return Err(Error::invalid(
          0,
          "MS-OGRAPH FrtWrapper future-record header is invalid",
        ));
      }
      let mut wrapped = encode_ograph_record(&value.wrapped, options)?;
      if wrapped.len() != 1 || !is_ograph_wrapped_record(wrapped[0].0) {
        return Err(Error::invalid(
          0,
          "MS-OGRAPH FrtWrapper must contain exactly one permitted logical record",
        ));
      }
      let (record_type, wrapped_payload) = wrapped.pop().unwrap();
      let expected_padding = 8usize.saturating_sub(wrapped_payload.len() + 4);
      if value.padding.len() != expected_padding || value.padding.iter().any(|byte| *byte != 0) {
        return Err(Error::invalid(
          0,
          "MS-OGRAPH FrtWrapper padding does not match the wrapped record size",
        ));
      }
      let mut payload = Vec::with_capacity(8 + wrapped_payload.len() + value.padding.len());
      push_u16(&mut payload, value.header.record_type);
      push_u16(&mut payload, value.header.flags.bits());
      push_u16(&mut payload, record_type);
      push_u16(
        &mut payload,
        u16::try_from(wrapped_payload.len())
          .map_err(|_| Error::Limit("MS-OGRAPH wrapped record length exceeds u16".into()))?,
      );
      payload.extend_from_slice(&wrapped_payload);
      payload.extend_from_slice(&value.padding);
      one(GRAPH_FRT_WRAPPER, payload)
    }
    OgraphRecordData::BofDatasheet(value) => {
      let mut payload = Vec::with_capacity(4);
      push_u16(&mut payload, value.unused1);
      push_u16(&mut payload, value.unused2);
      one(GRAPH_BOF_DATASHEET, payload)
    }
    OgraphRecordData::Blank(value) => {
      let mut payload = Vec::with_capacity(7);
      encode_cell_header(&mut payload, value.cell);
      one(GRAPH_BLANK, payload)
    }
    OgraphRecordData::Number(value) => {
      let mut payload = Vec::with_capacity(15);
      encode_cell_header(&mut payload, value.cell);
      push_u64(&mut payload, value.value_bits);
      one(GRAPH_NUMBER, payload)
    }
    OgraphRecordData::Label(value) => {
      if value.legacy_record_id && !options.preserves_compatibility() {
        return Err(Error::invalid(
          0,
          "strict MS-OGRAPH save rejects historical Label record type 0x0004",
        ));
      }
      let mut payload = Vec::new();
      encode_cell_header(&mut payload, value.cell);
      payload.extend_from_slice(&value.text.to_bytes()?);
      one(
        if value.legacy_record_id {
          GRAPH_LABEL_COMPATIBILITY
        } else {
          GRAPH_LABEL
        },
        payload,
      )
    }
    OgraphRecordData::Dimensions(value) => {
      let mut payload = Vec::with_capacity(14);
      push_u32(&mut payload, value.reserved1);
      push_u32(&mut payload, value.longest_row_cell_count);
      push_u16(&mut payload, value.reserved2);
      push_u16(&mut payload, value.non_empty_row_count);
      push_u16(&mut payload, value.reserved3);
      one(GRAPH_DIMENSIONS, payload)
    }
    OgraphRecordData::ChartColors(value) => {
      let mut payload = Vec::with_capacity(2);
      push_i16(&mut payload, value.color_count);
      one(GRAPH_CHART_COLORS, payload)
    }
    OgraphRecordData::ColumnWidth(value) => {
      let mut payload = Vec::with_capacity(6);
      push_u16(&mut payload, value.first_column);
      push_u16(&mut payload, value.last_column);
      push_u16(&mut payload, value.width);
      one(GRAPH_COLUMN_WIDTH, payload)
    }
    OgraphRecordData::ExcludeRows(value) | OgraphRecordData::ExcludeColumns(value) => {
      let mut payload = Vec::with_capacity(value.transitions.len() * 2);
      for transition in &value.transitions {
        push_u16(&mut payload, *transition);
      }
      one(
        if matches!(data, OgraphRecordData::ExcludeRows(_)) {
          GRAPH_EXCLUDE_ROWS
        } else {
          GRAPH_EXCLUDE_COLUMNS
        },
        payload,
      )
    }
    OgraphRecordData::Orient(value) => {
      let mut payload = Vec::with_capacity(6);
      payload.push(value.series_in_rows);
      push_u16(&mut payload, value.horizontal_series_row);
      push_u16(&mut payload, value.horizontal_series_column);
      payload.push(value.reserved);
      one(GRAPH_ORIENT, payload)
    }
    OgraphRecordData::Brai(value) => {
      let mut payload = Vec::with_capacity(8);
      payload.push(value.data_role);
      payload.push(value.reference_type);
      push_u16(&mut payload, value.flags);
      push_u16(&mut payload, value.format_index);
      push_u16(&mut payload, value.row_or_column);
      one(GRAPH_BRAI, payload)
    }
    OgraphRecordData::WinDoc(value) => one(GRAPH_WIN_DOC, vec![value.chart_selected]),
    OgraphRecordData::MaxStatus(value) => one(GRAPH_MAX_STATUS, vec![value.unused1, value.unused2]),
    OgraphRecordData::MainWindow(value) => {
      let mut payload = Vec::with_capacity(8);
      push_i16(&mut payload, value.left);
      push_i16(&mut payload, value.top);
      push_i16(&mut payload, value.width);
      push_i16(&mut payload, value.height);
      one(GRAPH_MAIN_WINDOW, payload)
    }
    OgraphRecordData::Window1_10(value) => {
      let mut payload = Vec::with_capacity(10);
      push_u16(&mut payload, value.x);
      push_u16(&mut payload, value.y);
      push_u16(&mut payload, value.width);
      push_u16(&mut payload, value.height);
      push_u16(&mut payload, value.reserved);
      one(GRAPH_WINDOW1, payload)
    }
    OgraphRecordData::Window2(value) => {
      let mut payload = Vec::with_capacity(14);
      payload.extend_from_slice(&[
        value.reserved1,
        value.reserved2,
        value.reserved3,
        value.reserved4,
        value.reserved5,
      ]);
      push_u16(&mut payload, value.first_row);
      push_u16(&mut payload, value.first_column);
      payload.push(value.reserved6);
      push_u16(&mut payload, value.reserved7);
      push_u16(&mut payload, value.reserved8);
      one(GRAPH_WINDOW2, payload)
    }
    OgraphRecordData::Selection(value) => {
      let mut payload = Vec::with_capacity(17);
      payload.push(value.pane);
      for item in [
        value.active_row,
        value.active_column,
        value.reserved,
        value.unused,
        value.first_row,
        value.last_row,
        value.first_column,
        value.last_column,
      ] {
        push_u16(&mut payload, item);
      }
      one(GRAPH_SELECTION, payload)
    }
    OgraphRecordData::LinkedSelection(value) => {
      let mut payload = Vec::with_capacity(8);
      push_u16(&mut payload, value.first_row);
      push_u16(&mut payload, value.last_row);
      push_u16(&mut payload, value.first_column);
      push_u16(&mut payload, value.last_column);
      one(GRAPH_LINKED_SELECTION, payload)
    }
    OgraphRecordData::Compatibility {
      record_type,
      payload,
    } if options.preserves_compatibility() => one(*record_type, payload.clone()),
    OgraphRecordData::Compatibility { .. } => Err(Error::invalid(
      0,
      "strict MS-OGRAPH save rejects an undeclared compatibility record",
    )),
  }
}

fn normalize_ograph_derived_fields(
  data: &mut OgraphRecordData,
  options: SaveOptions,
) -> Result<()> {
  let OgraphRecordData::FrtWrapper(value) = data else {
    return Ok(());
  };
  normalize_ograph_derived_fields(&mut value.wrapped, options)?;
  let wrapped = encode_ograph_record(&value.wrapped, options)?;
  if wrapped.len() != 1 || !is_ograph_wrapped_record(wrapped[0].0) {
    return Err(Error::invalid(
      0,
      "MS-OGRAPH FrtWrapper must contain exactly one permitted logical record",
    ));
  }
  value.padding = vec![0; 8usize.saturating_sub(wrapped[0].1.len() + 4)];
  Ok(())
}

impl OgraphWorkbookStream {
  fn relayout_in_place(&mut self, options: SaveOptions) -> Result<()> {
    for record in &mut self.records {
      normalize_ograph_derived_fields(&mut record.data, options)?;
    }
    let mut offset = 0u64;
    for record in &mut self.records {
      record.offset = u32::try_from(offset)
        .map_err(|_| Error::Limit("MS-OGRAPH record offset exceeds u32".into()))?;
      for (_, payload) in encode_ograph_record(&record.data, options)? {
        let physical = 4u64
          .checked_add(payload.len() as u64)
          .ok_or_else(|| Error::Limit("MS-OGRAPH record size overflow".into()))?;
        offset = offset
          .checked_add(physical)
          .ok_or_else(|| Error::Limit("MS-OGRAPH Workbook size overflow".into()))?;
      }
    }

    let chart_bof_offset = self
      .records
      .iter()
      .filter(|record| {
        matches!(
          &record.data,
          OgraphRecordData::Common(BiffRecordData::Bof(_))
        )
      })
      .nth(1)
      .map(|record| record.offset)
      .ok_or_else(|| Error::invalid(0, "MS-OGRAPH Workbook has no chart-sheet BOF"))?;
    let mut bound_sheet_count = 0usize;
    for record in &mut self.records {
      if let OgraphRecordData::Common(BiffRecordData::BoundSheet8(value)) = &mut record.data {
        value.sheet_bof_offset = chart_bof_offset;
        bound_sheet_count += 1;
      }
    }
    if bound_sheet_count != 1 {
      return Err(Error::invalid(
        0,
        "MS-OGRAPH Workbook must contain exactly one BoundSheet8",
      ));
    }
    if self.trailing_padding.iter().any(|value| *value != 0) {
      return Err(Error::invalid(
        offset,
        "MS-OGRAPH Workbook trailing padding must be zero",
      ));
    }
    self.validate(!options.preserves_compatibility())
  }

  pub fn relayout(&mut self) -> Result<()> {
    let mut rebuilt = self.clone();
    rebuilt.relayout_in_place(SaveOptions::default())?;
    *self = rebuilt;
    Ok(())
  }

  pub fn relayout_preserving_compatibility(&mut self) -> Result<()> {
    let mut rebuilt = self.clone();
    rebuilt.relayout_in_place(SaveOptions::preserving_compatibility())?;
    *self = rebuilt;
    Ok(())
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.to_bytes_with_options(SaveOptions::default())
  }

  pub fn to_bytes_preserving_compatibility(&self) -> Result<Vec<u8>> {
    self.to_bytes_with_options(SaveOptions::preserving_compatibility())
  }

  pub fn to_bytes_with_options(&self, options: SaveOptions) -> Result<Vec<u8>> {
    let mut rebuilt = self.clone();
    rebuilt.relayout_in_place(options)?;
    let mut bytes = Vec::new();
    for record in &rebuilt.records {
      for (record_type, payload) in encode_ograph_record(&record.data, options)? {
        if payload.len() > MAX_BIFF_RECORD_DATA {
          return Err(Error::Limit(format!(
            "MS-OGRAPH record 0x{record_type:04x} exceeds 8224 bytes"
          )));
        }
        push_u16(&mut bytes, record_type);
        push_u16(
          &mut bytes,
          u16::try_from(payload.len())
            .map_err(|_| Error::Limit("MS-OGRAPH record length exceeds u16".into()))?,
        );
        bytes.extend_from_slice(&payload);
      }
    }
    bytes.extend_from_slice(&rebuilt.trailing_padding);
    Ok(bytes)
  }
}
