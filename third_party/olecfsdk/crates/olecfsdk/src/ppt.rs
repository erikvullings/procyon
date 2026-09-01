//! Static framing and incremental-save structures for the PPT97 binary stream.

mod file;

pub use file::{PptAppendUserEditReport, PptFile, PptHistoryStrategy};

use std::{
  collections::{BTreeMap, BTreeSet},
  io::{Cursor, Read, Write},
};

use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};

use crate::{
  Error, Result, SdkBitfield, SdkObject,
  cfb::CompoundFile,
  forms::ParentControlStorageModel,
  io::{BinaryFormat, Reader, SdkRead, SdkSize, SdkWrite, Writer},
  limits::Limits,
  office_art::{
    OfficeArtBStoreDelay, OfficeArtBStoreDelayLayout, OfficeArtBlipReference, OfficeArtDggBlock,
    OfficeArtDrawingGraph, OfficeArtGraphBlipStoreEntryInput, OfficeArtGraphBlipStoreInput,
    OfficeArtGraphDrawingInput, OfficeArtPartialStream, OfficeArtPropertyTableLocation,
    OfficeArtRecord, OfficeArtRecordData, OfficeArtStream,
    collect_office_art_record_blip_references,
  },
  parse::{ParseDiagnostic, ParseDiagnosticCode, ParseOutcome, SpecificationReference},
  vba::{LocatedVbaProject, VbaModuleSourceMutation, VbaProject},
};

const HEADER_LEN: usize = 8;
const MAX_RECORD_DEPTH: usize = 256;

pub const USER_EDIT_ATOM: u16 = 0x0ff5;
pub const CURRENT_USER_ATOM: u16 = 0x0ff6;
/// Absolute CFB path of the required MS-PPT presentation stream.
pub const POWERPOINT_DOCUMENT_STREAM_PATH: &str = "/PowerPoint Document";
/// Absolute CFB path of the required MS-PPT current-user stream.
pub const CURRENT_USER_STREAM_PATH: &str = "/Current User";
/// Absolute CFB path of the optional MS-PPT pictures stream.
pub const PICTURES_STREAM_PATH: &str = "/Pictures";
pub const DOCUMENT_CONTAINER: u16 = 0x03e8;
pub const SLIDE_CONTAINER: u16 = 0x03ee;
pub const NOTES_CONTAINER: u16 = 0x03f0;
pub const MAIN_MASTER_CONTAINER: u16 = 0x03f8;
pub const HANDOUT_CONTAINER: u16 = 0x0fc9;
pub const EXTERNAL_OLE_OBJECT_STORAGE: u16 = 0x1011;
pub const VBA_INFO_CONTAINER: u16 = 0x03ff;
pub const EXTERNAL_OBJECT_LIST_CONTAINER: u16 = 0x0409;
pub const DOCUMENT_INFO_LIST_CONTAINER: u16 = 0x07d0;
pub const EXTERNAL_OLE_EMBED_CONTAINER: u16 = 0x0fcc;
pub const EXTERNAL_OLE_LINK_CONTAINER: u16 = 0x0fce;
pub const EXTERNAL_OLE_CONTROL_CONTAINER: u16 = 0x0fee;
pub const SLIDE_LIST_WITH_TEXT_CONTAINER: u16 = 0x0ff0;
pub const ROUND_TRIP_CONTENT_MASTER_INFO_12_ATOM: u16 = 0x041e;
pub const ROUND_TRIP_THEME_12_ATOM: u16 = 0x040e;
pub const ROUND_TRIP_OART_TEXT_STYLES_12_ATOM: u16 = 0x0423;
pub const ROUND_TRIP_CUSTOM_TABLE_STYLES_12_ATOM: u16 = 0x0428;
pub const BINARY_TAG_DATA_BLOB: u16 = 0x138b;
pub const PROG_BINARY_TAG: u16 = 0x138a;
pub const PROG_TAGS: u16 = 0x1388;
pub const PERSIST_DIRECTORY_FULL_BLOCK: u16 = 0x1771;
pub const PERSIST_DIRECTORY_ATOM: u16 = 0x1772;
pub const DOCUMENT_ATOM: u16 = 0x03e9;
pub const SLIDE_ATOM: u16 = 0x03ef;
pub const NOTES_ATOM: u16 = 0x03f1;
pub const OUTLINE_TEXT_REF_ATOM: u16 = 0x0f9e;
pub const TEXT_HEADER_ATOM: u16 = 0x0f9f;
pub const TEXT_CHARS_ATOM: u16 = 0x0fa0;
pub const STYLE_TEXT_PROP_ATOM: u16 = 0x0fa1;
pub const TEXT_BYTES_ATOM: u16 = 0x0fa8;
pub const C_STRING_ATOM: u16 = 0x0fba;
pub const SLIDE_PERSIST_ATOM: u16 = 0x03f3;
pub const COLOR_SCHEME_ATOM: u16 = 0x07f0;
const PP_DRAWING: u16 = 0x040c;
pub const EXTERNAL_OBJECT_REF_ATOM: u16 = 0x0bc1;
pub const PLACEHOLDER_ATOM: u16 = 0x0bc3;
pub const HEADERS_FOOTERS_ATOM: u16 = 0x0fda;
pub const MASTER_TEXT_PROP_ATOM: u16 = 0x0fa2;
pub const TEXT_MASTER_STYLE_ATOM: u16 = 0x0fa3;
pub const TEXT_CF_EXCEPTION_ATOM: u16 = 0x0fa4;
pub const TEXT_PF_EXCEPTION_ATOM: u16 = 0x0fa5;
pub const TEXT_RULER_ATOM: u16 = 0x0fa6;
pub const DEFAULT_RULER_ATOM: u16 = 0x0fab;
pub const TEXT_SPECIAL_INFO_ATOM: u16 = 0x0faa;
pub const TEXT_SI_EXCEPTION_ATOM: u16 = 0x0fa9;
pub const STYLE_TEXT_PROP9_ATOM: u16 = 0x0fac;
pub const TEXT_MASTER_STYLE9_ATOM: u16 = 0x0fad;
pub const STYLE_TEXT_PROP10_ATOM: u16 = 0x0fb1;
pub const TEXT_MASTER_STYLE10_ATOM: u16 = 0x0fb2;
pub const TEXT_DEFAULTS10_ATOM: u16 = 0x0fb4;
pub const STYLE_TEXT_PROP11_ATOM: u16 = 0x0fb6;
pub const TIME_NODE_ATOM: u16 = 0xf127;
pub const TIME_CONDITION_ATOM: u16 = 0xf128;
pub const TIME_MODIFIER_ATOM: u16 = 0xf129;
pub const TIME_BEHAVIOR_ATOM: u16 = 0xf133;
pub const TIME_ANIMATE_BEHAVIOR_ATOM: u16 = 0xf134;
pub const TIME_EFFECT_BEHAVIOR_ATOM: u16 = 0xf136;
pub const TIME_MOTION_BEHAVIOR_ATOM: u16 = 0xf137;
pub const TIME_SCALE_BEHAVIOR_ATOM: u16 = 0xf139;
pub const TIME_SET_BEHAVIOR_ATOM: u16 = 0xf13a;
pub const TIME_COMMAND_BEHAVIOR_ATOM: u16 = 0xf13b;
pub const TIME_SEQUENCE_DATA_ATOM: u16 = 0xf141;
pub const TIME_ANIMATION_VALUE_ATOM: u16 = 0xf143;
pub const TIME_VARIANT_ATOM: u16 = 0xf142;
pub const VISUAL_SHAPE_ATOM: u16 = 0x2afb;
pub const HASH_CODE_ATOM: u16 = 0x2b00;
pub const VISUAL_PAGE_ATOM: u16 = 0x2b01;
pub const BUILD_ATOM: u16 = 0x2b03;
pub const PARA_BUILD_ATOM: u16 = 0x2b09;
pub const LEVEL_INFO_ATOM: u16 = 0x2b0a;
pub const SLIDE_TIME_10_ATOM: u16 = 0x2eeb;
pub const FONT_ENTITY_ATOM: u16 = 0x0fb7;
pub const EXTERNAL_OLE_OBJECT_ATOM: u16 = 0x0fc3;
pub const EXTERNAL_OLE_EMBED_ATOM: u16 = 0x0fcd;
pub const KINSOKU_ATOM: u16 = 0x0fd2;
pub const EXTERNAL_HYPERLINK_ATOM: u16 = 0x0fd3;
pub const SLIDE_NUMBER_META_CHARACTER_ATOM: u16 = 0x0fd8;
pub const TEXT_INTERACTIVE_INFO_ATOM: u16 = 0x0fdf;
pub const ANIMATION_INFO_ATOM: u16 = 0x0ff1;
pub const INTERACTIVE_INFO_ATOM: u16 = 0x0ff3;
pub const DATE_TIME_META_CHARACTER_ATOM: u16 = 0x0ff7;
pub const GENERIC_DATE_META_CHARACTER_ATOM: u16 = 0x0ff8;
pub const HEADER_META_CHARACTER_ATOM: u16 = 0x0ff9;
pub const FOOTER_META_CHARACTER_ATOM: u16 = 0x0ffa;
pub const EXTERNAL_HYPERLINK_FLAGS_ATOM: u16 = 0x1018;
pub const RECOLOR_INFO_ATOM: u16 = 0x0fe7;
pub const VIEW_INFO_ATOM: u16 = 0x03fd;
pub const BLIP_ENTITY9_ATOM: u16 = 0x07f9;
pub const ROUND_TRIP_COLOR_MAPPING_12_ATOM: u16 = 0x040f;
pub const ROUND_TRIP_NOTES_MASTER_TEXT_STYLES_12_ATOM: u16 = 0x0427;
pub const ROUND_TRIP_ANIMATION_12_ATOM: u16 = 0x2b0b;
pub const ROUND_TRIP_ANIMATION_HASH_12_ATOM: u16 = 0x2b0d;
pub const SLIDE_SHOW_SLIDE_INFO_ATOM: u16 = 0x03f9;
pub const GUIDE_ATOM: u16 = 0x03fb;
pub const SLIDE_VIEW_INFO_ATOM: u16 = 0x03fe;
pub const VBA_INFO_ATOM: u16 = 0x0400;
pub const SLIDE_SHOW_DOC_INFO_ATOM: u16 = 0x0401;
pub const EXTERNAL_OBJECT_LIST_ATOM: u16 = 0x040a;
pub const GRID_SPACING_10_ATOM: u16 = 0x040d;
pub const NORMAL_VIEW_SET_INFO_9_ATOM: u16 = 0x0415;
pub const ROUND_TRIP_ORIGINAL_MAIN_MASTER_ID_12_ATOM: u16 = 0x041c;
pub const ROUND_TRIP_COMPOSITE_MASTER_ID_12_ATOM: u16 = 0x041d;
pub const ROUND_TRIP_SHAPE_ID_12_ATOM: u16 = 0x041f;
pub const ROUND_TRIP_HF_PLACEHOLDER_12_ATOM: u16 = 0x0420;
pub const ROUND_TRIP_CONTENT_MASTER_ID_12_ATOM: u16 = 0x0422;
pub const ROUND_TRIP_HEADER_FOOTER_DEFAULTS_12_ATOM: u16 = 0x0424;
pub const ROUND_TRIP_DOC_FLAGS_12_ATOM: u16 = 0x0425;
pub const ROUND_TRIP_SHAPE_CHECKSUM_12_ATOM: u16 = 0x0426;
pub const END_DOCUMENT_ATOM: u16 = 0x03ea;
pub const SOUND_COLLECTION_ATOM: u16 = 0x07e5;
pub const SOUND_DATA_BLOB: u16 = 0x07e7;
pub const TEXT_BOOKMARK_ATOM: u16 = 0x0fa7;
pub const OUTLINE_TEXT_PROPS_HEADER9_ATOM: u16 = 0x0faf;
pub const EXTERNAL_MEDIA_ATOM: u16 = 0x1004;
pub const EXTERNAL_WAV_AUDIO_EMBEDDED_ATOM: u16 = 0x1013;
pub const PRINT_OPTIONS_ATOM: u16 = 0x1770;
pub const PRESENTATION_ADVISOR_FLAGS9_ATOM: u16 = 0x177a;
pub const HTML_DOC_INFO9_ATOM: u16 = 0x177b;
pub const HTML_PUBLISH_INFO_ATOM: u16 = 0x177c;
pub const COMMENT10_ATOM: u16 = 0x2ee1;
pub const COMMENT_INDEX10_ATOM: u16 = 0x2ee5;
pub const SLIDE_FLAGS10_ATOM: u16 = 0x2eea;
pub const FILTER_PRIVACY_FLAGS10_ATOM: u16 = 0x36b0;
pub const DOC_TOOLBAR_STATES10_ATOM: u16 = 0x36b1;
pub const MAC_PRINT_SETTINGS_ATOM: u16 = 0x178c;
pub const MAC_PAGE_FORMAT_ATOM: u16 = 0x178d;
pub const PPT11_FONT_DESCRIPTOR_ATOM: u16 = 0x1019;
pub const PPT11_FONT_DESCRIPTOR_COLLECTION_ATOM: u16 = 0x101a;
pub const PPT10_RESERVED_ATOM: u16 = 0x101d;
pub const MAC_LEGACY_PRINT_INFO_ATOM: u16 = 0x1773;
pub const MAC_PRINT_DRIVER_INFO_ATOM: u16 = 0x1789;
pub const HANDOUT_COMPATIBILITY_ATOM: u16 = 0x200a;
pub const NAMED_SHOW_SLIDES_ATOM: u16 = 0x0412;
pub const BOOKMARK_SEED_ATOM: u16 = 0x07e9;
pub const SHAPE_ATOM: u16 = 0x0bdb;
pub const SHAPE_FLAGS10_ATOM: u16 = 0x0bdc;
pub const ROUND_TRIP_NEW_PLACEHOLDER_ID_12_ATOM: u16 = 0x0bdd;
pub const FONT_EMBED_DATA_BLOB: u16 = 0x0fb8;
pub const BOOKMARK_ENTITY_ATOM: u16 = 0x0fd0;
pub const RTF_DATE_TIME_META_CHARACTER_ATOM: u16 = 0x1015;
pub const CHART_BUILD_ATOM: u16 = 0x2b05;
pub const DIAGRAM_BUILD_ATOM: u16 = 0x2b07;
pub const LINKED_SHAPE10_ATOM: u16 = 0x2ee6;
pub const LINKED_SLIDE10_ATOM: u16 = 0x2ee7;
pub const DIFF10_ATOM: u16 = 0x2eee;
pub const SLIDE_LIST_TABLE_SIZE10_ATOM: u16 = 0x2eef;
pub const SLIDE_LIST_ENTRY10_ATOM: u16 = 0x2ef0;
pub const FONT_EMBED_FLAGS10_ATOM: u16 = 0x32c8;
pub const PHOTO_ALBUM_INFO10_ATOM: u16 = 0x36b2;
pub const TIME_ITERATE_DATA_ATOM: u16 = 0xf140;
pub const TEXT_DEFAULTS9_ATOM: u16 = 0x0fb0;
pub const EXTERNAL_OLE_LINK_ATOM: u16 = 0x0fd1;
pub const EXTERNAL_OLE_CONTROL_ATOM: u16 = 0x0ffb;
pub const EXTERNAL_CD_AUDIO_ATOM: u16 = 0x1012;
pub const BROADCAST_DOC_INFO9_ATOM: u16 = 0x177f;
pub const ENVELOPE_FLAGS9_ATOM: u16 = 0x1784;
pub const ENVELOPE_DATA9_ATOM: u16 = 0x1785;
pub const DOC_ROUTING_SLIP_ATOM: u16 = 0x0406;
pub const METAFILE_BLOB: u16 = 0x0fc1;
pub const ROUND_TRIP_SLIDE_SYNC_INFO12_ATOM: u16 = 0x3715;
pub const TIME_COLOR_BEHAVIOR_ATOM: u16 = 0xf135;
pub const TIME_ROTATION_BEHAVIOR_ATOM: u16 = 0xf138;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PptRecordHeader {
  pub version: u8,
  pub instance: u16,
  pub record_type: u16,
  pub declared_length: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkBitfield)]
#[sdk(repr = "u16")]
struct PptRecordVersionInstance {
  #[sdk(bits = 0..=3)]
  version: u8,
  #[sdk(bits = 4..=15)]
  instance: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
struct PptRecordHeaderWire {
  version_instance: PptRecordVersionInstance,
  record_type: u16,
  declared_length: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PowerPointDocument {
  pub records: PptRecordSequence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentUserStream {
  pub header: PptRecordHeader,
  pub data: CurrentUserData,
  /// Zero padding after the single CurrentUserAtom record.
  pub padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PicturesStream {
  Complete(OfficeArtBStoreDelay),
  Compatibility {
    stream: OfficeArtStream,
    reason: String,
  },
  Partial(OfficeArtPartialStream),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CurrentUserData {
  Parsed(CurrentUserAtom),
  Compatibility(Vec<u8>),
  Truncated(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentUserAtom {
  pub fixed_size: u32,
  pub header_token: u32,
  pub offset_to_current_edit: u32,
  pub declared_user_name_byte_length: u16,
  pub document_file_version: u16,
  pub major_version: u8,
  pub minor_version: u8,
  pub unused: u16,
  pub ansi_user_name: Vec<u8>,
  pub release_version: u32,
  pub unicode_user_name: Option<CurrentUserUnicodeName>,
  pub trailing: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentUserUnicodeName {
  pub code_units: Vec<u16>,
  pub is_complete: bool,
  /// Whether the Unicode name is counted by `RecordHeader.recLen`.
  pub inside_record: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PptRecordSequence {
  pub records: Vec<PptRecord>,
  /// Bytes that cannot form another complete 8-byte record header.
  pub trailing_header_bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PptRecord {
  /// Byte offset from the start of the PowerPoint Document stream.
  pub offset: u64,
  pub header: PptRecordHeader,
  pub data: PptRecordData,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PptRecordData {
  Container(PptRecordSequence),
  ProgBinaryTag(ProgBinaryTag),
  ProgTags(PptRecordSequence),
  Document(DocumentAtom),
  Slide(SlideAtom),
  Notes(NotesAtom),
  OutlineTextRef(OutlineTextRefAtom),
  TextHeader(TextHeaderAtom),
  TextChars(String),
  TextBytes(String),
  /// Exact UTF-16 units from a nonconforming TextCharsAtom. Strict file
  /// roots reject this variant; compatible roots preserve it explicitly.
  CompatibilityTextChars(Vec<u16>),
  StyleTextProp(StyleTextPropAtom),
  MalformedStyleTextProp(MalformedStyleTextPropAtom),
  UnresolvedStyleTextProp(Vec<u8>),
  CString(String),
  /// Exact UTF-16 units from a nonconforming CString.
  CompatibilityCString(Vec<u16>),
  SlidePersist(SlidePersistAtom),
  ColorScheme(ColorSchemeAtom),
  ExternalObjectRef(ExternalObjectRefAtom),
  Placeholder(PlaceholderAtom),
  HeadersFooters(HeadersFootersAtom),
  MasterTextProp(MasterTextPropAtom),
  TextMasterStyle(TextMasterStyleAtom),
  MalformedTextMasterStyle(Vec<u8>),
  TextCfException(TextCharacterException),
  TextPfException(TextPfExceptionAtom),
  TextSiException(TextSpecialInfoException),
  TextRuler(TextRulerAtom),
  MalformedTextRuler(Vec<u8>),
  TextSpecialInfo(TextSpecialInfoAtom),
  MalformedTextSpecialInfo(Vec<u8>),
  StyleTextProp9(StyleTextProp9Atom),
  MalformedStyleTextProp9(Vec<u8>),
  TextMasterStyle9(TextMasterStyle9Atom),
  StyleTextProp10(StyleTextProp10Atom),
  TextMasterStyle10(TextMasterStyle10Atom),
  TextDefaults10(TextCharacterException10),
  StyleTextProp11(StyleTextProp11Atom),
  RecolorInfo(RecolorInfoAtom),
  MacPrintSettings(MacPlistAtom),
  MacPageFormat(MacPlistAtom),
  Ppt11FontDescriptors(Ppt11FontDescriptorAtom),
  Ppt11FontDescriptorCollection(Ppt11FontDescriptorCollectionAtom),
  Ppt10Reserved(HashCodeAtom),
  MacLegacyPrintInfo(MacLegacyPrintInfoAtom),
  MacPrintDriverInfo(MacPrintDriverInfoAtom),
  HandoutCompatibility(HandoutCompatibilityAtom),
  NamedShowSlides(Vec<u32>),
  BookmarkSeed(UnsignedIdAtom),
  ShapeFlags(ByteAtom),
  ShapeFlags10(ByteAtom),
  RoundTripNewPlaceholderId12(ByteAtom),
  FontEmbedDataBlob(Vec<u8>),
  BookmarkEntity(BookmarkEntityAtom),
  RtfDateTimeMeta(RtfDateTimeMetaCharacterAtom),
  ChartBuild(ChartBuildAtom),
  DiagramBuild(DiagramBuildAtom),
  LinkedShape10(LinkedShape10Atom),
  LinkedSlide10(LinkedSlide10Atom),
  Diff10(Diff10Atom),
  SlideListTableSize10(SignedCountAtom),
  SlideListEntry10(SlideListEntry10Atom),
  FontEmbedFlags10(HashCodeAtom),
  PhotoAlbumInfo10(PhotoAlbumInfo10Atom),
  TimeIterateData(TimeIterateDataAtom),
  TextDefaults9(TextDefaults9Atom),
  ExternalOleLink(ExternalOleLinkAtom),
  ExternalOleControl(UnsignedIdAtom),
  ExternalCdAudio(ExternalCdAudioAtom),
  BroadcastDocInfo9(BroadcastDocInfo9Atom),
  EnvelopeFlags9(HashCodeAtom),
  /// MsoEnvelopeCLSID is defined by MS-OSHARED, outside the MS-PPT schema.
  EnvelopeData9(Vec<u8>),
  DocRoutingSlip(DocRoutingSlipAtom),
  Metafile(MetafileBlob),
  RoundTripSlideSyncInfo12(SlideSyncInfoAtom12),
  TimeColorBehavior(TimeColorBehaviorAtom),
  TimeRotationBehavior(TimeRotationBehaviorAtom),
  TimeNode(TimeNodeAtom),
  TimeCondition(TimeConditionAtom),
  TimeModifier(TimeModifierAtom),
  TimeBehavior(TimeBehaviorAtom),
  TimeAnimateBehavior(TimeAnimateBehaviorAtom),
  TimeEffectBehavior(TimeEffectBehaviorAtom),
  TimeMotionBehavior(TimeMotionBehaviorAtom),
  TimeScaleBehavior(TimeScaleBehaviorAtom),
  TimeSetBehavior(TimeSetBehaviorAtom),
  TimeCommandBehavior(TimeCommandBehaviorAtom),
  TimeSequenceData(TimeSequenceDataAtom),
  TimeAnimationValue(TimeAnimationValueAtom),
  TimeVariant(TimeVariantAtom),
  MalformedTimeVariant(Vec<u8>),
  VisualShape(VisualShapeAtom),
  HashCode(HashCodeAtom),
  VisualPage(VisualPageAtom),
  Build(BuildAtom),
  ParaBuild(ParaBuildAtom),
  LevelInfo(LevelInfoAtom),
  SlideTime10(SlideTime10Atom),
  FontEntity(FontEntityAtom),
  ExternalOleObject(ExternalOleObjectAtom),
  ExternalOleEmbed(ExternalOleEmbedAtom),
  Kinsoku(KinsokuAtom),
  ExternalHyperlinkId(ExternalHyperlinkIdAtom),
  ExternalHyperlinkFlags(ExternalHyperlinkFlagsAtom),
  SlideNumberMeta(TextPositionAtom),
  TextInteractiveInfo(TextRange),
  AnimationInfo(AnimationInfoAtom),
  InteractiveInfo(InteractiveInfoAtom),
  DateTimeMeta(DateTimeMetaCharacterAtom),
  GenericDateMeta(TextPositionAtom),
  HeaderMeta(TextPositionAtom),
  FooterMeta(TextPositionAtom),
  ViewInfo(ViewInfoAtom),
  BlipEntity9(Box<BlipEntity9Atom>),
  MalformedBlipEntity9 {
    body: Vec<u8>,
    reason: String,
  },
  RoundTripColorMapping12(RoundTripColorMapping12Atom),
  RoundTripAnimation12(Box<RoundTripAnimation12Atom>),
  RoundTripAnimationHash12(HashCodeAtom),
  SlideShowSlideInfo(SlideShowSlideInfoAtom),
  Guide(GuideAtom),
  SlideViewInfo(SlideViewInfoAtom),
  VbaInfo(VbaInfoAtom),
  SlideShowDocInfo(SlideShowDocInfoAtom),
  ExternalObjectList(ExternalObjectListAtom),
  GridSpacing10(PptPoint),
  NormalViewSetInfo9(NormalViewSetInfoAtom),
  RoundTripOriginalMainMasterId12(UnsignedIdAtom),
  RoundTripCompositeMasterId12(UnsignedIdAtom),
  RoundTripShapeId12(UnsignedIdAtom),
  RoundTripHfPlaceholder12(ByteAtom),
  RoundTripContentMasterId12(RoundTripContentMasterId12Atom),
  RoundTripHeaderFooterDefaults12(ByteAtom),
  RoundTripDocFlags12(ByteAtom),
  RoundTripShapeChecksum12(RoundTripShapeChecksum12Atom),
  EndDocument,
  SoundCollection(SoundCollectionAtom),
  SoundDataBlob(Vec<u8>),
  TextBookmark(TextBookmarkAtom),
  OutlineTextPropsHeader9(OutlineTextPropsHeader9Atom),
  ExternalMedia(ExternalMediaAtom),
  ExternalWavAudioEmbedded(ExternalWavAudioEmbeddedAtom),
  PrintOptions(PrintOptionsAtom),
  PresentationAdvisorFlags9(HashCodeAtom),
  HtmlDocInfo9(HtmlDocInfo9Atom),
  HtmlPublishInfo(HtmlPublishInfoAtom),
  Comment10(Comment10Atom),
  CommentIndex10(CommentIndex10Atom),
  SlideFlags10(HashCodeAtom),
  FilterPrivacyFlags10(HashCodeAtom),
  DocToolbarStates10(ByteAtom),
  ExternalStorage(ExternalStorageAtom),
  RoundTripContentMasterInfo12(Box<RoundTripContentMasterInfo12Atom>),
  RoundTripTheme12(Box<RoundTripTheme12Atom>),
  RoundTripStyle12(Box<RoundTripStyle12Atom>),
  BinaryTagData(BinaryTagData),
  OfficeArt(Box<OfficeArtRecord>),
  UserEdit(UserEditAtom),
  PersistDirectory(PersistDirectoryAtom),
  /// A record whose `recType` is not specified by the RecordType
  /// enumeration. MS-PPT requires readers to ignore these records and
  /// permits preserving them.
  Unknown(UnknownPptRecord),
  /// A record whose type is defined by MS-PPT but whose body violates its schema.
  MalformedSpecRecord(UnknownPptRecord),
  /// All bytes physically available for a record whose declared body crosses its boundary.
  Truncated(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProgBinaryTag {
  pub records: PptRecordSequence,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BinaryTagData {
  Records(PptRecordSequence),
  /// Application- or platform-private tag data whose schema is not MS-PPT records.
  Opaque(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProgrammableTagKind {
  Ppt9,
  Ppt10,
  Ppt11,
  Ppt12,
  PptMac11,
  Ppt2001,
  Other,
}

impl ProgBinaryTag {
  pub fn tag(&self) -> Option<&str> {
    self.records.records.iter().find_map(|record| {
      if let PptRecordData::CString(value) = &record.data {
        Some(value.as_str())
      } else {
        None
      }
    })
  }

  pub fn tag_kind(&self) -> Option<ProgrammableTagKind> {
    Some(match self.tag()? {
      "___PPT9" => ProgrammableTagKind::Ppt9,
      "___PPT10" => ProgrammableTagKind::Ppt10,
      "___PPT11" => ProgrammableTagKind::Ppt11,
      "___PPT12" => ProgrammableTagKind::Ppt12,
      "___PPTMac11" => ProgrammableTagKind::PptMac11,
      "___PPT2001" => ProgrammableTagKind::Ppt2001,
      _ => ProgrammableTagKind::Other,
    })
  }

  pub fn binary_tag_data(&self) -> Option<&BinaryTagData> {
    self.records.records.iter().find_map(|record| {
      if let PptRecordData::BinaryTagData(data) = &record.data {
        Some(data)
      } else {
        None
      }
    })
  }

  fn preserve_private_tag_data(&mut self) -> Result<()> {
    if matches!(
      self.tag_kind(),
      Some(
        ProgrammableTagKind::Ppt9
          | ProgrammableTagKind::Ppt10
          | ProgrammableTagKind::Ppt11
          | ProgrammableTagKind::Ppt12
      )
    ) {
      return Ok(());
    }
    for record in &mut self.records.records {
      if let PptRecordData::BinaryTagData(BinaryTagData::Records(records)) = &record.data {
        let bytes = records.to_bytes()?;
        record.data = PptRecordData::BinaryTagData(BinaryTagData::Opaque(bytes));
      }
    }
    Ok(())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate_at = "validate_user_edit_atom")]
pub struct UserEditAtom {
  pub last_slide_id_ref: u32,
  pub version: u16,
  pub minor_version: u8,
  pub major_version: u8,
  pub offset_last_edit: u32,
  pub offset_persist_directory: u32,
  pub doc_persist_id_ref: u32,
  pub persist_id_seed: u32,
  pub last_view: u16,
  pub unused: u16,
  #[sdk(optional_remaining)]
  pub encrypt_session_persist_id_ref: Option<u32>,
}

fn validate_user_edit_atom(value: &UserEditAtom, offset: u64) -> Result<()> {
  if value.minor_version != 0 {
    return Err(Error::invalid(
      offset + 6,
      "UserEditAtom.minorVersion must be 0x00",
    ));
  }
  if value.major_version != 3 {
    return Err(Error::invalid(
      offset + 7,
      "UserEditAtom.majorVersion must be 0x03",
    ));
  }
  if value.doc_persist_id_ref != 1 {
    return Err(Error::invalid(
      offset + 16,
      "UserEditAtom.docPersistIdRef must be 0x00000001",
    ));
  }
  if !(1..=0x12).contains(&value.last_view) {
    return Err(Error::invalid(
      offset + 24,
      "UserEditAtom.lastView is not a ViewTypeEnum value",
    ));
  }
  Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistDirectoryAtom {
  pub entries: Vec<PersistDirectoryEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistDirectoryEntry {
  pub first_persist_id: u32,
  pub stream_offsets: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct PptPoint {
  pub x: i32,
  pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct DocumentAtom {
  pub slide_size: PptPoint,
  pub notes_size: PptPoint,
  pub server_zoom: PptPoint,
  pub notes_master_persist_id_ref: u32,
  pub handout_master_persist_id_ref: u32,
  pub first_slide_number: u16,
  pub slide_size_type: u16,
  pub save_with_fonts: u8,
  pub omit_title_placeholders: u8,
  pub right_to_left: u8,
  pub show_comments: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct SlideAtom {
  pub geometry: u32,
  pub placeholder_types: [u8; 8],
  pub master_id_ref: u32,
  pub notes_id_ref: u32,
  pub slide_flags: u16,
  pub unused: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct NotesAtom {
  pub slide_id_ref: u32,
  pub slide_flags: u16,
  pub unused: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct OutlineTextRefAtom {
  pub index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PptTextType {
  Title,
  Body,
  Notes,
  Other,
  CenterBody,
  CenterTitle,
  HalfBody,
  QuarterBody,
  Compatibility(u32),
}

impl PptTextType {
  pub const fn from_raw(value: u32) -> Self {
    match value {
      0 => Self::Title,
      1 => Self::Body,
      2 => Self::Notes,
      4 => Self::Other,
      5 => Self::CenterBody,
      6 => Self::CenterTitle,
      7 => Self::HalfBody,
      8 => Self::QuarterBody,
      value => Self::Compatibility(value),
    }
  }

  pub const fn raw(self) -> u32 {
    match self {
      Self::Title => 0,
      Self::Body => 1,
      Self::Notes => 2,
      Self::Other => 4,
      Self::CenterBody => 5,
      Self::CenterTitle => 6,
      Self::HalfBody => 7,
      Self::QuarterBody => 8,
      Self::Compatibility(value) => value,
    }
  }
}

impl SdkRead for PptTextType {
  fn read_from<R: Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    Ok(Self::from_raw(reader.read_u32()?))
  }
}

impl SdkWrite for PptTextType {
  fn write_to<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    writer.write_u32(self.raw())
  }
}

impl SdkSize for PptTextType {
  fn sdk_size(&self) -> u64 {
    4
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TextHeaderAtom {
  pub text_type: PptTextType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct SlidePersistAtom {
  pub persist_id_ref: u32,
  pub flags: u32,
  pub text_count: i32,
  pub slide_id: u32,
  pub reserved: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ColorSchemeAtom {
  pub colors: [u32; 8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ExternalObjectRefAtom {
  pub external_object_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PptPlaceholderType {
  None,
  MasterTitle,
  MasterBody,
  MasterCenterTitle,
  MasterSubTitle,
  MasterNotesSlideImage,
  MasterNotesBody,
  MasterDate,
  MasterSlideNumber,
  MasterFooter,
  MasterHeader,
  NotesSlideImage,
  NotesBody,
  Title,
  Body,
  CenterTitle,
  SubTitle,
  VerticalTitle,
  VerticalBody,
  Object,
  Graph,
  Table,
  ClipArt,
  OrganizationChart,
  Media,
  VerticalObject,
  Picture,
  Compatibility(u8),
}

impl PptPlaceholderType {
  pub const fn from_raw(value: u8) -> Self {
    match value {
      0x00 => Self::None,
      0x01 => Self::MasterTitle,
      0x02 => Self::MasterBody,
      0x03 => Self::MasterCenterTitle,
      0x04 => Self::MasterSubTitle,
      0x05 => Self::MasterNotesSlideImage,
      0x06 => Self::MasterNotesBody,
      0x07 => Self::MasterDate,
      0x08 => Self::MasterSlideNumber,
      0x09 => Self::MasterFooter,
      0x0a => Self::MasterHeader,
      0x0b => Self::NotesSlideImage,
      0x0c => Self::NotesBody,
      0x0d => Self::Title,
      0x0e => Self::Body,
      0x0f => Self::CenterTitle,
      0x10 => Self::SubTitle,
      0x11 => Self::VerticalTitle,
      0x12 => Self::VerticalBody,
      0x13 => Self::Object,
      0x14 => Self::Graph,
      0x15 => Self::Table,
      0x16 => Self::ClipArt,
      0x17 => Self::OrganizationChart,
      0x18 => Self::Media,
      0x19 => Self::VerticalObject,
      0x1a => Self::Picture,
      value => Self::Compatibility(value),
    }
  }

  pub const fn raw(self) -> u8 {
    match self {
      Self::None => 0x00,
      Self::MasterTitle => 0x01,
      Self::MasterBody => 0x02,
      Self::MasterCenterTitle => 0x03,
      Self::MasterSubTitle => 0x04,
      Self::MasterNotesSlideImage => 0x05,
      Self::MasterNotesBody => 0x06,
      Self::MasterDate => 0x07,
      Self::MasterSlideNumber => 0x08,
      Self::MasterFooter => 0x09,
      Self::MasterHeader => 0x0a,
      Self::NotesSlideImage => 0x0b,
      Self::NotesBody => 0x0c,
      Self::Title => 0x0d,
      Self::Body => 0x0e,
      Self::CenterTitle => 0x0f,
      Self::SubTitle => 0x10,
      Self::VerticalTitle => 0x11,
      Self::VerticalBody => 0x12,
      Self::Object => 0x13,
      Self::Graph => 0x14,
      Self::Table => 0x15,
      Self::ClipArt => 0x16,
      Self::OrganizationChart => 0x17,
      Self::Media => 0x18,
      Self::VerticalObject => 0x19,
      Self::Picture => 0x1a,
      Self::Compatibility(value) => value,
    }
  }
}

impl SdkRead for PptPlaceholderType {
  fn read_from<R: Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    Ok(Self::from_raw(reader.read_u8()?))
  }
}

impl SdkWrite for PptPlaceholderType {
  fn write_to<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    writer.write_u8(self.raw())
  }
}

impl SdkSize for PptPlaceholderType {
  fn sdk_size(&self) -> u64 {
    1
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PptPlaceholderSize {
  Full,
  Half,
  Quarter,
  Compatibility(u8),
}

impl PptPlaceholderSize {
  pub const fn from_raw(value: u8) -> Self {
    match value {
      0 => Self::Full,
      1 => Self::Half,
      2 => Self::Quarter,
      value => Self::Compatibility(value),
    }
  }

  pub const fn raw(self) -> u8 {
    match self {
      Self::Full => 0,
      Self::Half => 1,
      Self::Quarter => 2,
      Self::Compatibility(value) => value,
    }
  }
}

impl SdkRead for PptPlaceholderSize {
  fn read_from<R: Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    Ok(Self::from_raw(reader.read_u8()?))
  }
}

impl SdkWrite for PptPlaceholderSize {
  fn write_to<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    writer.write_u8(self.raw())
  }
}

impl SdkSize for PptPlaceholderSize {
  fn sdk_size(&self) -> u64 {
    1
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct PlaceholderAtom {
  pub position: i32,
  pub placement_id: PptPlaceholderType,
  pub size: PptPlaceholderSize,
  pub unused: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct HeadersFootersAtom {
  pub format_id: i16,
  pub flags: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct BookmarkEntityAtom {
  pub bookmark_id: u32,
  pub bookmark_name: [u16; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct RtfDateTimeMetaCharacterAtom {
  pub position: u32,
  pub format: [u16; 64],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ChartBuildAtom {
  pub chart_build: u32,
  pub animate_background: u8,
  pub unused: [u8; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct DiagramBuildAtom {
  pub diagram_build: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct LinkedShape10Atom {
  pub shape_id_ref: u32,
  pub linked_shape_id_ref: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct LinkedSlide10Atom {
  pub linked_slide_id_ref: u32,
  pub linked_shape_count: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct Diff10Atom {
  pub index: u8,
  pub unused1: u8,
  pub unused2: u8,
  pub unused3: u8,
  pub diff_type: u32,
  pub unused4: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct SignedCountAtom {
  pub count: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct SlideListEntry10Atom {
  pub slide_id_ref: u32,
  pub high_date_time: u32,
  pub low_date_time: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct PhotoAlbumInfo10Atom {
  pub use_black_white: u8,
  pub has_caption: u8,
  pub layout: u8,
  pub unused: u8,
  pub frame_shape: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TimeIterateDataAtom {
  pub iterate_interval: u32,
  pub iterate_type: u32,
  pub iterate_direction: u32,
  pub iterate_interval_type: u32,
  pub property_flags: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ExternalOleLinkAtom {
  pub slide_id_ref: u32,
  pub update_mode: u32,
  pub unused: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TmsfTime {
  pub track: u8,
  pub minute: u8,
  pub second: u8,
  pub frame: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ExternalCdAudioAtom {
  pub start: TmsfTime,
  pub end: TmsfTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct BroadcastDocInfo9Atom {
  pub flags: u16,
  pub start_time: SystemTime,
  pub end_time: SystemTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TextDefaults9Atom {
  pub character: TextCharacterException9,
  pub paragraph: TextParagraphException9,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocRoutingSlipAtom {
  pub unused1: u32,
  pub current_recipient: u32,
  pub flags: u32,
  pub unused2: u32,
  pub originator: DocRoutingSlipString,
  pub recipients: Vec<DocRoutingSlipString>,
  pub subject: DocRoutingSlipString,
  pub message: DocRoutingSlipString,
  pub unused3: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocRoutingSlipString {
  pub string_type: u16,
  /// Physical bytes, including the final required NUL/ignored byte.
  pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetafileBlob {
  pub mapping_mode: i16,
  pub x_extent: i16,
  pub y_extent: i16,
  /// WMF data is defined by MS-WMF and intentionally remains an external payload.
  pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct SlideSyncInfoAtom12 {
  pub modified: SystemTime,
  pub inserted: SystemTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeColorBehaviorAtom {
  pub property_flags: u32,
  pub color_by: TimeAnimateColorBy,
  pub color_from: TimeAnimateColor,
  pub color_to: TimeAnimateColor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeAnimateColorBy {
  Rgb {
    red: i32,
    green: i32,
    blue: i32,
  },
  Hsl {
    hue: i32,
    saturation: i32,
    luminance: i32,
  },
  Scheme(IndexSchemeColor),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeAnimateColor {
  Rgb { red: u32, green: u32, blue: u32 },
  Scheme(IndexSchemeColor),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct IndexSchemeColor {
  pub index: u32,
  pub reserved1: u32,
  pub reserved2: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, SdkObject)]
pub struct TimeRotationBehaviorAtom {
  pub property_flags: u32,
  pub by: f32,
  pub from: f32,
  pub to: f32,
  pub direction: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TimeNodeAtom {
  pub reserved1: u32,
  pub restart: u32,
  pub node_type: u32,
  pub fill: u32,
  pub reserved2: u32,
  pub reserved3: u8,
  pub unused: [u8; 3],
  pub duration: i32,
  pub property_flags: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TimeConditionAtom {
  pub trigger_object: u32,
  pub trigger_event: u32,
  pub target_id: u32,
  pub delay: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TimeModifierAtom {
  pub modifier_type: u32,
  pub value: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TimeBehaviorAtom {
  pub property_flags: u32,
  pub additive: u32,
  pub accumulate: u32,
  pub transform: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TimeAnimateBehaviorAtom {
  pub calculation_mode: u32,
  pub property_flags: u32,
  pub value_type: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TimeEffectBehaviorAtom {
  pub property_flags: u32,
  pub transition: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, SdkObject)]
pub struct TimeMotionBehaviorAtom {
  pub property_flags: u32,
  pub x_by: f32,
  pub y_by: f32,
  pub x_from: f32,
  pub y_from: f32,
  pub x_to: f32,
  pub y_to: f32,
  pub origin: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, SdkObject)]
pub struct TimeScaleBehaviorAtom {
  pub property_flags: u32,
  pub x_by: f32,
  pub y_by: f32,
  pub x_from: f32,
  pub y_from: f32,
  pub x_to: f32,
  pub y_to: f32,
  pub zoom_contents: u8,
  pub unused: [u8; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TimeSetBehaviorAtom {
  pub property_flags: u32,
  pub value_type: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TimeCommandBehaviorAtom {
  pub property_flags: u32,
  pub command_type: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TimeSequenceDataAtom {
  pub concurrency: u32,
  pub next_action: u32,
  pub previous_action: u32,
  pub reserved: u32,
  pub property_flags: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TimeAnimationValueAtom {
  pub time: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TimeVariantAtom {
  Bool(u8),
  Int(i32),
  Float(f32),
  String(Vec<u16>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct VisualShapeAtom {
  pub visual_element_type: u32,
  pub reference_type: u32,
  pub reference_id: u32,
  pub data1: i32,
  pub data2: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct HashCodeAtom {
  pub hash: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct VisualPageAtom {
  pub visual_element_type: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct BuildAtom {
  pub build_type: u32,
  pub build_id: u32,
  pub shape_id_ref: u32,
  pub expanded: u8,
  pub ui_expanded: u8,
  pub unused: [u8; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ParaBuildAtom {
  pub paragraph_build: u32,
  pub build_level: u32,
  pub animate_background: u8,
  pub reverse: u8,
  pub user_set_animate_background: u8,
  pub automatic: u8,
  pub delay_time: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct LevelInfoAtom {
  pub level: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct SlideTime10Atom {
  pub file_time: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct FontEntityAtom {
  pub face_name: [u16; 32],
  pub character_set: u8,
  pub embedding_flags: u8,
  pub font_type_flags: u8,
  pub pitch_and_family: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ExternalOleObjectAtom {
  pub draw_aspect: u32,
  pub object_type: u32,
  pub external_object_id: u32,
  pub object_subtype: u32,
  pub persist_id_ref: u32,
  pub unused: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ExternalOleEmbedAtom {
  pub color_follow: u32,
  pub cannot_lock_server: u8,
  pub no_size_to_server: u8,
  pub is_table: u8,
  pub unused: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct KinsokuAtom {
  /// KinsokuAtom uses a signed level; Kinsoku9Atom uses four packed 2-bit levels.
  pub level_bits: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ExternalHyperlinkIdAtom {
  pub id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ExternalHyperlinkFlagsAtom {
  pub flags: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TextPositionAtom {
  pub position: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TextRange {
  pub begin: u32,
  pub end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct AnimationInfoAtom {
  pub dim_color: u32,
  pub flags: u32,
  pub sound_id_ref: u32,
  pub delay_time: i32,
  pub order_id: i16,
  pub slide_count: u16,
  pub build_type: u8,
  pub effect: u8,
  pub effect_direction: u8,
  pub after_effect: u8,
  pub text_build_sub_effect: u8,
  pub ole_verb: u8,
  pub unused: [u8; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct InteractiveInfoAtom {
  pub sound_id_ref: u32,
  pub external_hyperlink_id_ref: u32,
  pub action: u8,
  pub ole_verb: u8,
  pub jump: u8,
  pub flags: u8,
  pub hyperlink_type: u8,
  pub unused: [u8; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct DateTimeMetaCharacterAtom {
  pub position: u32,
  pub format_index: u8,
  pub unused: [u8; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct SlideShowSlideInfoAtom {
  pub slide_time: i32,
  pub sound_id_ref: u32,
  pub effect_direction: u8,
  pub effect_type: u8,
  pub flags: u16,
  pub speed: u8,
  pub unused: [u8; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct GuideAtom {
  pub guide_type: u32,
  pub position: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct SlideViewInfoAtom {
  pub unused: u8,
  pub snap_to_grid: u8,
  pub snap_to_shape: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct VbaInfoAtom {
  pub persist_id_ref: u32,
  pub has_macros: u32,
  pub version: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct SlideShowDocInfoAtom {
  pub pen_color: u32,
  pub restart_time: i32,
  pub start_slide: i16,
  pub end_slide: i16,
  pub named_show: [u16; 32],
  pub flags: u16,
  pub unused: [u8; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ExternalObjectListAtom {
  pub id_seed: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct NormalViewSetInfoAtom {
  pub left_portion: PptRatio,
  pub top_portion: PptRatio,
  pub vertical_bar_state: u8,
  pub horizontal_bar_state: u8,
  pub prefer_single_set: u8,
  pub flags: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct UnsignedIdAtom {
  pub id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ByteAtom {
  pub value: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct RoundTripContentMasterId12Atom {
  pub main_master_id: u32,
  pub content_master_instance_id: u16,
  pub unused: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct RoundTripShapeChecksum12Atom {
  pub shape_checksum: u32,
  pub text_checksum: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct SoundCollectionAtom {
  pub sound_id_seed: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TextBookmarkAtom {
  pub begin: u32,
  pub end: u32,
  pub bookmark_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct OutlineTextPropsHeader9Atom {
  pub slide_id_ref: u32,
  pub text_type: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ExternalMediaAtom {
  pub external_object_id: u32,
  pub flags: u16,
  pub unused: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ExternalWavAudioEmbeddedAtom {
  pub sound_id_ref: u32,
  pub sound_length: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct PrintOptionsAtom {
  pub print_what: u8,
  pub color_mode: u8,
  pub print_hidden: u8,
  pub scale_to_fit_paper: u8,
  pub frame_slides: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct HtmlDocInfo9Atom {
  pub unused1: u32,
  pub encoding: u32,
  pub frame_color_type: u16,
  pub screen_size: u8,
  pub unused2: u8,
  pub output_type: u8,
  pub flags: u8,
  pub unused3: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct HtmlPublishInfoAtom {
  pub start_slide: i32,
  pub end_slide: i32,
  pub output_type: u8,
  pub flags: u8,
  pub unused: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct SystemTime {
  pub year: u16,
  pub month: u16,
  pub day_of_week: u16,
  pub day: u16,
  pub hour: u16,
  pub minute: u16,
  pub second: u16,
  pub milliseconds: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct Comment10Atom {
  pub index: i32,
  pub datetime: SystemTime,
  pub anchor: PptPoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct CommentIndex10Atom {
  pub color_index: i32,
  pub comment_index_seed: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct PptRatio {
  pub numerator: i32,
  pub denominator: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct PptScaling {
  pub x: PptRatio,
  pub y: PptRatio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct ViewInfoAtom {
  pub current_scale: PptScaling,
  pub unused1: [u8; 24],
  pub origin: PptPoint,
  /// fUseVarScale for ZoomViewInfoAtom; unused for NoZoomViewInfoAtom.
  pub variable_scale_or_unused: u8,
  pub draft_mode: u8,
  pub unused2: [u8; 2],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlipEntity9Atom {
  pub windows_blip_type: u8,
  pub unused: u8,
  pub blip: OfficeArtRecord,
}

impl TimeVariantAtom {
  fn parse(bytes: &[u8]) -> Option<Self> {
    match bytes {
      [0, value] => Some(Self::Bool(*value)),
      [1, value @ ..] if value.len() == 4 => Some(Self::Int(i32::from_le_bytes(
        value.try_into().expect("four-byte TimeVariantInt"),
      ))),
      [2, value @ ..] if value.len() == 4 => Some(Self::Float(f32::from_le_bytes(
        value.try_into().expect("four-byte TimeVariantFloat"),
      ))),
      [3, value @ ..] if value.len().is_multiple_of(2) => Some(Self::String(read_utf16(value))),
      _ => None,
    }
  }

  fn to_bytes(&self) -> Vec<u8> {
    let mut bytes = Vec::new();
    match self {
      Self::Bool(value) => bytes.extend_from_slice(&[0, *value]),
      Self::Int(value) => {
        bytes.push(1);
        bytes.extend_from_slice(&value.to_le_bytes());
      }
      Self::Float(value) => {
        bytes.push(2);
        bytes.extend_from_slice(&value.to_le_bytes());
      }
      Self::String(value) => {
        bytes.push(3);
        bytes.extend_from_slice(&write_utf16(value));
      }
    }
    bytes
  }
}

impl BlipEntity9Atom {
  fn parse(bytes: &[u8], limits: Limits) -> Result<Self> {
    if bytes.len() < 10 {
      return Err(Error::invalid(
        0,
        "BlipEntity9Atom is shorter than its prefix and BLIP",
      ));
    }
    let stream = OfficeArtStream::from_bytes_with_limits(&bytes[2..], limits)?;
    let [blip] = stream.records.as_slice() else {
      return Err(Error::invalid(
        2,
        "BlipEntity9Atom does not contain exactly one OfficeArt file block",
      ));
    };
    if blip.header.record_type != 0xf007 && !(0xf018..=0xf117).contains(&blip.header.record_type) {
      return Err(Error::invalid(
        2,
        "BlipEntity9Atom contains a non-BStore OfficeArt record",
      ));
    }
    if matches!(blip.data, OfficeArtRecordData::Atom(_)) {
      return Err(Error::invalid(
        2,
        "BlipEntity9Atom contains an unsupported generic OfficeArt BLIP",
      ));
    }
    Ok(Self {
      windows_blip_type: bytes[0],
      unused: bytes[1],
      blip: blip.clone(),
    })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = vec![self.windows_blip_type, self.unused];
    bytes.extend_from_slice(
      &OfficeArtStream {
        records: vec![self.blip.clone()],
      }
      .to_bytes()?,
    );
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MasterTextPropAtom {
  pub runs: Vec<MasterTextPropRun>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextMasterStyleAtom {
  pub text_type: u16,
  pub levels: Vec<TextMasterStyleLevel>,
  pub tail: TextMasterStyleTail,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextMasterStyleTail {
  None,
  /// A following record header was physically swallowed by a corrupt recLen.
  TruncatedRecord {
    header: PptRecordHeader,
    available_body: Vec<u8>,
  },
  /// Producer-specific bytes that do not begin with a PPT record header.
  Compatibility(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextMasterStyleLevel {
  /// Present for TextTypeEnum values 5 and above.
  pub level: Option<u16>,
  pub paragraph: TextParagraphException,
  pub character: TextCharacterException,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRulerAtom {
  pub flags: u32,
  pub level_count: Option<i16>,
  pub default_tab_size: Option<u16>,
  pub tab_stops: Option<Vec<TextTabStop>>,
  pub levels: [TextRulerLevel; 5],
  pub trailing: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextRulerLevel {
  pub left_margin: Option<i16>,
  pub indent: Option<i16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleTextPropAtom {
  /// Character count supplied by the corresponding TextCharsAtom or TextBytesAtom.
  pub corresponding_text_character_count: u32,
  pub paragraph_runs: Vec<TextParagraphRun>,
  pub character_runs: Vec<TextCharacterRun>,
  /// Compatibility bytes after both run arrays, retained byte-for-byte.
  pub trailing: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MalformedStyleTextPropAtom {
  pub corresponding_text_character_count: u32,
  pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextParagraphRun {
  pub character_count: u32,
  pub indent_level: u16,
  pub properties: TextParagraphException,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextCharacterRun {
  pub character_count: u32,
  pub properties: TextCharacterException,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextParagraphException {
  pub mask: u32,
  pub bullet_flags: Option<u16>,
  pub bullet_character: Option<u16>,
  pub bullet_font_ref: Option<u16>,
  pub bullet_size: Option<i16>,
  pub bullet_color: Option<u32>,
  pub text_alignment: Option<u16>,
  pub line_spacing: Option<i16>,
  pub space_before: Option<i16>,
  pub space_after: Option<i16>,
  pub left_margin: Option<i16>,
  pub indent: Option<i16>,
  pub default_tab_size: Option<u16>,
  pub tab_stops: Option<Vec<TextTabStop>>,
  pub font_alignment: Option<u16>,
  pub wrap_flags: Option<u16>,
  pub text_direction: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextPfExceptionAtom {
  /// Reserved by MS-PPT; retained so nonzero producer data remains byte-exact.
  pub reserved: u16,
  pub paragraph: TextParagraphException,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextTabStop {
  pub position: i16,
  /// Native TextTabType value; unknown values are retained.
  pub kind: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextCharacterException {
  pub mask: u32,
  pub font_style: Option<u16>,
  pub font_ref: Option<u16>,
  pub old_east_asian_font_ref: Option<u16>,
  pub ansi_font_ref: Option<u16>,
  pub symbol_font_ref: Option<u16>,
  pub font_size: Option<i16>,
  pub color: Option<u32>,
  pub position: Option<i16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct MasterTextPropRun {
  pub character_count: u32,
  pub indent_level: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSpecialInfoAtom {
  pub runs: Vec<TextSpecialInfoRun>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSpecialInfoRun {
  pub character_count: u32,
  pub mask: u32,
  pub spelling_flags: Option<u16>,
  pub language_id: Option<u16>,
  pub alternate_language_id: Option<u16>,
  pub bidi: Option<i16>,
  pub pp10_extension: Option<u32>,
  pub smart_tag_indices: Option<Vec<u32>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleTextProp9Atom {
  pub runs: Vec<StyleTextProp9>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextMasterStyle9Atom {
  pub text_type: u16,
  pub levels: Vec<TextMasterStyle9Level>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TextMasterStyle9Level {
  pub paragraph: TextParagraphException9,
  pub character: TextCharacterException9,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleTextProp10Atom {
  pub runs: Vec<TextCharacterException10>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextMasterStyle10Atom {
  pub text_type: u16,
  pub levels: Vec<TextCharacterException10>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TextCharacterException10 {
  pub mask: u32,
  #[sdk(condition = "mask", mask = 0x0100_0000)]
  pub new_east_asian_font_ref: Option<u16>,
  #[sdk(condition = "mask", mask = 0x0200_0000)]
  pub complex_script_font_ref: Option<u16>,
  /// Undefined PP11 extension bits, retained verbatim.
  #[sdk(condition = "mask", mask = 0x0400_0000)]
  pub pp11_extension: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleTextProp11Atom {
  pub runs: Vec<TextSpecialInfoException>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecolorInfoAtom {
  pub flags: u16,
  pub color_count: u16,
  pub fill_count: u16,
  pub mono_color: WideColor,
  pub entries: Vec<RecolorEntry>,
  pub unused: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct WideColor {
  pub red: u16,
  pub green: u16,
  pub blue: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecolorEntry {
  pub flags: u16,
  pub to_color: WideColor,
  pub to_index: u8,
  pub unused: u8,
  pub source: RecolorEntrySource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecolorEntrySource {
  Color {
    from_color: WideColor,
    unused: [u8; 26],
  },
  Brush {
    style: u16,
    color: WideColor,
    hatch: u16,
    foreground_color: WideColor,
    background_color: WideColor,
    bitmap_type: u16,
    pattern: [u8; 8],
  },
  Unknown {
    variant_type: u16,
    body: [u8; 32],
  },
}

#[derive(Clone, Debug, PartialEq)]
pub struct MacPlistAtom {
  /// Exact XML payload. Property-list XML is an external format.
  pub physical_xml: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ppt11FontDescriptorAtom {
  pub descriptors: Vec<Ppt11FontDescriptor>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ppt11FontDescriptor {
  pub byte_order: Ppt11FontDescriptorByteOrder,
  /// Fixed-size Office 11 font descriptor serialization. Its framing is static;
  /// the internal numbered property stream is retained for further promotion.
  pub serialized_properties: [u8; 276],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ppt11FontDescriptorByteOrder {
  LittleEndian,
  BigEndian,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ppt11FontDescriptorCollectionAtom {
  pub descriptors: Vec<Ppt11FontDescriptor>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacLegacyPrintInfoAtom {
  pub bytes: [u8; 120],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacPrintDriverInfoAtom {
  pub bytes: [u8; 52],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoutCompatibilityAtom {
  pub bytes: [u8; 8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownPptRecord {
  pub record_type: u16,
  pub body: Vec<u8>,
}

fn malformed_spec_record(record_type: u16, body: &[u8]) -> PptRecordData {
  let value = UnknownPptRecord {
    record_type,
    body: body.to_vec(),
  };
  if is_ms_ppt_record_type(record_type) {
    PptRecordData::MalformedSpecRecord(value)
  } else {
    PptRecordData::Unknown(value)
  }
}

fn preserved_unparsed_record(record_type: u16, body: &[u8]) -> PptRecordData {
  malformed_spec_record(record_type, body)
}

fn is_ms_ppt_record_type(record_type: u16) -> bool {
  matches!(
      record_type,
      0x03e8..=0x03ea
          | 0x03ee..=0x03f3
          | 0x03f8..=0x03fb
          | 0x03fd..=0x0402
          | 0x0406..=0x0415
          | 0x041c..=0x0420
          | 0x0422..=0x0428
          | 0x07d0
          | 0x07d5..=0x07d6
          | 0x07e3..=0x07e7
          | 0x07e9
          | 0x07f0
          | 0x07f8..=0x07f9
          | 0x0bc1
          | 0x0bc3
          | 0x0bdb..=0x0bdd
          | 0x0f9e..=0x0fb8
          | 0x0fba
          | 0x0fc1
          | 0x0fc3
          | 0x0fc8..=0x0fc9
          | 0x0fcc..=0x0fce
          | 0x0fd0..=0x0fd3
          | 0x0fd7..=0x0fda
          | 0x0fdf
          | 0x0fe4
          | 0x0fe7
          | 0x0fee
          | 0x0ff0..=0x0ff3
          | 0x0ff5..=0x0ffb
          | 0x1004..=0x1007
          | 0x100d..=0x1015
          | 0x1018
          | 0x1388..=0x138b
          | 0x1770
          | 0x1772
          | 0x177a..=0x177f
          | 0x1784..=0x1785
          | 0x2afb
          | 0x2b00..=0x2b0b
          | 0x2b0d
          | 0x2ee0..=0x2ee1
          | 0x2ee4..=0x2ee7
          | 0x2eea..=0x2ef1
          | 0x2f14
          | 0x32c8
          | 0x36b0..=0x36b3
          | 0x3714..=0x3715
          | 0xf125
          | 0xf127..=0xf145
  )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleTextProp9 {
  pub paragraph: TextParagraphException9,
  pub character: TextCharacterException9,
  pub special_info: TextSpecialInfoException,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TextParagraphException9 {
  pub mask: u32,
  #[sdk(condition = "mask", mask = 0x0080_0000)]
  pub bullet_blip_ref: Option<u16>,
  #[sdk(condition = "mask", mask = 0x0200_0000)]
  pub bullet_has_auto_number: Option<i16>,
  #[sdk(condition = "mask", mask = 0x0100_0000)]
  pub auto_number_scheme: Option<TextAutoNumberScheme>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TextAutoNumberScheme {
  pub scheme: u16,
  pub start_number: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct TextCharacterException9 {
  pub mask: u32,
  /// Low four bits are pp10runid; the remaining bits are retained unused data.
  #[sdk(condition = "mask", mask = 0x0010_0000)]
  pub pp10_extension: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSpecialInfoException {
  pub mask: u32,
  pub spelling_flags: Option<u16>,
  pub language_id: Option<u16>,
  pub alternate_language_id: Option<u16>,
  pub bidi: Option<i16>,
  pub pp10_extension: Option<u32>,
  pub smart_tag_indices: Option<Vec<u32>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalStorageAtom {
  Parsed(Box<ParsedExternalStorage>),
  MalformedCompressed {
    body: Vec<u8>,
    reason: String,
  },
  InvalidCompressed {
    declared_decompressed_size: u32,
    compressed_bytes: Vec<u8>,
    reason: String,
  },
  InvalidUncompressed {
    storage_bytes: Vec<u8>,
    reason: String,
  },
  UnsupportedInstance {
    instance: u16,
    body: Vec<u8>,
  },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedExternalStorage {
  compound_file: CompoundFile,
  encoding: ExternalStorageEncoding,
  vba_project: ExternalStorageVba,
}

impl ParsedExternalStorage {
  pub const fn compound_file(&self) -> &CompoundFile {
    &self.compound_file
  }

  pub const fn vba_project(&self) -> &ExternalStorageVba {
    &self.vba_project
  }

  pub const fn encoding(&self) -> &ExternalStorageEncoding {
    &self.encoding
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalStorageVba {
  NotPresent,
  Parsed(Box<LocatedVbaProject>),
  Invalid(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalStorageEncoding {
  Uncompressed,
  Zlib {
    declared_decompressed_size: u32,
    compressed_bytes: Vec<u8>,
  },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoundTripContentMasterInfo12Atom {
  pub layout_index: u16,
  pub package: SlideLayoutOpcPackage,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlideLayoutOpcPackage {
  /// Exact OPC package payload. OPC and its XML parts are external formats.
  pub physical_bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoundTripTheme12Atom {
  pub package: ThemeOpcPackage,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThemeOpcPackage {
  /// Exact OPC package payload. OPC and its XML parts are external formats.
  pub physical_bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoundTripColorMapping12Atom {
  /// Exact XML payload. DrawingML is an external format.
  pub physical_xml: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoundTripAnimation12Atom {
  pub package: TimingOpcPackage,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimingOpcPackage {
  /// Exact OPC package payload. OPC and its XML parts are external formats.
  pub physical_bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoundTripStyle12Atom {
  pub record_type: u16,
  pub package: StyleOpcPackage,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StyleOpcPackage {
  /// Exact OPC package payload. OPC and its XML parts are external formats.
  pub physical_bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncrementalSaveChain {
  /// User edits ordered from the current edit toward the oldest edit.
  pub edits: Vec<IncrementalSaveEdit>,
  /// Effective newest-wins mapping from persist object identifier to stream offset.
  pub persist_object_offsets: BTreeMap<u32, u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncrementalSaveEdit {
  pub user_edit_offset: u32,
  pub user_edit: UserEditAtom,
  pub persist_directory_offset: u32,
  pub persist_directory: PersistDirectoryAtom,
}

/// MS-PPT 2.1.2 Part 1 newest-wins directory plus its physical history.
///
/// A current directory reference is not necessarily a live presentation
/// object. Presentation liveness additionally depends on the references
/// reached from the current DocumentContainer in Parts 2 through 11.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistObjectDirectory {
  /// User-edit chain used to construct this directory.
  pub incremental_save_chain: IncrementalSaveChain,
  /// Every physical directory entry, ordered from the current edit toward
  /// the oldest edit and in physical entry order within each edit.
  pub references: Vec<PersistObjectReference>,
  /// Effective newest-wins reference for each persist object identifier.
  pub current_references: BTreeMap<u32, PersistObjectReference>,
  /// Classification of every physical top-level record in stream order.
  pub top_level_records: Vec<PptTopLevelRecordState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersistObjectReference {
  pub persist_id: u32,
  pub stream_offset: u32,
  pub record_index: usize,
  pub user_edit_offset: u32,
  pub persist_directory_offset: u32,
  pub status: PersistObjectReferenceStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistObjectReferenceStatus {
  /// The newest entry for this persist object identifier.
  Current,
  /// An older entry replaced by a later user edit.
  Superseded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PptTopLevelRecordState {
  pub record_index: usize,
  pub stream_offset: u64,
  pub record_type: u16,
  pub role: PptTopLevelRecordRole,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PptTopLevelRecordRole {
  PersistObject {
    /// IDs whose newest directory entry points at this record.
    current_persist_ids: Vec<u32>,
    /// IDs from older directory entries that point at this record.
    superseded_persist_ids: Vec<u32>,
  },
  /// UserEditAtom or PersistDirectoryAtom reached from CurrentUserAtom.
  IncrementalSaveMetadata(IncrementalSaveMetadataKind),
  /// A top-level record not reached by the current directory or edit chain.
  Unreferenced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncrementalSaveMetadataKind {
  UserEditAtom,
  PersistDirectoryAtom,
}

/// MS-PPT 2.1.2 Parts 1 through 11 resolved against the current
/// `DocumentContainer`.
///
/// This is an index over the authoritative recursive record tree. Every
/// descendant of a live top-level record has the same live status by the
/// specification; dead records remain present in `PowerPointDocument` for
/// physical-history preservation.
#[derive(Clone, Debug, PartialEq)]
pub struct PptLivePresentation<'a> {
  pub persist_object_directory: PersistObjectDirectory,
  pub document: PptLivePersistObject<'a>,
  pub document_atom_record: &'a PptRecord,
  pub document_atom: &'a DocumentAtom,
  pub notes_master_slide: Option<PptLivePersistObject<'a>>,
  pub handout_master_slide: Option<PptLivePersistObject<'a>>,
  pub master_slides: Vec<PptLivePersistObject<'a>>,
  pub presentation_slides: Vec<PptLivePersistObject<'a>>,
  pub notes_slides: Vec<PptLivePersistObject<'a>>,
  pub active_x_controls: Vec<PptLivePersistObject<'a>>,
  pub embedded_ole_objects: Vec<PptLivePersistObject<'a>>,
  pub linked_ole_objects: Vec<PptLivePersistObject<'a>>,
  pub vba_project: Option<PptLivePersistObject<'a>>,
  pub top_level_records: Vec<PptTopLevelLiveRecordState>,
}

/// Borrowed document-wide image store resolved from the live OfficeArt
/// `BStoreContainer` and the optional PPT `Pictures` stream. Payload bytes
/// remain owned exactly once by the corresponding typed record tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PptLiveImageStore<'a> {
  pub entries: Vec<PptLiveImageLink<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PptLiveImageLink<'a> {
  Resolved(PptLiveImageRef<'a>),
  Unresolved {
    blip_identifier: u32,
    issue: PptImageResolutionIssue,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PptLiveImageRef<'a> {
  pub blip_identifier: u32,
  pub source: PptLiveImageSource,
  pub image: crate::office_art::OfficeArtImageRef<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PptLiveImageSource {
  Embedded {
    store_index: usize,
  },
  PicturesStream {
    record_index: usize,
    offset: u32,
    compatible_stream: bool,
  },
  DirectBlipStoreEntry {
    store_index: usize,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PptImageResolutionIssue {
  MissingPicturesStream,
  PartialPicturesStream,
  MissingDelayOffset,
  UnsupportedPayload,
  UnsupportedBlipStoreEntry,
}

/// One live persist object joined directly to both the record containing its
/// `PersistIdRef` and the current target record selected by the newest-wins
/// persist object directory. The records remain owned exactly once by the
/// [`PowerPointDocument`] physical tree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PptLivePersistObject<'a> {
  pub reference: PersistObjectReference,
  pub role: PptLivePersistObjectRole,
  /// Record containing the `PersistIdRef` that made this object live.
  pub source_record: &'a PptRecord,
  /// Current top-level persist object reached by that reference.
  pub record: &'a PptRecord,
  /// Records following a `SlidePersistAtom` up to the next persist atom in
  /// its owning `SlideListWithTextContainer`. Empty for objects not reached
  /// through a slide/master/notes list.
  pub list_records: &'a [PptRecord],
}

/// One complete text-body group beginning at a `TextHeaderAtom` and ending
/// immediately before the next header or slide persist, as defined by
/// MS-PPT 2.9.79. All records borrow the physical presentation tree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PptLiveTextBodyRef<'a> {
  pub header_record: &'a PptRecord,
  pub header: &'a TextHeaderAtom,
  pub records: &'a [PptRecord],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PptTextEncoding {
  Utf16,
  Bytes,
}

/// One physical TextCharsAtom or TextBytesAtom in a live text body. Normal
/// values borrow a Rust string directly; malformed UTF-16 remains explicit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PptLiveTextAtomRef<'a> {
  String {
    source_record: &'a PptRecord,
    value: &'a str,
    encoding: PptTextEncoding,
  },
  CompatibilityUtf16 {
    source_record: &'a PptRecord,
    code_units: &'a [u16],
  },
}

/// One OfficeArt SpContainer in a live persist object, joined to its FSP,
/// placeholder, outline-text body, and table marker owners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PptLiveShapeRef<'a> {
  pub source_record: &'a PptRecord,
  pub parent_shape_record: Option<&'a PptRecord>,
  pub group_record: Option<&'a PptRecord>,
  pub shape_record: &'a PptRecord,
  pub shape: &'a crate::office_art::OfficeArtShape,
  pub placeholder_record: Option<&'a PptRecord>,
  pub placeholder: Option<&'a PlaceholderAtom>,
  pub outline_text: Option<PptLiveOutlineTextRef<'a>>,
  pub table_property: Option<&'a crate::office_art::OfficeArtProperty>,
}

/// One row or column interval in a native PPT table, expressed in master
/// coordinates and derived from the physical cell anchors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PptLiveTableTrack {
  pub start: i32,
  pub end: i32,
}

/// One native PPT table-cell shape projected onto its exact row/column grid.
/// The physical shape remains the sole owner of text, formatting and anchor
/// data; this handle only retains grid identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PptLiveTableCellRef<'a> {
  pub shape: PptLiveShapeRef<'a>,
  pub row: usize,
  pub column: usize,
  pub row_span: usize,
  pub column_span: usize,
}

/// Borrowed native table relationship assembled from the table marker and
/// its actual group children. Border line shapes remain explicit because
/// they carry formatting that is separate from cell content.
#[derive(Clone, Debug, PartialEq)]
pub struct PptLiveTableRef<'a> {
  pub shape: PptLiveShapeRef<'a>,
  pub anchor: crate::office_art::OfficeArtRect,
  pub rows: Vec<PptLiveTableTrack>,
  pub columns: Vec<PptLiveTableTrack>,
  pub cells: Vec<PptLiveTableCellRef<'a>>,
  pub borders: Vec<PptLiveShapeRef<'a>>,
}

/// Mutable static record group for one list text body. It deliberately does
/// not collapse text, styles, bookmarks, special information, or interactive
/// records into a string projection.
pub struct PptLiveTextBodyMut<'a> {
  records: &'a mut [PptRecord],
}

impl<'a> PptLiveTextBodyMut<'a> {
  pub fn header_mut(&mut self) -> &mut TextHeaderAtom {
    let PptRecordData::TextHeader(value) = &mut self.records[0].data else {
      unreachable!("a text body begins with TextHeaderAtom")
    };
    value
  }

  pub fn records(&self) -> &[PptRecord] {
    self.records
  }

  pub fn records_mut(&mut self) -> &mut [PptRecord] {
    self.records
  }
}

impl<'a> PptLiveTextBodyRef<'a> {
  pub fn character_atoms(self) -> impl Iterator<Item = PptLiveTextAtomRef<'a>> {
    self.records.iter().filter_map(|record| match &record.data {
      PptRecordData::TextChars(value) => Some(PptLiveTextAtomRef::String {
        source_record: record,
        value,
        encoding: PptTextEncoding::Utf16,
      }),
      PptRecordData::TextBytes(value) => Some(PptLiveTextAtomRef::String {
        source_record: record,
        value,
        encoding: PptTextEncoding::Bytes,
      }),
      PptRecordData::CompatibilityTextChars(code_units) => {
        Some(PptLiveTextAtomRef::CompatibilityUtf16 {
          source_record: record,
          code_units,
        })
      }
      _ => None,
    })
  }

  pub fn style_text_properties(self) -> impl Iterator<Item = &'a StyleTextPropAtom> {
    self.records.iter().filter_map(|record| {
      if let PptRecordData::StyleTextProp(value) = &record.data {
        Some(value)
      } else {
        None
      }
    })
  }
}

/// One `OutlineTextRefAtom` in a live persist object, joined to the indexed
/// text body in the corresponding `SlideListWithTextContainer`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PptLiveOutlineTextRef<'a> {
  pub shape_record: Option<&'a PptRecord>,
  pub source_record: &'a PptRecord,
  pub value: &'a OutlineTextRefAtom,
  pub text_body: PptLiveTextBodyRef<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PptLiveOutlineTextLink<'a> {
  Resolved(PptLiveOutlineTextRef<'a>),
  Unresolved {
    shape_record: Option<&'a PptRecord>,
    source_record: &'a PptRecord,
    value: &'a OutlineTextRefAtom,
    text_body_count: usize,
  },
}

/// A live presentation slide joined to its list identity, concrete
/// `SlideContainer`/`SlideAtom`, master, and notes relationships.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PptLiveSlideRef<'view, 'a> {
  pub object: &'view PptLivePersistObject<'a>,
  pub persist: &'a SlidePersistAtom,
  pub slide_atom_record: &'a PptRecord,
  pub slide_atom: &'a SlideAtom,
  pub master: PptLiveMasterLink<'view, 'a>,
  pub notes: PptLiveNotesLink<'view, 'a>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PptLiveSlideTransitionRef<'a> {
  pub source_record: &'a PptRecord,
  pub value: &'a SlideShowSlideInfoAtom,
}

/// The active legacy eight-color scheme owned by one live PPT sheet.
///
/// MS-PPT permits additional palette atoms before the sheet's PPDrawing. The
/// active sheet palette is the unique ColorSchemeAtom following that drawing,
/// matching the relationship used by PowerPoint and Apache POI without
/// copying its eight RGB values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PptLiveColorSchemeRef<'a> {
  pub source_record: &'a PptRecord,
  pub value: &'a ColorSchemeAtom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PptSlideId(u32);

impl PptSlideId {
  pub const fn value(self) -> u32 {
    self.0
  }
}

impl<'view, 'a> PptLiveSlideRef<'view, 'a> {
  pub const fn id(self) -> PptSlideId {
    PptSlideId(self.persist.slide_id)
  }

  pub fn shapes(self) -> Result<Vec<PptLiveShapeRef<'a>>> {
    self.object.shapes()
  }

  /// Returns the slide-show transition atom owned by this slide. A slide
  /// with duplicate transition atoms is rejected instead of selecting one
  /// by record order.
  pub fn transition(self) -> Result<Option<PptLiveSlideTransitionRef<'a>>> {
    let mut transitions = Vec::new();
    collect_slide_transitions(self.object.record, &mut transitions);
    match transitions.as_slice() {
      [] => Ok(None),
      [transition] => Ok(Some(*transition)),
      _ => Err(Error::invalid(
        self.object.record.offset,
        "PPT slide contains multiple SlideShowSlideInfoAtom records",
      )),
    }
  }
}

fn collect_slide_transitions<'a>(
  record: &'a PptRecord,
  output: &mut Vec<PptLiveSlideTransitionRef<'a>>,
) {
  if let PptRecordData::SlideShowSlideInfo(value) = &record.data {
    output.push(PptLiveSlideTransitionRef {
      source_record: record,
      value,
    });
  }
  match &record.data {
    PptRecordData::Container(children) => {
      for child in &children.records {
        collect_slide_transitions(child, output);
      }
    }
    PptRecordData::BinaryTagData(BinaryTagData::Records(children)) => {
      for child in &children.records {
        collect_slide_transitions(child, output);
      }
    }
    PptRecordData::ProgBinaryTag(value) => {
      for child in &value.records.records {
        collect_slide_transitions(child, output);
      }
    }
    PptRecordData::ProgTags(children) => {
      for child in &children.records {
        collect_slide_transitions(child, output);
      }
    }
    _ => {}
  }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PptLiveMasterLink<'view, 'a> {
  Resolved(&'view PptLivePersistObject<'a>),
  NotSpecified,
  Missing { master_id: u32 },
  Ambiguous { master_id: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PptLiveNotesLink<'view, 'a> {
  Resolved {
    object: &'view PptLivePersistObject<'a>,
    notes_atom_record: &'a PptRecord,
    notes_atom: &'a NotesAtom,
  },
  NotSpecified,
  Missing {
    notes_id: u32,
  },
  Ambiguous {
    notes_id: u32,
  },
  SlideMismatch {
    object: &'view PptLivePersistObject<'a>,
    notes_atom_record: &'a PptRecord,
    notes_atom: &'a NotesAtom,
    notes_id: u32,
    expected_slide_id: u32,
    actual_slide_id: u32,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PptLivePersistObjectRole {
  Document,
  NotesMasterSlide,
  HandoutMasterSlide,
  MainMasterSlide,
  TitleMasterSlide,
  PresentationSlide,
  NotesSlide,
  ActiveXControl,
  EmbeddedOleObject,
  LinkedOleObject,
  VbaProject,
}

impl<'a> PptLivePersistObject<'a> {
  pub const fn source_record(self) -> &'a PptRecord {
    self.source_record
  }

  pub const fn record(self) -> &'a PptRecord {
    self.record
  }

  /// Resolves the active sheet color scheme by structural ownership. A
  /// duplicate active scheme is rejected instead of depending on record
  /// order, while pre-drawing palette atoms remain distinct legacy data.
  pub fn color_scheme(self) -> Result<Option<PptLiveColorSchemeRef<'a>>> {
    let children = ppt_container_children(self.record, "PPT live persist object")?;
    let mut after_drawing = false;
    let mut active = None;
    for record in &children.records {
      if record.header.record_type == PP_DRAWING {
        after_drawing = true;
        continue;
      }
      if !after_drawing {
        continue;
      }
      let PptRecordData::ColorScheme(value) = &record.data else {
        continue;
      };
      if active.is_some() {
        return Err(Error::invalid(
          record.offset,
          "PPT sheet contains multiple active ColorSchemeAtom records",
        ));
      }
      active = Some(PptLiveColorSchemeRef {
        source_record: record,
        value,
      });
    }
    Ok(active)
  }

  pub const fn slide_persist(self) -> Option<&'a SlidePersistAtom> {
    match &self.source_record.data {
      PptRecordData::SlidePersist(value) => Some(value),
      _ => None,
    }
  }

  /// Returns the direct `SlideAtom` owned by a live `SlideContainer`.
  ///
  /// Presentation slides and title masters use the same wire container. A
  /// main master has a `MainMasterContainer` instead and therefore returns
  /// `None`. The pair retains both the record identity and the typed atom so
  /// relationship diagnostics never need to rediscover an offset.
  pub fn slide_atom(self) -> Result<Option<(&'a PptRecord, &'a SlideAtom)>> {
    if self.record.header.record_type != SLIDE_CONTAINER {
      return Ok(None);
    }
    let children = ppt_container_children(self.record, "SlideContainer")?;
    let record = required_direct_record(children, SLIDE_ATOM, Some(0), "SlideContainer.slideAtom")?;
    let PptRecordData::Slide(value) = &record.data else {
      return Err(Error::invalid(
        record.offset,
        "SlideContainer.slideAtom is not a conforming SlideAtom",
      ));
    };
    Ok(Some((record, value)))
  }

  /// Returns every `TextHeaderAtom` group following this object's
  /// `SlidePersistAtom`. The returned vector contains only small borrowed
  /// handles and does not copy text or formatting records.
  pub fn text_bodies(self) -> Vec<PptLiveTextBodyRef<'a>> {
    let mut bodies = Vec::new();
    let mut cursor = 0usize;
    while cursor < self.list_records.len() {
      let record = &self.list_records[cursor];
      let PptRecordData::TextHeader(header) = &record.data else {
        cursor += 1;
        continue;
      };
      let end = self.list_records[cursor + 1..]
        .iter()
        .position(|candidate| matches!(candidate.data, PptRecordData::TextHeader(_)))
        .map_or(self.list_records.len(), |relative| cursor + 1 + relative);
      bodies.push(PptLiveTextBodyRef {
        header_record: record,
        header,
        records: &self.list_records[cursor..end],
      });
      cursor = end;
    }
    bodies
  }

  /// Returns text-body groups physically contained in the persist object
  /// itself. This is the native path for notes text and shape-local text;
  /// list text bodies remain available through [`Self::text_bodies`].
  pub fn record_text_bodies(self) -> Vec<PptLiveTextBodyRef<'a>> {
    let mut bodies = Vec::new();
    collect_record_text_bodies(self.record, &mut bodies);
    bodies
  }

  /// Returns live OfficeArt shapes in physical z-order. Each shape retains
  /// the unique SpContainer/FSP source and any exact PPT client-data links.
  pub fn shapes(self) -> Result<Vec<PptLiveShapeRef<'a>>> {
    let outline_text = self.outline_text_references()?;
    let mut shapes = Vec::new();
    collect_live_shapes(self.record, None, None, &outline_text, &mut shapes);
    Ok(shapes)
  }

  /// Resolves every live `OutlineTextRefAtom.index` in this persist object
  /// to the exact text-body group following its `SlidePersistAtom`.
  pub fn outline_text_references(self) -> Result<Vec<PptLiveOutlineTextRef<'a>>> {
    self
      .outline_text_references_compatible()
      .into_iter()
      .map(|link| match link {
        PptLiveOutlineTextLink::Resolved(value) => Ok(value),
        PptLiveOutlineTextLink::Unresolved {
          source_record,
          value,
          ..
        } => Err(Error::invalid(
          source_record.offset,
          format!(
            "OutlineTextRefAtom.index {} has no corresponding TextHeaderAtom",
            value.index
          ),
        )),
      })
      .collect()
  }

  /// Preserves dangling producer references explicitly instead of guessing
  /// a text body by proximity or clamping the index.
  pub fn outline_text_references_compatible(self) -> Vec<PptLiveOutlineTextLink<'a>> {
    let text_bodies = self.text_bodies();
    let mut references = Vec::new();
    collect_live_outline_text_links(self.record, None, &text_bodies, &mut references);
    references
  }
}

impl<'a> PptLivePresentation<'a> {
  /// Resolves every presentation slide relationship required by MS-PPT
  /// 2.4.14.5 and 2.5.1. Missing, duplicate, or inconsistent targets are
  /// rejected rather than selected by list position.
  pub fn slides(&self) -> Result<Vec<PptLiveSlideRef<'_, 'a>>> {
    self.slides_compatible()?
            .into_iter()
            .map(|slide| match slide.master {
                PptLiveMasterLink::Resolved(_) => match slide.notes {
                    PptLiveNotesLink::Resolved { .. } | PptLiveNotesLink::NotSpecified => Ok(slide),
                    PptLiveNotesLink::Missing { notes_id } => Err(Error::invalid(
                        slide.slide_atom_record.offset,
                        format!("SlideAtom.notesIdRef {notes_id} does not resolve"),
                    )),
                    PptLiveNotesLink::Ambiguous { notes_id } => Err(Error::invalid(
                        slide.slide_atom_record.offset,
                        format!("SlideAtom.notesIdRef {notes_id} is ambiguous"),
                    )),
                    PptLiveNotesLink::SlideMismatch {
                        expected_slide_id,
                        actual_slide_id,
                        ..
                    } => Err(Error::invalid(
                        slide.slide_atom_record.offset,
                        format!(
                            "NotesAtom.slideIdRef {actual_slide_id} does not match SlidePersistAtom.slideId {expected_slide_id}"
                        ),
                    )),
                },
                PptLiveMasterLink::NotSpecified => Err(Error::invalid(
                    slide.slide_atom_record.offset,
                    "presentation SlideAtom.masterIdRef is null",
                )),
                PptLiveMasterLink::Missing { master_id } => Err(Error::invalid(
                    slide.slide_atom_record.offset,
                    format!("SlideAtom.masterIdRef {master_id} does not resolve"),
                )),
                PptLiveMasterLink::Ambiguous { master_id } => Err(Error::invalid(
                    slide.slide_atom_record.offset,
                    format!("SlideAtom.masterIdRef {master_id} is ambiguous"),
                )),
            })
            .collect()
  }

  /// Builds all unambiguous slide links and retains exact named unresolved
  /// relationships for producer-compatible inspection.
  pub fn slides_compatible(&self) -> Result<Vec<PptLiveSlideRef<'_, 'a>>> {
    let mut slides = Vec::with_capacity(self.presentation_slides.len());
    for object in &self.presentation_slides {
      let persist = object.slide_persist().ok_or_else(|| {
        Error::invalid(
          object.source_record.offset,
          "presentation slide source is not a SlidePersistAtom",
        )
      })?;
      let children = ppt_container_children(object.record, "SlideContainer")?;
      let slide_atom_record =
        required_direct_record(children, SLIDE_ATOM, Some(0), "SlideContainer.slideAtom")?;
      let PptRecordData::Slide(slide_atom) = &slide_atom_record.data else {
        return Err(Error::invalid(
          slide_atom_record.offset,
          "SlideContainer.slideAtom is not a conforming SlideAtom",
        ));
      };

      let master = resolve_live_master_link(&self.master_slides, slide_atom.master_id_ref);
      let notes = resolve_live_notes_link(
        &self.notes_slides,
        slide_atom.notes_id_ref,
        persist.slide_id,
      )?;
      slides.push(PptLiveSlideRef {
        object,
        persist,
        slide_atom_record,
        slide_atom,
        master,
        notes,
      });
    }
    Ok(slides)
  }
}

fn resolve_live_master_link<'view, 'a>(
  masters: &'view [PptLivePersistObject<'a>],
  master_id: u32,
) -> PptLiveMasterLink<'view, 'a> {
  if master_id == 0 {
    return PptLiveMasterLink::NotSpecified;
  }
  let mut candidates = masters.iter().filter(|object| {
    object
      .slide_persist()
      .is_some_and(|value| value.slide_id == master_id)
  });
  let first = candidates.next();
  match (first, candidates.next()) {
    (Some(value), None) => PptLiveMasterLink::Resolved(value),
    (None, _) => PptLiveMasterLink::Missing { master_id },
    (Some(_), Some(_)) => PptLiveMasterLink::Ambiguous { master_id },
  }
}

fn resolve_live_notes_link<'view, 'a>(
  notes: &'view [PptLivePersistObject<'a>],
  notes_id: u32,
  slide_id: u32,
) -> Result<PptLiveNotesLink<'view, 'a>> {
  if notes_id == 0 {
    return Ok(PptLiveNotesLink::NotSpecified);
  }
  let mut candidates = notes.iter().filter(|object| {
    object
      .slide_persist()
      .is_some_and(|value| value.slide_id == notes_id)
  });
  let Some(object) = candidates.next() else {
    return Ok(PptLiveNotesLink::Missing { notes_id });
  };
  if candidates.next().is_some() {
    return Ok(PptLiveNotesLink::Ambiguous { notes_id });
  }
  let children = ppt_container_children(object.record, "NotesContainer")?;
  let notes_atom_record =
    required_direct_record(children, NOTES_ATOM, Some(0), "NotesContainer.notesAtom")?;
  let PptRecordData::Notes(notes_atom) = &notes_atom_record.data else {
    return Err(Error::invalid(
      notes_atom_record.offset,
      "NotesContainer.notesAtom is not a conforming NotesAtom",
    ));
  };
  if notes_atom.slide_id_ref != slide_id {
    return Ok(PptLiveNotesLink::SlideMismatch {
      object,
      notes_atom_record,
      notes_atom,
      notes_id,
      expected_slide_id: slide_id,
      actual_slide_id: notes_atom.slide_id_ref,
    });
  }
  Ok(PptLiveNotesLink::Resolved {
    object,
    notes_atom_record,
    notes_atom,
  })
}

fn collect_live_outline_text_links<'a>(
  record: &'a PptRecord,
  containing_shape: Option<&'a PptRecord>,
  text_bodies: &[PptLiveTextBodyRef<'a>],
  output: &mut Vec<PptLiveOutlineTextLink<'a>>,
) {
  const OFFICE_ART_SP_CONTAINER: u16 = 0xf004;

  let containing_shape = (record.header.record_type == OFFICE_ART_SP_CONTAINER)
    .then_some(record)
    .or(containing_shape);
  if let PptRecordData::OutlineTextRef(value) = &record.data {
    let text_body = usize::try_from(value.index)
      .ok()
      .and_then(|index| text_bodies.get(index))
      .copied();
    output.push(match text_body {
      Some(text_body) => PptLiveOutlineTextLink::Resolved(PptLiveOutlineTextRef {
        shape_record: containing_shape,
        source_record: record,
        value,
        text_body,
      }),
      None => PptLiveOutlineTextLink::Unresolved {
        shape_record: containing_shape,
        source_record: record,
        value,
        text_body_count: text_bodies.len(),
      },
    });
  }
  let children = match &record.data {
    PptRecordData::Container(children)
    | PptRecordData::ProgTags(children)
    | PptRecordData::BinaryTagData(BinaryTagData::Records(children)) => Some(children),
    _ => None,
  };
  if let Some(children) = children {
    for child in &children.records {
      collect_live_outline_text_links(child, containing_shape, text_bodies, output);
    }
  }
}

impl<'a> PptLiveShapeRef<'a> {
  pub const fn shape_id(self) -> u32 {
    self.shape.shape_id
  }

  pub const fn shape_type(self) -> u16 {
    self.shape_record.header.instance
  }

  pub const fn is_table(self) -> bool {
    self.table_property.is_some()
  }

  /// Returns the nearest owning shape without allocating or rebuilding the
  /// OfficeArt tree. Direct children of the patriarch return that patriarch;
  /// children of nested groups return the nested group shape.
  pub fn parent_shape(self) -> Option<&'a crate::office_art::OfficeArtShape> {
    let mut parent = None;
    if let Some(record) = self.parent_shape_record {
      visit_shape_owned_records(record, &mut |candidate| {
        if parent.is_none()
          && let PptRecordData::OfficeArt(office_art) = &candidate.data
          && let OfficeArtRecordData::Shape(value) = &office_art.data
        {
          parent = Some(value);
        }
      });
    }
    parent
  }

  /// Whether this shape's anchor is expressed in a nested group's local
  /// coordinate system rather than the slide/master coordinate system.
  pub fn is_nested_group_child(self) -> bool {
    self.parent_shape().is_some_and(|parent| {
      !parent
        .flags
        .contains(crate::office_art::OfficeArtShapeFlags::PATRIARCH)
    })
  }

  /// Resolves the primary `pib` (`0x0104`) image-store identifier owned by
  /// this shape. Fill-pattern BLIPs remain separate properties and are not
  /// mistaken for the shape's primary picture payload.
  pub fn primary_blip_identifier(self) -> Result<Option<u32>> {
    let mut identifier = None;
    let mut incomplete = false;
    let mut invalid = false;
    visit_shape_owned_records(self.source_record, &mut |record| {
      let PptRecordData::OfficeArt(office_art) = &record.data else {
        return;
      };
      match &office_art.data {
        OfficeArtRecordData::PropertyTable(table) => {
          for property in &table.properties {
            if property.property_id != 0x0104 {
              continue;
            }
            let crate::office_art::OfficeArtPropertyValue::Simple(value) = property.value else {
              invalid = true;
              continue;
            };
            if !property.is_blip_id || value != 0 && identifier.replace(value).is_some() {
              invalid = true;
            }
          }
        }
        OfficeArtRecordData::IncompletePropertyTable(table)
          if table
            .entries
            .iter()
            .any(|property| property.property_id == 0x0104) =>
        {
          incomplete = true;
        }
        _ => {}
      }
    });
    if incomplete {
      return Err(Error::invalid(
        self.source_record.offset,
        "PPT shape primary BLIP property table is incomplete",
      ));
    }
    if invalid {
      return Err(Error::invalid(
        self.source_record.offset,
        "PPT shape has an invalid or duplicate primary BLIP property",
      ));
    }
    Ok(identifier)
  }

  /// Resolves the one PowerPoint client/child anchor owned by this shape.
  /// Group-child coordinates are returned in their native group coordinate
  /// space; callers can use [`Self::group_record`] to distinguish them.
  pub fn anchor(self) -> Result<Option<crate::office_art::OfficeArtRect>> {
    let mut anchor = None;
    let mut invalid_host_anchor = false;
    let mut duplicate = false;
    visit_shape_owned_records(self.source_record, &mut |record| {
      let PptRecordData::OfficeArt(office_art) = &record.data else {
        return;
      };
      let value = match office_art.data {
        OfficeArtRecordData::ChildAnchor(value)
        | OfficeArtRecordData::ClientAnchor(
          crate::office_art::OfficeArtClientAnchor::PowerPointRect(value),
        ) => Some(value),
        OfficeArtRecordData::ClientAnchor(crate::office_art::OfficeArtClientAnchor::Words8 {
          coordinates,
        }) => Some(crate::office_art::OfficeArtRect {
          left: i32::from(coordinates[1]),
          top: i32::from(coordinates[0]),
          right: i32::from(coordinates[2]),
          bottom: i32::from(coordinates[3]),
        }),
        OfficeArtRecordData::ClientAnchor(_) => {
          invalid_host_anchor = true;
          None
        }
        _ => None,
      };
      if let Some(value) = value
        && anchor.replace(value).is_some()
      {
        duplicate = true;
      }
    });
    if invalid_host_anchor {
      return Err(Error::invalid(
        self.source_record.offset,
        "PPT shape uses a non-PowerPoint OfficeArtClientAnchor",
      ));
    }
    if duplicate {
      return Err(Error::invalid(
        self.source_record.offset,
        "PPT shape owns multiple OfficeArt anchors",
      ));
    }
    Ok(anchor)
  }

  /// Returns directly nested SpContainers in their physical order. For a
  /// table these are its actual child shapes; row/column projection remains
  /// separate because it depends on anchors and merged-cell geometry.
  pub fn child_shapes(self) -> Vec<PptLiveShapeRef<'a>> {
    if !self
      .shape
      .flags
      .contains(crate::office_art::OfficeArtShapeFlags::GROUP)
    {
      return Vec::new();
    }
    let mut shapes = Vec::new();
    if let Some(group_record) = self.group_record {
      collect_group_shapes(group_record, self.source_record, &mut shapes);
    } else {
      collect_direct_child_shapes(self.source_record, &mut shapes);
    }
    shapes
  }

  /// Projects a native PPT table group into rows, columns and spanning
  /// cells without copying any cell text or OfficeArt record data.
  pub fn table(self) -> Result<Option<PptLiveTableRef<'a>>> {
    if !self.is_table() {
      return Ok(None);
    }
    let mut candidates = Vec::new();
    let mut borders = Vec::new();
    for child in self.child_shapes() {
      match child.shape_type() {
        1 => {
          let anchor = child.anchor()?.ok_or_else(|| {
            Error::invalid(
              child.source_record.offset,
              "PPT table cell has no OfficeArt child anchor",
            )
          })?;
          if anchor.right <= anchor.left || anchor.bottom <= anchor.top {
            return Err(Error::invalid(
              child.source_record.offset,
              "PPT table cell has an empty or reversed anchor",
            ));
          }
          candidates.push((child, anchor));
        }
        20 => borders.push(child),
        value => {
          return Err(Error::invalid(
            child.source_record.offset,
            format!("PPT table child has unsupported shape type {value}"),
          ));
        }
      }
    }
    if candidates.is_empty() {
      return Err(Error::invalid(
        self.source_record.offset,
        "PPT table has no cell shapes",
      ));
    }
    let row_starts = candidates
      .iter()
      .map(|(_, anchor)| anchor.top)
      .collect::<BTreeSet<_>>()
      .into_iter()
      .collect::<Vec<_>>();
    let column_starts = candidates
      .iter()
      .map(|(_, anchor)| anchor.left)
      .collect::<BTreeSet<_>>()
      .into_iter()
      .collect::<Vec<_>>();
    let content_right = candidates
      .iter()
      .map(|(_, anchor)| anchor.right)
      .max()
      .expect("a PPT table has cell candidates");
    let content_bottom = candidates
      .iter()
      .map(|(_, anchor)| anchor.bottom)
      .max()
      .expect("a PPT table has cell candidates");
    let anchor = self.anchor()?.unwrap_or(crate::office_art::OfficeArtRect {
      left: column_starts[0],
      top: row_starts[0],
      right: content_right,
      bottom: content_bottom,
    });
    let columns = table_tracks(&column_starts, content_right, "column")?;
    let rows = table_tracks(&row_starts, content_bottom, "row")?;
    let mut occupied = vec![false; rows.len() * columns.len()];
    let mut cells = Vec::with_capacity(candidates.len());
    for (shape, cell_anchor) in candidates {
      let row = row_starts
        .binary_search(&cell_anchor.top)
        .expect("PPT table row start comes from the same set");
      let column = column_starts
        .binary_search(&cell_anchor.left)
        .expect("PPT table column start comes from the same set");
      let row_span = row_starts[row..]
        .iter()
        .take_while(|&&start| start < cell_anchor.bottom)
        .count();
      let column_span = column_starts[column..]
        .iter()
        .take_while(|&&start| start < cell_anchor.right)
        .count();
      if row_span == 0 || column_span == 0 {
        return Err(Error::invalid(
          shape.source_record.offset,
          "PPT table cell does not cover its grid origin",
        ));
      }
      for occupied_row in row..row + row_span {
        for occupied_column in column..column + column_span {
          let slot = occupied_row * columns.len() + occupied_column;
          if std::mem::replace(&mut occupied[slot], true) {
            return Err(Error::invalid(
              shape.source_record.offset,
              "PPT table cell spans overlap",
            ));
          }
        }
      }
      cells.push(PptLiveTableCellRef {
        shape,
        row,
        column,
        row_span,
        column_span,
      });
    }
    if occupied.contains(&false) {
      return Err(Error::invalid(
        self.source_record.offset,
        "PPT table grid contains a cell hole",
      ));
    }
    cells.sort_by_key(|cell| (cell.row, cell.column));
    Ok(Some(PptLiveTableRef {
      shape: self,
      anchor,
      rows,
      columns,
      cells,
      borders,
    }))
  }

  pub fn text_bodies(self) -> Vec<PptLiveTextBodyRef<'a>> {
    let mut bodies = Vec::new();
    collect_record_text_bodies(self.source_record, &mut bodies);
    bodies
  }
}

fn table_tracks(starts: &[i32], content_end: i32, axis: &str) -> Result<Vec<PptLiveTableTrack>> {
  starts
    .iter()
    .copied()
    .enumerate()
    .map(|(index, start)| {
      let end = starts.get(index + 1).copied().unwrap_or(content_end);
      if end <= start {
        return Err(Error::invalid(
          u64::try_from(index).unwrap_or(u64::MAX),
          format!("PPT table {axis} track is empty or reversed"),
        ));
      }
      Ok(PptLiveTableTrack { start, end })
    })
    .collect()
}

fn collect_live_shapes<'a>(
  record: &'a PptRecord,
  parent_shape_record: Option<&'a PptRecord>,
  group_record: Option<&'a PptRecord>,
  outline_text: &[PptLiveOutlineTextRef<'a>],
  output: &mut Vec<PptLiveShapeRef<'a>>,
) {
  let mut child_parent = parent_shape_record;
  let child_group = if record.header.record_type == 0xf003 {
    Some(record)
  } else {
    group_record
  };
  if record.header.record_type == 0xf004 {
    if let Some(shape) = make_live_shape(record, parent_shape_record, group_record, outline_text) {
      output.push(shape);
    }
    child_parent = Some(record);
  }
  if let Some(children) = ppt_record_children(record) {
    for child in &children.records {
      collect_live_shapes(child, child_parent, child_group, outline_text, output);
    }
  }
}

fn make_live_shape<'a>(
  source_record: &'a PptRecord,
  parent_shape_record: Option<&'a PptRecord>,
  group_record: Option<&'a PptRecord>,
  outline_text: &[PptLiveOutlineTextRef<'a>],
) -> Option<PptLiveShapeRef<'a>> {
  let mut shape_record = None;
  let mut shape = None;
  let mut placeholder_record = None;
  let mut placeholder = None;
  let mut table_property = None;
  visit_shape_owned_records(source_record, &mut |record| {
    match &record.data {
        PptRecordData::OfficeArt(office_art) => match &office_art.data {
            OfficeArtRecordData::Shape(value) if shape.is_none() => {
                shape_record = Some(record);
                shape = Some(value);
            }
            OfficeArtRecordData::PropertyTable(table) => {
                if let Some(property) = table.properties.iter().find(|property| {
                    property.property_id == 0x039f
                        && matches!(property.value, crate::office_art::OfficeArtPropertyValue::Simple(value) if value & 1 != 0)
                }) {
                    table_property = Some(property);
                }
            }
            _ => {}
        },
        PptRecordData::Placeholder(value) if placeholder.is_none() => {
            placeholder_record = Some(record);
            placeholder = Some(value);
        }
        _ => {}
    }
  });
  let shape_record = shape_record?;
  let outline_text = outline_text
    .iter()
    .find(|reference| {
      reference
        .shape_record
        .is_some_and(|record| std::ptr::eq(record, source_record))
    })
    .copied();
  Some(PptLiveShapeRef {
    source_record,
    parent_shape_record,
    group_record,
    shape_record,
    shape: shape?,
    placeholder_record,
    placeholder,
    outline_text,
    table_property,
  })
}

fn collect_direct_child_shapes<'a>(record: &'a PptRecord, output: &mut Vec<PptLiveShapeRef<'a>>) {
  let Some(children) = ppt_record_children(record) else {
    return;
  };
  for child in &children.records {
    if child.header.record_type == 0xf004 {
      if let Some(shape) = make_live_shape(child, Some(record), None, &[]) {
        output.push(shape);
      }
    } else {
      collect_direct_child_shapes(child, output);
    }
  }
}

fn collect_group_shapes<'a>(
  group_record: &'a PptRecord,
  table_shape_record: &'a PptRecord,
  output: &mut Vec<PptLiveShapeRef<'a>>,
) {
  let Some(children) = ppt_record_children(group_record) else {
    return;
  };
  for child in &children.records {
    if child.header.record_type == 0xf004 {
      if !std::ptr::eq(child, table_shape_record)
        && let Some(shape) = make_live_shape(child, None, Some(group_record), &[])
      {
        output.push(shape);
      }
    } else if child.header.record_type != 0xf003 {
      collect_group_shapes(child, table_shape_record, output);
    }
  }
}

fn visit_shape_owned_records<'a>(record: &'a PptRecord, visit: &mut impl FnMut(&'a PptRecord)) {
  visit(record);
  if let Some(children) = ppt_record_children(record) {
    for child in &children.records {
      if child.header.record_type != 0xf004 {
        visit_shape_owned_records(child, visit);
      }
    }
  }
}

fn ppt_record_children(record: &PptRecord) -> Option<&PptRecordSequence> {
  match &record.data {
    PptRecordData::Container(children)
    | PptRecordData::ProgTags(children)
    | PptRecordData::BinaryTagData(BinaryTagData::Records(children)) => Some(children),
    PptRecordData::ProgBinaryTag(value) => Some(&value.records),
    _ => None,
  }
}

fn collect_record_text_bodies<'a>(record: &'a PptRecord, output: &mut Vec<PptLiveTextBodyRef<'a>>) {
  let Some(children) = ppt_record_children(record) else {
    return;
  };
  let mut cursor = 0usize;
  while cursor < children.records.len() {
    let child = &children.records[cursor];
    if let PptRecordData::TextHeader(header) = &child.data {
      let end = children.records[cursor + 1..]
        .iter()
        .position(|candidate| matches!(candidate.data, PptRecordData::TextHeader(_)))
        .map_or(children.records.len(), |relative| cursor + 1 + relative);
      output.push(PptLiveTextBodyRef {
        header_record: child,
        header,
        records: &children.records[cursor..end],
      });
      cursor = end;
      continue;
    }
    collect_record_text_bodies(child, output);
    cursor += 1;
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PptTopLevelLiveRecordState {
  pub record_index: usize,
  pub stream_offset: u64,
  pub record_type: u16,
  pub status: PptTopLevelLiveRecordStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PptTopLevelLiveRecordStatus {
  LiveIncrementalSaveMetadata(IncrementalSaveMetadataKind),
  LivePersistObject {
    persist_ids: Vec<u32>,
    roles: Vec<PptLivePersistObjectRole>,
  },
  Dead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PptTopLevelRecordKind {
  UserEdit,
  PersistDirectory,
  Other,
}

#[derive(Clone, Copy, Debug)]
struct PptTopLevelLayoutEntry {
  old_offset: u32,
  new_offset: u32,
}

#[derive(Clone, Debug, Default)]
struct PptTopLevelLayout {
  entries: Vec<PptTopLevelLayoutEntry>,
  offsets: BTreeMap<u32, Option<(u32, PptTopLevelRecordKind)>>,
}

impl PptTopLevelLayout {
  fn insert(&mut self, old_offset: u32, new_offset: u32, kind: PptTopLevelRecordKind) {
    self.entries.push(PptTopLevelLayoutEntry {
      old_offset,
      new_offset,
    });
    self
      .offsets
      .entry(old_offset)
      .and_modify(|value| *value = None)
      .or_insert(Some((new_offset, kind)));
  }

  fn positions_changed(&self) -> bool {
    self
      .entries
      .iter()
      .any(|entry| entry.old_offset != entry.new_offset)
  }

  fn relocate(
    &self,
    old_offset: u32,
    expected: Option<PptTopLevelRecordKind>,
    message: &'static str,
  ) -> Result<u32> {
    let (new_offset, kind) = self
      .offsets
      .get(&old_offset)
      .copied()
      .flatten()
      .ok_or_else(|| Error::invalid(u64::from(old_offset), message))?;
    if expected.is_some_and(|expected| expected != kind) {
      return Err(Error::invalid(u64::from(old_offset), message));
    }
    Ok(new_offset)
  }
}

impl PowerPointDocument {
  pub(crate) fn edit_list_text_body<T>(
    &mut self,
    slide_persist_offset: u64,
    text_body_index: usize,
    edit: impl FnOnce(PptLiveTextBodyMut<'_>) -> Result<T>,
  ) -> Result<T> {
    let mut path = Vec::new();
    if !find_ppt_record_path(
      &self.records,
      slide_persist_offset,
      SLIDE_PERSIST_ATOM,
      &mut path,
    ) {
      return Err(Error::invalid(
        slide_persist_offset,
        "live SlidePersistAtom is no longer present in the document tree",
      ));
    }
    let (source_index, parent_path) = path
      .split_last()
      .expect("record path always contains its source index");
    let sequence = ppt_sequence_at_path_mut(&mut self.records, parent_path)?;
    let group_end = sequence.records[*source_index + 1..]
      .iter()
      .position(|record| record.header.record_type == SLIDE_PERSIST_ATOM)
      .map_or(sequence.records.len(), |relative| {
        *source_index + 1 + relative
      });
    let start = sequence.records[*source_index + 1..group_end]
      .iter()
      .enumerate()
      .filter(|(_, record)| matches!(record.data, PptRecordData::TextHeader(_)))
      .map(|(relative, _)| *source_index + 1 + relative)
      .nth(text_body_index)
      .ok_or_else(|| {
        Error::invalid(
          slide_persist_offset,
          format!("SlidePersistAtom has no text body {text_body_index}"),
        )
      })?;
    let end = sequence.records[start + 1..group_end]
      .iter()
      .position(|record| matches!(record.data, PptRecordData::TextHeader(_)))
      .map_or(group_end, |relative| start + 1 + relative);
    edit(PptLiveTextBodyMut {
      records: &mut sequence.records[start..end],
    })
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_bytes_with_limits(bytes, Limits::default())
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    if bytes.len() as u64 > limits.max_stream_size {
      return Err(Error::Limit(format!(
        "PowerPoint Document stream length {} exceeds {}",
        bytes.len(),
        limits.max_stream_size
      )));
    }
    if bytes.len() > limits.max_allocation {
      return Err(Error::Limit(format!(
        "PowerPoint Document allocation {} exceeds {}",
        bytes.len(),
        limits.max_allocation
      )));
    }
    let mut record_count = 0usize;
    Ok(Self {
      records: PptRecordSequence::parse(bytes, 0, 0, limits, &mut record_count)?,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.records.to_bytes()
  }

  pub(crate) fn serialized_len(&self) -> Result<usize> {
    self.records.serialized_len_from_layout()
  }

  pub(crate) fn write_to(&self, writer: &mut dyn Write) -> Result<()> {
    self.records.write_to(writer)
  }

  /// Aggregates the OfficeArt Dgg/Dg atoms reachable from the current PPT
  /// live presentation into one document-level drawing graph.
  pub fn live_drawing_graph(
    &self,
    current_user: &CurrentUserAtom,
  ) -> Result<OfficeArtDrawingGraph> {
    let presentation = self.live_presentation(current_user)?;
    let mut drawing_groups = Vec::new();
    let mut blip_stores = Vec::new();
    let mut drawing_group_blip_references = Vec::new();
    let mut drawing_group_incomplete_property_tables = Vec::new();
    let mut drawings = Vec::new();
    for state in &presentation.top_level_records {
      if !matches!(
        state.status,
        PptTopLevelLiveRecordStatus::LivePersistObject { .. }
      ) {
        continue;
      }
      let record = self
        .records
        .records
        .get(state.record_index)
        .ok_or_else(|| {
          Error::invalid(
            state.stream_offset,
            "PPT live record index is out of bounds",
          )
        })?;
      collect_ppt_office_art_drawing_components(
        record,
        &mut drawing_groups,
        &mut blip_stores,
        &mut drawing_group_blip_references,
        &mut drawing_group_incomplete_property_tables,
        &mut drawings,
      )?;
    }
    let [drawing_group] = drawing_groups.as_slice() else {
      return Err(Error::invalid(
        0,
        format!(
          "PPT live presentation contains {} OfficeArtFDGGBlock records, expected 1",
          drawing_groups.len()
        ),
      ));
    };
    OfficeArtDrawingGraph::from_components_with_blips(
      drawing_group.clone(),
      blip_stores,
      drawing_group_blip_references,
      drawing_group_incomplete_property_tables,
      drawings,
    )
  }

  /// Resolves the live document-wide OfficeArt image store without copying
  /// any image payload. One-based BLIP identifiers remain stable and can be
  /// joined directly to [`PptLiveShapeRef::primary_blip_identifier`].
  pub fn live_image_store<'a>(
    &'a self,
    current_user: &CurrentUserAtom,
    pictures: Option<&'a PicturesStream>,
  ) -> Result<PptLiveImageStore<'a>> {
    let presentation = self.live_presentation(current_user)?;
    let mut stores = Vec::new();
    for state in &presentation.top_level_records {
      if !matches!(
        state.status,
        PptTopLevelLiveRecordStatus::LivePersistObject { .. }
      ) {
        continue;
      }
      let record = self
        .records
        .records
        .get(state.record_index)
        .ok_or_else(|| {
          Error::invalid(
            state.stream_offset,
            "PPT live record index is out of bounds",
          )
        })?;
      collect_ppt_office_art_blip_stores(record, &mut stores);
    }
    let store = match stores.as_slice() {
      [] => {
        return Ok(PptLiveImageStore {
          entries: Vec::new(),
        });
      }
      [store] => *store,
      _ => {
        return Err(Error::invalid(
          0,
          format!(
            "PPT live presentation contains {} OfficeArtBStoreContainer records, expected at most 1",
            stores.len()
          ),
        ));
      }
    };
    let mut entries = Vec::with_capacity(store.len());
    for (index, entry) in store.iter().enumerate() {
      let blip_identifier = u32::try_from(index + 1)
        .map_err(|_| Error::Limit("PPT BLIP store index exceeds u32".into()))?;
      let PptRecordData::OfficeArt(office_art) = &entry.data else {
        entries.push(PptLiveImageLink::Unresolved {
          blip_identifier,
          issue: PptImageResolutionIssue::UnsupportedBlipStoreEntry,
        });
        continue;
      };
      let resolved = match &office_art.data {
        OfficeArtRecordData::Fbse(fbse) => {
          if let Some(blip) = fbse.embedded_blip.as_deref() {
            blip.image_ref().map(|image| PptLiveImageRef {
              blip_identifier,
              source: PptLiveImageSource::Embedded { store_index: index },
              image,
            })
          } else {
            resolve_delay_image(pictures, fbse.delay_offset, blip_identifier)?.map(
              |(record_index, image, compatible_stream)| PptLiveImageRef {
                blip_identifier,
                source: PptLiveImageSource::PicturesStream {
                  record_index,
                  offset: fbse.delay_offset,
                  compatible_stream,
                },
                image,
              },
            )
          }
        }
        _ => office_art.image_ref().map(|image| PptLiveImageRef {
          blip_identifier,
          source: PptLiveImageSource::DirectBlipStoreEntry { store_index: index },
          image,
        }),
      };
      if let Some(value) = resolved {
        entries.push(PptLiveImageLink::Resolved(value));
        continue;
      }
      let issue = match &office_art.data {
        OfficeArtRecordData::Fbse(fbse) if fbse.embedded_blip.is_none() => match pictures {
          None => PptImageResolutionIssue::MissingPicturesStream,
          Some(PicturesStream::Partial(_)) => PptImageResolutionIssue::PartialPicturesStream,
          Some(PicturesStream::Complete(_) | PicturesStream::Compatibility { .. }) => {
            PptImageResolutionIssue::MissingDelayOffset
          }
        },
        OfficeArtRecordData::Fbse(fbse) if fbse.embedded_blip.is_some() => {
          PptImageResolutionIssue::UnsupportedPayload
        }
        OfficeArtRecordData::Fbse(_) => unreachable!("FBSE cases are exhaustive"),
        _ if office_art.image_ref().is_none() => PptImageResolutionIssue::UnsupportedBlipStoreEntry,
        _ => PptImageResolutionIssue::UnsupportedPayload,
      };
      entries.push(PptLiveImageLink::Unresolved {
        blip_identifier,
        issue,
      });
    }
    Ok(PptLiveImageStore { entries })
  }

  /// Rebuilds record lengths and offsets, then relocates the complete
  /// MS-PPT incremental-save reference graph. The update is transactional.
  pub fn relayout(&mut self, current_user: &mut CurrentUserAtom) -> Result<()> {
    self.relayout_with_policy(current_user, false)
  }

  pub(crate) fn relayout_with_policy(
    &mut self,
    current_user: &mut CurrentUserAtom,
    preserve_compatibility: bool,
  ) -> Result<()> {
    let mut document = self.clone();
    let mut current_user_atom = current_user.clone();
    document.relayout_in_place(&mut current_user_atom, preserve_compatibility)?;
    *self = document;
    *current_user = current_user_atom;
    Ok(())
  }

  /// Replaces the physical history with one user edit containing only the
  /// persist objects that are live under MS-PPT 2.1.2 Parts 2 through 11.
  /// Dead records, superseded definitions, and unreferenced records are
  /// deliberately discarded. The update is transactional.
  pub fn rebuild_current_live_state(&mut self, current_user: &mut CurrentUserAtom) -> Result<()> {
    let mut document = self.clone();
    let mut current_user_atom = current_user.clone();
    document.rebuild_current_live_state_in_place(&mut current_user_atom)?;
    *self = document;
    *current_user = current_user_atom;
    Ok(())
  }

  fn rebuild_current_live_state_in_place(
    &mut self,
    current_user: &mut CurrentUserAtom,
  ) -> Result<()> {
    let presentation = self.live_presentation(current_user)?;
    let current_edit = presentation
      .persist_object_directory
      .incremental_save_chain
      .edits
      .first()
      .ok_or_else(|| Error::invalid(0, "PPT incremental-save chain is empty"))?;
    if current_edit
      .user_edit
      .encrypt_session_persist_id_ref
      .is_some()
    {
      return Err(Error::invalid(
        u64::from(current_edit.user_edit_offset),
        "current-live-state rebuild does not support encrypted presentations",
      ));
    }

    let mut live_offsets_by_id = BTreeMap::new();
    let mut records = Vec::new();
    for state in &presentation.top_level_records {
      let PptTopLevelLiveRecordStatus::LivePersistObject { persist_ids, .. } = &state.status else {
        continue;
      };
      let record = self
        .records
        .records
        .get(state.record_index)
        .ok_or_else(|| Error::invalid(state.stream_offset, "live record index is invalid"))?
        .clone();
      let old_offset = u32::try_from(record.offset)
        .map_err(|_| Error::Limit("PPT source record offset exceeds u32".into()))?;
      for persist_id in persist_ids {
        if live_offsets_by_id.insert(*persist_id, old_offset).is_some() {
          return Err(Error::invalid(
            state.stream_offset,
            "live persist object identifier occurs more than once",
          ));
        }
      }
      records.push(record);
    }
    if live_offsets_by_id.is_empty() {
      return Err(Error::invalid(
        u64::from(current_edit.user_edit_offset),
        "PPT live presentation has no persist objects",
      ));
    }

    let persist_directory_offset = u32::MAX - 1;
    let user_edit_offset = u32::MAX;
    let persist_directory = PersistDirectoryAtom {
      entries: persist_directory_entries(&live_offsets_by_id)?,
    };
    records.push(PptRecord {
      offset: u64::from(persist_directory_offset),
      header: PptRecordHeader {
        version: 0,
        instance: 0,
        record_type: PERSIST_DIRECTORY_ATOM,
        declared_length: 0,
      },
      data: PptRecordData::PersistDirectory(persist_directory),
    });

    let max_persist_id = *live_offsets_by_id
      .last_key_value()
      .expect("nonempty live persist map")
      .0;
    let mut user_edit = current_edit.user_edit;
    user_edit.offset_last_edit = 0;
    user_edit.offset_persist_directory = persist_directory_offset;
    user_edit.persist_id_seed = user_edit.persist_id_seed.max(max_persist_id);
    user_edit.encrypt_session_persist_id_ref = None;
    records.push(PptRecord {
      offset: u64::from(user_edit_offset),
      header: PptRecordHeader {
        version: 0,
        instance: 0,
        record_type: USER_EDIT_ATOM,
        declared_length: 0,
      },
      data: PptRecordData::UserEdit(user_edit),
    });

    self.records = PptRecordSequence {
      records,
      trailing_header_bytes: Vec::new(),
    };
    current_user.offset_to_current_edit = user_edit_offset;
    self.relayout_in_place(current_user, false)?;
    self.live_presentation(current_user)?;
    Ok(())
  }

  /// Appends a full current persist-object checkpoint while restoring the
  /// preceding checkpoint from the source document. The caller performs
  /// the final cross-stream layout transaction.
  pub(crate) fn append_user_edit_from_baseline(
    &mut self,
    current_user: &mut CurrentUserAtom,
    baseline: &PowerPointDocument,
    baseline_current_user: &CurrentUserAtom,
    source_pictures_layout: Option<&OfficeArtBStoreDelayLayout>,
  ) -> Result<Vec<u32>> {
    let presentation = self.live_presentation(current_user)?;
    let baseline_directory = baseline.persist_object_directory(baseline_current_user)?;
    let current_edit = presentation
      .persist_object_directory
      .incremental_save_chain
      .edits
      .first()
      .ok_or_else(|| Error::invalid(0, "PPT incremental-save chain is empty"))?
      .clone();
    let current_references = presentation
      .persist_object_directory
      .current_references
      .clone();
    drop(presentation);
    if current_edit
      .user_edit
      .encrypt_session_persist_id_ref
      .is_some()
    {
      return Err(Error::invalid(
        u64::from(current_edit.user_edit_offset),
        "append-user-edit does not support encrypted presentations",
      ));
    }

    let mut current_ids_by_record = BTreeMap::<usize, Vec<u32>>::new();
    for (&persist_id, reference) in &current_references {
      current_ids_by_record
        .entry(reference.record_index)
        .or_default()
        .push(persist_id);
    }
    let checkpoint_records = current_ids_by_record.into_iter().collect::<Vec<_>>();
    if checkpoint_records.is_empty() {
      return Err(Error::invalid(
        u64::from(current_edit.user_edit_offset),
        "PPT current persist object directory is empty",
      ));
    }

    let synthetic_count = checkpoint_records
      .len()
      .checked_add(2)
      .ok_or_else(|| Error::Limit("PPT appended user-edit record count overflow".into()))?;
    let synthetic_count = u32::try_from(synthetic_count)
      .map_err(|_| Error::Limit("PPT appended user-edit record count exceeds u32".into()))?;
    let synthetic_start = u32::MAX
      .checked_sub(synthetic_count - 1)
      .ok_or_else(|| Error::Limit("PPT synthetic record offset underflow".into()))?;
    if synthetic_start <= current_edit.user_edit_offset
      || self
        .records
        .records
        .iter()
        .any(|record| record.offset >= u64::from(synthetic_start))
    {
      return Err(Error::Limit(
        "PowerPoint Document stream has no synthetic offset space for an appended user edit".into(),
      ));
    }

    let mut appended_records = Vec::with_capacity(checkpoint_records.len());
    let mut offsets_by_id = BTreeMap::new();
    let mut appended_ids = Vec::new();
    for (append_index, (record_index, persist_ids)) in checkpoint_records.into_iter().enumerate() {
      let edited_record = self
        .records
        .records
        .get(record_index)
        .ok_or_else(|| Error::invalid(0, "live persist record index is invalid"))?
        .clone();
      let first_persist_id = *persist_ids.first().ok_or_else(|| {
        Error::invalid(
          edited_record.offset,
          "current persist record has no persist ID",
        )
      })?;
      let baseline_reference = baseline_directory
        .current_reference(first_persist_id)
        .ok_or_else(|| {
          Error::invalid(
            edited_record.offset,
            "current persist ID does not exist in the source checkpoint",
          )
        })?;
      for persist_id in &persist_ids {
        let reference = baseline_directory
          .current_reference(*persist_id)
          .ok_or_else(|| {
            Error::invalid(
              edited_record.offset,
              "current persist ID does not exist in the source checkpoint",
            )
          })?;
        if reference.record_index != baseline_reference.record_index {
          return Err(Error::invalid(
            edited_record.offset,
            "aliased current persist IDs do not share one source persist object",
          ));
        }
      }

      let mut restored_record = baseline
        .records
        .records
        .get(baseline_reference.record_index)
        .ok_or_else(|| Error::invalid(edited_record.offset, "source record is missing"))?
        .clone();
      restored_record.offset = edited_record.offset;
      if let Some(layout) = source_pictures_layout {
        let mut relocated = 0;
        relocate_ppt_record_picture_references(
          &mut restored_record,
          Some(layout),
          false,
          &mut relocated,
        )?;
      }
      self.records.records[record_index] = restored_record;

      let append_index = u32::try_from(append_index)
        .map_err(|_| Error::Limit("PPT append index exceeds u32".into()))?;
      let synthetic_offset = synthetic_start
        .checked_add(append_index)
        .ok_or_else(|| Error::Limit("PPT synthetic record offset overflow".into()))?;
      let mut appended_record = edited_record;
      appended_record.offset = u64::from(synthetic_offset);
      for persist_id in persist_ids {
        if offsets_by_id.insert(persist_id, synthetic_offset).is_some() {
          return Err(Error::invalid(
            u64::from(synthetic_offset),
            "current persist ID occurs in more than one appended record",
          ));
        }
        appended_ids.push(persist_id);
      }
      appended_records.push(appended_record);
    }
    self.records.records.extend(appended_records);

    let persist_directory_offset = synthetic_start
      .checked_add(synthetic_count - 2)
      .ok_or_else(|| Error::Limit("PPT synthetic persist-directory offset overflow".into()))?;
    self.records.records.push(PptRecord {
      offset: u64::from(persist_directory_offset),
      header: PptRecordHeader {
        version: 0,
        instance: 0,
        record_type: PERSIST_DIRECTORY_ATOM,
        declared_length: 0,
      },
      data: PptRecordData::PersistDirectory(PersistDirectoryAtom {
        entries: persist_directory_entries(&offsets_by_id)?,
      }),
    });

    let user_edit_offset = persist_directory_offset
      .checked_add(1)
      .ok_or_else(|| Error::Limit("PPT synthetic user-edit offset overflow".into()))?;
    let max_persist_id = *current_references
      .last_key_value()
      .ok_or_else(|| Error::invalid(0, "PPT persist object directory is empty"))?
      .0;
    let mut user_edit = current_edit.user_edit;
    user_edit.offset_last_edit = current_edit.user_edit_offset;
    user_edit.offset_persist_directory = persist_directory_offset;
    user_edit.persist_id_seed = user_edit.persist_id_seed.max(max_persist_id);
    user_edit.encrypt_session_persist_id_ref = None;
    self.records.records.push(PptRecord {
      offset: u64::from(user_edit_offset),
      header: PptRecordHeader {
        version: 0,
        instance: 0,
        record_type: USER_EDIT_ATOM,
        declared_length: 0,
      },
      data: PptRecordData::UserEdit(user_edit),
    });
    current_user.offset_to_current_edit = user_edit_offset;
    appended_ids.sort_unstable();
    Ok(appended_ids)
  }

  pub(crate) fn relayout_in_place(
    &mut self,
    current_user: &mut CurrentUserAtom,
    preserve_compatibility: bool,
  ) -> Result<()> {
    let mut layout = PptTopLevelLayout::default();
    self
      .records
      .relayout(0, Some(&mut layout), preserve_compatibility)?;
    if layout.positions_changed() {
      for record in &mut self.records.records {
        match &mut record.data {
          PptRecordData::UserEdit(value) => {
            if value.offset_last_edit != 0 {
              value.offset_last_edit = layout.relocate(
                value.offset_last_edit,
                Some(PptTopLevelRecordKind::UserEdit),
                "UserEditAtom.offsetLastEdit does not reference UserEditAtom",
              )?;
            }
            value.offset_persist_directory = layout.relocate(
              value.offset_persist_directory,
              Some(PptTopLevelRecordKind::PersistDirectory),
              "UserEditAtom.offsetPersistDirectory does not reference PersistDirectoryAtom",
            )?;
          }
          PptRecordData::PersistDirectory(value) => {
            for entry in &mut value.entries {
              for offset in &mut entry.stream_offsets {
                *offset = layout.relocate(
                  *offset,
                  None,
                  "PersistDirectoryAtom offset does not reference a top-level record",
                )?;
              }
            }
          }
          _ => {}
        }
      }
      current_user.offset_to_current_edit = layout.relocate(
        current_user.offset_to_current_edit,
        Some(PptTopLevelRecordKind::UserEdit),
        "CurrentUserAtom.offsetToCurrentEdit does not reference UserEditAtom",
      )?;
      self.incremental_save_chain(current_user)?;
    }
    Ok(())
  }

  pub fn incremental_save_chain(
    &self,
    current_user: &CurrentUserAtom,
  ) -> Result<IncrementalSaveChain> {
    let mut edits = Vec::new();
    let mut seen_user_edits = BTreeSet::new();
    let mut offset = current_user.offset_to_current_edit;
    loop {
      if !seen_user_edits.insert(offset) {
        return Err(Error::invalid(
          u64::from(offset),
          "PPT UserEditAtom chain contains a cycle",
        ));
      }
      let record = self.top_level_record(offset).ok_or_else(|| {
        Error::invalid(u64::from(offset), "PPT UserEditAtom offset is not a record")
      })?;
      let PptRecordData::UserEdit(user_edit) = &record.data else {
        return Err(Error::invalid(
          u64::from(offset),
          "PPT current-edit offset does not reference UserEditAtom",
        ));
      };
      if user_edit.offset_last_edit != 0 && user_edit.offset_last_edit >= offset {
        return Err(Error::invalid(
          u64::from(user_edit.offset_last_edit),
          "PPT previous UserEditAtom offset is not before the current UserEditAtom",
        ));
      }
      let persist_offset = user_edit.offset_persist_directory;
      if persist_offset <= user_edit.offset_last_edit || persist_offset >= offset {
        return Err(Error::invalid(
          u64::from(persist_offset),
          "PPT PersistDirectoryAtom offset is not between the previous and current UserEditAtom offsets",
        ));
      }
      let persist_record = self.top_level_record(persist_offset).ok_or_else(|| {
        Error::invalid(
          u64::from(persist_offset),
          "PPT persist-directory offset is not a record",
        )
      })?;
      let PptRecordData::PersistDirectory(persist_directory) = &persist_record.data else {
        return Err(Error::invalid(
          u64::from(persist_offset),
          "PPT offset does not reference PersistDirectoryAtom",
        ));
      };
      edits.push(IncrementalSaveEdit {
        user_edit_offset: offset,
        user_edit: *user_edit,
        persist_directory_offset: persist_offset,
        persist_directory: persist_directory.clone(),
      });
      if user_edit.offset_last_edit == 0 {
        break;
      }
      offset = user_edit.offset_last_edit;
    }

    let mut persist_object_offsets = BTreeMap::new();
    for edit in edits.iter().rev() {
      for entry in &edit.persist_directory.entries {
        for (index, stream_offset) in entry.stream_offsets.iter().enumerate() {
          let persist_id = entry
            .first_persist_id
            .checked_add(
              u32::try_from(index)
                .map_err(|_| Error::Limit("PPT persist-directory index exceeds u32".into()))?,
            )
            .ok_or_else(|| {
              Error::invalid(
                u64::from(edit.persist_directory_offset),
                "PPT persist object identifier overflow",
              )
            })?;
          persist_object_offsets.insert(persist_id, *stream_offset);
        }
      }
    }
    Ok(IncrementalSaveChain {
      edits,
      persist_object_offsets,
    })
  }

  /// Constructs and validates the MS-PPT 2.1.2 Part 1 persist object
  /// directory without conflating directory membership with presentation
  /// liveness.
  pub fn persist_object_directory(
    &self,
    current_user: &CurrentUserAtom,
  ) -> Result<PersistObjectDirectory> {
    let chain = self.incremental_save_chain(current_user)?;
    let mut record_index_by_offset = BTreeMap::new();
    for (record_index, record) in self.records.records.iter().enumerate() {
      if record_index_by_offset
        .insert(record.offset, record_index)
        .is_some()
      {
        return Err(Error::invalid(
          record.offset,
          "duplicate PPT top-level record offset",
        ));
      }
    }

    let mut metadata = vec![None; self.records.records.len()];
    for edit in &chain.edits {
      let user_edit_index = *record_index_by_offset
        .get(&u64::from(edit.user_edit_offset))
        .ok_or_else(|| {
          Error::invalid(
            u64::from(edit.user_edit_offset),
            "PPT user edit is not a top-level record",
          )
        })?;
      set_incremental_save_metadata(
        &mut metadata,
        user_edit_index,
        IncrementalSaveMetadataKind::UserEditAtom,
        edit.user_edit_offset,
      )?;

      let persist_directory_index = *record_index_by_offset
        .get(&u64::from(edit.persist_directory_offset))
        .ok_or_else(|| {
          Error::invalid(
            u64::from(edit.persist_directory_offset),
            "PPT persist directory is not a top-level record",
          )
        })?;
      set_incremental_save_metadata(
        &mut metadata,
        persist_directory_index,
        IncrementalSaveMetadataKind::PersistDirectoryAtom,
        edit.persist_directory_offset,
      )?;
    }

    let mut references = Vec::new();
    let mut current_references = BTreeMap::new();
    let mut seen_persist_ids = BTreeSet::new();
    let mut current_ids_by_record = vec![Vec::new(); self.records.records.len()];
    let mut superseded_ids_by_record = vec![Vec::new(); self.records.records.len()];
    for edit in &chain.edits {
      let mut edit_persist_ids = BTreeSet::new();
      for entry in &edit.persist_directory.entries {
        if entry.stream_offsets.is_empty() {
          return Err(Error::invalid(
            u64::from(edit.persist_directory_offset),
            "PersistDirectoryEntry.cPersist must be at least 1",
          ));
        }
        if entry.stream_offsets.len() > 0x0fff {
          return Err(Error::invalid(
            u64::from(edit.persist_directory_offset),
            "PersistDirectoryEntry.cPersist exceeds 12 bits",
          ));
        }
        if entry.first_persist_id > 0x000f_fffe {
          return Err(Error::invalid(
            u64::from(edit.persist_directory_offset),
            "PersistDirectoryEntry.persistId exceeds 0xFFFFE",
          ));
        }

        for (entry_index, stream_offset) in entry.stream_offsets.iter().copied().enumerate() {
          let persist_id = entry
            .first_persist_id
            .checked_add(
              u32::try_from(entry_index)
                .map_err(|_| Error::Limit("PPT persist-directory index exceeds u32".into()))?,
            )
            .ok_or_else(|| {
              Error::invalid(
                u64::from(edit.persist_directory_offset),
                "PPT persist object identifier overflow",
              )
            })?;
          if !edit_persist_ids.insert(persist_id) {
            return Err(Error::invalid(
              u64::from(edit.persist_directory_offset),
              "duplicate persist object identifier in PersistDirectoryAtom",
            ));
          }
          if stream_offset < edit.user_edit.offset_last_edit
            || stream_offset >= edit.persist_directory_offset
          {
            return Err(Error::invalid(
              u64::from(stream_offset),
              "PersistOffsetEntry is outside its corresponding user edit",
            ));
          }
          let record_index = *record_index_by_offset
            .get(&u64::from(stream_offset))
            .ok_or_else(|| {
              Error::invalid(
                u64::from(stream_offset),
                "PersistOffsetEntry does not reference a top-level record",
              )
            })?;
          if metadata[record_index].is_some() {
            return Err(Error::invalid(
              u64::from(stream_offset),
              "PersistOffsetEntry references incremental-save metadata",
            ));
          }
          let record = &self.records.records[record_index];
          if !is_persist_object_record(record) {
            return Err(Error::invalid(
              u64::from(stream_offset),
              "PersistOffsetEntry does not reference an MS-PPT persist object",
            ));
          }

          let status = if seen_persist_ids.insert(persist_id) {
            PersistObjectReferenceStatus::Current
          } else {
            PersistObjectReferenceStatus::Superseded
          };
          let reference = PersistObjectReference {
            persist_id,
            stream_offset,
            record_index,
            user_edit_offset: edit.user_edit_offset,
            persist_directory_offset: edit.persist_directory_offset,
            status,
          };
          match status {
            PersistObjectReferenceStatus::Current => {
              current_ids_by_record[record_index].push(persist_id);
              current_references.insert(persist_id, reference);
            }
            PersistObjectReferenceStatus::Superseded => {
              superseded_ids_by_record[record_index].push(persist_id);
            }
          }
          references.push(reference);
        }
      }
    }

    let top_level_records = self
      .records
      .records
      .iter()
      .enumerate()
      .map(|(record_index, record)| {
        current_ids_by_record[record_index].sort_unstable();
        superseded_ids_by_record[record_index].sort_unstable();
        let role = if let Some(kind) = metadata[record_index] {
          PptTopLevelRecordRole::IncrementalSaveMetadata(kind)
        } else if !current_ids_by_record[record_index].is_empty()
          || !superseded_ids_by_record[record_index].is_empty()
        {
          PptTopLevelRecordRole::PersistObject {
            current_persist_ids: std::mem::take(&mut current_ids_by_record[record_index]),
            superseded_persist_ids: std::mem::take(&mut superseded_ids_by_record[record_index]),
          }
        } else {
          PptTopLevelRecordRole::Unreferenced
        };
        PptTopLevelRecordState {
          record_index,
          stream_offset: record.offset,
          record_type: record.header.record_type,
          role,
        }
      })
      .collect();

    Ok(PersistObjectDirectory {
      incremental_save_chain: chain,
      references,
      current_references,
      top_level_records,
    })
  }

  /// Resolves the live top-level records by executing MS-PPT 2.1.2 Parts 1
  /// through 11 against the current `DocumentContainer`.
  pub fn live_presentation(
    &self,
    current_user: &CurrentUserAtom,
  ) -> Result<PptLivePresentation<'_>> {
    let mut diagnostics = Vec::new();
    self.live_presentation_with_policy(current_user, true, &mut diagnostics)
  }

  /// Resolves the live presentation while retaining explicitly diagnosed
  /// producer compatibility shapes.
  pub fn live_presentation_compatible(
    &self,
    current_user: &CurrentUserAtom,
  ) -> Result<ParseOutcome<PptLivePresentation<'_>>> {
    let mut diagnostics = Vec::new();
    let value = self.live_presentation_with_policy(current_user, false, &mut diagnostics)?;
    Ok(ParseOutcome::new(value, diagnostics))
  }

  fn live_presentation_with_policy(
    &self,
    current_user: &CurrentUserAtom,
    strict: bool,
    diagnostics: &mut Vec<ParseDiagnostic>,
  ) -> Result<PptLivePresentation<'_>> {
    let persist_object_directory = self.persist_object_directory(current_user)?;
    let current_edit = persist_object_directory
      .incremental_save_chain
      .edits
      .first()
      .ok_or_else(|| Error::invalid(0, "PPT incremental-save chain is empty"))?;

    let (document_reference, document_record) = self.resolve_live_persist_object(
      &persist_object_directory,
      current_edit.user_edit.doc_persist_id_ref,
      u64::from(current_edit.user_edit_offset),
      &[DOCUMENT_CONTAINER],
      "docPersistIdRef does not resolve to DocumentContainer",
    )?;
    let current_edit_record = self
      .top_level_record(current_edit.user_edit_offset)
      .ok_or_else(|| {
        Error::invalid(
          u64::from(current_edit.user_edit_offset),
          "current UserEditAtom offset is not a top-level record",
        )
      })?;
    let document = PptLivePersistObject {
      reference: document_reference,
      role: PptLivePersistObjectRole::Document,
      source_record: current_edit_record,
      record: document_record,
      list_records: &[],
    };
    let document_children = ppt_container_children(document_record, "DocumentContainer")?;
    let document_atom_record = required_direct_record(
      document_children,
      DOCUMENT_ATOM,
      Some(0),
      "DocumentContainer.documentAtom",
    )?;
    require_record_version(document_atom_record, 1, "DocumentAtom")?;
    let PptRecordData::Document(document_atom) = &document_atom_record.data else {
      return Err(Error::invalid(
        document_atom_record.offset,
        "DocumentContainer.documentAtom is not a conforming DocumentAtom",
      ));
    };

    let notes_master_slide = optional_live_object(
      self,
      &persist_object_directory,
      document_atom.notes_master_persist_id_ref,
      document_atom_record,
      &[NOTES_CONTAINER],
      PptLivePersistObjectRole::NotesMasterSlide,
      "notesMasterPersistIdRef does not resolve to NotesContainer",
    )?;
    let handout_master_slide = optional_live_object(
      self,
      &persist_object_directory,
      document_atom.handout_master_persist_id_ref,
      document_atom_record,
      &[HANDOUT_CONTAINER],
      PptLivePersistObjectRole::HandoutMasterSlide,
      "handoutMasterPersistIdRef does not resolve to HandoutContainer",
    )?;

    let master_list_record = optional_direct_record(
      document_children,
      SLIDE_LIST_WITH_TEXT_CONTAINER,
      Some(1),
      "DocumentContainer.masterList",
    )?;
    let mut master_slides = Vec::new();
    if let Some(master_list_record) = master_list_record {
      let master_list = ppt_container_children(master_list_record, "MasterListWithTextContainer")?;
      for source in direct_slide_persist_atoms(master_list, "MasterListWithTextContainer")? {
        let resolved = self.resolve_live_persist_object(
          &persist_object_directory,
          source.1.persist_id_ref,
          source.0.offset,
          &[MAIN_MASTER_CONTAINER, SLIDE_CONTAINER],
          "MasterPersistAtom.persistIdRef does not resolve to MasterOrSlideContainer",
        );
        let (reference, target) = match resolved {
          Ok(value) => value,
          Err(error) if !strict => {
            push_live_presentation_diagnostic(
              diagnostics,
              ParseDiagnosticCode::InvalidReference,
              source.0.offset,
              "MasterPersistAtom.persistIdRef",
              "2.4.14.2",
              error.to_string(),
            );
            continue;
          }
          Err(error) => return Err(error),
        };
        let role = match target.header.record_type {
          MAIN_MASTER_CONTAINER => PptLivePersistObjectRole::MainMasterSlide,
          SLIDE_CONTAINER => PptLivePersistObjectRole::TitleMasterSlide,
          _ => unreachable!("target record type was checked"),
        };
        master_slides.push(PptLivePersistObject {
          reference,
          role,
          source_record: source.0,
          record: target,
          list_records: source.2,
        });
      }
    } else if strict {
      return Err(Error::invalid(
        document_record.offset,
        "required DocumentContainer.masterList is missing",
      ));
    } else {
      push_live_presentation_diagnostic(
        diagnostics,
        ParseDiagnosticCode::NonconformingRecord,
        document_record.offset,
        "DocumentContainer.masterList",
        "2.4.1",
        "required DocumentContainer.masterList is missing; treated as an empty master list",
      );
    }

    let presentation_slides = resolve_list_persist_objects(
      self,
      &persist_object_directory,
      document_children,
      ListPersistObjectSpec {
        list_instance: 0,
        target_record_type: SLIDE_CONTAINER,
        role: PptLivePersistObjectRole::PresentationSlide,
        list_name: "SlideListWithTextContainer",
        source_field: "SlidePersistAtom.persistIdRef",
        specification_section: "2.4.14.5",
        error_message: "SlidePersistAtom.persistIdRef does not resolve to SlideContainer",
      },
      strict,
      diagnostics,
    )?;
    let notes_slides = resolve_list_persist_objects(
      self,
      &persist_object_directory,
      document_children,
      ListPersistObjectSpec {
        list_instance: 2,
        target_record_type: NOTES_CONTAINER,
        role: PptLivePersistObjectRole::NotesSlide,
        list_name: "NotesListWithTextContainer",
        source_field: "NotesPersistAtom.persistIdRef",
        specification_section: "2.4.14.7",
        error_message: "NotesPersistAtom.persistIdRef does not resolve to NotesContainer",
      },
      strict,
      diagnostics,
    )?;

    let mut active_x_controls = Vec::new();
    let mut embedded_ole_objects = Vec::new();
    let mut linked_ole_objects = Vec::new();
    if let Some(external_object_list_record) = optional_direct_record(
      document_children,
      EXTERNAL_OBJECT_LIST_CONTAINER,
      Some(0),
      "DocumentContainer.exObjList",
    )? {
      let external_object_list =
        ppt_container_children(external_object_list_record, "ExObjListContainer")?;
      resolve_external_persist_objects(
        self,
        &persist_object_directory,
        external_object_list,
        EXTERNAL_OLE_CONTROL_CONTAINER,
        PptLivePersistObjectRole::ActiveXControl,
        "ExControlContainer",
        &mut active_x_controls,
      )?;
      resolve_external_persist_objects(
        self,
        &persist_object_directory,
        external_object_list,
        EXTERNAL_OLE_EMBED_CONTAINER,
        PptLivePersistObjectRole::EmbeddedOleObject,
        "ExOleEmbedContainer",
        &mut embedded_ole_objects,
      )?;
      resolve_external_persist_objects(
        self,
        &persist_object_directory,
        external_object_list,
        EXTERNAL_OLE_LINK_CONTAINER,
        PptLivePersistObjectRole::LinkedOleObject,
        "ExOleLinkContainer",
        &mut linked_ole_objects,
      )?;
    }

    let vba_project = if let Some(document_info_list_record) = optional_direct_record(
      document_children,
      DOCUMENT_INFO_LIST_CONTAINER,
      Some(0),
      "DocumentContainer.docInfoList",
    )? {
      let document_info_list =
        ppt_container_children(document_info_list_record, "DocInfoListContainer")?;
      if let Some(vba_info_record) = optional_direct_record(
        document_info_list,
        VBA_INFO_CONTAINER,
        Some(1),
        "DocInfoListContainer.vbaInfo",
      )? {
        let vba_info = ppt_container_children(vba_info_record, "VBAInfoContainer")?;
        let vba_info_atom_record = required_direct_record(
          vba_info,
          VBA_INFO_ATOM,
          Some(0),
          "VBAInfoContainer.vbaInfoAtom",
        )?;
        require_record_version(vba_info_atom_record, 2, "VBAInfoAtom")?;
        let PptRecordData::VbaInfo(vba_info_atom) = &vba_info_atom_record.data else {
          return Err(Error::invalid(
            vba_info_atom_record.offset,
            "VBAInfoContainer.vbaInfoAtom is not a conforming VBAInfoAtom",
          ));
        };
        if vba_info_atom.has_macros > 1 || vba_info_atom.version != 2 {
          let message = format!(
            "VBAInfoAtom fHasMacros {} or version {} violates MS-PPT 2.4.11",
            vba_info_atom.has_macros, vba_info_atom.version
          );
          if strict {
            return Err(Error::invalid(vba_info_atom_record.offset, message));
          }
          diagnostics.push(ParseDiagnostic::warning(
            ParseDiagnosticCode::NonconformingRecord,
            BinaryFormat::Ppt,
            Some(POWERPOINT_DOCUMENT_STREAM_PATH),
            Some(vba_info_atom_record.offset),
            "VBAInfoAtom",
            SpecificationReference {
              document: "MS-PPT",
              section: "2.4.11",
            },
            message,
          ));
        }
        if vba_info_atom.persist_id_ref == 0 {
          if vba_info_atom.has_macros != 0 {
            let message = "nonempty VBAInfoAtom has a null persistIdRef";
            if strict {
              return Err(Error::invalid(vba_info_atom_record.offset, message));
            }
            diagnostics.push(ParseDiagnostic::warning(
              ParseDiagnosticCode::InvalidReference,
              BinaryFormat::Ppt,
              Some(POWERPOINT_DOCUMENT_STREAM_PATH),
              Some(vba_info_atom_record.offset),
              "VBAInfoAtom.persistIdRef",
              SpecificationReference {
                document: "MS-PPT",
                section: "2.4.11",
              },
              message,
            ));
          }
          None
        } else {
          let (reference, record) = self.resolve_live_persist_object(
            &persist_object_directory,
            vba_info_atom.persist_id_ref,
            vba_info_atom_record.offset,
            &[EXTERNAL_OLE_OBJECT_STORAGE],
            "VBAInfoAtom.persistIdRef does not resolve to VbaProjectStg",
          )?;
          Some(PptLivePersistObject {
            reference,
            role: PptLivePersistObjectRole::VbaProject,
            source_record: vba_info_atom_record,
            record,
            list_records: &[],
          })
        }
      } else {
        None
      }
    } else {
      None
    };

    let mut live_persist_ids = vec![BTreeSet::new(); self.records.records.len()];
    let mut live_roles = vec![BTreeSet::new(); self.records.records.len()];
    let mut add_live_object = |object: PptLivePersistObject<'_>| {
      live_persist_ids[object.reference.record_index].insert(object.reference.persist_id);
      live_roles[object.reference.record_index].insert(object.role);
    };
    add_live_object(document);
    if let Some(value) = notes_master_slide {
      add_live_object(value);
    }
    if let Some(value) = handout_master_slide {
      add_live_object(value);
    }
    for value in master_slides
      .iter()
      .chain(&presentation_slides)
      .chain(&notes_slides)
      .chain(&active_x_controls)
      .chain(&embedded_ole_objects)
      .chain(&linked_ole_objects)
    {
      add_live_object(*value);
    }
    if let Some(value) = vba_project {
      add_live_object(value);
    }

    let top_level_records = persist_object_directory
      .top_level_records
      .iter()
      .map(|record| {
        let status = match &record.role {
          PptTopLevelRecordRole::IncrementalSaveMetadata(kind) => {
            PptTopLevelLiveRecordStatus::LiveIncrementalSaveMetadata(*kind)
          }
          _ if !live_roles[record.record_index].is_empty() => {
            PptTopLevelLiveRecordStatus::LivePersistObject {
              persist_ids: live_persist_ids[record.record_index]
                .iter()
                .copied()
                .collect(),
              roles: live_roles[record.record_index].iter().copied().collect(),
            }
          }
          _ => PptTopLevelLiveRecordStatus::Dead,
        };
        PptTopLevelLiveRecordState {
          record_index: record.record_index,
          stream_offset: record.stream_offset,
          record_type: record.record_type,
          status,
        }
      })
      .collect();

    Ok(PptLivePresentation {
      persist_object_directory,
      document,
      document_atom_record,
      document_atom,
      notes_master_slide,
      handout_master_slide,
      master_slides,
      presentation_slides,
      notes_slides,
      active_x_controls,
      embedded_ole_objects,
      linked_ole_objects,
      vba_project,
      top_level_records,
    })
  }

  pub(crate) fn relocate_picture_references(
    &mut self,
    pictures_layout: Option<&OfficeArtBStoreDelayLayout>,
    preserve_compatibility: bool,
  ) -> Result<usize> {
    let mut relocated = 0usize;
    for record in &mut self.records.records {
      relocate_ppt_record_picture_references(
        record,
        pictures_layout,
        preserve_compatibility,
        &mut relocated,
      )?;
    }
    Ok(relocated)
  }

  fn resolve_live_persist_object<'a>(
    &'a self,
    directory: &PersistObjectDirectory,
    persist_id: u32,
    source_record_offset: u64,
    expected_record_types: &[u16],
    message: &'static str,
  ) -> Result<(PersistObjectReference, &'a PptRecord)> {
    let reference = directory
      .current_reference(persist_id)
      .copied()
      .ok_or_else(|| Error::invalid(source_record_offset, message))?;
    let record = self
      .records
      .records
      .get(reference.record_index)
      .ok_or_else(|| Error::invalid(u64::from(reference.stream_offset), message))?;
    if !expected_record_types.contains(&record.header.record_type)
      || !ppt_live_target_has_conforming_shape(record)
    {
      return Err(Error::invalid(u64::from(reference.stream_offset), message));
    }
    Ok((reference, record))
  }

  fn top_level_record(&self, offset: u32) -> Option<&PptRecord> {
    self
      .records
      .records
      .iter()
      .find(|record| record.offset == u64::from(offset))
  }
}

fn find_ppt_record_path(
  sequence: &PptRecordSequence,
  offset: u64,
  record_type: u16,
  path: &mut Vec<usize>,
) -> bool {
  for (index, record) in sequence.records.iter().enumerate() {
    path.push(index);
    if record.offset == offset && record.header.record_type == record_type {
      return true;
    }
    let children = match &record.data {
      PptRecordData::Container(children)
      | PptRecordData::ProgTags(children)
      | PptRecordData::BinaryTagData(BinaryTagData::Records(children)) => Some(children),
      _ => None,
    };
    if children.is_some_and(|children| find_ppt_record_path(children, offset, record_type, path)) {
      return true;
    }
    path.pop();
  }
  false
}

fn ppt_sequence_at_path_mut<'a>(
  mut sequence: &'a mut PptRecordSequence,
  path: &[usize],
) -> Result<&'a mut PptRecordSequence> {
  for &index in path {
    let record = sequence
      .records
      .get_mut(index)
      .ok_or_else(|| Error::invalid(0, "PPT record path is out of bounds"))?;
    sequence = match &mut record.data {
      PptRecordData::Container(children)
      | PptRecordData::ProgTags(children)
      | PptRecordData::BinaryTagData(BinaryTagData::Records(children)) => children,
      _ => {
        return Err(Error::invalid(
          record.offset,
          "PPT record path crosses a non-container record",
        ));
      }
    };
  }
  Ok(sequence)
}

fn relocate_ppt_record_picture_references(
  record: &mut PptRecord,
  pictures_layout: Option<&OfficeArtBStoreDelayLayout>,
  preserve_compatibility: bool,
  relocated: &mut usize,
) -> Result<()> {
  match &mut record.data {
    PptRecordData::Container(children)
    | PptRecordData::ProgTags(children)
    | PptRecordData::BinaryTagData(BinaryTagData::Records(children)) => {
      for child in &mut children.records {
        relocate_ppt_record_picture_references(
          child,
          pictures_layout,
          preserve_compatibility,
          relocated,
        )?;
      }
    }
    PptRecordData::ProgBinaryTag(value) => {
      for child in &mut value.records.records {
        relocate_ppt_record_picture_references(
          child,
          pictures_layout,
          preserve_compatibility,
          relocated,
        )?;
      }
    }
    PptRecordData::OfficeArt(value) => {
      relocate_office_art_picture_references(
        value,
        pictures_layout,
        preserve_compatibility,
        relocated,
      )?;
    }
    PptRecordData::BlipEntity9(value) => {
      relocate_office_art_picture_references(
        &mut value.blip,
        pictures_layout,
        preserve_compatibility,
        relocated,
      )?;
    }
    _ => {}
  }
  Ok(())
}

fn relocate_office_art_picture_references(
  record: &mut OfficeArtRecord,
  pictures_layout: Option<&OfficeArtBStoreDelayLayout>,
  preserve_compatibility: bool,
  relocated: &mut usize,
) -> Result<()> {
  match &mut record.data {
    OfficeArtRecordData::Container(children)
    | OfficeArtRecordData::CompatibilityContainer(children) => {
      for child in children {
        relocate_office_art_picture_references(
          child,
          pictures_layout,
          preserve_compatibility,
          relocated,
        )?;
      }
    }
    OfficeArtRecordData::Fbse(fbse) => {
      if let Some(blip) = &mut fbse.embedded_blip {
        relocate_office_art_picture_references(
          blip,
          pictures_layout,
          preserve_compatibility,
          relocated,
        )?;
        return Ok(());
      }
      if fbse.reference_count == 0 {
        return Ok(());
      }
      if fbse.delay_offset == u32::MAX {
        if fbse.reference_count != 0 {
          return Err(Error::invalid(
            0,
            "OfficeArtFBSE.foDelay is 0xFFFFFFFF but cRef is nonzero",
          ));
        }
        return Ok(());
      }
      let Some(layout) = pictures_layout else {
        if preserve_compatibility {
          return Ok(());
        }
        return Err(Error::invalid(
          u64::from(fbse.delay_offset),
          "OfficeArtFBSE.foDelay requires a Pictures Stream",
        ));
      };
      let file_block = layout.file_block_at_old_offset(fbse.delay_offset);
      let Some(file_block) = file_block else {
        if preserve_compatibility {
          return Ok(());
        }
        return Err(Error::invalid(
          u64::from(fbse.delay_offset),
          "OfficeArtFBSE.foDelay does not reference a Pictures Stream file block",
        ));
      };
      if !(0xf018..=0xf117).contains(&file_block.record_type) {
        if preserve_compatibility {
          return Ok(());
        }
        return Err(Error::invalid(
          u64::from(fbse.delay_offset),
          "OfficeArtFBSE.foDelay does not reference an OfficeArtBlip",
        ));
      }
      if file_block.old_offset != file_block.new_offset {
        fbse.delay_offset = file_block.new_offset;
      }
      if file_block.old_size != file_block.new_size {
        fbse.declared_blip_size = file_block.new_size;
      }
      *relocated = relocated
        .checked_add(1)
        .ok_or_else(|| Error::Limit("OfficeArtFBSE reference count overflow".into()))?;
    }
    _ => {}
  }
  Ok(())
}

impl PersistObjectDirectory {
  pub fn current_reference(&self, persist_id: u32) -> Option<&PersistObjectReference> {
    self.current_references.get(&persist_id)
  }

  pub fn top_level_record(&self, record_index: usize) -> Option<&PptTopLevelRecordState> {
    self.top_level_records.get(record_index)
  }
}

fn set_incremental_save_metadata(
  metadata: &mut [Option<IncrementalSaveMetadataKind>],
  record_index: usize,
  kind: IncrementalSaveMetadataKind,
  offset: u32,
) -> Result<()> {
  if metadata[record_index].replace(kind).is_some() {
    return Err(Error::invalid(
      u64::from(offset),
      "incremental-save metadata record is referenced more than once",
    ));
  }
  Ok(())
}

fn persist_directory_entries(
  offsets_by_id: &BTreeMap<u32, u32>,
) -> Result<Vec<PersistDirectoryEntry>> {
  let mut entries = Vec::<PersistDirectoryEntry>::new();
  for (&persist_id, &stream_offset) in offsets_by_id {
    if persist_id > 0x000f_fffe {
      return Err(Error::invalid(
        u64::from(stream_offset),
        "live persist object identifier exceeds 0xFFFFE",
      ));
    }
    let can_extend = entries.last().is_some_and(|entry| {
      entry.stream_offsets.len() < 0x0fff
        && entry.first_persist_id
          + u32::try_from(entry.stream_offsets.len()).expect("cPersist fits u32")
          == persist_id
    });
    if can_extend {
      entries
        .last_mut()
        .expect("entry was checked above")
        .stream_offsets
        .push(stream_offset);
    } else {
      entries.push(PersistDirectoryEntry {
        first_persist_id: persist_id,
        stream_offsets: vec![stream_offset],
      });
    }
  }
  Ok(entries)
}

fn is_persist_object_record(record: &PptRecord) -> bool {
  match &record.data {
    PptRecordData::Container(_) => matches!(
      record.header.record_type,
      DOCUMENT_CONTAINER
        | SLIDE_CONTAINER
        | NOTES_CONTAINER
        | MAIN_MASTER_CONTAINER
        | HANDOUT_CONTAINER
    ),
    // Part 1 indexes the top-level storage record. Strict/compatible
    // auditing of its compressed or compound-file payload is separate.
    PptRecordData::ExternalStorage(_) => record.header.record_type == EXTERNAL_OLE_OBJECT_STORAGE,
    _ => false,
  }
}

fn ppt_live_target_has_conforming_shape(record: &PptRecord) -> bool {
  if record.header.record_type == EXTERNAL_OLE_OBJECT_STORAGE {
    record.header.version == 0
      && record.header.instance <= 1
      && matches!(record.data, PptRecordData::ExternalStorage(_))
  } else {
    record.header.version == 0x0f
      && record.header.instance == 0
      && matches!(record.data, PptRecordData::Container(_))
  }
}

fn require_record_version(record: &PptRecord, version: u8, structure: &str) -> Result<()> {
  if record.header.version != version {
    return Err(Error::invalid(
      record.offset,
      format!("{structure}.rh.recVer must be 0x{version:X}"),
    ));
  }
  Ok(())
}

fn push_live_presentation_diagnostic(
  diagnostics: &mut Vec<ParseDiagnostic>,
  code: ParseDiagnosticCode,
  offset: u64,
  structure: &'static str,
  section: &'static str,
  message: impl Into<String>,
) {
  diagnostics.push(ParseDiagnostic::warning(
    code,
    BinaryFormat::Ppt,
    Some(POWERPOINT_DOCUMENT_STREAM_PATH),
    Some(offset),
    structure,
    SpecificationReference {
      document: "MS-PPT",
      section,
    },
    message,
  ));
}

fn ppt_container_children<'a>(
  record: &'a PptRecord,
  structure: &str,
) -> Result<&'a PptRecordSequence> {
  let PptRecordData::Container(children) = &record.data else {
    return Err(Error::invalid(
      record.offset,
      format!("{structure} is not a conforming container record"),
    ));
  };
  Ok(children)
}

fn optional_direct_record<'a>(
  sequence: &'a PptRecordSequence,
  record_type: u16,
  instance: Option<u16>,
  field: &str,
) -> Result<Option<&'a PptRecord>> {
  let mut matching = sequence.records.iter().filter(|record| {
    record.header.record_type == record_type
      && instance.is_none_or(|instance| record.header.instance == instance)
  });
  let first = matching.next();
  if let Some(duplicate) = matching.next() {
    return Err(Error::invalid(
      duplicate.offset,
      format!("{field} occurs more than once"),
    ));
  }
  Ok(first)
}

fn required_direct_record<'a>(
  sequence: &'a PptRecordSequence,
  record_type: u16,
  instance: Option<u16>,
  field: &str,
) -> Result<&'a PptRecord> {
  optional_direct_record(sequence, record_type, instance, field)?.ok_or_else(|| {
    Error::invalid(
      sequence.records.first().map_or(0, |record| record.offset),
      format!("required {field} is missing"),
    )
  })
}

fn direct_slide_persist_atoms<'a>(
  sequence: &'a PptRecordSequence,
  structure: &str,
) -> Result<Vec<(&'a PptRecord, &'a SlidePersistAtom, &'a [PptRecord])>> {
  let mut values = Vec::new();
  for (index, record) in sequence.records.iter().enumerate() {
    if record.header.record_type != SLIDE_PERSIST_ATOM {
      continue;
    }
    let PptRecordData::SlidePersist(value) = &record.data else {
      return Err(Error::invalid(
        record.offset,
        format!("{structure} contains a nonconforming persist atom"),
      ));
    };
    require_record_version(record, 0, "SlidePersistAtom")?;
    if record.header.instance != 0 {
      return Err(Error::invalid(
        record.offset,
        "SlidePersistAtom.rh.recInstance must be 0",
      ));
    }
    let end = sequence.records[index + 1..]
      .iter()
      .position(|candidate| candidate.header.record_type == SLIDE_PERSIST_ATOM)
      .map_or(sequence.records.len(), |relative| index + 1 + relative);
    values.push((record, value, &sequence.records[index + 1..end]));
  }
  Ok(values)
}

fn optional_live_object<'a>(
  document: &'a PowerPointDocument,
  directory: &PersistObjectDirectory,
  persist_id: u32,
  source_record: &'a PptRecord,
  expected_record_types: &[u16],
  role: PptLivePersistObjectRole,
  message: &'static str,
) -> Result<Option<PptLivePersistObject<'a>>> {
  if persist_id == 0 {
    return Ok(None);
  }
  let (reference, record) = document.resolve_live_persist_object(
    directory,
    persist_id,
    source_record.offset,
    expected_record_types,
    message,
  )?;
  Ok(Some(PptLivePersistObject {
    reference,
    role,
    source_record,
    record,
    list_records: &[],
  }))
}

struct ListPersistObjectSpec {
  list_instance: u16,
  target_record_type: u16,
  role: PptLivePersistObjectRole,
  list_name: &'static str,
  source_field: &'static str,
  specification_section: &'static str,
  error_message: &'static str,
}

fn resolve_list_persist_objects<'a>(
  document: &'a PowerPointDocument,
  directory: &PersistObjectDirectory,
  document_children: &'a PptRecordSequence,
  spec: ListPersistObjectSpec,
  strict: bool,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<Vec<PptLivePersistObject<'a>>> {
  let Some(list_record) = optional_direct_record(
    document_children,
    SLIDE_LIST_WITH_TEXT_CONTAINER,
    Some(spec.list_instance),
    spec.list_name,
  )?
  else {
    return Ok(Vec::new());
  };
  let list = ppt_container_children(list_record, spec.list_name)?;
  let mut objects = Vec::new();
  for (source_record, source, list_records) in direct_slide_persist_atoms(list, spec.list_name)? {
    let resolved = document.resolve_live_persist_object(
      directory,
      source.persist_id_ref,
      source_record.offset,
      &[spec.target_record_type],
      spec.error_message,
    );
    let (reference, record) = match resolved {
      Ok(value) => value,
      Err(error) if !strict => {
        push_live_presentation_diagnostic(
          diagnostics,
          ParseDiagnosticCode::InvalidReference,
          source_record.offset,
          spec.source_field,
          spec.specification_section,
          error.to_string(),
        );
        continue;
      }
      Err(error) => return Err(error),
    };
    objects.push(PptLivePersistObject {
      reference,
      role: spec.role,
      source_record,
      record,
      list_records,
    });
  }
  Ok(objects)
}

fn resolve_external_persist_objects<'a>(
  document: &'a PowerPointDocument,
  directory: &PersistObjectDirectory,
  external_object_list: &'a PptRecordSequence,
  container_record_type: u16,
  role: PptLivePersistObjectRole,
  container_name: &str,
  output: &mut Vec<PptLivePersistObject<'a>>,
) -> Result<()> {
  for container_record in external_object_list
    .records
    .iter()
    .filter(|record| record.header.record_type == container_record_type)
  {
    let container = ppt_container_children(container_record, container_name)?;
    let source_record = required_direct_record(
      container,
      EXTERNAL_OLE_OBJECT_ATOM,
      Some(0),
      &format!("{container_name}.exOleObjAtom"),
    )?;
    let PptRecordData::ExternalOleObject(source) = &source_record.data else {
      return Err(Error::invalid(
        source_record.offset,
        format!("{container_name}.exOleObjAtom is not conforming"),
      ));
    };
    require_record_version(source_record, 1, "ExOleObjAtom")?;
    let (reference, record) = document.resolve_live_persist_object(
      directory,
      source.persist_id_ref,
      source_record.offset,
      &[EXTERNAL_OLE_OBJECT_STORAGE],
      "ExOleObjAtom.persistIdRef does not resolve to an external storage record",
    )?;
    output.push(PptLivePersistObject {
      reference,
      role,
      source_record,
      record,
      list_records: &[],
    });
  }
  Ok(())
}

impl CurrentUserStream {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < HEADER_LEN {
      return Err(Error::invalid(
        0,
        "Current User stream lacks a record header",
      ));
    }
    let header = PptRecordHeader::from_bytes(&bytes[..HEADER_LEN]);
    let declared_length = usize::try_from(header.declared_length)
      .map_err(|_| Error::Limit("CurrentUserAtom length exceeds usize".into()))?;
    let available = &bytes[HEADER_LEN..];
    if declared_length > available.len() {
      return Ok(Self {
        header,
        data: CurrentUserData::Truncated(available.to_vec()),
        padding: Vec::new(),
      });
    }
    let body = &available[..declared_length];
    let (data, following_consumed) =
      if header.record_type == CURRENT_USER_ATOM && header.version != 0x0f {
        CurrentUserAtom::parse(body, &available[declared_length..])
          .map(|(value, consumed)| (CurrentUserData::Parsed(value), consumed))
          .unwrap_or_else(|| (CurrentUserData::Compatibility(body.to_vec()), 0))
      } else {
        (CurrentUserData::Compatibility(body.to_vec()), 0)
      };
    Ok(Self {
      header,
      data,
      padding: available[declared_length + following_consumed..].to_vec(),
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let (body, following) = match &self.data {
      CurrentUserData::Parsed(value) => {
        if self.header.record_type != CURRENT_USER_ATOM || self.header.version == 0x0f {
          return Err(Error::invalid(0, "CurrentUserAtom record header changed"));
        }
        value.to_parts()?
      }
      CurrentUserData::Compatibility(bytes) | CurrentUserData::Truncated(bytes) => {
        (bytes.clone(), Vec::new())
      }
    };
    let declared = usize::try_from(self.header.declared_length)
      .map_err(|_| Error::Limit("CurrentUserAtom length exceeds usize".into()))?;
    match self.data {
      CurrentUserData::Truncated(_) if body.len() >= declared => {
        return Err(Error::invalid(
          0,
          "truncated CurrentUserAtom is no longer shorter than declared",
        ));
      }
      CurrentUserData::Truncated(_) => {}
      _ if body.len() != declared => {
        return Err(Error::invalid(0, "CurrentUserAtom body length mismatch"));
      }
      _ => {}
    }
    let mut bytes = Vec::new();
    if self.padding.iter().any(|byte| *byte != 0) {
      return Err(Error::invalid(0, "Current User stream padding is nonzero"));
    }
    self.header.write(&mut bytes)?;
    bytes.extend_from_slice(&body);
    bytes.extend_from_slice(&following);
    bytes.extend_from_slice(&self.padding);
    Ok(bytes)
  }
}

impl PicturesStream {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_bytes_with_limits(bytes, Limits::default())
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    match OfficeArtBStoreDelay::from_bytes_with_limits(bytes, limits) {
      Ok(stream) => Ok(Self::Complete(stream)),
      Err(error) => {
        if let Ok(stream) = OfficeArtStream::from_bytes_with_limits(bytes, limits) {
          return Ok(Self::Compatibility {
            stream,
            reason: error.to_string(),
          });
        }
        match OfficeArtPartialStream::from_bytes_with_limits(bytes, limits, error.to_string()) {
          Ok(stream) => Ok(Self::Partial(stream)),
          Err(_) => Err(error),
        }
      }
    }
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    match self {
      Self::Complete(stream) => stream.to_bytes(),
      Self::Compatibility { stream, .. } => stream.to_bytes(),
      Self::Partial(stream) => stream.to_bytes(),
    }
  }

  pub(crate) fn serialized_len(&self) -> Result<usize> {
    match self {
      Self::Complete(stream) => stream.serialized_len(),
      Self::Compatibility { stream, .. } => stream.serialized_len(),
      Self::Partial(stream) => Ok(stream.available_len()),
    }
  }

  pub(crate) fn write_to(&self, writer: &mut dyn Write) -> Result<()> {
    match self {
      Self::Complete(stream) => stream.write_to(writer),
      Self::Compatibility { stream, .. } => stream.write_to(writer),
      Self::Partial(stream) => writer.write_all(&stream.to_bytes()?).map_err(Error::from),
    }
  }

  pub fn relayout(&mut self) -> Result<OfficeArtBStoreDelayLayout> {
    match self {
      Self::Complete(stream) => stream.relayout(),
      Self::Compatibility { .. } | Self::Partial(_) => Err(Error::invalid(
        0,
        "cannot relayout a nonconforming OfficeArtBStoreDelay",
      )),
    }
  }
}

impl PptRecordSequence {
  fn parse(
    bytes: &[u8],
    base_offset: u64,
    depth: usize,
    limits: Limits,
    record_count: &mut usize,
  ) -> Result<Self> {
    if depth > MAX_RECORD_DEPTH {
      return Err(Error::Limit(format!(
        "PPT record nesting exceeds {MAX_RECORD_DEPTH}"
      )));
    }
    let mut cursor = 0usize;
    let mut records = Vec::new();
    let mut corresponding_text_character_count = None;
    while bytes.len().saturating_sub(cursor) >= HEADER_LEN {
      *record_count = record_count
        .checked_add(1)
        .ok_or_else(|| Error::Limit("PPT record count overflow".into()))?;
      if *record_count > limits.max_entries {
        return Err(Error::Limit(format!(
          "PPT record count exceeds {}",
          limits.max_entries
        )));
      }
      let record_offset = base_offset
        .checked_add(cursor as u64)
        .ok_or_else(|| Error::Limit("PPT record offset overflow".into()))?;
      let header = PptRecordHeader::from_bytes(&bytes[cursor..cursor + HEADER_LEN]);
      cursor += HEADER_LEN;
      let declared_length = usize::try_from(header.declared_length)
        .map_err(|_| Error::Limit("PPT record length exceeds usize".into()))?;
      let available = bytes.len() - cursor;
      if declared_length > available {
        records.push(PptRecord {
          offset: record_offset,
          header,
          data: PptRecordData::Truncated(bytes[cursor..].to_vec()),
        });
        cursor = bytes.len();
        break;
      }
      let body = &bytes[cursor..cursor + declared_length];
      let data = if header.version == 0x0f
        || header.record_type == BINARY_TAG_DATA_BLOB
        || header.record_type == PROG_TAGS
      {
        let child_offset = record_offset
          .checked_add(HEADER_LEN as u64)
          .ok_or_else(|| Error::Limit("PPT child offset overflow".into()))?;
        let children = Self::parse(body, child_offset, depth + 1, limits, record_count)?;
        if header.record_type == PROG_TAGS {
          PptRecordData::ProgTags(children)
        } else if header.record_type == PROG_BINARY_TAG {
          let mut tag = ProgBinaryTag { records: children };
          tag.preserve_private_tag_data()?;
          PptRecordData::ProgBinaryTag(tag)
        } else if header.record_type == BINARY_TAG_DATA_BLOB {
          PptRecordData::BinaryTagData(BinaryTagData::Records(children))
        } else {
          PptRecordData::Container(children)
        }
      } else if header.record_type == USER_EDIT_ATOM {
        parse_fixed(body)
          .map(PptRecordData::UserEdit)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == DOCUMENT_ATOM && body.len() == 40 {
        PptRecordData::Document(parse_fixed(body).expect("fixed DocumentAtom"))
      } else if header.record_type == SLIDE_ATOM && body.len() == 24 {
        PptRecordData::Slide(parse_fixed(body).expect("fixed SlideAtom"))
      } else if header.record_type == NOTES_ATOM && body.len() == 8 {
        PptRecordData::Notes(parse_fixed(body).expect("fixed NotesAtom"))
      } else if header.record_type == OUTLINE_TEXT_REF_ATOM && body.len() == 4 {
        PptRecordData::OutlineTextRef(OutlineTextRefAtom {
          index: read_u32(body, 0),
        })
      } else if header.record_type == TEXT_HEADER_ATOM && body.len() == 4 {
        PptRecordData::TextHeader(TextHeaderAtom {
          text_type: PptTextType::from_raw(read_u32(body, 0)),
        })
      } else if header.record_type == TEXT_CHARS_ATOM && body.len().is_multiple_of(2) {
        let code_units = read_utf16(body);
        match String::from_utf16(&code_units) {
          Ok(value) => PptRecordData::TextChars(value),
          Err(_) => PptRecordData::CompatibilityTextChars(code_units),
        }
      } else if header.record_type == TEXT_BYTES_ATOM {
        PptRecordData::TextBytes(body.iter().copied().map(char::from).collect())
      } else if header.record_type == STYLE_TEXT_PROP_ATOM {
        match corresponding_text_character_count {
          Some(character_count) => StyleTextPropAtom::parse(body, character_count)
            .map(PptRecordData::StyleTextProp)
            .unwrap_or_else(|| {
              PptRecordData::MalformedStyleTextProp(MalformedStyleTextPropAtom {
                corresponding_text_character_count: character_count,
                body: body.to_vec(),
              })
            }),
          None => PptRecordData::UnresolvedStyleTextProp(body.to_vec()),
        }
      } else if header.record_type == C_STRING_ATOM && body.len().is_multiple_of(2) {
        let code_units = read_utf16(body);
        match String::from_utf16(&code_units) {
          Ok(value) => PptRecordData::CString(value),
          Err(_) => PptRecordData::CompatibilityCString(code_units),
        }
      } else if header.record_type == SLIDE_PERSIST_ATOM && body.len() == 20 {
        PptRecordData::SlidePersist(parse_fixed(body).expect("fixed SlidePersistAtom"))
      } else if header.record_type == COLOR_SCHEME_ATOM && body.len() == 32 {
        PptRecordData::ColorScheme(parse_fixed(body).expect("fixed ColorSchemeAtom"))
      } else if header.record_type == EXTERNAL_OBJECT_REF_ATOM && body.len() == 4 {
        PptRecordData::ExternalObjectRef(parse_fixed(body).expect("fixed ExternalObjectRefAtom"))
      } else if header.record_type == PLACEHOLDER_ATOM && body.len() == 8 {
        PptRecordData::Placeholder(parse_fixed(body).expect("fixed PlaceholderAtom"))
      } else if header.record_type == HEADERS_FOOTERS_ATOM && body.len() == 4 {
        PptRecordData::HeadersFooters(parse_fixed(body).expect("fixed HeadersFootersAtom"))
      } else if header.record_type == MASTER_TEXT_PROP_ATOM && body.len().is_multiple_of(6) {
        PptRecordData::MasterTextProp(MasterTextPropAtom {
          runs: body
            .chunks_exact(6)
            .map(|bytes| parse_fixed(bytes).expect("fixed MasterTextPropRun"))
            .collect(),
        })
      } else if header.record_type == TEXT_MASTER_STYLE_ATOM {
        TextMasterStyleAtom::parse(body, header.instance)
          .map(PptRecordData::TextMasterStyle)
          .unwrap_or_else(|| PptRecordData::MalformedTextMasterStyle(body.to_vec()))
      } else if header.record_type == TEXT_CF_EXCEPTION_ATOM {
        let mut body_cursor = 0usize;
        TextCharacterException::parse(body, &mut body_cursor)
          .filter(|_| body_cursor == body.len())
          .map(PptRecordData::TextCfException)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == TEXT_PF_EXCEPTION_ATOM {
        let mut body_cursor = 0usize;
        let parsed = read_u16_checked(body, &mut body_cursor).and_then(|reserved| {
          TextParagraphException::parse(body, &mut body_cursor).map(|paragraph| {
            TextPfExceptionAtom {
              reserved,
              paragraph,
            }
          })
        });
        parsed
          .filter(|_| body_cursor == body.len())
          .map(PptRecordData::TextPfException)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == TEXT_SI_EXCEPTION_ATOM {
        let mut body_cursor = 0usize;
        TextSpecialInfoException::parse(body, &mut body_cursor)
          .filter(|_| body_cursor == body.len())
          .map(PptRecordData::TextSiException)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if matches!(header.record_type, TEXT_RULER_ATOM | DEFAULT_RULER_ATOM) {
        TextRulerAtom::parse(body)
          .map(PptRecordData::TextRuler)
          .unwrap_or_else(|| PptRecordData::MalformedTextRuler(body.to_vec()))
      } else if header.record_type == TEXT_SPECIAL_INFO_ATOM {
        TextSpecialInfoAtom::parse(body)
          .map(PptRecordData::TextSpecialInfo)
          .unwrap_or_else(|| PptRecordData::MalformedTextSpecialInfo(body.to_vec()))
      } else if header.record_type == STYLE_TEXT_PROP9_ATOM {
        StyleTextProp9Atom::parse(body)
          .map(PptRecordData::StyleTextProp9)
          .unwrap_or_else(|| PptRecordData::MalformedStyleTextProp9(body.to_vec()))
      } else if header.record_type == TEXT_MASTER_STYLE9_ATOM {
        TextMasterStyle9Atom::parse(body, header.instance)
          .map(PptRecordData::TextMasterStyle9)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == STYLE_TEXT_PROP10_ATOM {
        StyleTextProp10Atom::parse(body)
          .map(PptRecordData::StyleTextProp10)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == TEXT_MASTER_STYLE10_ATOM {
        TextMasterStyle10Atom::parse(body, header.instance)
          .map(PptRecordData::TextMasterStyle10)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == TEXT_DEFAULTS10_ATOM {
        parse_fixed(body)
          .map(PptRecordData::TextDefaults10)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == STYLE_TEXT_PROP11_ATOM {
        StyleTextProp11Atom::parse(body)
          .map(PptRecordData::StyleTextProp11)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == RECOLOR_INFO_ATOM {
        RecolorInfoAtom::parse(body)
          .map(PptRecordData::RecolorInfo)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == MAC_PRINT_SETTINGS_ATOM {
        PptRecordData::MacPrintSettings(MacPlistAtom::from_bytes(body))
      } else if header.record_type == MAC_PAGE_FORMAT_ATOM {
        PptRecordData::MacPageFormat(MacPlistAtom::from_bytes(body))
      } else if header.record_type == PPT11_FONT_DESCRIPTOR_ATOM {
        Ppt11FontDescriptorAtom::parse(body)
          .map(PptRecordData::Ppt11FontDescriptors)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == PPT11_FONT_DESCRIPTOR_COLLECTION_ATOM {
        Ppt11FontDescriptorCollectionAtom::parse(body)
          .map(PptRecordData::Ppt11FontDescriptorCollection)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == PPT10_RESERVED_ATOM && body.len() == 4 {
        PptRecordData::Ppt10Reserved(parse_fixed(body).expect("fixed PPT10 reserved atom"))
      } else if header.record_type == MAC_LEGACY_PRINT_INFO_ATOM && body.len() == 120 {
        PptRecordData::MacLegacyPrintInfo(MacLegacyPrintInfoAtom {
          bytes: body.try_into().expect("120-byte Macintosh print info"),
        })
      } else if header.record_type == MAC_PRINT_DRIVER_INFO_ATOM && body.len() == 52 {
        PptRecordData::MacPrintDriverInfo(MacPrintDriverInfoAtom {
          bytes: body.try_into().expect("52-byte Macintosh driver info"),
        })
      } else if header.record_type == HANDOUT_COMPATIBILITY_ATOM && body.len() == 8 {
        PptRecordData::HandoutCompatibility(HandoutCompatibilityAtom {
          bytes: body.try_into().expect("8-byte handout compatibility atom"),
        })
      } else if header.record_type == NAMED_SHOW_SLIDES_ATOM && body.len().is_multiple_of(4) {
        PptRecordData::NamedShowSlides(
          body
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("4-byte slide id")))
            .collect(),
        )
      } else if header.record_type == BOOKMARK_SEED_ATOM && body.len() == 4 {
        PptRecordData::BookmarkSeed(parse_fixed(body).expect("fixed BookmarkSeedAtom"))
      } else if header.record_type == SHAPE_ATOM && body.len() == 1 {
        PptRecordData::ShapeFlags(parse_fixed(body).expect("fixed ShapeFlagsAtom"))
      } else if header.record_type == SHAPE_FLAGS10_ATOM && body.len() == 1 {
        PptRecordData::ShapeFlags10(parse_fixed(body).expect("fixed ShapeFlags10Atom"))
      } else if header.record_type == ROUND_TRIP_NEW_PLACEHOLDER_ID_12_ATOM && body.len() == 1 {
        PptRecordData::RoundTripNewPlaceholderId12(
          parse_fixed(body).expect("fixed RoundTripNewPlaceholderId12Atom"),
        )
      } else if header.record_type == FONT_EMBED_DATA_BLOB {
        PptRecordData::FontEmbedDataBlob(body.to_vec())
      } else if header.record_type == BOOKMARK_ENTITY_ATOM && body.len() == 68 {
        PptRecordData::BookmarkEntity(parse_fixed(body).expect("fixed BookmarkEntityAtom"))
      } else if header.record_type == RTF_DATE_TIME_META_CHARACTER_ATOM && body.len() == 132 {
        PptRecordData::RtfDateTimeMeta(parse_fixed(body).expect("fixed RTFDateTimeMCAtom"))
      } else if header.record_type == CHART_BUILD_ATOM && body.len() == 8 {
        PptRecordData::ChartBuild(parse_fixed(body).expect("fixed ChartBuildAtom"))
      } else if header.record_type == DIAGRAM_BUILD_ATOM && body.len() == 4 {
        PptRecordData::DiagramBuild(parse_fixed(body).expect("fixed DiagramBuildAtom"))
      } else if header.record_type == LINKED_SHAPE10_ATOM && body.len() == 8 {
        PptRecordData::LinkedShape10(parse_fixed(body).expect("fixed LinkedShape10Atom"))
      } else if header.record_type == LINKED_SLIDE10_ATOM && body.len() == 8 {
        PptRecordData::LinkedSlide10(parse_fixed(body).expect("fixed LinkedSlide10Atom"))
      } else if header.record_type == DIFF10_ATOM && body.len() == 12 {
        PptRecordData::Diff10(parse_fixed(body).expect("fixed Diff10Atom"))
      } else if header.record_type == SLIDE_LIST_TABLE_SIZE10_ATOM && body.len() == 4 {
        PptRecordData::SlideListTableSize10(
          parse_fixed(body).expect("fixed SlideListTableSize10Atom"),
        )
      } else if header.record_type == SLIDE_LIST_ENTRY10_ATOM && body.len() == 12 {
        PptRecordData::SlideListEntry10(parse_fixed(body).expect("fixed SlideListEntry10Atom"))
      } else if header.record_type == FONT_EMBED_FLAGS10_ATOM && body.len() == 4 {
        PptRecordData::FontEmbedFlags10(parse_fixed(body).expect("fixed FontEmbedFlags10Atom"))
      } else if header.record_type == PHOTO_ALBUM_INFO10_ATOM && body.len() == 6 {
        PptRecordData::PhotoAlbumInfo10(parse_fixed(body).expect("fixed PhotoAlbumInfo10Atom"))
      } else if header.record_type == TIME_ITERATE_DATA_ATOM && body.len() == 20 {
        PptRecordData::TimeIterateData(parse_fixed(body).expect("fixed TimeIterateDataAtom"))
      } else if header.record_type == TEXT_DEFAULTS9_ATOM {
        parse_fixed(body)
          .map(PptRecordData::TextDefaults9)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == EXTERNAL_OLE_LINK_ATOM && body.len() == 12 {
        PptRecordData::ExternalOleLink(parse_fixed(body).expect("fixed ExOleLinkAtom"))
      } else if header.record_type == EXTERNAL_OLE_CONTROL_ATOM && body.len() == 4 {
        PptRecordData::ExternalOleControl(parse_fixed(body).expect("fixed ExControlAtom"))
      } else if header.record_type == EXTERNAL_CD_AUDIO_ATOM && body.len() == 8 {
        PptRecordData::ExternalCdAudio(parse_fixed(body).expect("fixed ExCDAudioAtom"))
      } else if header.record_type == BROADCAST_DOC_INFO9_ATOM && body.len() == 34 {
        PptRecordData::BroadcastDocInfo9(parse_fixed(body).expect("fixed BroadcastDocInfo9Atom"))
      } else if header.record_type == ENVELOPE_FLAGS9_ATOM && body.len() == 4 {
        PptRecordData::EnvelopeFlags9(parse_fixed(body).expect("fixed EnvelopeFlags9Atom"))
      } else if header.record_type == ENVELOPE_DATA9_ATOM {
        PptRecordData::EnvelopeData9(body.to_vec())
      } else if header.record_type == DOC_ROUTING_SLIP_ATOM {
        DocRoutingSlipAtom::parse(body, limits)
          .map(PptRecordData::DocRoutingSlip)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == METAFILE_BLOB {
        MetafileBlob::parse(body)
          .map(PptRecordData::Metafile)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == ROUND_TRIP_SLIDE_SYNC_INFO12_ATOM && body.len() == 32 {
        PptRecordData::RoundTripSlideSyncInfo12(
          parse_fixed(body).expect("fixed SlideSyncInfoAtom12"),
        )
      } else if header.record_type == TIME_COLOR_BEHAVIOR_ATOM {
        TimeColorBehaviorAtom::parse(body)
          .map(PptRecordData::TimeColorBehavior)
          .unwrap_or_else(|| malformed_spec_record(header.record_type, body))
      } else if header.record_type == TIME_ROTATION_BEHAVIOR_ATOM && body.len() == 20 {
        PptRecordData::TimeRotationBehavior(
          parse_fixed(body).expect("fixed TimeRotationBehaviorAtom"),
        )
      } else if header.record_type == TIME_NODE_ATOM && body.len() == 32 {
        PptRecordData::TimeNode(parse_fixed(body).expect("fixed TimeNodeAtom"))
      } else if header.record_type == TIME_CONDITION_ATOM && body.len() == 16 {
        PptRecordData::TimeCondition(parse_fixed(body).expect("fixed TimeConditionAtom"))
      } else if header.record_type == TIME_MODIFIER_ATOM && body.len() == 8 {
        PptRecordData::TimeModifier(parse_fixed(body).expect("fixed TimeModifierAtom"))
      } else if header.record_type == TIME_BEHAVIOR_ATOM && body.len() == 16 {
        PptRecordData::TimeBehavior(parse_fixed(body).expect("fixed TimeBehaviorAtom"))
      } else if header.record_type == TIME_ANIMATE_BEHAVIOR_ATOM && body.len() == 12 {
        PptRecordData::TimeAnimateBehavior(
          parse_fixed(body).expect("fixed TimeAnimateBehaviorAtom"),
        )
      } else if header.record_type == TIME_EFFECT_BEHAVIOR_ATOM && body.len() == 8 {
        PptRecordData::TimeEffectBehavior(parse_fixed(body).expect("fixed TimeEffectBehaviorAtom"))
      } else if header.record_type == TIME_MOTION_BEHAVIOR_ATOM && body.len() == 32 {
        PptRecordData::TimeMotionBehavior(parse_fixed(body).expect("fixed TimeMotionBehaviorAtom"))
      } else if header.record_type == TIME_SCALE_BEHAVIOR_ATOM && body.len() == 32 {
        PptRecordData::TimeScaleBehavior(parse_fixed(body).expect("fixed TimeScaleBehaviorAtom"))
      } else if header.record_type == TIME_SET_BEHAVIOR_ATOM && body.len() == 8 {
        PptRecordData::TimeSetBehavior(parse_fixed(body).expect("fixed TimeSetBehaviorAtom"))
      } else if header.record_type == TIME_COMMAND_BEHAVIOR_ATOM && body.len() == 8 {
        PptRecordData::TimeCommandBehavior(
          parse_fixed(body).expect("fixed TimeCommandBehaviorAtom"),
        )
      } else if header.record_type == TIME_SEQUENCE_DATA_ATOM && body.len() == 20 {
        PptRecordData::TimeSequenceData(parse_fixed(body).expect("fixed TimeSequenceDataAtom"))
      } else if header.record_type == TIME_ANIMATION_VALUE_ATOM && body.len() == 4 {
        PptRecordData::TimeAnimationValue(parse_fixed(body).expect("fixed TimeAnimationValueAtom"))
      } else if header.record_type == TIME_VARIANT_ATOM {
        TimeVariantAtom::parse(body)
          .map(PptRecordData::TimeVariant)
          .unwrap_or_else(|| PptRecordData::MalformedTimeVariant(body.to_vec()))
      } else if header.record_type == VISUAL_SHAPE_ATOM && body.len() == 20 {
        PptRecordData::VisualShape(parse_fixed(body).expect("fixed VisualShapeAtom"))
      } else if header.record_type == HASH_CODE_ATOM && body.len() == 4 {
        PptRecordData::HashCode(parse_fixed(body).expect("fixed HashCodeAtom"))
      } else if header.record_type == VISUAL_PAGE_ATOM && body.len() == 4 {
        PptRecordData::VisualPage(parse_fixed(body).expect("fixed VisualPageAtom"))
      } else if header.record_type == BUILD_ATOM && body.len() == 16 {
        PptRecordData::Build(parse_fixed(body).expect("fixed BuildAtom"))
      } else if header.record_type == PARA_BUILD_ATOM && body.len() == 16 {
        PptRecordData::ParaBuild(parse_fixed(body).expect("fixed ParaBuildAtom"))
      } else if header.record_type == LEVEL_INFO_ATOM && body.len() == 4 {
        PptRecordData::LevelInfo(parse_fixed(body).expect("fixed LevelInfoAtom"))
      } else if header.record_type == SLIDE_TIME_10_ATOM && body.len() == 8 {
        PptRecordData::SlideTime10(parse_fixed(body).expect("fixed SlideTime10Atom"))
      } else if header.record_type == FONT_ENTITY_ATOM && body.len() == 68 {
        PptRecordData::FontEntity(parse_fixed(body).expect("fixed FontEntityAtom"))
      } else if header.record_type == EXTERNAL_OLE_OBJECT_ATOM && body.len() == 24 {
        PptRecordData::ExternalOleObject(parse_fixed(body).expect("fixed ExternalOleObjectAtom"))
      } else if header.record_type == EXTERNAL_OLE_EMBED_ATOM && body.len() == 8 {
        PptRecordData::ExternalOleEmbed(parse_fixed(body).expect("fixed ExternalOleEmbedAtom"))
      } else if header.record_type == KINSOKU_ATOM && body.len() == 4 {
        PptRecordData::Kinsoku(parse_fixed(body).expect("fixed KinsokuAtom"))
      } else if header.record_type == EXTERNAL_HYPERLINK_ATOM && body.len() == 4 {
        PptRecordData::ExternalHyperlinkId(
          parse_fixed(body).expect("fixed ExternalHyperlinkIdAtom"),
        )
      } else if header.record_type == EXTERNAL_HYPERLINK_FLAGS_ATOM && body.len() == 4 {
        PptRecordData::ExternalHyperlinkFlags(
          parse_fixed(body).expect("fixed ExternalHyperlinkFlagsAtom"),
        )
      } else if header.record_type == SLIDE_NUMBER_META_CHARACTER_ATOM && body.len() == 4 {
        PptRecordData::SlideNumberMeta(parse_fixed(body).expect("fixed SlideNumberMCAtom"))
      } else if header.record_type == TEXT_INTERACTIVE_INFO_ATOM && body.len() == 8 {
        PptRecordData::TextInteractiveInfo(
          parse_fixed(body).expect("fixed TextInteractiveInfoAtom"),
        )
      } else if header.record_type == ANIMATION_INFO_ATOM && body.len() == 28 {
        PptRecordData::AnimationInfo(parse_fixed(body).expect("fixed AnimationInfoAtom"))
      } else if header.record_type == INTERACTIVE_INFO_ATOM && body.len() == 16 {
        PptRecordData::InteractiveInfo(parse_fixed(body).expect("fixed InteractiveInfoAtom"))
      } else if header.record_type == DATE_TIME_META_CHARACTER_ATOM && body.len() == 8 {
        PptRecordData::DateTimeMeta(parse_fixed(body).expect("fixed DateTimeMCAtom"))
      } else if header.record_type == GENERIC_DATE_META_CHARACTER_ATOM && body.len() == 4 {
        PptRecordData::GenericDateMeta(parse_fixed(body).expect("fixed GenericDateMCAtom"))
      } else if header.record_type == HEADER_META_CHARACTER_ATOM && body.len() == 4 {
        PptRecordData::HeaderMeta(parse_fixed(body).expect("fixed HeaderMCAtom"))
      } else if header.record_type == FOOTER_META_CHARACTER_ATOM && body.len() == 4 {
        PptRecordData::FooterMeta(parse_fixed(body).expect("fixed FooterMCAtom"))
      } else if header.record_type == VIEW_INFO_ATOM && body.len() == 52 {
        PptRecordData::ViewInfo(parse_fixed(body).expect("fixed ViewInfoAtom"))
      } else if header.record_type == BLIP_ENTITY9_ATOM {
        match BlipEntity9Atom::parse(body, limits) {
          Ok(value) => PptRecordData::BlipEntity9(Box::new(value)),
          Err(error) => PptRecordData::MalformedBlipEntity9 {
            body: body.to_vec(),
            reason: error.to_string(),
          },
        }
      } else if header.record_type == ROUND_TRIP_ANIMATION_12_ATOM {
        PptRecordData::RoundTripAnimation12(Box::new(RoundTripAnimation12Atom {
          package: TimingOpcPackage::from_bytes(body),
        }))
      } else if header.record_type == ROUND_TRIP_ANIMATION_HASH_12_ATOM && body.len() == 4 {
        PptRecordData::RoundTripAnimationHash12(
          parse_fixed(body).expect("fixed RoundTripAnimationHashAtom"),
        )
      } else if header.record_type == SLIDE_SHOW_SLIDE_INFO_ATOM && body.len() == 16 {
        PptRecordData::SlideShowSlideInfo(parse_fixed(body).expect("fixed SlideShowSlideInfoAtom"))
      } else if header.record_type == GUIDE_ATOM && body.len() == 8 {
        PptRecordData::Guide(parse_fixed(body).expect("fixed GuideAtom"))
      } else if header.record_type == SLIDE_VIEW_INFO_ATOM && body.len() == 3 {
        PptRecordData::SlideViewInfo(parse_fixed(body).expect("fixed SlideViewInfoAtom"))
      } else if header.record_type == VBA_INFO_ATOM && body.len() == 12 {
        PptRecordData::VbaInfo(parse_fixed(body).expect("fixed VBAInfoAtom"))
      } else if header.record_type == SLIDE_SHOW_DOC_INFO_ATOM && body.len() == 80 {
        PptRecordData::SlideShowDocInfo(parse_fixed(body).expect("fixed SlideShowDocInfoAtom"))
      } else if header.record_type == EXTERNAL_OBJECT_LIST_ATOM && body.len() == 4 {
        PptRecordData::ExternalObjectList(parse_fixed(body).expect("fixed ExObjListAtom"))
      } else if header.record_type == GRID_SPACING_10_ATOM && body.len() == 8 {
        PptRecordData::GridSpacing10(parse_fixed(body).expect("fixed GridSpacing10Atom"))
      } else if header.record_type == NORMAL_VIEW_SET_INFO_9_ATOM && body.len() == 20 {
        PptRecordData::NormalViewSetInfo9(parse_fixed(body).expect("fixed NormalViewSetInfoAtom"))
      } else if header.record_type == ROUND_TRIP_ORIGINAL_MAIN_MASTER_ID_12_ATOM && body.len() == 4
      {
        PptRecordData::RoundTripOriginalMainMasterId12(
          parse_fixed(body).expect("fixed RoundTripOriginalMainMasterId12Atom"),
        )
      } else if header.record_type == ROUND_TRIP_COMPOSITE_MASTER_ID_12_ATOM && body.len() == 4 {
        PptRecordData::RoundTripCompositeMasterId12(
          parse_fixed(body).expect("fixed RoundTripCompositeMasterId12Atom"),
        )
      } else if header.record_type == ROUND_TRIP_SHAPE_ID_12_ATOM && body.len() == 4 {
        PptRecordData::RoundTripShapeId12(parse_fixed(body).expect("fixed RoundTripShapeId12Atom"))
      } else if header.record_type == ROUND_TRIP_HF_PLACEHOLDER_12_ATOM && body.len() == 1 {
        PptRecordData::RoundTripHfPlaceholder12(
          parse_fixed(body).expect("fixed RoundTripHFPlaceholder12Atom"),
        )
      } else if header.record_type == ROUND_TRIP_CONTENT_MASTER_ID_12_ATOM && body.len() == 8 {
        PptRecordData::RoundTripContentMasterId12(
          parse_fixed(body).expect("fixed RoundTripContentMasterId12Atom"),
        )
      } else if header.record_type == ROUND_TRIP_HEADER_FOOTER_DEFAULTS_12_ATOM && body.len() == 1 {
        PptRecordData::RoundTripHeaderFooterDefaults12(
          parse_fixed(body).expect("fixed RoundTripHeaderFooterDefaults12Atom"),
        )
      } else if header.record_type == ROUND_TRIP_DOC_FLAGS_12_ATOM && body.len() == 1 {
        PptRecordData::RoundTripDocFlags12(
          parse_fixed(body).expect("fixed RoundTripDocFlags12Atom"),
        )
      } else if header.record_type == ROUND_TRIP_SHAPE_CHECKSUM_12_ATOM && body.len() == 8 {
        PptRecordData::RoundTripShapeChecksum12(
          parse_fixed(body).expect("fixed RoundTripShapeChecksum12Atom"),
        )
      } else if header.record_type == END_DOCUMENT_ATOM && body.is_empty() {
        PptRecordData::EndDocument
      } else if header.record_type == SOUND_COLLECTION_ATOM && body.len() == 4 {
        PptRecordData::SoundCollection(parse_fixed(body).expect("fixed SoundCollectionAtom"))
      } else if header.record_type == SOUND_DATA_BLOB {
        PptRecordData::SoundDataBlob(body.to_vec())
      } else if header.record_type == TEXT_BOOKMARK_ATOM && body.len() == 12 {
        PptRecordData::TextBookmark(parse_fixed(body).expect("fixed TextBookmarkAtom"))
      } else if header.record_type == OUTLINE_TEXT_PROPS_HEADER9_ATOM && body.len() == 8 {
        PptRecordData::OutlineTextPropsHeader9(
          parse_fixed(body).expect("fixed OutlineTextPropsHeaderExAtom"),
        )
      } else if header.record_type == EXTERNAL_MEDIA_ATOM && body.len() == 8 {
        PptRecordData::ExternalMedia(parse_fixed(body).expect("fixed ExMediaAtom"))
      } else if header.record_type == EXTERNAL_WAV_AUDIO_EMBEDDED_ATOM && body.len() == 8 {
        PptRecordData::ExternalWavAudioEmbedded(
          parse_fixed(body).expect("fixed ExWAVAudioEmbeddedAtom"),
        )
      } else if header.record_type == PRINT_OPTIONS_ATOM && body.len() == 5 {
        PptRecordData::PrintOptions(parse_fixed(body).expect("fixed PrintOptionsAtom"))
      } else if header.record_type == PRESENTATION_ADVISOR_FLAGS9_ATOM && body.len() == 4 {
        PptRecordData::PresentationAdvisorFlags9(
          parse_fixed(body).expect("fixed PresAdvisorFlags9Atom"),
        )
      } else if header.record_type == HTML_DOC_INFO9_ATOM && body.len() == 16 {
        PptRecordData::HtmlDocInfo9(parse_fixed(body).expect("fixed HTMLDocInfo9Atom"))
      } else if header.record_type == HTML_PUBLISH_INFO_ATOM && body.len() == 12 {
        PptRecordData::HtmlPublishInfo(parse_fixed(body).expect("fixed HTMLPublishInfoAtom"))
      } else if header.record_type == COMMENT10_ATOM && body.len() == 28 {
        PptRecordData::Comment10(parse_fixed(body).expect("fixed Comment10Atom"))
      } else if header.record_type == COMMENT_INDEX10_ATOM && body.len() == 8 {
        PptRecordData::CommentIndex10(parse_fixed(body).expect("fixed CommentIndex10Atom"))
      } else if header.record_type == SLIDE_FLAGS10_ATOM && body.len() == 4 {
        PptRecordData::SlideFlags10(parse_fixed(body).expect("fixed SlideFlags10Atom"))
      } else if header.record_type == FILTER_PRIVACY_FLAGS10_ATOM && body.len() == 4 {
        PptRecordData::FilterPrivacyFlags10(
          parse_fixed(body).expect("fixed FilterPrivacyFlags10Atom"),
        )
      } else if header.record_type == DOC_TOOLBAR_STATES10_ATOM && body.len() == 1 {
        PptRecordData::DocToolbarStates10(parse_fixed(body).expect("fixed DocToolbarStates10Atom"))
      } else if header.record_type == EXTERNAL_OLE_OBJECT_STORAGE {
        PptRecordData::ExternalStorage(ExternalStorageAtom::parse(header.instance, body, limits))
      } else if header.record_type == ROUND_TRIP_CONTENT_MASTER_INFO_12_ATOM {
        PptRecordData::RoundTripContentMasterInfo12(Box::new(RoundTripContentMasterInfo12Atom {
          layout_index: header.instance,
          package: SlideLayoutOpcPackage::from_bytes(body),
        }))
      } else if header.record_type == ROUND_TRIP_COLOR_MAPPING_12_ATOM {
        PptRecordData::RoundTripColorMapping12(RoundTripColorMapping12Atom::from_bytes(body))
      } else if header.record_type == ROUND_TRIP_THEME_12_ATOM {
        PptRecordData::RoundTripTheme12(Box::new(RoundTripTheme12Atom {
          package: ThemeOpcPackage::from_bytes(body),
        }))
      } else if matches!(
        header.record_type,
        ROUND_TRIP_OART_TEXT_STYLES_12_ATOM
          | ROUND_TRIP_NOTES_MASTER_TEXT_STYLES_12_ATOM
          | ROUND_TRIP_CUSTOM_TABLE_STYLES_12_ATOM
      ) {
        PptRecordData::RoundTripStyle12(Box::new(RoundTripStyle12Atom {
          record_type: header.record_type,
          package: StyleOpcPackage::from_bytes(body),
        }))
      } else if matches!(
        header.record_type,
        PERSIST_DIRECTORY_FULL_BLOCK | PERSIST_DIRECTORY_ATOM
      ) {
        PersistDirectoryAtom::parse(body, limits)
          .map(PptRecordData::PersistDirectory)
          .unwrap_or_else(|| preserved_unparsed_record(header.record_type, body))
      } else if let Some(record) = parse_office_art_atom(header, body, limits) {
        PptRecordData::OfficeArt(Box::new(record))
      } else {
        preserved_unparsed_record(header.record_type, body)
      };
      records.push(PptRecord {
        offset: record_offset,
        header,
        data,
      });
      match &records.last().expect("record was just pushed").data {
        // A TextHeaderAtom without a following text atom denotes an empty text body;
        // its style runs can still cover the implicit paragraph terminator.
        PptRecordData::TextHeader(_) => corresponding_text_character_count = Some(0),
        PptRecordData::TextChars(values) => {
          corresponding_text_character_count = u32::try_from(values.encode_utf16().count()).ok()
        }
        PptRecordData::TextBytes(values) => {
          corresponding_text_character_count = u32::try_from(values.chars().count()).ok()
        }
        PptRecordData::CompatibilityTextChars(values) => {
          corresponding_text_character_count = u32::try_from(values.len()).ok()
        }
        _ => {}
      }
      cursor += declared_length;
    }
    Ok(Self {
      records,
      trailing_header_bytes: bytes[cursor..].to_vec(),
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(self.serialized_len_from_layout()?);
    self.write_to(&mut bytes)?;
    Ok(bytes)
  }

  fn write_to<W: Write + ?Sized>(&self, writer: &mut W) -> Result<()> {
    for record in &self.records {
      record.write_to(writer)?;
    }
    writer.write_all(&self.trailing_header_bytes)?;
    Ok(())
  }

  pub(crate) fn serialized_len_from_layout(&self) -> Result<usize> {
    let mut length = self.trailing_header_bytes.len();
    for record in &self.records {
      let body_len = record.body_len_from_layout()?;
      length = length
        .checked_add(HEADER_LEN)
        .and_then(|length| length.checked_add(body_len))
        .ok_or_else(|| Error::Limit("PPT record sequence length overflow".into()))?;
    }
    Ok(length)
  }

  fn relayout(
    &mut self,
    base_offset: u64,
    mut top_level_layout: Option<&mut PptTopLevelLayout>,
    preserve_compatibility: bool,
  ) -> Result<u64> {
    let mut offset = base_offset;
    for record in &mut self.records {
      let old_offset = record.offset;
      record.relayout_children(offset, preserve_compatibility)?;
      let body_len = record.body_len_for_relayout()?;
      if !matches!(record.data, PptRecordData::Truncated(_)) {
        record.header.declared_length = u32::try_from(body_len)
          .map_err(|_| Error::Limit("PPT record body exceeds u32".into()))?;
      }
      record.offset = offset;
      if let Some(layout) = top_level_layout.as_deref_mut() {
        layout.insert(
          u32::try_from(old_offset)
            .map_err(|_| Error::Limit("PPT source record offset exceeds u32".into()))?,
          u32::try_from(offset)
            .map_err(|_| Error::Limit("PPT record offset exceeds u32".into()))?,
          PptTopLevelRecordKind::from_data(&record.data),
        );
      }
      offset = offset
        .checked_add(HEADER_LEN as u64)
        .and_then(|value| value.checked_add(body_len as u64))
        .ok_or_else(|| Error::Limit("PPT record sequence length overflow".into()))?;
    }
    offset
      .checked_add(self.trailing_header_bytes.len() as u64)
      .ok_or_else(|| Error::Limit("PPT record sequence tail overflow".into()))
  }

  pub fn visit(&self, visitor: &mut impl FnMut(&PptRecord)) {
    for record in &self.records {
      visitor(record);
      match &record.data {
        PptRecordData::Container(children) => children.visit(visitor),
        PptRecordData::BinaryTagData(BinaryTagData::Records(children)) => children.visit(visitor),
        PptRecordData::ProgBinaryTag(value) => value.records.visit(visitor),
        PptRecordData::ProgTags(children) => children.visit(visitor),
        _ => {}
      }
    }
  }
}

fn collect_ppt_office_art_drawing_components(
  record: &PptRecord,
  drawing_groups: &mut Vec<OfficeArtDggBlock>,
  blip_stores: &mut Vec<OfficeArtGraphBlipStoreInput>,
  drawing_group_blip_references: &mut Vec<OfficeArtBlipReference>,
  drawing_group_incomplete_property_tables: &mut Vec<OfficeArtPropertyTableLocation>,
  drawings: &mut Vec<OfficeArtGraphDrawingInput>,
) -> Result<()> {
  let PptRecordData::Container(children) = &record.data else {
    return Ok(());
  };
  if record.header.record_type == 0xf000 {
    let mut property_table_index = 0usize;
    children.visit(&mut |child| {
      if let PptRecordData::OfficeArt(value) = &child.data {
        if let OfficeArtRecordData::DggBlock(value) = &value.data {
          drawing_groups.push(value.clone());
        }
        collect_office_art_record_blip_references(
          value,
          None,
          &mut property_table_index,
          drawing_group_blip_references,
          drawing_group_incomplete_property_tables,
        );
      }
    });
    for child in &children.records {
      if child.header.record_type != 0xf001 {
        continue;
      }
      let PptRecordData::Container(file_blocks) = &child.data else {
        continue;
      };
      blip_stores.push(OfficeArtGraphBlipStoreInput {
        declared_entry_count: child.header.instance,
        entries: file_blocks
          .records
          .iter()
          .map(|file_block| OfficeArtGraphBlipStoreEntryInput {
            record_type: file_block.header.record_type,
            fbse: match &file_block.data {
              PptRecordData::OfficeArt(value) => match &value.data {
                OfficeArtRecordData::Fbse(value) => Some((
                  value.reference_count,
                  value.delay_offset,
                  value.embedded_blip.is_some(),
                )),
                _ => None,
              },
              _ => None,
            },
          })
          .collect(),
      });
    }
  } else if record.header.record_type == 0xf002 {
    let mut fdg_records = Vec::new();
    let mut shapes = Vec::new();
    let mut blip_references = Vec::new();
    let mut incomplete_property_tables = Vec::new();
    let mut property_table_index = 0usize;
    children.visit(&mut |child| {
      if let PptRecordData::OfficeArt(value) = &child.data {
        match &value.data {
          OfficeArtRecordData::Drawing(value) => {
            fdg_records.push((child.header.instance, *value));
          }
          OfficeArtRecordData::Shape(value) => shapes.push(*value),
          _ => {}
        }
        collect_office_art_record_blip_references(
          value,
          fdg_records.first().map(|(drawing_id, _)| *drawing_id),
          &mut property_table_index,
          &mut blip_references,
          &mut incomplete_property_tables,
        );
      }
    });
    let [(drawing_id, drawing)] = fdg_records.as_slice() else {
      return Err(Error::invalid(
        record.offset,
        format!(
          "PPT OfficeArtDgContainer contains {} OfficeArtFDG records, expected 1",
          fdg_records.len()
        ),
      ));
    };
    for reference in &mut blip_references {
      reference.drawing_id = Some(*drawing_id);
    }
    drawings.push(OfficeArtGraphDrawingInput {
      drawing_id: *drawing_id,
      drawing: *drawing,
      shapes,
      blip_references,
      incomplete_property_tables,
    });
  }
  for child in &children.records {
    collect_ppt_office_art_drawing_components(
      child,
      drawing_groups,
      blip_stores,
      drawing_group_blip_references,
      drawing_group_incomplete_property_tables,
      drawings,
    )?;
  }
  Ok(())
}

fn collect_ppt_office_art_blip_stores<'a>(
  record: &'a PptRecord,
  stores: &mut Vec<&'a [PptRecord]>,
) {
  let Some(children) = ppt_record_children(record) else {
    return;
  };
  if record.header.record_type == 0xf001 {
    stores.push(&children.records);
    return;
  }
  for child in &children.records {
    collect_ppt_office_art_blip_stores(child, stores);
  }
}

fn resolve_delay_image<'a>(
  pictures: Option<&'a PicturesStream>,
  delay_offset: u32,
  blip_identifier: u32,
) -> Result<Option<(usize, crate::office_art::OfficeArtImageRef<'a>, bool)>> {
  let (records, compatible_stream) = match pictures {
    Some(PicturesStream::Complete(stream)) => (stream.records.as_slice(), false),
    Some(PicturesStream::Compatibility { stream, .. }) => (stream.records.as_slice(), true),
    Some(PicturesStream::Partial(_)) | None => return Ok(None),
  };
  let mut offset = 0u32;
  for (record_index, record) in records.iter().enumerate() {
    if offset == delay_offset {
      return Ok(
        record
          .image_ref()
          .map(|image| (record_index, image, compatible_stream)),
      );
    }
    offset = offset
      .checked_add(HEADER_LEN as u32)
      .and_then(|value| value.checked_add(record.header.declared_length))
      .ok_or_else(|| {
        Error::Limit(format!(
          "PPT Pictures offset overflow while resolving BLIP {blip_identifier}"
        ))
      })?;
  }
  Ok(None)
}

impl PptTopLevelRecordKind {
  fn from_data(data: &PptRecordData) -> Self {
    match data {
      PptRecordData::UserEdit(_) => Self::UserEdit,
      PptRecordData::PersistDirectory(_) => Self::PersistDirectory,
      _ => Self::Other,
    }
  }
}

impl PptRecordHeader {
  fn from_bytes(bytes: &[u8]) -> Self {
    debug_assert_eq!(bytes.len(), HEADER_LEN);
    let wire = parse_fixed::<PptRecordHeaderWire>(bytes)
      .expect("an eight-byte PPT record header is a complete fixed layout");
    Self {
      version: wire.version_instance.version,
      instance: wire.version_instance.instance,
      record_type: wire.record_type,
      declared_length: wire.declared_length,
    }
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.write_to(bytes)
  }

  fn write_to<W: Write + ?Sized>(&self, writer: &mut W) -> Result<()> {
    if self.version > 0x0f {
      return Err(Error::invalid(0, "PPT record version exceeds four bits"));
    }
    if self.instance > 0x0fff {
      return Err(Error::invalid(0, "PPT record instance exceeds twelve bits"));
    }
    let version_instance = u16::from(self.version) | (self.instance << 4);
    writer.write_all(&version_instance.to_le_bytes())?;
    writer.write_all(&self.record_type.to_le_bytes())?;
    writer.write_all(&self.declared_length.to_le_bytes())?;
    Ok(())
  }
}

impl PptRecord {
  fn direct_body_sequence(&self) -> Result<Option<&PptRecordSequence>> {
    match &self.data {
      PptRecordData::Container(children) => {
        if self.header.version != 0x0f {
          return Err(Error::invalid(0, "PPT container lacks recVer 0xF"));
        }
        Ok(Some(children))
      }
      PptRecordData::ProgBinaryTag(value) => {
        if self.header.record_type != PROG_BINARY_TAG || self.header.version != 0x0f {
          return Err(Error::invalid(0, "ProgBinaryTag header changed"));
        }
        Ok(Some(&value.records))
      }
      PptRecordData::ProgTags(children) => {
        if self.header.record_type != PROG_TAGS {
          return Err(Error::invalid(0, "ProgTags header changed"));
        }
        Ok(Some(children))
      }
      PptRecordData::BinaryTagData(BinaryTagData::Records(children)) => {
        if self.header.record_type != BINARY_TAG_DATA_BLOB || self.header.version == 0x0f {
          return Err(Error::invalid(0, "BinaryTagDataBlob header changed"));
        }
        Ok(Some(children))
      }
      _ => Ok(None),
    }
  }

  fn body_len_from_layout(&self) -> Result<usize> {
    if let Some(children) = self.direct_body_sequence()? {
      children.serialized_len_from_layout()
    } else if let PptRecordData::Truncated(bytes) = &self.data {
      Ok(bytes.len())
    } else {
      usize::try_from(self.header.declared_length)
        .map_err(|_| Error::Limit("PPT record length exceeds usize".into()))
    }
  }

  fn body_len_for_relayout(&self) -> Result<usize> {
    if let Some(children) = self.direct_body_sequence()? {
      children.serialized_len_from_layout()
    } else {
      Ok(self.body_bytes()?.len())
    }
  }

  fn validate_body_len(&self, body_len: usize) -> Result<()> {
    let declared = usize::try_from(self.header.declared_length)
      .map_err(|_| Error::Limit("PPT record length exceeds usize".into()))?;
    match self.data {
      PptRecordData::Truncated(_) if body_len >= declared => Err(Error::invalid(
        0,
        "truncated PPT record is no longer shorter than declared",
      )),
      PptRecordData::Truncated(_) => Ok(()),
      _ if body_len != declared => Err(Error::invalid(0, "PPT record body length mismatch")),
      _ => Ok(()),
    }
  }

  fn relayout_children(&mut self, body_offset: u64, preserve_compatibility: bool) -> Result<()> {
    let body_offset = body_offset
      .checked_add(HEADER_LEN as u64)
      .ok_or_else(|| Error::Limit("PPT child record offset overflow".into()))?;
    match &mut self.data {
      PptRecordData::CompatibilityTextChars(_) | PptRecordData::CompatibilityCString(_)
        if !preserve_compatibility =>
      {
        return Err(Error::invalid(
          self.offset,
          "PPT invalid UTF-16 requires compatibility-preserving save",
        ));
      }
      PptRecordData::Container(children) => {
        children.relayout(body_offset, None, preserve_compatibility)?;
        if self.header.record_type == 0xf001 && !preserve_compatibility {
          if children.records.iter().any(|child| {
            child.header.record_type != 0xf007
              && !(0xf018..=0xf117).contains(&child.header.record_type)
          }) {
            return Err(Error::invalid(
              self.offset,
              "OfficeArtBStoreContainer contains an invalid file-block record type",
            ));
          }
          if children.records.len() > 0x0fff {
            return Err(Error::Limit(
              "OfficeArtBStoreContainer file-block count exceeds recInstance".into(),
            ));
          }
          self.header.instance = children.records.len() as u16;
        }
      }
      PptRecordData::ProgTags(children)
      | PptRecordData::BinaryTagData(BinaryTagData::Records(children)) => {
        children.relayout(body_offset, None, preserve_compatibility)?;
      }
      PptRecordData::ProgBinaryTag(value) => {
        value
          .records
          .relayout(body_offset, None, preserve_compatibility)?;
      }
      PptRecordData::OfficeArt(value) => {
        if value.header.version != self.header.version
          || value.header.instance != self.header.instance
          || value.header.record_type != self.header.record_type
        {
          return Err(Error::invalid(
            self.offset,
            "embedded OfficeArt record header changed",
          ));
        }
        value.relayout()?;
        self.header.instance = value.header.instance;
        self.header.declared_length = value.header.declared_length;
      }
      PptRecordData::BlipEntity9(value) => value.blip.relayout()?,
      _ => {}
    }
    Ok(())
  }

  fn borrowed_body_bytes(&self) -> Result<Option<&[u8]>> {
    macro_rules! checked {
      ($condition:expr, $message:literal, $bytes:expr) => {{
        if !$condition {
          return Err(Error::invalid(0, $message));
        }
        Some($bytes)
      }};
    }
    Ok(match &self.data {
      PptRecordData::BinaryTagData(BinaryTagData::Opaque(bytes)) => checked!(
        self.header.record_type == BINARY_TAG_DATA_BLOB && self.header.version != 0x0f,
        "BinaryTagDataBlob header changed",
        bytes.as_slice()
      ),
      PptRecordData::MalformedStyleTextProp(value) => checked!(
        self.header.record_type == STYLE_TEXT_PROP_ATOM,
        "malformed StyleTextPropAtom header changed",
        value.body.as_slice()
      ),
      PptRecordData::UnresolvedStyleTextProp(bytes) => checked!(
        self.header.record_type == STYLE_TEXT_PROP_ATOM,
        "unresolved StyleTextPropAtom header changed",
        bytes.as_slice()
      ),
      PptRecordData::MalformedTextMasterStyle(bytes) => checked!(
        self.header.record_type == TEXT_MASTER_STYLE_ATOM,
        "malformed TextMasterStyleAtom header changed",
        bytes.as_slice()
      ),
      PptRecordData::MalformedTextRuler(bytes) => checked!(
        matches!(
          self.header.record_type,
          TEXT_RULER_ATOM | DEFAULT_RULER_ATOM
        ),
        "malformed TextRulerAtom header changed",
        bytes.as_slice()
      ),
      PptRecordData::MalformedTextSpecialInfo(bytes) => checked!(
        self.header.record_type == TEXT_SPECIAL_INFO_ATOM,
        "malformed TextSpecialInfoAtom header changed",
        bytes.as_slice()
      ),
      PptRecordData::MalformedStyleTextProp9(bytes) => checked!(
        self.header.record_type == STYLE_TEXT_PROP9_ATOM,
        "malformed StyleTextProp9Atom header changed",
        bytes.as_slice()
      ),
      PptRecordData::FontEmbedDataBlob(bytes) => checked!(
        self.header.record_type == FONT_EMBED_DATA_BLOB,
        "FontEmbedDataBlob header changed",
        bytes.as_slice()
      ),
      PptRecordData::EnvelopeData9(bytes) => checked!(
        self.header.record_type == ENVELOPE_DATA9_ATOM,
        "EnvelopeData9Atom header changed",
        bytes.as_slice()
      ),
      PptRecordData::MalformedTimeVariant(bytes) => checked!(
        self.header.record_type == TIME_VARIANT_ATOM,
        "malformed TimeVariant header changed",
        bytes.as_slice()
      ),
      PptRecordData::MalformedBlipEntity9 { body, .. } => checked!(
        self.header.record_type == BLIP_ENTITY9_ATOM,
        "malformed BlipEntity9Atom header changed",
        body.as_slice()
      ),
      PptRecordData::RoundTripAnimation12(value) => checked!(
        self.header.record_type == ROUND_TRIP_ANIMATION_12_ATOM,
        "RoundTripAnimationAtom header changed",
        value.package.physical_bytes.as_slice()
      ),
      PptRecordData::SoundDataBlob(bytes) => checked!(
        self.header.record_type == SOUND_DATA_BLOB,
        "SoundDataBlob header changed",
        bytes.as_slice()
      ),
      PptRecordData::RoundTripContentMasterInfo12(value) => checked!(
        self.header.record_type == ROUND_TRIP_CONTENT_MASTER_INFO_12_ATOM
          && self.header.instance == value.layout_index,
        "RoundTripContentMasterInfo12Atom header changed",
        value.package.physical_bytes.as_slice()
      ),
      PptRecordData::RoundTripColorMapping12(value) => checked!(
        self.header.record_type == ROUND_TRIP_COLOR_MAPPING_12_ATOM,
        "RoundTripColorMapping12Atom header changed",
        value.physical_xml.as_slice()
      ),
      PptRecordData::RoundTripTheme12(value) => checked!(
        self.header.record_type == ROUND_TRIP_THEME_12_ATOM && self.header.instance == 0,
        "RoundTripTheme12Atom header changed",
        value.package.physical_bytes.as_slice()
      ),
      PptRecordData::RoundTripStyle12(value) => checked!(
        self.header.record_type == value.record_type
          && matches!(
            value.record_type,
            ROUND_TRIP_OART_TEXT_STYLES_12_ATOM
              | ROUND_TRIP_NOTES_MASTER_TEXT_STYLES_12_ATOM
              | ROUND_TRIP_CUSTOM_TABLE_STYLES_12_ATOM
          ),
        "RoundTripStyle12Atom header changed",
        value.package.physical_bytes.as_slice()
      ),
      PptRecordData::Unknown(value) => checked!(
        self.header.record_type == value.record_type,
        "unknown record header changed",
        value.body.as_slice()
      ),
      PptRecordData::MalformedSpecRecord(value) => checked!(
        self.header.record_type == value.record_type && is_ms_ppt_record_type(value.record_type),
        "malformed MS-PPT record header changed",
        value.body.as_slice()
      ),
      PptRecordData::Truncated(bytes) => Some(bytes.as_slice()),
      _ => None,
    })
  }

  fn body_bytes(&self) -> Result<Vec<u8>> {
    macro_rules! fixed_record_body {
      ($expected:expr, $value:expr, $message:literal) => {{
        if self.header.record_type != $expected {
          return Err(Error::invalid(0, $message));
        }
        write_fixed($value)?
      }};
    }
    let body = match &self.data {
      PptRecordData::Container(children) => {
        if self.header.version != 0x0f {
          return Err(Error::invalid(0, "PPT container lacks recVer 0xF"));
        }
        children.to_bytes()?
      }
      PptRecordData::ProgBinaryTag(value) => {
        if self.header.record_type != PROG_BINARY_TAG || self.header.version != 0x0f {
          return Err(Error::invalid(0, "ProgBinaryTag header changed"));
        }
        value.records.to_bytes()?
      }
      PptRecordData::ProgTags(children) => {
        if self.header.record_type != PROG_TAGS {
          return Err(Error::invalid(0, "ProgTags header changed"));
        }
        children.to_bytes()?
      }
      PptRecordData::BinaryTagData(data) => {
        if self.header.record_type != BINARY_TAG_DATA_BLOB || self.header.version == 0x0f {
          return Err(Error::invalid(0, "BinaryTagDataBlob header changed"));
        }
        match data {
          BinaryTagData::Records(children) => children.to_bytes()?,
          BinaryTagData::Opaque(bytes) => bytes.clone(),
        }
      }
      PptRecordData::UserEdit(value) => {
        if self.header.record_type != USER_EDIT_ATOM || self.header.version == 0x0f {
          return Err(Error::invalid(0, "UserEditAtom record header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::Document(value) => {
        if self.header.record_type != DOCUMENT_ATOM || self.header.version == 0x0f {
          return Err(Error::invalid(0, "DocumentAtom record header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::Slide(value) => {
        if self.header.record_type != SLIDE_ATOM || self.header.version == 0x0f {
          return Err(Error::invalid(0, "SlideAtom record header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::Notes(value) => {
        if self.header.record_type != NOTES_ATOM || self.header.version == 0x0f {
          return Err(Error::invalid(0, "NotesAtom record header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::OutlineTextRef(value) => {
        if self.header.record_type != OUTLINE_TEXT_REF_ATOM {
          return Err(Error::invalid(0, "OutlineTextRefAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TextHeader(value) => {
        if self.header.record_type != TEXT_HEADER_ATOM {
          return Err(Error::invalid(0, "TextHeaderAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TextChars(values) => {
        if self.header.record_type != TEXT_CHARS_ATOM {
          return Err(Error::invalid(0, "TextCharsAtom header changed"));
        }
        write_utf16_string(values)
      }
      PptRecordData::TextBytes(values) => {
        if self.header.record_type != TEXT_BYTES_ATOM {
          return Err(Error::invalid(0, "TextBytesAtom header changed"));
        }
        write_byte_string(values)?
      }
      PptRecordData::CompatibilityTextChars(values) => {
        if self.header.record_type != TEXT_CHARS_ATOM {
          return Err(Error::invalid(0, "compatible TextCharsAtom header changed"));
        }
        write_utf16(values)
      }
      PptRecordData::StyleTextProp(value) => {
        if self.header.record_type != STYLE_TEXT_PROP_ATOM {
          return Err(Error::invalid(0, "StyleTextPropAtom header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::MalformedStyleTextProp(value) => {
        if self.header.record_type != STYLE_TEXT_PROP_ATOM {
          return Err(Error::invalid(
            0,
            "malformed StyleTextPropAtom header changed",
          ));
        }
        value.body.clone()
      }
      PptRecordData::UnresolvedStyleTextProp(value) => {
        if self.header.record_type != STYLE_TEXT_PROP_ATOM {
          return Err(Error::invalid(
            0,
            "unresolved StyleTextPropAtom header changed",
          ));
        }
        value.clone()
      }
      PptRecordData::CString(values) => {
        if self.header.record_type != C_STRING_ATOM {
          return Err(Error::invalid(0, "CString header changed"));
        }
        write_utf16_string(values)
      }
      PptRecordData::CompatibilityCString(values) => {
        if self.header.record_type != C_STRING_ATOM {
          return Err(Error::invalid(0, "compatible CString header changed"));
        }
        write_utf16(values)
      }
      PptRecordData::SlidePersist(value) => {
        if self.header.record_type != SLIDE_PERSIST_ATOM {
          return Err(Error::invalid(0, "SlidePersistAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::ColorScheme(value) => {
        if self.header.record_type != COLOR_SCHEME_ATOM {
          return Err(Error::invalid(0, "ColorSchemeAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::ExternalObjectRef(value) => {
        if self.header.record_type != EXTERNAL_OBJECT_REF_ATOM {
          return Err(Error::invalid(0, "ExternalObjectRefAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::Placeholder(value) => {
        if self.header.record_type != PLACEHOLDER_ATOM {
          return Err(Error::invalid(0, "PlaceholderAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::HeadersFooters(value) => {
        if self.header.record_type != HEADERS_FOOTERS_ATOM {
          return Err(Error::invalid(0, "HeadersFootersAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::MasterTextProp(value) => {
        if self.header.record_type != MASTER_TEXT_PROP_ATOM {
          return Err(Error::invalid(0, "MasterTextPropAtom header changed"));
        }
        let mut bytes = Vec::new();
        for run in &value.runs {
          bytes.extend_from_slice(&write_fixed(run)?);
        }
        bytes
      }
      PptRecordData::TextMasterStyle(value) => {
        if self.header.record_type != TEXT_MASTER_STYLE_ATOM
          || self.header.instance != value.text_type
        {
          return Err(Error::invalid(0, "TextMasterStyleAtom header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::MalformedTextMasterStyle(bytes) => {
        if self.header.record_type != TEXT_MASTER_STYLE_ATOM {
          return Err(Error::invalid(
            0,
            "malformed TextMasterStyleAtom header changed",
          ));
        }
        bytes.clone()
      }
      PptRecordData::TextCfException(value) => {
        if self.header.record_type != TEXT_CF_EXCEPTION_ATOM {
          return Err(Error::invalid(0, "TextCFExceptionAtom header changed"));
        }
        let mut bytes = Vec::new();
        value.write(&mut bytes)?;
        bytes
      }
      PptRecordData::TextPfException(value) => {
        if self.header.record_type != TEXT_PF_EXCEPTION_ATOM {
          return Err(Error::invalid(0, "TextPFExceptionAtom header changed"));
        }
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&value.reserved.to_le_bytes());
        value.paragraph.write(&mut bytes)?;
        bytes
      }
      PptRecordData::TextSiException(value) => {
        if self.header.record_type != TEXT_SI_EXCEPTION_ATOM {
          return Err(Error::invalid(0, "TextSIExceptionAtom header changed"));
        }
        let mut bytes = Vec::new();
        value.write(&mut bytes)?;
        bytes
      }
      PptRecordData::TextRuler(value) => {
        if !matches!(
          self.header.record_type,
          TEXT_RULER_ATOM | DEFAULT_RULER_ATOM
        ) {
          return Err(Error::invalid(0, "TextRulerAtom header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::MalformedTextRuler(bytes) => {
        if !matches!(
          self.header.record_type,
          TEXT_RULER_ATOM | DEFAULT_RULER_ATOM
        ) {
          return Err(Error::invalid(0, "malformed TextRulerAtom header changed"));
        }
        bytes.clone()
      }
      PptRecordData::TextSpecialInfo(value) => {
        if self.header.record_type != TEXT_SPECIAL_INFO_ATOM {
          return Err(Error::invalid(0, "TextSpecialInfoAtom header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::MalformedTextSpecialInfo(bytes) => {
        if self.header.record_type != TEXT_SPECIAL_INFO_ATOM {
          return Err(Error::invalid(
            0,
            "malformed TextSpecialInfoAtom header changed",
          ));
        }
        bytes.clone()
      }
      PptRecordData::StyleTextProp9(value) => {
        if self.header.record_type != STYLE_TEXT_PROP9_ATOM {
          return Err(Error::invalid(0, "StyleTextProp9Atom header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::MalformedStyleTextProp9(bytes) => {
        if self.header.record_type != STYLE_TEXT_PROP9_ATOM {
          return Err(Error::invalid(
            0,
            "malformed StyleTextProp9Atom header changed",
          ));
        }
        bytes.clone()
      }
      PptRecordData::TextMasterStyle9(value) => {
        if self.header.record_type != TEXT_MASTER_STYLE9_ATOM
          || self.header.instance != value.text_type
        {
          return Err(Error::invalid(0, "TextMasterStyle9Atom header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::StyleTextProp10(value) => {
        if self.header.record_type != STYLE_TEXT_PROP10_ATOM {
          return Err(Error::invalid(0, "StyleTextProp10Atom header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::TextMasterStyle10(value) => {
        if self.header.record_type != TEXT_MASTER_STYLE10_ATOM
          || self.header.instance != value.text_type
        {
          return Err(Error::invalid(0, "TextMasterStyle10Atom header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::TextDefaults10(value) => {
        if self.header.record_type != TEXT_DEFAULTS10_ATOM {
          return Err(Error::invalid(0, "TextDefaults10Atom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::StyleTextProp11(value) => {
        if self.header.record_type != STYLE_TEXT_PROP11_ATOM {
          return Err(Error::invalid(0, "StyleTextProp11Atom header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::RecolorInfo(value) => {
        if self.header.record_type != RECOLOR_INFO_ATOM {
          return Err(Error::invalid(0, "RecolorInfoAtom header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::MacPrintSettings(value) => {
        if self.header.record_type != MAC_PRINT_SETTINGS_ATOM {
          return Err(Error::invalid(0, "MacPrintSettings plist header changed"));
        }
        value.physical_xml.clone()
      }
      PptRecordData::MacPageFormat(value) => {
        if self.header.record_type != MAC_PAGE_FORMAT_ATOM {
          return Err(Error::invalid(0, "MacPageFormat plist header changed"));
        }
        value.physical_xml.clone()
      }
      PptRecordData::Ppt11FontDescriptors(value) => {
        if self.header.record_type != PPT11_FONT_DESCRIPTOR_ATOM {
          return Err(Error::invalid(0, "PPT11 font descriptor header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::Ppt11FontDescriptorCollection(value) => {
        if self.header.record_type != PPT11_FONT_DESCRIPTOR_COLLECTION_ATOM {
          return Err(Error::invalid(
            0,
            "PPT11 font descriptor collection header changed",
          ));
        }
        value.to_bytes()?
      }
      PptRecordData::Ppt10Reserved(value) => {
        if self.header.record_type != PPT10_RESERVED_ATOM {
          return Err(Error::invalid(0, "PPT10 reserved atom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::MacLegacyPrintInfo(value) => {
        if self.header.record_type != MAC_LEGACY_PRINT_INFO_ATOM {
          return Err(Error::invalid(0, "Mac legacy print info header changed"));
        }
        value.bytes.to_vec()
      }
      PptRecordData::MacPrintDriverInfo(value) => {
        if self.header.record_type != MAC_PRINT_DRIVER_INFO_ATOM {
          return Err(Error::invalid(0, "Mac print driver info header changed"));
        }
        value.bytes.to_vec()
      }
      PptRecordData::HandoutCompatibility(value) => {
        if self.header.record_type != HANDOUT_COMPATIBILITY_ATOM {
          return Err(Error::invalid(0, "handout compatibility header changed"));
        }
        value.bytes.to_vec()
      }
      PptRecordData::NamedShowSlides(values) => {
        if self.header.record_type != NAMED_SHOW_SLIDES_ATOM {
          return Err(Error::invalid(0, "NamedShowSlidesAtom header changed"));
        }
        let mut body = Vec::with_capacity(values.len() * 4);
        for value in values {
          body.extend_from_slice(&value.to_le_bytes());
        }
        body
      }
      PptRecordData::BookmarkSeed(value) => {
        fixed_record_body!(BOOKMARK_SEED_ATOM, value, "BookmarkSeedAtom header changed")
      }
      PptRecordData::ShapeFlags(value) => {
        fixed_record_body!(SHAPE_ATOM, value, "ShapeFlagsAtom header changed")
      }
      PptRecordData::ShapeFlags10(value) => {
        fixed_record_body!(SHAPE_FLAGS10_ATOM, value, "ShapeFlags10Atom header changed")
      }
      PptRecordData::RoundTripNewPlaceholderId12(value) => fixed_record_body!(
        ROUND_TRIP_NEW_PLACEHOLDER_ID_12_ATOM,
        value,
        "RoundTripNewPlaceholderId12Atom header changed"
      ),
      PptRecordData::FontEmbedDataBlob(value) => {
        if self.header.record_type != FONT_EMBED_DATA_BLOB {
          return Err(Error::invalid(0, "FontEmbedDataBlob header changed"));
        }
        value.clone()
      }
      PptRecordData::BookmarkEntity(value) => fixed_record_body!(
        BOOKMARK_ENTITY_ATOM,
        value,
        "BookmarkEntityAtom header changed"
      ),
      PptRecordData::RtfDateTimeMeta(value) => fixed_record_body!(
        RTF_DATE_TIME_META_CHARACTER_ATOM,
        value,
        "RTFDateTimeMCAtom header changed"
      ),
      PptRecordData::ChartBuild(value) => {
        fixed_record_body!(CHART_BUILD_ATOM, value, "ChartBuildAtom header changed")
      }
      PptRecordData::DiagramBuild(value) => {
        fixed_record_body!(DIAGRAM_BUILD_ATOM, value, "DiagramBuildAtom header changed")
      }
      PptRecordData::LinkedShape10(value) => fixed_record_body!(
        LINKED_SHAPE10_ATOM,
        value,
        "LinkedShape10Atom header changed"
      ),
      PptRecordData::LinkedSlide10(value) => fixed_record_body!(
        LINKED_SLIDE10_ATOM,
        value,
        "LinkedSlide10Atom header changed"
      ),
      PptRecordData::Diff10(value) => {
        fixed_record_body!(DIFF10_ATOM, value, "Diff10Atom header changed")
      }
      PptRecordData::SlideListTableSize10(value) => fixed_record_body!(
        SLIDE_LIST_TABLE_SIZE10_ATOM,
        value,
        "SlideListTableSize10Atom header changed"
      ),
      PptRecordData::SlideListEntry10(value) => fixed_record_body!(
        SLIDE_LIST_ENTRY10_ATOM,
        value,
        "SlideListEntry10Atom header changed"
      ),
      PptRecordData::FontEmbedFlags10(value) => fixed_record_body!(
        FONT_EMBED_FLAGS10_ATOM,
        value,
        "FontEmbedFlags10Atom header changed"
      ),
      PptRecordData::PhotoAlbumInfo10(value) => fixed_record_body!(
        PHOTO_ALBUM_INFO10_ATOM,
        value,
        "PhotoAlbumInfo10Atom header changed"
      ),
      PptRecordData::TimeIterateData(value) => fixed_record_body!(
        TIME_ITERATE_DATA_ATOM,
        value,
        "TimeIterateDataAtom header changed"
      ),
      PptRecordData::TextDefaults9(value) => {
        if self.header.record_type != TEXT_DEFAULTS9_ATOM {
          return Err(Error::invalid(0, "TextDefaults9Atom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::ExternalOleLink(value) => fixed_record_body!(
        EXTERNAL_OLE_LINK_ATOM,
        value,
        "ExOleLinkAtom header changed"
      ),
      PptRecordData::ExternalOleControl(value) => fixed_record_body!(
        EXTERNAL_OLE_CONTROL_ATOM,
        value,
        "ExControlAtom header changed"
      ),
      PptRecordData::ExternalCdAudio(value) => fixed_record_body!(
        EXTERNAL_CD_AUDIO_ATOM,
        value,
        "ExCDAudioAtom header changed"
      ),
      PptRecordData::BroadcastDocInfo9(value) => fixed_record_body!(
        BROADCAST_DOC_INFO9_ATOM,
        value,
        "BroadcastDocInfo9Atom header changed"
      ),
      PptRecordData::EnvelopeFlags9(value) => fixed_record_body!(
        ENVELOPE_FLAGS9_ATOM,
        value,
        "EnvelopeFlags9Atom header changed"
      ),
      PptRecordData::EnvelopeData9(value) => {
        if self.header.record_type != ENVELOPE_DATA9_ATOM {
          return Err(Error::invalid(0, "EnvelopeData9Atom header changed"));
        }
        value.clone()
      }
      PptRecordData::DocRoutingSlip(value) => {
        if self.header.record_type != DOC_ROUTING_SLIP_ATOM {
          return Err(Error::invalid(0, "DocRoutingSlipAtom header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::Metafile(value) => {
        if self.header.record_type != METAFILE_BLOB {
          return Err(Error::invalid(0, "MetafileBlob header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::RoundTripSlideSyncInfo12(value) => fixed_record_body!(
        ROUND_TRIP_SLIDE_SYNC_INFO12_ATOM,
        value,
        "SlideSyncInfoAtom12 header changed"
      ),
      PptRecordData::TimeColorBehavior(value) => {
        if self.header.record_type != TIME_COLOR_BEHAVIOR_ATOM {
          return Err(Error::invalid(0, "TimeColorBehaviorAtom header changed"));
        }
        value.to_bytes()
      }
      PptRecordData::TimeRotationBehavior(value) => fixed_record_body!(
        TIME_ROTATION_BEHAVIOR_ATOM,
        value,
        "TimeRotationBehaviorAtom header changed"
      ),
      PptRecordData::TimeNode(value) => {
        if self.header.record_type != TIME_NODE_ATOM {
          return Err(Error::invalid(0, "TimeNodeAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TimeCondition(value) => {
        if self.header.record_type != TIME_CONDITION_ATOM {
          return Err(Error::invalid(0, "TimeConditionAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TimeModifier(value) => {
        if self.header.record_type != TIME_MODIFIER_ATOM {
          return Err(Error::invalid(0, "TimeModifierAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TimeBehavior(value) => {
        if self.header.record_type != TIME_BEHAVIOR_ATOM {
          return Err(Error::invalid(0, "TimeBehaviorAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TimeAnimateBehavior(value) => {
        if self.header.record_type != TIME_ANIMATE_BEHAVIOR_ATOM {
          return Err(Error::invalid(0, "TimeAnimateBehaviorAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TimeEffectBehavior(value) => {
        if self.header.record_type != TIME_EFFECT_BEHAVIOR_ATOM {
          return Err(Error::invalid(0, "TimeEffectBehaviorAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TimeMotionBehavior(value) => {
        if self.header.record_type != TIME_MOTION_BEHAVIOR_ATOM {
          return Err(Error::invalid(0, "TimeMotionBehaviorAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TimeScaleBehavior(value) => {
        if self.header.record_type != TIME_SCALE_BEHAVIOR_ATOM {
          return Err(Error::invalid(0, "TimeScaleBehaviorAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TimeSetBehavior(value) => {
        if self.header.record_type != TIME_SET_BEHAVIOR_ATOM {
          return Err(Error::invalid(0, "TimeSetBehaviorAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TimeCommandBehavior(value) => {
        if self.header.record_type != TIME_COMMAND_BEHAVIOR_ATOM {
          return Err(Error::invalid(0, "TimeCommandBehaviorAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TimeSequenceData(value) => {
        if self.header.record_type != TIME_SEQUENCE_DATA_ATOM {
          return Err(Error::invalid(0, "TimeSequenceDataAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TimeAnimationValue(value) => {
        if self.header.record_type != TIME_ANIMATION_VALUE_ATOM {
          return Err(Error::invalid(0, "TimeAnimationValueAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TimeVariant(value) => {
        if self.header.record_type != TIME_VARIANT_ATOM {
          return Err(Error::invalid(0, "TimeVariant header changed"));
        }
        value.to_bytes()
      }
      PptRecordData::MalformedTimeVariant(bytes) => {
        if self.header.record_type != TIME_VARIANT_ATOM {
          return Err(Error::invalid(0, "malformed TimeVariant header changed"));
        }
        bytes.clone()
      }
      PptRecordData::VisualShape(value) => {
        if self.header.record_type != VISUAL_SHAPE_ATOM {
          return Err(Error::invalid(0, "VisualShapeAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::HashCode(value) => {
        if self.header.record_type != HASH_CODE_ATOM {
          return Err(Error::invalid(0, "HashCodeAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::VisualPage(value) => {
        if self.header.record_type != VISUAL_PAGE_ATOM {
          return Err(Error::invalid(0, "VisualPageAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::Build(value) => {
        if self.header.record_type != BUILD_ATOM {
          return Err(Error::invalid(0, "BuildAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::ParaBuild(value) => {
        if self.header.record_type != PARA_BUILD_ATOM {
          return Err(Error::invalid(0, "ParaBuildAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::LevelInfo(value) => {
        if self.header.record_type != LEVEL_INFO_ATOM {
          return Err(Error::invalid(0, "LevelInfoAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::SlideTime10(value) => {
        if self.header.record_type != SLIDE_TIME_10_ATOM {
          return Err(Error::invalid(0, "SlideTime10Atom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::FontEntity(value) => {
        if self.header.record_type != FONT_ENTITY_ATOM {
          return Err(Error::invalid(0, "FontEntityAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::ExternalOleObject(value) => {
        if self.header.record_type != EXTERNAL_OLE_OBJECT_ATOM {
          return Err(Error::invalid(0, "ExOleObjAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::ExternalOleEmbed(value) => {
        if self.header.record_type != EXTERNAL_OLE_EMBED_ATOM {
          return Err(Error::invalid(0, "ExOleEmbedAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::Kinsoku(value) => {
        if self.header.record_type != KINSOKU_ATOM {
          return Err(Error::invalid(0, "KinsokuAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::ExternalHyperlinkId(value) => {
        if self.header.record_type != EXTERNAL_HYPERLINK_ATOM {
          return Err(Error::invalid(0, "ExHyperlinkAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::ExternalHyperlinkFlags(value) => {
        if self.header.record_type != EXTERNAL_HYPERLINK_FLAGS_ATOM {
          return Err(Error::invalid(0, "ExHyperlinkFlagsAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::SlideNumberMeta(value) => {
        if self.header.record_type != SLIDE_NUMBER_META_CHARACTER_ATOM {
          return Err(Error::invalid(0, "SlideNumberMCAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::TextInteractiveInfo(value) => {
        if self.header.record_type != TEXT_INTERACTIVE_INFO_ATOM {
          return Err(Error::invalid(0, "TextInteractiveInfoAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::AnimationInfo(value) => {
        if self.header.record_type != ANIMATION_INFO_ATOM {
          return Err(Error::invalid(0, "AnimationInfoAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::InteractiveInfo(value) => {
        if self.header.record_type != INTERACTIVE_INFO_ATOM {
          return Err(Error::invalid(0, "InteractiveInfoAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::DateTimeMeta(value) => {
        if self.header.record_type != DATE_TIME_META_CHARACTER_ATOM {
          return Err(Error::invalid(0, "DateTimeMCAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::GenericDateMeta(value) => {
        if self.header.record_type != GENERIC_DATE_META_CHARACTER_ATOM {
          return Err(Error::invalid(0, "GenericDateMCAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::HeaderMeta(value) => {
        if self.header.record_type != HEADER_META_CHARACTER_ATOM {
          return Err(Error::invalid(0, "HeaderMCAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::FooterMeta(value) => {
        if self.header.record_type != FOOTER_META_CHARACTER_ATOM {
          return Err(Error::invalid(0, "FooterMCAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::ViewInfo(value) => {
        if self.header.record_type != VIEW_INFO_ATOM {
          return Err(Error::invalid(0, "ViewInfoAtom header changed"));
        }
        write_fixed(value)?
      }
      PptRecordData::BlipEntity9(value) => {
        if self.header.record_type != BLIP_ENTITY9_ATOM {
          return Err(Error::invalid(0, "BlipEntity9Atom header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::MalformedBlipEntity9 { body, .. } => {
        if self.header.record_type != BLIP_ENTITY9_ATOM {
          return Err(Error::invalid(
            0,
            "malformed BlipEntity9Atom header changed",
          ));
        }
        body.clone()
      }
      PptRecordData::RoundTripAnimation12(value) => {
        if self.header.record_type != ROUND_TRIP_ANIMATION_12_ATOM {
          return Err(Error::invalid(0, "RoundTripAnimationAtom header changed"));
        }
        value.package.physical_bytes.clone()
      }
      PptRecordData::RoundTripAnimationHash12(value) => {
        if self.header.record_type != ROUND_TRIP_ANIMATION_HASH_12_ATOM {
          return Err(Error::invalid(
            0,
            "RoundTripAnimationHashAtom header changed",
          ));
        }
        write_fixed(value)?
      }
      PptRecordData::SlideShowSlideInfo(value) => fixed_record_body!(
        SLIDE_SHOW_SLIDE_INFO_ATOM,
        value,
        "SlideShowSlideInfoAtom header changed"
      ),
      PptRecordData::Guide(value) => {
        fixed_record_body!(GUIDE_ATOM, value, "GuideAtom header changed")
      }
      PptRecordData::SlideViewInfo(value) => fixed_record_body!(
        SLIDE_VIEW_INFO_ATOM,
        value,
        "SlideViewInfoAtom header changed"
      ),
      PptRecordData::VbaInfo(value) => {
        fixed_record_body!(VBA_INFO_ATOM, value, "VBAInfoAtom header changed")
      }
      PptRecordData::SlideShowDocInfo(value) => fixed_record_body!(
        SLIDE_SHOW_DOC_INFO_ATOM,
        value,
        "SlideShowDocInfoAtom header changed"
      ),
      PptRecordData::ExternalObjectList(value) => fixed_record_body!(
        EXTERNAL_OBJECT_LIST_ATOM,
        value,
        "ExObjListAtom header changed"
      ),
      PptRecordData::GridSpacing10(value) => fixed_record_body!(
        GRID_SPACING_10_ATOM,
        value,
        "GridSpacing10Atom header changed"
      ),
      PptRecordData::NormalViewSetInfo9(value) => fixed_record_body!(
        NORMAL_VIEW_SET_INFO_9_ATOM,
        value,
        "NormalViewSetInfoAtom header changed"
      ),
      PptRecordData::RoundTripOriginalMainMasterId12(value) => fixed_record_body!(
        ROUND_TRIP_ORIGINAL_MAIN_MASTER_ID_12_ATOM,
        value,
        "RoundTripOriginalMainMasterId12Atom header changed"
      ),
      PptRecordData::RoundTripCompositeMasterId12(value) => fixed_record_body!(
        ROUND_TRIP_COMPOSITE_MASTER_ID_12_ATOM,
        value,
        "RoundTripCompositeMasterId12Atom header changed"
      ),
      PptRecordData::RoundTripShapeId12(value) => fixed_record_body!(
        ROUND_TRIP_SHAPE_ID_12_ATOM,
        value,
        "RoundTripShapeId12Atom header changed"
      ),
      PptRecordData::RoundTripHfPlaceholder12(value) => fixed_record_body!(
        ROUND_TRIP_HF_PLACEHOLDER_12_ATOM,
        value,
        "RoundTripHFPlaceholder12Atom header changed"
      ),
      PptRecordData::RoundTripContentMasterId12(value) => fixed_record_body!(
        ROUND_TRIP_CONTENT_MASTER_ID_12_ATOM,
        value,
        "RoundTripContentMasterId12Atom header changed"
      ),
      PptRecordData::RoundTripHeaderFooterDefaults12(value) => fixed_record_body!(
        ROUND_TRIP_HEADER_FOOTER_DEFAULTS_12_ATOM,
        value,
        "RoundTripHeaderFooterDefaults12Atom header changed"
      ),
      PptRecordData::RoundTripDocFlags12(value) => fixed_record_body!(
        ROUND_TRIP_DOC_FLAGS_12_ATOM,
        value,
        "RoundTripDocFlags12Atom header changed"
      ),
      PptRecordData::RoundTripShapeChecksum12(value) => fixed_record_body!(
        ROUND_TRIP_SHAPE_CHECKSUM_12_ATOM,
        value,
        "RoundTripShapeChecksum12Atom header changed"
      ),
      PptRecordData::EndDocument => {
        if self.header.record_type != END_DOCUMENT_ATOM {
          return Err(Error::invalid(0, "EndDocumentAtom header changed"));
        }
        Vec::new()
      }
      PptRecordData::SoundCollection(value) => fixed_record_body!(
        SOUND_COLLECTION_ATOM,
        value,
        "SoundCollectionAtom header changed"
      ),
      PptRecordData::SoundDataBlob(value) => {
        if self.header.record_type != SOUND_DATA_BLOB {
          return Err(Error::invalid(0, "SoundDataBlob header changed"));
        }
        value.clone()
      }
      PptRecordData::TextBookmark(value) => {
        fixed_record_body!(TEXT_BOOKMARK_ATOM, value, "TextBookmarkAtom header changed")
      }
      PptRecordData::OutlineTextPropsHeader9(value) => fixed_record_body!(
        OUTLINE_TEXT_PROPS_HEADER9_ATOM,
        value,
        "OutlineTextPropsHeaderExAtom header changed"
      ),
      PptRecordData::ExternalMedia(value) => {
        fixed_record_body!(EXTERNAL_MEDIA_ATOM, value, "ExMediaAtom header changed")
      }
      PptRecordData::ExternalWavAudioEmbedded(value) => fixed_record_body!(
        EXTERNAL_WAV_AUDIO_EMBEDDED_ATOM,
        value,
        "ExWAVAudioEmbeddedAtom header changed"
      ),
      PptRecordData::PrintOptions(value) => {
        fixed_record_body!(PRINT_OPTIONS_ATOM, value, "PrintOptionsAtom header changed")
      }
      PptRecordData::PresentationAdvisorFlags9(value) => fixed_record_body!(
        PRESENTATION_ADVISOR_FLAGS9_ATOM,
        value,
        "PresAdvisorFlags9Atom header changed"
      ),
      PptRecordData::HtmlDocInfo9(value) => fixed_record_body!(
        HTML_DOC_INFO9_ATOM,
        value,
        "HTMLDocInfo9Atom header changed"
      ),
      PptRecordData::HtmlPublishInfo(value) => fixed_record_body!(
        HTML_PUBLISH_INFO_ATOM,
        value,
        "HTMLPublishInfoAtom header changed"
      ),
      PptRecordData::Comment10(value) => {
        fixed_record_body!(COMMENT10_ATOM, value, "Comment10Atom header changed")
      }
      PptRecordData::CommentIndex10(value) => fixed_record_body!(
        COMMENT_INDEX10_ATOM,
        value,
        "CommentIndex10Atom header changed"
      ),
      PptRecordData::SlideFlags10(value) => {
        fixed_record_body!(SLIDE_FLAGS10_ATOM, value, "SlideFlags10Atom header changed")
      }
      PptRecordData::FilterPrivacyFlags10(value) => fixed_record_body!(
        FILTER_PRIVACY_FLAGS10_ATOM,
        value,
        "FilterPrivacyFlags10Atom header changed"
      ),
      PptRecordData::DocToolbarStates10(value) => fixed_record_body!(
        DOC_TOOLBAR_STATES10_ATOM,
        value,
        "DocToolbarStates10Atom header changed"
      ),
      PptRecordData::ExternalStorage(value) => {
        if self.header.record_type != EXTERNAL_OLE_OBJECT_STORAGE
          || self.header.instance != value.instance()
        {
          return Err(Error::invalid(0, "external storage record header changed"));
        }
        value.to_bytes()?
      }
      PptRecordData::RoundTripContentMasterInfo12(value) => {
        if self.header.record_type != ROUND_TRIP_CONTENT_MASTER_INFO_12_ATOM
          || self.header.instance != value.layout_index
        {
          return Err(Error::invalid(
            0,
            "RoundTripContentMasterInfo12Atom header changed",
          ));
        }
        value.package.physical_bytes.clone()
      }
      PptRecordData::RoundTripColorMapping12(value) => {
        if self.header.record_type != ROUND_TRIP_COLOR_MAPPING_12_ATOM {
          return Err(Error::invalid(
            0,
            "RoundTripColorMapping12Atom header changed",
          ));
        }
        value.physical_xml.clone()
      }
      PptRecordData::RoundTripTheme12(value) => {
        if self.header.record_type != ROUND_TRIP_THEME_12_ATOM || self.header.instance != 0 {
          return Err(Error::invalid(0, "RoundTripTheme12Atom header changed"));
        }
        value.package.physical_bytes.clone()
      }
      PptRecordData::RoundTripStyle12(value) => {
        if self.header.record_type != value.record_type
          || !matches!(
            value.record_type,
            ROUND_TRIP_OART_TEXT_STYLES_12_ATOM
              | ROUND_TRIP_NOTES_MASTER_TEXT_STYLES_12_ATOM
              | ROUND_TRIP_CUSTOM_TABLE_STYLES_12_ATOM
          )
        {
          return Err(Error::invalid(0, "RoundTripStyle12Atom header changed"));
        }
        value.package.physical_bytes.clone()
      }
      PptRecordData::OfficeArt(value) => {
        if self.header.version == 0x0f
          || value.header.version != self.header.version
          || value.header.instance != self.header.instance
          || value.header.record_type != self.header.record_type
          || value.header.declared_length != self.header.declared_length
        {
          return Err(Error::invalid(
            0,
            "embedded OfficeArt record header changed",
          ));
        }
        value.payload_bytes()?
      }
      PptRecordData::PersistDirectory(value) => {
        if !matches!(
          self.header.record_type,
          PERSIST_DIRECTORY_FULL_BLOCK | PERSIST_DIRECTORY_ATOM
        ) || self.header.version == 0x0f
        {
          return Err(Error::invalid(
            0,
            "PersistDirectoryAtom record header changed",
          ));
        }
        value.to_bytes()?
      }
      PptRecordData::Unknown(value) => {
        if self.header.record_type != value.record_type {
          return Err(Error::invalid(0, "unknown record header changed"));
        }
        value.body.clone()
      }
      PptRecordData::MalformedSpecRecord(value) => {
        if self.header.record_type != value.record_type || !is_ms_ppt_record_type(value.record_type)
        {
          return Err(Error::invalid(0, "malformed MS-PPT record header changed"));
        }
        value.body.clone()
      }
      PptRecordData::Truncated(value) => value.clone(),
    };
    Ok(body)
  }

  fn write_to<W: Write + ?Sized>(&self, writer: &mut W) -> Result<()> {
    if let Some(children) = self.direct_body_sequence()? {
      let body_len = children.serialized_len_from_layout()?;
      self.validate_body_len(body_len)?;
      self.header.write_to(writer)?;
      return children.write_to(writer);
    }
    if let Some(body) = self.borrowed_body_bytes()? {
      self.validate_body_len(body.len())?;
      self.header.write_to(writer)?;
      writer.write_all(body)?;
      return Ok(());
    }
    macro_rules! write_fixed_body {
      ($condition:expr, $message:literal, $value:expr) => {{
        if !$condition {
          return Err(Error::invalid(0, $message));
        }
        let body_len = usize::try_from($value.sdk_size())
          .map_err(|_| Error::Limit("PPT fixed body length exceeds usize".into()))?;
        self.validate_body_len(body_len)?;
        self.header.write_to(writer)?;
        let mut sdk_writer = Writer::new(&mut *writer);
        $value.write_to(&mut sdk_writer)?;
        return Ok(());
      }};
    }
    match &self.data {
      PptRecordData::UserEdit(value) => write_fixed_body!(
        self.header.record_type == USER_EDIT_ATOM && self.header.version != 0x0f,
        "UserEditAtom record header changed",
        value
      ),
      PptRecordData::Document(value) => write_fixed_body!(
        self.header.record_type == DOCUMENT_ATOM && self.header.version != 0x0f,
        "DocumentAtom record header changed",
        value
      ),
      PptRecordData::Slide(value) => write_fixed_body!(
        self.header.record_type == SLIDE_ATOM && self.header.version != 0x0f,
        "SlideAtom record header changed",
        value
      ),
      PptRecordData::Notes(value) => write_fixed_body!(
        self.header.record_type == NOTES_ATOM && self.header.version != 0x0f,
        "NotesAtom record header changed",
        value
      ),
      PptRecordData::OutlineTextRef(value) => write_fixed_body!(
        self.header.record_type == OUTLINE_TEXT_REF_ATOM,
        "OutlineTextRefAtom header changed",
        value
      ),
      PptRecordData::TextHeader(value) => write_fixed_body!(
        self.header.record_type == TEXT_HEADER_ATOM,
        "TextHeaderAtom header changed",
        value
      ),
      PptRecordData::SlidePersist(value) => write_fixed_body!(
        self.header.record_type == SLIDE_PERSIST_ATOM,
        "SlidePersistAtom header changed",
        value
      ),
      PptRecordData::ColorScheme(value) => write_fixed_body!(
        self.header.record_type == COLOR_SCHEME_ATOM,
        "ColorSchemeAtom header changed",
        value
      ),
      PptRecordData::ExternalObjectRef(value) => write_fixed_body!(
        self.header.record_type == EXTERNAL_OBJECT_REF_ATOM,
        "ExternalObjectRefAtom header changed",
        value
      ),
      PptRecordData::Placeholder(value) => write_fixed_body!(
        self.header.record_type == PLACEHOLDER_ATOM,
        "PlaceholderAtom header changed",
        value
      ),
      PptRecordData::HeadersFooters(value) => write_fixed_body!(
        self.header.record_type == HEADERS_FOOTERS_ATOM,
        "HeadersFootersAtom header changed",
        value
      ),
      PptRecordData::TextChars(value) | PptRecordData::CString(value) => {
        let expected_type = if matches!(self.data, PptRecordData::TextChars(_)) {
          TEXT_CHARS_ATOM
        } else {
          C_STRING_ATOM
        };
        if self.header.record_type != expected_type {
          return Err(Error::invalid(0, "UTF-16 PPT string header changed"));
        }
        let body_len = value
          .encode_utf16()
          .count()
          .checked_mul(2)
          .ok_or_else(|| Error::Limit("PPT UTF-16 string length overflow".into()))?;
        self.validate_body_len(body_len)?;
        self.header.write_to(writer)?;
        for code_unit in value.encode_utf16() {
          writer.write_all(&code_unit.to_le_bytes())?;
        }
        return Ok(());
      }
      PptRecordData::CompatibilityTextChars(values)
      | PptRecordData::CompatibilityCString(values) => {
        let expected_type = if matches!(self.data, PptRecordData::CompatibilityTextChars(_)) {
          TEXT_CHARS_ATOM
        } else {
          C_STRING_ATOM
        };
        if self.header.record_type != expected_type {
          return Err(Error::invalid(
            0,
            "compatible UTF-16 PPT string header changed",
          ));
        }
        let body_len = values
          .len()
          .checked_mul(2)
          .ok_or_else(|| Error::Limit("PPT UTF-16 string length overflow".into()))?;
        self.validate_body_len(body_len)?;
        self.header.write_to(writer)?;
        for code_unit in values {
          writer.write_all(&code_unit.to_le_bytes())?;
        }
        return Ok(());
      }
      PptRecordData::TextBytes(value) => {
        if self.header.record_type != TEXT_BYTES_ATOM {
          return Err(Error::invalid(0, "TextBytesAtom header changed"));
        }
        let body_len = value.chars().count();
        self.validate_body_len(body_len)?;
        self.header.write_to(writer)?;
        for character in value.chars() {
          let byte = u8::try_from(u32::from(character)).map_err(|_| {
            Error::invalid(
              u64::from(u32::from(character)),
              "TextBytesAtom String contains a character above U+00FF",
            )
          })?;
          writer.write_all(&[byte])?;
        }
        return Ok(());
      }
      PptRecordData::NamedShowSlides(values) => {
        if self.header.record_type != NAMED_SHOW_SLIDES_ATOM {
          return Err(Error::invalid(0, "NamedShowSlidesAtom header changed"));
        }
        let body_len = values
          .len()
          .checked_mul(4)
          .ok_or_else(|| Error::Limit("PPT slide identifier array overflow".into()))?;
        self.validate_body_len(body_len)?;
        self.header.write_to(writer)?;
        for value in values {
          writer.write_all(&value.to_le_bytes())?;
        }
        return Ok(());
      }
      PptRecordData::EndDocument => {
        if self.header.record_type != END_DOCUMENT_ATOM {
          return Err(Error::invalid(0, "EndDocumentAtom header changed"));
        }
        self.validate_body_len(0)?;
        self.header.write_to(writer)?;
        return Ok(());
      }
      _ => {}
    }
    let body = self.body_bytes()?;
    self.validate_body_len(body.len())?;
    self.header.write_to(writer)?;
    writer.write_all(&body)?;
    Ok(())
  }
}

fn parse_office_art_atom(
  header: PptRecordHeader,
  body: &[u8],
  limits: Limits,
) -> Option<OfficeArtRecord> {
  if header.version == 0x0f || !(0xe000..=0xf1ff).contains(&header.record_type) {
    return None;
  }
  let mut bytes = Vec::with_capacity(HEADER_LEN + body.len());
  header.write(&mut bytes).ok()?;
  bytes.extend_from_slice(body);
  let mut stream = OfficeArtStream::from_bytes_with_limits(&bytes, limits).ok()?;
  if stream.records.len() != 1 {
    return None;
  }
  let record = stream.records.pop()?;
  if matches!(record.data, OfficeArtRecordData::Atom(_)) {
    None
  } else {
    Some(record)
  }
}

impl TextSpecialInfoAtom {
  fn parse(bytes: &[u8]) -> Option<Self> {
    let mut cursor = 0usize;
    let mut runs = Vec::new();
    while cursor < bytes.len() {
      let character_count = read_u32_checked(bytes, &mut cursor)?;
      let mask = read_u32_checked(bytes, &mut cursor)?;
      let spelling_flags = if mask & 0x0000_0001 != 0 {
        Some(read_u16_checked(bytes, &mut cursor)?)
      } else {
        None
      };
      let language_id = if mask & 0x0000_0002 != 0 {
        Some(read_u16_checked(bytes, &mut cursor)?)
      } else {
        None
      };
      let alternate_language_id = if mask & 0x0000_0004 != 0 {
        Some(read_u16_checked(bytes, &mut cursor)?)
      } else {
        None
      };
      let bidi = if mask & 0x0000_0040 != 0 {
        Some(read_u16_checked(bytes, &mut cursor)? as i16)
      } else {
        None
      };
      let pp10_extension = if mask & 0x0000_0020 != 0 {
        Some(read_u32_checked(bytes, &mut cursor)?)
      } else {
        None
      };
      let smart_tag_indices = if mask & 0x0000_0200 != 0 {
        let count = usize::try_from(read_u32_checked(bytes, &mut cursor)?).ok()?;
        let remaining_words = bytes.len().checked_sub(cursor)? / 4;
        if count > remaining_words {
          return None;
        }
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
          values.push(read_u32_checked(bytes, &mut cursor)?);
        }
        Some(values)
      } else {
        None
      };
      runs.push(TextSpecialInfoRun {
        character_count,
        mask,
        spelling_flags,
        language_id,
        alternate_language_id,
        bidi,
        pp10_extension,
        smart_tag_indices,
      });
    }
    Some(Self { runs })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for run in &self.runs {
      for (bit, present) in [
        (0x0000_0001, run.spelling_flags.is_some()),
        (0x0000_0002, run.language_id.is_some()),
        (0x0000_0004, run.alternate_language_id.is_some()),
        (0x0000_0040, run.bidi.is_some()),
        (0x0000_0020, run.pp10_extension.is_some()),
        (0x0000_0200, run.smart_tag_indices.is_some()),
      ] {
        if (run.mask & bit != 0) != present {
          return Err(Error::invalid(
            0,
            "TextSpecialInfo mask and optional field disagree",
          ));
        }
      }
      bytes.extend_from_slice(&run.character_count.to_le_bytes());
      bytes.extend_from_slice(&run.mask.to_le_bytes());
      for value in [
        run.spelling_flags,
        run.language_id,
        run.alternate_language_id,
        run.bidi.map(|value| value as u16),
      ]
      .into_iter()
      .flatten()
      {
        bytes.extend_from_slice(&value.to_le_bytes());
      }
      if let Some(value) = run.pp10_extension {
        bytes.extend_from_slice(&value.to_le_bytes());
      }
      if let Some(values) = &run.smart_tag_indices {
        bytes.extend_from_slice(
          &u32::try_from(values.len())
            .map_err(|_| Error::Limit("TextSpecialInfo smart-tag count overflow".into()))?
            .to_le_bytes(),
        );
        for value in values {
          bytes.extend_from_slice(&value.to_le_bytes());
        }
      }
    }
    Ok(bytes)
  }
}

impl StyleTextProp9Atom {
  fn parse(bytes: &[u8]) -> Option<Self> {
    let mut cursor = 0usize;
    let mut runs = Vec::new();
    while cursor < bytes.len() {
      let paragraph = parse_at(bytes, &mut cursor)?;
      let character = parse_at(bytes, &mut cursor)?;
      let special_mask = read_u32_checked(bytes, &mut cursor)?;
      let spelling_flags = read_optional_u16(bytes, &mut cursor, special_mask & 0x0000_0001 != 0)?;
      let language_id = read_optional_u16(bytes, &mut cursor, special_mask & 0x0000_0002 != 0)?;
      let alternate_language_id =
        read_optional_u16(bytes, &mut cursor, special_mask & 0x0000_0004 != 0)?;
      let bidi = read_optional_i16(bytes, &mut cursor, special_mask & 0x0000_0040 != 0)?;
      let pp10_special_extension =
        read_optional_u32(bytes, &mut cursor, special_mask & 0x0000_0020 != 0)?;
      let smart_tag_indices = if special_mask & 0x0000_0200 != 0 {
        let count = usize::try_from(read_u32_checked(bytes, &mut cursor)?).ok()?;
        if count > bytes.len().checked_sub(cursor)? / 4 {
          return None;
        }
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
          values.push(read_u32_checked(bytes, &mut cursor)?);
        }
        Some(values)
      } else {
        None
      };
      runs.push(StyleTextProp9 {
        paragraph,
        character,
        special_info: TextSpecialInfoException {
          mask: special_mask,
          spelling_flags,
          language_id,
          alternate_language_id,
          bidi,
          pp10_extension: pp10_special_extension,
          smart_tag_indices,
        },
      });
    }
    Some(Self { runs })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for run in &self.runs {
      bytes.extend_from_slice(&write_fixed(&run.paragraph)?);
      bytes.extend_from_slice(&write_fixed(&run.character)?);

      let special = &run.special_info;
      for (bit, present) in [
        (0x0000_0001, special.spelling_flags.is_some()),
        (0x0000_0002, special.language_id.is_some()),
        (0x0000_0004, special.alternate_language_id.is_some()),
        (0x0000_0040, special.bidi.is_some()),
        (0x0000_0020, special.pp10_extension.is_some()),
        (0x0000_0200, special.smart_tag_indices.is_some()),
      ] {
        validate_mask_option(special.mask, bit, present)?;
      }
      bytes.extend_from_slice(&special.mask.to_le_bytes());
      write_optional_u16(&mut bytes, special.spelling_flags);
      write_optional_u16(&mut bytes, special.language_id);
      write_optional_u16(&mut bytes, special.alternate_language_id);
      write_optional_i16(&mut bytes, special.bidi);
      write_optional_u32(&mut bytes, special.pp10_extension);
      if let Some(values) = &special.smart_tag_indices {
        bytes.extend_from_slice(
          &u32::try_from(values.len())
            .map_err(|_| Error::Limit("StyleTextProp9 smart-tag count overflow".into()))?
            .to_le_bytes(),
        );
        for value in values {
          bytes.extend_from_slice(&value.to_le_bytes());
        }
      }
    }
    Ok(bytes)
  }
}

impl DocRoutingSlipString {
  fn parse(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
    let string_type = read_u16_checked(bytes, cursor)?;
    let string_length = usize::from(read_u16_checked(bytes, cursor)?);
    let physical_length = string_length.checked_add(1)?;
    let end = cursor.checked_add(physical_length)?;
    let value = bytes.get(*cursor..end)?.to_vec();
    *cursor = end;
    let parsed = Self {
      string_type,
      bytes: value,
    };
    parsed.is_valid().then_some(parsed)
  }

  fn is_valid(&self) -> bool {
    match self.string_type {
      1 | 2 => self.bytes.len() >= 2 && self.bytes[self.bytes.len() - 2] == 0,
      3 | 4 => self.bytes.last() == Some(&0),
      _ => false,
    }
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    if !self.is_valid() {
      return Err(Error::invalid(0, "invalid DocRoutingSlipString"));
    }
    let string_length = self
      .bytes
      .len()
      .checked_sub(1)
      .and_then(|value| u16::try_from(value).ok())
      .ok_or_else(|| Error::Limit("DocRoutingSlipString length overflow".into()))?;
    bytes.extend_from_slice(&self.string_type.to_le_bytes());
    bytes.extend_from_slice(&string_length.to_le_bytes());
    bytes.extend_from_slice(&self.bytes);
    Ok(())
  }
}

impl DocRoutingSlipAtom {
  fn parse(bytes: &[u8], limits: Limits) -> Option<Self> {
    let mut cursor = 0usize;
    let length = usize::try_from(read_u32_checked(bytes, &mut cursor)?).ok()?;
    let routing_end = length.checked_sub(HEADER_LEN)?;
    if routing_end > bytes.len() || routing_end < 24 {
      return None;
    }
    let unused1 = read_u32_checked(bytes, &mut cursor)?;
    let recipient_count = usize::try_from(read_u32_checked(bytes, &mut cursor)?).ok()?;
    if recipient_count > limits.max_entries {
      return None;
    }
    let current_recipient = read_u32_checked(bytes, &mut cursor)?;
    let flags = read_u32_checked(bytes, &mut cursor)?;
    let unused2 = read_u32_checked(bytes, &mut cursor)?;
    let routing = &bytes[..routing_end];
    let originator = DocRoutingSlipString::parse(routing, &mut cursor)?;
    if originator.string_type != 1 {
      return None;
    }
    let mut recipients = Vec::with_capacity(recipient_count);
    for _ in 0..recipient_count {
      let recipient = DocRoutingSlipString::parse(routing, &mut cursor)?;
      if recipient.string_type != 2 {
        return None;
      }
      recipients.push(recipient);
    }
    let subject = DocRoutingSlipString::parse(routing, &mut cursor)?;
    let message = DocRoutingSlipString::parse(routing, &mut cursor)?;
    if subject.string_type != 3 || message.string_type != 4 || cursor != routing_end {
      return None;
    }
    Some(Self {
      unused1,
      current_recipient,
      flags,
      unused2,
      originator,
      recipients,
      subject,
      message,
      unused3: bytes[routing_end..].to_vec(),
    })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.originator.string_type != 1
      || self.recipients.iter().any(|value| value.string_type != 2)
      || self.subject.string_type != 3
      || self.message.string_type != 4
    {
      return Err(Error::invalid(0, "DocRoutingSlipString type changed"));
    }
    let recipient_count = u32::try_from(self.recipients.len())
      .map_err(|_| Error::Limit("DocRoutingSlip recipient count overflow".into()))?;
    if self.current_recipient > recipient_count.saturating_add(1) {
      return Err(Error::invalid(
        0,
        "DocRoutingSlip current recipient overflow",
      ));
    }
    let mut bytes = vec![0; 4];
    bytes.extend_from_slice(&self.unused1.to_le_bytes());
    bytes.extend_from_slice(&recipient_count.to_le_bytes());
    bytes.extend_from_slice(&self.current_recipient.to_le_bytes());
    bytes.extend_from_slice(&self.flags.to_le_bytes());
    bytes.extend_from_slice(&self.unused2.to_le_bytes());
    self.originator.write(&mut bytes)?;
    for recipient in &self.recipients {
      recipient.write(&mut bytes)?;
    }
    self.subject.write(&mut bytes)?;
    self.message.write(&mut bytes)?;
    let length = u32::try_from(
      HEADER_LEN
        .checked_add(bytes.len())
        .ok_or_else(|| Error::Limit("DocRoutingSlip length overflow".into()))?,
    )
    .map_err(|_| Error::Limit("DocRoutingSlip length overflow".into()))?;
    bytes[..4].copy_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(&self.unused3);
    Ok(bytes)
  }
}

impl MetafileBlob {
  fn parse(bytes: &[u8]) -> Option<Self> {
    if bytes.len() <= 16 {
      return None;
    }
    Some(Self {
      mapping_mode: i16::from_le_bytes(bytes[0..2].try_into().ok()?),
      x_extent: i16::from_le_bytes(bytes[2..4].try_into().ok()?),
      y_extent: i16::from_le_bytes(bytes[4..6].try_into().ok()?),
      data: bytes[6..].to_vec(),
    })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.data.len() <= 10 {
      return Err(Error::invalid(0, "MetafileBlob is shorter than MS-PPT"));
    }
    let mut bytes = Vec::with_capacity(6 + self.data.len());
    bytes.extend_from_slice(&self.mapping_mode.to_le_bytes());
    bytes.extend_from_slice(&self.x_extent.to_le_bytes());
    bytes.extend_from_slice(&self.y_extent.to_le_bytes());
    bytes.extend_from_slice(&self.data);
    Ok(bytes)
  }
}

impl TimeAnimateColorBy {
  fn parse(bytes: &[u8]) -> Option<Self> {
    if bytes.len() != 16 {
      return None;
    }
    let values = [read_u32(bytes, 4), read_u32(bytes, 8), read_u32(bytes, 12)];
    Some(match read_u32(bytes, 0) {
      0 => Self::Rgb {
        red: values[0] as i32,
        green: values[1] as i32,
        blue: values[2] as i32,
      },
      1 => Self::Hsl {
        hue: values[0] as i32,
        saturation: values[1] as i32,
        luminance: values[2] as i32,
      },
      2 => Self::Scheme(IndexSchemeColor {
        index: values[0],
        reserved1: values[1],
        reserved2: values[2],
      }),
      _ => return None,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) {
    let (model, values): (u32, [u32; 3]) = match self {
      Self::Rgb { red, green, blue } => (0, [red as u32, green as u32, blue as u32]),
      Self::Hsl {
        hue,
        saturation,
        luminance,
      } => (1, [hue as u32, saturation as u32, luminance as u32]),
      Self::Scheme(value) => (2, [value.index, value.reserved1, value.reserved2]),
    };
    bytes.extend_from_slice(&model.to_le_bytes());
    for value in values {
      bytes.extend_from_slice(&value.to_le_bytes());
    }
  }
}

impl TimeAnimateColor {
  fn parse(bytes: &[u8]) -> Option<Self> {
    if bytes.len() != 16 {
      return None;
    }
    let values = [read_u32(bytes, 4), read_u32(bytes, 8), read_u32(bytes, 12)];
    Some(match read_u32(bytes, 0) {
      0 => Self::Rgb {
        red: values[0],
        green: values[1],
        blue: values[2],
      },
      2 => Self::Scheme(IndexSchemeColor {
        index: values[0],
        reserved1: values[1],
        reserved2: values[2],
      }),
      _ => return None,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) {
    let (model, values): (u32, [u32; 3]) = match self {
      Self::Rgb { red, green, blue } => (0, [red, green, blue]),
      Self::Scheme(value) => (2, [value.index, value.reserved1, value.reserved2]),
    };
    bytes.extend_from_slice(&model.to_le_bytes());
    for value in values {
      bytes.extend_from_slice(&value.to_le_bytes());
    }
  }
}

impl TimeColorBehaviorAtom {
  fn parse(bytes: &[u8]) -> Option<Self> {
    (bytes.len() == 52).then_some(())?;
    Some(Self {
      property_flags: read_u32(bytes, 0),
      color_by: TimeAnimateColorBy::parse(&bytes[4..20])?,
      color_from: TimeAnimateColor::parse(&bytes[20..36])?,
      color_to: TimeAnimateColor::parse(&bytes[36..52])?,
    })
  }

  fn to_bytes(&self) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(52);
    bytes.extend_from_slice(&self.property_flags.to_le_bytes());
    self.color_by.write(&mut bytes);
    self.color_from.write(&mut bytes);
    self.color_to.write(&mut bytes);
    bytes
  }
}

impl TextMasterStyle9Atom {
  fn parse(bytes: &[u8], text_type: u16) -> Option<Self> {
    let mut cursor = 0usize;
    let count = usize::from(read_u16_checked(bytes, &mut cursor)?);
    if count > 5 {
      return None;
    }
    let mut levels = Vec::with_capacity(count);
    for _ in 0..count {
      levels.push(parse_at(bytes, &mut cursor)?);
    }
    (cursor == bytes.len()).then_some(Self { text_type, levels })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.levels.len() > 5 {
      return Err(Error::invalid(
        0,
        "TextMasterStyle9 has more than five levels",
      ));
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(self.levels.len() as u16).to_le_bytes());
    for level in &self.levels {
      bytes.extend_from_slice(&write_fixed(level)?);
    }
    Ok(bytes)
  }
}

impl StyleTextProp10Atom {
  fn parse(bytes: &[u8]) -> Option<Self> {
    let mut cursor = 0usize;
    let mut runs = Vec::new();
    while cursor < bytes.len() {
      runs.push(parse_at(bytes, &mut cursor)?);
    }
    Some(Self { runs })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for run in &self.runs {
      bytes.extend_from_slice(&write_fixed(run)?);
    }
    Ok(bytes)
  }
}

impl TextMasterStyle10Atom {
  fn parse(bytes: &[u8], text_type: u16) -> Option<Self> {
    let mut cursor = 0usize;
    let count = usize::from(read_u16_checked(bytes, &mut cursor)?);
    if count > 5 {
      return None;
    }
    let mut levels = Vec::with_capacity(count);
    for _ in 0..count {
      levels.push(parse_at(bytes, &mut cursor)?);
    }
    (cursor == bytes.len()).then_some(Self { text_type, levels })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.levels.len() > 5 {
      return Err(Error::invalid(
        0,
        "TextMasterStyle10 has more than five levels",
      ));
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(self.levels.len() as u16).to_le_bytes());
    for level in &self.levels {
      bytes.extend_from_slice(&write_fixed(level)?);
    }
    Ok(bytes)
  }
}

impl StyleTextProp11Atom {
  fn parse(bytes: &[u8]) -> Option<Self> {
    let mut cursor = 0usize;
    let mut runs = Vec::new();
    while cursor < bytes.len() {
      runs.push(TextSpecialInfoException::parse(bytes, &mut cursor)?);
    }
    Some(Self { runs })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for run in &self.runs {
      run.write(&mut bytes)?;
    }
    Ok(bytes)
  }
}

impl RecolorInfoAtom {
  fn parse(bytes: &[u8]) -> Option<Self> {
    if bytes.len() < 12 {
      return None;
    }
    let flags = read_u16(bytes, 0);
    let color_count = read_u16(bytes, 2);
    let fill_count = read_u16(bytes, 4);
    let mono_color = WideColor {
      red: read_u16(bytes, 6),
      green: read_u16(bytes, 8),
      blue: read_u16(bytes, 10),
    };
    let entry_count = usize::from(color_count).checked_add(usize::from(fill_count))?;
    let entries_bytes = entry_count.checked_mul(44)?;
    let entries_end = 12usize.checked_add(entries_bytes)?;
    if entries_end > bytes.len() {
      return None;
    }
    let mut entries = Vec::with_capacity(entry_count);
    for chunk in bytes[12..entries_end].chunks_exact(44) {
      let variant_type = read_u16(chunk, 10);
      let source = match variant_type {
        0 => RecolorEntrySource::Color {
          from_color: WideColor {
            red: read_u16(chunk, 12),
            green: read_u16(chunk, 14),
            blue: read_u16(chunk, 16),
          },
          unused: chunk[18..44].try_into().ok()?,
        },
        1 => RecolorEntrySource::Brush {
          style: read_u16(chunk, 12),
          color: WideColor {
            red: read_u16(chunk, 14),
            green: read_u16(chunk, 16),
            blue: read_u16(chunk, 18),
          },
          hatch: read_u16(chunk, 20),
          foreground_color: WideColor {
            red: read_u16(chunk, 22),
            green: read_u16(chunk, 24),
            blue: read_u16(chunk, 26),
          },
          background_color: WideColor {
            red: read_u16(chunk, 28),
            green: read_u16(chunk, 30),
            blue: read_u16(chunk, 32),
          },
          bitmap_type: read_u16(chunk, 34),
          pattern: chunk[36..44].try_into().ok()?,
        },
        variant_type => RecolorEntrySource::Unknown {
          variant_type,
          body: chunk[12..44].try_into().ok()?,
        },
      };
      entries.push(RecolorEntry {
        flags: read_u16(chunk, 0),
        to_color: WideColor {
          red: read_u16(chunk, 2),
          green: read_u16(chunk, 4),
          blue: read_u16(chunk, 6),
        },
        to_index: chunk[8],
        unused: chunk[9],
        source,
      });
    }
    Some(Self {
      flags,
      color_count,
      fill_count,
      mono_color,
      entries,
      unused: bytes[entries_end..].to_vec(),
    })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let expected_count = usize::from(self.color_count)
      .checked_add(usize::from(self.fill_count))
      .ok_or_else(|| Error::Limit("RecolorInfo entry count overflow".into()))?;
    if self.entries.len() != expected_count {
      return Err(Error::invalid(0, "RecolorInfo entry count changed"));
    }
    let known_colors = self
      .entries
      .iter()
      .filter(|entry| matches!(entry.source, RecolorEntrySource::Color { .. }))
      .count();
    let known_fills = self
      .entries
      .iter()
      .filter(|entry| matches!(entry.source, RecolorEntrySource::Brush { .. }))
      .count();
    let has_unknown = self
      .entries
      .iter()
      .any(|entry| matches!(entry.source, RecolorEntrySource::Unknown { .. }));
    if !has_unknown
      && (known_colors != usize::from(self.color_count)
        || known_fills != usize::from(self.fill_count))
    {
      return Err(Error::invalid(0, "RecolorInfo variant counts changed"));
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&self.flags.to_le_bytes());
    bytes.extend_from_slice(&self.color_count.to_le_bytes());
    bytes.extend_from_slice(&self.fill_count.to_le_bytes());
    bytes.extend_from_slice(&write_fixed(&self.mono_color)?);
    for entry in &self.entries {
      bytes.extend_from_slice(&entry.flags.to_le_bytes());
      bytes.extend_from_slice(&write_fixed(&entry.to_color)?);
      bytes.push(entry.to_index);
      bytes.push(entry.unused);
      match &entry.source {
        RecolorEntrySource::Color { from_color, unused } => {
          bytes.extend_from_slice(&0u16.to_le_bytes());
          bytes.extend_from_slice(&write_fixed(from_color)?);
          bytes.extend_from_slice(unused);
        }
        RecolorEntrySource::Brush {
          style,
          color,
          hatch,
          foreground_color,
          background_color,
          bitmap_type,
          pattern,
        } => {
          bytes.extend_from_slice(&1u16.to_le_bytes());
          bytes.extend_from_slice(&style.to_le_bytes());
          bytes.extend_from_slice(&write_fixed(color)?);
          bytes.extend_from_slice(&hatch.to_le_bytes());
          bytes.extend_from_slice(&write_fixed(foreground_color)?);
          bytes.extend_from_slice(&write_fixed(background_color)?);
          bytes.extend_from_slice(&bitmap_type.to_le_bytes());
          bytes.extend_from_slice(pattern);
        }
        RecolorEntrySource::Unknown { variant_type, body } => {
          bytes.extend_from_slice(&variant_type.to_le_bytes());
          bytes.extend_from_slice(body);
        }
      }
    }
    bytes.extend_from_slice(&self.unused);
    Ok(bytes)
  }
}

impl Ppt11FontDescriptorAtom {
  fn parse(bytes: &[u8]) -> Option<Self> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(276) {
      return None;
    }
    let mut descriptors = Vec::with_capacity(bytes.len() / 276);
    for chunk in bytes.chunks_exact(276) {
      descriptors.push(Ppt11FontDescriptor::parse(chunk)?);
    }
    Some(Self { descriptors })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.descriptors.is_empty() {
      return Err(Error::invalid(0, "PPT11 font descriptor array is empty"));
    }
    let mut bytes = Vec::with_capacity(self.descriptors.len().saturating_mul(276));
    for descriptor in &self.descriptors {
      descriptor.write(&mut bytes)?;
    }
    Ok(bytes)
  }
}

impl Ppt11FontDescriptor {
  fn parse(bytes: &[u8]) -> Option<Self> {
    if bytes.len() != 276 || bytes[..4] != [0, 0, 0, 8] || read_u32(bytes, 4) != 268 {
      return None;
    }
    let byte_order = if read_u32(bytes, 8) == 2 && read_u32(bytes, 12) == 268 {
      Ppt11FontDescriptorByteOrder::LittleEndian
    } else if u32::from_be_bytes(bytes[8..12].try_into().ok()?) == 2
      && u32::from_be_bytes(bytes[12..16].try_into().ok()?) == 268
    {
      Ppt11FontDescriptorByteOrder::BigEndian
    } else {
      return None;
    };
    Some(Self {
      byte_order,
      serialized_properties: bytes.try_into().ok()?,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    let value = &self.serialized_properties;
    let framing_matches = match self.byte_order {
      Ppt11FontDescriptorByteOrder::LittleEndian => {
        read_u32(value, 8) == 2 && read_u32(value, 12) == 268
      }
      Ppt11FontDescriptorByteOrder::BigEndian => {
        u32::from_be_bytes(value[8..12].try_into().expect("four bytes")) == 2
          && u32::from_be_bytes(value[12..16].try_into().expect("four bytes")) == 268
      }
    };
    if value[..4] != [0, 0, 0, 8] || read_u32(value, 4) != 268 || !framing_matches {
      return Err(Error::invalid(0, "PPT11 font descriptor framing changed"));
    }
    bytes.extend_from_slice(value);
    Ok(())
  }
}

impl Ppt11FontDescriptorCollectionAtom {
  fn parse(bytes: &[u8]) -> Option<Self> {
    if bytes.len() < 2 {
      return None;
    }
    let count = usize::from(read_u16(bytes, 0));
    if bytes.len() != 2usize.checked_add(count.checked_mul(276)?)? {
      return None;
    }
    let descriptors = bytes[2..]
      .chunks_exact(276)
      .map(Ppt11FontDescriptor::parse)
      .collect::<Option<Vec<_>>>()?;
    if descriptors
      .iter()
      .any(|descriptor| descriptor.byte_order != Ppt11FontDescriptorByteOrder::BigEndian)
    {
      return None;
    }
    Some(Self { descriptors })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let count = u16::try_from(self.descriptors.len())
      .map_err(|_| Error::Limit("PPT11 font descriptor count exceeds u16".into()))?;
    let mut bytes = Vec::with_capacity(2 + self.descriptors.len().saturating_mul(276));
    bytes.extend_from_slice(&count.to_le_bytes());
    for descriptor in &self.descriptors {
      descriptor.write(&mut bytes)?;
    }
    Ok(bytes)
  }
}

impl ExternalStorageAtom {
  fn parse(instance: u16, body: &[u8], limits: Limits) -> Self {
    match instance {
      0 => match CompoundFile::from_bytes_with_limits(body, limits) {
        Ok(compound_file) => Self::Parsed(Box::new(ParsedExternalStorage::new(
          compound_file,
          ExternalStorageEncoding::Uncompressed,
          limits,
        ))),
        Err(error) => Self::InvalidUncompressed {
          storage_bytes: body.to_vec(),
          reason: error.to_string(),
        },
      },
      1 if body.len() < 4 => Self::MalformedCompressed {
        body: body.to_vec(),
        reason: "compressed external storage lacks decompressedSize".into(),
      },
      1 => {
        let declared_decompressed_size = read_u32(body, 0);
        let declared = usize::try_from(declared_decompressed_size)
          .ok()
          .filter(|size| *size <= limits.max_allocation)
          .filter(|size| (*size as u64) <= limits.max_file_size);
        let compressed_bytes = body[4..].to_vec();
        let Some(declared) = declared else {
          return Self::InvalidCompressed {
            declared_decompressed_size,
            compressed_bytes,
            reason: "declared external storage size exceeds limits".into(),
          };
        };
        let mut decoded = Vec::with_capacity(declared);
        let decode_result = ZlibDecoder::new(compressed_bytes.as_slice())
          .take(u64::from(declared_decompressed_size) + 1)
          .read_to_end(&mut decoded);
        if let Err(error) = decode_result {
          return Self::InvalidCompressed {
            declared_decompressed_size,
            compressed_bytes,
            reason: format!("zlib decode failed: {error}"),
          };
        }
        if decoded.len() != declared {
          return Self::InvalidCompressed {
            declared_decompressed_size,
            compressed_bytes,
            reason: format!(
              "decompressed size {} differs from declared {declared}",
              decoded.len()
            ),
          };
        }
        match CompoundFile::from_bytes_with_limits(&decoded, limits) {
          Ok(compound_file) => Self::Parsed(Box::new(ParsedExternalStorage::new(
            compound_file,
            ExternalStorageEncoding::Zlib {
              declared_decompressed_size,
              compressed_bytes,
            },
            limits,
          ))),
          Err(error) => Self::InvalidCompressed {
            declared_decompressed_size,
            compressed_bytes,
            reason: format!("decompressed storage is not valid CFB: {error}"),
          },
        }
      }
      _ => Self::UnsupportedInstance {
        instance,
        body: body.to_vec(),
      },
    }
  }

  fn instance(&self) -> u16 {
    match self {
      Self::Parsed(value) => match &value.encoding {
        ExternalStorageEncoding::Uncompressed => 0,
        ExternalStorageEncoding::Zlib { .. } => 1,
      },
      Self::MalformedCompressed { .. } | Self::InvalidCompressed { .. } => 1,
      Self::InvalidUncompressed { .. } => 0,
      Self::UnsupportedInstance { instance, .. } => *instance,
    }
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    match self {
      Self::Parsed(value) => match &value.encoding {
        ExternalStorageEncoding::Uncompressed => value.compound_file.to_bytes(),
        ExternalStorageEncoding::Zlib {
          declared_decompressed_size,
          compressed_bytes,
        } => {
          let mut bytes = Vec::with_capacity(4 + compressed_bytes.len());
          bytes.extend_from_slice(&declared_decompressed_size.to_le_bytes());
          bytes.extend_from_slice(compressed_bytes);
          Ok(bytes)
        }
      },
      Self::MalformedCompressed { body, .. } | Self::UnsupportedInstance { body, .. } => {
        Ok(body.clone())
      }
      Self::InvalidCompressed {
        declared_decompressed_size,
        compressed_bytes,
        ..
      } => {
        let mut bytes = Vec::with_capacity(4 + compressed_bytes.len());
        bytes.extend_from_slice(&declared_decompressed_size.to_le_bytes());
        bytes.extend_from_slice(compressed_bytes);
        Ok(bytes)
      }
      Self::InvalidUncompressed { storage_bytes, .. } => Ok(storage_bytes.clone()),
    }
  }

  pub fn recompress(compound_file: CompoundFile) -> Result<Self> {
    let storage_bytes = compound_file.to_bytes()?;
    let declared_decompressed_size = u32::try_from(storage_bytes.len())
      .map_err(|_| Error::Limit("external storage exceeds u32".into()))?;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&storage_bytes)?;
    let compressed_bytes = encoder.finish()?;
    Ok(Self::Parsed(Box::new(ParsedExternalStorage::new(
      compound_file,
      ExternalStorageEncoding::Zlib {
        declared_decompressed_size,
        compressed_bytes,
      },
      Limits::default(),
    ))))
  }

  pub(crate) fn replace_vba_module_source(
    &mut self,
    stream_name: &str,
    source: &[u8],
  ) -> Result<VbaModuleSourceMutation> {
    let Self::Parsed(storage) = self else {
      return Err(Error::invalid(
        0,
        "PPT VBA project storage is not a parsed external storage",
      ));
    };
    let ExternalStorageVba::Parsed(project) = &mut storage.vba_project else {
      return Err(Error::invalid(
        0,
        "PPT external storage has no parsed VBA project",
      ));
    };
    let report = project.replace_module_source(stream_name, source)?;
    project.write_if_modified(&mut storage.compound_file)?;
    if matches!(storage.encoding, ExternalStorageEncoding::Zlib { .. }) {
      let storage_bytes = storage.compound_file.to_bytes()?;
      let declared_decompressed_size = u32::try_from(storage_bytes.len())
        .map_err(|_| Error::Limit("external storage exceeds u32".into()))?;
      let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
      encoder.write_all(&storage_bytes)?;
      storage.encoding = ExternalStorageEncoding::Zlib {
        declared_decompressed_size,
        compressed_bytes: encoder.finish()?,
      };
    }
    Ok(report)
  }

  pub(crate) fn edit_vba_designer_storage(
    &mut self,
    index: usize,
    edit: impl FnOnce(&mut ParentControlStorageModel) -> Result<()>,
  ) -> Result<()> {
    let Self::Parsed(storage) = self else {
      return Err(Error::invalid(
        0,
        "PPT VBA project storage is not a parsed external storage",
      ));
    };
    let ExternalStorageVba::Parsed(project) = &mut storage.vba_project else {
      return Err(Error::invalid(
        0,
        "PPT external storage has no parsed VBA project",
      ));
    };
    project.edit_designer_storage(index, edit)?;
    project.write_if_modified(&mut storage.compound_file)?;
    if matches!(storage.encoding, ExternalStorageEncoding::Zlib { .. }) {
      let storage_bytes = storage.compound_file.to_bytes()?;
      let declared_decompressed_size = u32::try_from(storage_bytes.len())
        .map_err(|_| Error::Limit("external storage exceeds u32".into()))?;
      let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
      encoder.write_all(&storage_bytes)?;
      storage.encoding = ExternalStorageEncoding::Zlib {
        declared_decompressed_size,
        compressed_bytes: encoder.finish()?,
      };
    }
    Ok(())
  }
}

impl ParsedExternalStorage {
  fn new(compound_file: CompoundFile, encoding: ExternalStorageEncoding, limits: Limits) -> Self {
    let vba_project = if VbaProject::is_present(&compound_file) {
      match LocatedVbaProject::from_compound_file_with_limits(&compound_file, limits) {
        Ok(project) => ExternalStorageVba::Parsed(Box::new(project)),
        Err(error) => ExternalStorageVba::Invalid(error.to_string()),
      }
    } else {
      ExternalStorageVba::NotPresent
    };
    Self {
      compound_file,
      encoding,
      vba_project,
    }
  }
}

impl MacPlistAtom {
  pub fn from_bytes(bytes: &[u8]) -> Self {
    Self {
      physical_xml: bytes.to_vec(),
    }
  }
}

impl SlideLayoutOpcPackage {
  pub fn from_bytes(bytes: &[u8]) -> Self {
    Self {
      physical_bytes: bytes.to_vec(),
    }
  }
}

impl ThemeOpcPackage {
  pub fn from_bytes(bytes: &[u8]) -> Self {
    Self {
      physical_bytes: bytes.to_vec(),
    }
  }
}

impl RoundTripColorMapping12Atom {
  pub fn from_bytes(bytes: &[u8]) -> Self {
    Self {
      physical_xml: bytes.to_vec(),
    }
  }
}

impl TimingOpcPackage {
  pub fn from_bytes(bytes: &[u8]) -> Self {
    Self {
      physical_bytes: bytes.to_vec(),
    }
  }
}

impl StyleOpcPackage {
  pub fn from_bytes(bytes: &[u8]) -> Self {
    Self {
      physical_bytes: bytes.to_vec(),
    }
  }
}

impl StyleTextPropAtom {
  fn parse(bytes: &[u8], corresponding_text_character_count: u32) -> Option<Self> {
    let target = corresponding_text_character_count;
    let mut cursor = 0usize;
    let mut paragraph_runs = Vec::new();
    let mut paragraph_covered = 0u32;
    let mut paragraph_target = if target == 0 { 1 } else { target };
    while cursor < bytes.len() && paragraph_covered < paragraph_target {
      let character_count = read_u32_checked(bytes, &mut cursor)?;
      let indent_level = read_u16_checked(bytes, &mut cursor)?;
      let properties = TextParagraphException::parse(bytes, &mut cursor)?;
      paragraph_runs.push(TextParagraphRun {
        character_count,
        indent_level,
        properties,
      });
      paragraph_covered = add_style_coverage(paragraph_covered, character_count, target)?;
      // PowerPoint commonly styles the implicit paragraph terminator as size + 1.
      if cursor < bytes.len() && paragraph_covered == target {
        paragraph_target = target.checked_add(1)?;
      }
    }

    let mut character_runs = Vec::new();
    let mut character_covered = 0u32;
    let mut character_target = if target == 0 { 1 } else { target };
    while cursor < bytes.len() && character_covered < character_target {
      let character_count = read_u32_checked(bytes, &mut cursor)?;
      let properties = TextCharacterException::parse(bytes, &mut cursor)?;
      character_runs.push(TextCharacterRun {
        character_count,
        properties,
      });
      character_covered = add_style_coverage(character_covered, character_count, target)?;
      if cursor < bytes.len() && character_covered == target {
        character_target = target.checked_add(1)?;
      }
    }

    Some(Self {
      corresponding_text_character_count,
      paragraph_runs,
      character_runs,
      trailing: bytes[cursor..].to_vec(),
    })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for run in &self.paragraph_runs {
      bytes.extend_from_slice(&run.character_count.to_le_bytes());
      bytes.extend_from_slice(&run.indent_level.to_le_bytes());
      run.properties.write(&mut bytes)?;
    }
    for run in &self.character_runs {
      bytes.extend_from_slice(&run.character_count.to_le_bytes());
      run.properties.write(&mut bytes)?;
    }
    bytes.extend_from_slice(&self.trailing);
    Ok(bytes)
  }
}

impl TextMasterStyleAtom {
  fn parse(bytes: &[u8], text_type: u16) -> Option<Self> {
    let mut cursor = 0usize;
    let level_count = usize::from(read_u16_checked(bytes, &mut cursor)?);
    if level_count > 5 {
      return None;
    }
    let has_explicit_level = text_type >= 5;
    let mut levels = Vec::with_capacity(level_count);
    for _ in 0..level_count {
      let level = if has_explicit_level {
        Some(read_u16_checked(bytes, &mut cursor)?)
      } else {
        None
      };
      levels.push(TextMasterStyleLevel {
        level,
        paragraph: TextParagraphException::parse(bytes, &mut cursor)?,
        character: TextCharacterException::parse(bytes, &mut cursor)?,
      });
    }
    let remaining = &bytes[cursor..];
    let tail = if remaining.is_empty() {
      TextMasterStyleTail::None
    } else if remaining.len() >= HEADER_LEN {
      let header = PptRecordHeader::from_bytes(&remaining[..HEADER_LEN]);
      let available_body = remaining[HEADER_LEN..].to_vec();
      if usize::try_from(header.declared_length).ok()? > available_body.len() {
        TextMasterStyleTail::TruncatedRecord {
          header,
          available_body,
        }
      } else {
        TextMasterStyleTail::Compatibility(remaining.to_vec())
      }
    } else {
      TextMasterStyleTail::Compatibility(remaining.to_vec())
    };
    Some(Self {
      text_type,
      levels,
      tail,
    })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.levels.len() > 5 {
      return Err(Error::invalid(
        0,
        "TextMasterStyleAtom has more than five levels",
      ));
    }
    let expects_explicit_level = self.text_type >= 5;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(self.levels.len() as u16).to_le_bytes());
    for style in &self.levels {
      if style.level.is_some() != expects_explicit_level {
        return Err(Error::invalid(
          0,
          "TextMasterStyleLevel presence disagrees with text type",
        ));
      }
      write_optional_u16(&mut bytes, style.level);
      style.paragraph.write(&mut bytes)?;
      style.character.write(&mut bytes)?;
    }
    self.tail.write(&mut bytes)?;
    Ok(bytes)
  }
}

impl TextMasterStyleTail {
  pub fn physical_len(&self) -> usize {
    match self {
      Self::None => 0,
      Self::TruncatedRecord { available_body, .. } => HEADER_LEN + available_body.len(),
      Self::Compatibility(bytes) => bytes.len(),
    }
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    match self {
      Self::None => {}
      Self::TruncatedRecord {
        header,
        available_body,
      } => {
        let declared = usize::try_from(header.declared_length)
          .map_err(|_| Error::Limit("PPT record length exceeds usize".into()))?;
        if available_body.len() >= declared {
          return Err(Error::invalid(
            0,
            "TextMasterStyle truncated tail is no longer truncated",
          ));
        }
        header.write(bytes)?;
        bytes.extend_from_slice(available_body);
      }
      Self::Compatibility(value) => bytes.extend_from_slice(value),
    }
    Ok(())
  }
}

impl TextRulerAtom {
  fn parse(bytes: &[u8]) -> Option<Self> {
    let mut cursor = 0usize;
    let flags = read_u32_checked(bytes, &mut cursor)?;
    let level_count = read_optional_i16(bytes, &mut cursor, flags & 0x0002 != 0)?;
    let default_tab_size = read_optional_u16(bytes, &mut cursor, flags & 0x0001 != 0)?;
    let tab_stops = if flags & 0x0004 != 0 {
      let count = usize::from(read_u16_checked(bytes, &mut cursor)?);
      let remaining = bytes.len().checked_sub(cursor)? / 4;
      if count > remaining {
        return None;
      }
      let mut values = Vec::with_capacity(count);
      for _ in 0..count {
        values.push(TextTabStop {
          position: read_u16_checked(bytes, &mut cursor)? as i16,
          kind: read_u16_checked(bytes, &mut cursor)?,
        });
      }
      Some(values)
    } else {
      None
    };
    let mut levels = [TextRulerLevel::default(); 5];
    for (index, level) in levels.iter_mut().enumerate() {
      level.left_margin = read_optional_i16(bytes, &mut cursor, flags & (0x0008 << index) != 0)?;
      level.indent = read_optional_i16(bytes, &mut cursor, flags & (0x0100 << index) != 0)?;
    }
    Some(Self {
      flags,
      level_count,
      default_tab_size,
      tab_stops,
      levels,
      trailing: bytes[cursor..].to_vec(),
    })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_mask_option(self.flags, 0x0002, self.level_count.is_some())?;
    validate_mask_option(self.flags, 0x0001, self.default_tab_size.is_some())?;
    validate_mask_option(self.flags, 0x0004, self.tab_stops.is_some())?;
    for (index, level) in self.levels.iter().enumerate() {
      validate_mask_option(self.flags, 0x0008 << index, level.left_margin.is_some())?;
      validate_mask_option(self.flags, 0x0100 << index, level.indent.is_some())?;
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&self.flags.to_le_bytes());
    write_optional_i16(&mut bytes, self.level_count);
    write_optional_u16(&mut bytes, self.default_tab_size);
    if let Some(tab_stops) = &self.tab_stops {
      bytes.extend_from_slice(
        &u16::try_from(tab_stops.len())
          .map_err(|_| Error::Limit("PPT tab-stop count exceeds u16".into()))?
          .to_le_bytes(),
      );
      for tab in tab_stops {
        bytes.extend_from_slice(&tab.position.to_le_bytes());
        bytes.extend_from_slice(&tab.kind.to_le_bytes());
      }
    }
    for level in &self.levels {
      write_optional_i16(&mut bytes, level.left_margin);
      write_optional_i16(&mut bytes, level.indent);
    }
    bytes.extend_from_slice(&self.trailing);
    Ok(bytes)
  }
}

impl TextParagraphException {
  fn parse(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
    let mask = read_u32_checked(bytes, cursor)?;
    let bullet_flags = read_optional_u16(bytes, cursor, mask & 0x0000_000f != 0)?;
    let bullet_character = read_optional_u16(bytes, cursor, mask & 0x0000_0080 != 0)?;
    let bullet_font_ref = read_optional_u16(bytes, cursor, mask & 0x0000_0010 != 0)?;
    let bullet_size = read_optional_i16(bytes, cursor, mask & 0x0000_0040 != 0)?;
    let bullet_color = read_optional_u32(bytes, cursor, mask & 0x0000_0020 != 0)?;
    let text_alignment = read_optional_u16(bytes, cursor, mask & 0x0000_0800 != 0)?;
    let line_spacing = read_optional_i16(bytes, cursor, mask & 0x0000_1000 != 0)?;
    let space_before = read_optional_i16(bytes, cursor, mask & 0x0000_2000 != 0)?;
    let space_after = read_optional_i16(bytes, cursor, mask & 0x0000_4000 != 0)?;
    let left_margin = read_optional_i16(bytes, cursor, mask & 0x0000_0100 != 0)?;
    let indent = read_optional_i16(bytes, cursor, mask & 0x0000_0400 != 0)?;
    let default_tab_size = read_optional_u16(bytes, cursor, mask & 0x0000_8000 != 0)?;
    let tab_stops = if mask & 0x0010_0000 != 0 {
      let count = usize::from(read_u16_checked(bytes, cursor)?);
      let remaining = bytes.len().checked_sub(*cursor)? / 4;
      if count > remaining {
        return None;
      }
      let mut values = Vec::with_capacity(count);
      for _ in 0..count {
        values.push(TextTabStop {
          position: read_u16_checked(bytes, cursor)? as i16,
          kind: read_u16_checked(bytes, cursor)?,
        });
      }
      Some(values)
    } else {
      None
    };
    let font_alignment = read_optional_u16(bytes, cursor, mask & 0x0001_0000 != 0)?;
    let wrap_flags = read_optional_u16(bytes, cursor, mask & 0x000e_0000 != 0)?;
    let text_direction = read_optional_u16(bytes, cursor, mask & 0x0020_0000 != 0)?;
    Some(Self {
      mask,
      bullet_flags,
      bullet_character,
      bullet_font_ref,
      bullet_size,
      bullet_color,
      text_alignment,
      line_spacing,
      space_before,
      space_after,
      left_margin,
      indent,
      default_tab_size,
      tab_stops,
      font_alignment,
      wrap_flags,
      text_direction,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    validate_mask_option(self.mask, 0x0000_000f, self.bullet_flags.is_some())?;
    validate_mask_option(self.mask, 0x0000_0080, self.bullet_character.is_some())?;
    validate_mask_option(self.mask, 0x0000_0010, self.bullet_font_ref.is_some())?;
    validate_mask_option(self.mask, 0x0000_0040, self.bullet_size.is_some())?;
    validate_mask_option(self.mask, 0x0000_0020, self.bullet_color.is_some())?;
    validate_mask_option(self.mask, 0x0000_0800, self.text_alignment.is_some())?;
    validate_mask_option(self.mask, 0x0000_1000, self.line_spacing.is_some())?;
    validate_mask_option(self.mask, 0x0000_2000, self.space_before.is_some())?;
    validate_mask_option(self.mask, 0x0000_4000, self.space_after.is_some())?;
    validate_mask_option(self.mask, 0x0000_0100, self.left_margin.is_some())?;
    validate_mask_option(self.mask, 0x0000_0400, self.indent.is_some())?;
    validate_mask_option(self.mask, 0x0000_8000, self.default_tab_size.is_some())?;
    validate_mask_option(self.mask, 0x0010_0000, self.tab_stops.is_some())?;
    validate_mask_option(self.mask, 0x0001_0000, self.font_alignment.is_some())?;
    validate_mask_option(self.mask, 0x000e_0000, self.wrap_flags.is_some())?;
    validate_mask_option(self.mask, 0x0020_0000, self.text_direction.is_some())?;
    bytes.extend_from_slice(&self.mask.to_le_bytes());
    write_optional_u16(bytes, self.bullet_flags);
    write_optional_u16(bytes, self.bullet_character);
    write_optional_u16(bytes, self.bullet_font_ref);
    write_optional_i16(bytes, self.bullet_size);
    write_optional_u32(bytes, self.bullet_color);
    write_optional_u16(bytes, self.text_alignment);
    write_optional_i16(bytes, self.line_spacing);
    write_optional_i16(bytes, self.space_before);
    write_optional_i16(bytes, self.space_after);
    write_optional_i16(bytes, self.left_margin);
    write_optional_i16(bytes, self.indent);
    write_optional_u16(bytes, self.default_tab_size);
    if let Some(tab_stops) = &self.tab_stops {
      bytes.extend_from_slice(
        &u16::try_from(tab_stops.len())
          .map_err(|_| Error::Limit("PPT tab-stop count exceeds u16".into()))?
          .to_le_bytes(),
      );
      for tab in tab_stops {
        bytes.extend_from_slice(&tab.position.to_le_bytes());
        bytes.extend_from_slice(&tab.kind.to_le_bytes());
      }
    }
    write_optional_u16(bytes, self.font_alignment);
    write_optional_u16(bytes, self.wrap_flags);
    write_optional_u16(bytes, self.text_direction);
    Ok(())
  }
}

impl TextCharacterException {
  fn parse(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
    let mask = read_u32_checked(bytes, cursor)?;
    Some(Self {
      mask,
      font_style: read_optional_u16(bytes, cursor, mask & 0x0000_ffff != 0)?,
      font_ref: read_optional_u16(bytes, cursor, mask & 0x0001_0000 != 0)?,
      old_east_asian_font_ref: read_optional_u16(bytes, cursor, mask & 0x0020_0000 != 0)?,
      ansi_font_ref: read_optional_u16(bytes, cursor, mask & 0x0040_0000 != 0)?,
      symbol_font_ref: read_optional_u16(bytes, cursor, mask & 0x0080_0000 != 0)?,
      font_size: read_optional_i16(bytes, cursor, mask & 0x0002_0000 != 0)?,
      color: read_optional_u32(bytes, cursor, mask & 0x0004_0000 != 0)?,
      position: read_optional_i16(bytes, cursor, mask & 0x0008_0000 != 0)?,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    validate_mask_option(self.mask, 0x0000_ffff, self.font_style.is_some())?;
    validate_mask_option(self.mask, 0x0001_0000, self.font_ref.is_some())?;
    validate_mask_option(
      self.mask,
      0x0020_0000,
      self.old_east_asian_font_ref.is_some(),
    )?;
    validate_mask_option(self.mask, 0x0040_0000, self.ansi_font_ref.is_some())?;
    validate_mask_option(self.mask, 0x0080_0000, self.symbol_font_ref.is_some())?;
    validate_mask_option(self.mask, 0x0002_0000, self.font_size.is_some())?;
    validate_mask_option(self.mask, 0x0004_0000, self.color.is_some())?;
    validate_mask_option(self.mask, 0x0008_0000, self.position.is_some())?;
    bytes.extend_from_slice(&self.mask.to_le_bytes());
    write_optional_u16(bytes, self.font_style);
    write_optional_u16(bytes, self.font_ref);
    write_optional_u16(bytes, self.old_east_asian_font_ref);
    write_optional_u16(bytes, self.ansi_font_ref);
    write_optional_u16(bytes, self.symbol_font_ref);
    write_optional_i16(bytes, self.font_size);
    write_optional_u32(bytes, self.color);
    write_optional_i16(bytes, self.position);
    Ok(())
  }
}

impl TextSpecialInfoException {
  fn parse(bytes: &[u8], cursor: &mut usize) -> Option<Self> {
    let mask = read_u32_checked(bytes, cursor)?;
    let spelling_flags = read_optional_u16(bytes, cursor, mask & 0x0000_0001 != 0)?;
    let language_id = read_optional_u16(bytes, cursor, mask & 0x0000_0002 != 0)?;
    let alternate_language_id = read_optional_u16(bytes, cursor, mask & 0x0000_0004 != 0)?;
    let bidi = read_optional_i16(bytes, cursor, mask & 0x0000_0040 != 0)?;
    let pp10_extension = read_optional_u32(bytes, cursor, mask & 0x0000_0020 != 0)?;
    let smart_tag_indices = if mask & 0x0000_0200 != 0 {
      let count = usize::try_from(read_u32_checked(bytes, cursor)?).ok()?;
      if count > bytes.len().checked_sub(*cursor)? / 4 {
        return None;
      }
      let mut values = Vec::with_capacity(count);
      for _ in 0..count {
        values.push(read_u32_checked(bytes, cursor)?);
      }
      Some(values)
    } else {
      None
    };
    Some(Self {
      mask,
      spelling_flags,
      language_id,
      alternate_language_id,
      bidi,
      pp10_extension,
      smart_tag_indices,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    for (bit, present) in [
      (0x0000_0001, self.spelling_flags.is_some()),
      (0x0000_0002, self.language_id.is_some()),
      (0x0000_0004, self.alternate_language_id.is_some()),
      (0x0000_0040, self.bidi.is_some()),
      (0x0000_0020, self.pp10_extension.is_some()),
      (0x0000_0200, self.smart_tag_indices.is_some()),
    ] {
      validate_mask_option(self.mask, bit, present)?;
    }
    bytes.extend_from_slice(&self.mask.to_le_bytes());
    write_optional_u16(bytes, self.spelling_flags);
    write_optional_u16(bytes, self.language_id);
    write_optional_u16(bytes, self.alternate_language_id);
    write_optional_i16(bytes, self.bidi);
    write_optional_u32(bytes, self.pp10_extension);
    if let Some(values) = &self.smart_tag_indices {
      bytes.extend_from_slice(
        &u32::try_from(values.len())
          .map_err(|_| Error::Limit("PPT smart-tag count exceeds u32".into()))?
          .to_le_bytes(),
      );
      for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
      }
    }
    Ok(())
  }
}

impl CurrentUserAtom {
  fn parse(bytes: &[u8], following: &[u8]) -> Option<(Self, usize)> {
    if bytes.len() < 24 {
      return None;
    }
    let declared_user_name_byte_length = read_u16(bytes, 12);
    let ansi_end = 20usize.checked_add(usize::from(declared_user_name_byte_length))?;
    let release_end = ansi_end.checked_add(4)?;
    let ansi_user_name = bytes.get(20..ansi_end)?.to_vec();
    let release_version = read_u32(bytes.get(ansi_end..release_end)?, 0);
    let remaining = &bytes[release_end..];
    let expected_unicode_bytes = usize::from(declared_user_name_byte_length).checked_mul(2)?;
    let (unicode_source, external) = if remaining.is_empty() && !following.is_empty() {
      (following, true)
    } else {
      (remaining, false)
    };
    let (unicode_user_name, consumed) = if expected_unicode_bytes == 0 || unicode_source.is_empty()
    {
      (None, 0)
    } else {
      let byte_length = unicode_source.len().min(expected_unicode_bytes);
      let even_length = byte_length - byte_length % 2;
      (
        Some(CurrentUserUnicodeName {
          code_units: unicode_source[..even_length]
            .chunks_exact(2)
            .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
            .collect(),
          is_complete: even_length == expected_unicode_bytes,
          inside_record: !external,
        }),
        even_length,
      )
    };
    let value = Self {
      fixed_size: read_u32(bytes, 0),
      header_token: read_u32(bytes, 4),
      offset_to_current_edit: read_u32(bytes, 8),
      declared_user_name_byte_length,
      document_file_version: read_u16(bytes, 14),
      major_version: bytes[16],
      minor_version: bytes[17],
      unused: read_u16(bytes, 18),
      ansi_user_name,
      release_version,
      unicode_user_name,
      trailing: if external {
        Vec::new()
      } else {
        remaining[consumed..].to_vec()
      },
    };
    Some((value, if external { consumed } else { 0 }))
  }

  fn to_parts(&self) -> Result<(Vec<u8>, Vec<u8>)> {
    let declared_name_length = usize::from(self.declared_user_name_byte_length);
    if self.ansi_user_name.len() != declared_name_length {
      return Err(Error::invalid(
        0,
        "CurrentUserAtom ANSI name length mismatch",
      ));
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&self.fixed_size.to_le_bytes());
    bytes.extend_from_slice(&self.header_token.to_le_bytes());
    bytes.extend_from_slice(&self.offset_to_current_edit.to_le_bytes());
    bytes.extend_from_slice(&self.declared_user_name_byte_length.to_le_bytes());
    bytes.extend_from_slice(&self.document_file_version.to_le_bytes());
    bytes.push(self.major_version);
    bytes.push(self.minor_version);
    bytes.extend_from_slice(&self.unused.to_le_bytes());
    bytes.extend_from_slice(&self.ansi_user_name);
    bytes.extend_from_slice(&self.release_version.to_le_bytes());
    let mut following = Vec::new();
    if let Some(unicode) = &self.unicode_user_name {
      let unicode_bytes = unicode
        .code_units
        .len()
        .checked_mul(2)
        .ok_or_else(|| Error::Limit("CurrentUserAtom Unicode name overflow".into()))?;
      if (unicode.is_complete && unicode_bytes != declared_name_length * 2)
        || (!unicode.is_complete && unicode_bytes >= declared_name_length * 2)
      {
        return Err(Error::invalid(
          0,
          "CurrentUserAtom Unicode completion mismatch",
        ));
      }
      for unit in &unicode.code_units {
        if unicode.inside_record {
          bytes.extend_from_slice(&unit.to_le_bytes());
        } else {
          following.extend_from_slice(&unit.to_le_bytes());
        }
      }
      if unicode.inside_record {
        bytes.extend_from_slice(&self.trailing);
      } else {
        following.extend_from_slice(&self.trailing);
      }
    } else {
      bytes.extend_from_slice(&self.trailing);
    }
    Ok((bytes, following))
  }
}

impl PersistDirectoryAtom {
  fn parse(bytes: &[u8], limits: Limits) -> Option<Self> {
    let mut cursor = 0usize;
    let mut entries = Vec::new();
    let mut offset_count = 0usize;
    let mut persist_ids = BTreeSet::new();
    while cursor < bytes.len() {
      let info = read_u32_checked(bytes, &mut cursor)?;
      let count = usize::try_from(info >> 20).ok()?;
      let first_persist_id = info & 0x000f_ffff;
      if count == 0 || first_persist_id > 0x000f_fffe {
        return None;
      }
      offset_count = offset_count.checked_add(count)?;
      if entries.len() >= limits.max_entries || offset_count > limits.max_entries {
        return None;
      }
      let mut stream_offsets = Vec::with_capacity(count);
      for index in 0..count {
        let persist_id = first_persist_id.checked_add(u32::try_from(index).ok()?)?;
        if !persist_ids.insert(persist_id) {
          return None;
        }
        stream_offsets.push(read_u32_checked(bytes, &mut cursor)?);
      }
      entries.push(PersistDirectoryEntry {
        first_persist_id,
        stream_offsets,
      });
    }
    Some(Self { entries })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut persist_ids = BTreeSet::new();
    for entry in &self.entries {
      if entry.first_persist_id > 0x000f_fffe
        || entry.stream_offsets.is_empty()
        || entry.stream_offsets.len() > 0x0fff
      {
        return Err(Error::invalid(0, "PPT persist-directory entry overflow"));
      }
      for index in 0..entry.stream_offsets.len() {
        let persist_id = entry
          .first_persist_id
          .checked_add(
            u32::try_from(index)
              .map_err(|_| Error::Limit("PPT persist offset index exceeds u32".into()))?,
          )
          .ok_or_else(|| Error::invalid(0, "PPT persist-directory identifier overflow"))?;
        if !persist_ids.insert(persist_id) {
          return Err(Error::invalid(
            0,
            "duplicate persist object identifier in PersistDirectoryAtom",
          ));
        }
      }
      let count = u32::try_from(entry.stream_offsets.len())
        .map_err(|_| Error::Limit("PPT persist offset count exceeds u32".into()))?;
      let info = entry.first_persist_id | (count << 20);
      bytes.extend_from_slice(&info.to_le_bytes());
      for offset in &entry.stream_offsets {
        bytes.extend_from_slice(&offset.to_le_bytes());
      }
    }
    Ok(bytes)
  }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
  u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
  u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

fn read_u32_checked(bytes: &[u8], cursor: &mut usize) -> Option<u32> {
  let end = cursor.checked_add(4)?;
  let value = u32::from_le_bytes(bytes.get(*cursor..end)?.try_into().ok()?);
  *cursor = end;
  Some(value)
}

fn read_u16_checked(bytes: &[u8], cursor: &mut usize) -> Option<u16> {
  let end = cursor.checked_add(2)?;
  let value = u16::from_le_bytes(bytes.get(*cursor..end)?.try_into().ok()?);
  *cursor = end;
  Some(value)
}

fn read_optional_u16(bytes: &[u8], cursor: &mut usize, present: bool) -> Option<Option<u16>> {
  if present {
    read_u16_checked(bytes, cursor).map(Some)
  } else {
    Some(None)
  }
}

fn read_optional_i16(bytes: &[u8], cursor: &mut usize, present: bool) -> Option<Option<i16>> {
  read_optional_u16(bytes, cursor, present).map(|value| value.map(|value| value as i16))
}

fn read_optional_u32(bytes: &[u8], cursor: &mut usize, present: bool) -> Option<Option<u32>> {
  if present {
    read_u32_checked(bytes, cursor).map(Some)
  } else {
    Some(None)
  }
}

fn add_style_coverage(covered: u32, declared: u32, text_size: u32) -> Option<u32> {
  let maximum = text_size.checked_add(1)?;
  Some(covered.checked_add(declared)?.min(maximum))
}

fn validate_mask_option(mask: u32, field_mask: u32, present: bool) -> Result<()> {
  if (mask & field_mask != 0) != present {
    return Err(Error::invalid(
      0,
      "PPT text-property mask and optional field disagree",
    ));
  }
  Ok(())
}

fn write_optional_u16(bytes: &mut Vec<u8>, value: Option<u16>) {
  if let Some(value) = value {
    bytes.extend_from_slice(&value.to_le_bytes());
  }
}

fn write_optional_i16(bytes: &mut Vec<u8>, value: Option<i16>) {
  if let Some(value) = value {
    bytes.extend_from_slice(&value.to_le_bytes());
  }
}

fn write_optional_u32(bytes: &mut Vec<u8>, value: Option<u32>) {
  if let Some(value) = value {
    bytes.extend_from_slice(&value.to_le_bytes());
  }
}

fn read_utf16(bytes: &[u8]) -> Vec<u16> {
  bytes
    .chunks_exact(2)
    .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
    .collect()
}

fn write_utf16(values: &[u16]) -> Vec<u8> {
  let mut bytes = Vec::with_capacity(values.len().saturating_mul(2));
  for value in values {
    bytes.extend_from_slice(&value.to_le_bytes());
  }
  bytes
}

fn write_utf16_string(value: &str) -> Vec<u8> {
  let mut bytes = Vec::with_capacity(value.encode_utf16().count().saturating_mul(2));
  for code_unit in value.encode_utf16() {
    bytes.extend_from_slice(&code_unit.to_le_bytes());
  }
  bytes
}

fn write_byte_string(value: &str) -> Result<Vec<u8>> {
  value
    .chars()
    .map(|character| {
      u8::try_from(u32::from(character)).map_err(|_| {
        Error::invalid(
          u64::from(u32::from(character)),
          "TextBytesAtom String contains a character above U+00FF",
        )
      })
    })
    .collect()
}

fn parse_fixed<T: SdkRead>(bytes: &[u8]) -> Option<T> {
  let mut reader = Reader::new(Cursor::new(bytes)).ok()?;
  let value = T::read_from(&mut reader).ok()?;
  (reader.remaining().ok()? == 0).then_some(value)
}

fn parse_at<T: SdkRead>(bytes: &[u8], cursor: &mut usize) -> Option<T> {
  let remaining = bytes.len().checked_sub(*cursor)?;
  let mut reader = Reader::with_bounds(
    Cursor::new(bytes),
    u64::try_from(*cursor).ok()?,
    u64::try_from(remaining).ok()?,
  )
  .ok()?;
  let value = T::read_from(&mut reader).ok()?;
  *cursor = usize::try_from(reader.position().ok()?).ok()?;
  Some(value)
}

fn write_fixed<T: SdkWrite>(value: &T) -> Result<Vec<u8>> {
  let mut writer = Writer::new(Cursor::new(Vec::new()));
  value.write_to(&mut writer)?;
  Ok(writer.into_inner().into_inner())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn additional_spec_atoms_are_static_and_round_trip() {
    let routing_slip = DocRoutingSlipAtom {
      unused1: 0,
      current_recipient: 1,
      flags: 3,
      unused2: 0,
      originator: DocRoutingSlipString {
        string_type: 1,
        bytes: b"Ada\0\0".to_vec(),
      },
      recipients: vec![DocRoutingSlipString {
        string_type: 2,
        bytes: b"Bob\0\0".to_vec(),
      }],
      subject: DocRoutingSlipString {
        string_type: 3,
        bytes: b"Subject\0".to_vec(),
      },
      message: DocRoutingSlipString {
        string_type: 4,
        bytes: b"Message\0".to_vec(),
      },
      unused3: vec![0xaa, 0xbb],
    }
    .to_bytes()
    .unwrap();
    let mut cases = vec![
      (
        NAMED_SHOW_SLIDES_ATOM,
        0,
        [1u32, 9].into_iter().flat_map(u32::to_le_bytes).collect(),
      ),
      (BOOKMARK_SEED_ATOM, 2, 17u32.to_le_bytes().to_vec()),
      (SHAPE_ATOM, 0, vec![1]),
      (SHAPE_FLAGS10_ATOM, 0, vec![4]),
      (ROUND_TRIP_NEW_PLACEHOLDER_ID_12_ATOM, 0, vec![18]),
      (FONT_EMBED_DATA_BLOB, 1, vec![0, 1, 0, 0, 0xaa]),
      (BOOKMARK_ENTITY_ATOM, 0, vec![0; 68]),
      (RTF_DATE_TIME_META_CHARACTER_ATOM, 0, vec![0; 132]),
      (CHART_BUILD_ATOM, 0, vec![0; 8]),
      (DIAGRAM_BUILD_ATOM, 0, vec![0; 4]),
      (LINKED_SHAPE10_ATOM, 0, vec![0; 8]),
      (LINKED_SLIDE10_ATOM, 0, vec![0; 8]),
      (DIFF10_ATOM, 0, vec![0; 12]),
      (SLIDE_LIST_TABLE_SIZE10_ATOM, 0, vec![0; 4]),
      (SLIDE_LIST_ENTRY10_ATOM, 0, vec![0; 12]),
      (FONT_EMBED_FLAGS10_ATOM, 0, vec![3, 0, 0, 0]),
      (PHOTO_ALBUM_INFO10_ATOM, 0, vec![1, 1, 2, 0, 3, 0]),
      (TIME_ITERATE_DATA_ATOM, 0, vec![0; 20]),
      (TEXT_DEFAULTS9_ATOM, 0, vec![0; 8]),
      (EXTERNAL_OLE_LINK_ATOM, 0, vec![0; 12]),
      (EXTERNAL_OLE_CONTROL_ATOM, 0, vec![0; 4]),
      (EXTERNAL_CD_AUDIO_ATOM, 0, vec![1, 2, 3, 4, 5, 6, 7, 8]),
      (BROADCAST_DOC_INFO9_ATOM, 0, vec![0; 34]),
      (ENVELOPE_FLAGS9_ATOM, 0, vec![3, 0, 0, 0]),
      (ENVELOPE_DATA9_ATOM, 0, vec![1, 2, 3, 4, 5]),
      (DOC_ROUTING_SLIP_ATOM, 0, routing_slip),
      (METAFILE_BLOB, 0, vec![0; 17]),
      (ROUND_TRIP_SLIDE_SYNC_INFO12_ATOM, 0, vec![0; 32]),
      (TIME_COLOR_BEHAVIOR_ATOM, 0, vec![0; 52]),
      (TIME_ROTATION_BEHAVIOR_ATOM, 0, vec![0; 20]),
    ];
    cases[6].2[0..4].copy_from_slice(&23u32.to_le_bytes());
    cases[7].2[0..4].copy_from_slice(&5u32.to_le_bytes());

    let mut bytes = Vec::new();
    for (record_type, instance, body) in &cases {
      PptRecordHeader {
        version: 0,
        instance: *instance,
        record_type: *record_type,
        declared_length: body.len() as u32,
      }
      .write(&mut bytes)
      .unwrap();
      bytes.extend_from_slice(body);
    }

    let parsed = PowerPointDocument::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.records.records.len(), cases.len());
    assert!(parsed.records.records.iter().all(|record| !matches!(
      record.data,
      PptRecordData::MalformedSpecRecord(_) | PptRecordData::Unknown(_)
    )));
    assert!(matches!(
        parsed.records.records[0].data,
        PptRecordData::NamedShowSlides(ref values) if values == &[1, 9]
    ));
    assert!(matches!(
      parsed.records.records[6].data,
      PptRecordData::BookmarkEntity(BookmarkEntityAtom {
        bookmark_id: 23,
        ..
      })
    ));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn malformed_spec_and_unknown_extension_records_are_distinct() {
    let mut bytes = Vec::new();
    for (record_type, body) in [(DOCUMENT_ATOM, &[1u8][..]), (0x7777, &[2, 3][..])] {
      PptRecordHeader {
        version: 0,
        instance: 0,
        record_type,
        declared_length: body.len() as u32,
      }
      .write(&mut bytes)
      .unwrap();
      bytes.extend_from_slice(body);
    }
    let parsed = PowerPointDocument::from_bytes(&bytes).unwrap();
    assert!(matches!(
      parsed.records.records[0].data,
      PptRecordData::MalformedSpecRecord(UnknownPptRecord {
        record_type: DOCUMENT_ATOM,
        ..
      })
    ));
    assert!(matches!(
      parsed.records.records[1].data,
      PptRecordData::Unknown(UnknownPptRecord {
        record_type: 0x7777,
        ..
      })
    ));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn time_variant_discriminant_controls_static_value_layout() {
    let cases = [
      vec![0, 1],
      vec![1, 0xfe, 0xff, 0xff, 0xff],
      vec![2, 0, 0, 0xc0, 0x3f],
      vec![3, b'A', 0, b'B', 0],
    ];
    for bytes in cases {
      let value = TimeVariantAtom::parse(&bytes).expect("valid TimeVariant");
      assert_eq!(value.to_bytes(), bytes);
    }
    assert!(TimeVariantAtom::parse(&[3, b'A']).is_none());
    assert!(TimeVariantAtom::parse(&[4, 0]).is_none());
  }

  #[test]
  fn style_text_prop9_masks_control_all_optional_fields() {
    let value = StyleTextProp9Atom {
      runs: vec![StyleTextProp9 {
        paragraph: TextParagraphException9 {
          mask: 0x0380_0000,
          bullet_blip_ref: Some(7),
          bullet_has_auto_number: Some(1),
          auto_number_scheme: Some(TextAutoNumberScheme {
            scheme: 3,
            start_number: 4,
          }),
        },
        character: TextCharacterException9 {
          mask: 0x0010_0000,
          pp10_extension: Some(0x1234_5678),
        },
        special_info: TextSpecialInfoException {
          mask: 0x0000_0267,
          spelling_flags: Some(1),
          language_id: Some(0x0409),
          alternate_language_id: Some(0x0411),
          bidi: Some(1),
          pp10_extension: Some(0x8765_4321),
          smart_tag_indices: Some(vec![2, 5]),
        },
      }],
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(StyleTextProp9Atom::parse(&bytes), Some(value.clone()));
    assert!(StyleTextProp9Atom::parse(&bytes[..bytes.len() - 1]).is_none());

    let mut invalid = value;
    invalid.runs[0].paragraph.mask &= !0x0080_0000;
    assert!(invalid.to_bytes().is_err());
  }

  #[test]
  fn programmable_binary_tag_has_static_name_and_data_views() {
    let tag_units: Vec<u16> = "___PPT12".encode_utf16().collect();
    let tag_body = write_utf16(&tag_units);
    let mut children = Vec::new();
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: C_STRING_ATOM,
      declared_length: tag_body.len() as u32,
    }
    .write(&mut children)
    .unwrap();
    children.extend_from_slice(&tag_body);
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: BINARY_TAG_DATA_BLOB,
      declared_length: 0,
    }
    .write(&mut children)
    .unwrap();

    let mut bytes = Vec::new();
    PptRecordHeader {
      version: 0x0f,
      instance: 0,
      record_type: PROG_BINARY_TAG,
      declared_length: children.len() as u32,
    }
    .write(&mut bytes)
    .unwrap();
    bytes.extend_from_slice(&children);

    let parsed = PowerPointDocument::from_bytes(&bytes).unwrap();
    let PptRecordData::ProgBinaryTag(value) = &parsed.records.records[0].data else {
      panic!("expected ProgBinaryTag");
    };
    assert_eq!(value.tag(), Some("___PPT12"));
    assert_eq!(value.tag_kind(), Some(ProgrammableTagKind::Ppt12));
    assert!(value.binary_tag_data().is_some());
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn private_programmable_tag_data_is_not_misframed_as_ppt_records() {
    let tag_units: Vec<u16> = "___PPTMac11".encode_utf16().collect();
    let tag_body = write_utf16(&tag_units);
    let private_body = [0x00, 0x00, 0x19, 0x10, 0x04, 0, 0, 0, 1, 2, 3, 4];
    let mut children = Vec::new();
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: C_STRING_ATOM,
      declared_length: tag_body.len() as u32,
    }
    .write(&mut children)
    .unwrap();
    children.extend_from_slice(&tag_body);
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: BINARY_TAG_DATA_BLOB,
      declared_length: private_body.len() as u32,
    }
    .write(&mut children)
    .unwrap();
    children.extend_from_slice(&private_body);
    let mut bytes = Vec::new();
    PptRecordHeader {
      version: 0x0f,
      instance: 0,
      record_type: PROG_BINARY_TAG,
      declared_length: children.len() as u32,
    }
    .write(&mut bytes)
    .unwrap();
    bytes.extend_from_slice(&children);

    let parsed = PowerPointDocument::from_bytes(&bytes).unwrap();
    let PptRecordData::ProgBinaryTag(tag) = &parsed.records.records[0].data else {
      panic!("expected ProgBinaryTag");
    };
    assert_eq!(tag.tag_kind(), Some(ProgrammableTagKind::PptMac11));
    assert!(matches!(
        tag.binary_tag_data(),
        Some(BinaryTagData::Opaque(value)) if value == &private_body
    ));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn mac_plist_preserves_external_xml_bytes() {
    let xml = br#"<?xml version="1.0"?><plist version="1.0"><dict><key>name</key><string>A &amp; B</string><key>enabled</key><true/><key>items</key><array><integer>7</integer><real>1.5</real></array></dict></plist>"#;
    let value = MacPlistAtom::from_bytes(xml);
    assert_eq!(value.physical_xml, xml);
  }

  #[test]
  fn nested_records_and_static_incremental_save_atoms_round_trip() {
    let user_edit = UserEditAtom {
      last_slide_id_ref: 7,
      version: 0,
      minor_version: 0,
      major_version: 3,
      offset_last_edit: 0,
      offset_persist_directory: 8,
      doc_persist_id_ref: 1,
      persist_id_seed: 4,
      last_view: 1,
      unused: 0x55aa,
      encrypt_session_persist_id_ref: None,
    };
    let persist = PersistDirectoryAtom {
      entries: vec![PersistDirectoryEntry {
        first_persist_id: 1,
        stream_offsets: vec![100, 200, 300],
      }],
    };
    let persist_body = persist.to_bytes().unwrap();
    let user_body = write_fixed(&user_edit).unwrap();
    let mut child_bytes = Vec::new();
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: PERSIST_DIRECTORY_ATOM,
      declared_length: persist_body.len() as u32,
    }
    .write(&mut child_bytes)
    .unwrap();
    child_bytes.extend_from_slice(&persist_body);
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: USER_EDIT_ATOM,
      declared_length: user_body.len() as u32,
    }
    .write(&mut child_bytes)
    .unwrap();
    child_bytes.extend_from_slice(&user_body);

    let mut bytes = Vec::new();
    PptRecordHeader {
      version: 0x0f,
      instance: 2,
      record_type: 0x03e8,
      declared_length: child_bytes.len() as u32,
    }
    .write(&mut bytes)
    .unwrap();
    bytes.extend_from_slice(&child_bytes);

    let parsed = PowerPointDocument::from_bytes(&bytes).unwrap();
    let PptRecordData::Container(children) = &parsed.records.records[0].data else {
      panic!("expected container");
    };
    assert!(matches!(
      children.records[0].data,
      PptRecordData::PersistDirectory(_)
    ));
    assert!(matches!(
      children.records[1].data,
      PptRecordData::UserEdit(_)
    ));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn record_header_and_user_edit_derive_enforce_spec_layout() {
    for header in [
      PptRecordHeader {
        version: 0x10,
        instance: 0,
        record_type: USER_EDIT_ATOM,
        declared_length: 28,
      },
      PptRecordHeader {
        version: 0,
        instance: 0x1000,
        record_type: USER_EDIT_ATOM,
        declared_length: 28,
      },
    ] {
      assert!(header.write(&mut Vec::new()).is_err());
    }

    let user_edit = UserEditAtom {
      last_slide_id_ref: 7,
      version: 0,
      minor_version: 0,
      major_version: 3,
      offset_last_edit: 0,
      offset_persist_directory: 8,
      doc_persist_id_ref: 1,
      persist_id_seed: 4,
      last_view: 1,
      unused: 0,
      encrypt_session_persist_id_ref: Some(9),
    };
    let bytes = write_fixed(&user_edit).unwrap();
    assert_eq!(bytes.len(), 32);
    assert_eq!(parse_fixed::<UserEditAtom>(&bytes), Some(user_edit));
    assert!(parse_fixed::<UserEditAtom>(&bytes[..31]).is_none());

    let mut invalid = user_edit;
    invalid.last_view = 0;
    assert!(write_fixed(&invalid).is_err());
  }

  #[test]
  fn truncated_record_and_header_tail_are_explicit_and_exact() {
    let bytes = [0x00, 0x00, 0x34, 0x12, 0x06, 0, 0, 0, 1, 2, 3];
    let parsed = PowerPointDocument::from_bytes(&bytes).unwrap();
    assert!(matches!(
        &parsed.records.records[0].data,
        PptRecordData::Truncated(value) if value == &[1, 2, 3]
    ));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);

    let tail = PowerPointDocument::from_bytes(&[1, 2, 3]).unwrap();
    assert_eq!(tail.records.trailing_header_bytes, [1, 2, 3]);
    assert_eq!(tail.to_bytes().unwrap(), [1, 2, 3]);
  }

  #[test]
  fn current_user_atom_round_trips_ansi_and_unicode_names() {
    let atom = CurrentUserAtom {
      fixed_size: 20,
      header_token: 0xe391_c05f,
      offset_to_current_edit: 1234,
      declared_user_name_byte_length: 3,
      document_file_version: 0x03f4,
      major_version: 3,
      minor_version: 0,
      unused: 0,
      ansi_user_name: b"Ada".to_vec(),
      release_version: 8,
      unicode_user_name: Some(CurrentUserUnicodeName {
        code_units: vec![b'A' as u16, b'd' as u16, b'a' as u16],
        is_complete: true,
        inside_record: false,
      }),
      trailing: Vec::new(),
    };
    let (body, following) = atom.to_parts().unwrap();
    let mut bytes = Vec::new();
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: CURRENT_USER_ATOM,
      declared_length: body.len() as u32,
    }
    .write(&mut bytes)
    .unwrap();
    bytes.extend_from_slice(&body);
    bytes.extend_from_slice(&following);

    let parsed = CurrentUserStream::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.data, CurrentUserData::Parsed(atom));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn incremental_save_chain_builds_effective_persist_directory() {
    let persist = PersistDirectoryAtom {
      entries: vec![PersistDirectoryEntry {
        first_persist_id: 1,
        stream_offsets: vec![100, 200],
      }],
    };
    let persist_body = persist.to_bytes().unwrap();
    let prefix_len = HEADER_LEN;
    let persist_offset = prefix_len as u32;
    let user_offset = (prefix_len + HEADER_LEN + persist_body.len()) as u32;
    let user = UserEditAtom {
      last_slide_id_ref: 0,
      version: 0,
      minor_version: 0,
      major_version: 3,
      offset_last_edit: 0,
      offset_persist_directory: persist_offset,
      doc_persist_id_ref: 1,
      persist_id_seed: 3,
      last_view: 1,
      unused: 0,
      encrypt_session_persist_id_ref: None,
    };
    let user_body = write_fixed(&user).unwrap();
    let mut bytes = Vec::new();
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: 0x779f,
      declared_length: 0,
    }
    .write(&mut bytes)
    .unwrap();
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: PERSIST_DIRECTORY_ATOM,
      declared_length: persist_body.len() as u32,
    }
    .write(&mut bytes)
    .unwrap();
    bytes.extend_from_slice(&persist_body);
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: USER_EDIT_ATOM,
      declared_length: user_body.len() as u32,
    }
    .write(&mut bytes)
    .unwrap();
    bytes.extend_from_slice(&user_body);

    let mut document = PowerPointDocument::from_bytes(&bytes).unwrap();
    let current = CurrentUserAtom {
      fixed_size: 20,
      header_token: 0xe391_c05f,
      offset_to_current_edit: user_offset,
      declared_user_name_byte_length: 0,
      document_file_version: 0x03f4,
      major_version: 3,
      minor_version: 0,
      unused: 0,
      ansi_user_name: Vec::new(),
      release_version: 8,
      unicode_user_name: None,
      trailing: Vec::new(),
    };
    let chain = document.incremental_save_chain(&current).unwrap();
    assert_eq!(chain.edits.len(), 1);
    assert_eq!(
      chain.persist_object_offsets,
      BTreeMap::from([(1, 100), (2, 200)])
    );

    let PptRecordData::UserEdit(user_edit) = &mut document.records.records.last_mut().unwrap().data
    else {
      panic!("last record is not UserEditAtom");
    };
    user_edit.offset_persist_directory = 0;
    assert!(document.incremental_save_chain(&current).is_err());
  }

  #[test]
  fn persist_object_directory_classifies_physical_history_without_claiming_liveness() {
    fn append_record(bytes: &mut Vec<u8>, version: u8, record_type: u16, body: &[u8]) -> u32 {
      let offset = u32::try_from(bytes.len()).unwrap();
      PptRecordHeader {
        version,
        instance: 0,
        record_type,
        declared_length: u32::try_from(body.len()).unwrap(),
      }
      .write(bytes)
      .unwrap();
      bytes.extend_from_slice(body);
      offset
    }

    let mut bytes = Vec::new();
    let old_document_offset = append_record(&mut bytes, 0x0f, DOCUMENT_CONTAINER, &[]);
    let slide_offset = append_record(&mut bytes, 0x0f, SLIDE_CONTAINER, &[]);
    let old_directory_body = PersistDirectoryAtom {
      entries: vec![PersistDirectoryEntry {
        first_persist_id: 1,
        stream_offsets: vec![old_document_offset, slide_offset],
      }],
    }
    .to_bytes()
    .unwrap();
    let old_directory_offset =
      append_record(&mut bytes, 0, PERSIST_DIRECTORY_ATOM, &old_directory_body);
    let old_user_edit = UserEditAtom {
      last_slide_id_ref: 0,
      version: 0,
      minor_version: 0,
      major_version: 3,
      offset_last_edit: 0,
      offset_persist_directory: old_directory_offset,
      doc_persist_id_ref: 1,
      persist_id_seed: 3,
      last_view: 1,
      unused: 0,
      encrypt_session_persist_id_ref: None,
    };
    let old_user_edit_offset = append_record(
      &mut bytes,
      0,
      USER_EDIT_ATOM,
      &write_fixed(&old_user_edit).unwrap(),
    );

    let current_document_offset = append_record(&mut bytes, 0x0f, DOCUMENT_CONTAINER, &[]);
    let unreferenced_offset = append_record(&mut bytes, 0, 0x779f, &[]);
    let current_directory_body = PersistDirectoryAtom {
      entries: vec![PersistDirectoryEntry {
        first_persist_id: 1,
        stream_offsets: vec![current_document_offset],
      }],
    }
    .to_bytes()
    .unwrap();
    let current_directory_offset = append_record(
      &mut bytes,
      0,
      PERSIST_DIRECTORY_ATOM,
      &current_directory_body,
    );
    let current_user_edit = UserEditAtom {
      offset_last_edit: old_user_edit_offset,
      offset_persist_directory: current_directory_offset,
      persist_id_seed: 3,
      ..old_user_edit
    };
    let current_user_edit_offset = append_record(
      &mut bytes,
      0,
      USER_EDIT_ATOM,
      &write_fixed(&current_user_edit).unwrap(),
    );
    assert_eq!(
      (
        old_document_offset,
        slide_offset,
        old_directory_offset,
        old_user_edit_offset,
        current_document_offset,
        unreferenced_offset,
        current_directory_offset,
        current_user_edit_offset,
      ),
      (0, 8, 16, 36, 72, 80, 88, 104)
    );

    let document = PowerPointDocument::from_bytes(&bytes).unwrap();
    let current_user = CurrentUserAtom {
      fixed_size: 20,
      header_token: 0xe391_c05f,
      offset_to_current_edit: current_user_edit_offset,
      declared_user_name_byte_length: 0,
      document_file_version: 0x03f4,
      major_version: 3,
      minor_version: 0,
      unused: 0,
      ansi_user_name: Vec::new(),
      release_version: 8,
      unicode_user_name: None,
      trailing: Vec::new(),
    };
    let directory = document.persist_object_directory(&current_user).unwrap();
    assert_eq!(
      directory
        .references
        .iter()
        .map(|reference| (
          reference.persist_id,
          reference.stream_offset,
          reference.status,
        ))
        .collect::<Vec<_>>(),
      vec![
        (1, 72, PersistObjectReferenceStatus::Current),
        (1, 0, PersistObjectReferenceStatus::Superseded),
        (2, 8, PersistObjectReferenceStatus::Current),
      ]
    );
    assert_eq!(directory.current_reference(1).unwrap().record_index, 4);
    assert_eq!(directory.current_reference(2).unwrap().record_index, 1);
    assert!(matches!(
        &directory.top_level_record(0).unwrap().role,
        PptTopLevelRecordRole::PersistObject {
            current_persist_ids,
            superseded_persist_ids,
        } if current_persist_ids.is_empty() && superseded_persist_ids == &[1]
    ));
    assert!(matches!(
        &directory.top_level_record(1).unwrap().role,
        PptTopLevelRecordRole::PersistObject {
            current_persist_ids,
            superseded_persist_ids,
        } if current_persist_ids == &[2] && superseded_persist_ids.is_empty()
    ));
    assert!(matches!(
      directory.top_level_record(2).unwrap().role,
      PptTopLevelRecordRole::IncrementalSaveMetadata(
        IncrementalSaveMetadataKind::PersistDirectoryAtom
      )
    ));
    assert!(matches!(
      directory.top_level_record(3).unwrap().role,
      PptTopLevelRecordRole::IncrementalSaveMetadata(IncrementalSaveMetadataKind::UserEditAtom)
    ));
    assert!(matches!(
      directory.top_level_record(5).unwrap().role,
      PptTopLevelRecordRole::Unreferenced
    ));

    let mut invalid = document.clone();
    let PptRecordData::PersistDirectory(value) = &mut invalid.records.records[6].data else {
      unreachable!()
    };
    value.entries[0].stream_offsets[0] = old_user_edit_offset;
    assert!(invalid.persist_object_directory(&current_user).is_err());

    let mut duplicate = document;
    let PptRecordData::PersistDirectory(value) = &mut duplicate.records.records[2].data else {
      unreachable!()
    };
    value.entries.push(PersistDirectoryEntry {
      first_persist_id: 2,
      stream_offsets: vec![slide_offset],
    });
    assert!(duplicate.persist_object_directory(&current_user).is_err());

    assert!(
      PersistDirectoryAtom {
        entries: vec![PersistDirectoryEntry {
          first_persist_id: 1,
          stream_offsets: Vec::new(),
        }],
      }
      .to_bytes()
      .is_err()
    );

    // MS-PPT constrains the encoded starting persistId to 20 bits, but
    // defines following identifiers by addition and does not impose a
    // second 20-bit limit on persistId + cPersist - 1.
    let crossing_twenty_bits = PersistDirectoryAtom {
      entries: vec![PersistDirectoryEntry {
        first_persist_id: 0x000f_fffe,
        stream_offsets: vec![0, 8, 16],
      }],
    };
    let bytes = crossing_twenty_bits.to_bytes().unwrap();
    assert_eq!(
      PersistDirectoryAtom::parse(&bytes, Limits::default()).unwrap(),
      crossing_twenty_bits
    );
  }

  #[test]
  fn live_presentation_follows_parts_two_through_eleven_and_keeps_dead_objects() {
    fn append_record(
      bytes: &mut Vec<u8>,
      version: u8,
      instance: u16,
      record_type: u16,
      body: &[u8],
    ) -> u32 {
      let offset = u32::try_from(bytes.len()).unwrap();
      PptRecordHeader {
        version,
        instance,
        record_type,
        declared_length: u32::try_from(body.len()).unwrap(),
      }
      .write(bytes)
      .unwrap();
      bytes.extend_from_slice(body);
      offset
    }

    fn slide_persist_list(instance: u16, persist_ids: &[u32]) -> Vec<u8> {
      let mut body = Vec::new();
      for (index, persist_id) in persist_ids.iter().copied().enumerate() {
        let atom = SlidePersistAtom {
          persist_id_ref: persist_id,
          flags: 0,
          text_count: 0,
          slide_id: u32::try_from(index + 1).unwrap(),
          reserved: 0,
        };
        append_record(
          &mut body,
          0,
          0,
          SLIDE_PERSIST_ATOM,
          &write_fixed(&atom).unwrap(),
        );
      }
      let mut record = Vec::new();
      append_record(
        &mut record,
        0x0f,
        instance,
        SLIDE_LIST_WITH_TEXT_CONTAINER,
        &body,
      );
      record
    }

    fn external_object_container(record_type: u16, persist_id: u32) -> Vec<u8> {
      let atom = ExternalOleObjectAtom {
        draw_aspect: 1,
        object_type: 0,
        external_object_id: persist_id,
        object_subtype: 0,
        persist_id_ref: persist_id,
        unused: 0,
      };
      let mut body = Vec::new();
      append_record(
        &mut body,
        1,
        0,
        EXTERNAL_OLE_OBJECT_ATOM,
        &write_fixed(&atom).unwrap(),
      );
      let mut record = Vec::new();
      append_record(&mut record, 0x0f, 0, record_type, &body);
      record
    }

    let document_atom = DocumentAtom {
      slide_size: PptPoint { x: 720, y: 540 },
      notes_size: PptPoint { x: 540, y: 720 },
      server_zoom: PptPoint { x: 1, y: 1 },
      notes_master_persist_id_ref: 2,
      handout_master_persist_id_ref: 3,
      first_slide_number: 1,
      slide_size_type: 0,
      save_with_fonts: 0,
      omit_title_placeholders: 0,
      right_to_left: 0,
      show_comments: 0,
    };
    let mut document_body = Vec::new();
    append_record(
      &mut document_body,
      1,
      0,
      DOCUMENT_ATOM,
      &write_fixed(&document_atom).unwrap(),
    );
    document_body.extend_from_slice(&slide_persist_list(1, &[4, 5]));
    document_body.extend_from_slice(&slide_persist_list(0, &[6]));
    document_body.extend_from_slice(&slide_persist_list(2, &[7]));

    let mut external_object_list_body = Vec::new();
    external_object_list_body.extend_from_slice(&external_object_container(
      EXTERNAL_OLE_CONTROL_CONTAINER,
      8,
    ));
    external_object_list_body
      .extend_from_slice(&external_object_container(EXTERNAL_OLE_EMBED_CONTAINER, 9));
    external_object_list_body
      .extend_from_slice(&external_object_container(EXTERNAL_OLE_LINK_CONTAINER, 10));
    append_record(
      &mut document_body,
      0x0f,
      0,
      EXTERNAL_OBJECT_LIST_CONTAINER,
      &external_object_list_body,
    );

    let vba_info_atom = VbaInfoAtom {
      persist_id_ref: 11,
      has_macros: 1,
      version: 2,
    };
    let mut vba_info_body = Vec::new();
    append_record(
      &mut vba_info_body,
      2,
      0,
      VBA_INFO_ATOM,
      &write_fixed(&vba_info_atom).unwrap(),
    );
    let mut document_info_list_body = Vec::new();
    append_record(
      &mut document_info_list_body,
      0x0f,
      1,
      VBA_INFO_CONTAINER,
      &vba_info_body,
    );
    append_record(
      &mut document_body,
      0x0f,
      0,
      DOCUMENT_INFO_LIST_CONTAINER,
      &document_info_list_body,
    );

    let mut bytes = Vec::new();
    let document_offset = append_record(&mut bytes, 0x0f, 0, DOCUMENT_CONTAINER, &document_body);
    let notes_master_offset = append_record(&mut bytes, 0x0f, 0, NOTES_CONTAINER, &[]);
    let handout_master_offset = append_record(&mut bytes, 0x0f, 0, HANDOUT_CONTAINER, &[]);
    let main_master_offset = append_record(&mut bytes, 0x0f, 0, MAIN_MASTER_CONTAINER, &[]);
    let title_master_offset = append_record(&mut bytes, 0x0f, 0, SLIDE_CONTAINER, &[]);
    let slide_offset = append_record(&mut bytes, 0x0f, 0, SLIDE_CONTAINER, &[]);
    let notes_offset = append_record(&mut bytes, 0x0f, 0, NOTES_CONTAINER, &[]);
    let control_offset = append_record(&mut bytes, 0, 0, EXTERNAL_OLE_OBJECT_STORAGE, &[]);
    let embedded_ole_offset = append_record(&mut bytes, 0, 0, EXTERNAL_OLE_OBJECT_STORAGE, &[]);
    let linked_ole_offset = append_record(&mut bytes, 0, 0, EXTERNAL_OLE_OBJECT_STORAGE, &[]);
    let vba_offset = append_record(&mut bytes, 0, 0, EXTERNAL_OLE_OBJECT_STORAGE, &[]);
    let directory_current_but_dead_offset =
      append_record(&mut bytes, 0x0f, 0, SLIDE_CONTAINER, &[]);
    let unreferenced_offset = append_record(&mut bytes, 0, 0, 0x779f, &[]);
    let persist_offsets = vec![
      document_offset,
      notes_master_offset,
      handout_master_offset,
      main_master_offset,
      title_master_offset,
      slide_offset,
      notes_offset,
      control_offset,
      embedded_ole_offset,
      linked_ole_offset,
      vba_offset,
      directory_current_but_dead_offset,
    ];
    let directory_body = PersistDirectoryAtom {
      entries: vec![PersistDirectoryEntry {
        first_persist_id: 1,
        stream_offsets: persist_offsets,
      }],
    }
    .to_bytes()
    .unwrap();
    let directory_offset = append_record(&mut bytes, 0, 0, PERSIST_DIRECTORY_ATOM, &directory_body);
    let user_edit = UserEditAtom {
      last_slide_id_ref: 1,
      version: 0,
      minor_version: 0,
      major_version: 3,
      offset_last_edit: 0,
      offset_persist_directory: directory_offset,
      doc_persist_id_ref: 1,
      persist_id_seed: 13,
      last_view: 1,
      unused: 0,
      encrypt_session_persist_id_ref: None,
    };
    let user_edit_offset = append_record(
      &mut bytes,
      0,
      0,
      USER_EDIT_ATOM,
      &write_fixed(&user_edit).unwrap(),
    );
    let current_user = CurrentUserAtom {
      fixed_size: 20,
      header_token: 0xe391_c05f,
      offset_to_current_edit: user_edit_offset,
      declared_user_name_byte_length: 0,
      document_file_version: 0x03f4,
      major_version: 3,
      minor_version: 0,
      unused: 0,
      ansi_user_name: Vec::new(),
      release_version: 8,
      unicode_user_name: None,
      trailing: Vec::new(),
    };

    let document = PowerPointDocument::from_bytes(&bytes).unwrap();
    let presentation = document.live_presentation(&current_user).unwrap();
    assert_eq!(presentation.document.reference.persist_id, 1);
    assert_eq!(
      presentation
        .notes_master_slide
        .unwrap()
        .reference
        .persist_id,
      2
    );
    assert_eq!(
      presentation
        .handout_master_slide
        .unwrap()
        .reference
        .persist_id,
      3
    );
    assert_eq!(
      presentation
        .master_slides
        .iter()
        .map(|value| (value.reference.persist_id, value.role))
        .collect::<Vec<_>>(),
      vec![
        (4, PptLivePersistObjectRole::MainMasterSlide),
        (5, PptLivePersistObjectRole::TitleMasterSlide),
      ]
    );
    assert_eq!(presentation.presentation_slides[0].reference.persist_id, 6);
    assert_eq!(presentation.notes_slides[0].reference.persist_id, 7);
    assert_eq!(presentation.active_x_controls[0].reference.persist_id, 8);
    assert_eq!(presentation.embedded_ole_objects[0].reference.persist_id, 9);
    assert_eq!(presentation.linked_ole_objects[0].reference.persist_id, 10);
    assert_eq!(presentation.vba_project.unwrap().reference.persist_id, 11);
    assert!(matches!(
      presentation.top_level_records[11].status,
      PptTopLevelLiveRecordStatus::Dead
    ));
    assert_eq!(
      presentation.top_level_records[11].stream_offset,
      u64::from(directory_current_but_dead_offset)
    );
    assert!(matches!(
      presentation.top_level_records[12].status,
      PptTopLevelLiveRecordStatus::Dead
    ));
    assert_eq!(
      presentation.top_level_records[12].stream_offset,
      u64::from(unreferenced_offset)
    );
    assert!(matches!(
      presentation.top_level_records[13].status,
      PptTopLevelLiveRecordStatus::LiveIncrementalSaveMetadata(
        IncrementalSaveMetadataKind::PersistDirectoryAtom
      )
    ));
    assert!(matches!(
      presentation.top_level_records[14].status,
      PptTopLevelLiveRecordStatus::LiveIncrementalSaveMetadata(
        IncrementalSaveMetadataKind::UserEditAtom
      )
    ));

    let mut compact = document.clone();
    let mut compact_current_user = current_user.clone();
    compact
      .rebuild_current_live_state(&mut compact_current_user)
      .unwrap();
    assert_eq!(compact.records.records.len(), 13);
    let compact_presentation = compact.live_presentation(&compact_current_user).unwrap();
    assert_eq!(
      compact_presentation
        .persist_object_directory
        .current_references
        .keys()
        .copied()
        .collect::<Vec<_>>(),
      (1..=11).collect::<Vec<_>>()
    );
    assert!(
      compact_presentation
        .top_level_records
        .iter()
        .all(|record| { !matches!(record.status, PptTopLevelLiveRecordStatus::Dead) })
    );
    assert_eq!(
      compact_presentation
        .top_level_records
        .iter()
        .filter(|record| matches!(
          record.status,
          PptTopLevelLiveRecordStatus::LiveIncrementalSaveMetadata(_)
        ))
        .count(),
      2
    );
    assert_eq!(
      compact_presentation
        .persist_object_directory
        .incremental_save_chain
        .edits
        .len(),
      1
    );
    assert_eq!(
      compact_presentation
        .persist_object_directory
        .incremental_save_chain
        .edits[0]
        .user_edit
        .offset_last_edit,
      0
    );
    let reopened_compact = PowerPointDocument::from_bytes(&compact.to_bytes().unwrap()).unwrap();
    let reopened_presentation = reopened_compact
      .live_presentation(&compact_current_user)
      .unwrap();
    assert_eq!(reopened_presentation, compact_presentation);

    let mut legacy_empty_vba = document.clone();
    let PptRecordData::Container(document_children) = &mut legacy_empty_vba.records.records[0].data
    else {
      unreachable!()
    };
    let document_info_list = document_children
      .records
      .iter_mut()
      .find(|record| record.header.record_type == DOCUMENT_INFO_LIST_CONTAINER)
      .unwrap();
    let PptRecordData::Container(document_info_list) = &mut document_info_list.data else {
      unreachable!()
    };
    let vba_info = document_info_list
      .records
      .iter_mut()
      .find(|record| record.header.record_type == VBA_INFO_CONTAINER)
      .unwrap();
    let PptRecordData::Container(vba_info) = &mut vba_info.data else {
      unreachable!()
    };
    let PptRecordData::VbaInfo(vba_info_atom) = &mut vba_info.records[0].data else {
      unreachable!()
    };
    vba_info_atom.persist_id_ref = 0;
    vba_info_atom.has_macros = 0;
    vba_info_atom.version = 1;
    assert!(legacy_empty_vba.live_presentation(&current_user).is_err());
    let compatible = legacy_empty_vba
      .live_presentation_compatible(&current_user)
      .unwrap();
    assert!(compatible.value.vba_project.is_none());
    assert_eq!(compatible.diagnostics.len(), 1);
    assert_eq!(
      compatible.diagnostics[0].code,
      ParseDiagnosticCode::NonconformingRecord
    );

    let mut wrong_target = document.clone();
    let PptRecordData::Container(document_children) = &mut wrong_target.records.records[0].data
    else {
      unreachable!()
    };
    let slide_list = document_children
      .records
      .iter_mut()
      .find(|record| {
        record.header.record_type == SLIDE_LIST_WITH_TEXT_CONTAINER && record.header.instance == 0
      })
      .unwrap();
    let PptRecordData::Container(slide_list) = &mut slide_list.data else {
      unreachable!()
    };
    let PptRecordData::SlidePersist(slide) = &mut slide_list.records[0].data else {
      unreachable!()
    };
    slide.persist_id_ref = 7;
    assert!(wrong_target.live_presentation(&current_user).is_err());
    let unchanged_wrong_target = wrong_target.clone();
    let mut unchanged_current_user = current_user.clone();
    assert!(
      wrong_target
        .rebuild_current_live_state(&mut unchanged_current_user)
        .is_err()
    );
    assert_eq!(wrong_target, unchanged_wrong_target);
    assert_eq!(unchanged_current_user, current_user);
  }

  #[test]
  fn text_special_info_masked_fields_round_trip() {
    let value = TextSpecialInfoAtom {
      runs: vec![TextSpecialInfoRun {
        character_count: 12,
        mask: 0x0000_0267,
        spelling_flags: Some(1),
        language_id: Some(0x0409),
        alternate_language_id: Some(0x0411),
        bidi: Some(1),
        pp10_extension: Some(0x8000_0003),
        smart_tag_indices: Some(vec![7, 9]),
      }],
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(TextSpecialInfoAtom::parse(&bytes), Some(value));
  }

  #[test]
  fn style_text_properties_use_text_context_and_masked_fields() {
    let value = StyleTextPropAtom {
      corresponding_text_character_count: 3,
      paragraph_runs: vec![TextParagraphRun {
        character_count: 4,
        indent_level: 2,
        properties: TextParagraphException {
          mask: 0x0010_000f,
          bullet_flags: Some(5),
          bullet_character: None,
          bullet_font_ref: None,
          bullet_size: None,
          bullet_color: None,
          text_alignment: None,
          line_spacing: None,
          space_before: None,
          space_after: None,
          left_margin: None,
          indent: None,
          default_tab_size: None,
          tab_stops: Some(vec![TextTabStop {
            position: 120,
            kind: 2,
          }]),
          font_alignment: None,
          wrap_flags: None,
          text_direction: None,
        },
      }],
      character_runs: vec![TextCharacterRun {
        character_count: 4,
        properties: TextCharacterException {
          mask: 0x0006_0001,
          font_style: Some(1),
          font_ref: None,
          old_east_asian_font_ref: None,
          ansi_font_ref: None,
          symbol_font_ref: None,
          font_size: Some(20),
          color: Some(0x0102_0304),
          position: None,
        },
      }],
      trailing: Vec::new(),
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(StyleTextPropAtom::parse(&bytes, 3), Some(value));
  }

  #[test]
  fn text_master_style_level_presence_follows_text_type() {
    let ordinary = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let parsed = TextMasterStyleAtom::parse(&ordinary, 1).unwrap();
    assert_eq!(parsed.levels.len(), 1);
    assert_eq!(parsed.levels[0].level, None);
    assert_eq!(parsed.to_bytes().unwrap(), ordinary);

    let centered = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let parsed = TextMasterStyleAtom::parse(&centered, 5).unwrap();
    assert_eq!(parsed.levels[0].level, Some(0));
    assert_eq!(parsed.to_bytes().unwrap(), centered);
  }

  #[test]
  fn text_ruler_mask_controls_tabs_margins_and_indents() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0000_010fu32.to_le_bytes());
    bytes.extend_from_slice(&3i16.to_le_bytes());
    bytes.extend_from_slice(&576u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&200i16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&100i16.to_le_bytes());
    bytes.extend_from_slice(&50i16.to_le_bytes());
    let parsed = TextRulerAtom::parse(&bytes).unwrap();
    assert_eq!(parsed.level_count, Some(3));
    assert_eq!(parsed.default_tab_size, Some(576));
    assert_eq!(parsed.tab_stops.as_ref().unwrap().len(), 1);
    assert_eq!(parsed.levels[0].left_margin, Some(100));
    assert_eq!(parsed.levels[0].indent, Some(50));
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn external_storage_zlib_wraps_native_compound_file() {
    let compound_file = CompoundFile::new(crate::cfb::Version::V3).unwrap();
    let value = ExternalStorageAtom::recompress(compound_file.clone()).unwrap();
    let body = value.to_bytes().unwrap();
    let reparsed = ExternalStorageAtom::parse(1, &body, Limits::default());
    let ExternalStorageAtom::Parsed(storage) = reparsed else {
      panic!("expected parsed compressed storage");
    };
    assert!(compound_file.logical_eq(&storage.compound_file));
    assert_eq!(storage.vba_project, ExternalStorageVba::NotPresent);
    assert_eq!(
      storage.encoding,
      match value {
        ExternalStorageAtom::Parsed(value) => value.encoding,
        _ => unreachable!(),
      }
    );
  }

  #[test]
  fn relayout_synchronizes_office_art_instances_and_bstore_counts() {
    use crate::office_art::{OfficeArtProperty, OfficeArtPropertyValue};

    let mut fopt_bytes = Vec::new();
    fopt_bytes.extend_from_slice(&3u16.to_le_bytes());
    fopt_bytes.extend_from_slice(&0xf00bu16.to_le_bytes());
    fopt_bytes.extend_from_slice(&0u32.to_le_bytes());
    let mut document = PowerPointDocument::from_bytes(&fopt_bytes).unwrap();
    {
      let PptRecordData::OfficeArt(office_art) = &mut document.records.records[0].data else {
        panic!("expected OfficeArtFOPT");
      };
      let OfficeArtRecordData::PropertyTable(table) = &mut office_art.data else {
        panic!("expected typed OfficeArt property table");
      };
      table.properties.push(OfficeArtProperty {
        property_id: 1,
        is_blip_id: false,
        value: OfficeArtPropertyValue::Simple(7),
      });
    }
    document.records.relayout(0, None, false).unwrap();
    assert_eq!(document.records.records[0].header.instance, 1);
    let PptRecordData::OfficeArt(office_art) = &document.records.records[0].data else {
      panic!("expected OfficeArtFOPT");
    };
    assert_eq!(office_art.header.instance, 1);
    assert_eq!(document.records.records[0].header.declared_length, 6);
    let written = document.to_bytes().unwrap();
    assert_eq!(PowerPointDocument::from_bytes(&written).unwrap(), document);

    let mut bstore_bytes = Vec::new();
    bstore_bytes.extend_from_slice(&0x001fu16.to_le_bytes());
    bstore_bytes.extend_from_slice(&0xf001u16.to_le_bytes());
    bstore_bytes.extend_from_slice(&0u32.to_le_bytes());
    let mut document = PowerPointDocument::from_bytes(&bstore_bytes).unwrap();
    document.records.relayout(0, None, true).unwrap();
    assert_eq!(document.records.records[0].header.instance, 1);
    document.records.relayout(0, None, false).unwrap();
    assert_eq!(document.records.records[0].header.instance, 0);
    let written = document.to_bytes().unwrap();
    assert_eq!(PowerPointDocument::from_bytes(&written).unwrap(), document);
  }
}
