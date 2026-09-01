//! Shared MS-ODRAW OfficeArt record framing.

use std::{
  collections::BTreeSet,
  io::{Cursor, Read, Seek, Write},
  sync::Arc,
};

use crate::{
  Error, Result, SdkBitfield,
  io::{Reader, SdkRead, SdkSize, SdkWrite, Writer},
  limits::Limits,
};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};

const HEADER_LEN: usize = 8;
const MAX_CONTAINER_DEPTH: usize = 256;
const STANDARD_HYPERLINK_CLASS_ID: [u8; 16] = [
  0xd0, 0xc9, 0xea, 0x79, 0xf9, 0xba, 0xce, 0x11, 0x8c, 0x82, 0x00, 0xaa, 0x00, 0x4b, 0xa9, 0x0b,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtStream {
  pub records: Vec<OfficeArtRecord>,
}

/// MS-ODRAW 2.2.21 headerless delay-loaded sequence used by the PPT
/// Pictures Stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtBStoreDelay {
  pub records: Vec<OfficeArtRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtBStoreDelayLayout {
  pub file_blocks: Vec<OfficeArtBStoreDelayFileBlockLayout>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtBStoreDelayFileBlockLayout {
  pub record_index: usize,
  pub record_type: u16,
  pub old_offset: u32,
  pub new_offset: u32,
  pub old_size: u32,
  pub new_size: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtPartialStream {
  pub sequence: OfficeArtPartialSequence,
  pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtPartialSequence {
  pub records: Vec<OfficeArtPartialRecord>,
  pub trailing_header: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeArtPartialRecord {
  Complete(OfficeArtRecord),
  Incomplete(OfficeArtIncompleteRecord),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtIncompleteRecord {
  pub header: OfficeArtRecordHeader,
  pub data: OfficeArtIncompleteRecordData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeArtIncompleteRecordData {
  Container(OfficeArtPartialSequence),
  /// A typed FBSE whose on-disk record length omits high-order payload bits.
  /// The original header is retained by [`OfficeArtIncompleteRecord`].
  FbseWithUnderreportedLength(OfficeArtFbse),
  PropertyTable(OfficeArtIncompletePropertyTable),
  RecoveredSequence {
    prefix: OfficeArtRecoveredPrefix,
    sequence: OfficeArtPartialSequence,
  },
  Atom {
    available_payload: Vec<u8>,
  },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeArtRecoveredPrefix {
  Words2([u32; 2]),
  ClientAnchor(OfficeArtClientAnchor),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtRecordHeader {
  pub version: u8,
  pub instance: u16,
  pub record_type: u16,
  pub declared_length: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkBitfield)]
#[sdk(repr = "u16")]
struct OfficeArtRecordHeaderOptions {
  #[sdk(bits = 0..=3)]
  version: u8,
  #[sdk(bits = 4..=15)]
  instance: u16,
}

impl SdkRead for OfficeArtRecordHeader {
  fn read_from<R: Read + Seek>(reader: &mut Reader<R>) -> Result<Self> {
    let options = OfficeArtRecordHeaderOptions::read_from(reader)?;
    Ok(Self {
      version: options.version,
      instance: options.instance,
      record_type: reader.read_u16()?,
      declared_length: reader.read_u32()?,
    })
  }
}

impl SdkWrite for OfficeArtRecordHeader {
  fn write_to<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    OfficeArtRecordHeaderOptions {
      version: self.version,
      instance: self.instance,
    }
    .write_to(writer)?;
    writer.write_u16(self.record_type)?;
    writer.write_u32(self.declared_length)
  }
}

impl SdkSize for OfficeArtRecordHeader {
  fn sdk_size(&self) -> u64 {
    HEADER_LEN as u64
  }
}

impl OfficeArtRecordHeader {
  fn read_slice(bytes: &[u8]) -> Result<Self> {
    let header = bytes
      .get(..HEADER_LEN)
      .ok_or_else(|| Error::invalid(0, "truncated OfficeArt record header"))?;
    let mut reader = Reader::new(Cursor::new(header))?;
    Self::read_from(&mut reader)
  }

  fn append_to(self, bytes: &mut Vec<u8>) -> Result<()> {
    self.emit_to(bytes)?;
    Ok(())
  }

  fn emit_to(self, writer: &mut dyn Write) -> Result<()> {
    let mut writer = Writer::new(&mut *writer);
    self.write_to(&mut writer)?;
    Ok(())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtRecord {
  pub header: OfficeArtRecordHeader,
  pub data: OfficeArtRecordData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeArtRecordData {
  Container(Vec<OfficeArtRecord>),
  /// Children of a known container whose producer wrote a non-container recVer.
  CompatibilityContainer(Vec<OfficeArtRecord>),
  Atom(Vec<u8>),
  ArcRule(OfficeArtArcRule),
  CalloutRule(OfficeArtCalloutRule),
  ChildAnchor(OfficeArtRect),
  ClientAnchor(OfficeArtClientAnchor),
  ClientMarker(OfficeArtClientMarker),
  ColorMru(Vec<OfficeArtColor>),
  ConnectorRule(OfficeArtConnectorRule),
  BitmapBlip(OfficeArtBitmapBlip),
  Drawing(OfficeArtDrawing),
  DggBlock(OfficeArtDggBlock),
  EmptyCompatibilityAtom,
  Fbse(OfficeArtFbse),
  Frit(Vec<OfficeArtFrit>),
  GroupShape(OfficeArtRect),
  IncompletePropertyTable(OfficeArtIncompletePropertyTable),
  MetafileBlip(OfficeArtMetafileBlip),
  PropertyTable(OfficeArtPropertyTable),
  Shape(OfficeArtShape),
  SoftMakerNativeProperties(SoftMakerNativeProperties),
  SplitMenuColors([u32; 4]),
  WordClientAnchor(i32),
  WordClientData(i32),
  WordClientTextbox(OfficeArtWordClientTextbox),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtWordClientTextbox {
  pub story_index: u16,
  pub chain_index: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoftMakerNativeProperties {
  pub properties: Vec<SoftMakerNativeProperty>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoftMakerNativeProperty {
  pub selector: u16,
  pub reserved: u16,
  pub declared_length: u32,
  pub data: SoftMakerNativePropertyData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoftMakerNativePropertyData {
  Selector0 {
    leading: u8,
    words: [u32; 9],
  },
  Selector1 {
    double_bits: [u64; 10],
  },
  Selector2 {
    words: [u32; 35],
  },
  Selector3 {
    words: [u32; 15],
  },
  Selector4 {
    words: [u32; 24],
  },
  Selector6 {
    font_name: [u16; 6],
    words: [u32; 17],
    trailing: u8,
  },
  Selector8 {
    words: [u32; 5],
  },
  Selector9(u32),
  Selector12 {
    words: [u32; 4],
  },
  Unknown(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtDggBlock {
  pub maximum_shape_id: u32,
  pub declared_cluster_count: u32,
  pub saved_shape_count: u32,
  pub saved_drawing_count: u32,
  pub clusters: Vec<OfficeArtIdCluster>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtIdCluster {
  pub drawing_id: u32,
  /// `cspidCur`: number of local shape identifiers already allocated in
  /// this cluster (and therefore the next local identifier to allocate).
  pub current_shape_id_count: u32,
}

/// Document-level view of one complete MS-ODRAW drawing group and its
/// drawing containers. The source record trees remain the editable truth;
/// this value makes their cross-record identifiers and allocation high-water
/// marks explicit without flattening the trees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtDrawingGraph {
  pub drawing_group: OfficeArtDggBlock,
  pub drawings: Vec<OfficeArtDrawingGraphDrawing>,
  pub clusters: Vec<OfficeArtDrawingGraphCluster>,
  /// Document-wide `OfficeArtBStoreContainer`, when present.
  pub blip_store: Option<OfficeArtBlipStore>,
  /// Non-complex `OfficeArtFOPTE` values whose `fBid` bit is set. A zero
  /// value is ignored by MS-ODRAW and is therefore not included.
  pub blip_references: Vec<OfficeArtBlipReference>,
  /// Property tables whose fixed or complex region was physically
  /// incomplete. Known `fBid` entries are still exposed, but the list of
  /// BLIP references cannot be claimed complete while this is non-empty.
  pub incomplete_property_tables: Vec<OfficeArtPropertyTableLocation>,
  pub maximum_shape_id_relation: OfficeArtHighWaterRelation,
  pub saved_shape_count_relation: OfficeArtHighWaterRelation,
  pub saved_drawing_count_relation: OfficeArtHighWaterRelation,
  pub issues: Vec<OfficeArtDrawingGraphIssue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtDrawingGraphDrawing {
  pub drawing_id: u16,
  pub drawing: OfficeArtDrawing,
  pub shapes: Vec<OfficeArtShape>,
  pub patriarch_shape_count: usize,
  pub shape_count_basis: OfficeArtShapeCountBasis,
  pub current_shape_id_relation: OfficeArtHighWaterRelation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtDrawingGraphCluster {
  /// One-based position in `OfficeArtFDGGBlock.Rgidcl`.
  pub cluster_number: u32,
  pub cluster: OfficeArtIdCluster,
  pub present_shape_count: usize,
  pub present_max_local_shape_id: Option<u32>,
  pub shape_id_count_relation: OfficeArtHighWaterRelation,
}

/// Document-wide `OfficeArtBStoreContainer.rgfb` projected without copying
/// the potentially large BLIP payloads from the editable record tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtBlipStore {
  pub declared_entry_count: u16,
  pub entries: Vec<OfficeArtBlipStoreEntry>,
  pub entry_count_matches: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtBlipStoreEntry {
  /// One-based `rgfb` index used by BLIP properties.
  pub blip_identifier: u32,
  pub record_type: u16,
  pub kind: OfficeArtBlipStoreEntryKind,
  pub actual_reference_count: u32,
  /// Present only for an `OfficeArtFBSE`, because a direct
  /// `OfficeArtBlip` file block has no `cRef` field.
  pub reference_count_relation: Option<OfficeArtBlipReferenceCountRelation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeArtBlipStoreEntryKind {
  Fbse {
    declared_reference_count: u32,
    delay_offset: u32,
    has_embedded_blip: bool,
  },
  DirectBlip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeArtBlipReferenceCountRelation {
  BelowActual,
  EqualToActual,
  AboveActual,
}

/// One ordinary (non-inline) `OfficeArtFOPTE` reference into the document
/// `OfficeArtBStoreContainer.rgfb` array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtBlipReference {
  /// `None` denotes a document-wide property table in the Dgg container.
  pub drawing_id: Option<u16>,
  pub property_record_type: u16,
  pub property_table_index: usize,
  pub property_index: usize,
  pub property_id: u16,
  /// One-based `rgfb` index.
  pub blip_identifier: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtPropertyTableLocation {
  pub drawing_id: Option<u16>,
  pub property_record_type: u16,
  pub property_table_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeArtShapeCountBasis {
  /// `OfficeArtFDG.csp` counts every currently present `OfficeArtFSP`.
  AllPresentShapes,
  /// Producer compatibility shape used by LibreOffice: the single
  /// patriarch `OfficeArtFSP` is not included in `csp`.
  ExcludesPatriarchShapes,
  /// `csp` is above the number of present shapes and therefore retains an
  /// allocation/history count that cannot be reconstructed from the tree.
  HistoricalHighWater,
  /// `csp` is below both specified present-shape interpretations.
  BelowPresentShapes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeArtHighWaterRelation {
  BelowPresentTree,
  EqualToPresentTree,
  AbovePresentTree,
  EmptyZero,
  EmptyNonzero,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeArtDrawingGraphIssue {
  MaximumShapeIdOutOfRange {
    value: u32,
  },
  DrawingIdOutOfRange {
    drawing_id: u16,
  },
  DuplicateDrawingId {
    drawing_id: u16,
  },
  DuplicateShapeId {
    shape_id: u32,
  },
  ShapeInClusterZero {
    drawing_id: u16,
    shape_id: u32,
  },
  ShapeClusterMissing {
    drawing_id: u16,
    shape_id: u32,
    cluster_number: u32,
  },
  ShapeClusterDrawingMismatch {
    shape_id: u32,
    drawing_id: u16,
    cluster_number: u32,
    cluster_drawing_id: u32,
  },
  BlipStoreEntryCountMismatch {
    declared: u16,
    actual: usize,
  },
  BlipReferenceOutOfRange {
    drawing_id: Option<u16>,
    property_record_type: u16,
    property_id: u16,
    blip_identifier: u32,
  },
  EmptyBlipStoreSlotReferenced {
    drawing_id: Option<u16>,
    property_record_type: u16,
    property_id: u16,
    blip_identifier: u32,
  },
}

#[derive(Clone, Debug)]
pub(crate) struct OfficeArtGraphBlipStoreInput {
  pub declared_entry_count: u16,
  pub entries: Vec<OfficeArtGraphBlipStoreEntryInput>,
}

#[derive(Clone, Debug)]
pub(crate) struct OfficeArtGraphBlipStoreEntryInput {
  pub record_type: u16,
  pub fbse: Option<(u32, u32, bool)>,
}

#[derive(Clone, Debug)]
pub(crate) struct OfficeArtGraphDrawingInput {
  pub drawing_id: u16,
  pub drawing: OfficeArtDrawing,
  pub shapes: Vec<OfficeArtShape>,
  pub blip_references: Vec<OfficeArtBlipReference>,
  pub incomplete_property_tables: Vec<OfficeArtPropertyTableLocation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtFbse {
  pub win32_blip_type: u8,
  pub macos_blip_type: u8,
  pub uid: [u8; 16],
  pub tag: u16,
  pub declared_blip_size: u32,
  pub reference_count: u32,
  pub delay_offset: u32,
  pub unused1: u8,
  pub declared_name_length: u8,
  pub unused2: u8,
  pub unused3: u8,
  /// UTF-16 code units, including the terminating NUL when present.
  pub name_data: Vec<u16>,
  pub embedded_blip: Option<Box<OfficeArtRecord>>,
  /// Compatibility bytes following the optional embedded BLIP.
  pub trailing: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtBitmapBlip {
  pub uid1: [u8; 16],
  pub uid2: Option<[u8; 16]>,
  pub tag: u8,
  pub file_data: OfficeArtBitmapData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeArtBitmapData {
  /// Encoded DIB data. The image format is external to MS-ODRAW.
  Dib(Vec<u8>),
  /// Encoded JPEG, PNG, or TIFF data.
  Encoded(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtMetafileBlip {
  pub uid1: [u8; 16],
  pub uid2: Option<[u8; 16]>,
  pub metafile_header: OfficeArtMetafileHeader,
  pub file_data: OfficeArtMetafileData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeArtMetafileData {
  Emf(OfficeArtMetafileBytes),
  Wmf(OfficeArtMetafileBytes),
  /// Encoded Macintosh PICT file data. MS-ODRAW treats this as an image leaf;
  /// decoded bytes remain editable and are re-encoded with the original
  /// OfficeArt compression mode.
  Pict(OfficeArtMetafileBytes),
  /// Unsupported compression or producer data rejected by the typed SDK.
  Opaque {
    reason: OfficeArtMetafileOpaqueReason,
    decoded: Option<Vec<u8>>,
    original_encoded: Vec<u8>,
  },
}

/// File format of one decoded or natively encoded OfficeArt BLIP payload.
///
/// This closed enum replaces record-type literals at SDK call sites while
/// retaining the exact distinction needed for an OOXML image content type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OfficeArtImageFormat {
  Emf,
  Wmf,
  Pict,
  Jpeg,
  Png,
  Dib,
  Tiff,
}

/// Borrowed image payload exposed by a typed OfficeArt BLIP record.
///
/// Bitmap payloads already use their native encoded form. Metafiles expose
/// their decoded bytes because the OfficeArt compression wrapper is not part
/// of the image file stored in an OOXML media part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtImageRef<'a> {
  pub format: OfficeArtImageFormat,
  pub data: &'a [u8],
}

/// Exact clone-on-write state for one editable OfficeArt metafile payload.
///
/// The decoded baseline and encoded source share their allocations across
/// record/file-root clones. Calling [`Self::decoded_mut`] detaches only the
/// decoded bytes; unchanged serialization can therefore reuse the original
/// encoded payload without decompressing it a second time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtMetafileBytes {
  decoded: Arc<Vec<u8>>,
  original_decoded: Arc<Vec<u8>>,
  original_encoded: Arc<Vec<u8>>,
}

impl OfficeArtMetafileBytes {
  fn new(decoded: Vec<u8>, original_encoded: Vec<u8>) -> Self {
    let decoded = Arc::new(decoded);
    Self {
      original_decoded: Arc::clone(&decoded),
      decoded,
      original_encoded: Arc::new(original_encoded),
    }
  }

  pub fn decoded(&self) -> &[u8] {
    self.decoded.as_slice()
  }

  pub fn decoded_mut(&mut self) -> &mut Vec<u8> {
    Arc::make_mut(&mut self.decoded)
  }

  pub fn original_encoded(&self) -> &[u8] {
    self.original_encoded.as_slice()
  }

  fn is_unchanged(&self) -> bool {
    Arc::ptr_eq(&self.decoded, &self.original_decoded)
      || self.decoded.as_slice() == self.original_decoded.as_slice()
  }

  fn commit(&mut self, encoded: Vec<u8>) {
    self.original_decoded = Arc::clone(&self.decoded);
    self.original_encoded = Arc::new(encoded);
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OfficeArtMetafileOpaqueReason {
  DecodeFailed,
  UnsupportedCompression(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtMetafileHeader {
  pub uncompressed_size: u32,
  pub bounds: OfficeArtRect,
  pub render_size: OfficeArtPoint,
  pub saved_size: u32,
  pub compression: u8,
  pub filter: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtRect {
  pub left: i32,
  pub top: i32,
  pub right: i32,
  pub bottom: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtPoint {
  pub x: i32,
  pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtDrawing {
  pub shape_count: u32,
  pub current_shape_id: u32,
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct OfficeArtShapeFlags: u32 {
        const GROUP = 0x0001;
        const CHILD = 0x0002;
        const PATRIARCH = 0x0004;
        const DELETED = 0x0008;
        const OLE_SHAPE = 0x0010;
        const HAVE_MASTER = 0x0020;
        const FLIP_HORIZONTAL = 0x0040;
        const FLIP_VERTICAL = 0x0080;
        const CONNECTOR = 0x0100;
        const HAVE_ANCHOR = 0x0200;
        const BACKGROUND = 0x0400;
        const HAVE_SHAPE_TYPE = 0x0800;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtShape {
  pub shape_id: u32,
  pub flags: OfficeArtShapeFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeArtClientMarker {
  Textbox,
  Data,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeArtClientAnchor {
  /// Shared 18-byte representation used by sheet and chart anchors.
  Words18 { flags: u16, coordinates: [u16; 8] },
  /// Eight-byte host-defined representation retained as four wire words.
  /// PowerPoint interprets these as y1, x1, x2, y2 master coordinates.
  Words8 { coordinates: [i16; 4] },
  /// 16-byte host anchor used by PowerPoint drawing clients.
  PowerPointRect(OfficeArtRect),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtArcRule {
  pub rule_id: u32,
  pub shape_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtColor(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtConnectorRule {
  pub rule_id: u32,
  pub start_shape_id: u32,
  pub end_shape_id: u32,
  pub connector_shape_id: u32,
  pub start_connection_site: u32,
  pub end_connection_site: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtCalloutRule {
  pub rule_id: u32,
  pub shape_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtFrit {
  pub new_group_id: u16,
  pub old_group_id: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtPropertyTable {
  pub properties: Vec<OfficeArtProperty>,
  pub trailing: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtIncompletePropertyTable {
  pub entries: Vec<OfficeArtPropertyEntry>,
  pub incomplete_fixed_entry: OfficeArtIncompletePropertyEntry,
  pub complex_fragments: Vec<OfficeArtComplexPropertyFragment>,
  pub trailing_data: Vec<u8>,
  pub recovered_trailing: Option<Box<OfficeArtPartialSequence>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeArtIncompletePropertyEntry {
  None,
  LowWord {
    property_id: u16,
    is_blip_id: bool,
    is_complex: bool,
    value_low: u16,
  },
  Other(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtComplexPropertyFragment {
  pub entry_index: usize,
  pub property_id: u16,
  pub declared_length: u32,
  pub data: OfficeArtComplexPropertyData,
  pub is_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeArtComplexPropertyData {
  Bytes(Vec<u8>),
  Array(OfficeArtArray),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtPropertyEntry {
  pub property_id: u16,
  pub is_blip_id: bool,
  pub is_complex: bool,
  pub value_or_declared_length: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtProperty {
  pub property_id: u16,
  pub is_blip_id: bool,
  pub value: OfficeArtPropertyValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeArtPropertyValue {
  Simple(u32),
  Complex {
    declared_length: u32,
    data: Vec<u8>,
  },
  Utf16String {
    declared_length: u32,
    /// Exact UTF-16 code units, including a terminating NUL when present.
    code_units: Vec<u16>,
  },
  /// A complex-flagged property with a zero declared length and no body.
  EmptyComplex {
    declared_length: u32,
  },
  EmptyArray {
    declared_length: u32,
  },
  Array {
    declared_length: u32,
    /// Bytes present in the encoded array beyond the FOPTE-declared length.
    /// This is normally 0 or the 6-byte array header; damaged producers can
    /// retain another explicit delta without losing the typed array body.
    declared_length_delta: u8,
    value: OfficeArtArray,
  },
  MetroBlob {
    declared_length: u32,
    value: OfficeArtMetroBlob,
  },
  Hyperlink {
    declared_length: u32,
    class_id: [u8; 16],
    object: crate::xls::HyperlinkObject,
  },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtMetroBlob {
  /// Exact encoded OPC/ZIP package bytes.
  pub package_bytes: Vec<u8>,
  pub directory: OfficeArtZipDirectory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtZipDirectory {
  pub entry_count: u16,
  pub central_directory_size: u32,
  pub central_directory_offset: u32,
  pub comment: Vec<u8>,
  pub entries: Vec<OfficeArtZipEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtZipEntry {
  pub compression_method: u16,
  pub flags: u16,
  pub crc32: u32,
  pub compressed_size: u32,
  pub uncompressed_size: u32,
  pub file_name: Vec<u8>,
  pub extra_field: Vec<u8>,
  pub comment: Vec<u8>,
  pub local_header_offset: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtArray {
  pub element_count: u16,
  pub allocated_element_count: u16,
  pub encoded_element_size: u16,
  pub data: OfficeArtArrayData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeArtArrayData {
  Points16(Vec<OfficeArtPoint16>),
  Points32(Vec<OfficeArtPoint32>),
  Segments(Vec<u16>),
  FixedPointBits(Vec<u32>),
  Rectangles(Vec<OfficeArtRect>),
  ShadeColors(Vec<OfficeArtShadeColor>),
  Unsigned32(Vec<u32>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtPoint16 {
  pub x: i16,
  pub y: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtPoint32 {
  pub x: i32,
  pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeArtShadeColor {
  pub color: u32,
  pub position: u32,
}

impl OfficeArtStream {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_bytes_with_limits(bytes, Limits::default())
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    if bytes.len() > limits.max_allocation {
      return Err(Error::Limit(format!(
        "OfficeArt stream length exceeds {}",
        limits.max_allocation
      )));
    }
    let mut record_count = 0usize;
    let records = parse_records(bytes, 0, &mut record_count, limits)?;
    Ok(Self { records })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    encode_complete_records(&self.records)
  }

  pub(crate) fn serialized_len(&self) -> Result<usize> {
    complete_records_encoded_len(&self.records)
  }

  pub(crate) fn write_to(&self, writer: &mut dyn Write) -> Result<()> {
    write_complete_records(&self.records, writer)
  }

  /// Recomputes every complete OfficeArt record length from the typed tree.
  /// The update is transactional.
  pub fn relayout(&mut self) -> Result<()> {
    let mut rebuilt = self.clone();
    for record in &mut rebuilt.records {
      record.relayout()?;
    }
    *self = rebuilt;
    Ok(())
  }

  pub fn visit<'a>(&'a self, mut visitor: impl FnMut(&'a OfficeArtRecord)) {
    fn visit_records<'a>(
      records: &'a [OfficeArtRecord],
      visitor: &mut impl FnMut(&'a OfficeArtRecord),
    ) {
      for record in records {
        visitor(record);
        match &record.data {
          OfficeArtRecordData::Container(children)
          | OfficeArtRecordData::CompatibilityContainer(children) => {
            visit_records(children, visitor)
          }
          OfficeArtRecordData::Fbse(fbse) => {
            if let Some(blip) = &fbse.embedded_blip {
              visitor(blip);
              match &blip.data {
                OfficeArtRecordData::Container(children)
                | OfficeArtRecordData::CompatibilityContainer(children) => {
                  visit_records(children, visitor);
                }
                _ => {}
              }
            }
          }
          _ => {}
        }
      }
    }
    visit_records(&self.records, &mut visitor);
  }

  pub fn visit_mut(&mut self, mut visitor: impl FnMut(&mut OfficeArtRecord)) {
    fn visit_records(
      records: &mut [OfficeArtRecord],
      visitor: &mut impl FnMut(&mut OfficeArtRecord),
    ) {
      for record in records {
        visitor(record);
        match &mut record.data {
          OfficeArtRecordData::Container(children)
          | OfficeArtRecordData::CompatibilityContainer(children) => {
            visit_records(children, visitor)
          }
          OfficeArtRecordData::Fbse(fbse) => {
            if let Some(blip) = &mut fbse.embedded_blip {
              visitor(blip);
              match &mut blip.data {
                OfficeArtRecordData::Container(children)
                | OfficeArtRecordData::CompatibilityContainer(children) => {
                  visit_records(children, visitor);
                }
                _ => {}
              }
            }
          }
          _ => {}
        }
      }
    }
    visit_records(&mut self.records, &mut visitor);
  }
}

impl OfficeArtDrawingGraph {
  /// Host-format adapter for formats such as PPT whose recursive container
  /// framing is owned by the host record tree while OfficeArt atoms remain
  /// typed `OfficeArtRecord` values.
  pub fn from_components(
    drawing_group: OfficeArtDggBlock,
    drawings: Vec<(u16, OfficeArtDrawing, Vec<OfficeArtShape>)>,
  ) -> Result<Self> {
    let drawing_group = OfficeArtStream {
      records: vec![OfficeArtRecord {
        header: OfficeArtRecordHeader {
          version: 0x0f,
          instance: 0,
          record_type: 0xf000,
          declared_length: 0,
        },
        data: OfficeArtRecordData::Container(vec![OfficeArtRecord {
          header: OfficeArtRecordHeader {
            version: 0,
            instance: 0,
            record_type: 0xf006,
            declared_length: 0,
          },
          data: OfficeArtRecordData::DggBlock(drawing_group),
        }]),
      }],
    };
    let drawings = drawings
      .into_iter()
      .map(|(drawing_id, drawing, shapes)| OfficeArtStream {
        records: vec![OfficeArtRecord {
          header: OfficeArtRecordHeader {
            version: 0x0f,
            instance: 0,
            record_type: 0xf002,
            declared_length: 0,
          },
          data: OfficeArtRecordData::Container(
            std::iter::once(OfficeArtRecord {
              header: OfficeArtRecordHeader {
                version: 0,
                instance: drawing_id,
                record_type: 0xf008,
                declared_length: 8,
              },
              data: OfficeArtRecordData::Drawing(drawing),
            })
            .chain(shapes.into_iter().map(|shape| OfficeArtRecord {
              header: OfficeArtRecordHeader {
                version: 2,
                instance: 0,
                record_type: 0xf00a,
                declared_length: 8,
              },
              data: OfficeArtRecordData::Shape(shape),
            }))
            .collect(),
          ),
        }],
      })
      .collect::<Vec<_>>();
    let drawing_refs = drawings.iter().collect::<Vec<_>>();
    Self::from_streams(&drawing_group, &drawing_refs)
  }

  pub(crate) fn from_components_with_blips(
    drawing_group: OfficeArtDggBlock,
    blip_stores: Vec<OfficeArtGraphBlipStoreInput>,
    drawing_group_blip_references: Vec<OfficeArtBlipReference>,
    drawing_group_incomplete_property_tables: Vec<OfficeArtPropertyTableLocation>,
    drawings: Vec<OfficeArtGraphDrawingInput>,
  ) -> Result<Self> {
    let basic_drawings = drawings
      .iter()
      .map(|value| (value.drawing_id, value.drawing, value.shapes.clone()))
      .collect();
    let mut graph = Self::from_components(drawing_group, basic_drawings)?;
    let mut references = drawing_group_blip_references;
    let mut incomplete_property_tables = drawing_group_incomplete_property_tables;
    for drawing in drawings {
      references.extend(drawing.blip_references);
      incomplete_property_tables.extend(drawing.incomplete_property_tables);
    }
    graph.incomplete_property_tables = incomplete_property_tables;
    graph.attach_blip_graph(blip_stores, references)?;
    Ok(graph)
  }

  /// Builds a cross-record graph from one complete `OfficeArtDggContainer`
  /// and the complete `OfficeArtDgContainer` records owned by the host
  /// document. Structural identity errors are rejected; count conventions
  /// and allocation high-water relations remain explicit in the result.
  pub fn from_streams(
    drawing_group: &OfficeArtStream,
    drawings: &[&OfficeArtStream],
  ) -> Result<Self> {
    require_office_art_root(drawing_group, 0xf000, "OfficeArtDggContainer")?;
    let blip_stores = collect_office_art_blip_store_inputs(drawing_group);
    let (mut blip_references, mut incomplete_property_tables) =
      collect_office_art_blip_references(drawing_group, None);
    let mut dgg_records = Vec::new();
    drawing_group.visit(|record| {
      if let OfficeArtRecordData::DggBlock(value) = &record.data {
        dgg_records.push(value.clone());
      }
    });
    let [drawing_group] = dgg_records.as_slice() else {
      return Err(Error::invalid(
        0,
        format!(
          "OfficeArtDggContainer contains {} OfficeArtFDGGBlock records, expected 1",
          dgg_records.len()
        ),
      ));
    };
    let mut issues = Vec::new();
    if drawing_group.maximum_shape_id >= 0x03ff_d7ff {
      issues.push(OfficeArtDrawingGraphIssue::MaximumShapeIdOutOfRange {
        value: drawing_group.maximum_shape_id,
      });
    }

    let mut drawing_ids = BTreeSet::new();
    let mut shape_ids = BTreeSet::new();
    let mut graph_drawings = Vec::with_capacity(drawings.len());
    for (drawing_index, stream) in drawings.iter().enumerate() {
      require_office_art_root(stream, 0xf002, "OfficeArtDgContainer")?;
      let mut fdg_records = Vec::new();
      let mut shapes = Vec::new();
      stream.visit(|record| match &record.data {
        OfficeArtRecordData::Drawing(value) => {
          fdg_records.push((record.header.instance, *value));
        }
        OfficeArtRecordData::Shape(value) => shapes.push(*value),
        _ => {}
      });
      let [(drawing_id, drawing)] = fdg_records.as_slice() else {
        return Err(Error::invalid(
          0,
          format!(
            "OfficeArtDgContainer {drawing_index} contains {} OfficeArtFDG records, expected 1",
            fdg_records.len()
          ),
        ));
      };
      let (drawing_blip_references, drawing_incomplete_property_tables) =
        collect_office_art_blip_references(stream, Some(*drawing_id));
      blip_references.extend(drawing_blip_references);
      incomplete_property_tables.extend(drawing_incomplete_property_tables);
      if *drawing_id > 0x0ffe {
        issues.push(OfficeArtDrawingGraphIssue::DrawingIdOutOfRange {
          drawing_id: *drawing_id,
        });
      }
      if !drawing_ids.insert(*drawing_id) {
        issues.push(OfficeArtDrawingGraphIssue::DuplicateDrawingId {
          drawing_id: *drawing_id,
        });
      }
      for shape in &shapes {
        if !shape_ids.insert(shape.shape_id) {
          issues.push(OfficeArtDrawingGraphIssue::DuplicateShapeId {
            shape_id: shape.shape_id,
          });
        }
      }
      let patriarch_shape_count = shapes
        .iter()
        .filter(|shape| shape.flags.contains(OfficeArtShapeFlags::PATRIARCH))
        .count();
      let present_shape_count = shapes.len();
      let declared_shape_count = usize::try_from(drawing.shape_count)
        .map_err(|_| Error::Limit("OfficeArtFDG shape count exceeds usize".into()))?;
      let non_patriarch_count = present_shape_count.saturating_sub(patriarch_shape_count);
      let shape_count_basis = if declared_shape_count == present_shape_count {
        OfficeArtShapeCountBasis::AllPresentShapes
      } else if patriarch_shape_count != 0 && declared_shape_count == non_patriarch_count {
        OfficeArtShapeCountBasis::ExcludesPatriarchShapes
      } else if declared_shape_count > present_shape_count {
        OfficeArtShapeCountBasis::HistoricalHighWater
      } else {
        OfficeArtShapeCountBasis::BelowPresentShapes
      };
      let current_shape_id_relation = office_art_high_water_relation(
        drawing.current_shape_id,
        shapes.iter().map(|shape| shape.shape_id).max(),
      );
      graph_drawings.push(OfficeArtDrawingGraphDrawing {
        drawing_id: *drawing_id,
        drawing: *drawing,
        shapes,
        patriarch_shape_count,
        shape_count_basis,
        current_shape_id_relation,
      });
    }

    let mut clusters = Vec::with_capacity(drawing_group.clusters.len());
    for (index, cluster) in drawing_group.clusters.iter().enumerate() {
      clusters.push(OfficeArtDrawingGraphCluster {
        cluster_number: u32::try_from(index + 1)
          .map_err(|_| Error::Limit("OfficeArt cluster number exceeds u32".into()))?,
        cluster: *cluster,
        present_shape_count: 0,
        present_max_local_shape_id: None,
        shape_id_count_relation: OfficeArtHighWaterRelation::EmptyZero,
      });
    }
    for drawing in &graph_drawings {
      for shape in &drawing.shapes {
        let cluster_number = shape.shape_id / 0x400;
        let Some(cluster_number_minus_one) = cluster_number.checked_sub(1) else {
          issues.push(OfficeArtDrawingGraphIssue::ShapeInClusterZero {
            drawing_id: drawing.drawing_id,
            shape_id: shape.shape_id,
          });
          continue;
        };
        let cluster_index = usize::try_from(cluster_number_minus_one)
          .map_err(|_| Error::Limit("OfficeArt cluster index exceeds usize".into()))?;
        let Some(cluster) = clusters.get_mut(cluster_index) else {
          issues.push(OfficeArtDrawingGraphIssue::ShapeClusterMissing {
            drawing_id: drawing.drawing_id,
            shape_id: shape.shape_id,
            cluster_number,
          });
          continue;
        };
        if cluster.cluster.drawing_id != u32::from(drawing.drawing_id) {
          issues.push(OfficeArtDrawingGraphIssue::ShapeClusterDrawingMismatch {
            shape_id: shape.shape_id,
            drawing_id: drawing.drawing_id,
            cluster_number,
            cluster_drawing_id: cluster.cluster.drawing_id,
          });
        }
        cluster.present_shape_count += 1;
        let local_shape_id = shape.shape_id % 0x400;
        cluster.present_max_local_shape_id = Some(
          cluster
            .present_max_local_shape_id
            .map_or(local_shape_id, |current| current.max(local_shape_id)),
        );
      }
    }
    for cluster in &mut clusters {
      cluster.shape_id_count_relation = office_art_high_water_relation(
        cluster.cluster.current_shape_id_count,
        cluster
          .present_max_local_shape_id
          .and_then(|maximum| maximum.checked_add(1)),
      );
    }

    let maximum_shape_id_relation = office_art_high_water_relation(
      drawing_group.maximum_shape_id,
      shape_ids.iter().copied().max(),
    );
    let present_shape_count = graph_drawings.iter().try_fold(0usize, |count, drawing| {
      count
        .checked_add(drawing.shapes.len())
        .ok_or_else(|| Error::Limit("OfficeArt present shape count overflow".into()))
    })?;
    let present_shape_count = u32::try_from(present_shape_count)
      .map_err(|_| Error::Limit("OfficeArt present shape count exceeds u32".into()))?;
    let present_drawing_count = u32::try_from(graph_drawings.len())
      .map_err(|_| Error::Limit("OfficeArt present drawing count exceeds u32".into()))?;
    let saved_shape_count_relation =
      office_art_high_water_relation(drawing_group.saved_shape_count, Some(present_shape_count));
    let saved_drawing_count_relation = office_art_high_water_relation(
      drawing_group.saved_drawing_count,
      Some(present_drawing_count),
    );
    let mut graph = Self {
      drawing_group: drawing_group.clone(),
      drawings: graph_drawings,
      clusters,
      blip_store: None,
      blip_references: Vec::new(),
      incomplete_property_tables,
      maximum_shape_id_relation,
      saved_shape_count_relation,
      saved_drawing_count_relation,
      issues,
    };
    graph.attach_blip_graph(blip_stores, blip_references)?;
    Ok(graph)
  }

  fn attach_blip_graph(
    &mut self,
    blip_stores: Vec<OfficeArtGraphBlipStoreInput>,
    references: Vec<OfficeArtBlipReference>,
  ) -> Result<()> {
    if blip_stores.len() > 1 {
      return Err(Error::invalid(
        0,
        format!(
          "OfficeArtDggContainer contains {} OfficeArtBStoreContainer records, expected at most 1",
          blip_stores.len()
        ),
      ));
    }
    let Some(store) = blip_stores.into_iter().next() else {
      for reference in &references {
        self
          .issues
          .push(OfficeArtDrawingGraphIssue::BlipReferenceOutOfRange {
            drawing_id: reference.drawing_id,
            property_record_type: reference.property_record_type,
            property_id: reference.property_id,
            blip_identifier: reference.blip_identifier,
          });
      }
      self.blip_references = references;
      return Ok(());
    };

    let entry_count_matches = usize::from(store.declared_entry_count) == store.entries.len();
    if !entry_count_matches {
      self
        .issues
        .push(OfficeArtDrawingGraphIssue::BlipStoreEntryCountMismatch {
          declared: store.declared_entry_count,
          actual: store.entries.len(),
        });
    }
    let mut actual_reference_counts = vec![0u32; store.entries.len()];
    for reference in &references {
      let Some(zero_based) = reference.blip_identifier.checked_sub(1) else {
        continue;
      };
      let Ok(entry_index) = usize::try_from(zero_based) else {
        self
          .issues
          .push(OfficeArtDrawingGraphIssue::BlipReferenceOutOfRange {
            drawing_id: reference.drawing_id,
            property_record_type: reference.property_record_type,
            property_id: reference.property_id,
            blip_identifier: reference.blip_identifier,
          });
        continue;
      };
      let Some(entry) = store.entries.get(entry_index) else {
        self
          .issues
          .push(OfficeArtDrawingGraphIssue::BlipReferenceOutOfRange {
            drawing_id: reference.drawing_id,
            property_record_type: reference.property_record_type,
            property_id: reference.property_id,
            blip_identifier: reference.blip_identifier,
          });
        continue;
      };
      actual_reference_counts[entry_index] = actual_reference_counts[entry_index]
        .checked_add(1)
        .ok_or_else(|| Error::Limit("OfficeArt BLIP reference count overflow".into()))?;
      if matches!(entry.fbse, Some((0, _, _))) {
        self
          .issues
          .push(OfficeArtDrawingGraphIssue::EmptyBlipStoreSlotReferenced {
            drawing_id: reference.drawing_id,
            property_record_type: reference.property_record_type,
            property_id: reference.property_id,
            blip_identifier: reference.blip_identifier,
          });
      }
    }
    let entries = store
      .entries
      .into_iter()
      .enumerate()
      .map(|(index, entry)| {
        let actual_reference_count = actual_reference_counts[index];
        let (kind, reference_count_relation) = match entry.fbse {
          Some((declared_reference_count, delay_offset, has_embedded_blip)) => {
            let relation = match declared_reference_count.cmp(&actual_reference_count) {
              std::cmp::Ordering::Less => OfficeArtBlipReferenceCountRelation::BelowActual,
              std::cmp::Ordering::Equal => OfficeArtBlipReferenceCountRelation::EqualToActual,
              std::cmp::Ordering::Greater => OfficeArtBlipReferenceCountRelation::AboveActual,
            };
            (
              OfficeArtBlipStoreEntryKind::Fbse {
                declared_reference_count,
                delay_offset,
                has_embedded_blip,
              },
              Some(relation),
            )
          }
          None => (OfficeArtBlipStoreEntryKind::DirectBlip, None),
        };
        Ok(OfficeArtBlipStoreEntry {
          blip_identifier: u32::try_from(index + 1)
            .map_err(|_| Error::Limit("OfficeArt BLIP identifier exceeds u32".into()))?,
          record_type: entry.record_type,
          kind,
          actual_reference_count,
          reference_count_relation,
        })
      })
      .collect::<Result<Vec<_>>>()?;
    self.blip_store = Some(OfficeArtBlipStore {
      declared_entry_count: store.declared_entry_count,
      entries,
      entry_count_matches,
    });
    self.blip_references = references;
    Ok(())
  }

  /// Enforces the literal MS-ODRAW count interpretation. Compatibility
  /// producer conventions and stale allocation counts remain available from
  /// `from_streams`, but are not silently accepted here.
  pub fn validate_strict(&self) -> Result<()> {
    if !self.issues.is_empty() {
      return Err(Error::invalid(
        0,
        format!(
          "OfficeArt drawing graph contains {} identifier or cluster issues",
          self.issues.len()
        ),
      ));
    }
    if !self.incomplete_property_tables.is_empty() {
      return Err(Error::invalid(
        0,
        "OfficeArt drawing graph has incomplete property tables, so its BLIP reference inventory is not complete",
      ));
    }
    for drawing in &self.drawings {
      if drawing.shape_count_basis != OfficeArtShapeCountBasis::AllPresentShapes {
        return Err(Error::invalid(
          0,
          format!(
            "OfficeArtFDG {} shape count does not equal its present OfficeArtFSP count",
            drawing.drawing_id
          ),
        ));
      }
      if !matches!(
        drawing.current_shape_id_relation,
        OfficeArtHighWaterRelation::EqualToPresentTree | OfficeArtHighWaterRelation::EmptyZero
      ) {
        return Err(Error::invalid(
          0,
          format!(
            "OfficeArtFDG {} current shape identifier is not the last present shape",
            drawing.drawing_id
          ),
        ));
      }
    }
    if self.saved_shape_count_relation != OfficeArtHighWaterRelation::EqualToPresentTree {
      return Err(Error::invalid(
        0,
        "OfficeArtFDGG saved shape count does not equal the present OfficeArtFSP count",
      ));
    }
    if self.saved_drawing_count_relation != OfficeArtHighWaterRelation::EqualToPresentTree {
      return Err(Error::invalid(
        0,
        "OfficeArtFDGG saved drawing count does not equal the drawing containers",
      ));
    }
    if !matches!(
      self.maximum_shape_id_relation,
      OfficeArtHighWaterRelation::EqualToPresentTree | OfficeArtHighWaterRelation::EmptyZero
    ) {
      return Err(Error::invalid(
        0,
        "OfficeArtFDGG maximum shape identifier is not the maximum present shape",
      ));
    }
    if self.clusters.iter().any(|cluster| {
      !matches!(
        cluster.shape_id_count_relation,
        OfficeArtHighWaterRelation::EqualToPresentTree | OfficeArtHighWaterRelation::EmptyZero
      )
    }) {
      return Err(Error::invalid(
        0,
        "OfficeArtIDCL shape-identifier count does not match its present shapes",
      ));
    }
    if self.blip_store.as_ref().is_some_and(|store| {
      store.entries.iter().any(|entry| {
        entry.reference_count_relation != Some(OfficeArtBlipReferenceCountRelation::EqualToActual)
          && entry.reference_count_relation.is_some()
      })
    }) {
      return Err(Error::invalid(
        0,
        "OfficeArtFBSE reference count does not equal the ordinary BLIP property references",
      ));
    }
    Ok(())
  }
}

fn collect_office_art_blip_store_inputs(
  stream: &OfficeArtStream,
) -> Vec<OfficeArtGraphBlipStoreInput> {
  let mut stores = Vec::new();
  stream.visit(|record| {
    if record.header.record_type != 0xf001 {
      return;
    }
    let children = match &record.data {
      OfficeArtRecordData::Container(children)
      | OfficeArtRecordData::CompatibilityContainer(children) => children,
      _ => return,
    };
    stores.push(OfficeArtGraphBlipStoreInput {
      declared_entry_count: record.header.instance,
      entries: children
        .iter()
        .map(|child| OfficeArtGraphBlipStoreEntryInput {
          record_type: child.header.record_type,
          fbse: match &child.data {
            OfficeArtRecordData::Fbse(value) => Some((
              value.reference_count,
              value.delay_offset,
              value.embedded_blip.is_some(),
            )),
            _ => None,
          },
        })
        .collect(),
    });
  });
  stores
}

fn collect_office_art_blip_references(
  stream: &OfficeArtStream,
  drawing_id: Option<u16>,
) -> (
  Vec<OfficeArtBlipReference>,
  Vec<OfficeArtPropertyTableLocation>,
) {
  let mut references = Vec::new();
  let mut incomplete_property_tables = Vec::new();
  let mut property_table_index = 0usize;
  stream.visit(|record| {
    collect_office_art_record_blip_references(
      record,
      drawing_id,
      &mut property_table_index,
      &mut references,
      &mut incomplete_property_tables,
    );
  });
  (references, incomplete_property_tables)
}

pub(crate) fn collect_office_art_record_blip_references(
  record: &OfficeArtRecord,
  drawing_id: Option<u16>,
  property_table_index: &mut usize,
  references: &mut Vec<OfficeArtBlipReference>,
  incomplete_property_tables: &mut Vec<OfficeArtPropertyTableLocation>,
) {
  match &record.data {
    OfficeArtRecordData::PropertyTable(table) => {
      for (property_index, property) in table.properties.iter().enumerate() {
        if !property.is_blip_id {
          continue;
        }
        let OfficeArtPropertyValue::Simple(blip_identifier) = property.value else {
          continue;
        };
        if blip_identifier != 0 {
          references.push(OfficeArtBlipReference {
            drawing_id,
            property_record_type: record.header.record_type,
            property_table_index: *property_table_index,
            property_index,
            property_id: property.property_id,
            blip_identifier,
          });
        }
      }
      *property_table_index += 1;
    }
    OfficeArtRecordData::IncompletePropertyTable(table) => {
      incomplete_property_tables.push(OfficeArtPropertyTableLocation {
        drawing_id,
        property_record_type: record.header.record_type,
        property_table_index: *property_table_index,
      });
      for (property_index, property) in table.entries.iter().enumerate() {
        if property.is_blip_id && !property.is_complex && property.value_or_declared_length != 0 {
          references.push(OfficeArtBlipReference {
            drawing_id,
            property_record_type: record.header.record_type,
            property_table_index: *property_table_index,
            property_index,
            property_id: property.property_id,
            blip_identifier: property.value_or_declared_length,
          });
        }
      }
      *property_table_index += 1;
    }
    _ => {}
  }
}

fn require_office_art_root(
  stream: &OfficeArtStream,
  record_type: u16,
  structure: &str,
) -> Result<()> {
  let [root] = stream.records.as_slice() else {
    return Err(Error::invalid(
      0,
      format!("{structure} stream does not contain exactly one root record"),
    ));
  };
  if root.header.version != 0x0f
    || root.header.record_type != record_type
    || !matches!(root.data, OfficeArtRecordData::Container(_))
  {
    return Err(Error::invalid(
      0,
      format!("{structure} root record has invalid framing"),
    ));
  }
  Ok(())
}

fn office_art_high_water_relation(
  value: u32,
  present_maximum: Option<u32>,
) -> OfficeArtHighWaterRelation {
  match present_maximum {
    None if value == 0 => OfficeArtHighWaterRelation::EmptyZero,
    None => OfficeArtHighWaterRelation::EmptyNonzero,
    Some(maximum) if value < maximum => OfficeArtHighWaterRelation::BelowPresentTree,
    Some(maximum) if value == maximum => OfficeArtHighWaterRelation::EqualToPresentTree,
    Some(_) => OfficeArtHighWaterRelation::AbovePresentTree,
  }
}

impl OfficeArtBStoreDelay {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_bytes_with_limits(bytes, Limits::default())
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    let stream = OfficeArtStream::from_bytes_with_limits(bytes, limits)?;
    for record in &stream.records {
      if !is_bstore_delay_file_block(record.header.record_type) {
        return Err(Error::invalid(
          0,
          format!(
            "OfficeArtBStoreDelay contains invalid file-block record type 0x{:04X}",
            record.header.record_type
          ),
        ));
      }
    }
    Ok(Self {
      records: stream.records,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    encode_complete_records(&self.records)
  }

  pub(crate) fn serialized_len(&self) -> Result<usize> {
    complete_records_encoded_len(&self.records)
  }

  pub(crate) fn write_to(&self, writer: &mut dyn Write) -> Result<()> {
    write_complete_records(&self.records, writer)
  }

  /// Recomputes file-block sizes and returns the old-to-new `foDelay`
  /// relocation map. The update is transactional.
  pub fn relayout(&mut self) -> Result<OfficeArtBStoreDelayLayout> {
    let mut rebuilt = self.clone();
    let mut old_offset = 0u32;
    let mut new_offset = 0u32;
    let mut file_blocks = Vec::with_capacity(rebuilt.records.len());
    for (record_index, record) in rebuilt.records.iter_mut().enumerate() {
      if !is_bstore_delay_file_block(record.header.record_type) {
        return Err(Error::invalid(
          u64::from(old_offset),
          "OfficeArtBStoreDelay contains an invalid file-block record type",
        ));
      }
      let old_size = record
        .header
        .declared_length
        .checked_add(HEADER_LEN as u32)
        .ok_or_else(|| Error::Limit("OfficeArt file-block size overflow".into()))?;
      record.relayout()?;
      let new_size = record
        .header
        .declared_length
        .checked_add(HEADER_LEN as u32)
        .ok_or_else(|| Error::Limit("OfficeArt file-block size overflow".into()))?;
      file_blocks.push(OfficeArtBStoreDelayFileBlockLayout {
        record_index,
        record_type: record.header.record_type,
        old_offset,
        new_offset,
        old_size,
        new_size,
      });
      old_offset = old_offset
        .checked_add(old_size)
        .ok_or_else(|| Error::Limit("OfficeArt delay-stream offset overflow".into()))?;
      new_offset = new_offset
        .checked_add(new_size)
        .ok_or_else(|| Error::Limit("OfficeArt delay-stream offset overflow".into()))?;
    }
    *self = rebuilt;
    Ok(OfficeArtBStoreDelayLayout { file_blocks })
  }
}

impl OfficeArtBStoreDelayLayout {
  pub fn file_block_at_old_offset(
    &self,
    old_offset: u32,
  ) -> Option<&OfficeArtBStoreDelayFileBlockLayout> {
    self
      .file_blocks
      .iter()
      .find(|file_block| file_block.old_offset == old_offset)
  }

  pub fn changed(&self) -> bool {
    self.file_blocks.iter().any(|file_block| {
      file_block.old_offset != file_block.new_offset || file_block.old_size != file_block.new_size
    })
  }
}

fn is_bstore_delay_file_block(record_type: u16) -> bool {
  record_type == 0xf007 || (0xf018..=0xf117).contains(&record_type)
}

impl OfficeArtPartialStream {
  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits, reason: String) -> Result<Self> {
    if bytes.len() > limits.max_allocation {
      return Err(Error::Limit(format!(
        "OfficeArt partial stream length exceeds {}",
        limits.max_allocation
      )));
    }
    let mut record_count = 0usize;
    let sequence = parse_partial_sequence(bytes, 0, &mut record_count, limits)?;
    if !sequence.is_incomplete() {
      return Err(Error::invalid(
        0,
        "OfficeArt partial parser found no incomplete record",
      ));
    }
    Ok(Self { sequence, reason })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.sequence.to_bytes()
  }

  pub fn available_len(&self) -> usize {
    self.sequence.encoded_len()
  }

  pub fn complete_record_count(&self) -> usize {
    self.sequence.complete_record_count()
  }

  pub fn incomplete_record_count(&self) -> usize {
    self.sequence.incomplete_record_count()
  }

  pub fn unparsed_byte_count(&self) -> usize {
    self.sequence.unparsed_byte_count()
  }

  pub fn visit_complete(&self, mut visitor: impl FnMut(&OfficeArtRecord)) {
    self.sequence.visit_complete(&mut visitor);
  }

  pub fn visit_incomplete(&self, mut visitor: impl FnMut(&OfficeArtIncompleteRecord)) {
    self.sequence.visit_incomplete(&mut visitor);
  }

  pub fn trailing_header_lengths(&self) -> Vec<usize> {
    let mut lengths = Vec::new();
    self.sequence.collect_trailing_header_lengths(&mut lengths);
    lengths
  }
}

impl OfficeArtPartialSequence {
  fn is_incomplete(&self) -> bool {
    !self.trailing_header.is_empty()
      || self
        .records
        .iter()
        .any(|record| matches!(record, OfficeArtPartialRecord::Incomplete(_)))
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(self.encoded_len());
    for record in &self.records {
      record.write(&mut bytes)?;
    }
    bytes.extend_from_slice(&self.trailing_header);
    Ok(bytes)
  }

  fn encoded_len(&self) -> usize {
    self
      .records
      .iter()
      .map(OfficeArtPartialRecord::encoded_len)
      .sum::<usize>()
      .saturating_add(self.trailing_header.len())
  }

  fn complete_record_count(&self) -> usize {
    self
      .records
      .iter()
      .map(OfficeArtPartialRecord::complete_record_count)
      .sum()
  }

  fn incomplete_record_count(&self) -> usize {
    self
      .records
      .iter()
      .map(OfficeArtPartialRecord::incomplete_record_count)
      .sum()
  }

  fn unparsed_byte_count(&self) -> usize {
    self.trailing_header.len()
      + self
        .records
        .iter()
        .map(OfficeArtPartialRecord::unparsed_byte_count)
        .sum::<usize>()
  }

  fn visit_complete(&self, visitor: &mut impl FnMut(&OfficeArtRecord)) {
    for record in &self.records {
      match record {
        OfficeArtPartialRecord::Complete(record) => {
          visit_complete_tree(record, visitor);
        }
        OfficeArtPartialRecord::Incomplete(record) => match &record.data {
          OfficeArtIncompleteRecordData::Container(sequence)
          | OfficeArtIncompleteRecordData::RecoveredSequence { sequence, .. } => {
            sequence.visit_complete(visitor);
          }
          OfficeArtIncompleteRecordData::FbseWithUnderreportedLength(fbse) => {
            if let Some(blip) = fbse.embedded_blip.as_deref() {
              visit_complete_tree(blip, visitor);
            }
          }
          OfficeArtIncompleteRecordData::PropertyTable(table) => {
            if let Some(sequence) = table.recovered_trailing.as_deref() {
              sequence.visit_complete(visitor);
            }
          }
          _ => {}
        },
      }
    }
  }

  fn visit_incomplete(&self, visitor: &mut impl FnMut(&OfficeArtIncompleteRecord)) {
    for record in &self.records {
      if let OfficeArtPartialRecord::Incomplete(record) = record {
        visitor(record);
        match &record.data {
          OfficeArtIncompleteRecordData::Container(sequence)
          | OfficeArtIncompleteRecordData::RecoveredSequence { sequence, .. } => {
            sequence.visit_incomplete(visitor);
          }
          OfficeArtIncompleteRecordData::FbseWithUnderreportedLength(_) => {}
          OfficeArtIncompleteRecordData::PropertyTable(table) => {
            if let Some(sequence) = table.recovered_trailing.as_deref() {
              sequence.visit_incomplete(visitor);
            }
          }
          _ => {}
        }
      }
    }
  }

  fn collect_trailing_header_lengths(&self, lengths: &mut Vec<usize>) {
    if !self.trailing_header.is_empty() {
      lengths.push(self.trailing_header.len());
    }
    for record in &self.records {
      if let OfficeArtPartialRecord::Incomplete(record) = record {
        match &record.data {
          OfficeArtIncompleteRecordData::Container(sequence)
          | OfficeArtIncompleteRecordData::RecoveredSequence { sequence, .. } => {
            sequence.collect_trailing_header_lengths(lengths);
          }
          OfficeArtIncompleteRecordData::FbseWithUnderreportedLength(_) => {}
          OfficeArtIncompleteRecordData::PropertyTable(table) => {
            if let Some(sequence) = table.recovered_trailing.as_deref() {
              sequence.collect_trailing_header_lengths(lengths);
            }
          }
          _ => {}
        }
      }
    }
  }
}

impl OfficeArtPartialRecord {
  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    match self {
      Self::Complete(record) => record.write(bytes),
      Self::Incomplete(record) => record.write(bytes),
    }
  }

  fn encoded_len(&self) -> usize {
    match self {
      Self::Complete(record) => HEADER_LEN + record.header.declared_length as usize,
      Self::Incomplete(record) => HEADER_LEN + record.data.available_len(),
    }
  }

  fn complete_record_count(&self) -> usize {
    match self {
      Self::Complete(record) => complete_tree_record_count(record),
      Self::Incomplete(record) => match &record.data {
        OfficeArtIncompleteRecordData::Container(sequence) => sequence.complete_record_count(),
        OfficeArtIncompleteRecordData::FbseWithUnderreportedLength(fbse) => fbse
          .embedded_blip
          .as_deref()
          .map_or(0, complete_tree_record_count),
        OfficeArtIncompleteRecordData::RecoveredSequence { sequence, .. } => {
          sequence.complete_record_count()
        }
        OfficeArtIncompleteRecordData::PropertyTable(table) => table
          .recovered_trailing
          .as_deref()
          .map_or(0, OfficeArtPartialSequence::complete_record_count),
        OfficeArtIncompleteRecordData::Atom { .. } => 0,
      },
    }
  }

  fn incomplete_record_count(&self) -> usize {
    match self {
      Self::Complete(_) => 0,
      Self::Incomplete(record) => {
        1 + match &record.data {
          OfficeArtIncompleteRecordData::Container(sequence) => sequence.incomplete_record_count(),
          OfficeArtIncompleteRecordData::FbseWithUnderreportedLength(_) => 0,
          OfficeArtIncompleteRecordData::RecoveredSequence { sequence, .. } => {
            sequence.incomplete_record_count()
          }
          OfficeArtIncompleteRecordData::PropertyTable(table) => table
            .recovered_trailing
            .as_deref()
            .map_or(0, OfficeArtPartialSequence::incomplete_record_count),
          OfficeArtIncompleteRecordData::Atom { .. } => 0,
        }
      }
    }
  }

  fn unparsed_byte_count(&self) -> usize {
    match self {
      Self::Complete(record) => complete_tree_unparsed_byte_count(record),
      Self::Incomplete(record) => match &record.data {
        OfficeArtIncompleteRecordData::Container(sequence) => sequence.unparsed_byte_count(),
        OfficeArtIncompleteRecordData::FbseWithUnderreportedLength(fbse) => {
          fbse.trailing.len()
            + fbse
              .embedded_blip
              .as_deref()
              .map_or(0, complete_tree_unparsed_byte_count)
        }
        OfficeArtIncompleteRecordData::RecoveredSequence { prefix, sequence } => {
          prefix.unparsed_byte_count() + sequence.unparsed_byte_count()
        }
        OfficeArtIncompleteRecordData::PropertyTable(table) => {
          table.incomplete_fixed_entry.unparsed_byte_count()
            + table.unparsed_complex_len()
            + table
              .recovered_trailing
              .as_deref()
              .map_or(0, OfficeArtPartialSequence::unparsed_byte_count)
        }
        OfficeArtIncompleteRecordData::Atom { available_payload } => available_payload.len(),
      },
    }
  }
}

fn complete_tree_record_count(record: &OfficeArtRecord) -> usize {
  1 + match &record.data {
    OfficeArtRecordData::Container(children)
    | OfficeArtRecordData::CompatibilityContainer(children) => children
      .iter()
      .map(complete_tree_record_count)
      .sum::<usize>(),
    OfficeArtRecordData::Fbse(fbse) => fbse
      .embedded_blip
      .as_deref()
      .map_or(0, complete_tree_record_count),
    OfficeArtRecordData::IncompletePropertyTable(table) => table
      .recovered_trailing
      .as_deref()
      .map_or(0, OfficeArtPartialSequence::complete_record_count),
    _ => 0,
  }
}

fn visit_complete_tree(record: &OfficeArtRecord, visitor: &mut impl FnMut(&OfficeArtRecord)) {
  visitor(record);
  match &record.data {
    OfficeArtRecordData::Container(children)
    | OfficeArtRecordData::CompatibilityContainer(children) => {
      for child in children {
        visit_complete_tree(child, visitor);
      }
    }
    OfficeArtRecordData::Fbse(fbse) => {
      if let Some(blip) = fbse.embedded_blip.as_deref() {
        visit_complete_tree(blip, visitor);
      }
    }
    OfficeArtRecordData::IncompletePropertyTable(table) => {
      if let Some(sequence) = table.recovered_trailing.as_deref() {
        sequence.visit_complete(visitor);
      }
    }
    _ => {}
  }
}

fn complete_tree_unparsed_byte_count(record: &OfficeArtRecord) -> usize {
  match &record.data {
    OfficeArtRecordData::Atom(payload) => payload.len(),
    OfficeArtRecordData::Container(children)
    | OfficeArtRecordData::CompatibilityContainer(children) => {
      children.iter().map(complete_tree_unparsed_byte_count).sum()
    }
    OfficeArtRecordData::Fbse(fbse) => {
      fbse.trailing.len()
        + fbse
          .embedded_blip
          .as_deref()
          .map_or(0, complete_tree_unparsed_byte_count)
    }
    OfficeArtRecordData::SoftMakerNativeProperties(value) => value
      .properties
      .iter()
      .map(|property| property.data.unparsed_byte_count())
      .sum(),
    OfficeArtRecordData::IncompletePropertyTable(value) => {
      value.incomplete_fixed_entry.unparsed_byte_count()
        + value.unparsed_complex_len()
        + value
          .recovered_trailing
          .as_deref()
          .map_or(0, OfficeArtPartialSequence::unparsed_byte_count)
    }
    _ => 0,
  }
}

impl OfficeArtIncompleteRecord {
  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    let available_len = self.data.available_len();
    if !matches!(
      &self.data,
      OfficeArtIncompleteRecordData::FbseWithUnderreportedLength(_)
    ) && u64::try_from(available_len).unwrap_or(u64::MAX)
      > u64::from(self.header.declared_length)
    {
      return Err(Error::invalid(
        0,
        "OfficeArt incomplete payload exceeds declared length",
      ));
    }
    self.header.append_to(bytes)?;
    match &self.data {
      OfficeArtIncompleteRecordData::Container(sequence) => {
        bytes.extend_from_slice(&sequence.to_bytes()?);
      }
      OfficeArtIncompleteRecordData::FbseWithUnderreportedLength(fbse) => {
        fbse.write(bytes)?;
      }
      OfficeArtIncompleteRecordData::PropertyTable(table) => table.write(bytes)?,
      OfficeArtIncompleteRecordData::RecoveredSequence { prefix, sequence } => {
        prefix.write(bytes);
        bytes.extend_from_slice(&sequence.to_bytes()?);
      }
      OfficeArtIncompleteRecordData::Atom { available_payload } => {
        bytes.extend_from_slice(available_payload);
      }
    }
    Ok(())
  }
}

impl OfficeArtIncompleteRecordData {
  fn available_len(&self) -> usize {
    match self {
      Self::Container(sequence) => sequence.encoded_len(),
      Self::FbseWithUnderreportedLength(fbse) => fbse.encoded_len(),
      Self::PropertyTable(table) => {
        table.entries.len() * 6
          + table.incomplete_fixed_entry.encoded_len()
          + table.available_complex_len()
          + table
            .recovered_trailing
            .as_deref()
            .map_or(0, OfficeArtPartialSequence::encoded_len)
      }
      Self::RecoveredSequence { prefix, sequence } => prefix.encoded_len() + sequence.encoded_len(),
      Self::Atom { available_payload } => available_payload.len(),
    }
  }
}

impl OfficeArtRecord {
  /// Returns this record's supported image payload without copying it.
  ///
  /// `None` means that the record is not a supported typed BLIP. In
  /// particular, opaque metafile payloads remain distinguishable through
  /// [`OfficeArtRecordData::MetafileBlip`] and are never guessed here.
  pub fn image_ref(&self) -> Option<OfficeArtImageRef<'_>> {
    let (format, data) = match &self.data {
      OfficeArtRecordData::BitmapBlip(blip) => {
        let format = match self.header.record_type {
          0xf01d | 0xf02a => OfficeArtImageFormat::Jpeg,
          0xf01e => OfficeArtImageFormat::Png,
          0xf01f => OfficeArtImageFormat::Dib,
          0xf029 => OfficeArtImageFormat::Tiff,
          _ => return None,
        };
        let data = match &blip.file_data {
          OfficeArtBitmapData::Dib(data) | OfficeArtBitmapData::Encoded(data) => data.as_slice(),
        };
        (format, data)
      }
      OfficeArtRecordData::MetafileBlip(blip) => match &blip.file_data {
        OfficeArtMetafileData::Emf(data) => (OfficeArtImageFormat::Emf, data.decoded()),
        OfficeArtMetafileData::Wmf(data) => (OfficeArtImageFormat::Wmf, data.decoded()),
        OfficeArtMetafileData::Pict(data) => (OfficeArtImageFormat::Pict, data.decoded()),
        OfficeArtMetafileData::Opaque { .. } => return None,
      },
      _ => return None,
    };
    Some(OfficeArtImageRef { format, data })
  }

  fn direct_children(&self) -> Result<Option<&[OfficeArtRecord]>> {
    match &self.data {
      OfficeArtRecordData::Container(children) => {
        if self.header.version != 0x0f {
          return Err(Error::invalid(0, "OfficeArt container version is not 0xF"));
        }
        Ok(Some(children))
      }
      OfficeArtRecordData::CompatibilityContainer(children) => {
        if self.header.version == 0x0f {
          return Err(Error::invalid(
            0,
            "OfficeArt compatibility container uses standard recVer 0xF",
          ));
        }
        Ok(Some(children))
      }
      _ => Ok(None),
    }
  }

  /// Recomputes this record's payload length from its typed descendants.
  pub fn relayout(&mut self) -> Result<()> {
    match &mut self.data {
      OfficeArtRecordData::Container(children) => {
        for child in children.iter_mut() {
          child.relayout()?;
        }
        if self.header.record_type == 0xf001 {
          if children
            .iter()
            .any(|child| !is_bstore_delay_file_block(child.header.record_type))
          {
            return Err(Error::invalid(
              0,
              "OfficeArtBStoreContainer contains an invalid file-block record type",
            ));
          }
          self.header.instance =
            record_instance_from_len(children.len(), "OfficeArtBStoreContainerFileBlock")?;
        }
      }
      OfficeArtRecordData::CompatibilityContainer(children) => {
        for child in children {
          child.relayout()?;
        }
      }
      OfficeArtRecordData::Fbse(value) => {
        if let Some(blip) = &mut value.embedded_blip {
          let old_size = blip
            .header
            .declared_length
            .checked_add(HEADER_LEN as u32)
            .ok_or_else(|| Error::Limit("embedded BLIP size overflow".into()))?;
          blip.relayout()?;
          let new_size = blip
            .header
            .declared_length
            .checked_add(HEADER_LEN as u32)
            .ok_or_else(|| Error::Limit("embedded BLIP size overflow".into()))?;
          if old_size != new_size {
            value.declared_blip_size = new_size;
          }
        }
      }
      OfficeArtRecordData::DggBlock(value) => value.relayout()?,
      OfficeArtRecordData::MetafileBlip(value) => value.relayout()?,
      OfficeArtRecordData::Frit(values) => {
        self.header.instance = record_instance_from_len(values.len(), "OfficeArtFRIT")?;
      }
      OfficeArtRecordData::ColorMru(colors) => {
        self.header.instance = record_instance_from_len(colors.len(), "MSOCR")?;
      }
      OfficeArtRecordData::PropertyTable(value)
        if matches!(self.header.record_type, 0xf00b | 0xf121 | 0xf122) =>
      {
        value.relayout()?;
        self.header.instance = record_instance_from_len(value.properties.len(), "OfficeArtFOPTE")?;
      }
      _ => {}
    }
    let payload_len = if let Some(children) = self.direct_children()? {
      complete_records_encoded_len(children)?
    } else {
      self.payload_bytes()?.len()
    };
    self.header.declared_length = u32::try_from(payload_len)
      .map_err(|_| Error::Limit("OfficeArt record payload exceeds u32".into()))?;
    Ok(())
  }

  fn write_payload(&self, payload: &mut Vec<u8>) -> Result<()> {
    if self.header.version > 0x0f || self.header.instance > 0x0fff {
      return Err(Error::invalid(
        0,
        "OfficeArt header bit fields exceed their width",
      ));
    }
    match &self.data {
      OfficeArtRecordData::Container(children) => {
        if self.header.version != 0x0f {
          return Err(Error::invalid(0, "OfficeArt container version is not 0xF"));
        }
        for child in children {
          child.write(payload)?;
        }
      }
      OfficeArtRecordData::CompatibilityContainer(children) => {
        if self.header.version == 0x0f {
          return Err(Error::invalid(
            0,
            "OfficeArt compatibility container uses standard recVer 0xF",
          ));
        }
        for child in children {
          child.write(payload)?;
        }
      }
      OfficeArtRecordData::Atom(value) => {
        if self.header.version == 0x0f {
          return Err(Error::invalid(
            0,
            "OfficeArt atom uses container version 0xF",
          ));
        }
        payload.extend_from_slice(value);
      }
      OfficeArtRecordData::ArcRule(value) => value.write(payload),
      OfficeArtRecordData::CalloutRule(value) => value.write(payload),
      OfficeArtRecordData::ChildAnchor(value) | OfficeArtRecordData::GroupShape(value) => {
        value.write(payload)
      }
      OfficeArtRecordData::ClientAnchor(value) => value.write(payload),
      OfficeArtRecordData::ClientMarker(_) => {}
      OfficeArtRecordData::ColorMru(colors) => {
        for color in colors {
          payload.extend_from_slice(&color.0.to_le_bytes());
        }
      }
      OfficeArtRecordData::ConnectorRule(value) => value.write(payload),
      OfficeArtRecordData::BitmapBlip(value) => value.write(payload)?,
      OfficeArtRecordData::Drawing(value) => value.write(payload),
      OfficeArtRecordData::DggBlock(value) => value.write(payload)?,
      OfficeArtRecordData::EmptyCompatibilityAtom => {}
      OfficeArtRecordData::Fbse(value) => value.write(payload)?,
      OfficeArtRecordData::Frit(values) => {
        for value in values {
          value.write(payload);
        }
      }
      OfficeArtRecordData::IncompletePropertyTable(value) => value.write(payload)?,
      OfficeArtRecordData::MetafileBlip(value) => value.write(payload)?,
      OfficeArtRecordData::PropertyTable(value) => value.write(payload)?,
      OfficeArtRecordData::Shape(value) => value.write(payload),
      OfficeArtRecordData::SoftMakerNativeProperties(value) => value.write(payload)?,
      OfficeArtRecordData::SplitMenuColors(colors) => {
        for color in colors {
          payload.extend_from_slice(&color.to_le_bytes());
        }
      }
      OfficeArtRecordData::WordClientAnchor(value) | OfficeArtRecordData::WordClientData(value) => {
        payload.extend_from_slice(&value.to_le_bytes());
      }
      OfficeArtRecordData::WordClientTextbox(value) => {
        payload.extend_from_slice(&value.chain_index.to_le_bytes());
        payload.extend_from_slice(&value.story_index.to_le_bytes());
      }
    }
    Ok(())
  }

  pub(crate) fn payload_bytes(&self) -> Result<Vec<u8>> {
    let capacity = usize::try_from(self.header.declared_length)
      .map_err(|_| Error::Limit("OfficeArt record length exceeds usize".into()))?;
    let mut payload = Vec::with_capacity(capacity);
    self.write_payload(&mut payload)?;
    Ok(payload)
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    let record_start = bytes.len();
    self.header.append_to(bytes)?;
    let payload_start = bytes.len();
    if let Err(error) = self.write_payload(bytes) {
      bytes.truncate(record_start);
      return Err(error);
    }
    let payload_len = bytes.len() - payload_start;
    if usize::try_from(self.header.declared_length).ok() != Some(payload_len) {
      bytes.truncate(record_start);
      return Err(Error::invalid(
        0,
        "OfficeArt declared length does not match payload",
      ));
    }
    Ok(())
  }

  fn write_to(&self, writer: &mut dyn Write) -> Result<()> {
    if self.header.version > 0x0f || self.header.instance > 0x0fff {
      return Err(Error::invalid(
        0,
        "OfficeArt header bit fields exceed their width",
      ));
    }
    self.header.emit_to(writer)?;
    let mut payload = CountingWriter::new(writer);
    match &self.data {
      OfficeArtRecordData::Container(children) => {
        if self.header.version != 0x0f {
          return Err(Error::invalid(0, "OfficeArt container version is not 0xF"));
        }
        write_complete_records(children, &mut payload)?;
      }
      OfficeArtRecordData::CompatibilityContainer(children) => {
        if self.header.version == 0x0f {
          return Err(Error::invalid(
            0,
            "OfficeArt compatibility container uses standard recVer 0xF",
          ));
        }
        write_complete_records(children, &mut payload)?;
      }
      OfficeArtRecordData::Atom(value) => payload.write_all(value)?,
      OfficeArtRecordData::BitmapBlip(value) => value.write_to(&mut payload)?,
      OfficeArtRecordData::MetafileBlip(value) => value.write_to(&mut payload)?,
      OfficeArtRecordData::Fbse(value) => value.write_to(&mut payload)?,
      _ => {
        let body = self.payload_bytes()?;
        payload.write_all(&body)?;
      }
    }
    let expected = usize::try_from(self.header.declared_length)
      .map_err(|_| Error::Limit("OfficeArt record length exceeds usize".into()))?;
    if payload.written != expected {
      return Err(Error::invalid(
        0,
        "OfficeArt declared length does not match payload",
      ));
    }
    Ok(())
  }
}

/// Borrows an image payload from one complete encoded OfficeArt BLIP record.
/// Bitmap formats and uncompressed metafiles require no allocation. A
/// compressed metafile returns `None`; callers that need it should use the
/// typed [`OfficeArtRecord`] parser, which owns the decoded payload.
pub fn image_ref_from_record_bytes(bytes: &[u8]) -> Result<Option<OfficeArtImageRef<'_>>> {
  let header = OfficeArtRecordHeader::read_slice(bytes)?;
  let payload_len = usize::try_from(header.declared_length)
    .map_err(|_| Error::Limit("OfficeArt BLIP length exceeds usize".into()))?;
  let record_len = HEADER_LEN
    .checked_add(payload_len)
    .ok_or_else(|| Error::Limit("OfficeArt BLIP record length overflow".into()))?;
  let payload = bytes
        .get(HEADER_LEN..record_len)
        .ok_or_else(|| {
            Error::invalid(
                0,
                format!(
                    "delayed OfficeArt record {:#06x}/instance {:#05x} declares {} payload bytes, only {} are available",
                    header.record_type,
                    header.instance,
                    payload_len,
                    bytes.len().saturating_sub(HEADER_LEN)
                ),
            )
        })?;
  if let Some(uid_count) = bitmap_uid_count(header.record_type, header.instance) {
    let prefix_len = uid_count
      .checked_mul(16)
      .and_then(|value| value.checked_add(1))
      .ok_or_else(|| Error::Limit("OfficeArt bitmap prefix length overflow".into()))?;
    let data = payload
      .get(prefix_len..)
      .ok_or_else(|| Error::invalid(0, "delayed OfficeArt bitmap prefix is truncated"))?;
    let format = match header.record_type {
      0xf01d | 0xf02a => OfficeArtImageFormat::Jpeg,
      0xf01e => OfficeArtImageFormat::Png,
      0xf01f => OfficeArtImageFormat::Dib,
      0xf029 => OfficeArtImageFormat::Tiff,
      _ => unreachable!("bitmap UID validation accepted an unknown record type"),
    };
    return Ok(Some(OfficeArtImageRef { format, data }));
  }
  let Some(uid_count) = metafile_uid_count(header.record_type, header.instance) else {
    return Ok(None);
  };
  let uid_len = uid_count
    .checked_mul(16)
    .ok_or_else(|| Error::Limit("OfficeArt metafile UID length overflow".into()))?;
  let header_end = uid_len
    .checked_add(34)
    .ok_or_else(|| Error::Limit("OfficeArt metafile header length overflow".into()))?;
  let metafile_header = payload
    .get(uid_len..header_end)
    .and_then(OfficeArtMetafileHeader::parse)
    .ok_or_else(|| Error::invalid(0, "delayed OfficeArt metafile header is truncated"))?;
  if metafile_header.compression != 0xfe {
    return Ok(None);
  }
  let format = match header.record_type {
    0xf01a => OfficeArtImageFormat::Emf,
    0xf01b => OfficeArtImageFormat::Wmf,
    0xf01c => OfficeArtImageFormat::Pict,
    _ => unreachable!("metafile UID validation accepted an unknown record type"),
  };
  Ok(Some(OfficeArtImageRef {
    format,
    data: &payload[header_end..],
  }))
}

struct CountingWriter<'a> {
  inner: &'a mut dyn Write,
  written: usize,
}

impl<'a> CountingWriter<'a> {
  fn new(inner: &'a mut dyn Write) -> Self {
    Self { inner, written: 0 }
  }
}

impl Write for CountingWriter<'_> {
  fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
    let written = self.inner.write(bytes)?;
    self.written = self.written.saturating_add(written);
    Ok(written)
  }

  fn flush(&mut self) -> std::io::Result<()> {
    self.inner.flush()
  }
}

fn complete_records_encoded_len(records: &[OfficeArtRecord]) -> Result<usize> {
  records.iter().try_fold(0usize, |length, record| {
    let payload_len = usize::try_from(record.header.declared_length)
      .map_err(|_| Error::Limit("OfficeArt record length exceeds usize".into()))?;
    length
      .checked_add(HEADER_LEN)
      .and_then(|length| length.checked_add(payload_len))
      .ok_or_else(|| Error::Limit("OfficeArt stream length overflow".into()))
  })
}

fn encode_complete_records(records: &[OfficeArtRecord]) -> Result<Vec<u8>> {
  let mut bytes = Vec::with_capacity(complete_records_encoded_len(records)?);
  for record in records {
    record.write(&mut bytes)?;
  }
  Ok(bytes)
}

fn write_complete_records(records: &[OfficeArtRecord], writer: &mut dyn Write) -> Result<()> {
  for record in records {
    record.write_to(writer)?;
  }
  Ok(())
}

fn record_instance_from_len(len: usize, structure: &str) -> Result<u16> {
  let instance =
    u16::try_from(len).map_err(|_| Error::Limit(format!("{structure} count exceeds u16")))?;
  if instance > 0x0fff {
    return Err(Error::Limit(format!(
      "{structure} count exceeds OfficeArt recInstance"
    )));
  }
  Ok(instance)
}

fn parse_records(
  bytes: &[u8],
  depth: usize,
  record_count: &mut usize,
  limits: Limits,
) -> Result<Vec<OfficeArtRecord>> {
  if depth > MAX_CONTAINER_DEPTH {
    return Err(Error::Limit(format!(
      "OfficeArt container depth exceeds {MAX_CONTAINER_DEPTH}"
    )));
  }
  let mut cursor = 0usize;
  let mut records = Vec::new();
  while cursor < bytes.len() {
    let (record, consumed) = parse_one_record(&bytes[cursor..], depth, record_count, limits)?;
    cursor = cursor
      .checked_add(consumed)
      .ok_or_else(|| Error::Limit("OfficeArt record offset overflow".into()))?;
    records.push(record);
  }
  Ok(records)
}

fn parse_partial_sequence(
  bytes: &[u8],
  depth: usize,
  record_count: &mut usize,
  limits: Limits,
) -> Result<OfficeArtPartialSequence> {
  if depth > MAX_CONTAINER_DEPTH {
    return Err(Error::Limit(format!(
      "OfficeArt partial container depth exceeds {MAX_CONTAINER_DEPTH}"
    )));
  }
  let mut cursor = 0usize;
  let mut records = Vec::new();
  while bytes.len().saturating_sub(cursor) >= HEADER_LEN {
    let remaining = &bytes[cursor..];
    let mut complete_count = *record_count;
    if let Ok((record, consumed)) = parse_one_record(remaining, depth, &mut complete_count, limits)
    {
      if let Some(recovered_payload_len) =
        underreported_fbse_payload_len(&record, remaining, limits)
      {
        if *record_count >= limits.max_entries {
          return Err(Error::Limit(format!(
            "OfficeArt partial record count exceeds {}",
            limits.max_entries
          )));
        }
        let mut recovered_count = *record_count + 1;
        let recovered_payload = &remaining[HEADER_LEN..HEADER_LEN + recovered_payload_len];
        if let Some(fbse) =
          OfficeArtFbse::parse(recovered_payload, depth, &mut recovered_count, limits)?
          && fbse.embedded_blip.is_some()
          && fbse.trailing.is_empty()
        {
          *record_count = recovered_count;
          cursor = cursor
            .checked_add(HEADER_LEN + recovered_payload_len)
            .ok_or_else(|| Error::Limit("OfficeArt partial offset overflow".into()))?;
          records.push(OfficeArtPartialRecord::Incomplete(
            OfficeArtIncompleteRecord {
              header: record.header,
              data: OfficeArtIncompleteRecordData::FbseWithUnderreportedLength(fbse),
            },
          ));
          continue;
        }
      }
      *record_count = complete_count;
      cursor = cursor
        .checked_add(consumed)
        .ok_or_else(|| Error::Limit("OfficeArt partial offset overflow".into()))?;
      records.push(OfficeArtPartialRecord::Complete(record));
      continue;
    }

    if *record_count >= limits.max_entries {
      return Err(Error::Limit(format!(
        "OfficeArt partial record count exceeds {}",
        limits.max_entries
      )));
    }
    *record_count += 1;
    let header = OfficeArtRecordHeader::read_slice(remaining)?;
    let declared_len = usize::try_from(header.declared_length).unwrap_or(usize::MAX);
    let available_len = declared_len.min(remaining.len() - HEADER_LEN);
    let available_payload = &remaining[HEADER_LEN..HEADER_LEN + available_len];
    let data = if header.version == 0x0f {
      OfficeArtIncompleteRecordData::Container(parse_partial_sequence(
        available_payload,
        depth + 1,
        record_count,
        limits,
      )?)
    } else if let Some(prefix_len) = match (
      header.version,
      header.instance,
      header.record_type,
      available_payload.len(),
    ) {
      (0, 0x001, 0x0000, 26) => Some(18),
      (2, 0x0aa, 0x0032, 98) => Some(8),
      _ => None,
    } {
      OfficeArtIncompleteRecordData::RecoveredSequence {
        prefix: if prefix_len == 18 {
          OfficeArtRecoveredPrefix::ClientAnchor(
            OfficeArtClientAnchor::parse(&available_payload[..prefix_len])
              .expect("validated 18-byte client anchor"),
          )
        } else {
          OfficeArtRecoveredPrefix::Words2(std::array::from_fn(|index| {
            let start = index * 4;
            u32::from_le_bytes(
              available_payload[start..start + 4]
                .try_into()
                .expect("validated 8-byte damaged prefix"),
            )
          }))
        },
        sequence: parse_partial_sequence(
          &available_payload[prefix_len..],
          depth + 1,
          record_count,
          limits,
        )?,
      }
    } else if header.record_type == 0xf00b {
      let property_count = usize::from(header.instance);
      let mut table =
        OfficeArtIncompletePropertyTable::parse_partial(available_payload, property_count);
      if table.entries.len() == property_count
        && table.entries.iter().all(|entry| !entry.is_complex)
        && table.complex_fragments.is_empty()
        && !table.trailing_data.is_empty()
      {
        let mut nested_count = *record_count;
        if let Ok(sequence) =
          parse_partial_sequence(&table.trailing_data, depth + 1, &mut nested_count, limits)
          && !sequence.records.is_empty()
          && sequence.encoded_len() == table.trailing_data.len()
        {
          *record_count = nested_count;
          table.trailing_data.clear();
          table.recovered_trailing = Some(Box::new(sequence));
        }
      }
      OfficeArtIncompleteRecordData::PropertyTable(table)
    } else {
      OfficeArtIncompleteRecordData::Atom {
        available_payload: available_payload.to_vec(),
      }
    };
    records.push(OfficeArtPartialRecord::Incomplete(
      OfficeArtIncompleteRecord { header, data },
    ));
    cursor = cursor
      .checked_add(HEADER_LEN + available_len)
      .ok_or_else(|| Error::Limit("OfficeArt partial offset overflow".into()))?;
    if available_len < declared_len {
      break;
    }
  }
  Ok(OfficeArtPartialSequence {
    records,
    trailing_header: bytes[cursor..].to_vec(),
  })
}

fn underreported_fbse_payload_len(
  record: &OfficeArtRecord,
  remaining: &[u8],
  limits: Limits,
) -> Option<usize> {
  let OfficeArtRecordData::Fbse(fbse) = &record.data else {
    return None;
  };
  let embedded_len = usize::try_from(fbse.declared_blip_size).ok()?;
  let name_end = 36usize.checked_add(usize::from(fbse.declared_name_length))?;
  let recovered_payload_len = name_end.checked_add(embedded_len)?;
  if recovered_payload_len <= usize::try_from(record.header.declared_length).ok()?
    || recovered_payload_len > limits.max_allocation
    || HEADER_LEN.checked_add(recovered_payload_len)? > remaining.len()
  {
    return None;
  }

  let embedded = remaining.get(HEADER_LEN + name_end..HEADER_LEN + recovered_payload_len)?;
  if embedded.len() < HEADER_LEN || !is_blip_record_prefix(embedded) {
    return None;
  }
  let nested_declared_len = usize::try_from(u32::from_le_bytes(
    embedded[4..8].try_into().expect("four bytes"),
  ))
  .ok()?;
  (HEADER_LEN.checked_add(nested_declared_len)? == embedded_len).then_some(recovered_payload_len)
}

fn parse_one_record(
  bytes: &[u8],
  depth: usize,
  record_count: &mut usize,
  limits: Limits,
) -> Result<(OfficeArtRecord, usize)> {
  if depth > MAX_CONTAINER_DEPTH {
    return Err(Error::Limit(format!(
      "OfficeArt container depth exceeds {MAX_CONTAINER_DEPTH}"
    )));
  }
  let header = OfficeArtRecordHeader::read_slice(bytes)?;
  let payload_len = usize::try_from(header.declared_length)
    .map_err(|_| Error::Limit("OfficeArt payload length exceeds usize".into()))?;
  if payload_len > limits.max_allocation {
    return Err(Error::Limit(format!(
      "OfficeArt payload length exceeds {}",
      limits.max_allocation
    )));
  }
  let payload_end = HEADER_LEN
    .checked_add(payload_len)
    .ok_or_else(|| Error::Limit("OfficeArt payload offset overflow".into()))?;
  let payload = bytes
    .get(HEADER_LEN..payload_end)
    .ok_or_else(|| Error::invalid(4, "OfficeArt declared payload is truncated"))?;
  *record_count += 1;
  if *record_count > limits.max_entries {
    return Err(Error::Limit(format!(
      "OfficeArt record count exceeds {}",
      limits.max_entries
    )));
  }
  let data = if header.version == 0x0f {
    OfficeArtRecordData::Container(parse_records(payload, depth + 1, record_count, limits)?)
  } else if matches!(header.record_type, 0x0000 | 0xf002 | 0xf0f4) {
    let mut compatibility_count = *record_count;
    match parse_records(payload, depth + 1, &mut compatibility_count, limits) {
      Ok(children) => {
        *record_count = compatibility_count;
        OfficeArtRecordData::CompatibilityContainer(children)
      }
      Err(_) => parse_typed_atom(header, payload, depth, record_count, limits)?
        .unwrap_or_else(|| OfficeArtRecordData::Atom(payload.to_vec())),
    }
  } else {
    parse_typed_atom(header, payload, depth, record_count, limits)?
      .unwrap_or_else(|| OfficeArtRecordData::Atom(payload.to_vec()))
  };
  Ok((OfficeArtRecord { header, data }, payload_end))
}

fn parse_typed_atom(
  header: OfficeArtRecordHeader,
  payload: &[u8],
  depth: usize,
  record_count: &mut usize,
  limits: Limits,
) -> Result<Option<OfficeArtRecordData>> {
  let data = match header.record_type {
    0xf006 => OfficeArtDggBlock::parse(payload).map(OfficeArtRecordData::DggBlock),
    0xe007 | 0xf007 => {
      OfficeArtFbse::parse(payload, depth, record_count, limits)?.map(OfficeArtRecordData::Fbse)
    }
    0xf008 if payload.len() == 8 => Some(OfficeArtRecordData::Drawing(OfficeArtDrawing::parse(
      payload,
    ))),
    0xf009 if payload.len() == 16 => Some(OfficeArtRecordData::GroupShape(OfficeArtRect::parse(
      payload,
    ))),
    0x0000 | 0xf0aa if payload.len() == 16 => Some(OfficeArtRecordData::ChildAnchor(
      OfficeArtRect::parse(payload),
    )),
    0xf00a if payload.len() == 8 => {
      Some(OfficeArtRecordData::Shape(OfficeArtShape::parse(payload)))
    }
    0xf00b | 0xf121 | 0xf122 => {
      let property_count = usize::from(header.instance);
      OfficeArtPropertyTable::parse(payload, property_count)
        .map(OfficeArtRecordData::PropertyTable)
        .or_else(|| {
          OfficeArtIncompletePropertyTable::parse(payload, property_count)
            .map(OfficeArtRecordData::IncompletePropertyTable)
        })
        .or_else(|| {
          (payload.len() < property_count.saturating_mul(6)).then(|| {
            OfficeArtRecordData::IncompletePropertyTable(
              OfficeArtIncompletePropertyTable::parse_partial(payload, property_count),
            )
          })
        })
    }
    0xf043 if payload.len() == 48 => {
      OfficeArtPropertyTable::parse(payload, 8).map(OfficeArtRecordData::PropertyTable)
    }
    0xf051 | 0xf08d | 0xf10d if payload.is_empty() => {
      Some(OfficeArtRecordData::EmptyCompatibilityAtom)
    }
    0xf150 => SoftMakerNativeProperties::parse(payload, limits.max_entries)
      .map(OfficeArtRecordData::SoftMakerNativeProperties),
    0xf00d if payload.is_empty() => Some(OfficeArtRecordData::ClientMarker(
      OfficeArtClientMarker::Textbox,
    )),
    0xf00d if payload.len() == 16 => Some(OfficeArtRecordData::ChildAnchor(OfficeArtRect::parse(
      payload,
    ))),
    0xf00f if payload.len() == 16 => Some(OfficeArtRecordData::ChildAnchor(OfficeArtRect::parse(
      payload,
    ))),
    0xf010 => OfficeArtClientAnchor::parse(payload).map(OfficeArtRecordData::ClientAnchor),
    0xf011 if payload.is_empty() => Some(OfficeArtRecordData::ClientMarker(
      OfficeArtClientMarker::Data,
    )),
    0xf012 if payload.len() == 24 => Some(OfficeArtRecordData::ConnectorRule(
      OfficeArtConnectorRule::parse(payload),
    )),
    0xf014 if payload.len() == 8 => Some(OfficeArtRecordData::ArcRule(OfficeArtArcRule::parse(
      payload,
    ))),
    0xf017 if payload.len() == 8 => Some(OfficeArtRecordData::CalloutRule(
      OfficeArtCalloutRule::parse(payload),
    )),
    0xf11e if payload.len() == 16 => Some(OfficeArtRecordData::SplitMenuColors([
      u32::from_le_bytes(payload[0..4].try_into().expect("four bytes")),
      u32::from_le_bytes(payload[4..8].try_into().expect("four bytes")),
      u32::from_le_bytes(payload[8..12].try_into().expect("four bytes")),
      u32::from_le_bytes(payload[12..16].try_into().expect("four bytes")),
    ])),
    0xf118 if payload.len() == usize::from(header.instance) * 4 => Some(OfficeArtRecordData::Frit(
      payload.chunks_exact(4).map(OfficeArtFrit::parse).collect(),
    )),
    0xf11a if payload.len() == usize::from(header.instance) * 4 => {
      Some(OfficeArtRecordData::ColorMru(
        payload
          .chunks_exact(4)
          .map(|bytes| {
            OfficeArtColor(u32::from_le_bytes(
              bytes.try_into().expect("four-byte MSOCR"),
            ))
          })
          .collect(),
      ))
    }
    0xf01a..=0xf01c => metafile_uid_count(header.record_type, header.instance)
      .and_then(|uid_count| {
        OfficeArtMetafileBlip::parse(
          payload,
          uid_count,
          header.record_type,
          limits.max_allocation,
        )
      })
      .map(OfficeArtRecordData::MetafileBlip),
    0xf01d | 0xf01e | 0xf01f | 0xf029 | 0xf02a => {
      bitmap_uid_count(header.record_type, header.instance)
        .and_then(|uid_count| OfficeArtBitmapBlip::parse(payload, uid_count, header.record_type))
        .map(OfficeArtRecordData::BitmapBlip)
    }
    _ => None,
  };
  Ok(data)
}

fn metafile_uid_count(record_type: u16, instance: u16) -> Option<usize> {
  match (record_type, instance) {
    (0xf01a, 0x3d4) | (0xf01b, 0x216) | (0xf01c, 0x542) => Some(1),
    (0xf01a, 0x3d5) | (0xf01b, 0x217) | (0xf01c, 0x543) => Some(2),
    _ => None,
  }
}

fn bitmap_uid_count(record_type: u16, instance: u16) -> Option<usize> {
  match (record_type, instance) {
    (0xf01d | 0xf02a, 0x46a | 0x6e2) | (0xf01e, 0x6e0) | (0xf01f, 0x7a8) | (0xf029, 0x6e4) => {
      Some(1)
    }
    (0xf01d | 0xf02a, 0x46b | 0x6e3) | (0xf01e, 0x6e1) | (0xf01f, 0x7a9) | (0xf029, 0x6e5) => {
      Some(2)
    }
    _ => None,
  }
}

impl OfficeArtBitmapBlip {
  fn parse(payload: &[u8], uid_count: usize, record_type: u16) -> Option<Self> {
    let prefix_len = uid_count.checked_mul(16)?.checked_add(1)?;
    let prefix = payload.get(..prefix_len)?;
    let encoded = payload[prefix_len..].to_vec();
    let file_data = if record_type == 0xf01f {
      OfficeArtBitmapData::Dib(encoded)
    } else {
      OfficeArtBitmapData::Encoded(encoded)
    };
    Some(Self {
      uid1: prefix[..16].try_into().ok()?,
      uid2: (uid_count == 2).then(|| prefix[16..32].try_into().expect("sixteen bytes")),
      tag: prefix[prefix_len - 1],
      file_data,
    })
  }

  fn write(&self, payload: &mut Vec<u8>) -> Result<()> {
    self.write_to(payload)
  }

  fn write_to(&self, payload: &mut dyn Write) -> Result<()> {
    payload.write_all(&self.uid1)?;
    if let Some(uid2) = self.uid2 {
      payload.write_all(&uid2)?;
    }
    payload.write_all(&[self.tag])?;
    match &self.file_data {
      OfficeArtBitmapData::Dib(encoded) | OfficeArtBitmapData::Encoded(encoded) => {
        payload.write_all(encoded)?;
      }
    }
    Ok(())
  }
}

impl OfficeArtMetafileBlip {
  fn parse(
    payload: &[u8],
    uid_count: usize,
    record_type: u16,
    max_decoded_len: usize,
  ) -> Option<Self> {
    let uid_len = uid_count.checked_mul(16)?;
    let header_end = uid_len.checked_add(34)?;
    let prefix = payload.get(..header_end)?;
    let metafile_header = OfficeArtMetafileHeader::parse(&prefix[uid_len..header_end])?;
    let encoded = payload[header_end..].to_vec();
    let decoded = decode_metafile_data(&encoded, metafile_header.compression, max_decoded_len);
    let file_data = match (record_type, decoded) {
      (0xf01a, Some(decoded)) => {
        OfficeArtMetafileData::Emf(OfficeArtMetafileBytes::new(decoded, encoded))
      }
      (0xf01b, Some(decoded)) => {
        OfficeArtMetafileData::Wmf(OfficeArtMetafileBytes::new(decoded, encoded))
      }
      (0xf01c, Some(decoded)) => {
        OfficeArtMetafileData::Pict(OfficeArtMetafileBytes::new(decoded, encoded))
      }
      (_, None) => OfficeArtMetafileData::Opaque {
        reason: match metafile_header.compression {
          0x00 | 0xfe => OfficeArtMetafileOpaqueReason::DecodeFailed,
          value => OfficeArtMetafileOpaqueReason::UnsupportedCompression(value),
        },
        decoded: None,
        original_encoded: encoded,
      },
      _ => OfficeArtMetafileData::Opaque {
        reason: OfficeArtMetafileOpaqueReason::DecodeFailed,
        decoded: None,
        original_encoded: encoded,
      },
    };
    Some(Self {
      uid1: prefix[..16].try_into().ok()?,
      uid2: (uid_count == 2).then(|| prefix[16..32].try_into().expect("sixteen bytes")),
      metafile_header,
      file_data,
    })
  }

  fn write(&self, payload: &mut Vec<u8>) -> Result<()> {
    self.write_to(payload)
  }

  fn write_to(&self, payload: &mut dyn Write) -> Result<()> {
    payload.write_all(&self.uid1)?;
    if let Some(uid2) = self.uid2 {
      payload.write_all(&uid2)?;
    }
    self.metafile_header.write_to(payload)?;
    match &self.file_data {
      OfficeArtMetafileData::Emf(data)
      | OfficeArtMetafileData::Wmf(data)
      | OfficeArtMetafileData::Pict(data) => {
        write_typed_metafile_to(payload, data, self.metafile_header.compression)?
      }
      OfficeArtMetafileData::Opaque {
        original_encoded, ..
      } => payload.write_all(original_encoded)?,
    }
    Ok(())
  }

  fn relayout(&mut self) -> Result<()> {
    let (decoded_len, encoded) = match &self.file_data {
      OfficeArtMetafileData::Emf(data)
      | OfficeArtMetafileData::Wmf(data)
      | OfficeArtMetafileData::Pict(data) => {
        if data.is_unchanged() {
          return Ok(());
        }
        let mut encoded = Vec::new();
        write_typed_metafile(&mut encoded, data, self.metafile_header.compression)?;
        (Some(data.decoded().len()), encoded)
      }
      OfficeArtMetafileData::Opaque { .. } => return Ok(()),
    };
    if let Some(decoded_len) = decoded_len {
      self.metafile_header.uncompressed_size = u32::try_from(decoded_len)
        .map_err(|_| Error::Limit("OfficeArt metafile data exceeds u32".into()))?;
    }
    self.metafile_header.saved_size = u32::try_from(encoded.len())
      .map_err(|_| Error::Limit("OfficeArt encoded metafile exceeds u32".into()))?;
    match &mut self.file_data {
      OfficeArtMetafileData::Emf(data)
      | OfficeArtMetafileData::Wmf(data)
      | OfficeArtMetafileData::Pict(data) => data.commit(encoded),
      OfficeArtMetafileData::Opaque { .. } => unreachable!("opaque data returned above"),
    }
    Ok(())
  }
}

fn decode_metafile_data(
  encoded: &[u8],
  compression: u8,
  max_decoded_len: usize,
) -> Option<Vec<u8>> {
  match compression {
    0x00 => {
      let mut decoded = Vec::new();
      let read_limit = u64::try_from(max_decoded_len).ok()?.checked_add(1)?;
      ZlibDecoder::new(encoded)
        .take(read_limit)
        .read_to_end(&mut decoded)
        .ok()?;
      (decoded.len() <= max_decoded_len).then_some(decoded)
    }
    0xfe if encoded.len() <= max_decoded_len => Some(encoded.to_vec()),
    _ => None,
  }
}

fn write_typed_metafile(
  payload: &mut Vec<u8>,
  data: &OfficeArtMetafileBytes,
  compression: u8,
) -> Result<()> {
  write_typed_metafile_to(payload, data, compression)
}

fn write_typed_metafile_to(
  payload: &mut dyn Write,
  data: &OfficeArtMetafileBytes,
  compression: u8,
) -> Result<()> {
  if data.is_unchanged() {
    payload.write_all(data.original_encoded())?;
    return Ok(());
  }
  match compression {
    0x00 => {
      let mut encoder = ZlibEncoder::new(&mut *payload, Compression::default());
      encoder.write_all(data.decoded())?;
      encoder.finish()?;
      Ok(())
    }
    0xfe => {
      payload.write_all(data.decoded())?;
      Ok(())
    }
    _ => Err(Error::invalid(
      0,
      "cannot encode modified OfficeArt metafile with unknown compression",
    )),
  }
}

impl OfficeArtMetafileHeader {
  fn parse(bytes: &[u8]) -> Option<Self> {
    if bytes.len() != 34 {
      return None;
    }
    let i32_at = |offset| i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let u32_at = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    Some(Self {
      uncompressed_size: u32_at(0),
      bounds: OfficeArtRect {
        left: i32_at(4),
        top: i32_at(8),
        right: i32_at(12),
        bottom: i32_at(16),
      },
      render_size: OfficeArtPoint {
        x: i32_at(20),
        y: i32_at(24),
      },
      saved_size: u32_at(28),
      compression: bytes[32],
      filter: bytes[33],
    })
  }

  fn write_to(&self, payload: &mut dyn Write) -> Result<()> {
    payload.write_all(&self.uncompressed_size.to_le_bytes())?;
    payload.write_all(&self.bounds.left.to_le_bytes())?;
    payload.write_all(&self.bounds.top.to_le_bytes())?;
    payload.write_all(&self.bounds.right.to_le_bytes())?;
    payload.write_all(&self.bounds.bottom.to_le_bytes())?;
    payload.write_all(&self.render_size.x.to_le_bytes())?;
    payload.write_all(&self.render_size.y.to_le_bytes())?;
    payload.write_all(&self.saved_size.to_le_bytes())?;
    payload.write_all(&[self.compression, self.filter])?;
    Ok(())
  }
}

impl OfficeArtFbse {
  fn encoded_len(&self) -> usize {
    36usize
      .saturating_add(self.name_data.len().saturating_mul(2))
      .saturating_add(self.embedded_blip.as_deref().map_or(0, |blip| {
        HEADER_LEN.saturating_add(blip.header.declared_length as usize)
      }))
      .saturating_add(self.trailing.len())
  }

  fn parse(
    payload: &[u8],
    depth: usize,
    record_count: &mut usize,
    limits: Limits,
  ) -> Result<Option<Self>> {
    let fixed = match payload.get(..36) {
      Some(fixed) => fixed,
      None => return Ok(None),
    };
    let declared_name_length = fixed[33];
    if declared_name_length % 2 != 0 {
      return Ok(None);
    }
    let name_end = match 36usize.checked_add(usize::from(declared_name_length)) {
      Some(end) if end <= payload.len() => end,
      _ => return Ok(None),
    };
    let name_data = payload[36..name_end]
      .chunks_exact(2)
      .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
      .collect::<Vec<_>>();
    let remainder = &payload[name_end..];
    let (embedded_blip, trailing) = if is_blip_record_prefix(remainder) {
      let mut nested_count = *record_count;
      match parse_one_record(remainder, depth + 1, &mut nested_count, limits) {
        Ok((record, consumed)) => {
          *record_count = nested_count;
          (Some(Box::new(record)), remainder[consumed..].to_vec())
        }
        Err(_) => (None, remainder.to_vec()),
      }
    } else {
      (None, remainder.to_vec())
    };
    Ok(Some(Self {
      win32_blip_type: fixed[0],
      macos_blip_type: fixed[1],
      uid: fixed[2..18].try_into().expect("sixteen bytes"),
      tag: u16::from_le_bytes(fixed[18..20].try_into().expect("two bytes")),
      declared_blip_size: u32::from_le_bytes(fixed[20..24].try_into().expect("four bytes")),
      reference_count: u32::from_le_bytes(fixed[24..28].try_into().expect("four bytes")),
      delay_offset: u32::from_le_bytes(fixed[28..32].try_into().expect("four bytes")),
      unused1: fixed[32],
      declared_name_length,
      unused2: fixed[34],
      unused3: fixed[35],
      name_data,
      embedded_blip,
      trailing,
    }))
  }

  fn write(&self, payload: &mut Vec<u8>) -> Result<()> {
    self.write_to(payload)
  }

  fn write_to(&self, payload: &mut dyn Write) -> Result<()> {
    let name_length = self
      .name_data
      .len()
      .checked_mul(2)
      .ok_or_else(|| Error::Limit("OfficeArt FBSE name length overflow".into()))?;
    if usize::from(self.declared_name_length) != name_length {
      return Err(Error::invalid(0, "OfficeArt FBSE name length mismatch"));
    }
    payload.write_all(&[self.win32_blip_type, self.macos_blip_type])?;
    payload.write_all(&self.uid)?;
    payload.write_all(&self.tag.to_le_bytes())?;
    payload.write_all(&self.declared_blip_size.to_le_bytes())?;
    payload.write_all(&self.reference_count.to_le_bytes())?;
    payload.write_all(&self.delay_offset.to_le_bytes())?;
    payload.write_all(&[
      self.unused1,
      self.declared_name_length,
      self.unused2,
      self.unused3,
    ])?;
    for unit in &self.name_data {
      payload.write_all(&unit.to_le_bytes())?;
    }
    if let Some(blip) = &self.embedded_blip {
      blip.write_to(payload)?;
    }
    payload.write_all(&self.trailing)?;
    Ok(())
  }
}

fn is_blip_record_prefix(bytes: &[u8]) -> bool {
  bytes
    .get(2..4)
    .map(|record_type| {
      let record_type = u16::from_le_bytes([record_type[0], record_type[1]]);
      (0xf018..=0xf117).contains(&record_type)
    })
    .unwrap_or(false)
}

impl OfficeArtDrawing {
  fn parse(payload: &[u8]) -> Self {
    Self {
      shape_count: u32::from_le_bytes(payload[0..4].try_into().expect("four bytes")),
      current_shape_id: u32::from_le_bytes(payload[4..8].try_into().expect("four bytes")),
    }
  }

  fn write(&self, payload: &mut Vec<u8>) {
    payload.extend_from_slice(&self.shape_count.to_le_bytes());
    payload.extend_from_slice(&self.current_shape_id.to_le_bytes());
  }
}

impl OfficeArtRect {
  fn parse(payload: &[u8]) -> Self {
    Self {
      left: i32::from_le_bytes(payload[0..4].try_into().expect("four bytes")),
      top: i32::from_le_bytes(payload[4..8].try_into().expect("four bytes")),
      right: i32::from_le_bytes(payload[8..12].try_into().expect("four bytes")),
      bottom: i32::from_le_bytes(payload[12..16].try_into().expect("four bytes")),
    }
  }

  fn write(&self, payload: &mut Vec<u8>) {
    payload.extend_from_slice(&self.left.to_le_bytes());
    payload.extend_from_slice(&self.top.to_le_bytes());
    payload.extend_from_slice(&self.right.to_le_bytes());
    payload.extend_from_slice(&self.bottom.to_le_bytes());
  }
}

impl OfficeArtShape {
  fn parse(payload: &[u8]) -> Self {
    Self {
      shape_id: u32::from_le_bytes(payload[0..4].try_into().expect("four bytes")),
      flags: OfficeArtShapeFlags::from_bits_retain(u32::from_le_bytes(
        payload[4..8].try_into().expect("four bytes"),
      )),
    }
  }

  fn write(&self, payload: &mut Vec<u8>) {
    payload.extend_from_slice(&self.shape_id.to_le_bytes());
    payload.extend_from_slice(&self.flags.bits().to_le_bytes());
  }
}

impl OfficeArtClientAnchor {
  fn parse(payload: &[u8]) -> Option<Self> {
    match payload.len() {
      18 => Some(Self::Words18 {
        flags: u16::from_le_bytes(payload[0..2].try_into().expect("two bytes")),
        coordinates: std::array::from_fn(|index| {
          let offset = 2 + index * 2;
          u16::from_le_bytes([payload[offset], payload[offset + 1]])
        }),
      }),
      8 => Some(Self::Words8 {
        coordinates: std::array::from_fn(|index| {
          let offset = index * 2;
          i16::from_le_bytes([payload[offset], payload[offset + 1]])
        }),
      }),
      16 => Some(Self::PowerPointRect(OfficeArtRect::parse(payload))),
      _ => None,
    }
  }

  fn write(&self, payload: &mut Vec<u8>) {
    match self {
      Self::Words18 { flags, coordinates } => {
        payload.extend_from_slice(&flags.to_le_bytes());
        for coordinate in coordinates {
          payload.extend_from_slice(&coordinate.to_le_bytes());
        }
      }
      Self::Words8 { coordinates } => {
        for coordinate in coordinates {
          payload.extend_from_slice(&coordinate.to_le_bytes());
        }
      }
      Self::PowerPointRect(rect) => rect.write(payload),
    }
  }
}

impl OfficeArtArcRule {
  fn parse(payload: &[u8]) -> Self {
    Self {
      rule_id: u32::from_le_bytes(payload[0..4].try_into().expect("four bytes")),
      shape_id: u32::from_le_bytes(payload[4..8].try_into().expect("four bytes")),
    }
  }

  fn write(&self, payload: &mut Vec<u8>) {
    payload.extend_from_slice(&self.rule_id.to_le_bytes());
    payload.extend_from_slice(&self.shape_id.to_le_bytes());
  }
}

impl OfficeArtConnectorRule {
  fn parse(payload: &[u8]) -> Self {
    let field = |index: usize| {
      let offset = index * 4;
      u32::from_le_bytes(payload[offset..offset + 4].try_into().expect("four bytes"))
    };
    Self {
      rule_id: field(0),
      start_shape_id: field(1),
      end_shape_id: field(2),
      connector_shape_id: field(3),
      start_connection_site: field(4),
      end_connection_site: field(5),
    }
  }

  fn write(&self, payload: &mut Vec<u8>) {
    for value in [
      self.rule_id,
      self.start_shape_id,
      self.end_shape_id,
      self.connector_shape_id,
      self.start_connection_site,
      self.end_connection_site,
    ] {
      payload.extend_from_slice(&value.to_le_bytes());
    }
  }
}

impl OfficeArtCalloutRule {
  fn parse(payload: &[u8]) -> Self {
    Self {
      rule_id: u32::from_le_bytes(payload[0..4].try_into().expect("four bytes")),
      shape_id: u32::from_le_bytes(payload[4..8].try_into().expect("four bytes")),
    }
  }

  fn write(&self, payload: &mut Vec<u8>) {
    payload.extend_from_slice(&self.rule_id.to_le_bytes());
    payload.extend_from_slice(&self.shape_id.to_le_bytes());
  }
}

impl OfficeArtFrit {
  fn parse(payload: &[u8]) -> Self {
    Self {
      new_group_id: u16::from_le_bytes(payload[0..2].try_into().expect("two bytes")),
      old_group_id: u16::from_le_bytes(payload[2..4].try_into().expect("two bytes")),
    }
  }

  fn write(&self, payload: &mut Vec<u8>) {
    payload.extend_from_slice(&self.new_group_id.to_le_bytes());
    payload.extend_from_slice(&self.old_group_id.to_le_bytes());
  }
}

impl SoftMakerNativeProperties {
  fn parse(payload: &[u8], max_entries: usize) -> Option<Self> {
    let mut cursor = 0usize;
    let mut properties = Vec::new();
    while cursor < payload.len() {
      if properties.len() >= max_entries {
        return None;
      }
      let header = payload.get(cursor..cursor.checked_add(8)?)?;
      let declared_length = u32::from_le_bytes(header[4..8].try_into().ok()?);
      let length = usize::try_from(declared_length).ok()?;
      let data_start = cursor.checked_add(8)?;
      let data_end = data_start.checked_add(length)?;
      let raw = payload.get(data_start..data_end)?;
      let selector = u16::from_le_bytes(header[0..2].try_into().ok()?);
      let reserved = u16::from_le_bytes(header[2..4].try_into().ok()?);
      properties.push(SoftMakerNativeProperty {
        selector,
        reserved,
        declared_length,
        data: SoftMakerNativePropertyData::parse(selector, reserved, raw),
      });
      cursor = data_end;
    }
    Some(Self { properties })
  }

  fn write(&self, payload: &mut Vec<u8>) -> Result<()> {
    for property in &self.properties {
      let encoded = property.data.to_bytes();
      if usize::try_from(property.declared_length).ok() != Some(encoded.len()) {
        return Err(Error::invalid(
          0,
          "SoftMaker native property length does not match payload",
        ));
      }
      payload.extend_from_slice(&property.selector.to_le_bytes());
      payload.extend_from_slice(&property.reserved.to_le_bytes());
      payload.extend_from_slice(&property.declared_length.to_le_bytes());
      payload.extend_from_slice(&encoded);
    }
    Ok(())
  }
}

impl SoftMakerNativePropertyData {
  fn parse(selector: u16, reserved: u16, payload: &[u8]) -> Self {
    fn words<const N: usize>(payload: &[u8]) -> [u32; N] {
      std::array::from_fn(|index| {
        let start = index * 4;
        u32::from_le_bytes(
          payload[start..start + 4]
            .try_into()
            .expect("validated fixed-size native property"),
        )
      })
    }
    match (selector, reserved, payload.len()) {
      (0, 0, 37) => Self::Selector0 {
        leading: payload[0],
        words: words(&payload[1..]),
      },
      (1, 0, 80) => Self::Selector1 {
        double_bits: std::array::from_fn(|index| {
          let start = index * 8;
          u64::from_le_bytes(
            payload[start..start + 8]
              .try_into()
              .expect("validated fixed-size native property"),
          )
        }),
      },
      (2, 0, 140) => Self::Selector2 {
        words: words(payload),
      },
      (3, 0, 60) => Self::Selector3 {
        words: words(payload),
      },
      (4, 0, 96) => Self::Selector4 {
        words: words(payload),
      },
      (6, 0, 81) => Self::Selector6 {
        font_name: std::array::from_fn(|index| {
          let start = index * 2;
          u16::from_le_bytes(
            payload[start..start + 2]
              .try_into()
              .expect("validated fixed-size native property"),
          )
        }),
        words: words(&payload[12..80]),
        trailing: payload[80],
      },
      (8, 0, 20) => Self::Selector8 {
        words: words(payload),
      },
      (9, 0, 4) => Self::Selector9(u32::from_le_bytes(
        payload
          .try_into()
          .expect("validated fixed-size native property"),
      )),
      (12, 0, 16) => Self::Selector12 {
        words: words(payload),
      },
      _ => Self::Unknown(payload.to_vec()),
    }
  }

  pub fn encoded_len(&self) -> usize {
    match self {
      Self::Selector0 { .. } => 37,
      Self::Selector1 { .. } => 80,
      Self::Selector2 { .. } => 140,
      Self::Selector3 { .. } => 60,
      Self::Selector4 { .. } => 96,
      Self::Selector6 { .. } => 81,
      Self::Selector8 { .. } => 20,
      Self::Selector9(_) => 4,
      Self::Selector12 { .. } => 16,
      Self::Unknown(payload) => payload.len(),
    }
  }

  pub fn unparsed_byte_count(&self) -> usize {
    match self {
      Self::Unknown(payload) => payload.len(),
      _ => 0,
    }
  }

  fn to_bytes(&self) -> Vec<u8> {
    fn extend_words<const N: usize>(bytes: &mut Vec<u8>, words: &[u32; N]) {
      for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
      }
    }
    let mut bytes = Vec::with_capacity(self.encoded_len());
    match self {
      Self::Selector0 { leading, words } => {
        bytes.push(*leading);
        extend_words(&mut bytes, words);
      }
      Self::Selector1 { double_bits } => {
        for value in double_bits {
          bytes.extend_from_slice(&value.to_le_bytes());
        }
      }
      Self::Selector2 { words } => extend_words(&mut bytes, words),
      Self::Selector3 { words } => extend_words(&mut bytes, words),
      Self::Selector4 { words } => extend_words(&mut bytes, words),
      Self::Selector6 {
        font_name,
        words,
        trailing,
      } => {
        for character in font_name {
          bytes.extend_from_slice(&character.to_le_bytes());
        }
        extend_words(&mut bytes, words);
        bytes.push(*trailing);
      }
      Self::Selector8 { words } => extend_words(&mut bytes, words),
      Self::Selector9(value) => bytes.extend_from_slice(&value.to_le_bytes()),
      Self::Selector12 { words } => extend_words(&mut bytes, words),
      Self::Unknown(payload) => bytes.extend_from_slice(payload),
    }
    bytes
  }
}

impl OfficeArtDggBlock {
  fn parse(payload: &[u8]) -> Option<Self> {
    if payload.len() < 16 || !(payload.len() - 16).is_multiple_of(8) {
      return None;
    }
    let declared_cluster_count = u32::from_le_bytes(payload[4..8].try_into().ok()?);
    let cluster_count = usize::try_from(declared_cluster_count.checked_sub(1)?).ok()?;
    if payload.len() != 16usize.checked_add(cluster_count.checked_mul(8)?)? {
      return None;
    }
    let clusters = payload[16..]
      .chunks_exact(8)
      .map(|cluster| OfficeArtIdCluster {
        drawing_id: u32::from_le_bytes(cluster[0..4].try_into().expect("four bytes")),
        current_shape_id_count: u32::from_le_bytes(cluster[4..8].try_into().expect("four bytes")),
      })
      .collect::<Vec<_>>();
    Some(Self {
      maximum_shape_id: u32::from_le_bytes(payload[0..4].try_into().ok()?),
      declared_cluster_count,
      saved_shape_count: u32::from_le_bytes(payload[8..12].try_into().ok()?),
      saved_drawing_count: u32::from_le_bytes(payload[12..16].try_into().ok()?),
      clusters,
    })
  }

  fn write(&self, payload: &mut Vec<u8>) -> Result<()> {
    if usize::try_from(
      self
        .declared_cluster_count
        .checked_sub(1)
        .unwrap_or(u32::MAX),
    )
    .ok()
      != Some(self.clusters.len())
    {
      return Err(Error::invalid(0, "OfficeArt FDGG cluster count mismatch"));
    }
    payload.extend_from_slice(&self.maximum_shape_id.to_le_bytes());
    payload.extend_from_slice(&self.declared_cluster_count.to_le_bytes());
    payload.extend_from_slice(&self.saved_shape_count.to_le_bytes());
    payload.extend_from_slice(&self.saved_drawing_count.to_le_bytes());
    for cluster in &self.clusters {
      payload.extend_from_slice(&cluster.drawing_id.to_le_bytes());
      payload.extend_from_slice(&cluster.current_shape_id_count.to_le_bytes());
    }
    Ok(())
  }

  fn relayout(&mut self) -> Result<()> {
    self.declared_cluster_count = u32::try_from(self.clusters.len())
      .map_err(|_| Error::Limit("OfficeArt FDGG cluster count exceeds u32".into()))?
      .checked_add(1)
      .ok_or_else(|| Error::Limit("OfficeArt FDGG cluster count overflow".into()))?;
    Ok(())
  }
}

impl OfficeArtPropertyTable {
  fn parse(payload: &[u8], property_count: usize) -> Option<Self> {
    let fixed_len = property_count.checked_mul(6)?;
    if fixed_len > payload.len() {
      return None;
    }
    let mut properties = Vec::with_capacity(property_count);
    for entry in payload[..fixed_len].chunks_exact(6) {
      let opid = u16::from_le_bytes([entry[0], entry[1]]);
      let op = u32::from_le_bytes(entry[2..6].try_into().expect("four bytes"));
      properties.push(OfficeArtProperty {
        property_id: opid & 0x3fff,
        is_blip_id: opid & 0x4000 != 0,
        value: if opid & 0x8000 != 0 {
          OfficeArtPropertyValue::Complex {
            declared_length: op,
            data: Vec::new(),
          }
        } else {
          OfficeArtPropertyValue::Simple(op)
        },
      });
    }
    let mut cursor = fixed_len;
    for property in &mut properties {
      let declared_length = match &property.value {
        OfficeArtPropertyValue::Complex {
          declared_length, ..
        } => *declared_length,
        _ => continue,
      };
      let length = usize::try_from(declared_length).ok()?;
      let remaining = payload.get(cursor..)?;
      if is_typed_array_property(property.property_id) && length == 0 {
        property.value = OfficeArtPropertyValue::EmptyArray { declared_length };
        continue;
      }
      if length == 0 && !is_utf16_complex_property(property.property_id) {
        property.value = OfficeArtPropertyValue::EmptyComplex { declared_length };
        continue;
      }
      if let Some((value, consumed, declared_length_delta)) =
        OfficeArtArray::parse(property.property_id, remaining, length)
      {
        property.value = OfficeArtPropertyValue::Array {
          declared_length,
          declared_length_delta,
          value,
        };
        cursor = cursor.checked_add(consumed)?;
        continue;
      }
      if property.property_id == 0x03a9 {
        let end = cursor.checked_add(length)?;
        let data = payload.get(cursor..end)?;
        if let Some(value) = OfficeArtMetroBlob::parse(data) {
          property.value = OfficeArtPropertyValue::MetroBlob {
            declared_length,
            value,
          };
          cursor = end;
          continue;
        }
      }
      if property.property_id == 0x0382 && length >= 24 {
        let end = cursor.checked_add(length)?;
        let data = payload.get(cursor..end)?;
        if data[..16] == STANDARD_HYPERLINK_CLASS_ID
          && let Ok(object) = crate::xls::HyperlinkObject::parse(&data[16..])
        {
          property.value = OfficeArtPropertyValue::Hyperlink {
            declared_length,
            class_id: STANDARD_HYPERLINK_CLASS_ID,
            object,
          };
          cursor = end;
          continue;
        }
      }
      let end = cursor.checked_add(length)?;
      let data = payload.get(cursor..end)?;
      property.value =
        if is_utf16_complex_property(property.property_id) && data.len().is_multiple_of(2) {
          OfficeArtPropertyValue::Utf16String {
            declared_length,
            code_units: data
              .chunks_exact(2)
              .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
              .collect(),
          }
        } else {
          OfficeArtPropertyValue::Complex {
            declared_length,
            data: data.to_vec(),
          }
        };
      cursor = end;
    }
    Some(Self {
      properties,
      trailing: payload[cursor..].to_vec(),
    })
  }

  fn write(&self, payload: &mut Vec<u8>) -> Result<()> {
    for property in &self.properties {
      let (complex, op) = match &property.value {
        OfficeArtPropertyValue::Simple(value) => (false, *value),
        OfficeArtPropertyValue::Complex {
          declared_length,
          data,
        } => {
          if usize::try_from(*declared_length).ok() != Some(data.len()) {
            return Err(Error::invalid(
              0,
              "OfficeArt complex property length mismatch",
            ));
          }
          (true, *declared_length)
        }
        OfficeArtPropertyValue::EmptyComplex { declared_length } => {
          if *declared_length != 0 {
            return Err(Error::invalid(
              0,
              "OfficeArt empty complex property has a nonzero declared length",
            ));
          }
          (true, 0)
        }
        OfficeArtPropertyValue::EmptyArray { declared_length } => {
          if *declared_length != 0 {
            return Err(Error::invalid(
              0,
              "OfficeArt empty array has a nonzero declared length",
            ));
          }
          (true, 0)
        }
        OfficeArtPropertyValue::Array {
          declared_length,
          declared_length_delta,
          value,
        } => {
          let encoded_len = value.encoded_len()?;
          let expected_declared = encoded_len
            .checked_sub(usize::from(*declared_length_delta))
            .ok_or_else(|| Error::invalid(0, "OfficeArt array delta exceeds its encoded length"))?;
          if usize::try_from(*declared_length).ok() != Some(expected_declared) {
            return Err(Error::invalid(
              0,
              "OfficeArt array property length mismatch",
            ));
          }
          (true, *declared_length)
        }
        OfficeArtPropertyValue::MetroBlob {
          declared_length,
          value,
        } => {
          value.validate()?;
          if usize::try_from(*declared_length).ok() != Some(value.package_bytes.len()) {
            return Err(Error::invalid(
              0,
              "OfficeArt metroBlob property length mismatch",
            ));
          }
          (true, *declared_length)
        }
        OfficeArtPropertyValue::Hyperlink {
          declared_length,
          class_id,
          object,
        } => {
          if *class_id != STANDARD_HYPERLINK_CLASS_ID {
            return Err(Error::invalid(0, "OfficeArt IHlink CLSID changed"));
          }
          let object_len = object.to_bytes()?.len();
          if usize::try_from(*declared_length).ok() != object_len.checked_add(16) {
            return Err(Error::invalid(
              0,
              "OfficeArt IHlink property length mismatch",
            ));
          }
          (true, *declared_length)
        }
        OfficeArtPropertyValue::Utf16String {
          declared_length,
          code_units,
        } => {
          if usize::try_from(*declared_length).ok() != code_units.len().checked_mul(2) {
            return Err(Error::invalid(
              0,
              "OfficeArt UTF-16 property length mismatch",
            ));
          }
          (true, *declared_length)
        }
      };
      if property.property_id > 0x3fff {
        return Err(Error::invalid(0, "OfficeArt property id exceeds 14 bits"));
      }
      let opid = property.property_id
        | if property.is_blip_id { 0x4000 } else { 0 }
        | if complex { 0x8000 } else { 0 };
      payload.extend_from_slice(&opid.to_le_bytes());
      payload.extend_from_slice(&op.to_le_bytes());
    }
    for property in &self.properties {
      if let OfficeArtPropertyValue::Complex { data, .. } = &property.value {
        payload.extend_from_slice(data);
      } else if let OfficeArtPropertyValue::Utf16String { code_units, .. } = &property.value {
        for unit in code_units {
          payload.extend_from_slice(&unit.to_le_bytes());
        }
      } else if let OfficeArtPropertyValue::Array { value, .. } = &property.value {
        value.write(payload)?;
      } else if let OfficeArtPropertyValue::MetroBlob { value, .. } = &property.value {
        value.validate()?;
        payload.extend_from_slice(&value.package_bytes);
      } else if let OfficeArtPropertyValue::Hyperlink {
        class_id, object, ..
      } = &property.value
      {
        payload.extend_from_slice(class_id);
        payload.extend_from_slice(&object.to_bytes()?);
      }
    }
    payload.extend_from_slice(&self.trailing);
    Ok(())
  }

  fn relayout(&mut self) -> Result<()> {
    for property in &mut self.properties {
      match &mut property.value {
        OfficeArtPropertyValue::Simple(_) => {}
        OfficeArtPropertyValue::Complex {
          declared_length,
          data,
        } => {
          *declared_length = u32::try_from(data.len())
            .map_err(|_| Error::Limit("OfficeArt complex property exceeds u32".into()))?;
        }
        OfficeArtPropertyValue::Utf16String {
          declared_length,
          code_units,
        } => {
          *declared_length = u32::try_from(
            code_units
              .len()
              .checked_mul(2)
              .ok_or_else(|| Error::Limit("OfficeArt UTF-16 property length overflow".into()))?,
          )
          .map_err(|_| Error::Limit("OfficeArt UTF-16 property exceeds u32".into()))?;
        }
        OfficeArtPropertyValue::EmptyComplex { declared_length }
        | OfficeArtPropertyValue::EmptyArray { declared_length } => {
          *declared_length = 0;
        }
        OfficeArtPropertyValue::Array {
          declared_length,
          declared_length_delta,
          value,
        } => {
          let encoded_len = value.relayout()?;
          let declared = encoded_len
            .checked_sub(usize::from(*declared_length_delta))
            .ok_or_else(|| Error::invalid(0, "OfficeArt array delta exceeds its encoded length"))?;
          *declared_length = u32::try_from(declared)
            .map_err(|_| Error::Limit("OfficeArt array property exceeds u32".into()))?;
        }
        OfficeArtPropertyValue::MetroBlob {
          declared_length,
          value,
        } => {
          value.validate()?;
          *declared_length = u32::try_from(value.package_bytes.len())
            .map_err(|_| Error::Limit("OfficeArt metroBlob property exceeds u32".into()))?;
        }
        OfficeArtPropertyValue::Hyperlink {
          declared_length,
          class_id,
          object,
        } => {
          if *class_id != STANDARD_HYPERLINK_CLASS_ID {
            return Err(Error::invalid(0, "OfficeArt IHlink CLSID changed"));
          }
          let length = object
            .to_bytes()?
            .len()
            .checked_add(16)
            .ok_or_else(|| Error::Limit("OfficeArt IHlink property length overflow".into()))?;
          *declared_length = u32::try_from(length)
            .map_err(|_| Error::Limit("OfficeArt IHlink property exceeds u32".into()))?;
        }
      }
    }
    Ok(())
  }
}

fn is_utf16_complex_property(property_id: u16) -> bool {
  matches!(
    property_id,
    0x00c0 // geoText.unicode
            | 0x00c5 // geoText.fontFamilyName
            | 0x0105 // blip.blipFileName
            | 0x0110 // blip.printBlipFileName
            | 0x0187 // fill.blipFileName
            | 0x01c6 // lineStyle.fillBlipName
            | 0x0380 // groupShape.shapeName
            | 0x0381 // groupShape.description
            | 0x038d // groupShape.tooltip
            | 0x038e // groupShape.script
            | 0x0397 // groupShape.scriptExtAttr
            | 0x03a5 // groupShape.webBot
  )
}

fn is_typed_array_property(property_id: u16) -> bool {
  matches!(
    property_id,
    0x0145 // geometry.pVertices
            | 0x0146 // geometry.pSegmentInfo
            | 0x0151 // geometry.pConnectionSites
            | 0x0152 // geometry.pConnectionSitesDir
            | 0x0155 // geometry.pAdjustHandles
            | 0x0156 // geometry.pGuides
            | 0x0157 // geometry.pInscribe
            | 0x0197 // fill.fillShadeColors
            | 0x01cf // lineStyle.lineDashStyle
            | 0x0383 // groupShape.pWrapPolygonVertices
  )
}

impl OfficeArtMetroBlob {
  fn parse(bytes: &[u8]) -> Option<Self> {
    const LOCAL_FILE_HEADER: &[u8; 4] = b"PK\x03\x04";
    const CENTRAL_FILE_HEADER: &[u8; 4] = b"PK\x01\x02";
    const END_OF_CENTRAL_DIRECTORY: &[u8; 4] = b"PK\x05\x06";
    if !bytes.starts_with(LOCAL_FILE_HEADER) || bytes.len() < 22 {
      return None;
    }

    let minimum_offset = bytes.len().saturating_sub(22 + usize::from(u16::MAX));
    let eocd_offset = (minimum_offset..=bytes.len() - 22)
      .rev()
      .find(|offset| bytes[*offset..].starts_with(END_OF_CENTRAL_DIRECTORY))?;
    let eocd = &bytes[eocd_offset..];
    let read_u16 = |offset: usize| {
      eocd
        .get(offset..offset + 2)
        .map(|value| u16::from_le_bytes([value[0], value[1]]))
    };
    let read_u32 = |offset: usize| {
      eocd
        .get(offset..offset + 4)
        .map(|value| u32::from_le_bytes(value.try_into().expect("validated four-byte ZIP field")))
    };
    let disk_number = read_u16(4)?;
    let central_directory_disk = read_u16(6)?;
    let entries_on_disk = read_u16(8)?;
    let entry_count = read_u16(10)?;
    let central_directory_size = read_u32(12)?;
    let central_directory_offset = read_u32(16)?;
    let comment_len = usize::from(read_u16(20)?);
    if disk_number != 0
      || central_directory_disk != 0
      || entries_on_disk != entry_count
      || eocd_offset.checked_add(22)?.checked_add(comment_len)? != bytes.len()
    {
      return None;
    }
    let comment = eocd.get(22..22 + comment_len)?.to_vec();
    let directory_start = usize::try_from(central_directory_offset).ok()?;
    let directory_size = usize::try_from(central_directory_size).ok()?;
    let directory_end = directory_start.checked_add(directory_size)?;
    if directory_end != eocd_offset {
      return None;
    }

    let mut cursor = directory_start;
    let mut entries = Vec::with_capacity(usize::from(entry_count));
    for _ in 0..entry_count {
      let fixed = bytes.get(cursor..cursor.checked_add(46)?)?;
      if !fixed.starts_with(CENTRAL_FILE_HEADER) {
        return None;
      }
      let u16_at = |offset: usize| u16::from_le_bytes([fixed[offset], fixed[offset + 1]]);
      let u32_at = |offset: usize| {
        u32::from_le_bytes(
          fixed[offset..offset + 4]
            .try_into()
            .expect("validated central-directory field"),
        )
      };
      let file_name_len = usize::from(u16_at(28));
      let extra_len = usize::from(u16_at(30));
      let entry_comment_len = usize::from(u16_at(32));
      if u16_at(34) != 0 {
        return None;
      }
      let variable_len = file_name_len
        .checked_add(extra_len)?
        .checked_add(entry_comment_len)?;
      let end = cursor.checked_add(46)?.checked_add(variable_len)?;
      let variable = bytes.get(cursor + 46..end)?;
      let local_header_offset = u32_at(42);
      let local_offset = usize::try_from(local_header_offset).ok()?;
      if !bytes.get(local_offset..)?.starts_with(LOCAL_FILE_HEADER) {
        return None;
      }
      entries.push(OfficeArtZipEntry {
        compression_method: u16_at(10),
        flags: u16_at(8),
        crc32: u32_at(16),
        compressed_size: u32_at(20),
        uncompressed_size: u32_at(24),
        file_name: variable[..file_name_len].to_vec(),
        extra_field: variable[file_name_len..file_name_len + extra_len].to_vec(),
        comment: variable[file_name_len + extra_len..].to_vec(),
        local_header_offset,
      });
      cursor = end;
    }
    if cursor != directory_end {
      return None;
    }

    Some(Self {
      package_bytes: bytes.to_vec(),
      directory: OfficeArtZipDirectory {
        entry_count,
        central_directory_size,
        central_directory_offset,
        comment,
        entries,
      },
    })
  }

  fn validate(&self) -> Result<()> {
    let parsed = Self::parse(&self.package_bytes)
      .ok_or_else(|| Error::invalid(0, "OfficeArt metroBlob is not a bounded OPC ZIP"))?;
    if parsed.directory != self.directory {
      return Err(Error::invalid(
        0,
        "OfficeArt metroBlob ZIP directory metadata changed",
      ));
    }
    Ok(())
  }
}

impl OfficeArtArray {
  fn parse(property_id: u16, bytes: &[u8], declared_length: usize) -> Option<(Self, usize, u8)> {
    if declared_length == 0 {
      return None;
    }
    let (value, encoded_len) = Self::parse_encoded(property_id, bytes)?;
    let declared_length_delta = encoded_len.checked_sub(declared_length)?;
    if !(matches!(declared_length_delta, 0 | 6)
      || property_id == 0x0145 && declared_length_delta == 5)
    {
      return None;
    }
    Some((
      value,
      encoded_len,
      u8::try_from(declared_length_delta).ok()?,
    ))
  }

  fn parse_encoded(property_id: u16, bytes: &[u8]) -> Option<(Self, usize)> {
    if !is_typed_array_property(property_id) {
      return None;
    }
    let header = bytes.get(..6)?;
    let element_count = u16::from_le_bytes([header[0], header[1]]);
    let allocated_element_count = u16::from_le_bytes([header[2], header[3]]);
    if allocated_element_count < element_count {
      return None;
    }
    let encoded_element_size = u16::from_le_bytes([header[4], header[5]]);
    let element_size = if encoded_element_size == 0xfff0 {
      4usize
    } else {
      usize::from(encoded_element_size)
    };
    let data_len = usize::from(element_count).checked_mul(element_size)?;
    let encoded_len = 6usize.checked_add(data_len)?;
    let data = bytes.get(6..encoded_len)?;
    let data = OfficeArtArrayData::parse(property_id, encoded_element_size, data)?;
    Some((
      Self {
        element_count,
        allocated_element_count,
        encoded_element_size,
        data,
      },
      encoded_len,
    ))
  }

  fn encoded_len(&self) -> Result<usize> {
    let data_len = self.data.encoded_len();
    let count = self.data.element_count();
    if usize::from(self.element_count) != count {
      return Err(Error::invalid(0, "OfficeArt array element count mismatch"));
    }
    if self.allocated_element_count < self.element_count {
      return Err(Error::invalid(
        0,
        "OfficeArt allocated array count is below its element count",
      ));
    }
    let expected_size = if self.encoded_element_size == 0xfff0 {
      4
    } else {
      usize::from(self.encoded_element_size)
    };
    if count != 0 && data_len / count != expected_size {
      return Err(Error::invalid(0, "OfficeArt array element size mismatch"));
    }
    6usize
      .checked_add(data_len)
      .ok_or_else(|| Error::Limit("OfficeArt array length overflow".into()))
  }

  fn relayout(&mut self) -> Result<usize> {
    let element_count = u16::try_from(self.data.element_count())
      .map_err(|_| Error::Limit("OfficeArt array element count exceeds u16".into()))?;
    self.element_count = element_count;
    self.allocated_element_count = self.allocated_element_count.max(element_count);
    let element_size = self.data.element_size();
    self.encoded_element_size = if self.encoded_element_size == 0xfff0 && element_size == 4 {
      0xfff0
    } else {
      element_size
    };
    self.encoded_len()
  }

  fn write(&self, payload: &mut Vec<u8>) -> Result<()> {
    self.encoded_len()?;
    payload.extend_from_slice(&self.element_count.to_le_bytes());
    payload.extend_from_slice(&self.allocated_element_count.to_le_bytes());
    payload.extend_from_slice(&self.encoded_element_size.to_le_bytes());
    self.data.write(payload);
    Ok(())
  }
}

impl OfficeArtArrayData {
  fn parse(property_id: u16, element_size: u16, bytes: &[u8]) -> Option<Self> {
    let words_u16 = || {
      bytes
        .chunks_exact(2)
        .map(|value| u16::from_le_bytes([value[0], value[1]]))
        .collect::<Vec<_>>()
    };
    let words_u32 = || {
      bytes
        .chunks_exact(4)
        .map(|value| u32::from_le_bytes(value.try_into().expect("four bytes")))
        .collect::<Vec<_>>()
    };
    match (property_id, element_size) {
      (0x0145 | 0x0151 | 0x0383, 0xfff0 | 4) => Some(Self::Points16(
        bytes
          .chunks_exact(4)
          .map(|value| OfficeArtPoint16 {
            x: i16::from_le_bytes([value[0], value[1]]),
            y: i16::from_le_bytes([value[2], value[3]]),
          })
          .collect(),
      )),
      (0x0145 | 0x0151 | 0x0383, 8) => Some(Self::Points32(
        bytes
          .chunks_exact(8)
          .map(|value| OfficeArtPoint32 {
            x: i32::from_le_bytes(value[0..4].try_into().expect("four bytes")),
            y: i32::from_le_bytes(value[4..8].try_into().expect("four bytes")),
          })
          .collect(),
      )),
      (0x0146, 2) => Some(Self::Segments(words_u16())),
      (0x0152, 4) => Some(Self::FixedPointBits(words_u32())),
      (0x0157, 16) => Some(Self::Rectangles(
        bytes.chunks_exact(16).map(OfficeArtRect::parse).collect(),
      )),
      (0x0197, 8) => Some(Self::ShadeColors(
        bytes
          .chunks_exact(8)
          .map(|value| OfficeArtShadeColor {
            color: u32::from_le_bytes(value[0..4].try_into().expect("four bytes")),
            position: u32::from_le_bytes(value[4..8].try_into().expect("four bytes")),
          })
          .collect(),
      )),
      (0x01cf, 4) => Some(Self::Unsigned32(words_u32())),
      _ => None,
    }
  }

  fn element_count(&self) -> usize {
    match self {
      Self::Points16(values) => values.len(),
      Self::Points32(values) => values.len(),
      Self::Segments(values) => values.len(),
      Self::FixedPointBits(values) => values.len(),
      Self::Rectangles(values) => values.len(),
      Self::ShadeColors(values) => values.len(),
      Self::Unsigned32(values) => values.len(),
    }
  }

  fn encoded_len(&self) -> usize {
    match self {
      Self::Points16(values) => values.len() * 4,
      Self::Points32(values) => values.len() * 8,
      Self::Segments(values) => values.len() * 2,
      Self::FixedPointBits(values) | Self::Unsigned32(values) => values.len() * 4,
      Self::Rectangles(values) => values.len() * 16,
      Self::ShadeColors(values) => values.len() * 8,
    }
  }

  fn element_size(&self) -> u16 {
    match self {
      Self::Points16(_) | Self::FixedPointBits(_) | Self::Unsigned32(_) => 4,
      Self::Points32(_) | Self::ShadeColors(_) => 8,
      Self::Segments(_) => 2,
      Self::Rectangles(_) => 16,
    }
  }

  fn write(&self, payload: &mut Vec<u8>) {
    match self {
      Self::Points16(values) => {
        for value in values {
          payload.extend_from_slice(&value.x.to_le_bytes());
          payload.extend_from_slice(&value.y.to_le_bytes());
        }
      }
      Self::Points32(values) => {
        for value in values {
          payload.extend_from_slice(&value.x.to_le_bytes());
          payload.extend_from_slice(&value.y.to_le_bytes());
        }
      }
      Self::Segments(values) => {
        for value in values {
          payload.extend_from_slice(&value.to_le_bytes());
        }
      }
      Self::FixedPointBits(values) | Self::Unsigned32(values) => {
        for value in values {
          payload.extend_from_slice(&value.to_le_bytes());
        }
      }
      Self::Rectangles(values) => {
        for value in values {
          value.write(payload);
        }
      }
      Self::ShadeColors(values) => {
        for value in values {
          payload.extend_from_slice(&value.color.to_le_bytes());
          payload.extend_from_slice(&value.position.to_le_bytes());
        }
      }
    }
  }
}

impl OfficeArtIncompletePropertyTable {
  pub fn available_complex_len(&self) -> usize {
    self
      .complex_fragments
      .iter()
      .map(|fragment| fragment.data.encoded_len())
      .sum::<usize>()
      + self.trailing_data.len()
  }

  pub fn unparsed_complex_len(&self) -> usize {
    self
      .complex_fragments
      .iter()
      .filter(|fragment| !fragment.is_complete)
      .map(|fragment| fragment.data.unparsed_byte_count())
      .sum::<usize>()
      + self.trailing_data.len()
  }

  fn split_complex_data(
    entries: &[OfficeArtPropertyEntry],
    payload: &[u8],
  ) -> (Vec<OfficeArtComplexPropertyFragment>, Vec<u8>) {
    let mut cursor = 0usize;
    let mut fragments = Vec::new();
    for (entry_index, entry) in entries.iter().enumerate() {
      if !entry.is_complex {
        continue;
      }
      let declared_length = entry.value_or_declared_length;
      let declared = usize::try_from(declared_length).unwrap_or(usize::MAX);
      let remaining = &payload[cursor..];
      if let Some((array, encoded_len)) =
        OfficeArtArray::parse_encoded(entry.property_id, remaining)
      {
        let declared_delta = encoded_len.checked_sub(declared);
        let is_complete = declared == encoded_len
          || declared_delta == Some(6)
          || entry.property_id == 0x0145 && declared_delta == Some(5);
        fragments.push(OfficeArtComplexPropertyFragment {
          entry_index,
          property_id: entry.property_id,
          declared_length,
          data: OfficeArtComplexPropertyData::Array(array),
          is_complete,
        });
        cursor += encoded_len;
        continue;
      }
      let available = declared.min(payload.len().saturating_sub(cursor));
      let end = cursor + available;
      let raw = &payload[cursor..end];
      fragments.push(OfficeArtComplexPropertyFragment {
        entry_index,
        property_id: entry.property_id,
        declared_length,
        data: OfficeArtComplexPropertyData::Bytes(raw.to_vec()),
        is_complete: available == declared,
      });
      cursor = end;
      if available < declared {
        return (fragments, Vec::new());
      }
    }
    (fragments, payload[cursor..].to_vec())
  }

  fn parse(payload: &[u8], property_count: usize) -> Option<Self> {
    let fixed_len = property_count.checked_mul(6)?;
    let fixed = payload.get(..fixed_len)?;
    let entries = fixed
      .chunks_exact(6)
      .map(|entry| {
        let opid = u16::from_le_bytes([entry[0], entry[1]]);
        OfficeArtPropertyEntry {
          property_id: opid & 0x3fff,
          is_blip_id: opid & 0x4000 != 0,
          is_complex: opid & 0x8000 != 0,
          value_or_declared_length: u32::from_le_bytes(entry[2..6].try_into().expect("four bytes")),
        }
      })
      .collect::<Vec<_>>();
    let (complex_fragments, trailing_data) =
      Self::split_complex_data(&entries, &payload[fixed_len..]);
    Some(Self {
      entries,
      incomplete_fixed_entry: OfficeArtIncompletePropertyEntry::None,
      complex_fragments,
      trailing_data,
      recovered_trailing: None,
    })
  }

  fn parse_partial(payload: &[u8], property_count: usize) -> Self {
    let available_entry_count = (payload.len() / 6).min(property_count);
    let complete_fixed_len = available_entry_count * 6;
    let entries = payload[..complete_fixed_len]
      .chunks_exact(6)
      .map(|entry| {
        let opid = u16::from_le_bytes([entry[0], entry[1]]);
        OfficeArtPropertyEntry {
          property_id: opid & 0x3fff,
          is_blip_id: opid & 0x4000 != 0,
          is_complex: opid & 0x8000 != 0,
          value_or_declared_length: u32::from_le_bytes(entry[2..6].try_into().expect("four bytes")),
        }
      })
      .collect::<Vec<_>>();
    let declared_fixed_len = property_count.saturating_mul(6);
    let incomplete_end = payload.len().min(declared_fixed_len);
    let (complex_fragments, trailing_data) =
      Self::split_complex_data(&entries, &payload[incomplete_end..]);
    Self {
      entries,
      incomplete_fixed_entry: OfficeArtIncompletePropertyEntry::parse(
        &payload[complete_fixed_len..incomplete_end],
      ),
      complex_fragments,
      trailing_data,
      recovered_trailing: None,
    }
  }

  fn write(&self, payload: &mut Vec<u8>) -> Result<()> {
    for entry in &self.entries {
      let opid = entry.property_id
        | if entry.is_blip_id { 0x4000 } else { 0 }
        | if entry.is_complex { 0x8000 } else { 0 };
      payload.extend_from_slice(&opid.to_le_bytes());
      payload.extend_from_slice(&entry.value_or_declared_length.to_le_bytes());
    }
    self.incomplete_fixed_entry.write(payload);
    for fragment in &self.complex_fragments {
      fragment.data.write(payload)?;
    }
    payload.extend_from_slice(&self.trailing_data);
    if let Some(sequence) = &self.recovered_trailing {
      payload.extend_from_slice(&sequence.to_bytes()?);
    }
    Ok(())
  }
}

impl OfficeArtRecoveredPrefix {
  pub fn encoded_len(&self) -> usize {
    match self {
      Self::Words2(_) => 8,
      Self::ClientAnchor(_) => 18,
    }
  }

  pub fn unparsed_byte_count(&self) -> usize {
    match self {
      Self::Words2(_) => 0,
      Self::ClientAnchor(_) => 0,
    }
  }

  fn write(&self, payload: &mut Vec<u8>) {
    match self {
      Self::Words2(words) => {
        for word in words {
          payload.extend_from_slice(&word.to_le_bytes());
        }
      }
      Self::ClientAnchor(anchor) => anchor.write(payload),
    }
  }
}

impl OfficeArtComplexPropertyData {
  pub fn encoded_len(&self) -> usize {
    match self {
      Self::Bytes(bytes) => bytes.len(),
      Self::Array(value) => 6 + value.data.encoded_len(),
    }
  }

  pub fn unparsed_byte_count(&self) -> usize {
    match self {
      Self::Bytes(bytes) => bytes.len(),
      Self::Array(_) => 0,
    }
  }

  fn write(&self, payload: &mut Vec<u8>) -> Result<()> {
    match self {
      Self::Bytes(bytes) => payload.extend_from_slice(bytes),
      Self::Array(value) => value.write(payload)?,
    }
    Ok(())
  }
}

impl OfficeArtIncompletePropertyEntry {
  fn parse(bytes: &[u8]) -> Self {
    match bytes {
      [] => Self::None,
      [opid0, opid1, value0, value1] => {
        let opid = u16::from_le_bytes([*opid0, *opid1]);
        Self::LowWord {
          property_id: opid & 0x3fff,
          is_blip_id: opid & 0x4000 != 0,
          is_complex: opid & 0x8000 != 0,
          value_low: u16::from_le_bytes([*value0, *value1]),
        }
      }
      _ => Self::Other(bytes.to_vec()),
    }
  }

  pub fn encoded_len(&self) -> usize {
    match self {
      Self::None => 0,
      Self::LowWord { .. } => 4,
      Self::Other(bytes) => bytes.len(),
    }
  }

  pub fn unparsed_byte_count(&self) -> usize {
    match self {
      Self::None | Self::LowWord { .. } => 0,
      Self::Other(bytes) => bytes.len(),
    }
  }

  fn write(&self, payload: &mut Vec<u8>) {
    match self {
      Self::None => {}
      Self::LowWord {
        property_id,
        is_blip_id,
        is_complex,
        value_low,
      } => {
        let opid = *property_id
          | if *is_blip_id { 0x4000 } else { 0 }
          | if *is_complex { 0x8000 } else { 0 };
        payload.extend_from_slice(&opid.to_le_bytes());
        payload.extend_from_slice(&value_low.to_le_bytes());
      }
      Self::Other(bytes) => payload.extend_from_slice(bytes),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn shared_record_header_is_byte_exact_and_enforces_bit_widths() {
    let header = OfficeArtRecordHeader {
      version: 5,
      instance: 0x0abc,
      record_type: 0xf119,
      declared_length: 0x1122_3344,
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    header.write_to(&mut writer).unwrap();
    let bytes = writer.into_inner().into_inner();
    assert_eq!(bytes, [0xc5, 0xab, 0x19, 0xf1, 0x44, 0x33, 0x22, 0x11]);
    assert_eq!(header.sdk_size(), HEADER_LEN as u64);
    let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
    assert_eq!(
      OfficeArtRecordHeader::read_from(&mut reader).unwrap(),
      header
    );

    for invalid in [
      OfficeArtRecordHeader {
        version: 0x10,
        ..header
      },
      OfficeArtRecordHeader {
        instance: 0x1000,
        ..header
      },
    ] {
      assert!(
        invalid
          .write_to(&mut Writer::new(Cursor::new(Vec::new())))
          .is_err()
      );
    }
  }

  #[test]
  fn nested_container_and_atom_round_trip() {
    let bytes = [
      0x0f, 0x00, 0x00, 0xf0, 0x0b, 0, 0, 0, 0x12, 0x00, 0x08, 0xf0, 0x03, 0, 0, 0, 1, 2, 3,
    ];
    let parsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.records.len(), 1);
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn known_container_with_atom_recver_keeps_typed_children() {
    let bytes = [
      0x00, 0x00, 0x02, 0xf0, 0x10, 0, 0, 0, 0x10, 0x00, 0x08, 0xf0, 0x08, 0, 0, 0, 3, 0, 0, 0, 2,
      4, 0, 0,
    ];
    let parsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    let OfficeArtRecordData::CompatibilityContainer(children) = &parsed.records[0].data else {
      panic!("expected a compatibility container");
    };
    assert!(matches!(
      children.as_slice(),
      [OfficeArtRecord {
        data: OfficeArtRecordData::Drawing(_),
        ..
      }]
    ));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn damaged_record_ids_keep_property_table_and_anchors_typed() {
    let property_payload = [
      0xbf, 0x00, 0x08, 0x00, 0x08, 0x00, 0x44, 0x01, 0x04, 0x00, 0x00, 0x00, 0x7f, 0x01, 0x00,
      0x00, 0x01, 0x00, 0xbf, 0x01, 0x00, 0x00, 0x11, 0x00, 0xc0, 0x01, 0x40, 0x00, 0x00, 0x08,
      0xd1, 0x01, 0x01, 0x00, 0x00, 0x00, 0xff, 0x01, 0x10, 0x00, 0x10, 0x00, 0xbf, 0x03, 0x00,
      0x00, 0x08, 0x00,
    ];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0100u16.to_le_bytes());
    bytes.extend_from_slice(&0xf043u16.to_le_bytes());
    bytes.extend_from_slice(&48u32.to_le_bytes());
    bytes.extend_from_slice(&property_payload);
    bytes.extend_from_slice(&0x0001u16.to_le_bytes());
    bytes.extend_from_slice(&0xf0aau16.to_le_bytes());
    bytes.extend_from_slice(&16u32.to_le_bytes());
    for coordinate in [5904i32, 576, 6552, 3888] {
      bytes.extend_from_slice(&coordinate.to_le_bytes());
    }
    bytes.extend_from_slice(&0x002du16.to_le_bytes());
    bytes.extend_from_slice(&0x0000u16.to_le_bytes());
    bytes.extend_from_slice(&16u32.to_le_bytes());
    for coordinate in [5904i32, 3888, 6048, 3888] {
      bytes.extend_from_slice(&coordinate.to_le_bytes());
    }

    let parsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    assert!(matches!(
        &parsed.records[0].data,
        OfficeArtRecordData::PropertyTable(table) if table.properties.len() == 8
    ));
    assert!(matches!(
      parsed.records[1].data,
      OfficeArtRecordData::ChildAnchor(_)
    ));
    assert!(matches!(
      parsed.records[2].data,
      OfficeArtRecordData::ChildAnchor(_)
    ));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn softmaker_native_properties_have_bounded_static_subrecords() {
    let mut payload = Vec::new();
    payload.extend_from_slice(&9u16.to_le_bytes());
    payload.extend_from_slice(&0u16.to_le_bytes());
    payload.extend_from_slice(&4u32.to_le_bytes());
    payload.extend_from_slice(&0x1234_5678u32.to_le_bytes());
    payload.extend_from_slice(&12u16.to_le_bytes());
    payload.extend_from_slice(&0u16.to_le_bytes());
    payload.extend_from_slice(&16u32.to_le_bytes());
    for word in [1u32, 2, 3, 4] {
      payload.extend_from_slice(&word.to_le_bytes());
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0040u16.to_le_bytes());
    bytes.extend_from_slice(&0xf150u16.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&payload);

    let parsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    let OfficeArtRecordData::SoftMakerNativeProperties(value) = &parsed.records[0].data else {
      panic!("expected SoftMaker native properties");
    };
    assert!(matches!(
      value.properties[0].data,
      SoftMakerNativePropertyData::Selector9(0x1234_5678)
    ));
    assert!(matches!(
      value.properties[1].data,
      SoftMakerNativePropertyData::Selector12 {
        words: [1, 2, 3, 4]
      }
    ));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn damaged_fbse_id_and_empty_marker_are_static() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0002u16.to_le_bytes());
    bytes.extend_from_slice(&0xe007u16.to_le_bytes());
    bytes.extend_from_slice(&36u32.to_le_bytes());
    let mut fbse = [0u8; 36];
    fbse[18] = 0xff;
    bytes.extend_from_slice(&fbse);
    bytes.extend_from_slice(&0x0000u16.to_le_bytes());
    bytes.extend_from_slice(&0xf08du16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());

    let parsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    assert!(matches!(
      parsed.records[0].data,
      OfficeArtRecordData::Fbse(OfficeArtFbse { tag: 0x00ff, .. })
    ));
    assert!(matches!(
      parsed.records[1].data,
      OfficeArtRecordData::EmptyCompatibilityAtom
    ));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn pict_blip_uses_a_named_editable_leaf() {
    let decoded = vec![0x00, 0x11, 0x02, 0xff];
    let blip = OfficeArtMetafileBlip {
      uid1: [0x5a; 16],
      uid2: None,
      metafile_header: OfficeArtMetafileHeader {
        uncompressed_size: decoded.len() as u32,
        bounds: OfficeArtRect {
          left: 0,
          top: 0,
          right: 10,
          bottom: 20,
        },
        render_size: OfficeArtPoint { x: 30, y: 40 },
        saved_size: decoded.len() as u32,
        compression: 0xfe,
        filter: 0xfe,
      },
      file_data: OfficeArtMetafileData::Pict(OfficeArtMetafileBytes::new(decoded.clone(), decoded)),
    };
    let record = OfficeArtRecord {
      header: OfficeArtRecordHeader {
        version: 0,
        instance: 0x542,
        record_type: 0xf01c,
        declared_length: 54,
      },
      data: OfficeArtRecordData::MetafileBlip(blip),
    };
    let stream = OfficeArtStream {
      records: vec![record],
    };
    let bytes = stream.to_bytes().unwrap();
    let reparsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    assert!(matches!(
        &reparsed.records[0].data,
        OfficeArtRecordData::MetafileBlip(OfficeArtMetafileBlip {
            file_data: OfficeArtMetafileData::Pict(data),
            ..
        }) if data.decoded() == [0x00, 0x11, 0x02, 0xff]
    ));
    assert_eq!(reparsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn metafile_clone_shares_exact_bytes_until_edited() {
    let source = OfficeArtMetafileBytes::new(vec![1, 2, 3], vec![4, 5, 6]);
    let mut cloned = source.clone();

    assert!(Arc::ptr_eq(&source.decoded, &cloned.decoded));
    assert!(Arc::ptr_eq(
      &source.original_decoded,
      &cloned.original_decoded
    ));
    assert!(Arc::ptr_eq(
      &source.original_encoded,
      &cloned.original_encoded
    ));
    cloned.decoded_mut()[0] = 7;
    assert_eq!(source.decoded(), [1, 2, 3]);
    assert_eq!(cloned.decoded(), [7, 2, 3]);
    assert!(!Arc::ptr_eq(&source.decoded, &cloned.decoded));
    assert!(!cloned.is_unchanged());
  }

  #[test]
  fn rejects_container_garbage_and_truncated_atoms() {
    let container = [0x0f, 0, 0, 0xf0, 1, 0, 0, 0, 0];
    assert!(OfficeArtStream::from_bytes(&container).is_err());
    let atom = [0, 0, 8, 0xf0, 4, 0, 0, 0, 1];
    assert!(OfficeArtStream::from_bytes(&atom).is_err());
  }

  #[test]
  fn partial_tree_preserves_typed_prefix_and_recursive_truncation() {
    let bytes = [
      0x0f, 0x00, 0x02, 0xf0, 40, 0, 0, 0, 0x10, 0x00, 0x08, 0xf0, 8, 0, 0, 0, 3, 0, 0, 0, 2, 4, 0,
      0, 0x0f, 0x00, 0x03, 0xf0, 16, 0, 0, 0, 1, 2, 3, 4,
    ];
    assert!(OfficeArtStream::from_bytes(&bytes).is_err());
    let partial = OfficeArtPartialStream::from_bytes_with_limits(
      &bytes,
      Limits::default(),
      "truncated fixture".into(),
    )
    .unwrap();
    assert_eq!(partial.complete_record_count(), 1);
    assert_eq!(partial.incomplete_record_count(), 2);
    assert_eq!(partial.unparsed_byte_count(), 4);
    assert_eq!(partial.available_len(), bytes.len());
    assert_eq!(partial.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn partial_tree_recovers_fbse_with_underreported_length() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0062u16.to_le_bytes());
    bytes.extend_from_slice(&0xf007u16.to_le_bytes());
    bytes.extend_from_slice(&65u32.to_le_bytes());

    let mut fbse = [0u8; 36];
    fbse[0] = 6;
    fbse[1] = 6;
    fbse[20..24].copy_from_slice(&33u32.to_le_bytes());
    fbse[24..28].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&fbse);

    bytes.extend_from_slice(&0x6e00u16.to_le_bytes());
    bytes.extend_from_slice(&0xf01eu16.to_le_bytes());
    bytes.extend_from_slice(&25u32.to_le_bytes());
    bytes.extend_from_slice(&[0x55; 16]);
    bytes.push(0xff);
    bytes.extend_from_slice(b"\x89PNG\r\n\x1a\n");

    assert!(OfficeArtStream::from_bytes(&bytes).is_err());
    let partial = OfficeArtPartialStream::from_bytes_with_limits(
      &bytes,
      Limits::default(),
      "underreported FBSE fixture".into(),
    )
    .unwrap();
    assert_eq!(partial.complete_record_count(), 1);
    assert_eq!(partial.incomplete_record_count(), 1);
    assert_eq!(partial.unparsed_byte_count(), 0);
    partial.visit_incomplete(|record| {
      let OfficeArtIncompleteRecordData::FbseWithUnderreportedLength(fbse) = &record.data else {
        panic!("expected an underreported FBSE");
      };
      assert_eq!(record.header.declared_length, 65);
      assert!(fbse.trailing.is_empty());
      assert!(fbse.embedded_blip.is_some());
    });
    assert_eq!(partial.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn property_table_preserves_simple_and_complex_properties() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0023u16.to_le_bytes());
    bytes.extend_from_slice(&0xf00bu16.to_le_bytes());
    bytes.extend_from_slice(&15u32.to_le_bytes());
    bytes.extend_from_slice(&5u16.to_le_bytes());
    bytes.extend_from_slice(&42u32.to_le_bytes());
    bytes.extend_from_slice(&0x8007u16.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"abc");

    let parsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    let OfficeArtRecordData::PropertyTable(table) = &parsed.records[0].data else {
      panic!("expected a typed property table");
    };
    assert_eq!(table.properties.len(), 2);
    assert_eq!(
      table.properties[0].value,
      OfficeArtPropertyValue::Simple(42)
    );
    assert_eq!(
      table.properties[1].value,
      OfficeArtPropertyValue::Complex {
        declared_length: 3,
        data: b"abc".to_vec(),
      }
    );
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn property_table_types_utf16_complex_properties() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0013u16.to_le_bytes());
    bytes.extend_from_slice(&0xf00bu16.to_le_bytes());
    bytes.extend_from_slice(&14u32.to_le_bytes());
    bytes.extend_from_slice(&0x8380u16.to_le_bytes());
    bytes.extend_from_slice(&8u32.to_le_bytes());
    for unit in [b'N' as u16, b'a' as u16, 0xd800, 0] {
      bytes.extend_from_slice(&unit.to_le_bytes());
    }

    let parsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    let OfficeArtRecordData::PropertyTable(table) = &parsed.records[0].data else {
      panic!("expected a typed property table");
    };
    assert_eq!(
      table.properties[0].value,
      OfficeArtPropertyValue::Utf16String {
        declared_length: 8,
        code_units: vec![b'N' as u16, b'a' as u16, 0xd800, 0],
      }
    );
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn property_table_keeps_zero_length_complex_flag_explicit() {
    let bytes = [
      0x13, 0x00, 0x0b, 0xf0, 0x06, 0, 0, 0, 0xa1, 0x81, 0, 0, 0, 0,
    ];
    let parsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    let OfficeArtRecordData::PropertyTable(table) = &parsed.records[0].data else {
      panic!("expected a typed property table");
    };
    assert!(matches!(
      table.properties[0].value,
      OfficeArtPropertyValue::EmptyComplex { declared_length: 0 }
    ));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn property_table_types_metro_blob_zip_directory() {
    let mut package = Vec::new();
    package.extend_from_slice(b"PK\x03\x04");
    package.extend_from_slice(&20u16.to_le_bytes());
    package.extend_from_slice(&[0; 20]);
    package.extend_from_slice(&1u16.to_le_bytes());
    package.extend_from_slice(&0u16.to_le_bytes());
    package.push(b'a');
    assert_eq!(package.len(), 31);

    package.extend_from_slice(b"PK\x01\x02");
    package.extend_from_slice(&20u16.to_le_bytes());
    package.extend_from_slice(&20u16.to_le_bytes());
    package.extend_from_slice(&[0; 20]);
    package.extend_from_slice(&1u16.to_le_bytes());
    package.extend_from_slice(&0u16.to_le_bytes());
    package.extend_from_slice(&0u16.to_le_bytes());
    package.extend_from_slice(&0u16.to_le_bytes());
    package.extend_from_slice(&0u16.to_le_bytes());
    package.extend_from_slice(&0u32.to_le_bytes());
    package.extend_from_slice(&0u32.to_le_bytes());
    package.push(b'a');
    assert_eq!(package.len(), 78);

    package.extend_from_slice(b"PK\x05\x06");
    package.extend_from_slice(&0u16.to_le_bytes());
    package.extend_from_slice(&0u16.to_le_bytes());
    package.extend_from_slice(&1u16.to_le_bytes());
    package.extend_from_slice(&1u16.to_le_bytes());
    package.extend_from_slice(&47u32.to_le_bytes());
    package.extend_from_slice(&31u32.to_le_bytes());
    package.extend_from_slice(&0u16.to_le_bytes());

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0013u16.to_le_bytes());
    bytes.extend_from_slice(&0xf00bu16.to_le_bytes());
    bytes.extend_from_slice(&106u32.to_le_bytes());
    bytes.extend_from_slice(&0x83a9u16.to_le_bytes());
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&package);

    let parsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    let OfficeArtRecordData::PropertyTable(table) = &parsed.records[0].data else {
      panic!("expected a typed property table");
    };
    let OfficeArtPropertyValue::MetroBlob { value, .. } = &table.properties[0].value else {
      panic!("expected a typed metroBlob");
    };
    assert_eq!(value.directory.entry_count, 1);
    assert_eq!(value.directory.entries[0].file_name, b"a");
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn property_table_reuses_static_hyperlink_for_ihlink() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0013u16.to_le_bytes());
    bytes.extend_from_slice(&0xf00bu16.to_le_bytes());
    bytes.extend_from_slice(&30u32.to_le_bytes());
    bytes.extend_from_slice(&0x8382u16.to_le_bytes());
    bytes.extend_from_slice(&24u32.to_le_bytes());
    bytes.extend_from_slice(&STANDARD_HYPERLINK_CLASS_ID);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());

    let parsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    let OfficeArtRecordData::PropertyTable(table) = &parsed.records[0].data else {
      panic!("expected a typed property table");
    };
    assert!(matches!(
        &table.properties[0].value,
        OfficeArtPropertyValue::Hyperlink {
            class_id,
            object: crate::xls::HyperlinkObject::Parsed {
                stream_version: 2,
                flags,
                trailing,
                ..
            },
            ..
        } if class_id == &STANDARD_HYPERLINK_CLASS_ID && flags.is_empty() && trailing.is_empty()
    ));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn property_table_types_arrays_and_preserves_damaged_length_delta() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0023u16.to_le_bytes());
    bytes.extend_from_slice(&0xf00bu16.to_le_bytes());
    bytes.extend_from_slice(&30u32.to_le_bytes());
    bytes.extend_from_slice(&0x8145u16.to_le_bytes());
    bytes.extend_from_slice(&5u32.to_le_bytes());
    bytes.extend_from_slice(&0x8146u16.to_le_bytes());
    bytes.extend_from_slice(&8u32.to_le_bytes());

    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&0xfff0u16.to_le_bytes());
    bytes.extend_from_slice(&12i16.to_le_bytes());
    bytes.extend_from_slice(&(-34i16).to_le_bytes());

    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0x4000u16.to_le_bytes());

    let parsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    let OfficeArtRecordData::PropertyTable(table) = &parsed.records[0].data else {
      panic!("expected a typed property table");
    };
    assert!(matches!(
        &table.properties[0].value,
        OfficeArtPropertyValue::Array {
            declared_length: 5,
            declared_length_delta: 5,
            value: OfficeArtArray {
                data: OfficeArtArrayData::Points16(points),
                ..
            },
        } if points == &[OfficeArtPoint16 { x: 12, y: -34 }]
    ));
    assert!(matches!(
        &table.properties[1].value,
        OfficeArtPropertyValue::Array {
            declared_length: 8,
            declared_length_delta: 0,
            value: OfficeArtArray {
                data: OfficeArtArrayData::Segments(segments),
                ..
            },
        } if segments == &[0x4000]
    ));
    assert!(table.trailing.is_empty());
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn incomplete_property_table_preserves_fixed_entries_and_available_complex_data() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0023u16.to_le_bytes());
    bytes.extend_from_slice(&0xf00bu16.to_le_bytes());
    bytes.extend_from_slice(&14u32.to_le_bytes());
    bytes.extend_from_slice(&5u16.to_le_bytes());
    bytes.extend_from_slice(&42u32.to_le_bytes());
    bytes.extend_from_slice(&0x8007u16.to_le_bytes());
    bytes.extend_from_slice(&5u32.to_le_bytes());
    bytes.extend_from_slice(b"ab");

    let parsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    let OfficeArtRecordData::IncompletePropertyTable(table) = &parsed.records[0].data else {
      panic!("expected an incomplete property table");
    };
    assert_eq!(table.entries.len(), 2);
    assert_eq!(table.complex_fragments.len(), 1);
    assert_eq!(
      table.complex_fragments[0].data,
      OfficeArtComplexPropertyData::Bytes(b"ab".to_vec())
    );
    assert!(!table.complex_fragments[0].is_complete);
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn incomplete_property_table_recovers_self_describing_arrays() {
    let entries = [
      OfficeArtPropertyEntry {
        property_id: 0x0145,
        is_blip_id: false,
        is_complex: true,
        value_or_declared_length: 36,
      },
      OfficeArtPropertyEntry {
        property_id: 0x0146,
        is_blip_id: false,
        is_complex: true,
        value_or_declared_length: 0x6430_002c,
      },
    ];
    let mut payload = Vec::new();
    payload.extend_from_slice(&9u16.to_le_bytes());
    payload.extend_from_slice(&9u16.to_le_bytes());
    payload.extend_from_slice(&0xfff0u16.to_le_bytes());
    for point in 0..9i16 {
      payload.extend_from_slice(&point.to_le_bytes());
      payload.extend_from_slice(&(-point).to_le_bytes());
    }
    payload.extend_from_slice(&19u16.to_le_bytes());
    payload.extend_from_slice(&20u16.to_le_bytes());
    payload.extend_from_slice(&2u16.to_le_bytes());
    for segment in 0..19u16 {
      payload.extend_from_slice(&segment.to_le_bytes());
    }

    let (fragments, trailing) =
      OfficeArtIncompletePropertyTable::split_complex_data(&entries, &payload);
    assert!(trailing.is_empty());
    assert_eq!(fragments.len(), 2);
    assert!(fragments[0].is_complete);
    assert!(!fragments[1].is_complete);
    assert!(matches!(
        &fragments[0].data,
        OfficeArtComplexPropertyData::Array(OfficeArtArray {
            data: OfficeArtArrayData::Points16(points),
            ..
        }) if points.len() == 9
    ));
    assert!(matches!(
        &fragments[1].data,
        OfficeArtComplexPropertyData::Array(OfficeArtArray {
            data: OfficeArtArrayData::Segments(segments),
            ..
        }) if segments.len() == 19
    ));
  }

  #[test]
  fn fbse_exposes_the_embedded_blip_record() {
    let mut payload = vec![0; 36];
    payload[0] = 6;
    payload[1] = 6;
    payload[20..24].copy_from_slice(&18u32.to_le_bytes());
    payload[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
    payload.extend_from_slice(&0x6e00u16.to_le_bytes());
    payload.extend_from_slice(&0xf01eu16.to_le_bytes());
    payload.extend_from_slice(&18u32.to_le_bytes());
    payload.extend_from_slice(&[0x11; 16]);
    payload.push(0xff);
    payload.push(0x89);

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0062u16.to_le_bytes());
    bytes.extend_from_slice(&0xf007u16.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&payload);

    let parsed = OfficeArtStream::from_bytes(&bytes).unwrap();
    let OfficeArtRecordData::Fbse(fbse) = &parsed.records[0].data else {
      panic!("expected a typed FBSE");
    };
    assert!(matches!(
      fbse.embedded_blip.as_deref().map(|record| &record.data),
      Some(OfficeArtRecordData::BitmapBlip(_))
    ));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn relayout_rebuilds_local_record_counts_and_property_lengths() {
    let mut stream = OfficeArtStream {
      records: vec![
        OfficeArtRecord {
          header: OfficeArtRecordHeader {
            version: 0,
            instance: 0,
            record_type: 0xf006,
            declared_length: 24,
          },
          data: OfficeArtRecordData::DggBlock(OfficeArtDggBlock {
            maximum_shape_id: 1024,
            declared_cluster_count: 2,
            saved_shape_count: 1,
            saved_drawing_count: 1,
            clusters: vec![OfficeArtIdCluster {
              drawing_id: 1,
              current_shape_id_count: 2,
            }],
          }),
        },
        OfficeArtRecord {
          header: OfficeArtRecordHeader {
            version: 0,
            instance: 1,
            record_type: 0xf118,
            declared_length: 4,
          },
          data: OfficeArtRecordData::Frit(vec![OfficeArtFrit {
            new_group_id: 2,
            old_group_id: 1,
          }]),
        },
        OfficeArtRecord {
          header: OfficeArtRecordHeader {
            version: 0,
            instance: 1,
            record_type: 0xf11a,
            declared_length: 4,
          },
          data: OfficeArtRecordData::ColorMru(vec![OfficeArtColor(0x0011_2233)]),
        },
        OfficeArtRecord {
          header: OfficeArtRecordHeader {
            version: 3,
            instance: 1,
            record_type: 0xf00b,
            declared_length: 10,
          },
          data: OfficeArtRecordData::PropertyTable(OfficeArtPropertyTable {
            properties: vec![OfficeArtProperty {
              property_id: 0x0380,
              is_blip_id: false,
              value: OfficeArtPropertyValue::Utf16String {
                declared_length: 4,
                code_units: vec![u16::from(b'A'), 0],
              },
            }],
            trailing: Vec::new(),
          }),
        },
        OfficeArtRecord {
          header: OfficeArtRecordHeader {
            version: 3,
            instance: 1,
            record_type: 0xf121,
            declared_length: 14,
          },
          data: OfficeArtRecordData::PropertyTable(OfficeArtPropertyTable {
            properties: vec![OfficeArtProperty {
              property_id: 0x0146,
              is_blip_id: false,
              value: OfficeArtPropertyValue::Array {
                declared_length: 8,
                declared_length_delta: 0,
                value: OfficeArtArray {
                  element_count: 1,
                  allocated_element_count: 1,
                  encoded_element_size: 2,
                  data: OfficeArtArrayData::Segments(vec![1]),
                },
              },
            }],
            trailing: Vec::new(),
          }),
        },
        OfficeArtRecord {
          header: OfficeArtRecordHeader {
            version: 0x0f,
            instance: 0,
            record_type: 0xf001,
            declared_length: 0,
          },
          data: OfficeArtRecordData::Container(vec![OfficeArtRecord {
            header: OfficeArtRecordHeader {
              version: 0,
              instance: 0,
              record_type: 0xf018,
              declared_length: 0,
            },
            data: OfficeArtRecordData::Atom(Vec::new()),
          }]),
        },
      ],
    };

    let OfficeArtRecordData::DggBlock(dgg) = &mut stream.records[0].data else {
      unreachable!()
    };
    dgg.clusters.push(OfficeArtIdCluster {
      drawing_id: 2,
      current_shape_id_count: 3,
    });
    let OfficeArtRecordData::Frit(values) = &mut stream.records[1].data else {
      unreachable!()
    };
    values.push(OfficeArtFrit {
      new_group_id: 4,
      old_group_id: 3,
    });
    let OfficeArtRecordData::ColorMru(colors) = &mut stream.records[2].data else {
      unreachable!()
    };
    colors.push(OfficeArtColor(0x0044_5566));
    let OfficeArtRecordData::PropertyTable(table) = &mut stream.records[3].data else {
      unreachable!()
    };
    let OfficeArtPropertyValue::Utf16String { code_units, .. } = &mut table.properties[0].value
    else {
      unreachable!()
    };
    code_units.insert(1, u16::from(b'B'));
    table.properties.push(OfficeArtProperty {
      property_id: 0x0181,
      is_blip_id: false,
      value: OfficeArtPropertyValue::Simple(7),
    });
    let OfficeArtRecordData::PropertyTable(table) = &mut stream.records[4].data else {
      unreachable!()
    };
    let OfficeArtPropertyValue::Array { value, .. } = &mut table.properties[0].value else {
      unreachable!()
    };
    let OfficeArtArrayData::Segments(segments) = &mut value.data else {
      unreachable!()
    };
    segments.push(2);

    stream.relayout().unwrap();
    let OfficeArtRecordData::DggBlock(dgg) = &stream.records[0].data else {
      unreachable!()
    };
    assert_eq!(dgg.declared_cluster_count, 3);
    assert_eq!(stream.records[0].header.declared_length, 32);
    assert_eq!(stream.records[1].header.instance, 2);
    assert_eq!(stream.records[1].header.declared_length, 8);
    assert_eq!(stream.records[2].header.instance, 2);
    assert_eq!(stream.records[2].header.declared_length, 8);
    assert_eq!(stream.records[3].header.instance, 2);
    assert_eq!(stream.records[3].header.declared_length, 18);
    let OfficeArtRecordData::PropertyTable(table) = &stream.records[3].data else {
      unreachable!()
    };
    assert!(matches!(
      table.properties[0].value,
      OfficeArtPropertyValue::Utf16String {
        declared_length: 6,
        ..
      }
    ));
    let OfficeArtRecordData::PropertyTable(table) = &stream.records[4].data else {
      unreachable!()
    };
    assert!(matches!(
      table.properties[0].value,
      OfficeArtPropertyValue::Array {
        declared_length: 10,
        value: OfficeArtArray {
          element_count: 2,
          allocated_element_count: 2,
          ..
        },
        ..
      }
    ));
    assert_eq!(stream.records[4].header.declared_length, 16);
    assert_eq!(stream.records[5].header.instance, 1);
    assert_eq!(stream.records[5].header.declared_length, 8);

    let bytes = stream.to_bytes().unwrap();
    assert_eq!(OfficeArtStream::from_bytes(&bytes).unwrap(), stream);

    let mut oversized = OfficeArtStream {
      records: vec![OfficeArtRecord {
        header: OfficeArtRecordHeader {
          version: 0,
          instance: 0,
          record_type: 0xf11a,
          declared_length: 0,
        },
        data: OfficeArtRecordData::ColorMru(vec![OfficeArtColor(0); 0x1000]),
      }],
    };
    let unchanged = oversized.clone();
    assert!(oversized.relayout().is_err());
    assert_eq!(oversized, unchanged);
  }

  #[test]
  fn drawing_graph_aggregates_ids_and_keeps_count_conventions_explicit() {
    let mut drawing_group = OfficeArtStream {
      records: vec![OfficeArtRecord {
        header: OfficeArtRecordHeader {
          version: 0x0f,
          instance: 0,
          record_type: 0xf000,
          declared_length: 0,
        },
        data: OfficeArtRecordData::Container(vec![OfficeArtRecord {
          header: OfficeArtRecordHeader {
            version: 0,
            instance: 0,
            record_type: 0xf006,
            declared_length: 0,
          },
          data: OfficeArtRecordData::DggBlock(OfficeArtDggBlock {
            maximum_shape_id: 1025,
            declared_cluster_count: 2,
            saved_shape_count: 2,
            saved_drawing_count: 1,
            clusters: vec![OfficeArtIdCluster {
              drawing_id: 1,
              current_shape_id_count: 2,
            }],
          }),
        }]),
      }],
    };
    let mut drawing = OfficeArtStream {
      records: vec![OfficeArtRecord {
        header: OfficeArtRecordHeader {
          version: 0x0f,
          instance: 0,
          record_type: 0xf002,
          declared_length: 0,
        },
        data: OfficeArtRecordData::Container(vec![
          OfficeArtRecord {
            header: OfficeArtRecordHeader {
              version: 0,
              instance: 1,
              record_type: 0xf008,
              declared_length: 8,
            },
            data: OfficeArtRecordData::Drawing(OfficeArtDrawing {
              shape_count: 2,
              current_shape_id: 1025,
            }),
          },
          OfficeArtRecord {
            header: OfficeArtRecordHeader {
              version: 2,
              instance: 0,
              record_type: 0xf00a,
              declared_length: 8,
            },
            data: OfficeArtRecordData::Shape(OfficeArtShape {
              shape_id: 1024,
              flags: OfficeArtShapeFlags::PATRIARCH,
            }),
          },
          OfficeArtRecord {
            header: OfficeArtRecordHeader {
              version: 2,
              instance: 1,
              record_type: 0xf00a,
              declared_length: 8,
            },
            data: OfficeArtRecordData::Shape(OfficeArtShape {
              shape_id: 1025,
              flags: OfficeArtShapeFlags::empty(),
            }),
          },
        ]),
      }],
    };
    drawing_group.relayout().unwrap();
    drawing.relayout().unwrap();

    let graph = OfficeArtDrawingGraph::from_streams(&drawing_group, &[&drawing]).unwrap();
    assert_eq!(graph.drawings.len(), 1);
    assert_eq!(graph.drawings[0].drawing_id, 1);
    assert_eq!(graph.drawings[0].shapes.len(), 2);
    assert_eq!(graph.drawings[0].patriarch_shape_count, 1);
    assert_eq!(
      graph.drawings[0].shape_count_basis,
      OfficeArtShapeCountBasis::AllPresentShapes
    );
    assert_eq!(
      graph.maximum_shape_id_relation,
      OfficeArtHighWaterRelation::EqualToPresentTree
    );
    assert_eq!(
      graph.clusters[0].shape_id_count_relation,
      OfficeArtHighWaterRelation::EqualToPresentTree
    );
    graph.validate_strict().unwrap();

    let mut next_id_compatibility = drawing_group.clone();
    next_id_compatibility.visit_mut(|record| {
      if let OfficeArtRecordData::DggBlock(dgg) = &mut record.data {
        dgg.maximum_shape_id += 1;
      }
    });
    let graph = OfficeArtDrawingGraph::from_streams(&next_id_compatibility, &[&drawing]).unwrap();
    assert_eq!(
      graph.maximum_shape_id_relation,
      OfficeArtHighWaterRelation::AbovePresentTree
    );
    assert!(graph.validate_strict().is_err());

    let OfficeArtRecordData::Container(children) = &mut drawing.records[0].data else {
      unreachable!()
    };
    let OfficeArtRecordData::Drawing(fdg) = &mut children[0].data else {
      unreachable!()
    };
    fdg.shape_count = 1;
    let graph = OfficeArtDrawingGraph::from_streams(&drawing_group, &[&drawing]).unwrap();
    assert_eq!(
      graph.drawings[0].shape_count_basis,
      OfficeArtShapeCountBasis::ExcludesPatriarchShapes
    );
    assert!(graph.validate_strict().is_err());
  }

  #[test]
  fn empty_drawing_graph_accepts_exact_zero_counts() {
    let graph = OfficeArtDrawingGraph::from_components(
      OfficeArtDggBlock {
        maximum_shape_id: 0,
        declared_cluster_count: 1,
        saved_shape_count: 0,
        saved_drawing_count: 0,
        clusters: Vec::new(),
      },
      Vec::new(),
    )
    .unwrap();
    assert_eq!(
      graph.maximum_shape_id_relation,
      OfficeArtHighWaterRelation::EmptyZero
    );
    assert_eq!(
      graph.saved_shape_count_relation,
      OfficeArtHighWaterRelation::EqualToPresentTree
    );
    assert_eq!(
      graph.saved_drawing_count_relation,
      OfficeArtHighWaterRelation::EqualToPresentTree
    );
    graph.validate_strict().unwrap();
  }

  #[test]
  fn blip_graph_resolves_one_based_properties_and_fbse_reference_counts() {
    let references = vec![
      OfficeArtBlipReference {
        drawing_id: None,
        property_record_type: 0xf00b,
        property_table_index: 0,
        property_index: 0,
        property_id: 0x0104,
        blip_identifier: 1,
      },
      OfficeArtBlipReference {
        drawing_id: None,
        property_record_type: 0xf122,
        property_table_index: 1,
        property_index: 0,
        property_id: 0x0186,
        blip_identifier: 1,
      },
      OfficeArtBlipReference {
        drawing_id: None,
        property_record_type: 0xf00b,
        property_table_index: 2,
        property_index: 0,
        property_id: 0x01c5,
        blip_identifier: 2,
      },
    ];
    let graph = OfficeArtDrawingGraph::from_components_with_blips(
      OfficeArtDggBlock {
        maximum_shape_id: 0,
        declared_cluster_count: 1,
        saved_shape_count: 0,
        saved_drawing_count: 0,
        clusters: Vec::new(),
      },
      vec![OfficeArtGraphBlipStoreInput {
        declared_entry_count: 2,
        entries: vec![
          OfficeArtGraphBlipStoreEntryInput {
            record_type: 0xf007,
            fbse: Some((2, 0, true)),
          },
          OfficeArtGraphBlipStoreEntryInput {
            record_type: 0xf01e,
            fbse: None,
          },
        ],
      }],
      references,
      Vec::new(),
      Vec::new(),
    )
    .unwrap();

    let store = graph.blip_store.as_ref().unwrap();
    assert!(store.entry_count_matches);
    assert_eq!(store.entries[0].blip_identifier, 1);
    assert_eq!(store.entries[0].actual_reference_count, 2);
    assert_eq!(
      store.entries[0].reference_count_relation,
      Some(OfficeArtBlipReferenceCountRelation::EqualToActual)
    );
    assert_eq!(store.entries[1].actual_reference_count, 1);
    assert_eq!(store.entries[1].reference_count_relation, None);
    graph.validate_strict().unwrap();
  }
}
