//! Static structures for the Word 97-2007 binary (`.doc`) format.
//!
//! The FIB is the root of every Word 97+ document and is kept separate from
//! the remainder of the `WordDocument` stream so callers can replace or edit
//! it without losing physical bytes outside the structure.

mod file;

pub use file::{
  DocAnnotationBookmarkRef, DocBlockRef, DocBlocks, DocBookmarkRef, DocBookmarks,
  DocCharacterRunRef, DocChpxRun, DocCommentRef, DocComments, DocCompatibilityObjectStorage,
  DocContentTree, DocCp, DocCpRange, DocDataNode, DocDataNodeValue, DocDataStream,
  DocDirectCharacterFormatting, DocDirectFormatting, DocDirectFormattingRef,
  DocDirectParagraphFormatting, DocDirectTableState, DocDocumentPartRef, DocEmbeddedObjectStorage,
  DocFc, DocFcRange, DocFieldRef, DocFile, DocFkpPage, DocLocated, DocLocatedBookmarks,
  DocNoteKind, DocNoteRef, DocNotes, DocObjectPoolStorage, DocOfficeArtColor, DocOfficeArtFill,
  DocOfficeArtLine, DocOfficeArtPictureCrop, DocOfficeArtShapeRef, DocOfficeArtTextInsets,
  DocOfficeArtWrapDistances, DocOfficeArtWrapPolygonRef, DocOutlineLevel, DocPapxRun,
  DocParagraphKind, DocParagraphRef, DocParagraphStyleRef, DocRelationshipDiagnostic,
  DocSectionProperties, DocSectionRef, DocSections, DocShapeAnchorRef, DocSpecialContentLink,
  DocSpecialContentRef, DocStyleProperties, DocTableCellRef, DocTableCells, DocTableDiagnostic,
  DocTableRef, DocTableRowRef, DocTableRows, DocTableStream, DocTableStreamName, DocTables,
  DocTextPiece, DocTextPieceRef, DocTextPieceValueRef, DocTextRangeRef, DocTextboxBreakRef,
  DocTextboxShapeLink, DocTextboxStoryRef, DocTextboxes, DocWordDocumentStream,
};

use std::{
  collections::{BTreeMap, BTreeSet},
  io::Cursor,
  ops::Range,
  sync::Arc,
};

use bitflags::bitflags;

use crate::{
  Error, Result, SdkBitfield, SdkEnum, SdkObject,
  common::{CodePage, FileTime, Guid},
  io::{Reader, SdkRead, SdkWrite, Writer},
  limits::Limits,
  office_art::{
    OfficeArtDrawingGraph, OfficeArtImageRef, OfficeArtIncompleteRecordData,
    OfficeArtPartialRecord, OfficeArtPartialSequence, OfficeArtPartialStream, OfficeArtRecord,
    OfficeArtRecordData, OfficeArtStream, OfficeArtWordClientTextbox,
  },
  shared::NumberingFormat,
};

pub const WORD97_FILE_IDENTIFIER: u16 = 0xa5ec;

/// Absolute CFB path of the required MS-DOC `WordDocument` stream.
pub const WORD_DOCUMENT_STREAM_PATH: &str = "/WordDocument";
/// Absolute CFB path of the MS-DOC table stream selected when `fWhichTblStm` is clear.
pub const TABLE0_STREAM_PATH: &str = "/0Table";
/// Absolute CFB path of the MS-DOC table stream selected when `fWhichTblStm` is set.
pub const TABLE1_STREAM_PATH: &str = "/1Table";
/// Absolute CFB path of the optional MS-DOC `Data` stream.
pub const DATA_STREAM_PATH: &str = "/Data";
/// Absolute CFB path of the optional MS-DOC `ObjectPool` storage.
pub const OBJECT_POOL_STORAGE_PATH: &str = "/ObjectPool";
/// Fixed leaf name of an embedded object's required descriptor stream.
pub const OBJECT_INFO_STREAM_NAME: &str = "\u{3}ObjInfo";
pub const FIB_FC_LCB_CLX_INDEX: usize = 33;
pub const FIB_FC_LCB_GRP_XST_ATN_OWNERS_INDEX: usize = 36;
pub const FIB_FC_LCB_STTBF_ATN_BKMK_INDEX: usize = 37;
pub const FIB_FC_LCB_PLC_SPA_MOM_INDEX: usize = 40;
pub const FIB_FC_LCB_PLC_SPA_HDR_INDEX: usize = 41;
pub const FIB_FC_LCB_PLCF_ATN_BKF_INDEX: usize = 42;
pub const FIB_FC_LCB_PLCF_ATN_BKL_INDEX: usize = 43;
pub const FIB_FC_LCB_PMS_INDEX: usize = 44;
pub const FIB_FC_LCB_PLCF_FND_REF_INDEX: usize = 2;
pub const FIB_FC_LCB_PLCF_FND_TXT_INDEX: usize = 3;
pub const FIB_FC_LCB_PLCF_AND_REF_INDEX: usize = 4;
pub const FIB_FC_LCB_PLCF_AND_TXT_INDEX: usize = 5;
pub const FIB_FC_LCB_PLCF_SED_INDEX: usize = 6;
pub const FIB_FC_LCB_STSHF_INDEX: usize = 1;
pub const FIB_FC_LCB_PLC_BTE_CHPX_INDEX: usize = 12;
pub const FIB_FC_LCB_PLC_BTE_PAPX_INDEX: usize = 13;
pub const FIB_FC_LCB_STTBF_FFN_INDEX: usize = 15;
pub const FIB_FC_LCB_PLCF_HDD_INDEX: usize = 11;
pub const FIB_FC_LCB_PLCF_FLD_MOM_INDEX: usize = 16;
pub const FIB_FC_LCB_PLCF_FLD_HDR_INDEX: usize = 17;
pub const FIB_FC_LCB_PLCF_FLD_FTN_INDEX: usize = 18;
pub const FIB_FC_LCB_PLCF_FLD_ATN_INDEX: usize = 19;
pub const FIB_FC_LCB_PLCF_FLD_MCR_INDEX: usize = 20;
pub const FIB_FC_LCB_STTBF_BKMK_INDEX: usize = 21;
pub const FIB_FC_LCB_PLCF_BKF_INDEX: usize = 22;
pub const FIB_FC_LCB_PLCF_BKL_INDEX: usize = 23;
pub const FIB_FC_LCB_PLCF_FLD_EDN_INDEX: usize = 48;
pub const FIB_FC_LCB_PLCF_END_REF_INDEX: usize = 46;
pub const FIB_FC_LCB_PLCF_END_TXT_INDEX: usize = 47;
pub const FIB_FC_LCB_DGG_INFO_INDEX: usize = 50;
pub const FIB_FC_LCB_STTBF_RMARK_INDEX: usize = 51;
pub const FIB_FC_LCB_STTBF_CAPTION_INDEX: usize = 52;
pub const FIB_FC_LCB_STTBF_AUTO_CAPTION_INDEX: usize = 53;
pub const FIB_FC_LCB_PLCF_WKB_INDEX: usize = 54;
pub const FIB_FC_LCB_STW_USER_INDEX: usize = 60;
pub const FIB_FC_LCB_STTB_TTMBD_INDEX: usize = 61;
pub const FIB_FC_LCB_COOKIE_DATA_INDEX: usize = 62;
pub const FIB_FC_LCB_PLCF_SPL_INDEX: usize = 55;
pub const FIB_FC_LCB_STTB_LIST_NAMES_INDEX: usize = 91;
pub const FIB_FC_LCB_PLCF_GRAM_INDEX: usize = 90;
pub const FIB_FC_LCB_PLCF_LAD_INDEX: usize = 98;
pub const FIB_LAST_SAVED_FILETIME_INDEX: usize = 87;
pub const FIB_FC_LCB_PLCF_TCH_INDEX: usize = 93;
pub const FIB_FC_LCB_RMD_THREADING_INDEX: usize = 94;
pub const FIB_FC_LCB_STTB_RGTPLC_INDEX: usize = 96;
pub const FIB_FC_LCB_MSO_ENVELOPE_INDEX: usize = 97;
pub const FIB_FC_LCB_RG_DOFR_INDEX: usize = 99;
pub const FIB_FC_LCB_PLF_COSL_INDEX: usize = 100;
pub const FIB_FC_LCB_PLCF_COOKIE_OLD_INDEX: usize = 101;
pub const FIB_FC_LCB_PLF_GOSL_INDEX: usize = 84;
pub const FIB_FC_LCB_PLCF_ASUMY_INDEX: usize = 89;
pub const FIB_FC_LCB_PLCF_FACTOID_INDEX: usize = 132;
pub const FIB_FC_LCB_HPLXSDR_INDEX: usize = 136;
pub const FIB_FC_LCB_STTBF_BKMK_SDT_INDEX: usize = 137;
pub const FIB_FC_LCB_PLCF_BKF_SDT_INDEX: usize = 138;
pub const FIB_FC_LCB_PLCF_BKL_SDT_INDEX: usize = 139;
pub const FIB_FC_LCB_CUSTOM_XFORM_INDEX: usize = 140;
pub const FIB_FC_LCB_STTBF_BKMK_PROT_INDEX: usize = 141;
pub const FIB_FC_LCB_PLCF_BKF_PROT_INDEX: usize = 142;
pub const FIB_FC_LCB_PLCF_BKL_PROT_INDEX: usize = 143;
pub const FIB_FC_LCB_STTB_PROT_USER_INDEX: usize = 144;
pub const FIB_FC_LCB_PLCF_PGP_INDEX: usize = 109;
pub const FIB_FC_LCB_PLCF_UIM_INDEX: usize = 110;
pub const FIB_FC_LCB_PLF_GUID_UIM_INDEX: usize = 111;
pub const FIB_FC_LCB_STTB_SAVED_BY_INDEX: usize = 71;
pub const FIB_FC_LCB_STTB_FNM_INDEX: usize = 72;
pub const FIB_FC_LCB_STTBF_BKMK_FACTOID_INDEX: usize = 114;
pub const FIB_FC_LCB_PLCF_BKF_FACTOID_INDEX: usize = 115;
pub const FIB_FC_LCB_PLCF_BKL_FACTOID_INDEX: usize = 117;
pub const FIB_FC_LCB_PLCF_COOKIE_INDEX: usize = 116;
pub const FIB_FC_LCB_FACTOID_DATA_INDEX: usize = 118;
pub const FIB_FC_LCB_STTBF_BKMK_FCC_INDEX: usize = 120;
pub const FIB_FC_LCB_PLCF_BKF_FCC_INDEX: usize = 121;
pub const FIB_FC_LCB_PLCF_BKL_FCC_INDEX: usize = 122;
pub const FIB_FC_LCB_STTBF_BKMK_BP_REPAIRS_INDEX: usize = 123;
pub const FIB_FC_LCB_PLCF_BKF_BP_REPAIRS_INDEX: usize = 124;
pub const FIB_FC_LCB_PLCF_BKL_BP_REPAIRS_INDEX: usize = 125;
pub const FIB_FC_LCB_PMS_NEW_INDEX: usize = 126;
pub const FIB_FC_LCB_ODSO_INDEX: usize = 127;
pub const FIB_FC_LCB_ATRD_EXTRA_INDEX: usize = 112;
pub const FIB_FC_LCB_PLRSID_INDEX: usize = 113;
pub const FIB_FC_LCB_PLCF_TXBX_TXT_INDEX: usize = 56;
pub const FIB_FC_LCB_PLCF_FLD_TXBX_INDEX: usize = 57;
pub const FIB_FC_LCB_PLCF_HDR_TXBX_TXT_INDEX: usize = 58;
pub const FIB_FC_LCB_PLCF_FLD_HDR_TXBX_INDEX: usize = 59;
pub const FIB_FC_LCB_PLCF_TXBX_BKD_INDEX: usize = 75;
pub const FIB_FC_LCB_PLCF_HDR_TXBX_BKD_INDEX: usize = 76;
pub const FIB_FC_LCB_DOP_INDEX: usize = 31;
pub const FIB_FC_LCB_STTBF_ASSOC_INDEX: usize = 32;
pub const FIB_FC_LCB_WSS_INDEX: usize = 30;
pub const FIB_FC_LCB_CMDS_INDEX: usize = 24;
pub const FIB_FC_LCB_PR_DRVR_INDEX: usize = 27;
pub const FIB_FC_LCB_PLF_LST_INDEX: usize = 73;
pub const FIB_FC_LCB_PLF_LFO_INDEX: usize = 74;
pub const FIB_FC_LCB_RGX_OCX_INFO_INDEX: usize = 85;
pub const FIB_FC_LCB_PLCF_BTE_LVC_INDEX: usize = 86;
pub const SPRM_P_CHG_TABS: u16 = 0xc615;
pub const SPRM_T_DEF_TABLE: u16 = 0xd608;
const UNKNOWN_REVISION_AUTHOR: [u16; 7] = [0x0055, 0x006e, 0x006b, 0x006e, 0x006f, 0x0077, 0x006e];

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FibBaseFlags: u16 {
        const DOCUMENT_TEMPLATE = 0x0001;
        const GLOSSARY_DOCUMENT = 0x0002;
        const COMPLEX = 0x0004;
        const HAS_PICTURES = 0x0008;
        const ENCRYPTED = 0x0100;
        const USE_1_TABLE = 0x0200;
        const READ_ONLY_RECOMMENDED = 0x0400;
        const WRITE_RESERVATION = 0x0800;
        const EXTENDED_CHARACTERS = 0x1000;
        const LOAD_OVERRIDE = 0x2000;
        const FAR_EAST = 0x4000;
        const OBFUSCATED = 0x8000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FibBaseEnvironmentFlags: u8 {
        const MAC = 0x01;
        const EMPTY_SPECIAL = 0x02;
        const LOAD_OVERRIDE_PAGE = 0x04;
        const RESERVED_1 = 0x08;
        const RESERVED_2 = 0x10;
        const SPARE_0 = 0xe0;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FibBase {
  pub file_identifier: u16,
  pub n_fib: u16,
  pub unused: u16,
  pub language_id: u16,
  pub next_fib_page: u16,
  pub flags: FibBaseFlags,
  pub quick_save_count: u8,
  pub n_fib_back: u16,
  pub encryption_key_or_header_size: u32,
  pub environment: u8,
  pub environment_flags: FibBaseEnvironmentFlags,
  pub reserved3: u16,
  pub reserved4: u16,
  pub reserved5: u32,
  pub reserved6: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FibRgW97 {
  pub reserved: [u16; 13],
  pub far_east_language_id: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FibRgLw97 {
  pub cb_mac: u32,
  pub reserved1: u32,
  pub reserved2: u32,
  pub ccp_text: i32,
  pub ccp_footnote: i32,
  pub ccp_header: i32,
  pub reserved3: u32,
  pub ccp_comment: i32,
  pub ccp_endnote: i32,
  pub ccp_textbox: i32,
  pub ccp_header_textbox: i32,
  pub reserved: [u32; 11],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FibFcLcb {
  pub fc: u32,
  pub lcb: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FieldDocumentPart {
  Main,
  Header,
  Footnote,
  Comment,
  Macro,
  Endnote,
  Textbox,
  HeaderTextbox,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextboxDocumentPart {
  Main,
  Header,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FibVersion {
  Word97,
  Word2000,
  Word2002,
  Word2003,
  Word2007,
  Compatibility(u16),
}

impl FibVersion {
  pub fn n_fib(self) -> u16 {
    match self {
      Self::Word97 => 0x00c1,
      Self::Word2000 => 0x00d9,
      Self::Word2002 => 0x0101,
      Self::Word2003 => 0x010c,
      Self::Word2007 => 0x0112,
      Self::Compatibility(value) => value,
    }
  }

  pub fn documented_fc_lcb_count(self) -> Option<usize> {
    Some(match self {
      Self::Word97 => 0x005d,
      Self::Word2000 => 0x006c,
      Self::Word2002 => 0x0088,
      Self::Word2003 => 0x00a4,
      Self::Word2007 => 0x00b7,
      Self::Compatibility(_) => return None,
    })
  }

  fn from_n_fib(value: u16) -> Self {
    match value {
      0x00c1 => Self::Word97,
      0x00d9 => Self::Word2000,
      0x0101 => Self::Word2002,
      0x010c => Self::Word2003,
      0x0112 => Self::Word2007,
      _ => Self::Compatibility(value),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FibRgCswNew {
  None,
  Word2000 {
    n_fib_new: u16,
    quick_save_count: u16,
  },
  Word2007 {
    n_fib_new: u16,
    quick_save_count: u16,
    theme_language_other: u16,
    theme_language_far_east: u16,
    theme_language_complex_script: u16,
  },
  Compatibility {
    words: Vec<u16>,
  },
}

impl FibRgCswNew {
  pub fn word_count(&self) -> usize {
    match self {
      Self::None => 0,
      Self::Word2000 { .. } => 2,
      Self::Word2007 { .. } => 5,
      Self::Compatibility { words } => words.len(),
    }
  }

  fn n_fib_new(&self) -> Option<u16> {
    match self {
      Self::None => None,
      Self::Word2000 { n_fib_new, .. } | Self::Word2007 { n_fib_new, .. } => Some(*n_fib_new),
      Self::Compatibility { words } => words.first().copied(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fib {
  pub base: FibBase,
  pub rg_w: FibRgW97,
  pub rg_lw: FibRgLw97,
  pub fc_lcb: Vec<FibFcLcb>,
  pub csw_new: FibRgCswNew,
}

impl Fib {
  pub fn from_word_document(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let base = FibBase::read(&mut input)?;
    if base.file_identifier != WORD97_FILE_IDENTIFIER {
      return Err(Error::invalid(
        0,
        "WordDocument does not contain a Word 97+ FIB",
      ));
    }

    let csw = input.u16()?;
    if csw != 14 {
      return Err(Error::invalid(32, format!("Fib.csw is {csw}, expected 14")));
    }
    let rg_w = FibRgW97::read(&mut input)?;

    let cslw = input.u16()?;
    if cslw != 22 {
      return Err(Error::invalid(
        62,
        format!("Fib.cslw is {cslw}, expected 22"),
      ));
    }
    let rg_lw = FibRgLw97::read(&mut input)?;

    let fc_lcb_count = usize::from(input.u16()?);
    let mut fc_lcb = Vec::with_capacity(fc_lcb_count);
    for _ in 0..fc_lcb_count {
      fc_lcb.push(FibFcLcb {
        fc: input.u32()?,
        lcb: input.u32()?,
      });
    }

    let csw_new_count = usize::from(input.u16()?);
    if csw_new_count > 1024 {
      return Err(Error::Limit(format!(
        "Fib.cswNew count {csw_new_count} exceeds 1024"
      )));
    }
    let mut csw_new_words = Vec::with_capacity(csw_new_count);
    for _ in 0..csw_new_count {
      csw_new_words.push(input.u16()?);
    }
    let csw_new = match csw_new_words.as_slice() {
      [] => FibRgCswNew::None,
      [n_fib_new, quick_save_count] if matches!(*n_fib_new, 0x00d9 | 0x0101 | 0x010c) => {
        FibRgCswNew::Word2000 {
          n_fib_new: *n_fib_new,
          quick_save_count: *quick_save_count,
        }
      }
      [
        0x0112,
        quick_save_count,
        theme_language_other,
        theme_language_far_east,
        theme_language_complex_script,
      ] => FibRgCswNew::Word2007 {
        n_fib_new: 0x0112,
        quick_save_count: *quick_save_count,
        theme_language_other: *theme_language_other,
        theme_language_far_east: *theme_language_far_east,
        theme_language_complex_script: *theme_language_complex_script,
      },
      _ => FibRgCswNew::Compatibility {
        words: csw_new_words,
      },
    };

    Ok(Self {
      base,
      rg_w,
      rg_lw,
      fc_lcb,
      csw_new,
    })
  }

  pub fn version(&self) -> FibVersion {
    FibVersion::from_n_fib(self.csw_new.n_fib_new().unwrap_or(self.base.n_fib))
  }

  pub fn encoded_len(&self) -> usize {
    32 + 2 + 28 + 2 + 88 + 2 + self.fc_lcb.len() * 8 + 2 + self.csw_new.word_count() * 2
  }

  pub fn fc_lcb(&self, index: usize) -> Option<FibFcLcb> {
    self.fc_lcb.get(index).copied()
  }

  pub(crate) fn relocate_table_locations(
    &mut self,
    mut relocate: impl FnMut(FibFcLcb) -> Result<Option<FibFcLcb>>,
  ) -> Result<()> {
    for (index, location) in self.fc_lcb.iter_mut().enumerate() {
      // This pair is a FILETIME split across the fc/lcb words, not a
      // Table Stream range (MS-DOC FibRgFcLcb2000).
      if index == FIB_LAST_SAVED_FILETIME_INDEX || location.lcb == 0 {
        continue;
      }
      if let Some(relocated) = relocate(*location)? {
        *location = relocated;
      }
    }
    Ok(())
  }

  pub fn clx_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_CLX_INDEX)
  }

  pub fn section_table_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLCF_SED_INDEX)
  }

  pub fn style_sheet_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_STSHF_INDEX)
  }

  pub fn chpx_bte_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLC_BTE_CHPX_INDEX)
  }

  pub fn papx_bte_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLC_BTE_PAPX_INDEX)
  }

  pub fn font_table_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_STTBF_FFN_INDEX)
  }

  pub fn field_table_locations(&self) -> Vec<(FieldDocumentPart, FibFcLcb)> {
    [
      (FieldDocumentPart::Main, FIB_FC_LCB_PLCF_FLD_MOM_INDEX),
      (FieldDocumentPart::Header, FIB_FC_LCB_PLCF_FLD_HDR_INDEX),
      (FieldDocumentPart::Footnote, FIB_FC_LCB_PLCF_FLD_FTN_INDEX),
      (FieldDocumentPart::Comment, FIB_FC_LCB_PLCF_FLD_ATN_INDEX),
      (FieldDocumentPart::Macro, FIB_FC_LCB_PLCF_FLD_MCR_INDEX),
      (FieldDocumentPart::Endnote, FIB_FC_LCB_PLCF_FLD_EDN_INDEX),
      (FieldDocumentPart::Textbox, FIB_FC_LCB_PLCF_FLD_TXBX_INDEX),
      (
        FieldDocumentPart::HeaderTextbox,
        FIB_FC_LCB_PLCF_FLD_HDR_TXBX_INDEX,
      ),
    ]
    .into_iter()
    .filter_map(|(part, index)| self.fc_lcb(index).map(|location| (part, location)))
    .collect()
  }

  pub fn bookmark_locations(&self) -> Option<(FibFcLcb, FibFcLcb, FibFcLcb)> {
    Some((
      self.fc_lcb(FIB_FC_LCB_STTBF_BKMK_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_BKF_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_BKL_INDEX)?,
    ))
  }

  pub fn header_text_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLCF_HDD_INDEX)
  }

  pub fn footnote_locations(&self) -> Option<(FibFcLcb, FibFcLcb)> {
    Some((
      self.fc_lcb(FIB_FC_LCB_PLCF_FND_REF_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_FND_TXT_INDEX)?,
    ))
  }

  pub fn endnote_locations(&self) -> Option<(FibFcLcb, FibFcLcb)> {
    Some((
      self.fc_lcb(FIB_FC_LCB_PLCF_END_REF_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_END_TXT_INDEX)?,
    ))
  }

  pub fn annotation_locations(&self) -> Option<(FibFcLcb, FibFcLcb)> {
    Some((
      self.fc_lcb(FIB_FC_LCB_PLCF_AND_REF_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_AND_TXT_INDEX)?,
    ))
  }

  pub fn annotation_owner_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_GRP_XST_ATN_OWNERS_INDEX)
  }

  pub fn annotation_bookmark_locations(&self) -> Option<(FibFcLcb, FibFcLcb, FibFcLcb)> {
    Some((
      self.fc_lcb(FIB_FC_LCB_STTBF_ATN_BKMK_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_ATN_BKF_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_ATN_BKL_INDEX)?,
    ))
  }

  pub fn mail_merge_state_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PMS_INDEX)
  }

  pub fn textbox_story_locations(&self) -> Vec<(TextboxDocumentPart, FibFcLcb)> {
    [
      (TextboxDocumentPart::Main, FIB_FC_LCB_PLCF_TXBX_TXT_INDEX),
      (
        TextboxDocumentPart::Header,
        FIB_FC_LCB_PLCF_HDR_TXBX_TXT_INDEX,
      ),
    ]
    .into_iter()
    .filter_map(|(part, index)| self.fc_lcb(index).map(|location| (part, location)))
    .collect()
  }

  pub fn textbox_break_locations(&self) -> Vec<(TextboxDocumentPart, FibFcLcb)> {
    [
      (TextboxDocumentPart::Main, FIB_FC_LCB_PLCF_TXBX_BKD_INDEX),
      (
        TextboxDocumentPart::Header,
        FIB_FC_LCB_PLCF_HDR_TXBX_BKD_INDEX,
      ),
    ]
    .into_iter()
    .filter_map(|(part, index)| self.fc_lcb(index).map(|location| (part, location)))
    .collect()
  }

  pub fn shape_anchor_locations(&self) -> Vec<(TextboxDocumentPart, FibFcLcb)> {
    [
      (TextboxDocumentPart::Main, FIB_FC_LCB_PLC_SPA_MOM_INDEX),
      (TextboxDocumentPart::Header, FIB_FC_LCB_PLC_SPA_HDR_INDEX),
    ]
    .into_iter()
    .filter_map(|(part, index)| self.fc_lcb(index).map(|location| (part, location)))
    .collect()
  }

  pub fn office_art_content_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_DGG_INFO_INDEX)
  }

  pub fn revision_authors_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_STTBF_RMARK_INDEX)
  }

  pub fn caption_locations(&self) -> Option<(FibFcLcb, FibFcLcb)> {
    Some((
      self.fc_lcb(FIB_FC_LCB_STTBF_CAPTION_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_STTBF_AUTO_CAPTION_INDEX)?,
    ))
  }

  pub fn subdocuments_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLCF_WKB_INDEX)
  }

  pub fn user_variables_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_STW_USER_INDEX)
  }

  pub fn embedded_fonts_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_STTB_TTMBD_INDEX)
  }

  pub fn grammar_cookie_data_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_COOKIE_DATA_INDEX)
  }

  pub fn spelling_state_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLCF_SPL_INDEX)
  }

  pub fn list_names_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_STTB_LIST_NAMES_INDEX)
  }

  pub fn grammar_state_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLCF_GRAM_INDEX)
  }

  pub fn language_detection_state_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLCF_LAD_INDEX)
  }

  pub fn last_saved_file_time(&self) -> Option<FileTime> {
    let value = self.fc_lcb(FIB_LAST_SAVED_FILETIME_INDEX)?;
    Some(FileTime::from_parts(value.fc, value.lcb))
  }

  pub fn table_character_cache_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLCF_TCH_INDEX)
  }

  pub fn revision_message_threading_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_RMD_THREADING_INDEX)
  }

  pub fn list_style_templates_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_STTB_RGTPLC_INDEX)
  }

  pub fn mso_envelope_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_MSO_ENVELOPE_INDEX)
  }

  pub fn frame_and_list_records_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_RG_DOFR_INDEX)
  }

  pub fn grammar_option_sets_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLF_COSL_INDEX)
  }

  pub fn legacy_grammar_option_sets_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLF_GOSL_INDEX)
  }

  pub fn auto_summary_ranges_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLCF_ASUMY_INDEX)
  }

  pub fn smart_tag_recognizer_state_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLCF_FACTOID_INDEX)
  }

  pub fn xml_schema_references_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_HPLXSDR_INDEX)
  }

  pub fn structured_tag_bookmark_locations(&self) -> Option<[FibFcLcb; 3]> {
    Some([
      self.fc_lcb(FIB_FC_LCB_STTBF_BKMK_SDT_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_BKF_SDT_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_BKL_SDT_INDEX)?,
    ])
  }

  pub fn xml_transform_path_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_CUSTOM_XFORM_INDEX)
  }

  pub fn range_protection_locations(&self) -> Option<[FibFcLcb; 4]> {
    Some([
      self.fc_lcb(FIB_FC_LCB_STTBF_BKMK_PROT_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_BKF_PROT_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_BKL_PROT_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_STTB_PROT_USER_INDEX)?,
    ])
  }

  pub fn paragraph_group_properties_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLCF_PGP_INDEX)
  }

  pub fn user_input_method_locations(&self) -> Option<[FibFcLcb; 2]> {
    Some([
      self.fc_lcb(FIB_FC_LCB_PLCF_UIM_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLF_GUID_UIM_INDEX)?,
    ])
  }

  pub fn save_history_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_STTB_SAVED_BY_INDEX)
  }

  pub fn external_file_names_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_STTB_FNM_INDEX)
  }

  pub fn smart_tag_bookmark_locations(&self) -> Option<[FibFcLcb; 3]> {
    Some([
      self.fc_lcb(FIB_FC_LCB_STTBF_BKMK_FACTOID_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_BKF_FACTOID_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_BKL_FACTOID_INDEX)?,
    ])
  }

  pub fn grammar_checker_cookies_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLCF_COOKIE_INDEX)
  }

  pub fn legacy_grammar_checker_cookies_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLCF_COOKIE_OLD_INDEX)
  }

  pub fn smart_tag_data_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_FACTOID_DATA_INDEX)
  }

  pub fn format_consistency_bookmark_locations(&self) -> Option<[FibFcLcb; 3]> {
    Some([
      self.fc_lcb(FIB_FC_LCB_STTBF_BKMK_FCC_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_BKF_FCC_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_BKL_FCC_INDEX)?,
    ])
  }

  pub fn repair_bookmark_locations(&self) -> Option<[FibFcLcb; 3]> {
    Some([
      self.fc_lcb(FIB_FC_LCB_STTBF_BKMK_BP_REPAIRS_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_BKF_BP_REPAIRS_INDEX)?,
      self.fc_lcb(FIB_FC_LCB_PLCF_BKL_BP_REPAIRS_INDEX)?,
    ])
  }

  pub fn new_mail_merge_state_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PMS_NEW_INDEX)
  }

  pub fn office_data_source_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_ODSO_INDEX)
  }

  pub fn annotation_extended_data_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_ATRD_EXTRA_INDEX)
  }

  pub fn revision_save_ids_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLRSID_INDEX)
  }

  pub fn list_definition_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLF_LST_INDEX)
  }

  pub fn list_override_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLF_LFO_INDEX)
  }

  pub fn deprecated_numbering_field_cache_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PLCF_BTE_LVC_INDEX)
  }

  pub fn document_properties_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_DOP_INDEX)
  }

  pub fn associated_strings_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_STTBF_ASSOC_INDEX)
  }

  pub fn selection_state_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_WSS_INDEX)
  }

  pub fn command_customizations_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_CMDS_INDEX)
  }

  pub fn printer_driver_info_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_PR_DRVR_INDEX)
  }

  pub fn ole_control_info_location(&self) -> Option<FibFcLcb> {
    self.fc_lcb(FIB_FC_LCB_RGX_OCX_INFO_INDEX)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.base.file_identifier != WORD97_FILE_IDENTIFIER {
      return Err(Error::invalid(0, "FIB framing changed"));
    }
    if self.base.quick_save_count > 0x0f {
      return Err(Error::invalid(
        10,
        "FibBase quick-save count exceeds four bits",
      ));
    }
    let mut bytes = Vec::with_capacity(self.encoded_len());
    self.base.write(&mut bytes);
    push_u16(&mut bytes, 14);
    self.rg_w.write(&mut bytes);
    push_u16(&mut bytes, 22);
    self.rg_lw.write(&mut bytes);
    push_u16(
      &mut bytes,
      u16::try_from(self.fc_lcb.len())
        .map_err(|_| Error::Limit("FIB Fc/Lcb count exceeds u16".into()))?,
    );
    for pair in &self.fc_lcb {
      push_u32(&mut bytes, pair.fc);
      push_u32(&mut bytes, pair.lcb);
    }
    push_u16(
      &mut bytes,
      u16::try_from(self.csw_new.word_count())
        .map_err(|_| Error::Limit("FIB cswNew count exceeds u16".into()))?,
    );
    match self.csw_new {
      FibRgCswNew::None => {}
      FibRgCswNew::Word2000 {
        n_fib_new,
        quick_save_count,
      } => {
        push_u16(&mut bytes, n_fib_new);
        push_u16(&mut bytes, quick_save_count);
      }
      FibRgCswNew::Word2007 {
        n_fib_new,
        quick_save_count,
        theme_language_other,
        theme_language_far_east,
        theme_language_complex_script,
      } => {
        push_u16(&mut bytes, n_fib_new);
        push_u16(&mut bytes, quick_save_count);
        push_u16(&mut bytes, theme_language_other);
        push_u16(&mut bytes, theme_language_far_east);
        push_u16(&mut bytes, theme_language_complex_script);
      }
      FibRgCswNew::Compatibility { ref words } => {
        for value in words {
          push_u16(&mut bytes, *value);
        }
      }
    }
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clx {
  pub property_runs: Vec<Prc>,
  pub piece_table: PlcPcd,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlcBte {
  pub file_positions: Vec<u32>,
  pub pages: Vec<FkpPageNumber>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlcfSed {
  pub character_positions: Vec<i32>,
  pub sections: Vec<Sed>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpOnlyTable {
  pub positions: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderTextTable {
  pub boundaries: Vec<HeaderStoryBoundary>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderStoryBoundary {
  Position(u32),
  Missing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteReferenceTable {
  pub positions: Vec<u32>,
  pub indices: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotationReferenceTable {
  pub positions: Vec<u32>,
  pub annotations: Vec<AnnotationReference>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotationReference {
  pub initials_length: u16,
  pub initials_buffer: [u16; 9],
  pub author_index: i16,
  pub bits_not_used: u16,
  pub flags_not_used: u16,
  pub bookmark_tag: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotationExtendedData {
  pub comments: Vec<AnnotationPost10>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotationPost10 {
  pub modified: Dttm,
  pub padding1: u16,
  pub depth: u32,
  pub parent_offset: i32,
  pub ows_discussion_item: bool,
  pub ink: bool,
  pub padding2: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserInputMethods {
  pub positions: Vec<u32>,
  pub methods: Vec<UserInputMethod>,
  pub service_guids: Vec<[u8; 16]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserInputMethod {
  pub service_category_index: i16,
  pub service_clsid_index: i16,
  pub service_data_offset: i32,
  pub character_count: i32,
  pub service_data_size: u32,
  pub private_data: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrinterDriverInfo {
  pub printer_name: Vec<u8>,
  pub port_name: Vec<u8>,
  pub driver_name: Vec<u8>,
  pub product_name: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OleControlInfos {
  pub controls: Vec<OleControlInfo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OleControlInfo {
  pub cookie: u32,
  pub field_index: u32,
  pub ignored_accelerator_handle: u32,
  pub accelerator_count: u16,
  pub field_linked: bool,
  pub eats_return: bool,
  pub eats_escape: bool,
  pub default_button: bool,
  pub cancel_button: bool,
  pub failed_load: bool,
  pub right_to_left: bool,
  pub corrupt: bool,
  pub reserved1: u8,
  pub document_part: OleControlDocumentPart,
  pub reserved2: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OleControlDocumentPart {
  Main,
  Header,
  Footnote,
  Textbox,
  Endnote,
  Comment,
  HeaderTextbox,
  Compatibility(u16),
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct OleObjectPersist1Flags: u16 {
        const RESERVED1 = 1 << 0;
        const DEFAULT_HANDLER = 1 << 1;
        const RESERVED2 = 1 << 2;
        const RESERVED3 = 1 << 3;
        const LINK = 1 << 4;
        const RESERVED4 = 1 << 5;
        const ICON = 1 << 6;
        const OLE1_ONLY = 1 << 7;
        const MANUAL_UPDATE = 1 << 8;
        const RECOMPOSE_ON_RESIZE = 1 << 9;
        const RESERVED_MUST_ZERO_1 = 1 << 10;
        const RESERVED_MUST_ZERO_2 = 1 << 11;
        const OCX = 1 << 12;
        const STREAM = 1 << 13;
        const RESERVED7 = 1 << 14;
        const VIEW_OBJECT = 1 << 15;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct OleObjectPersist2Flags: u16 {
        const EMF_PRESENTATION = 1 << 0;
        const RESERVED_MUST_ZERO = 1 << 1;
        const QUERIED_EMF = 1 << 2;
        const STORED_AS_EMF = 1 << 3;
        const RESERVED2 = 1 << 4;
        const RESERVED3 = 1 << 5;
        const RESERVED4 = 0xffc0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OleObjectClipboardFormat {
  RichText,
  Text,
  Metafile,
  Bitmap,
  Dib,
  Html,
  UnicodeText,
  Compatibility(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OleObjectDescriptor {
  pub persist1: OleObjectPersist1Flags,
  pub clipboard_format: OleObjectClipboardFormat,
  pub persist2: Option<OleObjectPersist2Flags>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotationOwners {
  pub names: Vec<Vec<u16>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotationBookmarkInfo {
  pub bookmark_class: u16,
  pub tag: i32,
  pub old_tag: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotationBookmarkInfos {
  pub present: bool,
  pub extended_marker: u16,
  pub extra_data_size: u16,
  pub entries: Vec<AnnotationBookmarkInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotationBookmarks {
  pub infos: AnnotationBookmarkInfos,
  pub starts: BookmarkStartTable,
  pub ends: BookmarkEndTable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextboxStoryTable {
  pub positions: Vec<u32>,
  pub stories: Vec<TextboxStory>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextboxStory {
  pub chain: TextboxStoryChain,
  pub reusable_flags: u16,
  pub destination_index: u32,
  pub shape_id: u32,
  pub undo_transaction_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextboxStoryChain {
  NonReusable {
    textbox_count: i32,
    edited_textbox_count: i32,
  },
  Reusable {
    next_reusable_index: i32,
    reusable_count: i32,
  },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextboxBreakTable {
  pub positions: Vec<u32>,
  pub breaks: Vec<TextboxBreak>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextboxBreak {
  pub story_index: i16,
  pub dependent_character_count: u16,
  pub reserved1: u16,
  pub mark_delete: bool,
  pub unused: bool,
  pub text_overflow: bool,
  pub reserved2: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapeAnchorTable {
  pub positions: Vec<u32>,
  pub anchors: Vec<ShapeAnchor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeAnchor {
  pub shape_id: u32,
  pub rectangle: ShapeAnchorRectangle,
  pub header: bool,
  pub horizontal_origin: u8,
  pub vertical_origin: u8,
  pub wrap_style: u8,
  pub wrap_side: u8,
  pub simple_rectangle: bool,
  pub below_text: bool,
  pub anchor_locked: bool,
  pub textbox_count: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeAnchorRectangle {
  pub left: i32,
  pub top: i32,
  pub right: i32,
  pub bottom: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocOfficeArtContent {
  pub drawing_group: DocOfficeArtRecordTree,
  pub drawings: Vec<OfficeArtWordDrawing>,
}

/// Zero-copy resolution of one document-wide OfficeArt BLIP-store entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocOfficeArtImageLink<'a> {
  Resolved(OfficeArtImageRef<'a>),
  Delayed { word_document_offset: u32 },
  Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeArtWordDrawing {
  pub document_part: TextboxDocumentPart,
  pub container: DocOfficeArtRecordTree,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocOfficeArtRecordTree {
  Complete(OfficeArtStream),
  Partial(OfficeArtPartialStream),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListDefinitions {
  pub levels_in_declared_length: bool,
  pub definitions: Vec<ListDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListNamesTable {
  pub names: Vec<Vec<u16>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListDefinition {
  pub info: ListDefinitionInfo,
  pub levels: Vec<ListLevel>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListDefinitionInfo {
  pub list_id: i32,
  pub template_code: u32,
  pub paragraph_style_indexes: [i16; 9],
  pub simple: bool,
  pub unused1: bool,
  pub auto_number: bool,
  pub unused2: bool,
  pub hybrid: bool,
  pub reserved: u8,
  pub html_incompatibilities: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListLevel {
  pub info: ListLevelInfo,
  pub paragraph_properties: GrpPrl,
  pub paragraph_incomplete_prl_tail: Vec<u8>,
  pub number_properties: GrpPrl,
  pub number_incomplete_prl_tail: Vec<u8>,
  pub number_text: Vec<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListLevelInfo {
  pub start_at: i32,
  pub number_format: u8,
  pub justification: u8,
  pub legal: bool,
  pub no_restart: bool,
  pub indent_saved: bool,
  pub converted: bool,
  pub unused1: bool,
  pub tentative: bool,
  pub placeholder_offsets: [u8; 9],
  pub follow_character: u8,
  pub saved_indent: i32,
  pub unused2: i32,
  pub restart_limit: u8,
  pub html_incompatibilities: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListOverrides {
  pub overrides: Vec<ListOverride>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListOverride {
  pub info: ListOverrideInfo,
  pub data: ListOverrideData,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListOverrideInfo {
  pub list_id: i32,
  pub unused1: u32,
  pub unused2: u32,
  pub field_type: u8,
  pub html_incompatibilities: u8,
  pub unused3: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListOverrideData {
  pub first_paragraph_position: u32,
  pub levels: Vec<ListLevelOverride>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListLevelOverride {
  pub start_at: i32,
  pub level_index: u8,
  pub overrides_start: bool,
  pub overrides_formatting: bool,
  pub html_incompatibilities: u8,
  pub unused1: u16,
  pub unused2: u8,
  pub level: Option<ListLevel>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentProperties {
  pub word97: DocumentProperties97,
  pub extension: DocumentPropertiesExtension,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentPropertiesExtension {
  None,
  Word2000(DocumentProperties2000),
  Word2002(DocumentProperties2002),
  Compatibility600 {
    word2002: DocumentProperties2002,
    words: [u16; 3],
  },
  Compatibility610 {
    word2002: DocumentProperties2002,
    words: [u16; 8],
  },
  Word2003(DocumentProperties2003),
  Word2003WithTrailingByte {
    word2003: DocumentProperties2003,
    trailing: u8,
  },
  Word2007(DocumentProperties2007),
  Word2010(DocumentProperties2010),
  Word2013(DocumentProperties2013),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentProperties2000 {
  pub last_bullet_level: u8,
  pub last_numbering_level: u8,
  pub click_and_type_style: u16,
  pub flags: DocumentProperties2000Flags,
  pub compatibility_options: CompatibilityOptions,
  pub pre_word10_features: PreWord10Features,
  pub flags2: DocumentProperties2000Flags2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentProperties2000Flags {
  pub language_detection_all_done: bool,
  pub envelope_visible: bool,
  pub maybe_tentative_list: bool,
  pub maybe_fit_text: bool,
  pub format_consistency_all_done: bool,
  pub rely_on_css: bool,
  pub rely_on_vml: bool,
  pub allow_png: bool,
  pub target_screen_size: WebTargetScreenSize,
  pub organize_in_folder: bool,
  pub use_long_file_names: bool,
  pub pixels_per_inch: u16,
  pub web_options_initialized: bool,
  pub maybe_east_asian_layout: bool,
  pub character_line_units: bool,
  pub unused1: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WebTargetScreenSize {
  Size544x376,
  Size640x480,
  Size720x512,
  Size800x600,
  Size1024x768,
  Size1152x882,
  Size1152x900,
  Size1280x1024,
  Size1600x1200,
  Size1800x1440,
  Size1920x1200,
  Compatibility11,
  Compatibility12,
  Compatibility13,
  Compatibility14,
  Compatibility15,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreWord10Features {
  pub word95: bool,
  pub word97: bool,
  pub east_asian_word95: bool,
  pub word2003: bool,
  pub unused: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentProperties2000Flags2 {
  pub suppress_page_boundaries: bool,
  pub unused2_to_4: u8,
  pub bullet_proofed: bool,
  pub save_uim: bool,
  pub filter_privacy: bool,
  pub seen_repairs: bool,
  pub has_xml: bool,
  pub unused5: bool,
  pub validate_xml: bool,
  pub save_invalid_xml: bool,
  pub show_xml_errors: bool,
  pub always_merge_empty_namespace: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentProperties2002 {
  pub word2000: DocumentProperties2000,
  pub unused: u32,
  pub flags: DocumentProperties2002Flags,
  pub default_table_style: u16,
  pub feature_compatibility: FeatureCompatibility,
  pub style_filter: u16,
  pub booklet_pages: u16,
  pub text_code_page: u32,
  pub minimum_revision_positions: RevisionMinimumPositions,
  pub root_revision_save_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentProperties2002Flags {
  pub do_not_embed_system_font: bool,
  pub word_compatibility: bool,
  pub live_recover: bool,
  pub embed_factoids: bool,
  pub factoid_xml: bool,
  pub factoid_all_done: bool,
  pub folio_print: bool,
  pub reverse_folio: bool,
  pub text_line_ending: TextLineEnding,
  pub hide_format_consistency: bool,
  pub show_markup: bool,
  pub show_comments: bool,
  pub show_insertions_deletions: bool,
  pub show_property_changes: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextLineEnding {
  CrLf,
  Cr,
  Lf,
  LfCr,
  UnicodeSeparator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureCompatibility {
  pub internet_explorer4: bool,
  pub internet_explorer5: bool,
  pub word95: bool,
  pub word97: bool,
  pub word_html: bool,
  pub word_rtf: bool,
  pub east_asian_word95: bool,
  pub plain_text_email: bool,
  pub internet_explorer6: bool,
  pub word_xml: bool,
  pub rtf_email: bool,
  pub no_word2007_features: bool,
  pub plain_text: bool,
  pub unused: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevisionMinimumPositions {
  pub main: u32,
  pub footnote: u32,
  pub header: u32,
  pub comment: u32,
  pub endnote: u32,
  pub textbox: u32,
  pub header_textbox: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentProperties2003 {
  pub word2002: DocumentProperties2002,
  pub flags: DocumentProperties2003Flags,
  pub protection: DocumentProtectionSettings,
  pub page_lock_width: u32,
  pub page_lock_height: u32,
  pub locked_font_percentage: u32,
  pub state_toolbars: DocumentStateToolbars,
  pub list_override_cleanup_limit: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentProperties2003Flags {
  pub treat_comment_lock_as_read_only: bool,
  pub style_lock: bool,
  pub auto_format_override: bool,
  pub remove_wordml: bool,
  pub apply_custom_xml_transform: bool,
  pub style_lock_enforced: bool,
  pub compatibility_comment_lock: bool,
  pub ignore_mixed_content: bool,
  pub show_placeholder_text: bool,
  pub unused: bool,
  pub word97_document: bool,
  pub lock_theme: bool,
  pub lock_quick_format_style_set: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentProtectionSettings {
  pub reading_mode_ink_lockdown: bool,
  pub show_ink_annotations: bool,
  pub remove_annotation_date_time: bool,
  pub enforce: bool,
  pub mode: DocumentProtectionMode,
  pub display_background_shapes: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DocumentProtectionMode {
  TrackedChanges,
  CommentsAndRangePermissions,
  Forms,
  RangePermissions,
  None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentStateToolbars {
  pub reviewing: bool,
  pub web: bool,
  pub mail_merge: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentProperties2007 {
  pub word2003: DocumentProperties2003,
  pub reserved: u32,
  pub flags: DocumentProperties2007Flags,
  pub math: DocumentMathProperties,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentProperties2007Flags {
  pub track_formatting: bool,
  pub track_moves: bool,
  pub style_sort_method: StyleSortMethod,
  pub reading_mode_actual_pages: bool,
  pub auto_compress_pictures: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StyleSortMethod {
  Name,
  ApplicationDefault,
  Font,
  BasedOn,
  StyleType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentMathProperties {
  pub binary_operator_break: MathBinaryOperatorBreak,
  pub binary_subtraction_break: MathBinarySubtractionBreak,
  pub justification: MathJustification,
  pub reserved: bool,
  pub small_fraction: bool,
  pub integral_limits_above_below: bool,
  pub nary_limits_above_below: bool,
  pub wrapped_line_align_left: bool,
  pub use_display_defaults: bool,
  pub font_index: u16,
  pub left_margin: i32,
  pub right_margin: i32,
  pub fixed_constants: MathFixedConstants,
  pub wrapped_line_indent: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MathBinaryOperatorBreak {
  Before,
  After,
  Repeat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MathBinarySubtractionBreak {
  MinusMinus,
  PlusMinus,
  MinusPlus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MathJustification {
  ProducerCompatibilityZero,
  CenteredAsGroup,
  Center,
  Left,
  Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MathFixedConstants {
  Standard120,
  ProducerCompatibilityZero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentProperties2010 {
  pub word2007: DocumentProperties2007,
  pub paragraph_identifier_context: ParagraphIdentifierContext,
  pub reserved: u32,
  pub discard_image_editing_data: bool,
  pub image_resolution_dpi: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParagraphIdentifierContext {
  Standard(u32),
  ProducerCompatibilityZero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentProperties2013 {
  pub word2010: DocumentProperties2010,
  pub chart_tracking_reference_based: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentProperties97 {
  pub base: DocumentPropertiesBase,
  pub compatibility_options_80: CompatibilityOptions80,
  pub document_classification: DocumentClassification,
  pub typography: DocumentTypography,
  pub drawing_grid: DocumentDrawingGrid,
  pub display_flags: DocumentDisplayFlags,
  pub version_flags: DocumentVersionFlags,
  pub auto_summary: AutoSummaryInfo,
  pub characters_with_spaces: DocumentCharacterCountPair,
  pub document_events: DocumentEvents,
  pub virus_info: VirusSessionInfo,
  pub undefined_space: DocumentProperties97Space,
  pub maximum_list_cache_position: i32,
  pub last_list_indexes: LastListIndexes,
  pub double_byte_characters: DocumentCharacterCountPair,
  pub reserved3a: u32,
  pub footnote_number_format: NumberingFormat,
  pub endnote_number_format: NumberingFormat,
  pub pagination_zoom_font_size: u16,
  pub pagination_screen_height: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DocumentClassification {
  NotSpecified,
  Letter,
  Email,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocumentProperties97Space([u8; 30]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LastListIndexes {
  pub bullet: u16,
  pub numbering: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocumentCharacterCountPair {
  pub main: i32,
  pub with_subdocuments: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeprecatedNumberingFieldCacheMetadata {
  pub location: Option<FibFcLcb>,
  pub maximum_valid_position: i32,
  pub invalid: bool,
}

impl DeprecatedNumberingFieldCacheMetadata {
  pub fn is_present(self) -> bool {
    self.location.is_some_and(|value| value.lcb != 0)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentTypography {
  pub kern_punctuation: bool,
  pub justification: TypographyJustification,
  pub kinsoku_level: KinsokuLevel,
  pub print_two_on_one: bool,
  pub unused: bool,
  pub custom_kinsoku_language: CustomKinsokuLanguage,
  pub japanese_use_level2: bool,
  pub following_punctuation_count: u16,
  pub leading_punctuation_count: u16,
  pub following_punctuation_slots: [u16; 101],
  pub leading_punctuation_slots: [u16; 51],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocumentDrawingGrid {
  pub horizontal_origin: u16,
  pub vertical_origin: u16,
  pub horizontal_spacing: u16,
  pub vertical_spacing: u16,
  pub vertical_display_frequency: GridDisplayFrequency,
  pub unused: bool,
  pub horizontal_display_frequency: GridDisplayFrequency,
  pub follow_margins: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GridDisplayFrequency {
  DisabledCompatibility,
  Every(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CompatibilityOptions60 {
  pub no_tab_for_hanging_indent: bool,
  pub no_space_for_raised_or_lowered_text: bool,
  pub suppress_space_before_after_page_break: bool,
  pub wrap_trailing_spaces: bool,
  pub map_print_text_color: bool,
  pub no_column_balance: bool,
  pub convert_mail_merge_escapes: bool,
  pub suppress_top_spacing: bool,
  pub original_word_table_rules: bool,
  pub unused: bool,
  pub show_breaks_in_frames: bool,
  pub swap_borders_on_facing_pages: bool,
  pub leave_backslash_alone: bool,
  pub expand_shift_return: bool,
  pub do_not_underline_trailing_space: bool,
  pub do_not_balance_single_double_byte_width: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CompatibilityOptions80 {
  pub word6: CompatibilityOptions60,
  pub suppress_top_spacing_mac5: bool,
  pub truncate_expanded_spacing: bool,
  pub print_body_before_header: bool,
  pub no_external_leading: bool,
  pub do_not_make_space_for_underline: bool,
  pub mac_word_small_caps: bool,
  pub two_point_external_leading_only: bool,
  pub truncate_font_height: bool,
  pub substitute_font_by_size: bool,
  pub line_wrap_like_word6: bool,
  pub word6_border_rules: bool,
  pub exact_line_height_on_top: bool,
  pub extra_after: bool,
  pub wordperfect_space_width: bool,
  pub wordperfect_justification: bool,
  pub use_printer_metrics: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompatibilityOptions {
  pub word8: CompatibilityOptions80,
  pub shape_layout_like_word8: bool,
  pub footnote_layout_like_word8: bool,
  pub do_not_use_html_paragraph_auto_spacing: bool,
  pub do_not_adjust_line_height_in_table: bool,
  pub forget_last_tab_alignment: bool,
  pub use_autospace_for_full_width_alpha: bool,
  pub align_tables_row_by_row: bool,
  pub layout_raw_table_width: bool,
  pub layout_table_rows_apart: bool,
  pub use_word97_line_breaking_rules: bool,
  pub do_not_break_wrapped_tables: bool,
  pub do_not_snap_to_grid_in_cell: bool,
  pub do_not_allow_field_end_select: bool,
  pub apply_breaking_rules: bool,
  pub do_not_wrap_text_with_punctuation: bool,
  pub do_not_use_asian_break_rules: bool,
  pub use_word2002_table_style_rules: bool,
  pub grow_autofit: bool,
  pub use_normal_style_for_list: bool,
  pub do_not_use_indent_as_numbering_tab_stop: bool,
  pub far_east_line_break11: bool,
  pub allow_same_style_spacing_in_table: bool,
  pub word11_indent_rules: bool,
  pub do_not_autofit_constrained_tables: bool,
  pub autofit_like_word11: bool,
  pub underline_tab_in_numbered_list: bool,
  pub hangul_width_like_word11: bool,
  pub split_page_break_and_paragraph_mark: bool,
  pub do_not_vertically_align_cell_with_shape: bool,
  pub do_not_break_constrained_forced_tables: bool,
  pub do_not_vertically_align_in_textbox: bool,
  pub word11_kerning_pairs: bool,
  pub cached_column_balance: bool,
  pub empty1: u32,
  pub empty: [u32; 5],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocumentFormatFlags {
  pub facing_pages: bool,
  pub unused1: bool,
  pub mail_merge_main_document: bool,
  pub unused2: u8,
  pub footnote_placement: FootnotePlacement,
  pub unused3: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FootnotePlacement {
  EndOfSection,
  BottomOfPage,
  BeneathText,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NoteNumbering {
  pub restart: NoteNumberingRestart,
  pub starting_number: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NoteNumberingRestart {
  Continuous,
  EachSection,
  EachPage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocumentStateFlags {
  pub unused5_to_10: u8,
  pub spelling_all_done: bool,
  pub spelling_all_clean: bool,
  pub hide_spelling_errors: bool,
  pub hide_grammar_errors: bool,
  pub labels_document: bool,
  pub hyphenate_capitals: bool,
  pub auto_hyphenate: bool,
  pub form_has_no_fields: bool,
  pub link_styles: bool,
  pub revision_marking: bool,
  pub unused11: bool,
  pub exact_statistics: bool,
  pub unused_page_hidden: bool,
  pub unused_page_results: bool,
  pub lock_annotations: bool,
  pub mirror_margins: bool,
  pub word97_compatibility: bool,
  pub unused12: bool,
  pub unused13: bool,
  pub form_protection: bool,
  pub display_form_field_selection: bool,
  pub revision_mark_view: bool,
  pub revision_mark_print: bool,
  pub lock_vba_project: bool,
  pub lock_revisions: bool,
  pub embed_fonts: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EndnoteOptions {
  pub placement: EndnotePlacement,
  pub unused14: u8,
  pub unused15: u8,
  pub print_form_data: bool,
  pub save_form_data: bool,
  pub shade_form_data: bool,
  pub shade_merge_fields: bool,
  pub include_subdocuments_in_statistics: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EndnotePlacement {
  EndOfSection,
  EndOfDocument,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SavedView {
  pub kind: SavedViewKind,
  pub zoom_percentage: u16,
  pub zoom_kind: SavedZoomKind,
  pub unused: bool,
  pub gutter_at_top: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SavedViewKind {
  None,
  Print,
  Outline,
  MasterPages,
  Normal,
  Web,
  Compatibility6,
  Compatibility7,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SavedZoomKind {
  None,
  FullPage,
  BestFit,
  TextFit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocumentDisplayFlags {
  pub unused1: bool,
  pub outline_level: SavedOutlineLevel,
  pub grammar_all_done: bool,
  pub grammar_all_clean: bool,
  pub subset_fonts: bool,
  pub unused2: bool,
  pub html_document: bool,
  pub list_cache_invalid: bool,
  pub snap_border: bool,
  pub include_header: bool,
  pub include_footer: bool,
  pub unused3: bool,
  pub unused4: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SavedOutlineLevel {
  Heading1,
  Heading2,
  Heading3,
  Heading4,
  Heading5,
  Heading6,
  Heading7,
  Heading8,
  Heading9,
  All9,
  All15,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocumentVersionFlags {
  pub unused: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocumentEvents {
  pub new: bool,
  pub open: bool,
  pub close: bool,
  pub sync: bool,
  pub xml_after_insert: bool,
  pub xml_before_delete: bool,
  pub building_block_after_insert: bool,
  pub building_block_before_delete: bool,
  pub building_block_on_exit: bool,
  pub building_block_on_enter: bool,
  pub store_update: bool,
  pub building_block_content_update: bool,
  pub lego_after_insert: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirusSessionInfo {
  pub prompted: bool,
  pub load_safe: bool,
  pub session_key: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TypographyJustification {
  DoNotCompress,
  CompressPunctuation,
  CompressPunctuationAndJapaneseKana,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum KinsokuLevel {
  LanguageDefault,
  JapaneseLevel2,
  Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CustomKinsokuLanguage {
  None,
  Japanese,
  ChineseSimplified,
  Korean,
  ChineseTraditional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentPropertiesBase {
  pub format_flags: DocumentFormatFlags,
  pub unused4: u8,
  pub footnote_numbering: NoteNumbering,
  pub document_flags: DocumentStateFlags,
  pub compatibility_options_60: CompatibilityOptions60,
  pub default_tab_width: i16,
  pub web_code_page: CodePage,
  pub hyphenation_zone: u16,
  pub consecutive_hyphen_limit: u16,
  pub reserved2: u16,
  pub created: Dttm,
  pub revised: Dttm,
  pub last_printed: Dttm,
  pub revision_count: i16,
  pub editing_time: i32,
  pub statistics: DocumentStatistics,
  pub endnote_numbering: NoteNumbering,
  pub endnote_options: EndnoteOptions,
  pub protection_password_hash: DocumentProtectionPasswordHash,
  pub saved_view: SavedView,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocumentStatistics {
  pub main: DocumentStoryStatistics,
  pub with_subdocuments: DocumentStoryStatistics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocumentStoryStatistics {
  pub words: i32,
  pub characters: i32,
  pub pages: i16,
  pub paragraphs: i32,
  pub lines: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocumentProtectionPasswordHash(pub i32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontTable {
  pub fonts: Vec<FontFamilyName>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFamilyName {
  pub family: FontFamilyIdentifier,
  pub weight: i16,
  pub character_set: u8,
  pub alternate_name_index: u8,
  pub panose: Panose,
  pub signature: FontSignature,
  pub name_units: Vec<u16>,
  pub trailing_name_nulls: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontFamilyIdentifier {
  pub pitch: u8,
  pub true_type: bool,
  pub unused1: bool,
  pub family: u8,
  pub unused2: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panose {
  pub family_type: u8,
  pub serif_style: u8,
  pub weight: u8,
  pub proportion: u8,
  pub contrast: u8,
  pub stroke_variation: u8,
  pub arm_style: u8,
  pub letterform: u8,
  pub midline: u8,
  pub height: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontSignature {
  pub unicode_subsets: [u32; 4],
  pub code_pages: [u32; 2],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssociatedStrings {
  pub unused0: Vec<u16>,
  pub template_path: Vec<u16>,
  pub title: Vec<u16>,
  pub subject: Vec<u16>,
  pub keywords: Vec<u16>,
  pub unused5: Vec<u16>,
  pub author: Vec<u16>,
  pub last_revised_by: Vec<u16>,
  pub mail_merge_data_source: Vec<u16>,
  pub mail_merge_header: Vec<u16>,
  pub unused10: Vec<u16>,
  pub unused11: Vec<u16>,
  pub unused12: Vec<u16>,
  pub unused13: Vec<u16>,
  pub unused14: Vec<u16>,
  pub unused15: Vec<u16>,
  pub unused16: Vec<u16>,
  pub write_reservation_password: Vec<u16>,
  pub trailing_zero_words: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserVariables {
  pub variables: Vec<UserVariable>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserVariable {
  pub name: Vec<u16>,
  pub ignored_name_metadata: u32,
  pub value: Vec<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum UserVariableKind {
  Ordinary,
  LegacyVbaSignature,
  AgileVbaSignature,
  VbaSignatureV3,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedFontTable {
  pub producer_offset: EmbeddedFontTableOffset,
  pub fonts: Vec<EmbeddedFontReference>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbeddedFontReference {
  pub word_document_offset: u32,
  pub font_index: i16,
  pub bold: bool,
  pub italic: bool,
  pub ignored_flags: u16,
  pub subset: EmbeddedFontSubset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EmbeddedFontTableOffset {
  Standard,
  Word97Compatibility,
  Compatibility(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EmbeddedFontSubset {
  EntireFont,
  UsageOrder(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailMergeState {
  pub status: MailMergeStatus,
  pub header_source_index: u8,
  pub fetch_source_index: u8,
  pub current_record: Option<u32>,
  pub sources: [MailMergeSource; 2],
  pub filter: MailMergeFilter,
  pub sql_query: Option<Vec<u16>>,
  pub strings: Option<MailMergeStrings>,
  pub document_type: Option<MailMergeDocumentTypeInfo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MailMergeStatus {
  pub main_document_selected: bool,
  pub data_source_selected: bool,
  pub header_source_selected: bool,
  pub document_type: MailMergeDocumentType,
  pub ignored1: bool,
  pub automatic: bool,
  pub suppress_blank_lines: bool,
  pub record_selection: bool,
  pub destination: MailMergeDestination,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MailMergeDocumentType {
  None,
  Letters,
  Labels,
  Envelopes,
  Catalog,
  Email,
  Fax,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MailMergeDestination {
  None,
  Printer,
  Email,
  Fax,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MailMergeSource {
  pub kind: MailMergeSourceKind,
  pub link_to_filename: bool,
  pub link_to_connection: bool,
  pub no_prompt_query_tool: bool,
  pub query: bool,
  pub ignored_flags: u8,
  pub field_separator: MailMergeSeparator,
  pub record_separator: MailMergeSeparator,
  pub file: MailMergeFileReference,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MailMergeSourceKind {
  None,
  DataFile,
  Access,
  Excel,
  MicrosoftQuery,
  Odbc,
  OfficeDataSourceObject,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailMergeSeparator {
  Token(MailMergeToken),
  Ignored(i16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailMergeToken {
  None,
  Enter,
  Tab,
  Character(u16),
  FieldEnd,
  TableCell,
  TableRow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailMergeFileReference {
  Identifier(u16),
  NilCompatibility,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MailMergeFilter {
  pub show_data: bool,
  pub error_handling: MailMergeErrorHandling,
  pub main_document_setup: bool,
  pub mail_as_text: bool,
  pub ignored1: bool,
  pub default_sql: bool,
  pub mail_as_html: bool,
  pub ignored2: u8,
  pub string_table_handle: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MailMergeErrorHandling {
  SimulateAndReport,
  CompleteAndPause,
  CompleteAndReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailMergeStrings {
  pub connection: Vec<u16>,
  pub header_connection: Vec<u16>,
  pub subject: Vec<u16>,
  pub recipient_column: Vec<u16>,
  pub ignored: Option<Vec<u16>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MailMergeDocumentTypeInfo {
  pub document_type: MailMergeDocumentType,
  pub ignored: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipientFilter {
  pub items: Vec<RecipientFilterItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipientFilterItem {
  pub column: u8,
  pub operator: FilterComparison,
  pub condition: FilterCondition,
  pub value: Vec<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterComparison {
  Equal,
  NotEqual,
  LessThan,
  GreaterThan,
  LessThanOrEqual,
  GreaterThanOrEqual,
  Empty,
  NotEmpty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterCondition {
  And,
  Or,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipientSort {
  pub columns: Vec<SortColumn>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortColumn {
  pub column: u8,
  pub direction: SortDirection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
  Ascending,
  Descending,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipientInfo {
  pub recipients: Vec<Recipient>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipient {
  pub items: Vec<RecipientData>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecipientData {
  Included(bool),
  UniqueColumn(u32),
  Hash(u32),
  UniqueValue(Vec<u16>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldMapInfo {
  pub fields: Vec<FieldMap>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldMap {
  pub items: Vec<FieldMapData>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldMapData {
  Mapped,
  DataSourceColumnName(Vec<u16>),
  StandardFieldName(Vec<u16>),
  ColumnIndex(Option<u32>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeDataSource {
  pub properties: Vec<OfficeDataSourceProperty>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeDataSourceProperty {
  ConnectionString(Vec<u16>),
  DataSet(Vec<u16>),
  FileName(Vec<u16>),
  ConnectionType(u8),
  ColumnDelimiter(u16),
  FirstRowIsHeader(bool),
  Filter(RecipientFilter),
  Sort(RecipientSort),
  Recipients(RecipientInfo),
  FieldMap(FieldMapInfo),
  WizardStep(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubdocumentTable {
  pub positions: Vec<u32>,
  pub subdocuments: Vec<SubdocumentReference>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubdocumentReference {
  pub ignored_flag3: bool,
  pub ignored_flag8: bool,
  pub file_identifier: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalFileNameTable {
  pub files: Vec<ExternalFileName>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalFileName {
  pub path: Vec<u16>,
  pub file_type: ExternalFileType,
  pub identifier: u16,
  pub relative_path: ExternalRelativePath,
  pub file_systems: ExternalFileSystems,
  pub ignored: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExternalFileType {
  MailMergeDataSource,
  Subdocument,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalRelativePath {
  None,
  Offset(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalFileSystems {
  pub fat: bool,
  pub ignored1: bool,
  pub ignored2: bool,
  pub ntfs: bool,
  pub non_file_system: bool,
  pub ignored3: u8,
  pub ignored4: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlSchemaReferences {
  pub schemas: Vec<XmlSchemaReference>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlSchemaReference {
  pub uri: Vec<u16>,
  pub manifest_location: Vec<u16>,
  pub elements: XmlSchemaStringTable,
  pub attributes: XmlSchemaStringTable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XmlSchemaStringTable {
  Ansi(Vec<Vec<u8>>),
  Utf16(Vec<Vec<u16>>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlTransformPath {
  pub path: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangeProtection {
  pub permissions: Vec<RangePermission>,
  pub starts: BookmarkStartTable,
  pub ends: BookmarkEndTable,
  pub users: ProtectedUsers,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangeProtectionBytes {
  pub permissions: Vec<u8>,
  pub starts: Vec<u8>,
  pub ends: Vec<u8>,
  pub users: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangePermission {
  pub editors: PermittedEditors,
  pub ignored_index: u16,
  pub ignored_use: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermittedEditors {
  UserIndex(u16),
  Editors,
  Owners,
  Everyone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectedUsers {
  pub users: Vec<ProtectedUser>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectedUser {
  pub name: Vec<u16>,
  pub role: ProtectedUserRole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectedUserRole {
  None,
  Owner,
  Editor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredTagBookmarks {
  pub tags: Vec<StructuredTagInfo>,
  pub starts: BookmarkStartTable,
  pub ends: BookmarkEndTable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredTagBookmarkBytes {
  pub tags: Vec<u8>,
  pub starts: Vec<u8>,
  pub ends: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredTagInfo {
  pub id: u32,
  pub name: TagQualifiedName,
  pub tag_type: StructuredTagType,
  pub attributes: Vec<StructuredTagAttribute>,
  pub placeholder: Vec<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TagQualifiedName {
  pub schema_index: u32,
  pub name_index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructuredTagType {
  Characters,
  Paragraphs,
  TableCells,
  TableRows,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredTagAttribute {
  pub name: TagQualifiedName,
  pub value: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatConsistencyBookmarks {
  pub records: Vec<FormatConsistencyBookmark>,
  pub starts: BookmarkStartTable,
  pub ends: BookmarkEndTable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookmarkSetBytes {
  pub metadata: Vec<u8>,
  pub starts: Vec<u8>,
  pub ends: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatConsistencyBookmark {
  pub padding1: u16,
  pub squiggle: bool,
  pub ignored: bool,
  pub squiggle_changed: bool,
  pub kind: FormatConsistencyKind,
  pub ignored_data: u32,
  pub properties: FormatConsistencyProperties,
  pub id: u32,
  pub padding2: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatConsistencyKind {
  CharacterFormatting,
  MatchingCharacterStyle,
  ParagraphFormatting,
  MatchingParagraphStyle,
  ListLevelFormatting,
  MatchingListStyle,
  MatchingTableStyle,
  RevisedCharacters,
  RevisedParagraphs,
  RevisedTables,
  RevisedSection,
  DuplicateInlineImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatConsistencyProperties {
  pub character: bool,
  pub table: bool,
  pub line_separation: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepairBookmarks {
  pub descriptions: Vec<Vec<u16>>,
  pub starts: BookmarkStartTable,
  pub ends: BookmarkEndTable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptionDefinitions {
  pub captions: Vec<CaptionDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptionDefinition {
  pub label: Vec<u16>,
  pub properties: CaptionProperties,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptionProperties {
  pub location: CaptionLocation,
  pub include_chapter_number: bool,
  pub heading: CaptionHeading,
  pub ignored: u8,
  pub no_label: bool,
  pub number_format: NumberingFormat,
  pub separator: CaptionSeparator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CaptionLocation {
  Below,
  Above,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionHeading {
  Heading(u8),
  Ignored(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionSeparator {
  Hyphen,
  Period,
  Colon,
  EnDash,
  EmDash,
  Ignored(u16),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoCaptionDefinitions {
  pub entries: Vec<AutoCaptionDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoCaptionDefinition {
  pub program_id: Vec<u16>,
  pub caption_index: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevisionAuthors {
  Standard { names: Vec<Vec<u16>> },
  CompatibilityZeroPlaceholder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpellingStateTable {
  pub positions: Vec<u32>,
  pub states: Vec<SpellingState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpellingState {
  pub kind: SpellingStateKind,
  pub error: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SpellingStateKind {
  MaybeDirty,
  Dirty,
  Edit,
  Foreign,
  Clean,
  RepeatWord,
  UnknownWord,
  Compatibility13,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarStateTable {
  pub positions: Vec<u32>,
  pub states: Vec<GrammarState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrammarState {
  pub kind: GrammarStateKind,
  pub error: bool,
  pub extend: bool,
  pub typo: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GrammarStateKind {
  MaybeDirty,
  Dirty,
  Edit,
  Foreign,
  Clean,
  ErrorMin,
  RepeatWord,
  UnknownWord,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageDetectionStateTable {
  pub positions: Vec<u32>,
  pub states: Vec<LanguageDetectionState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanguageDetectionState {
  pub kind: LanguageDetectionStateKind,
  pub error: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LanguageDetectionStateKind {
  MaybeDirty,
  Dirty,
  Edit,
  Foreign,
  Clean,
  NoLanguageDetection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListStyleTemplates {
  pub lists: Vec<Option<[ListLevelTemplateCode; 9]>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListLevelTemplateCode {
  BuiltIn { format: BuiltInListFormat, lid: u16 },
  UserDefined { random: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BuiltInListFormat {
  Format(u8),
  None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameAndListRecords {
  pub records: Vec<FrameAndListRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameAndListRecord {
  FrameSet,
  Frame(FrameRecord),
  ChildMarker { push: bool, unused: u32 },
  FrameName(Xstz),
  FrameFilePath(Xstz),
  FrameBorder(FrameBorder),
  ListStyles(Vec<ListStyleReference>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRecord {
  pub divider_units: FrameDividerUnits,
  pub divider_value: u32,
  pub child_layout: FrameChildLayout,
  pub kind: FrameRecordKind,
  pub horizontal_margin: i32,
  pub vertical_margin: i32,
  pub scroll: FrameScroll,
  pub linked: bool,
  pub no_resize: bool,
  pub unused_flags: u32,
  pub unused: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameDividerUnits {
  None,
  Pixels,
  Percent,
  Relative,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameChildLayout {
  None,
  Rows,
  Columns,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameRecordKind {
  Nil,
  FrameSet,
  Frame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameScroll {
  Auto,
  Yes,
  No,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameBorder {
  pub width_twips: i32,
  pub color: ColorRef,
  pub no_border: bool,
  pub three_dimensional: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListStyleReference {
  pub list_index: u16,
  pub style_index: u16,
  pub style_definition: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarOptionSets {
  pub options: Vec<GrammarOptionSet>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrammarOptionSet {
  pub option_set: u16,
  pub language_id: u16,
  pub checker_version: u32,
  pub company_id: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyGrammarOptionSets {
  pub options: Vec<LegacyGrammarOptionSet>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyGrammarOptionSet {
  pub option_set: u16,
  pub language_id: u16,
  pub checker_version: u16,
  pub company_id: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoSummaryRangeTable {
  pub positions: Vec<u32>,
  pub priorities: Vec<AutoSummaryPriority>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoSummaryPriority {
  pub level: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoSummaryInfo {
  pub valid: bool,
  pub view_active: bool,
  pub view_by: AutoSummaryView,
  pub update_properties: bool,
  pub desired_size: AutoSummaryDesiredSize,
  pub highest_level: i32,
  pub current_level: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AutoSummaryView {
  Highlight,
  HideNonSummaryText,
  InsertAtDocumentStart,
  CreateDocument,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AutoSummaryDesiredSize {
  Percentage(u16),
  TenSentences,
  TwentySentences,
  HundredWords,
  FiveHundredWords,
  TenPercent,
  TwentyFivePercent,
  FiftyPercent,
  SeventyFivePercent,
  Compatibility(u16),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartTagRecognizerStateTable {
  pub positions: Vec<u32>,
  pub states: Vec<SmartTagRecognizerState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmartTagRecognizerState {
  pub kind: SmartTagRecognizerStateKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SmartTagRecognizerStateKind {
  Pending,
  MaybeDirty,
  Dirty,
  Edit,
  Clean,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParagraphGroupProperties {
  pub entries: Vec<ParagraphGroupProperty>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParagraphGroupProperty {
  pub id: u32,
  pub parent_id: u32,
  pub table_depth: u32,
  pub options: ParagraphGroupOptions,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParagraphGroupOptions {
  pub left_margin: Option<i32>,
  pub right_margin: Option<i32>,
  pub top_margin: Option<i32>,
  pub bottom_margin: Option<i32>,
  pub left_border: Option<Brc>,
  pub right_border: Option<Brc>,
  pub top_border: Option<Brc>,
  pub bottom_border: Option<Brc>,
  pub html_block_type: Option<HtmlBlockType>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HtmlBlockType {
  Division,
  BlockQuote,
  Body,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveHistory {
  pub entries: Vec<SaveHistoryEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveHistoryEntry {
  pub author: Vec<u16>,
  pub path: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartTagBookmarks {
  pub infos: Vec<SmartTagBookmarkInfo>,
  pub starts: SmartTagBookmarkStartTable,
  pub ends: SmartTagBookmarkEndTable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmartTagBookmarkInfo {
  pub id: u32,
  pub sub_entity: bool,
  pub unused: u16,
  pub source: SmartTagSource,
  pub ignored_property_bag_pointer: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SmartTagSource {
  Unknown,
  Grammar,
  ScanDll,
  VisualBasic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartTagBookmarkStartTable {
  pub positions: Vec<u32>,
  pub bookmarks: Vec<SmartTagBookmarkStart>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmartTagBookmarkStart {
  pub bookmark: BookmarkStart,
  pub depth: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartTagBookmarkEndTable {
  pub positions: Vec<u32>,
  pub bookmarks: Vec<SmartTagBookmarkEnd>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmartTagBookmarkEnd {
  pub start_index: u16,
  pub depth: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarCheckerCookieTable {
  pub positions: Vec<u32>,
  pub cookies: Vec<GrammarCheckerCookie>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyGrammarCheckerCookieTable {
  pub positions: Vec<u32>,
  pub cookies: Vec<LegacyGrammarCheckerCookie>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyGrammarCheckerCookie {
  pub language_id: u16,
  pub character_count: i16,
  pub sentence_offset: i16,
  pub padding1: u16,
  pub error_type: GrammarCookieErrorType,
  pub spare: u16,
  pub error: bool,
  pub padding2: u16,
  pub data_offset: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarCookieStore {
  pub cookies: Vec<GrammarCookieData>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarCookieData {
  pub provider_data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrammarCheckerCookie {
  pub character_count: i16,
  pub sentence_offset: i16,
  pub data_offset: u32,
  pub error_type: GrammarCookieErrorType,
  pub error: bool,
  pub language_sub: u8,
  pub language_primary: u8,
  pub header: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GrammarCookieErrorType {
  Default,
  Typo,
  Homonym,
  Consistency,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartTagData {
  pub factoid_types: Vec<SmartTagFactoidType>,
  pub reserved_factoid_count: u32,
  pub strings: Vec<PropertyBagString>,
  pub property_bags: Vec<SmartTagPropertyBag>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartTagFactoidType {
  pub id: SmartTagFactoidTypeId,
  pub uri: PropertyBagString,
  pub tag: PropertyBagString,
  pub download_url: PropertyBagString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SmartTagFactoidTypeId {
  Standard(u16),
  MalformedCve20163133,
}

impl SmartTagFactoidTypeId {
  const MALFORMED_CVE_2016_3133_VALUE: u32 = 0x0004_0004;

  fn from_u32(value: u32) -> Result<Self> {
    match u16::try_from(value) {
      Ok(value) => Ok(Self::Standard(value)),
      Err(_) if value == Self::MALFORMED_CVE_2016_3133_VALUE => Ok(Self::MalformedCve20163133),
      Err(_) => Err(Error::invalid(
        0,
        format!("FactoidType id {value:#x} exceeds u16"),
      )),
    }
  }

  fn to_u32(self) -> u32 {
    match self {
      Self::Standard(value) => u32::from(value),
      Self::MalformedCve20163133 => Self::MALFORMED_CVE_2016_3133_VALUE,
    }
  }

  fn property_bag_id(self) -> u16 {
    match self {
      Self::Standard(value) => value,
      Self::MalformedCve20163133 => 4,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropertyBagString {
  Ansi(Vec<u8>),
  Unicode(Vec<u16>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartTagPropertyBag {
  pub factoid_type_id: u16,
  pub properties: Vec<SmartTagProperty>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmartTagProperty {
  pub key_index: u32,
  pub value_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCharacterCacheTable {
  pub positions: Vec<u32>,
  pub caches: Vec<TableCharacterCache>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableCharacterCache {
  pub unknown: bool,
  pub unused: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionMessageThreading {
  pub messages: Vec<RevisionThreadMessage>,
  pub styles: Vec<Vec<u16>>,
  pub author_attributes: Vec<RevisionThreadAttribute>,
  pub author_values: Vec<Vec<u16>>,
  pub message_attributes: Vec<RevisionThreadAttribute>,
  pub message_values: Vec<Vec<u16>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionThreadMessage {
  pub identifier: Vec<u16>,
  pub display: MessageDisplayProperties,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageDisplayProperties {
  pub created: Dttm,
  pub reserved: u16,
  pub author_index: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionThreadAttribute {
  pub name: Vec<u16>,
  pub target_index: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionSaveIdTable {
  pub reserved2: u32,
  pub reserved3: u32,
  pub ids: Vec<RevisionSaveId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RevisionSaveId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Dttm {
  pub minute: u8,
  pub hour: u8,
  pub day: u8,
  pub month: u8,
  pub year_offset: u16,
  pub weekday: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionState {
  pub flags: SelectionFlags,
  pub first_character: i32,
  pub character_limit: i32,
  pub unused4: u32,
  pub range: SelectionRange,
  pub anchor_character: i32,
  pub style: SelectionStyle,
  pub unused5: u16,
  pub shrink_anchor_character: i32,
  pub table_left: i16,
  pub table_right: i16,
  pub extension: SelectionStateExtension,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionFlags {
  pub rightward: bool,
  pub unused1: bool,
  pub within_cell: bool,
  pub table_anchor: bool,
  pub table_selection_non_shrink: bool,
  pub unused2: bool,
  pub discontiguous: bool,
  pub prefix: bool,
  pub shape: bool,
  pub frame: bool,
  pub column: bool,
  pub table: bool,
  pub graphics: bool,
  pub block: bool,
  pub unused3: bool,
  pub insertion_point: bool,
  pub forward: u8,
  pub prefix_word2007: bool,
  pub insertion_at_line_end: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionRange {
  Unused(u32),
  Block { first_pixel: i16, limit_pixel: i16 },
  Table { first_cell: i16, limit_cell: i16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SelectionStyle {
  Undefined,
  Character,
  Word,
  Sentence,
  Paragraph,
  Line,
  Column,
  Row,
  AllColumns,
  WholeTable,
  Prefix,
  Compatibility(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionStateExtension {
  None,
  Compatibility([u32; 2]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandCustomizations {
  pub records: Vec<CommandCustomizationRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandCustomizationRecord {
  MacroCommands(Vec<MacroCommandDescriptor>),
  CommandStrings(Vec<CommandString>),
  MacroNames(Vec<MacroName>),
  Toolbar(ToolbarWrapper),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MacroCommandDescriptor {
  pub reserved1: i8,
  pub reserved2: u8,
  pub macro_name_index: u16,
  pub command_string_index: u16,
  pub reserved3: u16,
  pub reserved4: u32,
  pub reserved5: u32,
  pub reserved6: u32,
  pub reserved7: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandString {
  pub value: Vec<u16>,
  pub reference_count: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroName {
  pub index: u16,
  pub value: Xstz,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarWrapper {
  pub reserved2: u16,
  pub reserved3: u8,
  pub reserved4: u16,
  pub reserved5: u16,
  pub toolbar_delta_size: i16,
  pub controls: Vec<ToolbarControl>,
  pub customizations: Vec<ToolbarCustomization>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarControl {
  pub header: ToolbarControlHeader,
  pub command_id: Option<u32>,
  pub data: Option<ToolbarControlData>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarControlHeader {
  pub signature: i8,
  pub version: i8,
  pub flags: u8,
  pub control_type: u8,
  pub control_id: u16,
  pub specific_flags: u32,
  pub priority: u8,
  pub size: Option<(u16, u16)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarControlData {
  pub general: ToolbarControlGeneralInfo,
  pub specific: ToolbarControlSpecific,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarControlGeneralInfo {
  pub flags: u8,
  pub custom_text: Option<Vec<u16>>,
  pub description: Option<Vec<u16>>,
  pub tooltip: Option<Vec<u16>>,
  pub extra: Option<ToolbarControlExtraInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarControlExtraInfo {
  pub help_file: Vec<u16>,
  pub help_context_id: i32,
  pub tag: Vec<u16>,
  pub on_action: Vec<u16>,
  pub parameter: Vec<u16>,
  pub toolbar_control_user: i8,
  pub toolbar_control_modified: i8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolbarControlSpecific {
  Menu {
    toolbar_id: i32,
    name: Option<Vec<u16>>,
  },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarCustomization {
  pub toolbar_id: i32,
  pub reserved: u16,
  pub deltas: Vec<ToolbarDelta>,
  pub custom_toolbar: Option<CustomToolbar>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomToolbar {
  pub name: Vec<u16>,
  pub declared_toolbar_data_size: i32,
  pub toolbar: ToolbarData,
  pub visual_data: [ToolbarVisualData; 5],
  pub customization_index: i32,
  pub reserved: u16,
  pub unused: u16,
  pub controls: Vec<ToolbarControl>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarData {
  pub signature: i8,
  pub version: i8,
  pub declared_control_count: i16,
  pub toolbar_id: i32,
  pub type_restrictions: u32,
  pub default_rows: u16,
  pub flags: u16,
  pub name: Vec<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarVisualData {
  pub dock_state: i8,
  pub visibility: i8,
  pub last_dock_state: i8,
  pub row: i8,
  pub docked: ToolbarRectangle,
  pub floating: ToolbarRectangle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarRectangle {
  pub left: i16,
  pub top: i16,
  pub right: i16,
  pub bottom: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarDelta {
  pub operation: u8,
  pub at_end: bool,
  pub reserved: u8,
  pub control_index: u8,
  pub next_command_id: i32,
  pub command_id: i32,
  pub file_offset: i32,
  pub toolbar_index_flags: u16,
  pub control_byte_count: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldTable {
  /// Top-level fields. Nested fields remain attached to the instruction or
  /// result range that physically contains them.
  pub fields: Vec<Field>,
  /// The final PLC CP terminates the last physical Fld range and does not
  /// identify a field character.
  pub terminal_position: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
  pub begin: FieldBegin,
  pub instruction_fields: Vec<Field>,
  pub separator: Option<FieldSeparator>,
  pub result_fields: Vec<Field>,
  pub end: FieldEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldBegin {
  pub position: u32,
  pub reserved: u8,
  pub field_type: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSeparator {
  pub position: u32,
  pub reserved: u8,
  pub value: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldEnd {
  pub position: u32,
  pub reserved: u8,
  pub flags: FieldEndFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldDescriptor {
  pub character: FieldCharacter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldCharacter {
  Begin { reserved: u8, field_type: u8 },
  Separator { reserved: u8, value: u8 },
  End { reserved: u8, flags: FieldEndFlags },
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FieldEndFlags: u8 {
        const DIFFERENT_DISPLAY = 0x01;
        const ZOMBIE_EMBED = 0x02;
        const RESULTS_DIRTY = 0x04;
        const RESULTS_EDITED = 0x08;
        const LOCKED = 0x10;
        const PRIVATE_RESULT = 0x20;
        const NESTED = 0x40;
        const HAS_SEPARATOR = 0x80;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookmarkNames {
  pub extended_marker: u16,
  pub extra_data_size: u16,
  pub names: Vec<Vec<u16>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookmarkStartTable {
  pub positions: Vec<u32>,
  pub bookmarks: Vec<BookmarkStart>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookmarkStart {
  pub end_index: u16,
  pub column_start: u8,
  pub published: bool,
  pub column_limit: u8,
  pub native: bool,
  pub column: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookmarkEndTable {
  pub positions: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmarks {
  pub names: BookmarkNames,
  pub starts: BookmarkStartTable,
  pub ends: BookmarkEndTable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sed {
  pub file_number: u16,
  pub sepx_offset: i32,
  pub mpr_file_number: u16,
  pub mpr_offset: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sepx {
  pub properties: GrpPrl,
  pub trailing_byte: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleSheet {
  pub info: StyleSheetInfo,
  pub styles: Vec<LengthPrefixedStyle>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleSheetInfo {
  pub header: Stshif,
  pub bidi_font_index: Option<i16>,
  pub latent_styles: Option<StshiLsd>,
  pub standard_character_properties: Option<GrpPrl>,
  pub standard_paragraph_properties: Option<GrpPrl>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StshiLsd {
  pub entry_size: u16,
  pub entries: Vec<LatentStyleData>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatentStyleData {
  pub locked: bool,
  pub semi_hidden: bool,
  pub unhide_when_used: bool,
  pub quick_format: bool,
  pub priority: u16,
  pub reserved: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stshif {
  pub style_count: u16,
  pub std_base_size: u16,
  pub style_names_written: bool,
  pub reserved: u16,
  pub max_builtin_style: u16,
  pub fixed_style_count: u16,
  pub builtin_name_version: u16,
  pub ascii_font_index: i16,
  pub east_asian_font_index: i16,
  pub other_font_index: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LengthPrefixedStyle {
  pub definition: Option<StyleDefinition>,
  pub alignment_padding: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleDefinition {
  pub base: StdfBase,
  pub post_2000: Option<StdfPost2000>,
  pub name: Xstz,
  pub formatting: StyleFormatting,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StyleFormatting {
  Paragraph {
    paragraph: StylePapx,
    character: StyleGrpPrl,
  },
  Character {
    character: StyleGrpPrl,
  },
  RevisionParagraph {
    paragraph: StylePapx,
    character: StyleGrpPrl,
    revision: StyleRevision,
    original_paragraph: StylePapx,
    original_character: StyleGrpPrl,
  },
  RevisionCharacter {
    character: StyleGrpPrl,
    revision: StyleRevision,
    original_character: StyleGrpPrl,
  },
  Table {
    table: StyleGrpPrl,
    paragraph: StylePapx,
    character: StyleGrpPrl,
  },
  Numbering {
    paragraph: StylePapx,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleRevision {
  pub modified: Dttm,
  pub author_index: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleGrpPrl {
  pub properties: GrpPrl,
  pub padding: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StylePapx {
  pub style_index: u16,
  pub properties: GrpPrl,
  pub padding: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StdfBase {
  pub invariant_style_id: u16,
  pub flags: StdfBaseFlags,
  pub style_kind: StyleKind,
  pub base_style_index: u16,
  pub formatting_count: u8,
  pub next_style_index: u16,
  pub byte_count: u16,
  pub general_flags: StyleGeneralFlags,
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct StdfBaseFlags: u8 {
        const SCRATCH = 0x01;
        const INVALID_HEIGHT = 0x02;
        const HAS_UPE = 0x04;
        const MASS_COPY = 0x08;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StyleKind {
  Paragraph,
  Character,
  Table,
  Numbering,
  Compatibility(u8),
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct StyleGeneralFlags: u16 {
        const AUTO_REDEFINE = 0x0001;
        const HIDDEN = 0x0002;
        const WORD97_LIDS_SET = 0x0004;
        const COPY_LANGUAGE = 0x0008;
        const PERSONAL_COMPOSE = 0x0010;
        const PERSONAL_REPLY = 0x0020;
        const PERSONAL = 0x0040;
        const NO_HTML_EXPORT = 0x0080;
        const SEMI_HIDDEN = 0x0100;
        const LOCKED = 0x0200;
        const INTERNAL_USE = 0x0400;
        const UNHIDE_WHEN_USED = 0x0800;
        const QUICK_FORMAT = 0x1000;
        const RESERVED = 0xe000;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StdfPost2000 {
  pub linked_style_index: u16,
  pub has_original_style: bool,
  pub spare: u8,
  pub revision_save_id: u32,
  pub html_font_index: u8,
  pub unused: bool,
  pub priority: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_xstz_encoding")]
pub struct Xstz {
  #[sdk(count_prefix = "u16")]
  pub characters: Vec<u16>,
  pub terminator: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FkpPageNumber {
  pub page_number: u32,
  pub unused: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChpxFkp {
  pub file_positions: Vec<u32>,
  pub runs: Vec<ChpxFkpRun>,
  pub unused_regions: Vec<FkpUnusedRegion>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChpxFkpRun {
  pub property_offset: Option<u16>,
  /// Clone-shared direct formatting; use [`Arc::make_mut`] for field edits.
  pub properties: Option<Arc<GrpPrl>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PapxFkp {
  pub file_positions: Vec<u32>,
  pub runs: Vec<PapxFkpRun>,
  pub unused_regions: Vec<FkpUnusedRegion>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PapxFkpRun {
  pub property_offset: Option<u16>,
  /// Version-specific PHE/PHE2 bytes in BxPap. They are a fixed-width
  /// physical field and remain distinct from page padding.
  pub paragraph_height_info: [u8; 12],
  /// Clone-shared paragraph formatting; use [`Arc::make_mut`] for field edits.
  pub properties: Option<Arc<PapxInFkp>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PapxInFkp {
  pub length_encoding: PapxLengthEncoding,
  pub style_index: u16,
  pub properties: GrpPrl,
  pub trailing_byte: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PapxLengthEncoding {
  HalfWordsMinusOne,
  ExtendedHalfWords,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FkpUnusedRegion {
  pub offset: u16,
  pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prc {
  pub properties: GrpPrl,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpPrl {
  pub properties: Vec<Prl>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prl {
  pub sprm: Sprm,
  pub operand: SprmOperand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sprm {
  pub property_id: u16,
  pub special: bool,
  pub group: SprmGroup,
  pub operand_size: SprmOperandSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SprmKind {
  Known(KnownSprm),
  Other(u16),
}

macro_rules! define_known_sprms {
    ($($variant:ident = $opcode:literal,)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u16)]
        pub enum KnownSprm {
            $($variant = $opcode,)+
        }

        impl KnownSprm {
            pub const fn from_opcode(opcode: u16) -> Option<Self> {
                match opcode {
                    $($opcode => Some(Self::$variant),)+
                    _ => None,
                }
            }

            pub const fn opcode(self) -> u16 {
                self as u16
            }

            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => concat!("sprm", stringify!($variant)),)+
                }
            }
        }
    };
}

define_known_sprms! {
    CFRMarkDel = 0x0800,
    CFRMarkIns = 0x0801,
    CFFldVanish = 0x0802,
    CFData = 0x0806,
    CFOle2 = 0x080a,
    CFWebHidden = 0x0811,
    CFSpecVanish = 0x0818,
    CFBold = 0x0835,
    CFItalic = 0x0836,
    CFStrike = 0x0837,
    CFOutline = 0x0838,
    CFShadow = 0x0839,
    CFSmallCaps = 0x083a,
    CFCaps = 0x083b,
    CFVanish = 0x083c,
    CFImprint = 0x0854,
    CFSpec = 0x0855,
    CFObj = 0x0856,
    CFEmboss = 0x0858,
    CFBiDi = 0x085a,
    CFBoldBi = 0x085c,
    CFItalicBi = 0x085d,
    CFUsePgsuSettings = 0x0868,
    CFNoProof = 0x0875,
    CFComplexScripts = 0x0882,
    PJc80 = 0x2403,
    PFKeep = 0x2405,
    PFKeepFollow = 0x2406,
    PFPageBreakBefore = 0x2407,
    PFNoLineNumb = 0x240c,
    PFInTable = 0x2416,
    PFTtp = 0x2417,
    PWr = 0x2423,
    PFNoAutoHyph = 0x242a,
    PFLocked = 0x2430,
    PFWidowControl = 0x2431,
    PFKinsoku = 0x2433,
    PFWordWrap = 0x2434,
    PFOverflowPunct = 0x2435,
    PFTopLinePunct = 0x2436,
    PFAutoSpaceDE = 0x2437,
    PFAutoSpaceDN = 0x2438,
    PFBiDi = 0x2441,
    PFNumRMIns = 0x2443,
    PFUsePgsuSettings = 0x2447,
    PFAdjustRight = 0x2448,
    PFInnerTableCell = 0x244b,
    PFInnerTtp = 0x244c,
    PFOpenTch = 0x245a,
    PFDyaBeforeAuto = 0x245b,
    PFDyaAfterAuto = 0x245c,
    PJc = 0x2461,
    PFNoAllowOverlap = 0x2462,
    PFContextualSpacing = 0x246d,
    PFMirrorIndents = 0x2470,
    PTtwo = 0x2471,
    PIncLvl = 0x2602,
    PIlvl = 0x260a,
    PPc = 0x261b,
    POutLvl = 0x2640,
    PWall = 0x2664,
    CSfxText = 0x2859,
    CIdctHint = 0x286f,
    CLbcCRJ = 0x2879,
    CHighlight = 0x2a0c,
    CPlain = 0x2a33,
    CKcd = 0x2a34,
    CKul = 0x2a3e,
    CIco = 0x2a42,
    CIss = 0x2a48,
    CFDStrike = 0x2a53,
    CWall = 0x2a83,
    CNeedFontFixup = 0x2a86,
    CFSdtVanish = 0x2a90,
    ScnsPgn = 0x3000,
    SiHeadingPgn = 0x3001,
    SFEvenlySpaced = 0x3005,
    SFProtected = 0x3006,
    SBkc = 0x3009,
    SFTitlePage = 0x300a,
    SNfcPgn = 0x300e,
    SFPgnRestart = 0x3011,
    SFEndnote = 0x3012,
    SLnc = 0x3013,
    SGprfIhdt = 0x3014,
    SLBetween = 0x3019,
    SVjc = 0x301a,
    SBOrientation = 0x301d,
    SFpc = 0x303b,
    SRncFtn = 0x303c,
    SRncEdn = 0x303e,
    SFBiDi = 0x3228,
    SFRTLGutter = 0x322a,
    SWall = 0x3239,
    TFCantSplit90 = 0x3403,
    TTableHeader = 0x3404,
    TFNoAllowOverlap = 0x3465,
    TFCantSplit = 0x3466,
    TCellVertAlignStyle = 0x347c,
    TCellNoWrapStyle = 0x347d,
    TCHorzBands = 0x3488,
    TCVertBands = 0x3489,
    TPc = 0x360d,
    TFAutofit = 0x3615,
    TFKeepFollow = 0x3619,
    TWall = 0x3668,
    PWHeightAbs = 0x442b,
    PDcs = 0x442c,
    PShd80 = 0x442d,
    PWAlignFont = 0x4439,
    PFrameTextFlow = 0x443a,
    PDxcRight = 0x4455,
    PDxcLeft = 0x4456,
    PDxcLeft1 = 0x4457,
    PDylBefore = 0x4458,
    PDylAfter = 0x4459,
    PIstd = 0x4600,
    PIlfo = 0x460b,
    PNest80 = 0x4610,
    PNest = 0x465f,
    CIbstRMark = 0x4804,
    CIdslRMark = 0x4807,
    CHpsPos = 0x4845,
    CHpsKern = 0x484b,
    CHresi = 0x484e,
    CCharScale = 0x4852,
    CLidBi = 0x485f,
    CIbstRMarkDel = 0x4863,
    CShd80 = 0x4866,
    CIdslRMarkDel = 0x4867,
    CRgLid0_80 = 0x486d,
    CRgLid1_80 = 0x486e,
    CRgLid0 = 0x4873,
    CRgLid1 = 0x4874,
    CPbiGrf = 0x4888,
    CIstd = 0x4a30,
    CHps = 0x4a43,
    CRgFtc0 = 0x4a4f,
    CRgFtc1 = 0x4a50,
    CRgFtc2 = 0x4a51,
    CFtcBi = 0x4a5e,
    CIcoBi = 0x4a60,
    CHpsBi = 0x4a61,
    SDmBinFirst = 0x5007,
    SDmBinOther = 0x5008,
    SCcolumns = 0x500b,
    SNLnnMod = 0x5015,
    SLnnMin = 0x501b,
    SPgnStart97 = 0x501c,
    SDmPaperReq = 0x5026,
    SClm = 0x5032,
    STextFlow = 0x5033,
    SNFtn = 0x503f,
    SNfcFtnRef = 0x5040,
    SNEdn = 0x5041,
    SNfcEdnRef = 0x5042,
    SPgbProp = 0x522f,
    TJc90 = 0x5400,
    TJc = 0x548a,
    TFBiDi = 0x560b,
    TDelete = 0x5622,
    TMerge = 0x5624,
    TSplit = 0x5625,
    TIstd = 0x563a,
    TFBiDi90 = 0x5664,
    PDyaLine = 0x6412,
    PBrcTop80 = 0x6424,
    PBrcLeft80 = 0x6425,
    PBrcBottom80 = 0x6426,
    PBrcRight80 = 0x6427,
    PBrcBetween80 = 0x6428,
    PIpgp = 0x6465,
    PRsid = 0x6467,
    PTableProps = 0x646b,
    PBrcBar80 = 0x6629,
    PHugePapx = 0x6646,
    PItap = 0x6649,
    PDtap = 0x664a,
    CDttmRMark = 0x6805,
    CRsidProp = 0x6815,
    CRsidText = 0x6816,
    CRsidRMDel = 0x6817,
    CDttmRMarkDel = 0x6864,
    CBrc80 = 0x6865,
    CCv = 0x6870,
    CCvUl = 0x6877,
    CPbiIBullet = 0x6887,
    CPicLocation = 0x6a03,
    CSymbol = 0x6a09,
    PicBrcTop80 = 0x6c02,
    PicBrcLeft80 = 0x6c03,
    PicBrcBottom80 = 0x6c04,
    PicBrcRight80 = 0x6c05,
    SBrcTop80 = 0x702b,
    SBrcLeft80 = 0x702c,
    SBrcBottom80 = 0x702d,
    SBrcRight80 = 0x702e,
    SDxtCharSpace = 0x7030,
    SRsid = 0x703a,
    SPgnStart = 0x7044,
    TTlp = 0x740a,
    TIpgp = 0x7469,
    TRsid = 0x7479,
    TInsert = 0x7621,
    TDxaCol = 0x7623,
    TTextFlow = 0x7629,
    PDxaRight80 = 0x840e,
    PDxaLeft80 = 0x840f,
    PDxaLeft180 = 0x8411,
    PDxaAbs = 0x8418,
    PDyaAbs = 0x8419,
    PDxaWidth = 0x841a,
    PDyaFromText = 0x842e,
    PDxaFromText = 0x842f,
    PDxaRight = 0x845d,
    PDxaLeft = 0x845e,
    PDxaLeft1 = 0x8460,
    CDxaSpace = 0x8840,
    SDxaColumns = 0x900c,
    SDxaLnn = 0x9016,
    SDyaTop = 0x9023,
    SDyaBottom = 0x9024,
    SDyaLinePitch = 0x9031,
    TDyaRowHeight = 0x9407,
    TDxaAbs = 0x940e,
    TDyaAbs = 0x940f,
    TDxaFromText = 0x9410,
    TDyaFromText = 0x9411,
    TDxaFromTextRight = 0x941e,
    TDyaFromTextBottom = 0x941f,
    TDxaLeft = 0x9601,
    TDxaGapHalf = 0x9602,
    PDyaBefore = 0xa413,
    PDyaAfter = 0xa414,
    SDyaHdrTop = 0xb017,
    SDyaHdrBottom = 0xb018,
    SXaPage = 0xb01f,
    SYaPage = 0xb020,
    SDxaLeft = 0xb021,
    SDxaRight = 0xb022,
    SDzaGutter = 0xb025,
    PIstdPermute = 0xc601,
    PChgTabsPapx = 0xc60d,
    PChgTabs = 0xc615,
    PNumRM = 0xc645,
    PShd = 0xc64d,
    PBrcTop = 0xc64e,
    PBrcLeft = 0xc64f,
    PBrcBottom = 0xc650,
    PBrcRight = 0xc651,
    PBrcBetween = 0xc652,
    PBrcBar = 0xc653,
    PCnf = 0xc666,
    PIstdListPermute = 0xc669,
    PTIstdInfo = 0xc66c,
    PPropRMark = 0xc66f,
    CFMathPr = 0xc81a,
    CIstdPermute = 0xca31,
    CMajority = 0xca47,
    CPropRMark90 = 0xca57,
    CDispFldRMark = 0xca62,
    CShd = 0xca71,
    CBrc = 0xca72,
    CFitText = 0xca76,
    CFELayout = 0xca78,
    CCnf = 0xca85,
    CPropRMark = 0xca89,
    PicBrcTop = 0xce08,
    PicBrcLeft = 0xce09,
    PicBrcBottom = 0xce0a,
    PicBrcRight = 0xce0b,
    SOlstAnm = 0xd202,
    SBrcTop = 0xd234,
    SBrcLeft = 0xd235,
    SBrcBottom = 0xd236,
    SBrcRight = 0xd237,
    SPropRMark = 0xd243,
    TCellBrcTopStyle = 0xd47f,
    TTableBorders80 = 0xd605,
    TDefTable = 0xd608,
    TDefTableShd80 = 0xd609,
    TDefTableShd3rd = 0xd60c,
    TDefTableShd = 0xd612,
    TTableBorders = 0xd613,
    TDefTableShd2nd = 0xd616,
    TBrcTopCv = 0xd61a,
    TBrcLeftCv = 0xd61b,
    TBrcBottomCv = 0xd61c,
    TBrcRightCv = 0xd61d,
    TSetBrc80 = 0xd620,
    TVertMerge = 0xd62b,
    TVertAlign = 0xd62c,
    TSetShd = 0xd62d,
    TSetShdOdd = 0xd62e,
    TSetBrc = 0xd62f,
    TCellPadding = 0xd632,
    TCellSpacingDefault = 0xd633,
    TCellPaddingDefault = 0xd634,
    TCellWidth = 0xd635,
    TFCellNoWrap = 0xd639,
    TCellPaddingStyle = 0xd63e,
    TCellFHideMark = 0xd642,
    TSetShdTable = 0xd660,
    TCellBrcType = 0xd662,
    TPropRMark = 0xd667,
    TCnf = 0xd66a,
    TDefTableShdRaw = 0xd670,
    TDefTableShdRaw2nd = 0xd671,
    TDefTableShdRaw3rd = 0xd672,
    TCellBrcBottomStyle = 0xd680,
    TCellBrcLeftStyle = 0xd681,
    TCellBrcRightStyle = 0xd682,
    TCellBrcInsideHStyle = 0xd683,
    TCellBrcInsideVStyle = 0xd684,
    TCellBrcTL2BRStyle = 0xd685,
    TCellBrcTR2BLStyle = 0xd686,
    TCellShdStyle = 0xd687,
    SDxaColWidth = 0xf203,
    SDxaColSpacing = 0xf204,
    TTableWidth = 0xf614,
    TWidthBefore = 0xf617,
    TWidthAfter = 0xf618,
    TFitText = 0xf636,
    TWidthIndent = 0xf661,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SprmGroup {
  Paragraph,
  Character,
  Picture,
  Section,
  Table,
  Compatibility(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SprmOperandSize {
  Toggle,
  Byte,
  Word,
  Dword,
  Word4,
  Word5,
  Variable,
  ThreeBytes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SprmOperand {
  Toggle(u8),
  Byte(u8),
  Word([u8; 2]),
  Dword([u8; 4]),
  Word4([u8; 2]),
  Word5([u8; 2]),
  Variable8(Vec<u8>),
  ParagraphChangeTabs(PChgTabsOperand),
  ParagraphChangeTabsPapx(PChgTabsPapxOperand),
  Shading(Shd),
  Border(Brc),
  PropertyRevisionMark(PropRMark),
  CharacterFitText(CFitTextOperand),
  TableCellSpacing(Cssa),
  TableBorderColors(Vec<ColorRef>),
  TableShading80(Vec<Shd80>),
  TableShading(Vec<Shd>),
  TableCellHideMark(CellHideMarkOperand),
  TableCellWidth(TableCellWidthOperand),
  ParagraphTableStyleInfo([u8; 16]),
  TableBorders(TableBordersOperand),
  TableBorders80(TableBordersOperand80),
  TableBorder(TableBrcOperand),
  TableBorder80(TableBrcOperand80),
  TableDefinition(TDefTableOperand),
  ParagraphNumberRevisionMark(NumRmOperand),
  CharacterMajority(Box<GrpPrl>),
  CharacterDisplayFieldRevisionMark(DispFldRmOperand),
  StylePermutation(SppOperand),
  ConditionalFormatting(CnfOperand),
  AutoNumberedListData(AnldOperand),
  OutlineListData(Box<OlstOperand>),
  SectionHeaderFooterFlags(SectionHeaderFooterFlags),
  /// `sprmTDefTable` stores a 16-bit count equal to the following byte
  /// count plus one.
  Variable16PlusOne(Vec<u8>),
  ThreeBytes([u8; 3]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PChgTabsOperand {
  pub deleted: Vec<DeletedTabStop>,
  pub added: Vec<AddedTabStop>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PChgTabsPapxOperand {
  pub deleted_positions: Vec<i16>,
  pub added: Vec<AddedTabStop>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorRef {
  pub red: u8,
  pub green: u8,
  pub blue: u8,
  pub auto: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shd {
  pub foreground: ColorRef,
  pub background: ColorRef,
  pub pattern: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Brc {
  pub color: ColorRef,
  pub line_width: u8,
  pub border_type: u8,
  pub spacing: u8,
  pub shadow: bool,
  pub frame: bool,
  pub reserved: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropRMark {
  pub has_revision: u8,
  pub author_index: i16,
  pub timestamp: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CFitTextOperand {
  pub width_twips: i32,
  pub fit_text_id: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CnfOperand {
  pub condition: i16,
  pub properties: Box<GrpPrl>,
}

/// MS-DOC `SPPOperand`, used by sprmPIstdPermute and sprmCIstdPermute.
/// The surrounding SPRM owns the one-byte `cb` prefix; this value models the
/// bounded body selected by that prefix.
#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_spp_operand")]
pub struct SppOperand {
  pub ignored_long: u8,
  pub first_style_index: u16,
  pub last_style_index: u16,
  #[sdk(remaining)]
  pub remapped_style_indices: Vec<u16>,
}

impl SppOperand {
  pub fn remap(&self, style_index: u16) -> Option<u16> {
    if !(self.first_style_index..=self.last_style_index).contains(&style_index) {
      return None;
    }
    self
      .remapped_style_indices
      .get(usize::from(style_index - self.first_style_index))
      .copied()
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anlv {
  pub number_format: u8,
  pub text_before: u8,
  pub text_after: u8,
  pub justification: u8,
  pub include_previous_levels: bool,
  pub hanging_indent: bool,
  pub set_bold: bool,
  pub set_italic: bool,
  pub set_small_caps: bool,
  pub set_caps: bool,
  pub set_strike: bool,
  pub set_underline: bool,
  pub previous_space: bool,
  pub bold: bool,
  pub italic: bool,
  pub small_caps: bool,
  pub caps: bool,
  pub strike: bool,
  pub underline: u8,
  pub color: u8,
  pub font_index: u16,
  pub font_size_half_points: u16,
  pub start_at: u16,
  pub indent_twips: i16,
  pub space_twips: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnldOperand {
  pub level: Anlv,
  pub number_one_per_cell: u8,
  pub number_across_cells: u8,
  pub restart_heading: u8,
  pub spare: u8,
  pub display_text: [u16; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OlstOperand {
  pub levels: [Anlv; 9],
  pub restart_heading: u8,
  pub reserved: [u8; 3],
  pub display_text: [u16; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SectionHeaderFooterFlags {
  pub even_header: bool,
  pub odd_header: bool,
  pub even_footer: bool,
  pub odd_footer: bool,
  pub first_header: bool,
  pub first_footer: bool,
  pub reserved: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellRange {
  pub first: u8,
  pub limit: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cssa {
  pub cells: CellRange,
  pub border_sides: u8,
  pub width_type: u8,
  pub width: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shd80 {
  pub foreground_color_index: u8,
  pub background_color_index: u8,
  pub pattern: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellHideMarkOperand {
  pub cells: CellRange,
  pub hide_when_empty: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableCellWidthOperand {
  pub cells: CellRange,
  pub width_type: u8,
  pub width: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkBitfield)]
#[sdk(repr = "u32")]
pub struct Brc80 {
  #[sdk(bits = 0..=7)]
  pub line_width: u8,
  #[sdk(bits = 8..=15)]
  pub border_type: u8,
  #[sdk(bits = 16..=23)]
  pub color_index: u8,
  #[sdk(bits = 24..=28)]
  pub spacing: u8,
  #[sdk(bit = 29)]
  pub shadow: bool,
  #[sdk(bit = 30)]
  pub frame: bool,
  #[sdk(bit = 31)]
  pub reserved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i16")]
pub enum PictureStorageFormat {
  Shape = 0x0064,
  ShapeFile = 0x0066,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct Mfpf {
  pub format: PictureStorageFormat,
  pub unused_x_extent: i16,
  pub unused_y_extent: i16,
  pub ignored_handle: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct PicfShape {
  pub ignored_flags: u32,
  pub padding1: u32,
  pub ignored_mapping_mode: i16,
  pub padding2: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
pub struct Picmid {
  pub goal_width_twips: i16,
  pub goal_height_twips: i16,
  pub horizontal_scale_tenths_percent: u16,
  pub vertical_scale_tenths_percent: u16,
  pub reserved_width1: i16,
  pub reserved_height1: i16,
  pub reserved_width2: i16,
  pub reserved_height2: i16,
  pub reserved_flags: u8,
  pub bits_per_pixel: u8,
  pub top_border: Brc80,
  pub left_border: Brc80,
  pub bottom_border: Brc80,
  pub right_border: Brc80,
  pub reserved_width3: i16,
  pub reserved_height3: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_picf")]
pub struct Picf {
  pub total_length: i32,
  pub header_length: u16,
  pub storage: Mfpf,
  pub shape: PicfShape,
  pub picture: Picmid,
  pub property_count: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PicfAndOfficeArtData {
  pub picf: Picf,
  pub shape_file_name: Option<Vec<u8>>,
  pub picture: OfficeArtStream,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NilPicfAndBinData {
  pub total_length: i32,
  pub header_length: u16,
  pub ignored_header: [u8; 62],
  pub binary_data: NilPicfBinaryData,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HyperlinkFieldType {
  Ref,
  PageRef,
  NoteRef,
  Hyperlink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormFieldType {
  Text,
  CheckBox,
  DropDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateFieldType {
  Private,
  AddIn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NilPicfFieldType {
  Hyperlink(HyperlinkFieldType),
  Form(FormFieldType),
  Private(PrivateFieldType),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NilPicfBinaryData {
  /// A physical NilPICF parsed without the owning Plcfld context.
  Unresolved(Vec<u8>),
  Hyperlink {
    field_type: HyperlinkFieldType,
    value: Hfd,
  },
  Form {
    field_type: FormFieldType,
    value: FfData,
  },
  Private {
    field_type: PrivateFieldType,
    bytes: Vec<u8>,
  },
  /// MS-DOC permits invalid binData and requires consumers to ignore it.
  Invalid {
    field_type: NilPicfFieldType,
    bytes: Vec<u8>,
  },
  /// The picture character could not be associated with one of the field
  /// types permitted by MS-DOC 2.9.158. Compatibility mode preserves it.
  InvalidContext(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_nil_picf_wire")]
struct NilPicfWire {
  total_length: i32,
  header_length: u16,
  ignored_header: [u8; 62],
  #[sdk(remaining)]
  binary_data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormFieldKind {
  Text,
  CheckBox,
  DropDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFormFieldKind {
  Regular,
  Number,
  DateOrTime,
  CurrentDate,
  CurrentTime,
  Calculated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FfDataBits {
  pub field_kind: FormFieldKind,
  pub result: u8,
  pub own_help: bool,
  pub own_status: bool,
  pub protected: bool,
  pub automatic_size: bool,
  pub text_kind: TextFormFieldKind,
  pub recalculate: bool,
  pub has_list_box: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HsttbDropList {
  pub entries: Vec<Vec<u16>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FfData {
  pub version: u32,
  pub bits: FfDataBits,
  pub maximum_text_length: u16,
  pub check_box_size_half_points: u16,
  pub name: Xstz,
  pub default_text: Option<Xstz>,
  pub default_selection: Option<u16>,
  pub text_format: Xstz,
  pub help_text: Xstz,
  pub status_text: Xstz,
  pub entry_macro: Xstz,
  pub exit_macro: Xstz,
  pub drop_down_list: Option<HsttbDropList>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HfdBits {
  pub open_in_new_window: bool,
  pub do_not_preserve_history: bool,
  pub image_map: bool,
  pub has_location: bool,
  pub has_tooltip: bool,
  pub unused: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hfd {
  pub bits: HfdBits,
  pub class_id: Guid,
  pub hyperlink: crate::xls::HyperlinkObject,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrcData {
  pub properties: GrpPrl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableBordersOperand {
  pub borders: [Brc; 6],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableBordersOperand80 {
  pub borders: [Brc80; 6],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableBrcOperand {
  pub cells: CellRange,
  pub borders_to_apply: u8,
  pub border: Brc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableBrcOperand80 {
  pub cells: CellRange,
  pub borders_to_apply: u8,
  pub border: Brc80,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TDefTableOperand {
  pub column_boundaries: Vec<i16>,
  pub cells: Vec<Tc80>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tc80 {
  pub formatting: TcGrf,
  pub preferred_width: u16,
  pub borders: [Brc80; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkBitfield)]
#[sdk(repr = "u16")]
pub struct TcGrf {
  #[sdk(bits = 0..=1)]
  pub horizontal_merge: u8,
  #[sdk(bits = 2..=4)]
  pub text_flow: u8,
  #[sdk(bits = 5..=6)]
  pub vertical_merge: u8,
  #[sdk(bits = 7..=8)]
  pub vertical_alignment: u8,
  #[sdk(bits = 9..=11)]
  pub width_type: u8,
  #[sdk(bit = 12)]
  pub fit_text: bool,
  #[sdk(bit = 13)]
  pub no_wrap: bool,
  #[sdk(bit = 14)]
  pub hide_mark: bool,
  #[sdk(bit = 15)]
  pub unused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeletedTabStop {
  pub position: i16,
  pub close_distance: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddedTabStop {
  pub position: i16,
  pub descriptor: TabDescriptor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkBitfield)]
#[sdk(repr = "u8")]
pub struct TabDescriptor {
  #[sdk(bits = 0..=2)]
  pub alignment: u8,
  #[sdk(bits = 3..=5)]
  pub leader: u8,
  #[sdk(bits = 6..=7)]
  pub reserved: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumRmOperand {
  pub numbered_before_tracking: u8,
  pub ignored_flag: u8,
  pub author_index: u16,
  pub timestamp: u32,
  pub placeholder_indices: [u8; 9],
  pub number_formats: [u8; 9],
  pub ignored: u16,
  pub number_values: [u32; 9],
  pub format_string: [u16; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispFldRmOperand {
  pub has_revision: u8,
  pub author_index: u16,
  pub timestamp: u32,
  pub previous_result: [u16; 16],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlcPcd {
  pub character_positions: Vec<i32>,
  pub pieces: Vec<Pcd>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pcd {
  pub no_paragraph_mark_at_end: bool,
  pub reserved1: bool,
  pub dirty: bool,
  pub reserved2: u16,
  pub file_position: FcCompressed,
  pub property_modifier: Prm,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextPiece {
  pub cp_start: i32,
  pub cp_end: i32,
  pub file_offset: u32,
  pub characters: TextPieceCharacters,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextPieceEncoding {
  Compressed,
  Utf16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextPieceString {
  pub value: String,
  pub encoding: TextPieceEncoding,
  code_units: Vec<u16>,
}

/// One DOC text-piece value together with its physical encoding.
///
/// Conforming text is exposed as a standard Rust [`String`]. An unpaired
/// surrogate cannot be represented by `String`, so compatible parsing keeps
/// its original code units in the explicit compatibility variant instead of
/// substituting U+FFFD.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextPieceCharacters {
  String(TextPieceString),
  CompatibilityUtf16 { code_units: Vec<u16> },
}

impl TextPieceCharacters {
  pub fn compressed(value: impl Into<String>) -> Result<Self> {
    let value = value.into();
    if value
      .chars()
      .any(|character| u32::from(character) > u32::from(u8::MAX))
    {
      return Err(Error::invalid(
        0,
        "DOC compressed text contains a character above U+00FF",
      ));
    }
    let code_units = value.encode_utf16().collect();
    Ok(Self::String(TextPieceString {
      value,
      encoding: TextPieceEncoding::Compressed,
      code_units,
    }))
  }

  pub fn utf16(value: impl Into<String>) -> Self {
    let value = value.into();
    let code_units = value.encode_utf16().collect();
    Self::String(TextPieceString {
      value,
      encoding: TextPieceEncoding::Utf16,
      code_units,
    })
  }

  fn from_compressed_bytes(bytes: &[u8]) -> Self {
    Self::String(TextPieceString {
      value: bytes.iter().copied().map(char::from).collect(),
      encoding: TextPieceEncoding::Compressed,
      code_units: bytes.iter().copied().map(u16::from).collect(),
    })
  }

  fn from_utf16_units(code_units: Vec<u16>) -> Self {
    match String::from_utf16(&code_units) {
      Ok(value) => Self::String(TextPieceString {
        value,
        encoding: TextPieceEncoding::Utf16,
        code_units,
      }),
      Err(_) => Self::CompatibilityUtf16 { code_units },
    }
  }

  pub const fn encoding(&self) -> TextPieceEncoding {
    match self {
      Self::String(value) => value.encoding,
      Self::CompatibilityUtf16 { .. } => TextPieceEncoding::Utf16,
    }
  }

  pub const fn value(&self) -> Option<&str> {
    match self {
      Self::String(value) => Some(value.value.as_str()),
      Self::CompatibilityUtf16 { .. } => None,
    }
  }

  pub const fn compatibility_code_units(&self) -> Option<&[u16]> {
    match self {
      Self::String(_) => None,
      Self::CompatibilityUtf16 { code_units } => Some(code_units.as_slice()),
    }
  }

  pub fn character_count(&self) -> usize {
    match self {
      Self::String(value) => value.code_units.len(),
      Self::CompatibilityUtf16 { code_units } => code_units.len(),
    }
  }

  pub(crate) const fn code_units(&self) -> &[u16] {
    match self {
      Self::String(value) => value.code_units.as_slice(),
      Self::CompatibilityUtf16 { code_units } => code_units.as_slice(),
    }
  }

  pub(crate) fn code_units_iter(&self) -> impl Iterator<Item = u16> + '_ {
    self.code_units().iter().copied()
  }

  pub(crate) fn string_range(&self, range: Range<usize>) -> Result<Option<&str>> {
    let Self::String(value) = self else {
      return Ok(None);
    };
    let unit_to_byte = |target: usize| -> Result<usize> {
      if target == 0 {
        return Ok(0);
      }
      let mut units = 0usize;
      for (byte, character) in value.value.char_indices() {
        units = units
          .checked_add(match value.encoding {
            TextPieceEncoding::Compressed => 1,
            TextPieceEncoding::Utf16 => character.len_utf16(),
          })
          .ok_or_else(|| Error::Limit("DOC text unit count overflow".into()))?;
        if units == target {
          return Ok(byte + character.len_utf8());
        }
        if units > target {
          return Err(Error::invalid(
            0,
            "DOC CP boundary splits a UTF-16 surrogate pair",
          ));
        }
      }
      if units == target {
        Ok(value.value.len())
      } else {
        Err(Error::invalid(0, "DOC text range exceeds its value"))
      }
    };
    let start = unit_to_byte(range.start)?;
    let end = unit_to_byte(range.end)?;
    Ok(Some(&value.value[start..end]))
  }

  pub(crate) fn replace_code_unit_range(
    &mut self,
    range: Range<usize>,
    replacement: &Self,
  ) -> Result<()> {
    if matches!(self, Self::CompatibilityUtf16 { .. })
      || matches!(replacement, Self::CompatibilityUtf16 { .. })
    {
      return Err(Error::invalid(
        0,
        "DOC text mutation does not accept compatibility UTF-16",
      ));
    }
    let destination_encoding = match (self.encoding(), replacement.encoding()) {
      (TextPieceEncoding::Compressed, TextPieceEncoding::Compressed) => {
        TextPieceEncoding::Compressed
      }
      _ => TextPieceEncoding::Utf16,
    };
    let mut code_units = self.code_units().to_vec();
    if range.end > code_units.len() {
      return Err(Error::invalid(
        0,
        "DOC text replacement exceeds its text piece",
      ));
    }
    code_units.splice(range, replacement.code_units().iter().copied());
    *self = match destination_encoding {
      TextPieceEncoding::Compressed => {
        let value = code_units
          .into_iter()
          .map(|unit| {
            u8::try_from(unit).map(char::from).map_err(|_| {
              Error::invalid(
                0,
                "DOC compressed replacement contains a character above U+00FF",
              )
            })
          })
          .collect::<Result<String>>()?;
        Self::compressed(value)?
      }
      TextPieceEncoding::Utf16 => Self::utf16(
        String::from_utf16(&code_units)
          .map_err(|_| Error::invalid(0, "DOC replacement creates an unpaired UTF-16 surrogate"))?,
      ),
    };
    Ok(())
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    if let Self::String(value) = self
      && !value
        .value
        .encode_utf16()
        .eq(value.code_units.iter().copied())
    {
      return Err(Error::invalid(
        0,
        "DOC text String changed outside the transactional file-root API",
      ));
    }
    match self {
      Self::String(value) if value.encoding == TextPieceEncoding::Compressed => value
        .value
        .chars()
        .map(|character| {
          u8::try_from(u32::from(character))
            .map_err(|_| Error::invalid(0, "DOC compressed text contains a character above U+00FF"))
        })
        .collect(),
      Self::String(value) => Ok(
        value
          .value
          .encode_utf16()
          .flat_map(u16::to_le_bytes)
          .collect(),
      ),
      Self::CompatibilityUtf16 { code_units } => Ok(
        code_units
          .iter()
          .flat_map(|unit| unit.to_le_bytes())
          .collect(),
      ),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkBitfield)]
#[sdk(repr = "u32")]
pub struct FcCompressed {
  #[sdk(bits = 0..=29)]
  pub fc: u32,
  #[sdk(bit = 30)]
  pub compressed: bool,
  #[sdk(bit = 31)]
  pub reserved: bool,
}

impl FcCompressed {
  pub fn byte_offset(self) -> u32 {
    if self.compressed {
      self.fc / 2
    } else {
      self.fc
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prm {
  Simple { isprm: u8, value: u8 },
  Complex { property_run_index: u16 },
}

/// A zero-allocation view of the property modification selected by a Pcd.Prm.
///
/// Prm0 encodes one closed-table SPRM and its byte operand inline. Prm1
/// borrows the referenced CLX Prc property array from its single owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrmPropertiesRef<'a> {
  Empty,
  Simple { sprm: KnownSprm, value: u8 },
  Complex(&'a GrpPrl),
}

impl Prm {
  /// Resolves this modifier without cloning a CLX property array or
  /// allocating a one-property `GrpPrl` for an inline Prm0.
  pub fn property_modifications_ref<'a>(self, clx: &'a Clx) -> Result<PrmPropertiesRef<'a>> {
    match self {
      Self::Simple { isprm: 0, value: 0 } => Ok(PrmPropertiesRef::Empty),
      Self::Simple { isprm, value } => {
        let sprm = simple_prm_sprm(isprm).ok_or_else(|| {
          Error::invalid(
            u64::from(isprm),
            format!("Prm0 isprm 0x{isprm:02x} is not defined by MS-DOC"),
          )
        })?;
        Ok(PrmPropertiesRef::Simple { sprm, value })
      }
      Self::Complex { property_run_index } => clx
        .property_runs
        .get(usize::from(property_run_index))
        .map(|run| PrmPropertiesRef::Complex(&run.properties))
        .ok_or_else(|| {
          Error::invalid(
            u64::from(property_run_index),
            "Prm1 property-run index exceeds the CLX Prc array",
          )
        }),
    }
  }

  /// Resolves the property modifications selected by this `Prm`.
  ///
  /// MS-DOC 2.9.215 defines `Prm0.isprm` as a closed, non-arithmetic
  /// mapping to one `Sprm`; MS-DOC 2.9.216 defines `Prm1` as an index into
  /// the preceding CLX `Prc` array. The returned `GrpPrl` retains the
  /// specification order and can subsequently be filtered by `SprmGroup`
  /// for direct paragraph or direct character formatting.
  pub fn property_modifications(self, clx: &Clx) -> Result<GrpPrl> {
    match self.property_modifications_ref(clx)? {
      PrmPropertiesRef::Empty => Ok(GrpPrl {
        properties: Vec::new(),
      }),
      PrmPropertiesRef::Simple { sprm, value } => {
        let opcode = sprm.opcode();
        GrpPrl::from_bytes(&[opcode.to_le_bytes()[0], opcode.to_le_bytes()[1], value])
      }
      PrmPropertiesRef::Complex(properties) => Ok(properties.clone()),
    }
  }
}

/// The normative `Prm0.isprm` table from MS-DOC 2.9.215.
///
/// This is deliberately an explicit table rather than a bit-layout derive:
/// the numeric keys do not encode the resulting SPRM opcode.
const fn simple_prm_sprm(isprm: u8) -> Option<KnownSprm> {
  Some(match isprm {
    0x00 => KnownSprm::CLbcCRJ,
    0x04 => KnownSprm::PIncLvl,
    0x05 => KnownSprm::PJc,
    0x07 => KnownSprm::PFKeep,
    0x08 => KnownSprm::PFKeepFollow,
    0x09 => KnownSprm::PFPageBreakBefore,
    0x0c => KnownSprm::PIlvl,
    0x0d => KnownSprm::PFMirrorIndents,
    0x0e => KnownSprm::PFNoLineNumb,
    0x0f => KnownSprm::PTtwo,
    0x18 => KnownSprm::PFInTable,
    0x19 => KnownSprm::PFTtp,
    0x1d => KnownSprm::PPc,
    0x25 => KnownSprm::PWr,
    0x2c => KnownSprm::PFNoAutoHyph,
    0x32 => KnownSprm::PFLocked,
    0x33 => KnownSprm::PFWidowControl,
    0x35 => KnownSprm::PFKinsoku,
    0x36 => KnownSprm::PFWordWrap,
    0x37 => KnownSprm::PFOverflowPunct,
    0x38 => KnownSprm::PFTopLinePunct,
    0x39 => KnownSprm::PFAutoSpaceDE,
    0x3a => KnownSprm::PFAutoSpaceDN,
    0x41 => KnownSprm::CFRMarkDel,
    0x42 => KnownSprm::CFRMarkIns,
    0x43 => KnownSprm::CFFldVanish,
    0x47 => KnownSprm::CFData,
    0x4b => KnownSprm::CFOle2,
    0x4d => KnownSprm::CHighlight,
    0x4e => KnownSprm::CFEmboss,
    0x4f => KnownSprm::CSfxText,
    0x50 => KnownSprm::CFWebHidden,
    0x51 => KnownSprm::CFSpecVanish,
    0x53 => KnownSprm::CPlain,
    0x55 => KnownSprm::CFBold,
    0x56 => KnownSprm::CFItalic,
    0x57 => KnownSprm::CFStrike,
    0x58 => KnownSprm::CFOutline,
    0x59 => KnownSprm::CFShadow,
    0x5a => KnownSprm::CFSmallCaps,
    0x5b => KnownSprm::CFCaps,
    0x5c => KnownSprm::CFVanish,
    0x5e => KnownSprm::CKul,
    0x62 => KnownSprm::CIco,
    0x68 => KnownSprm::CIss,
    0x73 => KnownSprm::CFDStrike,
    0x74 => KnownSprm::CFImprint,
    0x75 => KnownSprm::CFSpec,
    0x76 => KnownSprm::CFObj,
    0x78 => KnownSprm::POutLvl,
    0x7b => KnownSprm::CFSdtVanish,
    0x7c => KnownSprm::CNeedFontFixup,
    0x7e => KnownSprm::PFNumRMIns,
    _ => return None,
  })
}

impl Clx {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let mut property_runs = Vec::new();
    loop {
      let marker = input.u8()?;
      match marker {
        0x01 => {
          let length = input.i16()?;
          let length = usize::try_from(length).map_err(|_| {
            Error::invalid(input.offset.saturating_sub(2) as u64, "negative Prc length")
          })?;
          let grpprl = input.bytes(length)?;
          property_runs.push(Prc {
            properties: GrpPrl::from_bytes(grpprl).map_err(|error| {
              Error::invalid(
                input.offset.saturating_sub(length) as u64,
                format!(
                  "invalid PRC grpprl ({error}); bytes {:02x?}",
                  &grpprl[..grpprl.len().min(64)]
                ),
              )
            })?,
          });
        }
        0x02 => {
          let length = usize::try_from(input.u32()?)
            .map_err(|_| Error::Limit("PlcPcd length exceeds usize".into()))?;
          let piece_table = PlcPcd::from_bytes(input.bytes(length)?)?;
          if input.offset != bytes.len() {
            return Err(Error::invalid(
              input.offset as u64,
              "trailing bytes after Pcdt",
            ));
          }
          return Ok(Self {
            property_runs,
            piece_table,
          });
        }
        value => {
          return Err(Error::invalid(
            input.offset.saturating_sub(1) as u64,
            format!("invalid CLX marker 0x{value:02x}"),
          ));
        }
      }
    }
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for run in &self.property_runs {
      bytes.push(0x01);
      let grpprl = run.properties.to_bytes()?;
      let length =
        i16::try_from(grpprl.len()).map_err(|_| Error::Limit("Prc grpprl exceeds i16".into()))?;
      bytes.extend_from_slice(&length.to_le_bytes());
      bytes.extend_from_slice(&grpprl);
    }
    let piece_table = self.piece_table.to_bytes()?;
    bytes.push(0x02);
    push_u32(
      &mut bytes,
      u32::try_from(piece_table.len()).map_err(|_| Error::Limit("PlcPcd exceeds u32".into()))?,
    );
    bytes.extend_from_slice(&piece_table);
    Ok(bytes)
  }
}

impl PlcfSed {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let payload = bytes
      .len()
      .checked_sub(4)
      .ok_or_else(|| Error::invalid(0, "PlcfSed is shorter than its final CP"))?;
    if !payload.is_multiple_of(16) {
      return Err(Error::invalid(
        0,
        "PlcfSed length does not match 4-byte CPs and 12-byte SEDs",
      ));
    }
    let section_count = payload / 16;
    let mut input = SliceReader::new(bytes);
    let mut character_positions = Vec::with_capacity(section_count + 1);
    for _ in 0..=section_count {
      character_positions.push(input.i32()?);
    }
    let mut sections = Vec::with_capacity(section_count);
    for _ in 0..section_count {
      sections.push(Sed {
        file_number: input.u16()?,
        sepx_offset: input.i32()?,
        mpr_file_number: input.u16()?,
        mpr_offset: input.i32()?,
      });
    }
    Ok(Self {
      character_positions,
      sections,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.character_positions.len() != self.sections.len() + 1 {
      return Err(Error::invalid(0, "PlcfSed must have one more CP than SED"));
    }
    let mut bytes =
      Vec::with_capacity(self.character_positions.len() * 4 + self.sections.len() * 12);
    for position in &self.character_positions {
      bytes.extend_from_slice(&position.to_le_bytes());
    }
    for section in &self.sections {
      bytes.extend_from_slice(&section.file_number.to_le_bytes());
      bytes.extend_from_slice(&section.sepx_offset.to_le_bytes());
      bytes.extend_from_slice(&section.mpr_file_number.to_le_bytes());
      bytes.extend_from_slice(&section.mpr_offset.to_le_bytes());
    }
    Ok(bytes)
  }
}

impl CpOnlyTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(4) {
      return Err(Error::invalid(0, "CP-only PLC is not an array of CPs"));
    }
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(bytes.len() / 4);
    while input.offset < bytes.len() {
      positions.push(input.u32()?);
    }
    require_nondecreasing(&positions, "CP-only PLC")?;
    Ok(Self { positions })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.is_empty() {
      return Err(Error::invalid(0, "CP-only PLC is empty"));
    }
    require_nondecreasing(&self.positions, "CP-only PLC")?;
    let mut bytes = Vec::with_capacity(self.positions.len() * 4);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    Ok(bytes)
  }
}

impl HeaderTextTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(4) {
      return Err(Error::invalid(0, "Plcfhdd is not an array of CPs"));
    }
    let mut input = SliceReader::new(bytes);
    let mut boundaries = Vec::with_capacity(bytes.len() / 4);
    while input.offset < bytes.len() {
      boundaries.push(match input.u32()? {
        u32::MAX => HeaderStoryBoundary::Missing,
        value => HeaderStoryBoundary::Position(value),
      });
    }
    Ok(Self { boundaries })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.boundaries.is_empty() {
      return Err(Error::invalid(0, "Plcfhdd is empty"));
    }
    let mut bytes = Vec::with_capacity(self.boundaries.len() * 4);
    for boundary in &self.boundaries {
      push_u32(
        &mut bytes,
        match boundary {
          HeaderStoryBoundary::Position(value) => *value,
          HeaderStoryBoundary::Missing => u32::MAX,
        },
      );
    }
    Ok(bytes)
  }
}

impl NoteReferenceTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(6) {
      return Err(Error::invalid(0, "note reference PLC length is invalid"));
    }
    let count = (bytes.len() - 4) / 6;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut indices = Vec::with_capacity(count);
    for _ in 0..count {
      indices.push(input.u16()?);
    }
    require_nondecreasing(&positions, "note reference CP")?;
    Ok(Self { positions, indices })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.indices.len().saturating_add(1) {
      return Err(Error::invalid(
        0,
        "note reference CP/data cardinality changed",
      ));
    }
    require_nondecreasing(&self.positions, "note reference CP")?;
    let mut bytes = Vec::with_capacity(self.positions.len() * 4 + self.indices.len() * 2);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for index in &self.indices {
      push_u16(&mut bytes, *index);
    }
    Ok(bytes)
  }
}

impl AnnotationReferenceTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(34) {
      return Err(Error::invalid(
        0,
        "PlcfandRef length does not match ATRDPre10",
      ));
    }
    let count = (bytes.len() - 4) / 34;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut annotations = Vec::with_capacity(count);
    for _ in 0..count {
      annotations.push(AnnotationReference::read(&mut input)?);
    }
    require_nondecreasing(&positions, "PlcfandRef CP")?;
    Ok(Self {
      positions,
      annotations,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.annotations.len().saturating_add(1) {
      return Err(Error::invalid(0, "PlcfandRef CP/ATRD cardinality changed"));
    }
    require_nondecreasing(&self.positions, "PlcfandRef CP")?;
    let mut bytes = Vec::with_capacity(self.positions.len() * 4 + self.annotations.len() * 30);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for annotation in &self.annotations {
      annotation.write(&mut bytes)?;
    }
    Ok(bytes)
  }
}

impl AnnotationReference {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let initials_length = input.u16()?;
    if initials_length > 9 {
      return Err(Error::invalid(
        input.offset as u64 - 2,
        "ATRDPre10 initials length exceeds 9",
      ));
    }
    let mut initials_buffer = [0u16; 9];
    for character in &mut initials_buffer {
      *character = input.u16()?;
    }
    Ok(Self {
      initials_length,
      initials_buffer,
      author_index: input.i16()?,
      bits_not_used: input.u16()?,
      flags_not_used: input.u16()?,
      bookmark_tag: input.i32()?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.initials_length > 9 {
      return Err(Error::invalid(0, "ATRDPre10 initials length exceeds 9"));
    }
    push_u16(bytes, self.initials_length);
    for character in self.initials_buffer {
      push_u16(bytes, character);
    }
    bytes.extend_from_slice(&self.author_index.to_le_bytes());
    push_u16(bytes, self.bits_not_used);
    push_u16(bytes, self.flags_not_used);
    bytes.extend_from_slice(&self.bookmark_tag.to_le_bytes());
    Ok(())
  }
}

impl AnnotationExtendedData {
  const RECORD_SIZE: usize = 18;

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if !bytes.len().is_multiple_of(Self::RECORD_SIZE) {
      return Err(Error::invalid(
        0,
        "AtrdExtra length does not match ATRDPost10 records",
      ));
    }
    let mut input = SliceReader::new(bytes);
    let mut comments = Vec::with_capacity(bytes.len() / Self::RECORD_SIZE);
    while input.offset < bytes.len() {
      comments.push(AnnotationPost10::read(&mut input)?);
    }
    Self::validate_tree(&comments)?;
    Ok(Self { comments })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    Self::validate_tree(&self.comments)?;
    let mut bytes = Vec::with_capacity(self.comments.len() * Self::RECORD_SIZE);
    for comment in &self.comments {
      comment.write(&mut bytes)?;
    }
    Ok(bytes)
  }

  fn validate_tree(comments: &[AnnotationPost10]) -> Result<()> {
    for (index, comment) in comments.iter().enumerate() {
      if comment.depth == 0 {
        if comment.parent_offset != 0 {
          return Err(Error::invalid(
            index as u64 * Self::RECORD_SIZE as u64,
            "root ATRDPost10 has a parent offset",
          ));
        }
        continue;
      }
      if comment.parent_offset == 0 {
        return Err(Error::invalid(
          index as u64 * Self::RECORD_SIZE as u64,
          "non-root ATRDPost10 has no parent offset",
        ));
      }
      let parent_index = i64::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(i64::from(comment.parent_offset)))
        .and_then(|index| usize::try_from(index).ok())
        .filter(|parent_index| *parent_index < index)
        .ok_or_else(|| {
          Error::invalid(
            index as u64 * Self::RECORD_SIZE as u64,
            "ATRDPost10 parent is outside the preceding comment tree",
          )
        })?;
      if comments[parent_index].depth.checked_add(1) != Some(comment.depth) {
        return Err(Error::invalid(
          index as u64 * Self::RECORD_SIZE as u64,
          "ATRDPost10 depth does not follow its parent",
        ));
      }
    }
    Ok(())
  }
}

impl AnnotationPost10 {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let modified = Dttm::from_u32(input.u32()?)?;
    let padding1 = input.u16()?;
    let depth = input.u32()?;
    let parent_offset = input.i32()?;
    let flags = input.u32()?;
    Ok(Self {
      modified,
      padding1,
      depth,
      parent_offset,
      ows_discussion_item: flags & 1 != 0,
      ink: flags & 2 != 0,
      padding2: flags >> 2,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.padding2 > 0x3fff_ffff {
      return Err(Error::invalid(0, "ATRDPost10 padding2 exceeds 30 bits"));
    }
    push_u32(bytes, self.modified.to_u32()?);
    push_u16(bytes, self.padding1);
    push_u32(bytes, self.depth);
    bytes.extend_from_slice(&self.parent_offset.to_le_bytes());
    push_u32(
      bytes,
      u32::from(self.ows_discussion_item) | (u32::from(self.ink) << 1) | (self.padding2 << 2),
    );
    Ok(())
  }
}

impl UserInputMethods {
  const METHOD_SIZE: usize = 20;

  pub fn from_bytes(method_bytes: &[u8], guid_bytes: &[u8]) -> Result<Self> {
    if method_bytes.len() < 4 || !(method_bytes.len() - 4).is_multiple_of(24) {
      return Err(Error::invalid(
        0,
        "Plcfuim length does not match 20-byte UIM records",
      ));
    }
    let method_count = (method_bytes.len() - 4) / 24;
    let mut input = SliceReader::new(method_bytes);
    let mut positions = Vec::with_capacity(method_count + 1);
    for _ in 0..=method_count {
      positions.push(input.u32()?);
    }
    let mut methods = Vec::with_capacity(method_count);
    for _ in 0..method_count {
      methods.push(UserInputMethod::read(&mut input)?);
    }

    let mut input = SliceReader::new(guid_bytes);
    let guid_count = usize::try_from(input.u32()?)
      .map_err(|_| Error::Limit("PlfguidUim count exceeds usize".into()))?;
    let expected_guid_size = 4usize
      .checked_add(
        guid_count
          .checked_mul(16)
          .ok_or_else(|| Error::Limit("PlfguidUim size overflows usize".into()))?,
      )
      .ok_or_else(|| Error::Limit("PlfguidUim size overflows usize".into()))?;
    if guid_bytes.len() != expected_guid_size {
      return Err(Error::invalid(
        0,
        "PlfguidUim count does not match its physical length",
      ));
    }
    let mut service_guids = Vec::with_capacity(guid_count);
    for _ in 0..guid_count {
      service_guids.push(
        input
          .bytes(16)?
          .try_into()
          .expect("a 16-byte GUID slice was requested"),
      );
    }
    Self::validate_guid_references(&methods, service_guids.len())?;
    Ok(Self {
      positions,
      methods,
      service_guids,
    })
  }

  pub fn to_bytes(&self) -> Result<(Vec<u8>, Vec<u8>)> {
    if self.positions.len() != self.methods.len().saturating_add(1) {
      return Err(Error::invalid(0, "Plcfuim CP/UIM cardinality changed"));
    }
    Self::validate_guid_references(&self.methods, self.service_guids.len())?;
    let mut method_bytes =
      Vec::with_capacity(self.positions.len() * 4 + self.methods.len() * Self::METHOD_SIZE);
    for position in &self.positions {
      push_u32(&mut method_bytes, *position);
    }
    for method in &self.methods {
      method.write(&mut method_bytes);
    }
    let mut guid_bytes = Vec::with_capacity(4 + self.service_guids.len() * 16);
    push_u32(
      &mut guid_bytes,
      u32::try_from(self.service_guids.len())
        .map_err(|_| Error::Limit("PlfguidUim count exceeds u32".into()))?,
    );
    for guid in &self.service_guids {
      guid_bytes.extend_from_slice(guid);
    }
    Ok((method_bytes, guid_bytes))
  }

  fn validate_guid_references(methods: &[UserInputMethod], guid_count: usize) -> Result<()> {
    for method in methods {
      for index in [method.service_category_index, method.service_clsid_index] {
        if usize::try_from(index).map_or(true, |index| index >= guid_count) {
          return Err(Error::invalid(
            0,
            "UIM references a GUID outside PlfguidUim",
          ));
        }
      }
    }
    Ok(())
  }
}

impl UserInputMethod {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    Ok(Self {
      service_category_index: input.i16()?,
      service_clsid_index: input.i16()?,
      service_data_offset: input.i32()?,
      character_count: input.i32()?,
      service_data_size: input.u32()?,
      private_data: input.u32()?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&self.service_category_index.to_le_bytes());
    bytes.extend_from_slice(&self.service_clsid_index.to_le_bytes());
    bytes.extend_from_slice(&self.service_data_offset.to_le_bytes());
    bytes.extend_from_slice(&self.character_count.to_le_bytes());
    push_u32(bytes, self.service_data_size);
    push_u32(bytes, self.private_data);
  }

  pub fn service_data(self, table_stream: &[u8]) -> Result<&[u8]> {
    if self.service_data_size == 0 {
      return Ok(&table_stream[..0]);
    }
    let start = usize::try_from(self.service_data_offset)
      .map_err(|_| Error::invalid(0, "UIM service-data offset is negative"))?;
    let size = usize::try_from(self.service_data_size)
      .map_err(|_| Error::Limit("UIM service-data size exceeds usize".into()))?;
    let end = start
      .checked_add(size)
      .ok_or_else(|| Error::Limit("UIM service-data range overflows usize".into()))?;
    table_stream
      .get(start..end)
      .ok_or_else(|| Error::invalid(start as u64, "UIM service data exceeds Table stream"))
  }
}

impl PrinterDriverInfo {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut offset = 0usize;
    let mut next_string = |name: &str| -> Result<Vec<u8>> {
      let remaining = bytes
        .get(offset..)
        .ok_or_else(|| Error::invalid(offset as u64, format!("missing {name}")))?;
      let length = remaining
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| Error::invalid(offset as u64, format!("unterminated PrDrvr {name}")))?;
      let value = remaining[..length].to_vec();
      offset = offset
        .checked_add(length + 1)
        .ok_or_else(|| Error::Limit("PrDrvr offset overflows usize".into()))?;
      Ok(value)
    };
    let value = Self {
      printer_name: next_string("printer name")?,
      port_name: next_string("port name")?,
      driver_name: next_string("driver name")?,
      product_name: next_string("product name")?,
    };
    if offset != bytes.len() {
      return Err(Error::invalid(offset as u64, "PrDrvr has trailing bytes"));
    }
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for (name, value) in [
      ("printer name", &self.printer_name),
      ("port name", &self.port_name),
      ("driver name", &self.driver_name),
      ("product name", &self.product_name),
    ] {
      if value.contains(&0) {
        return Err(Error::invalid(
          0,
          format!("PrDrvr {name} contains an embedded NUL"),
        ));
      }
      bytes.extend_from_slice(value);
      bytes.push(0);
    }
    Ok(bytes)
  }
}

impl OleObjectDescriptor {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if !matches!(bytes.len(), 4 | 6) {
      return Err(Error::invalid(0, "ODT must contain exactly 4 or 6 bytes"));
    }
    let persist1 =
      OleObjectPersist1Flags::from_bits_retain(u16::from_le_bytes([bytes[0], bytes[1]]));
    let clipboard_format =
      OleObjectClipboardFormat::from_raw(u16::from_le_bytes([bytes[2], bytes[3]]));
    let persist2 = (bytes.len() == 6)
      .then(|| OleObjectPersist2Flags::from_bits_retain(u16::from_le_bytes([bytes[4], bytes[5]])));
    Ok(Self {
      persist1,
      clipboard_format,
      persist2,
    })
  }

  pub fn to_bytes(self) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(if self.persist2.is_some() { 6 } else { 4 });
    bytes.extend_from_slice(&self.persist1.bits().to_le_bytes());
    bytes.extend_from_slice(&self.clipboard_format.raw().to_le_bytes());
    if let Some(persist2) = self.persist2 {
      bytes.extend_from_slice(&persist2.bits().to_le_bytes());
    }
    bytes
  }

  pub fn is_ole_control(self) -> bool {
    self.persist1.contains(OleObjectPersist1Flags::OCX)
  }

  pub fn control_uses_stream(self) -> bool {
    self.is_ole_control() && self.persist1.contains(OleObjectPersist1Flags::STREAM)
  }
}

impl OleObjectClipboardFormat {
  fn from_raw(value: u16) -> Self {
    match value {
      0x0001 => Self::RichText,
      0x0002 => Self::Text,
      0x0003 => Self::Metafile,
      0x0004 => Self::Bitmap,
      0x0005 => Self::Dib,
      0x000a => Self::Html,
      0x0014 => Self::UnicodeText,
      value => Self::Compatibility(value),
    }
  }

  pub fn raw(self) -> u16 {
    match self {
      Self::RichText => 0x0001,
      Self::Text => 0x0002,
      Self::Metafile => 0x0003,
      Self::Bitmap => 0x0004,
      Self::Dib => 0x0005,
      Self::Html => 0x000a,
      Self::UnicodeText => 0x0014,
      Self::Compatibility(value) => value,
    }
  }
}

impl OleControlInfos {
  const RECORD_SIZE: usize = 20;

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(Self::RECORD_SIZE) {
      return Err(Error::invalid(
        0,
        "RgxOcxInfo length does not match 20-byte OcxInfo records",
      ));
    }
    let mut input = SliceReader::new(bytes);
    let count = usize::try_from(input.u32()?)
      .map_err(|_| Error::Limit("RgxOcxInfo count exceeds usize".into()))?;
    if count != (bytes.len() - 4) / Self::RECORD_SIZE {
      return Err(Error::invalid(
        0,
        "RgxOcxInfo count does not match physical length",
      ));
    }
    let mut controls = Vec::with_capacity(count);
    for _ in 0..count {
      let control = OleControlInfo::read(&mut input)?;
      controls.push(control);
    }
    Ok(Self { controls })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(4 + self.controls.len() * Self::RECORD_SIZE);
    push_u32(
      &mut bytes,
      u32::try_from(self.controls.len())
        .map_err(|_| Error::Limit("RgxOcxInfo count exceeds u32".into()))?,
    );
    for control in &self.controls {
      control.write(&mut bytes)?;
    }
    Ok(bytes)
  }
}

impl OleControlInfo {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let cookie = input.u32()?;
    let field_index = input.u32()?;
    let ignored_accelerator_handle = input.u32()?;
    let accelerator_count = input.u16()?;
    let flags = input.u16()?;
    Ok(Self {
      cookie,
      field_index,
      ignored_accelerator_handle,
      accelerator_count,
      field_linked: flags & 0x0001 != 0,
      eats_return: flags & 0x0002 != 0,
      eats_escape: flags & 0x0004 != 0,
      default_button: flags & 0x0008 != 0,
      cancel_button: flags & 0x0010 != 0,
      failed_load: flags & 0x0020 != 0,
      right_to_left: flags & 0x0040 != 0,
      corrupt: flags & 0x0080 != 0,
      reserved1: (flags >> 8) as u8,
      document_part: OleControlDocumentPart::from_u16(input.u16()?)?,
      reserved2: input.u16()?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    push_u32(bytes, self.cookie);
    push_u32(bytes, self.field_index);
    push_u32(bytes, self.ignored_accelerator_handle);
    push_u16(bytes, self.accelerator_count);
    push_u16(
      bytes,
      u16::from(self.field_linked)
        | (u16::from(self.eats_return) << 1)
        | (u16::from(self.eats_escape) << 2)
        | (u16::from(self.default_button) << 3)
        | (u16::from(self.cancel_button) << 4)
        | (u16::from(self.failed_load) << 5)
        | (u16::from(self.right_to_left) << 6)
        | (u16::from(self.corrupt) << 7)
        | (u16::from(self.reserved1) << 8),
    );
    push_u16(bytes, self.document_part.to_u16());
    push_u16(bytes, self.reserved2);
    Ok(())
  }
}

impl OleControlDocumentPart {
  fn from_u16(value: u16) -> Result<Self> {
    match value {
      1 => Ok(Self::Main),
      2 => Ok(Self::Header),
      3 => Ok(Self::Footnote),
      4 => Ok(Self::Textbox),
      6 => Ok(Self::Endnote),
      7 => Ok(Self::Comment),
      8 => Ok(Self::HeaderTextbox),
      value => Ok(Self::Compatibility(value)),
    }
  }

  fn to_u16(self) -> u16 {
    match self {
      Self::Main => 1,
      Self::Header => 2,
      Self::Footnote => 3,
      Self::Textbox => 4,
      Self::Endnote => 6,
      Self::Comment => 7,
      Self::HeaderTextbox => 8,
      Self::Compatibility(value) => value,
    }
  }
}

impl AnnotationOwners {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let mut names = Vec::new();
    while input.offset < bytes.len() {
      if names.len() == 0x7fff {
        return Err(Error::Limit("GrpXstAtnOwners count exceeds 0x7fff".into()));
      }
      let length = usize::from(input.u16()?);
      if length >= 56 {
        return Err(Error::invalid(
          input.offset as u64 - 2,
          "annotation owner name has 56 or more UTF-16 units",
        ));
      }
      let mut name = Vec::with_capacity(length);
      for _ in 0..length {
        name.push(input.u16()?);
      }
      names.push(name);
    }
    Ok(Self { names })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.names.len() > 0x7fff {
      return Err(Error::Limit("GrpXstAtnOwners count exceeds 0x7fff".into()));
    }
    let mut bytes = Vec::new();
    for name in &self.names {
      if name.len() >= 56 {
        return Err(Error::invalid(
          0,
          "annotation owner name has 56 or more UTF-16 units",
        ));
      }
      push_u16(
        &mut bytes,
        u16::try_from(name.len())
          .map_err(|_| Error::Limit("annotation owner name exceeds u16".into()))?,
      );
      for character in name {
        push_u16(&mut bytes, *character);
      }
    }
    Ok(bytes)
  }
}

impl AnnotationBookmarkInfos {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.is_empty() {
      return Ok(Self {
        present: false,
        extended_marker: 0,
        extra_data_size: 0,
        entries: Vec::new(),
      });
    }
    let mut input = SliceReader::new(bytes);
    let extended_marker = input.u16()?;
    let count = usize::from(input.u16()?);
    let extra_data_size = input.u16()?;
    if extended_marker != 0xffff || extra_data_size != 10 {
      return Err(Error::invalid(
        0,
        "SttbfAtnBkmk header is not UTF-16/10-byte-extra-data",
      ));
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
      let data_length = input.u16()?;
      if data_length != 0 {
        return Err(Error::invalid(
          input.offset as u64 - 2,
          "SttbfAtnBkmk data string is not empty",
        ));
      }
      let entry = AnnotationBookmarkInfo {
        bookmark_class: input.u16()?,
        tag: input.i32()?,
        old_tag: input.i32()?,
      };
      if entry.bookmark_class != 0x0100 || entry.old_tag != -1 {
        return Err(Error::invalid(
          input.offset as u64 - 10,
          "ATNBE reserved values are invalid",
        ));
      }
      entries.push(entry);
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after SttbfAtnBkmk",
      ));
    }
    Ok(Self {
      present: true,
      extended_marker,
      extra_data_size,
      entries,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if !self.present {
      if self.extended_marker != 0 || self.extra_data_size != 0 || !self.entries.is_empty() {
        return Err(Error::invalid(
          0,
          "absent SttbfAtnBkmk contains physical data",
        ));
      }
      return Ok(Vec::new());
    }
    if self.extended_marker != 0xffff || self.extra_data_size != 10 {
      return Err(Error::invalid(0, "SttbfAtnBkmk header changed"));
    }
    let mut bytes = Vec::with_capacity(6 + self.entries.len() * 12);
    push_u16(&mut bytes, self.extended_marker);
    push_u16(
      &mut bytes,
      u16::try_from(self.entries.len())
        .map_err(|_| Error::Limit("SttbfAtnBkmk count exceeds u16".into()))?,
    );
    push_u16(&mut bytes, self.extra_data_size);
    for entry in &self.entries {
      if entry.bookmark_class != 0x0100 || entry.old_tag != -1 {
        return Err(Error::invalid(0, "ATNBE reserved values changed"));
      }
      push_u16(&mut bytes, 0);
      push_u16(&mut bytes, entry.bookmark_class);
      bytes.extend_from_slice(&entry.tag.to_le_bytes());
      bytes.extend_from_slice(&entry.old_tag.to_le_bytes());
    }
    Ok(bytes)
  }
}

impl AnnotationBookmarks {
  pub fn from_bytes(infos: &[u8], starts: &[u8], ends: &[u8]) -> Result<Self> {
    let value = Self {
      infos: AnnotationBookmarkInfos::from_bytes(infos)?,
      starts: BookmarkStartTable::from_bytes(starts)?,
      ends: BookmarkEndTable::from_bytes(ends)?,
    };
    value.validate()?;
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    self.validate()?;
    Ok((
      self.infos.to_bytes()?,
      self.starts.to_bytes()?,
      self.ends.to_bytes()?,
    ))
  }

  fn validate(&self) -> Result<()> {
    let end_count = self.ends.positions.len().saturating_sub(1);
    if self.infos.entries.len() != self.starts.bookmarks.len()
      || self.starts.bookmarks.len() != end_count
    {
      return Err(Error::invalid(
        0,
        "parallel annotation bookmark table cardinality differs",
      ));
    }
    for bookmark in &self.starts.bookmarks {
      if usize::from(bookmark.end_index) >= end_count {
        return Err(Error::invalid(
          0,
          "annotation FBKF.ibkl is outside PlcfAtnBkl",
        ));
      }
    }
    for (index, entry) in self.infos.entries.iter().enumerate() {
      if self.infos.entries[..index]
        .iter()
        .any(|previous| previous.tag == entry.tag)
      {
        return Err(Error::invalid(0, "duplicate ATNBE tag"));
      }
    }
    Ok(())
  }
}

impl TextboxStoryTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(26) {
      return Err(Error::invalid(
        0,
        "textbox story PLC length does not match 22-byte FTXBXS records",
      ));
    }
    let count = (bytes.len() - 4) / 26;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut stories = Vec::with_capacity(count);
    for index in 0..count {
      let first = input.i32()?;
      let second = input.i32()?;
      let reusable_flags = input.u16()?;
      let chain = if index + 1 == count || reusable_flags != 0 {
        TextboxStoryChain::Reusable {
          next_reusable_index: first,
          reusable_count: second,
        }
      } else {
        TextboxStoryChain::NonReusable {
          textbox_count: first,
          edited_textbox_count: second,
        }
      };
      stories.push(TextboxStory {
        chain,
        reusable_flags,
        destination_index: input.u32()?,
        shape_id: input.u32()?,
        undo_transaction_id: input.u32()?,
      });
    }
    require_strictly_increasing(&positions, "textbox story CP")?;
    Ok(Self { positions, stories })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.stories.len().saturating_add(1) {
      return Err(Error::invalid(
        0,
        "textbox story CP/FTXBXS cardinality changed",
      ));
    }
    require_strictly_increasing(&self.positions, "textbox story CP")?;
    let mut bytes = Vec::with_capacity(self.positions.len() * 4 + self.stories.len() * 22);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for (index, story) in self.stories.iter().enumerate() {
      let reusable = index + 1 == self.stories.len() || story.reusable_flags != 0;
      let (first, second) = match (reusable, story.chain) {
        (
          false,
          TextboxStoryChain::NonReusable {
            textbox_count,
            edited_textbox_count,
          },
        ) => (textbox_count, edited_textbox_count),
        (
          true,
          TextboxStoryChain::Reusable {
            next_reusable_index,
            reusable_count,
          },
        ) => (next_reusable_index, reusable_count),
        _ => {
          return Err(Error::invalid(
            0,
            "FTXBXS union does not match reusable state",
          ));
        }
      };
      bytes.extend_from_slice(&first.to_le_bytes());
      bytes.extend_from_slice(&second.to_le_bytes());
      push_u16(&mut bytes, story.reusable_flags);
      push_u32(&mut bytes, story.destination_index);
      push_u32(&mut bytes, story.shape_id);
      push_u32(&mut bytes, story.undo_transaction_id);
    }
    Ok(bytes)
  }
}

impl TextboxBreakTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(10) {
      return Err(Error::invalid(
        0,
        "textbox break PLC length does not match 6-byte Tbkd records",
      ));
    }
    let count = (bytes.len() - 4) / 10;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut breaks = Vec::with_capacity(count);
    for _ in 0..count {
      let story_index = input.i16()?;
      let dependent_character_count = input.u16()?;
      let flags = input.u16()?;
      breaks.push(TextboxBreak {
        story_index,
        dependent_character_count,
        reserved1: flags & 0x03ff,
        mark_delete: flags & 0x0400 != 0,
        unused: flags & 0x0800 != 0,
        text_overflow: flags & 0x1000 != 0,
        reserved2: ((flags >> 13) & 0x07) as u8,
      });
    }
    require_strictly_increasing(&positions, "textbox break CP")?;
    Ok(Self { positions, breaks })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.breaks.len().saturating_add(1) {
      return Err(Error::invalid(
        0,
        "textbox break CP/Tbkd cardinality changed",
      ));
    }
    require_strictly_increasing(&self.positions, "textbox break CP")?;
    let mut bytes = Vec::with_capacity(self.positions.len() * 4 + self.breaks.len() * 6);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for record in &self.breaks {
      if record.reserved1 > 0x03ff || record.reserved2 > 0x07 {
        return Err(Error::invalid(0, "Tbkd bit field exceeds its width"));
      }
      bytes.extend_from_slice(&record.story_index.to_le_bytes());
      push_u16(&mut bytes, record.dependent_character_count);
      push_u16(
        &mut bytes,
        record.reserved1
          | (u16::from(record.mark_delete) << 10)
          | (u16::from(record.unused) << 11)
          | (u16::from(record.text_overflow) << 12)
          | (u16::from(record.reserved2) << 13),
      );
    }
    Ok(bytes)
  }
}

impl ShapeAnchorTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(30) {
      return Err(Error::invalid(
        0,
        "PlcfSpa length does not match 26-byte SPA records",
      ));
    }
    let count = (bytes.len() - 4) / 30;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut anchors = Vec::with_capacity(count);
    for _ in 0..count {
      let shape_id = input.u32()?;
      let rectangle = ShapeAnchorRectangle {
        left: input.i32()?,
        top: input.i32()?,
        right: input.i32()?,
        bottom: input.i32()?,
      };
      let flags = input.u16()?;
      anchors.push(ShapeAnchor {
        shape_id,
        rectangle,
        header: flags & 0x0001 != 0,
        horizontal_origin: ((flags >> 1) & 0x03) as u8,
        vertical_origin: ((flags >> 3) & 0x03) as u8,
        wrap_style: ((flags >> 5) & 0x0f) as u8,
        wrap_side: ((flags >> 9) & 0x0f) as u8,
        simple_rectangle: flags & 0x2000 != 0,
        below_text: flags & 0x4000 != 0,
        anchor_locked: flags & 0x8000 != 0,
        textbox_count: input.i32()?,
      });
    }
    require_strictly_increasing(&positions, "PlcfSpa CP")?;
    Ok(Self { positions, anchors })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.anchors.len().saturating_add(1) {
      return Err(Error::invalid(0, "PlcfSpa CP/SPA cardinality changed"));
    }
    require_strictly_increasing(&self.positions, "PlcfSpa CP")?;
    let mut bytes = Vec::with_capacity(self.positions.len() * 4 + self.anchors.len() * 26);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for anchor in &self.anchors {
      if anchor.horizontal_origin > 0x03
        || anchor.vertical_origin > 0x03
        || anchor.wrap_style > 0x0f
        || anchor.wrap_side > 0x0f
      {
        return Err(Error::invalid(0, "SPA bit field exceeds its width"));
      }
      push_u32(&mut bytes, anchor.shape_id);
      bytes.extend_from_slice(&anchor.rectangle.left.to_le_bytes());
      bytes.extend_from_slice(&anchor.rectangle.top.to_le_bytes());
      bytes.extend_from_slice(&anchor.rectangle.right.to_le_bytes());
      bytes.extend_from_slice(&anchor.rectangle.bottom.to_le_bytes());
      push_u16(
        &mut bytes,
        u16::from(anchor.header)
          | (u16::from(anchor.horizontal_origin) << 1)
          | (u16::from(anchor.vertical_origin) << 3)
          | (u16::from(anchor.wrap_style) << 5)
          | (u16::from(anchor.wrap_side) << 9)
          | (u16::from(anchor.simple_rectangle) << 13)
          | (u16::from(anchor.below_text) << 14)
          | (u16::from(anchor.anchor_locked) << 15),
      );
      bytes.extend_from_slice(&anchor.textbox_count.to_le_bytes());
    }
    Ok(bytes)
  }
}

impl DocOfficeArtContent {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.is_empty() {
      return Err(Error::invalid(0, "OfficeArtContent is empty"));
    }
    let (drawing_group, consumed) = parse_single_office_art_record(bytes, 0)?;
    Self::require_single_container(&drawing_group, 0xf000, "OfficeArtDggContainer")?;
    let mut cursor = consumed;
    let mut drawings = Vec::new();
    while cursor < bytes.len() {
      if drawings.len() == 2 {
        return Err(Error::invalid(
          cursor as u64,
          "OfficeArtContent contains more than two drawings",
        ));
      }
      let label = *bytes
        .get(cursor)
        .ok_or_else(|| Error::invalid(cursor as u64, "missing OfficeArt dgglbl"))?;
      cursor += 1;
      let document_part = match label {
        0 => TextboxDocumentPart::Main,
        1 => TextboxDocumentPart::Header,
        _ => {
          return Err(Error::invalid(
            cursor as u64 - 1,
            format!("OfficeArt dgglbl {label} is not 0 or 1"),
          ));
        }
      };
      let (container, consumed) = parse_single_office_art_record(bytes, cursor)?;
      Self::require_single_container(&container, 0xf002, "OfficeArtDgContainer")?;
      cursor = cursor
        .checked_add(consumed)
        .ok_or_else(|| Error::invalid(cursor as u64, "OfficeArtContent offset overflow"))?;
      drawings.push(OfficeArtWordDrawing {
        document_part,
        container,
      });
    }
    Ok(Self {
      drawing_group,
      drawings,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    Self::require_single_container(&self.drawing_group, 0xf000, "OfficeArtDggContainer")?;
    if self.drawings.len() > 2 {
      return Err(Error::invalid(
        0,
        "OfficeArtContent contains more than two drawings",
      ));
    }
    let mut bytes = self.drawing_group.to_bytes()?;
    for drawing in &self.drawings {
      Self::require_single_container(&drawing.container, 0xf002, "OfficeArtDgContainer")?;
      bytes.push(match drawing.document_part {
        TextboxDocumentPart::Main => 0,
        TextboxDocumentPart::Header => 1,
      });
      bytes.extend_from_slice(&drawing.container.to_bytes()?);
    }
    Ok(bytes)
  }

  /// Aggregates the complete Dgg/Dg record trees into their MS-ODRAW
  /// drawing, shape, and ID-cluster graph. Partial compatibility trees stay
  /// explicit and cannot be mistaken for a complete graph.
  pub fn drawing_graph(&self) -> Result<OfficeArtDrawingGraph> {
    let DocOfficeArtRecordTree::Complete(drawing_group) = &self.drawing_group else {
      return Err(Error::invalid(
        0,
        "OfficeArt drawing graph requires a complete DggContainer",
      ));
    };
    let mut drawings = Vec::with_capacity(self.drawings.len());
    for drawing in &self.drawings {
      let DocOfficeArtRecordTree::Complete(container) = &drawing.container else {
        return Err(Error::invalid(
          0,
          "OfficeArt drawing graph requires complete DgContainer records",
        ));
      };
      drawings.push(container);
    }
    OfficeArtDrawingGraph::from_streams(drawing_group, &drawings)
  }

  /// Resolves a one-based OfficeArt BLIP identifier without copying its
  /// payload. Delayed entries remain explicit because their `foDelay`
  /// offset belongs to the host's WordDocument stream rather than the
  /// record tree.
  pub fn image_link(&self, blip_identifier: u32) -> Result<Option<DocOfficeArtImageLink<'_>>> {
    if blip_identifier == 0 {
      return Ok(None);
    }
    if self.drawing_group.is_partial() {
      return Err(Error::invalid(
        0,
        "OfficeArt image resolution requires a complete DggContainer",
      ));
    }
    let mut store = None;
    let mut duplicate = false;
    let DocOfficeArtRecordTree::Complete(drawing_group) = &self.drawing_group else {
      unreachable!("partial drawing group was rejected above")
    };
    find_doc_blip_store(&drawing_group.records, &mut store, &mut duplicate);
    if duplicate {
      return Err(Error::invalid(
        0,
        "OfficeArt drawing group contains multiple BLIP stores",
      ));
    }
    let Some(store) = store else {
      return Ok(None);
    };
    let index = usize::try_from(blip_identifier - 1)
      .map_err(|_| Error::Limit("OfficeArt BLIP identifier exceeds usize".into()))?;
    let Some(entry) = store.get(index) else {
      return Ok(None);
    };
    let link = match &entry.data {
      OfficeArtRecordData::Fbse(fbse) => match fbse.embedded_blip.as_deref() {
        Some(blip) => blip.image_ref().map_or(
          DocOfficeArtImageLink::Unsupported,
          DocOfficeArtImageLink::Resolved,
        ),
        None => DocOfficeArtImageLink::Delayed {
          word_document_offset: fbse.delay_offset,
        },
      },
      _ => entry.image_ref().map_or(
        DocOfficeArtImageLink::Unsupported,
        DocOfficeArtImageLink::Resolved,
      ),
    };
    Ok(Some(link))
  }

  fn require_single_container(
    tree: &DocOfficeArtRecordTree,
    record_type: u16,
    name: &str,
  ) -> Result<()> {
    let Some(record) = tree.root_record() else {
      return Err(Error::invalid(0, format!("{name} has no root record")));
    };
    if record.header().record_type != record_type || record.header().version != 0x0f {
      return Err(Error::invalid(
        0,
        format!("{name} record framing is invalid"),
      ));
    }
    if let DocOfficeArtRootRecord::Complete(record) = record
      && !matches!(record.data, OfficeArtRecordData::Container(_))
    {
      return Err(Error::invalid(0, format!("{name} is not a container")));
    }
    Ok(())
  }
}

fn find_doc_blip_store<'a>(
  records: &'a [OfficeArtRecord],
  store: &mut Option<&'a [OfficeArtRecord]>,
  duplicate: &mut bool,
) {
  for record in records {
    let children = match &record.data {
      OfficeArtRecordData::Container(children)
      | OfficeArtRecordData::CompatibilityContainer(children) => Some(children.as_slice()),
      _ => None,
    };
    if record.header.record_type == 0xf001
      && let Some(children) = children
      && store.replace(children).is_some()
    {
      *duplicate = true;
    }
    if let Some(children) = children {
      find_doc_blip_store(children, store, duplicate);
    }
  }
}

impl DocOfficeArtRecordTree {
  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    match self {
      Self::Complete(stream) => stream.to_bytes(),
      Self::Partial(stream) => stream.to_bytes(),
    }
  }

  pub fn visit_complete(&self, visitor: impl FnMut(&OfficeArtRecord)) {
    match self {
      Self::Complete(stream) => stream.visit(visitor),
      Self::Partial(stream) => stream.visit_complete(visitor),
    }
  }

  pub fn is_partial(&self) -> bool {
    matches!(self, Self::Partial(_))
  }

  fn root_record(&self) -> Option<DocOfficeArtRootRecord<'_>> {
    match self {
      Self::Complete(stream) => match stream.records.as_slice() {
        [record] => Some(DocOfficeArtRootRecord::Complete(record)),
        _ => None,
      },
      Self::Partial(stream) => {
        if !stream.sequence.trailing_header.is_empty() {
          return None;
        }
        match stream.sequence.records.as_slice() {
          [OfficeArtPartialRecord::Complete(record)] => {
            Some(DocOfficeArtRootRecord::Complete(record))
          }
          [OfficeArtPartialRecord::Incomplete(record)] => {
            Some(DocOfficeArtRootRecord::Incomplete(record))
          }
          _ => None,
        }
      }
    }
  }
}

enum DocOfficeArtRootRecord<'a> {
  Complete(&'a OfficeArtRecord),
  Incomplete(&'a crate::office_art::OfficeArtIncompleteRecord),
}

impl DocOfficeArtRootRecord<'_> {
  fn header(&self) -> crate::office_art::OfficeArtRecordHeader {
    match self {
      Self::Complete(record) => record.header,
      Self::Incomplete(record) => record.header,
    }
  }
}

fn parse_single_office_art_record(
  bytes: &[u8],
  offset: usize,
) -> Result<(DocOfficeArtRecordTree, usize)> {
  let header_end = offset
    .checked_add(8)
    .ok_or_else(|| Error::invalid(offset as u64, "OfficeArt record offset overflow"))?;
  let header = bytes
    .get(offset..header_end)
    .ok_or_else(|| Error::invalid(offset as u64, "truncated OfficeArt record header"))?;
  let declared_length = usize::try_from(u32::from_le_bytes(
    header[4..8]
      .try_into()
      .expect("OfficeArt header length was checked"),
  ))
  .map_err(|_| Error::Limit("OfficeArt record length exceeds usize".into()))?;
  let record_length = 8usize
    .checked_add(declared_length)
    .ok_or_else(|| Error::invalid(offset as u64, "OfficeArt record length overflow"))?;
  let end = offset
    .checked_add(record_length)
    .ok_or_else(|| Error::invalid(offset as u64, "OfficeArt record end overflow"))?;
  let record = bytes
    .get(offset..end)
    .ok_or_else(|| Error::invalid(offset as u64, "truncated OfficeArt record"))?;
  let tree = match OfficeArtStream::from_bytes(record) {
    Ok(mut stream) => {
      type_word_office_art_records(&mut stream.records);
      DocOfficeArtRecordTree::Complete(stream)
    }
    Err(error) => {
      let mut partial = OfficeArtPartialStream::from_bytes_with_limits(
        record,
        Limits::default(),
        error.to_string(),
      )?;
      type_word_partial_office_art_records(&mut partial.sequence);
      DocOfficeArtRecordTree::Partial(partial)
    }
  };
  Ok((tree, record_length))
}

fn type_word_office_art_records(records: &mut [OfficeArtRecord]) {
  for record in records {
    let replacement = match &record.data {
      OfficeArtRecordData::Atom(payload) if payload.len() == 4 => {
        let value = payload
          .as_slice()
          .try_into()
          .expect("Word client payload length was checked");
        match record.header.record_type {
          0xf00d => {
            let raw = u32::from_le_bytes(value);
            Some(OfficeArtRecordData::WordClientTextbox(
              OfficeArtWordClientTextbox {
                story_index: (raw >> 16) as u16,
                chain_index: raw as u16,
              },
            ))
          }
          0xf010 => Some(OfficeArtRecordData::WordClientAnchor(i32::from_le_bytes(
            value,
          ))),
          0xf011 => Some(OfficeArtRecordData::WordClientData(i32::from_le_bytes(
            value,
          ))),
          _ => None,
        }
      }
      _ => None,
    };
    if let Some(replacement) = replacement {
      record.data = replacement;
    }
    match &mut record.data {
      OfficeArtRecordData::Container(children)
      | OfficeArtRecordData::CompatibilityContainer(children) => {
        type_word_office_art_records(children);
      }
      OfficeArtRecordData::Fbse(fbse) => {
        if let Some(blip) = &mut fbse.embedded_blip {
          type_word_office_art_records(std::slice::from_mut(blip));
        }
      }
      _ => {}
    }
  }
}

fn type_word_partial_office_art_records(sequence: &mut OfficeArtPartialSequence) {
  for record in &mut sequence.records {
    match record {
      OfficeArtPartialRecord::Complete(record) => {
        type_word_office_art_records(std::slice::from_mut(record));
      }
      OfficeArtPartialRecord::Incomplete(record) => match &mut record.data {
        OfficeArtIncompleteRecordData::Container(sequence)
        | OfficeArtIncompleteRecordData::RecoveredSequence { sequence, .. } => {
          type_word_partial_office_art_records(sequence);
        }
        _ => {}
      },
    }
  }
}

impl ListNamesTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0xffff {
      return Err(Error::invalid(0, "SttbListNames is not an extended STTB"));
    }
    let count = usize::from(input.u16()?);
    if input.u16()? != 0 {
      return Err(Error::invalid(4, "SttbListNames cbExtra is not zero"));
    }
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
      let length = usize::from(input.u16()?);
      if length > 255 {
        return Err(Error::invalid(
          input.offset.saturating_sub(2) as u64,
          "SttbListNames string exceeds 255 characters",
        ));
      }
      let mut name = Vec::with_capacity(length);
      for _ in 0..length {
        name.push(input.u16()?);
      }
      names.push(name);
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after SttbListNames",
      ));
    }
    Self::validate_names(&names)?;
    Ok(Self { names })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    Self::validate_names(&self.names)?;
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0xffff);
    push_u16(
      &mut bytes,
      u16::try_from(self.names.len())
        .map_err(|_| Error::Limit("SttbListNames count exceeds u16".into()))?,
    );
    push_u16(&mut bytes, 0);
    for name in &self.names {
      push_u16(&mut bytes, name.len() as u16);
      write_u16_array(&mut bytes, name);
    }
    Ok(bytes)
  }

  fn validate_names(names: &[Vec<u16>]) -> Result<()> {
    let mut nonempty = BTreeSet::new();
    for name in names {
      if name.len() > 255 {
        return Err(Error::invalid(
          0,
          "SttbListNames string exceeds 255 characters",
        ));
      }
      if !name.is_empty() && !nonempty.insert(name.as_slice()) {
        return Err(Error::invalid(
          0,
          "SttbListNames contains duplicate nonempty names",
        ));
      }
    }
    Ok(())
  }
}

impl ListDefinitions {
  pub fn from_table_stream(table: &[u8], location: FibFcLcb) -> Result<Self> {
    let start = usize::try_from(location.fc)
      .map_err(|_| Error::Limit("PlfLst offset exceeds usize".into()))?;
    let base_length = usize::try_from(location.lcb)
      .map_err(|_| Error::Limit("PlfLst length exceeds usize".into()))?;
    let declared_end = start
      .checked_add(base_length)
      .ok_or_else(|| Error::invalid(start as u64, "PlfLst end overflow"))?;
    let declared = table
      .get(start..declared_end)
      .ok_or_else(|| Error::invalid(start as u64, "PlfLst exceeds Table Stream"))?;
    let mut count_input = SliceReader::new(declared);
    let count = count_input.i16()?;
    if count < 0 {
      return Err(Error::invalid(0, "PlfLst.cLst is negative"));
    }
    let count = usize::try_from(count).expect("nonnegative i16 fits usize");
    let expected_base_length = 2usize
      .checked_add(
        count
          .checked_mul(28)
          .ok_or_else(|| Error::invalid(0, "PlfLst LSTF array length overflow"))?,
      )
      .ok_or_else(|| Error::invalid(0, "PlfLst base length overflow"))?;
    if declared.len() < expected_base_length {
      return Err(Error::invalid(
        0,
        format!(
          "PlfLst declared length {} is shorter than {count} LSTF records",
          declared.len()
        ),
      ));
    }
    let base_end = start + expected_base_length;
    let mut base_input = SliceReader::new(
      table
        .get(start..base_end)
        .ok_or_else(|| Error::invalid(start as u64, "PlfLst base exceeds Table Stream"))?,
    );
    let parsed_count = base_input.i16()?;
    debug_assert_eq!(parsed_count, count as i16);
    let mut infos = Vec::with_capacity(count);
    for _ in 0..count {
      infos.push(ListDefinitionInfo::read(&mut base_input)?);
    }
    let mut level_input = SliceReader::new(&table[base_end..]);
    let mut definitions = Vec::with_capacity(count);
    for info in infos {
      let level_count = if info.simple { 1 } else { 9 };
      let mut levels = Vec::with_capacity(level_count);
      for _ in 0..level_count {
        levels.push(ListLevel::read(&mut level_input)?);
      }
      definitions.push(ListDefinition { info, levels });
    }
    let level_length = level_input.offset;
    let levels_in_declared_length = match base_length {
      length if length == expected_base_length => false,
      length if length == expected_base_length + level_length => true,
      length => {
        return Err(Error::invalid(
          0,
          format!(
            "PlfLst declared length {length} matches neither base {expected_base_length} nor base+levels {}",
            expected_base_length + level_length
          ),
        ));
      }
    };
    let value = Self {
      levels_in_declared_length,
      definitions,
    };
    value.validate()?;
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<(Vec<u8>, Vec<u8>)> {
    self.validate()?;
    let mut base = Vec::with_capacity(2 + self.definitions.len() * 28);
    base.extend_from_slice(
      &i16::try_from(self.definitions.len())
        .map_err(|_| Error::Limit("PlfLst count exceeds i16".into()))?
        .to_le_bytes(),
    );
    let mut levels = Vec::new();
    for definition in &self.definitions {
      definition.info.write(&mut base)?;
    }
    for definition in &self.definitions {
      for level in &definition.levels {
        level.write(&mut levels)?;
      }
    }
    if self.levels_in_declared_length {
      base.extend_from_slice(&levels);
      levels.clear();
    }
    Ok((base, levels))
  }

  fn validate(&self) -> Result<()> {
    for (index, definition) in self.definitions.iter().enumerate() {
      let expected = if definition.info.simple { 1 } else { 9 };
      if definition.levels.len() != expected {
        return Err(Error::invalid(
          0,
          "LSTF simple flag does not match LVL count",
        ));
      }
      if definition.info.list_id == -1 {
        return Err(Error::invalid(0, "LSTF.lsid is -1"));
      }
      if self.definitions[..index]
        .iter()
        .any(|previous| previous.info.list_id == definition.info.list_id)
      {
        return Err(Error::invalid(0, "duplicate LSTF.lsid"));
      }
    }
    Ok(())
  }
}

impl ListDefinitionInfo {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let list_id = input.i32()?;
    let template_code = input.u32()?;
    let mut paragraph_style_indexes = [0i16; 9];
    for index in &mut paragraph_style_indexes {
      *index = input.i16()?;
    }
    let flags = input.u8()?;
    Ok(Self {
      list_id,
      template_code,
      paragraph_style_indexes,
      simple: flags & 0x01 != 0,
      unused1: flags & 0x02 != 0,
      auto_number: flags & 0x04 != 0,
      unused2: flags & 0x08 != 0,
      hybrid: flags & 0x10 != 0,
      reserved: (flags >> 5) & 0x07,
      html_incompatibilities: input.u8()?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.reserved > 0x07 {
      return Err(Error::invalid(0, "LSTF reserved bits exceed three bits"));
    }
    bytes.extend_from_slice(&self.list_id.to_le_bytes());
    push_u32(bytes, self.template_code);
    for index in self.paragraph_style_indexes {
      bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes.push(
      u8::from(self.simple)
        | (u8::from(self.unused1) << 1)
        | (u8::from(self.auto_number) << 2)
        | (u8::from(self.unused2) << 3)
        | (u8::from(self.hybrid) << 4)
        | (self.reserved << 5),
    );
    bytes.push(self.html_incompatibilities);
    Ok(())
  }
}

impl ListLevel {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let start_at = input.i32()?;
    let number_format = input.u8()?;
    let flags = input.u8()?;
    let mut placeholder_offsets = [0u8; 9];
    for offset in &mut placeholder_offsets {
      *offset = input.u8()?;
    }
    let follow_character = input.u8()?;
    let saved_indent = input.i32()?;
    let unused2 = input.i32()?;
    let number_property_length = usize::from(input.u8()?);
    let paragraph_property_length = usize::from(input.u8()?);
    let restart_limit = input.u8()?;
    let html_incompatibilities = input.u8()?;
    let paragraph_property_bytes = input.bytes(paragraph_property_length)?;
    let (paragraph_properties, paragraph_incomplete_prl_tail) =
      parse_list_level_grpprl(paragraph_property_bytes).map_err(|error| {
        Error::invalid(
          input.offset as u64 - paragraph_property_length as u64,
          format!("LVL paragraph grpprl {paragraph_property_bytes:02x?} is invalid: {error}"),
        )
      })?;
    let number_property_bytes = input.bytes(number_property_length)?;
    let (number_properties, number_incomplete_prl_tail) =
      parse_list_level_grpprl(number_property_bytes).map_err(|error| {
        Error::invalid(
          input.offset as u64 - number_property_length as u64,
          format!("LVL number grpprl {number_property_bytes:02x?} is invalid: {error}"),
        )
      })?;
    let text_length = usize::from(input.u16()?);
    let mut number_text = Vec::with_capacity(text_length);
    for _ in 0..text_length {
      number_text.push(input.u16()?);
    }
    Ok(Self {
      info: ListLevelInfo {
        start_at,
        number_format,
        justification: flags & 0x03,
        legal: flags & 0x04 != 0,
        no_restart: flags & 0x08 != 0,
        indent_saved: flags & 0x10 != 0,
        converted: flags & 0x20 != 0,
        unused1: flags & 0x40 != 0,
        tentative: flags & 0x80 != 0,
        placeholder_offsets,
        follow_character,
        saved_indent,
        unused2,
        restart_limit,
        html_incompatibilities,
      },
      paragraph_properties,
      paragraph_incomplete_prl_tail,
      number_properties,
      number_incomplete_prl_tail,
      number_text,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.info.justification > 0x03 {
      return Err(Error::invalid(0, "LVLF justification exceeds two bits"));
    }
    if self.paragraph_incomplete_prl_tail.len() > 10 || self.number_incomplete_prl_tail.len() > 10 {
      return Err(Error::invalid(
        0,
        "LVL incomplete PRL tail exceeds ten bytes",
      ));
    }
    let mut paragraph_properties = self.paragraph_properties.to_bytes()?;
    paragraph_properties.extend_from_slice(&self.paragraph_incomplete_prl_tail);
    let mut number_properties = self.number_properties.to_bytes()?;
    number_properties.extend_from_slice(&self.number_incomplete_prl_tail);
    bytes.extend_from_slice(&self.info.start_at.to_le_bytes());
    bytes.push(self.info.number_format);
    bytes.push(
      self.info.justification
        | (u8::from(self.info.legal) << 2)
        | (u8::from(self.info.no_restart) << 3)
        | (u8::from(self.info.indent_saved) << 4)
        | (u8::from(self.info.converted) << 5)
        | (u8::from(self.info.unused1) << 6)
        | (u8::from(self.info.tentative) << 7),
    );
    bytes.extend_from_slice(&self.info.placeholder_offsets);
    bytes.push(self.info.follow_character);
    bytes.extend_from_slice(&self.info.saved_indent.to_le_bytes());
    bytes.extend_from_slice(&self.info.unused2.to_le_bytes());
    bytes.push(
      u8::try_from(number_properties.len())
        .map_err(|_| Error::Limit("LVL character grpprl exceeds u8".into()))?,
    );
    bytes.push(
      u8::try_from(paragraph_properties.len())
        .map_err(|_| Error::Limit("LVL paragraph grpprl exceeds u8".into()))?,
    );
    bytes.push(self.info.restart_limit);
    bytes.push(self.info.html_incompatibilities);
    bytes.extend_from_slice(&paragraph_properties);
    bytes.extend_from_slice(&number_properties);
    push_u16(
      bytes,
      u16::try_from(self.number_text.len())
        .map_err(|_| Error::Limit("LVL number text exceeds u16".into()))?,
    );
    for character in &self.number_text {
      push_u16(bytes, *character);
    }
    Ok(())
  }
}

fn parse_list_level_grpprl(bytes: &[u8]) -> Result<(GrpPrl, Vec<u8>)> {
  match GrpPrl::from_bytes(bytes) {
    Ok(properties) => Ok((properties, Vec::new())),
    Err(original_error) => {
      for tail_length in 1..=bytes.len().min(10) {
        let split = bytes.len() - tail_length;
        if let Ok(properties) = GrpPrl::from_bytes(&bytes[..split]) {
          return Ok((properties, bytes[split..].to_vec()));
        }
      }
      Err(original_error)
    }
  }
}

impl ListOverrides {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let count = usize::try_from(input.u32()?)
      .map_err(|_| Error::Limit("PlfLfo count exceeds usize".into()))?;
    if count > 100_000 {
      return Err(Error::Limit("PlfLfo count exceeds 100000".into()));
    }
    let fixed_length = count
      .checked_mul(16)
      .and_then(|length| length.checked_add(4))
      .ok_or_else(|| Error::invalid(0, "PlfLfo LFO array length overflow"))?;
    if fixed_length > bytes.len() {
      return Err(Error::invalid(0, "PlfLfo is truncated inside rgLfo"));
    }
    let mut infos = Vec::with_capacity(count);
    for _ in 0..count {
      let list_id = input.i32()?;
      let unused1 = input.u32()?;
      let unused2 = input.u32()?;
      let level_count = usize::from(input.u8()?);
      infos.push((
        ListOverrideInfo {
          list_id,
          unused1,
          unused2,
          field_type: input.u8()?,
          html_incompatibilities: input.u8()?,
          unused3: input.u8()?,
        },
        level_count,
      ));
    }
    let mut overrides = Vec::with_capacity(count);
    for (info, level_count) in infos {
      let first_paragraph_position = input.u32()?;
      let mut levels = Vec::with_capacity(level_count);
      for _ in 0..level_count {
        levels.push(ListLevelOverride::read(&mut input)?);
      }
      overrides.push(ListOverride {
        info,
        data: ListOverrideData {
          first_paragraph_position,
          levels,
        },
      });
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after PlfLfo",
      ));
    }
    let value = Self { overrides };
    value.validate()?;
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.validate()?;
    let mut bytes = Vec::new();
    push_u32(
      &mut bytes,
      u32::try_from(self.overrides.len())
        .map_err(|_| Error::Limit("PlfLfo count exceeds u32".into()))?,
    );
    for value in &self.overrides {
      bytes.extend_from_slice(&value.info.list_id.to_le_bytes());
      push_u32(&mut bytes, value.info.unused1);
      push_u32(&mut bytes, value.info.unused2);
      bytes.push(
        u8::try_from(value.data.levels.len())
          .map_err(|_| Error::Limit("LFO override count exceeds u8".into()))?,
      );
      bytes.push(value.info.field_type);
      bytes.push(value.info.html_incompatibilities);
      bytes.push(value.info.unused3);
    }
    for value in &self.overrides {
      push_u32(&mut bytes, value.data.first_paragraph_position);
      for level in &value.data.levels {
        level.write(&mut bytes)?;
      }
    }
    Ok(bytes)
  }

  fn validate(&self) -> Result<()> {
    if self.overrides.len() > 100_000 {
      return Err(Error::Limit("PlfLfo count exceeds 100000".into()));
    }
    for value in &self.overrides {
      if value.data.levels.len() > usize::from(u8::MAX) {
        return Err(Error::Limit("LFO override count exceeds u8".into()));
      }
      for (index, level) in value.data.levels.iter().enumerate() {
        if level.level_index > 8 {
          return Err(Error::invalid(0, "LFOLVL.iLvl exceeds 8"));
        }
        if level.overrides_formatting != level.level.is_some() {
          return Err(Error::invalid(
            0,
            "LFOLVL formatting flag does not match nested LVL",
          ));
        }
        if value.data.levels[..index]
          .iter()
          .any(|previous| previous.level_index == level.level_index)
        {
          return Err(Error::invalid(0, "duplicate LFOLVL.iLvl in LFOData"));
        }
      }
    }
    Ok(())
  }
}

impl ListLevelOverride {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let start_at = input.i32()?;
    let flags = input.u32()?;
    let overrides_formatting = flags & 0x20 != 0;
    Ok(Self {
      start_at,
      level_index: (flags & 0x0f) as u8,
      overrides_start: flags & 0x10 != 0,
      overrides_formatting,
      html_incompatibilities: ((flags >> 6) & 0xff) as u8,
      unused1: ((flags >> 14) & 0x7fff) as u16,
      unused2: ((flags >> 29) & 0x07) as u8,
      level: if overrides_formatting {
        Some(ListLevel::read(input)?)
      } else {
        None
      },
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.level_index > 0x0f || self.unused1 > 0x7fff || self.unused2 > 0x07 {
      return Err(Error::invalid(0, "LFOLVL bit field exceeds its width"));
    }
    if self.overrides_formatting != self.level.is_some() {
      return Err(Error::invalid(
        0,
        "LFOLVL formatting flag does not match nested LVL",
      ));
    }
    bytes.extend_from_slice(&self.start_at.to_le_bytes());
    push_u32(
      bytes,
      u32::from(self.level_index)
        | (u32::from(self.overrides_start) << 4)
        | (u32::from(self.overrides_formatting) << 5)
        | (u32::from(self.html_incompatibilities) << 6)
        | (u32::from(self.unused1) << 14)
        | (u32::from(self.unused2) << 29),
    );
    if let Some(level) = &self.level {
      level.write(bytes)?;
    }
    Ok(())
  }
}

impl FontTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let count = usize::from(input.u16()?);
    if count > 0x7ff0 {
      return Err(Error::invalid(0, "SttbfFfn cData exceeds 0x7ff0"));
    }
    let extra_size = input.u16()?;
    if extra_size != 0 {
      return Err(Error::invalid(2, "SttbfFfn cbExtra is not zero"));
    }
    let mut fonts = Vec::with_capacity(count);
    for _ in 0..count {
      fonts.push(FontFamilyName::read(&mut input)?);
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after SttbfFfn",
      ));
    }
    Ok(Self { fonts })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.fonts.len() > 0x7ff0 {
      return Err(Error::invalid(0, "SttbfFfn has too many fonts"));
    }
    let mut bytes = Vec::new();
    push_u16(&mut bytes, self.fonts.len() as u16);
    push_u16(&mut bytes, 0);
    for font in &self.fonts {
      font.write(&mut bytes)?;
    }
    Ok(bytes)
  }
}

impl FontFamilyName {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let record_start = input.offset;
    let length = usize::from(input.u8()?) + 1;
    if length < 42 {
      return Err(Error::invalid(
        record_start as u64,
        "FFN is shorter than its fixed fields and font-name terminator",
      ));
    }
    let payload = input.bytes(length - 1)?;
    let mut record = SliceReader::new(payload);
    let family = FontFamilyIdentifier::from_byte(record.u8()?);
    let weight = record.i16()?;
    let character_set = record.u8()?;
    let alternate_name_index = record.u8()?;
    let panose = Panose::from_bytes(record.take()?);
    let mut unicode_subsets = [0; 4];
    for value in &mut unicode_subsets {
      *value = record.u32()?;
    }
    let mut code_pages = [0; 2];
    for value in &mut code_pages {
      *value = record.u32()?;
    }
    let name_byte_count = payload.len() - record.offset;
    if !name_byte_count.is_multiple_of(2) {
      return Err(Error::invalid(
        (record_start + 1 + record.offset) as u64,
        "FFN Unicode names have an odd byte length",
      ));
    }
    let mut name_units = Vec::with_capacity(name_byte_count / 2);
    while record.offset != payload.len() {
      name_units.push(record.u16()?);
    }
    let Some(primary_end) = name_units.iter().position(|value| *value == 0) else {
      return Err(Error::invalid(
        record_start as u64,
        "FFN font name is not terminated",
      ));
    };
    let logical_end = if alternate_name_index == 0 {
      primary_end + 1
    } else {
      let alternate_start = usize::from(alternate_name_index);
      if alternate_start != primary_end + 1 || alternate_start >= name_units.len() {
        return Err(Error::invalid(
          record_start as u64,
          "FFN alternate-name index does not follow the primary terminator",
        ));
      }
      alternate_start
        + name_units[alternate_start..]
          .iter()
          .position(|value| *value == 0)
          .ok_or_else(|| {
            Error::invalid(record_start as u64, "FFN alternate name is not terminated")
          })?
        + 1
    };
    if name_units[logical_end..].iter().any(|value| *value != 0) {
      return Err(Error::invalid(
        record_start as u64,
        "FFN has nonzero units after its final name terminator",
      ));
    }
    let trailing_name_nulls = u8::try_from(name_units.len() - logical_end)
      .map_err(|_| Error::invalid(record_start as u64, "FFN has excessive null padding"))?;
    name_units.truncate(logical_end);
    let value = Self {
      family,
      weight,
      character_set,
      alternate_name_index,
      panose,
      signature: FontSignature {
        unicode_subsets,
        code_pages,
      },
      name_units,
      trailing_name_nulls,
    };
    value.validate_names(record_start)?;
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate_names(0)?;
    let name_unit_count = self
      .name_units
      .len()
      .checked_add(usize::from(self.trailing_name_nulls))
      .ok_or_else(|| Error::invalid(0, "FFN Unicode-name length overflow"))?;
    let length = 40usize
      .checked_add(
        name_unit_count
          .checked_mul(2)
          .ok_or_else(|| Error::invalid(0, "FFN Unicode-name length overflow"))?,
      )
      .ok_or_else(|| Error::invalid(0, "FFN length overflow"))?;
    let length_minus_one = u8::try_from(length - 1)
      .map_err(|_| Error::invalid(0, "FFN exceeds its one-byte length limit"))?;
    bytes.push(length_minus_one);
    bytes.push(self.family.to_byte());
    bytes.extend_from_slice(&self.weight.to_le_bytes());
    bytes.extend_from_slice(&[self.character_set, self.alternate_name_index]);
    bytes.extend_from_slice(&self.panose.to_bytes());
    for value in self.signature.unicode_subsets {
      push_u32(bytes, value);
    }
    for value in self.signature.code_pages {
      push_u32(bytes, value);
    }
    write_u16_array(bytes, &self.name_units);
    for _ in 0..self.trailing_name_nulls {
      push_u16(bytes, 0);
    }
    Ok(())
  }

  fn validate_names(&self, offset: usize) -> Result<()> {
    let Some(primary_end) = self.name_units.iter().position(|value| *value == 0) else {
      return Err(Error::invalid(
        offset as u64,
        "FFN font name is not terminated",
      ));
    };
    if self.alternate_name_index == 0 {
      if primary_end + 1 != self.name_units.len() {
        return Err(Error::invalid(
          offset as u64,
          "FFN without alternate name has logical units after its terminator",
        ));
      }
    } else {
      let alternate_start = usize::from(self.alternate_name_index);
      if alternate_start != primary_end + 1 || alternate_start >= self.name_units.len() {
        return Err(Error::invalid(
          offset as u64,
          "FFN alternate-name index does not follow the primary terminator",
        ));
      }
      if self.name_units.last() != Some(&0) {
        return Err(Error::invalid(
          offset as u64,
          "FFN alternate name is not terminated",
        ));
      }
      if self.name_units[alternate_start..self.name_units.len() - 1].contains(&0) {
        return Err(Error::invalid(
          offset as u64,
          "FFN has units after its alternate-name terminator",
        ));
      }
    }
    Ok(())
  }

  pub fn primary_name(&self) -> &[u16] {
    let end = self
      .name_units
      .iter()
      .position(|value| *value == 0)
      .unwrap_or(self.name_units.len());
    &self.name_units[..end]
  }

  pub fn alternate_name(&self) -> Option<&[u16]> {
    let start = usize::from(self.alternate_name_index);
    if start == 0 || start >= self.name_units.len() {
      return None;
    }
    let end = self.name_units[start..]
      .iter()
      .position(|value| *value == 0)
      .map(|length| start + length)
      .unwrap_or(self.name_units.len());
    Some(&self.name_units[start..end])
  }
}

impl FontFamilyIdentifier {
  fn from_byte(value: u8) -> Self {
    Self {
      pitch: value & 0x03,
      true_type: value & 0x04 != 0,
      unused1: value & 0x08 != 0,
      family: (value >> 4) & 0x07,
      unused2: value & 0x80 != 0,
    }
  }

  fn to_byte(self) -> u8 {
    (self.pitch & 0x03)
      | (u8::from(self.true_type) << 2)
      | (u8::from(self.unused1) << 3)
      | ((self.family & 0x07) << 4)
      | (u8::from(self.unused2) << 7)
  }
}

impl Panose {
  fn from_bytes(value: [u8; 10]) -> Self {
    Self {
      family_type: value[0],
      serif_style: value[1],
      weight: value[2],
      proportion: value[3],
      contrast: value[4],
      stroke_variation: value[5],
      arm_style: value[6],
      letterform: value[7],
      midline: value[8],
      height: value[9],
    }
  }

  fn to_bytes(self) -> [u8; 10] {
    [
      self.family_type,
      self.serif_style,
      self.weight,
      self.proportion,
      self.contrast,
      self.stroke_variation,
      self.arm_style,
      self.letterform,
      self.midline,
      self.height,
    ]
  }
}

impl AssociatedStrings {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0xffff || input.u16()? != 18 || input.u16()? != 0 {
      return Err(Error::invalid(
        0,
        "SttbfAssoc header is not UTF-16/18-strings/no-extra-data",
      ));
    }
    let mut strings: [Vec<u16>; 18] = std::array::from_fn(|_| Vec::new());
    for (index, string) in strings.iter_mut().enumerate() {
      let length = usize::from(input.u16()?);
      let limit = if index == 17 { 15 } else { 255 };
      if length > limit {
        return Err(Error::invalid(
          input.offset.saturating_sub(2) as u64,
          format!("SttbfAssoc string {index} exceeds {limit} characters"),
        ));
      }
      string.reserve(length);
      for _ in 0..length {
        string.push(input.u16()?);
      }
    }
    let trailing = &bytes[input.offset..];
    if !trailing.len().is_multiple_of(2) || trailing.iter().any(|value| *value != 0) {
      return Err(Error::invalid(
        input.offset as u64,
        "nonzero or odd trailing bytes after SttbfAssoc",
      ));
    }
    let trailing_zero_words = u8::try_from(trailing.len() / 2)
      .map_err(|_| Error::invalid(input.offset as u64, "excessive SttbfAssoc zero padding"))?;
    let [
      unused0,
      template_path,
      title,
      subject,
      keywords,
      unused5,
      author,
      last_revised_by,
      mail_merge_data_source,
      mail_merge_header,
      unused10,
      unused11,
      unused12,
      unused13,
      unused14,
      unused15,
      unused16,
      write_reservation_password,
    ] = strings;
    Ok(Self {
      unused0,
      template_path,
      title,
      subject,
      keywords,
      unused5,
      author,
      last_revised_by,
      mail_merge_data_source,
      mail_merge_header,
      unused10,
      unused11,
      unused12,
      unused13,
      unused14,
      unused15,
      unused16,
      write_reservation_password,
      trailing_zero_words,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0xffff);
    push_u16(&mut bytes, 18);
    push_u16(&mut bytes, 0);
    for (index, string) in self.iter().enumerate() {
      let limit = if index == 17 { 15 } else { 255 };
      if string.len() > limit {
        return Err(Error::invalid(
          0,
          format!("SttbfAssoc string {index} exceeds {limit} characters"),
        ));
      }
      push_u16(&mut bytes, string.len() as u16);
      write_u16_array(&mut bytes, string);
    }
    for _ in 0..self.trailing_zero_words {
      push_u16(&mut bytes, 0);
    }
    Ok(bytes)
  }

  pub fn iter(&self) -> impl ExactSizeIterator<Item = &[u16]> {
    [
      self.unused0.as_slice(),
      self.template_path.as_slice(),
      self.title.as_slice(),
      self.subject.as_slice(),
      self.keywords.as_slice(),
      self.unused5.as_slice(),
      self.author.as_slice(),
      self.last_revised_by.as_slice(),
      self.mail_merge_data_source.as_slice(),
      self.mail_merge_header.as_slice(),
      self.unused10.as_slice(),
      self.unused11.as_slice(),
      self.unused12.as_slice(),
      self.unused13.as_slice(),
      self.unused14.as_slice(),
      self.unused15.as_slice(),
      self.unused16.as_slice(),
      self.write_reservation_password.as_slice(),
    ]
    .into_iter()
  }
}

impl UserVariables {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0xffff {
      return Err(Error::invalid(0, "StwUser SttbNames is not extended"));
    }
    let count = usize::from(input.u16()?);
    if input.u16()? != 4 {
      return Err(Error::invalid(4, "StwUser SttbNames cbExtra is not 4"));
    }

    let mut names = Vec::with_capacity(count);
    let mut unique_names = BTreeSet::new();
    for _ in 0..count {
      let length = usize::from(input.u16()?);
      let mut name = Vec::with_capacity(length);
      for _ in 0..length {
        name.push(input.u16()?);
      }
      if !unique_names.insert(name.clone()) {
        return Err(Error::invalid(0, "StwUser has duplicate variable names"));
      }
      names.push((name, input.u32()?));
    }

    let mut variables = Vec::with_capacity(count);
    for (name, ignored_name_metadata) in names {
      let length = usize::from(input.u16()?);
      let mut value = Vec::with_capacity(length);
      for _ in 0..length {
        value.push(input.u16()?);
      }
      variables.push(UserVariable {
        name,
        ignored_name_metadata,
        value,
      });
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after StwUser values",
      ));
    }
    Ok(Self { variables })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let count = u16::try_from(self.variables.len())
      .map_err(|_| Error::Limit("StwUser variable count exceeds u16".into()))?;
    let mut unique_names = BTreeSet::new();
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0xffff);
    push_u16(&mut bytes, count);
    push_u16(&mut bytes, 4);
    for variable in &self.variables {
      if !unique_names.insert(variable.name.as_slice()) {
        return Err(Error::invalid(0, "StwUser has duplicate variable names"));
      }
      push_u16(
        &mut bytes,
        u16::try_from(variable.name.len())
          .map_err(|_| Error::Limit("StwUser variable name exceeds u16".into()))?,
      );
      write_u16_array(&mut bytes, &variable.name);
      push_u32(&mut bytes, variable.ignored_name_metadata);
    }
    for variable in &self.variables {
      push_u16(
        &mut bytes,
        u16::try_from(variable.value.len())
          .map_err(|_| Error::Limit("StwUser variable value exceeds u16".into()))?,
      );
      write_u16_array(&mut bytes, &variable.value);
    }
    Ok(bytes)
  }
}

impl UserVariable {
  pub fn kind(&self) -> UserVariableKind {
    if utf16_equals_ascii(&self.name, b"Sign") {
      UserVariableKind::LegacyVbaSignature
    } else if utf16_equals_ascii(&self.name, b"SigAgile") {
      UserVariableKind::AgileVbaSignature
    } else if utf16_equals_ascii(&self.name, b"SigV3") {
      UserVariableKind::VbaSignatureV3
    } else {
      UserVariableKind::Ordinary
    }
  }
}

impl UserVariables {
  /// Removes every MS-DOC VBA signature variable while retaining ordinary
  /// user variables and their order.
  pub fn remove_vba_signatures(&mut self) -> usize {
    let before = self.variables.len();
    self
      .variables
      .retain(|variable| variable.kind() == UserVariableKind::Ordinary);
    before - self.variables.len()
  }
}

fn utf16_equals_ascii(value: &[u16], expected: &[u8]) -> bool {
  value.len() == expected.len()
    && value
      .iter()
      .zip(expected)
      .all(|(value, expected)| *value == u16::from(*expected))
}

impl EmbeddedFontTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0 {
      return Err(Error::invalid(0, "SttbW6 unused1 is not zero"));
    }
    let count = input.i16()?;
    if !(0..=64).contains(&count) {
      return Err(Error::invalid(2, "SttbW6 ibstMac is outside 0..=64"));
    }
    if input.i16()? != 64 {
      return Err(Error::invalid(4, "SttbW6 ibstMax is not 64"));
    }
    if input.u16()? != 0 {
      return Err(Error::invalid(6, "SttbW6 unused2 is not zero"));
    }
    let producer_offset = EmbeddedFontTableOffset::from_u16(input.u16()?);
    let expected = usize::try_from(count)
      .expect("nonnegative i16 fits usize")
      .checked_mul(12)
      .and_then(|length| length.checked_add(10))
      .ok_or_else(|| Error::Limit("SttbTtmbd encoded length overflow".into()))?;
    if bytes.len() != expected {
      return Err(Error::invalid(
        0,
        "SttbTtmbd count does not match its bounded length",
      ));
    }

    let mut fonts = Vec::with_capacity(count as usize);
    for _ in 0..count {
      let word_document_offset = input.u32()?;
      if word_document_offset == 0 {
        return Err(Error::invalid(0, "TTMBD fc is zero"));
      }
      let font_index = input.i16()?;
      if font_index < 0 {
        return Err(Error::invalid(0, "TTMBD iiffn is negative"));
      }
      let flags = input.u16()?;
      let subset = match input.u32()? {
        u32::MAX => EmbeddedFontSubset::EntireFont,
        value => EmbeddedFontSubset::UsageOrder(value),
      };
      fonts.push(EmbeddedFontReference {
        word_document_offset,
        font_index,
        bold: flags & 0x0001 != 0,
        italic: flags & 0x0002 != 0,
        ignored_flags: flags >> 2,
        subset,
      });
    }
    Ok(Self {
      producer_offset,
      fonts,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.fonts.len() > 64 {
      return Err(Error::Limit("SttbTtmbd contains more than 64 fonts".into()));
    }
    let mut bytes = Vec::with_capacity(10 + self.fonts.len() * 12);
    push_u16(&mut bytes, 0);
    bytes.extend_from_slice(&(self.fonts.len() as i16).to_le_bytes());
    bytes.extend_from_slice(&64_i16.to_le_bytes());
    push_u16(&mut bytes, 0);
    push_u16(&mut bytes, self.producer_offset.to_u16()?);
    for font in &self.fonts {
      if font.word_document_offset == 0 {
        return Err(Error::invalid(0, "TTMBD fc is zero"));
      }
      if font.font_index < 0 {
        return Err(Error::invalid(0, "TTMBD iiffn is negative"));
      }
      if font.ignored_flags > 0x3fff {
        return Err(Error::invalid(0, "TTMBD ignored flags exceed 14 bits"));
      }
      push_u32(&mut bytes, font.word_document_offset);
      bytes.extend_from_slice(&font.font_index.to_le_bytes());
      push_u16(
        &mut bytes,
        u16::from(font.bold) | (u16::from(font.italic) << 1) | (font.ignored_flags << 2),
      );
      push_u32(
        &mut bytes,
        match font.subset {
          EmbeddedFontSubset::EntireFont => u32::MAX,
          EmbeddedFontSubset::UsageOrder(value) => value,
        },
      );
    }
    Ok(bytes)
  }

  pub fn validate_against_font_table(&self, font_count: usize) -> Result<()> {
    let mut subset_orders = BTreeSet::new();
    for font in &self.fonts {
      if usize::try_from(font.font_index).map_or(true, |index| index >= font_count) {
        return Err(Error::invalid(0, "TTMBD iiffn is outside SttbfFfn"));
      }
      if let EmbeddedFontSubset::UsageOrder(order) = font.subset {
        if usize::try_from(order).map_or(true, |order| order > font_count) {
          return Err(Error::invalid(
            0,
            "TTMBD fcSubset exceeds the font-table count",
          ));
        }
        if !subset_orders.insert(order) {
          return Err(Error::invalid(0, "TTMBD has duplicate fcSubset order"));
        }
      }
    }
    Ok(())
  }
}

impl EmbeddedFontTableOffset {
  fn from_u16(value: u16) -> Self {
    match value {
      10 => Self::Standard,
      26 => Self::Word97Compatibility,
      value => Self::Compatibility(value),
    }
  }

  fn to_u16(self) -> Result<u16> {
    Ok(match self {
      Self::Standard => 10,
      Self::Word97Compatibility => 26,
      Self::Compatibility(value) if !matches!(value, 10 | 26) => value,
      Self::Compatibility(_) => {
        return Err(Error::invalid(
          0,
          "SttbW6 compatibility offset has a canonical variant",
        ));
      }
    })
  }
}

impl RecipientFilter {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let mut items = Vec::new();
    while input.offset < bytes.len() {
      let start = input.offset;
      let byte_length = usize::try_from(input.u32()?)
        .map_err(|_| Error::Limit("FilterDataItem length exceeds usize".into()))?;
      if byte_length < 18
        || !byte_length.is_multiple_of(2)
        || byte_length > 16 + (212 + 1) * 2
        || byte_length > bytes.len().saturating_sub(start)
      {
        return Err(Error::invalid(
          start as u64,
          "FilterDataItem length is invalid",
        ));
      }
      let column = input.u32()?;
      if column > 254 {
        return Err(Error::invalid(0, "FilterDataItem column exceeds 254"));
      }
      let operator = FilterComparison::from_u32(input.u32()?)?;
      let condition = FilterCondition::from_u32(input.u32()?)?;
      let value_units = (byte_length - 16) / 2;
      let mut value = Vec::with_capacity(value_units - 1);
      for _ in 1..value_units {
        value.push(input.u16()?);
      }
      if input.u16()? != 0 {
        return Err(Error::invalid(
          0,
          "FilterDataItem value is not null terminated",
        ));
      }
      if input.offset != start + byte_length {
        return Err(Error::invalid(
          0,
          "FilterDataItem length does not match its data",
        ));
      }
      items.push(RecipientFilterItem {
        column: column as u8,
        operator,
        condition,
        value,
      });
    }
    Ok(Self { items })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for item in &self.items {
      if item.value.len() > 212 {
        return Err(Error::invalid(
          0,
          "FilterDataItem value exceeds 212 characters",
        ));
      }
      let byte_length = 16 + (item.value.len() + 1) * 2;
      push_u32(&mut bytes, byte_length as u32);
      push_u32(&mut bytes, u32::from(item.column));
      push_u32(&mut bytes, item.operator.to_u32());
      push_u32(&mut bytes, item.condition.to_u32());
      write_u16_array(&mut bytes, &item.value);
      push_u16(&mut bytes, 0);
    }
    Ok(bytes)
  }
}

impl FilterComparison {
  fn from_u32(value: u32) -> Result<Self> {
    Ok(match value {
      0 => Self::Equal,
      1 => Self::NotEqual,
      2 => Self::LessThan,
      3 => Self::GreaterThan,
      4 => Self::LessThanOrEqual,
      5 => Self::GreaterThanOrEqual,
      6 => Self::Empty,
      7 => Self::NotEmpty,
      _ => return Err(Error::invalid(0, "FilterDataItem operator is invalid")),
    })
  }

  fn to_u32(self) -> u32 {
    self as u32
  }
}

impl FilterCondition {
  fn from_u32(value: u32) -> Result<Self> {
    Ok(match value {
      0 => Self::And,
      1 => Self::Or,
      _ => return Err(Error::invalid(0, "FilterDataItem condition is invalid")),
    })
  }

  fn to_u32(self) -> u32 {
    self as u32
  }
}

impl RecipientSort {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if !bytes.len().is_multiple_of(8) || bytes.len() > 24 {
      return Err(Error::invalid(
        0,
        "ODSO sort property is not up to three records",
      ));
    }
    let mut input = SliceReader::new(bytes);
    let mut columns = Vec::with_capacity(bytes.len() / 8);
    while input.offset < bytes.len() {
      let column = input.u32()?;
      if column > 254 {
        return Err(Error::invalid(0, "sort column exceeds 254"));
      }
      columns.push(SortColumn {
        column: column as u8,
        direction: SortDirection::from_u32(input.u32()?)?,
      });
    }
    Ok(Self { columns })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.columns.len() > 3 {
      return Err(Error::invalid(
        0,
        "ODSO sort property exceeds three records",
      ));
    }
    let mut bytes = Vec::with_capacity(self.columns.len() * 8);
    for column in &self.columns {
      push_u32(&mut bytes, u32::from(column.column));
      push_u32(&mut bytes, column.direction.to_u32());
    }
    Ok(bytes)
  }
}

impl SortDirection {
  fn from_u32(value: u32) -> Result<Self> {
    Ok(match value {
      0 => Self::Ascending,
      1 => Self::Descending,
      _ => return Err(Error::invalid(0, "sort direction is invalid")),
    })
  }

  fn to_u32(self) -> u32 {
    self as u32
  }
}

impl RecipientInfo {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0 || input.u16()? != 4 {
      return Err(Error::invalid(0, "RecipientInfo count marker is invalid"));
    }
    let count = usize::try_from(input.u32()?)
      .map_err(|_| Error::Limit("RecipientInfo count exceeds usize".into()))?;
    if input.u16()? != 1 {
      return Err(Error::invalid(
        8,
        "RecipientInfo list-size marker is invalid",
      ));
    }
    let standard_length = input.u16()?;
    let list_length = if standard_length == u16::MAX {
      usize::try_from(input.u32()?)
        .map_err(|_| Error::Limit("RecipientInfo list length exceeds usize".into()))?
    } else {
      usize::from(standard_length)
    };
    if list_length != bytes.len().saturating_sub(input.offset) || count > list_length / 4 {
      return Err(Error::invalid(
        0,
        "RecipientInfo list length or count is invalid",
      ));
    }
    if (standard_length == u16::MAX) != (list_length > 0xfffe) {
      return Err(Error::invalid(
        0,
        "RecipientInfo list length uses the wrong framing",
      ));
    }
    let list_end = input.offset + list_length;
    let mut recipients = Vec::with_capacity(count);
    for _ in 0..count {
      let mut items = Vec::new();
      loop {
        let id = input.u16()?;
        let length = usize::from(input.u16()?);
        if id == 0 {
          if length != 0 {
            return Err(Error::invalid(0, "RecipientTerminator length is nonzero"));
          }
          break;
        }
        if input.offset + length > list_end {
          return Err(Error::invalid(
            0,
            "RecipientDataItem exceeds recipient list",
          ));
        }
        let item = match id {
          1 => {
            if length != 4 {
              return Err(Error::invalid(0, "recipient status length is not four"));
            }
            match input.u32()? {
              0 => RecipientData::Included(false),
              1 => RecipientData::Included(true),
              _ => return Err(Error::invalid(0, "recipient status is invalid")),
            }
          }
          2 => {
            if length != 4 {
              return Err(Error::invalid(0, "recipient column length is not four"));
            }
            RecipientData::UniqueColumn(input.u32()?)
          }
          3 => {
            if length != 4 {
              return Err(Error::invalid(0, "recipient hash length is not four"));
            }
            RecipientData::Hash(input.u32()?)
          }
          4 => {
            if !length.is_multiple_of(2) {
              return Err(Error::invalid(0, "recipient unique value has odd length"));
            }
            let mut value = Vec::with_capacity(length / 2);
            for _ in 0..length / 2 {
              value.push(input.u16()?);
            }
            RecipientData::UniqueValue(value)
          }
          _ => return Err(Error::invalid(0, "RecipientDataItem id is invalid")),
        };
        items.push(item);
      }
      let recipient = Recipient { items };
      recipient.validate()?;
      recipients.push(recipient);
    }
    if input.offset != list_end {
      return Err(Error::invalid(0, "trailing bytes in RecipientInfo list"));
    }
    Ok(Self { recipients })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut list = Vec::new();
    for recipient in &self.recipients {
      recipient.validate()?;
      for item in &recipient.items {
        match item {
          RecipientData::Included(value) => {
            push_u16(&mut list, 1);
            push_u16(&mut list, 4);
            push_u32(&mut list, u32::from(*value));
          }
          RecipientData::UniqueColumn(value) => {
            push_u16(&mut list, 2);
            push_u16(&mut list, 4);
            push_u32(&mut list, *value);
          }
          RecipientData::Hash(value) => {
            push_u16(&mut list, 3);
            push_u16(&mut list, 4);
            push_u32(&mut list, *value);
          }
          RecipientData::UniqueValue(value) => {
            let byte_length = value
              .len()
              .checked_mul(2)
              .and_then(|length| u16::try_from(length).ok())
              .ok_or_else(|| Error::Limit("recipient unique value exceeds u16 bytes".into()))?;
            push_u16(&mut list, 4);
            push_u16(&mut list, byte_length);
            write_u16_array(&mut list, value);
          }
        }
      }
      push_u16(&mut list, 0);
      push_u16(&mut list, 0);
    }
    let list_length = u32::try_from(list.len())
      .map_err(|_| Error::Limit("RecipientInfo list exceeds u32".into()))?;
    let count = u32::try_from(self.recipients.len())
      .map_err(|_| Error::Limit("RecipientInfo count exceeds u32".into()))?;
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0);
    push_u16(&mut bytes, 4);
    push_u32(&mut bytes, count);
    push_u16(&mut bytes, 1);
    if list.len() > 0xfffe {
      push_u16(&mut bytes, u16::MAX);
      push_u32(&mut bytes, list_length);
    } else {
      push_u16(&mut bytes, list.len() as u16);
    }
    bytes.extend_from_slice(&list);
    Ok(bytes)
  }
}

impl Recipient {
  fn validate(&self) -> Result<()> {
    let has_hash = self
      .items
      .iter()
      .any(|item| matches!(item, RecipientData::Hash(_)));
    let has_column = self
      .items
      .iter()
      .any(|item| matches!(item, RecipientData::UniqueColumn(_)));
    let has_value = self
      .items
      .iter()
      .any(|item| matches!(item, RecipientData::UniqueValue(_)));
    if !(has_hash || has_column && has_value) {
      return Err(Error::invalid(
        0,
        "recipient lacks hash or column/value identity",
      ));
    }
    Ok(())
  }
}

impl FieldMapInfo {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0 || input.u16()? != 4 || input.u32()? != 30 {
      return Err(Error::invalid(0, "FieldMapInfo count header is invalid"));
    }
    if input.u16()? != 1 {
      return Err(Error::invalid(
        8,
        "FieldMapInfo list-size marker is invalid",
      ));
    }
    let standard_length = input.u16()?;
    let list_length = if standard_length == u16::MAX {
      usize::try_from(input.u32()?)
        .map_err(|_| Error::Limit("FieldMapInfo list length exceeds usize".into()))?
    } else {
      usize::from(standard_length)
    };
    if list_length != bytes.len().saturating_sub(input.offset) || list_length < 30 * 4 {
      return Err(Error::invalid(0, "FieldMapInfo list length is invalid"));
    }
    if (standard_length == u16::MAX) != (list_length > 0xfffe) {
      return Err(Error::invalid(
        0,
        "FieldMapInfo list uses the wrong length framing",
      ));
    }
    let list_end = input.offset + list_length;
    let mut fields = Vec::with_capacity(30);
    for _ in 0..30 {
      let mut items = Vec::new();
      loop {
        let id = input.u16()?;
        let length = usize::from(input.u16()?);
        if id == 0 {
          if length != 0 {
            return Err(Error::invalid(0, "FieldMapTerminator length is nonzero"));
          }
          break;
        }
        if input.offset + length > list_end {
          return Err(Error::invalid(0, "FieldMapDataItem exceeds field-map list"));
        }
        items.push(match id {
          1 => {
            if length != 4 || input.u32()? != 1 {
              return Err(Error::invalid(0, "field mapped marker is invalid"));
            }
            FieldMapData::Mapped
          }
          2 | 3 => {
            if !length.is_multiple_of(2) {
              return Err(Error::invalid(0, "field-map name has odd byte length"));
            }
            let mut value = Vec::with_capacity(length / 2);
            for _ in 0..length / 2 {
              value.push(input.u16()?);
            }
            if id == 2 {
              FieldMapData::DataSourceColumnName(value)
            } else {
              FieldMapData::StandardFieldName(value)
            }
          }
          4 => {
            if length != 4 {
              return Err(Error::invalid(
                0,
                "field-map column index length is not four",
              ));
            }
            match input.u32()? {
              u32::MAX => FieldMapData::ColumnIndex(None),
              value => FieldMapData::ColumnIndex(Some(value)),
            }
          }
          _ => return Err(Error::invalid(0, "FieldMapDataItem id is invalid")),
        });
      }
      fields.push(FieldMap { items });
    }
    if input.offset != list_end {
      return Err(Error::invalid(0, "trailing bytes in FieldMapInfo list"));
    }
    Ok(Self { fields })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.fields.len() != 30 {
      return Err(Error::invalid(0, "FieldMapInfo does not contain 30 fields"));
    }
    let mut list = Vec::new();
    for field in &self.fields {
      for item in &field.items {
        match item {
          FieldMapData::Mapped => {
            push_u16(&mut list, 1);
            push_u16(&mut list, 4);
            push_u32(&mut list, 1);
          }
          FieldMapData::DataSourceColumnName(value) | FieldMapData::StandardFieldName(value) => {
            let byte_length = value
              .len()
              .checked_mul(2)
              .and_then(|length| u16::try_from(length).ok())
              .ok_or_else(|| Error::Limit("field-map name exceeds u16 bytes".into()))?;
            push_u16(
              &mut list,
              if matches!(item, FieldMapData::DataSourceColumnName(_)) {
                2
              } else {
                3
              },
            );
            push_u16(&mut list, byte_length);
            write_u16_array(&mut list, value);
          }
          FieldMapData::ColumnIndex(value) => {
            push_u16(&mut list, 4);
            push_u16(&mut list, 4);
            push_u32(&mut list, value.unwrap_or(u32::MAX));
          }
        }
      }
      push_u16(&mut list, 0);
      push_u16(&mut list, 0);
    }
    let list_length = u32::try_from(list.len())
      .map_err(|_| Error::Limit("FieldMapInfo list exceeds u32".into()))?;
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0);
    push_u16(&mut bytes, 4);
    push_u32(&mut bytes, 30);
    push_u16(&mut bytes, 1);
    if list.len() > 0xfffe {
      push_u16(&mut bytes, u16::MAX);
      push_u32(&mut bytes, list_length);
    } else {
      push_u16(&mut bytes, list.len() as u16);
    }
    bytes.extend_from_slice(&list);
    Ok(bytes)
  }
}

impl OfficeDataSource {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.is_empty() {
      return Err(Error::invalid(0, "ODSO property array is empty"));
    }
    let mut input = SliceReader::new(bytes);
    let mut properties = Vec::new();
    while input.offset < bytes.len() {
      let id = input.u16()?;
      let standard_length = input.u16()?;
      let length = if standard_length == u16::MAX {
        usize::try_from(input.u32()?)
          .map_err(|_| Error::Limit("ODSO property length exceeds usize".into()))?
      } else {
        usize::from(standard_length)
      };
      if (standard_length == u16::MAX) != (length > 0xfffe) {
        return Err(Error::invalid(
          0,
          "ODSO property uses the wrong length framing",
        ));
      }
      let payload = input.bytes(length)?;
      properties.push(OfficeDataSourceProperty::from_payload(id, payload)?);
    }
    let value = Self { properties };
    value.validate()?;
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.validate()?;
    let mut bytes = Vec::new();
    for property in &self.properties {
      let payload = property.to_payload()?;
      push_u16(&mut bytes, property.id());
      if payload.len() > 0xfffe {
        push_u16(&mut bytes, u16::MAX);
        push_u32(
          &mut bytes,
          u32::try_from(payload.len())
            .map_err(|_| Error::Limit("ODSO property exceeds u32".into()))?,
        );
      } else {
        push_u16(&mut bytes, payload.len() as u16);
      }
      bytes.extend_from_slice(&payload);
    }
    Ok(bytes)
  }

  pub fn validate_mail_merge_state(&self, state: &MailMergeState) -> Result<()> {
    let odso_connection = self.properties.iter().find_map(|property| match property {
      OfficeDataSourceProperty::ConnectionString(value) => Some(value),
      _ => None,
    });
    let pms_connection = state.strings.as_ref().map(|strings| &strings.connection);
    if let (Some(odso), Some(pms)) = (odso_connection, pms_connection)
      && !odso.is_empty()
      && !pms.is_empty()
      && odso != pms
    {
      return Err(Error::invalid(
        0,
        "ODSO and Pms mail-merge connection strings differ",
      ));
    }
    Ok(())
  }

  fn validate(&self) -> Result<()> {
    if self.properties.is_empty() {
      return Err(Error::invalid(0, "ODSO property array is empty"));
    }
    let mut ids = BTreeSet::new();
    for property in &self.properties {
      if !ids.insert(property.id()) {
        return Err(Error::invalid(0, "ODSO contains a duplicate property id"));
      }
      property.validate()?;
    }
    Ok(())
  }
}

impl OfficeDataSourceProperty {
  fn from_payload(id: u16, payload: &[u8]) -> Result<Self> {
    Ok(match id {
      0 => Self::ConnectionString(read_odso_utf16(payload, "connection string")?),
      1 => Self::DataSet(read_odso_utf16(payload, "data set")?),
      2 => Self::FileName(read_odso_utf16(payload, "file name")?),
      0x10 => {
        if payload.len() != 4 {
          return Err(Error::invalid(0, "ODSO connection type length is not four"));
        }
        let value = u32::from_le_bytes(payload.try_into().expect("length checked"));
        if value > 7 {
          return Err(Error::invalid(0, "ODSO connection type exceeds seven"));
        }
        Self::ConnectionType(value as u8)
      }
      0x11 => {
        if payload.len() != 2 {
          return Err(Error::invalid(0, "ODSO delimiter length is not two"));
        }
        Self::ColumnDelimiter(u16::from_le_bytes(
          payload.try_into().expect("length checked"),
        ))
      }
      0x12 => {
        if payload.len() != 4 {
          return Err(Error::invalid(0, "ODSO header-row length is not four"));
        }
        match u32::from_le_bytes(payload.try_into().expect("length checked")) {
          0 => Self::FirstRowIsHeader(false),
          1 => Self::FirstRowIsHeader(true),
          _ => return Err(Error::invalid(0, "ODSO header-row flag is invalid")),
        }
      }
      0x13 => Self::Filter(RecipientFilter::from_bytes(payload)?),
      0x14 => Self::Sort(RecipientSort::from_bytes(payload)?),
      0x15 => Self::Recipients(RecipientInfo::from_bytes(payload)?),
      0x16 => Self::FieldMap(FieldMapInfo::from_bytes(payload)?),
      0x17 => {
        if payload.len() != 2 {
          return Err(Error::invalid(0, "ODSO wizard-step length is not two"));
        }
        let value = u16::from_le_bytes(payload.try_into().expect("length checked"));
        if !(1..=6).contains(&value) {
          return Err(Error::invalid(0, "ODSO wizard step is outside 1..=6"));
        }
        Self::WizardStep(value as u8)
      }
      _ => return Err(Error::invalid(0, "ODSO property id is invalid")),
    })
  }

  fn to_payload(&self) -> Result<Vec<u8>> {
    self.validate()?;
    Ok(match self {
      Self::ConnectionString(value) | Self::DataSet(value) | Self::FileName(value) => {
        let mut bytes = Vec::with_capacity(value.len() * 2);
        write_u16_array(&mut bytes, value);
        bytes
      }
      Self::ConnectionType(value) => u32::from(*value).to_le_bytes().to_vec(),
      Self::ColumnDelimiter(value) => value.to_le_bytes().to_vec(),
      Self::FirstRowIsHeader(value) => u32::from(*value).to_le_bytes().to_vec(),
      Self::Filter(value) => value.to_bytes()?,
      Self::Sort(value) => value.to_bytes()?,
      Self::Recipients(value) => value.to_bytes()?,
      Self::FieldMap(value) => value.to_bytes()?,
      Self::WizardStep(value) => u16::from(*value).to_le_bytes().to_vec(),
    })
  }

  fn id(&self) -> u16 {
    match self {
      Self::ConnectionString(_) => 0,
      Self::DataSet(_) => 1,
      Self::FileName(_) => 2,
      Self::ConnectionType(_) => 0x10,
      Self::ColumnDelimiter(_) => 0x11,
      Self::FirstRowIsHeader(_) => 0x12,
      Self::Filter(_) => 0x13,
      Self::Sort(_) => 0x14,
      Self::Recipients(_) => 0x15,
      Self::FieldMap(_) => 0x16,
      Self::WizardStep(_) => 0x17,
    }
  }

  fn validate(&self) -> Result<()> {
    match self {
      Self::ConnectionType(value) if *value > 7 => {
        Err(Error::invalid(0, "ODSO connection type exceeds seven"))
      }
      Self::WizardStep(value) if !(1..=6).contains(value) => {
        Err(Error::invalid(0, "ODSO wizard step is outside 1..=6"))
      }
      Self::Filter(value) => value.to_bytes().map(|_| ()),
      Self::Sort(value) => value.to_bytes().map(|_| ()),
      Self::Recipients(value) => value.to_bytes().map(|_| ()),
      Self::FieldMap(value) => value.to_bytes().map(|_| ()),
      _ => Ok(()),
    }
  }
}

fn read_odso_utf16(bytes: &[u8], name: &str) -> Result<Vec<u16>> {
  if !bytes.len().is_multiple_of(2) {
    return Err(Error::invalid(
      0,
      format!("ODSO {name} has odd byte length"),
    ));
  }
  let mut input = SliceReader::new(bytes);
  let mut value = Vec::with_capacity(bytes.len() / 2);
  while input.offset < bytes.len() {
    value.push(input.u16()?);
  }
  Ok(value)
}

impl MailMergeState {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let status = MailMergeStatus::from_u16(input.u16()?)?;
    let header_source_index = input.u8()?;
    let fetch_source_index = input.u8()?;
    let current_record = match input.u32()? {
      u32::MAX => None,
      value if value <= 0xffff_fff0 => Some(value),
      _ => return Err(Error::invalid(4, "Pms iRecCur is invalid")),
    };
    let sources = [
      MailMergeSource::read(&mut input)?,
      MailMergeSource::read(&mut input)?,
    ];
    let filter = MailMergeFilter::from_u32(input.u32()?)?;
    let sql_byte_length = usize::from(input.u16()?);
    let sql_query = if sql_byte_length == 0 {
      None
    } else {
      if sql_byte_length <= 2 || sql_byte_length > 512 || !sql_byte_length.is_multiple_of(2) {
        return Err(Error::invalid(28, "Pms SQL string byte length is invalid"));
      }
      let mut value = Vec::with_capacity(sql_byte_length / 2 - 1);
      for _ in 0..sql_byte_length / 2 {
        value.push(input.u16()?);
      }
      if value.pop() != Some(0) {
        return Err(Error::invalid(30, "Pms SQL string is not null terminated"));
      }
      Some(value)
    };
    let strings = if filter.string_table_handle == 0 {
      None
    } else {
      Some(MailMergeStrings::read(&mut input)?)
    };
    let document_type = match bytes.len() - input.offset {
      0 => None,
      4 => Some(MailMergeDocumentTypeInfo::from_u32(input.u32()?)?),
      _ => {
        return Err(Error::invalid(
          input.offset as u64,
          "Pms has a partial or oversized Wpmsdt",
        ));
      }
    };
    let state = Self {
      status,
      header_source_index,
      fetch_source_index,
      current_record,
      sources,
      filter,
      sql_query,
      strings,
      document_type,
    };
    state.validate()?;
    Ok(state)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.validate()?;
    let mut bytes = Vec::new();
    push_u16(&mut bytes, self.status.to_u16()?);
    bytes.push(self.header_source_index);
    bytes.push(self.fetch_source_index);
    push_u32(&mut bytes, self.current_record.unwrap_or(u32::MAX));
    for source in self.sources {
      source.write(&mut bytes)?;
    }
    push_u32(&mut bytes, self.filter.to_u32()?);
    match &self.sql_query {
      None => push_u16(&mut bytes, 0),
      Some(value) => {
        let byte_length = value
          .len()
          .checked_add(1)
          .and_then(|length| length.checked_mul(2))
          .ok_or_else(|| Error::Limit("Pms SQL string length overflow".into()))?;
        if !(4..=512).contains(&byte_length) {
          return Err(Error::invalid(0, "Pms SQL string byte length is invalid"));
        }
        push_u16(&mut bytes, byte_length as u16);
        write_u16_array(&mut bytes, value);
        push_u16(&mut bytes, 0);
      }
    }
    if let Some(strings) = &self.strings {
      strings.write(&mut bytes)?;
    }
    if let Some(document_type) = self.document_type {
      push_u32(&mut bytes, document_type.to_u32()?);
    }
    Ok(bytes)
  }

  fn validate(&self) -> Result<()> {
    if self.header_source_index > 1 || self.fetch_source_index > 1 {
      return Err(Error::invalid(2, "Pms source index is not 0 or 1"));
    }
    if self.current_record.is_some_and(|value| value > 0xffff_fff0) {
      return Err(Error::invalid(4, "Pms iRecCur is invalid"));
    }
    for source in self.sources {
      source.validate()?;
    }
    if (self.filter.string_table_handle == 0) != self.strings.is_none() {
      return Err(Error::invalid(
        0,
        "Pms Rfs handle disagrees with SttbfRfs presence",
      ));
    }
    Ok(())
  }

  pub fn validate_file_references(&self, files: &ExternalFileNameTable) -> Result<()> {
    files.validate()?;
    for source in self.sources {
      if let MailMergeFileReference::Identifier(identifier) = source.file
        && !files.contains(ExternalFileType::MailMergeDataSource, identifier)
      {
        return Err(Error::invalid(0, "Pms FNPI is absent from SttbFnm"));
      }
    }
    Ok(())
  }
}

impl MailMergeStatus {
  fn from_u16(value: u16) -> Result<Self> {
    if value & 0x1200 != 0 {
      return Err(Error::invalid(0, "Wpms has nonzero required-zero bits"));
    }
    Ok(Self {
      main_document_selected: value & 1 != 0,
      data_source_selected: value & 2 != 0,
      header_source_selected: value & 4 != 0,
      document_type: MailMergeDocumentType::from_wpms((value >> 3) & 0x0f)?,
      ignored1: value & 0x0080 != 0,
      automatic: value & 0x0100 != 0,
      suppress_blank_lines: value & 0x0400 != 0,
      record_selection: value & 0x0800 != 0,
      destination: MailMergeDestination::from_u16((value >> 13) & 7)?,
    })
  }

  fn to_u16(self) -> Result<u16> {
    Ok(
      u16::from(self.main_document_selected)
        | (u16::from(self.data_source_selected) << 1)
        | (u16::from(self.header_source_selected) << 2)
        | (self.document_type.to_wpms()? << 3)
        | (u16::from(self.ignored1) << 7)
        | (u16::from(self.automatic) << 8)
        | (u16::from(self.suppress_blank_lines) << 10)
        | (u16::from(self.record_selection) << 11)
        | (self.destination.to_u16() << 13),
    )
  }
}

impl MailMergeDocumentType {
  fn from_wpms(value: u16) -> Result<Self> {
    Ok(match value {
      0 => Self::None,
      1 => Self::Letters,
      2 => Self::Labels,
      4 => Self::Envelopes,
      8 => Self::Catalog,
      _ => return Err(Error::invalid(0, "Wpms document type is invalid")),
    })
  }

  fn to_wpms(self) -> Result<u16> {
    Ok(match self {
      Self::None => 0,
      Self::Letters => 1,
      Self::Labels => 2,
      Self::Envelopes => 4,
      Self::Catalog => 8,
      Self::Email | Self::Fax => {
        return Err(Error::invalid(
          0,
          "Wpms cannot encode email or fax document type",
        ));
      }
    })
  }

  fn from_wpmsdt(value: u32) -> Result<Self> {
    Ok(match value {
      0 => Self::None,
      1 => Self::Letters,
      2 => Self::Labels,
      4 => Self::Envelopes,
      8 => Self::Catalog,
      16 => Self::Email,
      32 => Self::Fax,
      _ => return Err(Error::invalid(0, "Wpmsdt document type is invalid")),
    })
  }

  fn to_wpmsdt(self) -> u32 {
    match self {
      Self::None => 0,
      Self::Letters => 1,
      Self::Labels => 2,
      Self::Envelopes => 4,
      Self::Catalog => 8,
      Self::Email => 16,
      Self::Fax => 32,
    }
  }
}

impl MailMergeDestination {
  fn from_u16(value: u16) -> Result<Self> {
    Ok(match value {
      0 => Self::None,
      1 => Self::Printer,
      2 => Self::Email,
      4 => Self::Fax,
      _ => return Err(Error::invalid(0, "Wpms destination is invalid")),
    })
  }

  fn to_u16(self) -> u16 {
    match self {
      Self::None => 0,
      Self::Printer => 1,
      Self::Email => 2,
      Self::Fax => 4,
    }
  }
}

impl MailMergeSource {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let kind = MailMergeSourceKind::from_u8(input.u8()?)?;
    let flags = input.u8()?;
    let field_raw = input.i16()?;
    let record_raw = input.i16()?;
    let file_raw = input.u16()?;
    let source = Self {
      kind,
      link_to_filename: flags & 1 != 0,
      link_to_connection: flags & 2 != 0,
      no_prompt_query_tool: flags & 4 != 0,
      query: flags & 8 != 0,
      ignored_flags: flags >> 4,
      field_separator: if kind == MailMergeSourceKind::DataFile {
        MailMergeSeparator::Token(MailMergeToken::from_i16(field_raw)?)
      } else {
        MailMergeSeparator::Ignored(field_raw)
      },
      record_separator: if kind == MailMergeSourceKind::DataFile {
        MailMergeSeparator::Token(MailMergeToken::from_i16(record_raw)?)
      } else {
        MailMergeSeparator::Ignored(record_raw)
      },
      file: MailMergeFileReference::from_u16(file_raw)?,
    };
    source.validate()?;
    Ok(source)
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    bytes.push(self.kind.to_u8());
    bytes.push(
      u8::from(self.link_to_filename)
        | (u8::from(self.link_to_connection) << 1)
        | (u8::from(self.no_prompt_query_tool) << 2)
        | (u8::from(self.query) << 3)
        | (self.ignored_flags << 4),
    );
    bytes.extend_from_slice(&self.field_separator.to_i16()?.to_le_bytes());
    bytes.extend_from_slice(&self.record_separator.to_i16()?.to_le_bytes());
    push_u16(bytes, self.file.to_u16()?);
    Ok(())
  }

  fn validate(self) -> Result<()> {
    if self.ignored_flags > 0x0f {
      return Err(Error::invalid(0, "Pmfs ignored flags exceed 4 bits"));
    }
    if self.kind == MailMergeSourceKind::DataFile {
      let (MailMergeSeparator::Token(field), MailMergeSeparator::Token(record)) =
        (self.field_separator, self.record_separator)
      else {
        return Err(Error::invalid(0, "data-file Pmfs has ignored separators"));
      };
      let compatibility_nil = field == MailMergeToken::None
        && record == MailMergeToken::None
        && self.file == MailMergeFileReference::NilCompatibility
        && !self.link_to_filename
        && !self.link_to_connection
        && !self.no_prompt_query_tool
        && !self.query
        && self.ignored_flags == 0;
      if !compatibility_nil
        && (record == MailMergeToken::None
          || record == field
          || self.file == MailMergeFileReference::NilCompatibility)
      {
        return Err(Error::invalid(
          0,
          "data-file Pmfs separators or FNPI are invalid",
        ));
      }
    } else if !matches!(self.field_separator, MailMergeSeparator::Ignored(_))
      || !matches!(self.record_separator, MailMergeSeparator::Ignored(_))
    {
      return Err(Error::invalid(
        0,
        "non-data-file Pmfs has interpreted separators",
      ));
    }
    Ok(())
  }
}

impl MailMergeSourceKind {
  fn from_u8(value: u8) -> Result<Self> {
    Ok(match value {
      0xff => Self::None,
      0 => Self::DataFile,
      1 => Self::Access,
      2 => Self::Excel,
      3 => Self::MicrosoftQuery,
      4 => Self::Odbc,
      5 => Self::OfficeDataSourceObject,
      _ => return Err(Error::invalid(0, "Pmfs data-source kind is invalid")),
    })
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::None => 0xff,
      Self::DataFile => 0,
      Self::Access => 1,
      Self::Excel => 2,
      Self::MicrosoftQuery => 3,
      Self::Odbc => 4,
      Self::OfficeDataSourceObject => 5,
    }
  }
}

impl MailMergeSeparator {
  fn to_i16(self) -> Result<i16> {
    match self {
      Self::Token(value) => value.to_i16(),
      Self::Ignored(value) => Ok(value),
    }
  }
}

impl MailMergeToken {
  fn from_i16(value: i16) -> Result<Self> {
    Ok(match value {
      0 => Self::None,
      2 => Self::Enter,
      6 => Self::Tab,
      0x0a..=0x1f | 0x21..=0x27 => Self::Character(value as u16),
      0x46 => Self::FieldEnd,
      0x47 => Self::TableCell,
      0x48 => Self::TableRow,
      _ => return Err(Error::invalid(0, "Pmfs separator token is invalid")),
    })
  }

  fn to_i16(self) -> Result<i16> {
    Ok(match self {
      Self::None => 0,
      Self::Enter => 2,
      Self::Tab => 6,
      Self::Character(value) if matches!(value, 0x0a..=0x1f | 0x21..=0x27) => value as i16,
      Self::Character(_) => {
        return Err(Error::invalid(0, "Pmfs separator character is invalid"));
      }
      Self::FieldEnd => 0x46,
      Self::TableCell => 0x47,
      Self::TableRow => 0x48,
    })
  }
}

impl MailMergeFileReference {
  fn from_u16(value: u16) -> Result<Self> {
    if value & 0x000f != 3 {
      return Err(Error::invalid(0, "Pmfs FNPI type is not mail merge"));
    }
    Ok(match value >> 4 {
      0x0fff => Self::NilCompatibility,
      identifier => Self::Identifier(identifier),
    })
  }

  fn to_u16(self) -> Result<u16> {
    Ok(match self {
      Self::Identifier(value) if value < 0x0fff => (value << 4) | 3,
      Self::Identifier(_) => return Err(Error::invalid(0, "Pmfs FNPI id is 0xfff")),
      Self::NilCompatibility => 0xfff3,
    })
  }
}

impl MailMergeFilter {
  fn from_u32(value: u32) -> Result<Self> {
    let error_handling = match (value >> 1) & 3 {
      0 => MailMergeErrorHandling::SimulateAndReport,
      1 => MailMergeErrorHandling::CompleteAndPause,
      2 => MailMergeErrorHandling::CompleteAndReport,
      _ => return Err(Error::invalid(0, "Rfs error-handling value is invalid")),
    };
    Ok(Self {
      show_data: value & 1 != 0,
      error_handling,
      main_document_setup: value & 8 != 0,
      mail_as_text: value & 0x10 != 0,
      ignored1: value & 0x20 != 0,
      default_sql: value & 0x40 != 0,
      mail_as_html: value & 0x80 != 0,
      ignored2: ((value >> 8) & 0xff) as u8,
      string_table_handle: (value >> 16) as u16,
    })
  }

  fn to_u32(self) -> Result<u32> {
    let error = match self.error_handling {
      MailMergeErrorHandling::SimulateAndReport => 0,
      MailMergeErrorHandling::CompleteAndPause => 1,
      MailMergeErrorHandling::CompleteAndReport => 2,
    };
    Ok(
      u32::from(self.show_data)
        | (error << 1)
        | (u32::from(self.main_document_setup) << 3)
        | (u32::from(self.mail_as_text) << 4)
        | (u32::from(self.ignored1) << 5)
        | (u32::from(self.default_sql) << 6)
        | (u32::from(self.mail_as_html) << 7)
        | (u32::from(self.ignored2) << 8)
        | (u32::from(self.string_table_handle) << 16),
    )
  }
}

impl MailMergeStrings {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    if input.u16()? != 0xffff {
      return Err(Error::invalid(0, "SttbfRfs is not extended"));
    }
    let count = input.u16()?;
    if !matches!(count, 4 | 5) || input.u16()? != 0 {
      return Err(Error::invalid(0, "SttbfRfs count or cbExtra is invalid"));
    }
    let mut values = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
      let length = usize::from(input.u16()?);
      if length >= 256 {
        return Err(Error::invalid(0, "SttbfRfs string has 256 characters"));
      }
      let mut value = Vec::with_capacity(length);
      for _ in 0..length {
        value.push(input.u16()?);
      }
      values.push(value);
    }
    let connection = values.remove(0);
    let header_connection = values.remove(0);
    let subject = values.remove(0);
    let recipient_column = values.remove(0);
    let ignored = values.pop();
    Ok(Self {
      connection,
      header_connection,
      subject,
      recipient_column,
      ignored,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    push_u16(bytes, 0xffff);
    push_u16(bytes, if self.ignored.is_some() { 5 } else { 4 });
    push_u16(bytes, 0);
    for value in [
      Some(&self.connection),
      Some(&self.header_connection),
      Some(&self.subject),
      Some(&self.recipient_column),
      self.ignored.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
      if value.len() >= 256 {
        return Err(Error::invalid(0, "SttbfRfs string has 256 characters"));
      }
      push_u16(bytes, value.len() as u16);
      write_u16_array(bytes, value);
    }
    Ok(())
  }
}

impl MailMergeDocumentTypeInfo {
  fn from_u32(value: u32) -> Result<Self> {
    Ok(Self {
      document_type: MailMergeDocumentType::from_wpmsdt(value & 0x3f)?,
      ignored: value >> 6,
    })
  }

  fn to_u32(self) -> Result<u32> {
    if self.ignored > 0x03ff_ffff {
      return Err(Error::invalid(0, "Wpmsdt ignored field exceeds 26 bits"));
    }
    Ok(self.document_type.to_wpmsdt() | (self.ignored << 6))
  }
}

impl ExternalFileNameTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0xffff {
      return Err(Error::invalid(0, "SttbFnm is not extended"));
    }
    let count = usize::from(input.u16()?);
    if input.u16()? != 8 || count > bytes.len().saturating_sub(6) / 10 {
      return Err(Error::invalid(0, "SttbFnm header or count is invalid"));
    }
    let mut files = Vec::with_capacity(count);
    for _ in 0..count {
      let length = usize::from(input.u16()?);
      let mut path = Vec::with_capacity(length);
      for _ in 0..length {
        path.push(input.u16()?);
      }
      let fnpi = input.u16()?;
      files.push(ExternalFileName {
        path,
        file_type: ExternalFileType::from_fnpt(fnpi & 0x000f)?,
        identifier: fnpi >> 4,
        relative_path: match input.u8()? {
          0xff => ExternalRelativePath::None,
          offset => ExternalRelativePath::Offset(offset),
        },
        file_systems: ExternalFileSystems::from_u8(input.u8()?)?,
        ignored: input.u32()?,
      });
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(0, "trailing bytes after SttbFnm"));
    }
    let table = Self { files };
    table.validate()?;
    Ok(table)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.validate()?;
    let count = u16::try_from(self.files.len())
      .map_err(|_| Error::Limit("SttbFnm count exceeds u16".into()))?;
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0xffff);
    push_u16(&mut bytes, count);
    push_u16(&mut bytes, 8);
    for file in &self.files {
      push_u16(
        &mut bytes,
        u16::try_from(file.path.len())
          .map_err(|_| Error::Limit("SttbFnm path exceeds u16".into()))?,
      );
      write_u16_array(&mut bytes, &file.path);
      push_u16(
        &mut bytes,
        (file.identifier << 4) | file.file_type.to_fnpt(),
      );
      bytes.push(match file.relative_path {
        ExternalRelativePath::None => 0xff,
        ExternalRelativePath::Offset(offset) => offset,
      });
      bytes.push(file.file_systems.to_u8()?);
      push_u32(&mut bytes, file.ignored);
    }
    Ok(bytes)
  }

  fn validate(&self) -> Result<()> {
    let mut identifiers = BTreeSet::new();
    for file in &self.files {
      file.validate()?;
      if !identifiers.insert((file.file_type, file.identifier)) {
        return Err(Error::invalid(0, "SttbFnm contains duplicate FNPI values"));
      }
    }
    Ok(())
  }

  fn contains(&self, file_type: ExternalFileType, identifier: u16) -> bool {
    self
      .files
      .iter()
      .any(|file| file.file_type == file_type && file.identifier == identifier)
  }
}

impl ExternalFileName {
  fn validate(&self) -> Result<()> {
    if self.path.len() > usize::from(u16::MAX) {
      return Err(Error::Limit("SttbFnm path exceeds u16".into()));
    }
    if self.identifier >= 0x0fff {
      return Err(Error::invalid(0, "SttbFnm FNPI id is 0xfff"));
    }
    if let ExternalRelativePath::Offset(offset) = self.relative_path
      && usize::from(offset) >= self.path.len()
    {
      return Err(Error::invalid(
        0,
        "SttbFnm relative path offset is outside the file name",
      ));
    }
    self.file_systems.validate()
  }
}

impl ExternalFileType {
  fn from_fnpt(value: u16) -> Result<Self> {
    Ok(match value {
      3 => Self::MailMergeDataSource,
      5 => Self::Subdocument,
      _ => return Err(Error::invalid(0, "SttbFnm FNPI type is invalid")),
    })
  }

  fn to_fnpt(self) -> u16 {
    match self {
      Self::MailMergeDataSource => 3,
      Self::Subdocument => 5,
    }
  }
}

impl ExternalFileSystems {
  fn from_u8(value: u8) -> Result<Self> {
    let systems = Self {
      fat: value & 0x01 != 0,
      ignored1: value & 0x02 != 0,
      ignored2: value & 0x04 != 0,
      ntfs: value & 0x08 != 0,
      non_file_system: value & 0x10 != 0,
      ignored3: (value >> 5) & 0x03,
      ignored4: value & 0x80 != 0,
    };
    systems.validate()?;
    Ok(systems)
  }

  fn to_u8(self) -> Result<u8> {
    self.validate()?;
    Ok(
      u8::from(self.fat)
        | (u8::from(self.ignored1) << 1)
        | (u8::from(self.ignored2) << 2)
        | (u8::from(self.ntfs) << 3)
        | (u8::from(self.non_file_system) << 4)
        | (self.ignored3 << 5)
        | (u8::from(self.ignored4) << 7),
    )
  }

  fn validate(self) -> Result<()> {
    if self.ignored3 > 3 {
      return Err(Error::invalid(0, "FNFB ignored3 exceeds two bits"));
    }
    if self.non_file_system && (self.fat || self.ntfs) {
      return Err(Error::invalid(
        0,
        "non-file-system FNFB also selects FAT or NTFS",
      ));
    }
    Ok(())
  }
}

impl XmlSchemaReferences {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let count = input.i32()?;
    if count < 0 {
      return Err(Error::invalid(0, "Hplxsdr cXSDR is negative"));
    }
    let count =
      usize::try_from(count).map_err(|_| Error::Limit("Hplxsdr count exceeds usize".into()))?;
    if count > bytes.len().saturating_sub(4) / 18 {
      return Err(Error::invalid(
        0,
        "Hplxsdr count exceeds its bounded payload",
      ));
    }
    let mut schemas = Vec::with_capacity(count);
    for _ in 0..count {
      schemas.push(XmlSchemaReference::read(&mut input)?);
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(0, "trailing bytes after Hplxsdr"));
    }
    Ok(Self { schemas })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let count = i32::try_from(self.schemas.len())
      .map_err(|_| Error::Limit("Hplxsdr count exceeds i32".into()))?;
    let mut bytes = Vec::new();
    push_i32(&mut bytes, count);
    for schema in &self.schemas {
      schema.write(&mut bytes)?;
    }
    Ok(bytes)
  }
}

impl XmlTransformPath {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.is_empty() || bytes.len() > 4168 || !bytes.len().is_multiple_of(2) {
      return Err(Error::invalid(
        0,
        "CustomXForm length is zero, odd, or exceeds 4168 bytes",
      ));
    }
    let mut input = SliceReader::new(bytes);
    let mut path = Vec::with_capacity(bytes.len() / 2);
    while input.offset < bytes.len() {
      path.push(input.u16()?);
    }
    Ok(Self { path })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let byte_length = self
      .path
      .len()
      .checked_mul(2)
      .ok_or_else(|| Error::Limit("CustomXForm length overflow".into()))?;
    if byte_length == 0 || byte_length > 4168 {
      return Err(Error::invalid(
        0,
        "CustomXForm length is zero or exceeds 4168 bytes",
      ));
    }
    let mut bytes = Vec::with_capacity(byte_length);
    write_u16_array(&mut bytes, &self.path);
    Ok(bytes)
  }
}

impl StructuredTagBookmarks {
  pub fn from_bytes(tags: &[u8], starts: &[u8], ends: &[u8]) -> Result<Self> {
    let value = Self {
      tags: read_structured_tag_infos(tags)?,
      starts: BookmarkStartTable::from_bytes(starts)?,
      ends: BookmarkEndTable::from_bytes(ends)?,
    };
    value.validate()?;
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<StructuredTagBookmarkBytes> {
    self.validate()?;
    Ok(StructuredTagBookmarkBytes {
      tags: write_structured_tag_infos(&self.tags)?,
      starts: self.starts.to_bytes()?,
      ends: self.ends.to_bytes()?,
    })
  }

  pub fn validate_schema_references(&self, schemas: &XmlSchemaReferences) -> Result<()> {
    for tag in &self.tags {
      validate_tag_name(&tag.name, schemas, false)?;
      for attribute in &tag.attributes {
        validate_tag_name(&attribute.name, schemas, true)?;
      }
    }
    Ok(())
  }

  fn validate(&self) -> Result<()> {
    let count = self.tags.len();
    if count == 0 || count > i32::MAX as usize {
      return Err(Error::invalid(
        0,
        "structured-tag bookmark count is invalid",
      ));
    }
    if self.starts.bookmarks.len() != count
      || self.starts.positions.len() != count + 1
      || self.ends.positions.len() != count + 1
    {
      return Err(Error::invalid(
        0,
        "parallel structured-tag bookmark cardinality differs",
      ));
    }
    require_nondecreasing(&self.starts.positions, "PlcfBkfSdt CP")?;
    require_nondecreasing(&self.ends.positions, "PlcfBklSdt CP")?;
    let mut ids = BTreeSet::new();
    for (tag, bookmark) in self.tags.iter().zip(&self.starts.bookmarks) {
      if tag.id == 0 || !ids.insert(tag.id) {
        return Err(Error::invalid(0, "SDTI id is zero or duplicate"));
      }
      tag.name.validate()?;
      for attribute in &tag.attributes {
        attribute.name.validate()?;
      }
      if usize::from(bookmark.end_index) >= count {
        return Err(Error::invalid(0, "SDT FBKF index is outside FBKL"));
      }
    }
    Ok(())
  }
}

impl TagQualifiedName {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let value = Self {
      schema_index: input.u32()?,
      name_index: input.u32()?,
    };
    value.validate()?;
    Ok(value)
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    push_u32(bytes, self.schema_index);
    push_u32(bytes, self.name_index);
    Ok(())
  }

  fn validate(self) -> Result<()> {
    if self.schema_index >= 0x7fff_ffff || self.name_index >= 0x7fff_ffff {
      return Err(Error::invalid(0, "TIQ index is not below 0x7fffffff"));
    }
    Ok(())
  }
}

impl StructuredTagType {
  fn from_u32(value: u32) -> Result<Self> {
    Ok(match value {
      1 => Self::Characters,
      2 => Self::Paragraphs,
      3 => Self::TableCells,
      4 => Self::TableRows,
      _ => return Err(Error::invalid(0, "SDTT is unknown or invalid")),
    })
  }

  fn to_u32(self) -> u32 {
    match self {
      Self::Characters => 1,
      Self::Paragraphs => 2,
      Self::TableCells => 3,
      Self::TableRows => 4,
    }
  }
}

fn read_structured_tag_infos(bytes: &[u8]) -> Result<Vec<StructuredTagInfo>> {
  let mut input = SliceReader::new(bytes);
  if input.u16()? != 0xffff {
    return Err(Error::invalid(0, "SttbfBkmkSdt is not extended"));
  }
  let count = input.i32()?;
  if count <= 0 || input.u16()? != 0 {
    return Err(Error::invalid(0, "SttbfBkmkSdt header or count is invalid"));
  }
  let count = count as usize;
  if count > bytes.len().saturating_sub(8) / 28 {
    return Err(Error::invalid(0, "SttbfBkmkSdt count exceeds its payload"));
  }
  let mut tags = Vec::with_capacity(count);
  for _ in 0..count {
    if input.u16()? != 12 {
      return Err(Error::invalid(0, "SttbfBkmkSdt cchData is not 12"));
    }
    let id = input.u32()?;
    let name = TagQualifiedName::read(&mut input)?;
    let tag_type = StructuredTagType::from_u32(input.u32()?)?;
    let attribute_count = usize::try_from(input.u32()?)
      .map_err(|_| Error::Limit("SDTI attribute count exceeds usize".into()))?;
    let placeholder_bytes = usize::try_from(input.u32()?)
      .map_err(|_| Error::Limit("SDTI placeholder length exceeds usize".into()))?;
    if placeholder_bytes < 2 || !placeholder_bytes.is_multiple_of(2) {
      return Err(Error::invalid(0, "SDTI placeholder byte length is invalid"));
    }
    let available = input.bytes.len().saturating_sub(input.offset);
    if placeholder_bytes > available || attribute_count > (available - placeholder_bytes) / 12 {
      return Err(Error::invalid(0, "SDTI variable data exceeds its payload"));
    }
    let mut attributes = Vec::with_capacity(attribute_count);
    for _ in 0..attribute_count {
      let name = TagQualifiedName::read(&mut input)?;
      let length = usize::from(input.u16()?);
      let mut value = Vec::with_capacity(length);
      for _ in 0..length {
        value.push(input.u16()?);
      }
      if input.u16()? != 0 {
        return Err(Error::invalid(0, "FSDAP value is not null terminated"));
      }
      attributes.push(StructuredTagAttribute { name, value });
    }
    let placeholder_units = placeholder_bytes / 2;
    let mut placeholder = Vec::with_capacity(placeholder_units - 1);
    for _ in 1..placeholder_units {
      placeholder.push(input.u16()?);
    }
    if input.u16()? != 0 {
      return Err(Error::invalid(0, "SDTI placeholder is not null terminated"));
    }
    tags.push(StructuredTagInfo {
      id,
      name,
      tag_type,
      attributes,
      placeholder,
    });
  }
  if input.offset != bytes.len() {
    return Err(Error::invalid(0, "trailing bytes after SttbfBkmkSdt"));
  }
  Ok(tags)
}

fn write_structured_tag_infos(tags: &[StructuredTagInfo]) -> Result<Vec<u8>> {
  if tags.is_empty() || tags.len() > i32::MAX as usize {
    return Err(Error::invalid(
      0,
      "structured-tag bookmark count is invalid",
    ));
  }
  let mut bytes = Vec::new();
  push_u16(&mut bytes, 0xffff);
  push_i32(&mut bytes, tags.len() as i32);
  push_u16(&mut bytes, 0);
  for tag in tags {
    push_u16(&mut bytes, 12);
    push_u32(&mut bytes, tag.id);
    tag.name.write(&mut bytes)?;
    push_u32(&mut bytes, tag.tag_type.to_u32());
    push_u32(
      &mut bytes,
      u32::try_from(tag.attributes.len())
        .map_err(|_| Error::Limit("SDTI attribute count exceeds u32".into()))?,
    );
    let placeholder_bytes = tag
      .placeholder
      .len()
      .checked_add(1)
      .and_then(|length| length.checked_mul(2))
      .and_then(|length| u32::try_from(length).ok())
      .ok_or_else(|| Error::Limit("SDTI placeholder length exceeds u32".into()))?;
    push_u32(&mut bytes, placeholder_bytes);
    for attribute in &tag.attributes {
      attribute.name.write(&mut bytes)?;
      push_u16(
        &mut bytes,
        u16::try_from(attribute.value.len())
          .map_err(|_| Error::Limit("FSDAP value exceeds u16".into()))?,
      );
      write_u16_array(&mut bytes, &attribute.value);
      push_u16(&mut bytes, 0);
    }
    write_u16_array(&mut bytes, &tag.placeholder);
    push_u16(&mut bytes, 0);
  }
  Ok(bytes)
}

fn validate_tag_name(
  name: &TagQualifiedName,
  schemas: &XmlSchemaReferences,
  attribute: bool,
) -> Result<()> {
  let schema = schemas
    .schemas
    .get(name.schema_index as usize)
    .ok_or_else(|| Error::invalid(0, "TIQ schema index is outside Hplxsdr"))?;
  let table = if attribute {
    &schema.elements
  } else {
    &schema.attributes
  };
  let count = match table {
    XmlSchemaStringTable::Ansi(values) => values.len(),
    XmlSchemaStringTable::Utf16(values) => values.len(),
  };
  if name.name_index as usize >= count {
    return Err(Error::invalid(
      0,
      "TIQ name index is outside its schema STTB",
    ));
  }
  Ok(())
}

impl FormatConsistencyBookmarks {
  pub fn from_bytes(metadata: &[u8], starts: &[u8], ends: &[u8]) -> Result<Self> {
    let value = Self {
      records: read_format_consistency_records(metadata)?,
      starts: BookmarkStartTable::from_bytes(starts)?,
      ends: BookmarkEndTable::from_bytes(ends)?,
    };
    value.validate()?;
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<BookmarkSetBytes> {
    self.validate()?;
    Ok(BookmarkSetBytes {
      metadata: write_format_consistency_records(&self.records)?,
      starts: self.starts.to_bytes()?,
      ends: self.ends.to_bytes()?,
    })
  }

  pub fn validate_main_document(&self, character_count: u32) -> Result<()> {
    validate_bookmark_positions(&self.starts, &self.ends, character_count, "FCC")?;
    if self.records.iter().any(|record| {
      record.squiggle
        || !record.ignored
        || !record.squiggle_changed
        || !matches!(
          record.kind,
          FormatConsistencyKind::CharacterFormatting
            | FormatConsistencyKind::ParagraphFormatting
            | FormatConsistencyKind::ListLevelFormatting
        )
    }) {
      return Err(Error::invalid(
        0,
        "main-document DPCID constraints are violated",
      ));
    }
    Ok(())
  }

  fn validate(&self) -> Result<()> {
    validate_bookmark_cardinality(self.records.len(), &self.starts, &self.ends, "FCC")?;
    let mut ids = BTreeSet::new();
    if self.records.iter().any(|record| !ids.insert(record.id)) {
      return Err(Error::invalid(
        0,
        "SttbfBkmkFcc contains duplicate DPCID ids",
      ));
    }
    Ok(())
  }
}

impl FormatConsistencyKind {
  fn from_u32(value: u32) -> Result<Self> {
    Ok(match value {
      0 => Self::CharacterFormatting,
      1 => Self::MatchingCharacterStyle,
      2 => Self::ParagraphFormatting,
      3 => Self::MatchingParagraphStyle,
      4 => Self::ListLevelFormatting,
      5 => Self::MatchingListStyle,
      6 => Self::MatchingTableStyle,
      7 => Self::RevisedCharacters,
      8 => Self::RevisedParagraphs,
      9 => Self::RevisedTables,
      10 => Self::RevisedSection,
      11 => Self::DuplicateInlineImage,
      _ => return Err(Error::invalid(0, "DPCID IDPCI is invalid")),
    })
  }

  fn to_u32(self) -> u32 {
    self as u32
  }
}

impl FormatConsistencyProperties {
  fn from_u8(value: u8) -> Result<Self> {
    if value & 0xf2 != 0 {
      return Err(Error::invalid(0, "DPCID FCCT reserved bits are nonzero"));
    }
    Ok(Self {
      character: value & 1 != 0,
      table: value & 4 != 0,
      line_separation: value & 8 != 0,
    })
  }

  fn to_u8(self) -> u8 {
    u8::from(self.character) | (u8::from(self.table) << 2) | (u8::from(self.line_separation) << 3)
  }
}

impl RepairBookmarks {
  pub fn from_bytes(metadata: &[u8], starts: &[u8], ends: &[u8]) -> Result<Self> {
    let value = Self {
      descriptions: read_repair_descriptions(metadata)?,
      starts: BookmarkStartTable::from_bytes(starts)?,
      ends: BookmarkEndTable::from_bytes(ends)?,
    };
    value.validate()?;
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<BookmarkSetBytes> {
    self.validate()?;
    Ok(BookmarkSetBytes {
      metadata: write_repair_descriptions(&self.descriptions)?,
      starts: self.starts.to_bytes()?,
      ends: self.ends.to_bytes()?,
    })
  }

  pub fn validate_main_document(&self, character_count: u32) -> Result<()> {
    validate_bookmark_positions(&self.starts, &self.ends, character_count, "repair")
  }

  fn validate(&self) -> Result<()> {
    if self.descriptions.len() > 0x7ff0 {
      return Err(Error::invalid(0, "repair bookmark count exceeds 0x7ff0"));
    }
    validate_bookmark_cardinality(self.descriptions.len(), &self.starts, &self.ends, "repair")
  }
}

fn read_format_consistency_records(bytes: &[u8]) -> Result<Vec<FormatConsistencyBookmark>> {
  let mut input = SliceReader::new(bytes);
  if input.u16()? != 0xffff {
    return Err(Error::invalid(0, "SttbfBkmkFcc is not extended"));
  }
  let count = usize::from(input.u16()?);
  if count > 0x7ff0 || input.u16()? != 0 || count > bytes.len().saturating_sub(6) / 22 {
    return Err(Error::invalid(0, "SttbfBkmkFcc header or count is invalid"));
  }
  let mut records = Vec::with_capacity(count);
  for _ in 0..count {
    if input.u16()? != 10 {
      return Err(Error::invalid(0, "SttbfBkmkFcc cchData is not 10"));
    }
    let padding1 = input.u16()?;
    let flags = input.u32()?;
    if flags & !7 != 0 {
      return Err(Error::invalid(0, "DPCID unused flags are nonzero"));
    }
    records.push(FormatConsistencyBookmark {
      padding1,
      squiggle: flags & 1 != 0,
      ignored: flags & 2 != 0,
      squiggle_changed: flags & 4 != 0,
      kind: FormatConsistencyKind::from_u32(input.u32()?)?,
      ignored_data: input.u32()?,
      properties: FormatConsistencyProperties::from_u8(input.u8()?)?,
      id: input.u32()?,
      padding2: input.u8()?,
    });
  }
  if input.offset != bytes.len() {
    return Err(Error::invalid(0, "trailing bytes after SttbfBkmkFcc"));
  }
  Ok(records)
}

fn write_format_consistency_records(records: &[FormatConsistencyBookmark]) -> Result<Vec<u8>> {
  if records.len() > 0x7ff0 {
    return Err(Error::invalid(0, "FCC bookmark count exceeds 0x7ff0"));
  }
  let mut bytes = Vec::with_capacity(6 + records.len() * 22);
  push_u16(&mut bytes, 0xffff);
  push_u16(&mut bytes, records.len() as u16);
  push_u16(&mut bytes, 0);
  for record in records {
    push_u16(&mut bytes, 10);
    push_u16(&mut bytes, record.padding1);
    push_u32(
      &mut bytes,
      u32::from(record.squiggle)
        | (u32::from(record.ignored) << 1)
        | (u32::from(record.squiggle_changed) << 2),
    );
    push_u32(&mut bytes, record.kind.to_u32());
    push_u32(&mut bytes, record.ignored_data);
    bytes.push(record.properties.to_u8());
    push_u32(&mut bytes, record.id);
    bytes.push(record.padding2);
  }
  Ok(bytes)
}

fn read_repair_descriptions(bytes: &[u8]) -> Result<Vec<Vec<u16>>> {
  let mut input = SliceReader::new(bytes);
  if input.u16()? != 0xffff {
    return Err(Error::invalid(0, "SttbfBkmkBPRepairs is not extended"));
  }
  let count = usize::from(input.u16()?);
  if count > 0x7ff0 || input.u16()? != 0 || count > bytes.len().saturating_sub(6) / 2 {
    return Err(Error::invalid(
      0,
      "SttbfBkmkBPRepairs header or count is invalid",
    ));
  }
  let mut descriptions = Vec::with_capacity(count);
  for _ in 0..count {
    let length = usize::from(input.u16()?);
    let mut description = Vec::with_capacity(length);
    for _ in 0..length {
      description.push(input.u16()?);
    }
    descriptions.push(description);
  }
  if input.offset != bytes.len() {
    return Err(Error::invalid(0, "trailing bytes after SttbfBkmkBPRepairs"));
  }
  Ok(descriptions)
}

fn write_repair_descriptions(descriptions: &[Vec<u16>]) -> Result<Vec<u8>> {
  if descriptions.len() > 0x7ff0 {
    return Err(Error::invalid(0, "repair bookmark count exceeds 0x7ff0"));
  }
  let mut bytes = Vec::new();
  push_u16(&mut bytes, 0xffff);
  push_u16(&mut bytes, descriptions.len() as u16);
  push_u16(&mut bytes, 0);
  for description in descriptions {
    push_u16(
      &mut bytes,
      u16::try_from(description.len())
        .map_err(|_| Error::Limit("repair description exceeds u16".into()))?,
    );
    write_u16_array(&mut bytes, description);
  }
  Ok(bytes)
}

fn validate_bookmark_cardinality(
  count: usize,
  starts: &BookmarkStartTable,
  ends: &BookmarkEndTable,
  name: &str,
) -> Result<()> {
  if starts.bookmarks.len() != count
    || starts.positions.len() != count + 1
    || ends.positions.len() != count + 1
  {
    return Err(Error::invalid(
      0,
      format!("parallel {name} bookmark cardinality differs"),
    ));
  }
  require_nondecreasing(&starts.positions, &format!("{name} start CP"))?;
  require_nondecreasing(&ends.positions, &format!("{name} end CP"))?;
  if starts
    .bookmarks
    .iter()
    .any(|bookmark| usize::from(bookmark.end_index) >= count)
  {
    return Err(Error::invalid(
      0,
      format!("{name} FBKF index is outside FBKL"),
    ));
  }
  Ok(())
}

fn validate_bookmark_positions(
  starts: &BookmarkStartTable,
  ends: &BookmarkEndTable,
  character_count: u32,
  name: &str,
) -> Result<()> {
  if starts
    .positions
    .iter()
    .chain(&ends.positions)
    .any(|position| *position > character_count)
  {
    return Err(Error::invalid(
      0,
      format!("{name} bookmark CP exceeds main document"),
    ));
  }
  Ok(())
}

impl RangeProtection {
  pub fn from_bytes(permissions: &[u8], starts: &[u8], ends: &[u8], users: &[u8]) -> Result<Self> {
    let value = Self {
      permissions: read_range_permissions(permissions)?,
      starts: BookmarkStartTable::from_bytes(starts)?,
      ends: BookmarkEndTable::from_bytes(ends)?,
      users: ProtectedUsers::from_bytes(users)?,
    };
    value.validate()?;
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<RangeProtectionBytes> {
    self.validate()?;
    Ok(RangeProtectionBytes {
      permissions: write_range_permissions(&self.permissions)?,
      starts: self.starts.to_bytes()?,
      ends: self.ends.to_bytes()?,
      users: self.users.to_bytes()?,
    })
  }

  fn validate(&self) -> Result<()> {
    let count = self.permissions.len();
    if count == 0 || count > 0x7ff0 {
      return Err(Error::invalid(
        0,
        "range permission count is outside 1..=0x7ff0",
      ));
    }
    if self.starts.bookmarks.len() != count
      || self.starts.positions.len() != count + 1
      || self.ends.positions.len() != count + 1
    {
      return Err(Error::invalid(
        0,
        "parallel range-protection table cardinality differs",
      ));
    }
    require_nondecreasing(&self.starts.positions, "PlcfBkfProt CP")?;
    require_nondecreasing(&self.ends.positions, "PlcfBklProt CP")?;
    for bookmark in &self.starts.bookmarks {
      if usize::from(bookmark.end_index) >= count {
        return Err(Error::invalid(
          0,
          "range-protection FBKF index is outside FBKL",
        ));
      }
    }
    self.users.validate()?;
    for permission in &self.permissions {
      if let PermittedEditors::UserIndex(index) = permission.editors
        && (index == 0 || usize::from(index) > self.users.users.len())
      {
        return Err(Error::invalid(0, "PRTI user index is outside SttbProtUser"));
      }
    }
    Ok(())
  }
}

impl PermittedEditors {
  fn from_i16(value: i16) -> Result<Self> {
    Ok(match value {
      1..=i16::MAX => Self::UserIndex(value as u16),
      -5 => Self::Editors,
      -4 => Self::Owners,
      -1 => Self::Everyone,
      _ => return Err(Error::invalid(0, "PRTI UidSel is invalid")),
    })
  }

  fn to_i16(self) -> Result<i16> {
    Ok(match self {
      Self::UserIndex(value) if (1..=i16::MAX as u16).contains(&value) => value as i16,
      Self::UserIndex(_) => return Err(Error::invalid(0, "PRTI user index exceeds i16")),
      Self::Editors => -5,
      Self::Owners => -4,
      Self::Everyone => -1,
    })
  }
}

impl ProtectedUsers {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.is_empty() {
      return Ok(Self { users: Vec::new() });
    }
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0xffff {
      return Err(Error::invalid(0, "SttbProtUser is not extended"));
    }
    let count = usize::from(input.u16()?);
    if input.u16()? != 2 || count > bytes.len().saturating_sub(6) / 4 {
      return Err(Error::invalid(0, "SttbProtUser header or count is invalid"));
    }
    let mut users = Vec::with_capacity(count);
    for _ in 0..count {
      let length = usize::from(input.u16()?);
      if length > 255 {
        return Err(Error::invalid(
          0,
          "SttbProtUser name exceeds 255 characters",
        ));
      }
      let mut name = Vec::with_capacity(length);
      for _ in 0..length {
        name.push(input.u16()?);
      }
      users.push(ProtectedUser {
        name,
        role: ProtectedUserRole::from_i16(input.i16()?)?,
      });
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(0, "trailing bytes after SttbProtUser"));
    }
    let value = Self { users };
    value.validate()?;
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.validate()?;
    if self.users.is_empty() {
      return Ok(Vec::new());
    }
    let count = u16::try_from(self.users.len())
      .map_err(|_| Error::Limit("SttbProtUser count exceeds u16".into()))?;
    if count == u16::MAX {
      return Err(Error::Limit("SttbProtUser count is 0xffff".into()));
    }
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0xffff);
    push_u16(&mut bytes, count);
    push_u16(&mut bytes, 2);
    for user in &self.users {
      push_u16(&mut bytes, user.name.len() as u16);
      write_u16_array(&mut bytes, &user.name);
      bytes.extend_from_slice(&user.role.to_i16().to_le_bytes());
    }
    Ok(bytes)
  }

  fn validate(&self) -> Result<()> {
    let mut names = BTreeSet::new();
    for user in &self.users {
      if user.name.len() > 255 {
        return Err(Error::invalid(
          0,
          "SttbProtUser name exceeds 255 characters",
        ));
      }
      if !names.insert(user.name.as_slice()) {
        return Err(Error::invalid(
          0,
          "SttbProtUser contains duplicate usernames",
        ));
      }
    }
    Ok(())
  }
}

impl ProtectedUserRole {
  fn from_i16(value: i16) -> Result<Self> {
    Ok(match value {
      0 => Self::None,
      -4 => Self::Owner,
      -5 => Self::Editor,
      _ => return Err(Error::invalid(0, "SttbProtUser role is invalid")),
    })
  }

  fn to_i16(self) -> i16 {
    match self {
      Self::None => 0,
      Self::Owner => -4,
      Self::Editor => -5,
    }
  }
}

fn read_range_permissions(bytes: &[u8]) -> Result<Vec<RangePermission>> {
  let mut input = SliceReader::new(bytes);
  if input.u16()? != 0xffff {
    return Err(Error::invalid(0, "SttbfBkmkProt is not extended"));
  }
  let count = input.i32()?;
  if !(1..=0x7ff0).contains(&count) || input.u16()? != 8 {
    return Err(Error::invalid(
      0,
      "SttbfBkmkProt header or count is invalid",
    ));
  }
  let count = count as usize;
  if count > bytes.len().saturating_sub(8) / 10 {
    return Err(Error::invalid(0, "SttbfBkmkProt count exceeds its payload"));
  }
  let mut permissions = Vec::with_capacity(count);
  for _ in 0..count {
    if input.u16()? != 0 {
      return Err(Error::invalid(0, "SttbfBkmkProt cchData is not zero"));
    }
    let editors = PermittedEditors::from_i16(input.i16()?)?;
    if input.u16()? != 1 {
      return Err(Error::invalid(0, "PRTI protection type is not ReadWrite"));
    }
    permissions.push(RangePermission {
      editors,
      ignored_index: input.u16()?,
      ignored_use: input.u16()?,
    });
  }
  if input.offset != bytes.len() {
    return Err(Error::invalid(0, "trailing bytes after SttbfBkmkProt"));
  }
  Ok(permissions)
}

fn write_range_permissions(permissions: &[RangePermission]) -> Result<Vec<u8>> {
  if permissions.is_empty() || permissions.len() > 0x7ff0 {
    return Err(Error::invalid(
      0,
      "range permission count is outside 1..=0x7ff0",
    ));
  }
  let mut bytes = Vec::with_capacity(8 + permissions.len() * 10);
  push_u16(&mut bytes, 0xffff);
  push_i32(&mut bytes, permissions.len() as i32);
  push_u16(&mut bytes, 8);
  for permission in permissions {
    push_u16(&mut bytes, 0);
    bytes.extend_from_slice(&permission.editors.to_i16()?.to_le_bytes());
    push_u16(&mut bytes, 1);
    push_u16(&mut bytes, permission.ignored_index);
    push_u16(&mut bytes, permission.ignored_use);
  }
  Ok(bytes)
}

impl XmlSchemaReference {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    Ok(Self {
      uri: read_xst(input, "XSDR URI")?,
      manifest_location: read_xst(input, "XSDR manifest location")?,
      elements: XmlSchemaStringTable::read(input, "XSDR elements")?,
      attributes: XmlSchemaStringTable::read(input, "XSDR attributes")?,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    write_xst(bytes, &self.uri, "XSDR URI")?;
    write_xst(bytes, &self.manifest_location, "XSDR manifest location")?;
    self.elements.write(bytes, "XSDR elements")?;
    self.attributes.write(bytes, "XSDR attributes")
  }
}

impl XmlSchemaStringTable {
  fn read(input: &mut SliceReader<'_>, name: &str) -> Result<Self> {
    let utf16 = if input.bytes.get(input.offset..input.offset + 2) == Some(&[0xff, 0xff]) {
      input.u16()?;
      true
    } else {
      false
    };
    let count = input.i32()?;
    if count <= 0 {
      return Err(Error::invalid(
        input.offset.saturating_sub(4) as u64,
        format!("{name} cData is not positive"),
      ));
    }
    let count =
      usize::try_from(count).map_err(|_| Error::Limit(format!("{name} count exceeds usize")))?;
    if input.u16()? != 0 {
      return Err(Error::invalid(
        input.offset.saturating_sub(2) as u64,
        format!("{name} cbExtra is not zero"),
      ));
    }
    let minimum = if utf16 { 2 } else { 1 };
    if count > input.bytes.len().saturating_sub(input.offset) / minimum {
      return Err(Error::invalid(
        input.offset as u64,
        format!("{name} count exceeds its bounded payload"),
      ));
    }
    if utf16 {
      let mut values = Vec::with_capacity(count);
      for _ in 0..count {
        let length = usize::from(input.u16()?);
        let mut value = Vec::with_capacity(length);
        for _ in 0..length {
          value.push(input.u16()?);
        }
        values.push(value);
      }
      Ok(Self::Utf16(values))
    } else {
      let mut values = Vec::with_capacity(count);
      for _ in 0..count {
        let length = usize::from(input.u8()?);
        values.push(input.bytes(length)?.to_vec());
      }
      Ok(Self::Ansi(values))
    }
  }

  fn write(&self, bytes: &mut Vec<u8>, name: &str) -> Result<()> {
    match self {
      Self::Ansi(values) => {
        write_schema_sttb_header(bytes, values.len(), false, name)?;
        for value in values {
          bytes.push(
            u8::try_from(value.len())
              .map_err(|_| Error::Limit(format!("{name} string exceeds u8")))?,
          );
          bytes.extend_from_slice(value);
        }
      }
      Self::Utf16(values) => {
        write_schema_sttb_header(bytes, values.len(), true, name)?;
        for value in values {
          push_u16(
            bytes,
            u16::try_from(value.len())
              .map_err(|_| Error::Limit(format!("{name} string exceeds u16")))?,
          );
          write_u16_array(bytes, value);
        }
      }
    }
    Ok(())
  }
}

fn read_xst(input: &mut SliceReader<'_>, name: &str) -> Result<Vec<u16>> {
  let length = usize::from(input.u16()?);
  if length > input.bytes.len().saturating_sub(input.offset) / 2 {
    return Err(Error::invalid(
      input.offset as u64,
      format!("{name} exceeds its bounded payload"),
    ));
  }
  let mut value = Vec::with_capacity(length);
  for _ in 0..length {
    value.push(input.u16()?);
  }
  Ok(value)
}

fn write_xst(bytes: &mut Vec<u8>, value: &[u16], name: &str) -> Result<()> {
  push_u16(
    bytes,
    u16::try_from(value.len()).map_err(|_| Error::Limit(format!("{name} exceeds u16")))?,
  );
  write_u16_array(bytes, value);
  Ok(())
}

fn write_schema_sttb_header(
  bytes: &mut Vec<u8>,
  count: usize,
  utf16: bool,
  name: &str,
) -> Result<()> {
  if count == 0 {
    return Err(Error::invalid(0, format!("{name} cData is not positive")));
  }
  let count =
    i32::try_from(count).map_err(|_| Error::Limit(format!("{name} count exceeds i32")))?;
  if utf16 {
    push_u16(bytes, 0xffff);
  }
  push_i32(bytes, count);
  push_u16(bytes, 0);
  Ok(())
}

impl SubdocumentTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(16) {
      return Err(Error::invalid(
        0,
        "PlcfWKB length does not match 12-byte WKB records",
      ));
    }
    let count = (bytes.len() - 4) / 16;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    require_strictly_increasing(&positions, "PlcfWKB CP")?;
    let mut subdocuments = Vec::with_capacity(count);
    for _ in 0..count {
      if input.u16()? != 0 {
        return Err(Error::invalid(0, "WKB fn is not zero"));
      }
      let flags = input.u16()?;
      if flags & 0xff5b != 0 || flags & 0x0020 == 0 {
        return Err(Error::invalid(0, "WKB reserved flags are invalid"));
      }
      if input.u16()? != 2 {
        return Err(Error::invalid(0, "WKB lvl is not 2"));
      }
      let fnpi = input.u16()?;
      if fnpi & 0x000f != 5 || fnpi >> 4 == 0x0fff {
        return Err(Error::invalid(0, "WKB FNPI is not a valid subdocument id"));
      }
      if input.u32()? != 0 {
        return Err(Error::invalid(0, "WKB pdod is not zero"));
      }
      subdocuments.push(SubdocumentReference {
        ignored_flag3: flags & 0x0004 != 0,
        ignored_flag8: flags & 0x0080 != 0,
        file_identifier: fnpi >> 4,
      });
    }
    Ok(Self {
      positions,
      subdocuments,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.subdocuments.len().saturating_add(1) {
      return Err(Error::invalid(0, "PlcfWKB CP/WKB cardinality changed"));
    }
    require_strictly_increasing(&self.positions, "PlcfWKB CP")?;
    let capacity = self
      .subdocuments
      .len()
      .checked_mul(16)
      .and_then(|length| length.checked_add(4))
      .ok_or_else(|| Error::Limit("PlcfWKB encoded length overflow".into()))?;
    let mut bytes = Vec::with_capacity(capacity);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for subdocument in &self.subdocuments {
      if subdocument.file_identifier >= 0x0fff {
        return Err(Error::invalid(0, "WKB FNPI id is 0xfff"));
      }
      push_u16(&mut bytes, 0);
      push_u16(
        &mut bytes,
        (u16::from(subdocument.ignored_flag3) << 2)
          | 0x0020
          | (u16::from(subdocument.ignored_flag8) << 7),
      );
      push_u16(&mut bytes, 2);
      push_u16(&mut bytes, (subdocument.file_identifier << 4) | 5);
      push_u32(&mut bytes, 0);
    }
    Ok(bytes)
  }

  pub fn validate_main_document_length(&self, character_count: u32) -> Result<()> {
    let limit = character_count
      .checked_add(2)
      .ok_or_else(|| Error::Limit("PlcfWKB final CP overflow".into()))?;
    if self.positions.last().copied() != Some(limit)
      || self.positions[..self.positions.len().saturating_sub(1)]
        .iter()
        .any(|position| *position >= character_count)
    {
      return Err(Error::invalid(
        0,
        "PlcfWKB CPs do not match the main-document length",
      ));
    }
    Ok(())
  }

  pub fn validate_file_references(&self, files: &ExternalFileNameTable) -> Result<()> {
    files.validate()?;
    if self.subdocuments.iter().any(|subdocument| {
      !files.contains(ExternalFileType::Subdocument, subdocument.file_identifier)
    }) {
      return Err(Error::invalid(0, "PlcfWKB FNPI is absent from SttbFnm"));
    }
    Ok(())
  }
}

impl CaptionDefinitions {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0xffff {
      return Err(Error::invalid(0, "SttbfCaption is not extended"));
    }
    let count = usize::from(input.u16()?);
    if input.u16()? != 6 || count > bytes.len().saturating_sub(6) / 8 {
      return Err(Error::invalid(0, "SttbfCaption header or count is invalid"));
    }
    let mut captions = Vec::with_capacity(count);
    for _ in 0..count {
      let length = usize::from(input.u16()?);
      if length > 40 {
        return Err(Error::invalid(0, "caption label exceeds 40 characters"));
      }
      let mut label = Vec::with_capacity(length);
      for _ in 0..length {
        label.push(input.u16()?);
      }
      captions.push(CaptionDefinition {
        label,
        properties: CaptionProperties::read(&mut input)?,
      });
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(0, "trailing bytes after SttbfCaption"));
    }
    Ok(Self { captions })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let count = u16::try_from(self.captions.len())
      .map_err(|_| Error::Limit("SttbfCaption count exceeds u16".into()))?;
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0xffff);
    push_u16(&mut bytes, count);
    push_u16(&mut bytes, 6);
    for caption in &self.captions {
      if caption.label.len() > 40 {
        return Err(Error::invalid(0, "caption label exceeds 40 characters"));
      }
      push_u16(&mut bytes, caption.label.len() as u16);
      write_u16_array(&mut bytes, &caption.label);
      caption.properties.write(&mut bytes)?;
    }
    Ok(bytes)
  }
}

impl CaptionProperties {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let flags = input.u16()?;
    let location = match flags & 3 {
      0 => CaptionLocation::Below,
      1 => CaptionLocation::Above,
      _ => return Err(Error::invalid(0, "CAPI iLocation is invalid")),
    };
    let include_chapter_number = flags & 4 != 0;
    let heading_value = ((flags >> 3) & 0x0f) as u8;
    let heading = if include_chapter_number {
      if !(1..=9).contains(&heading_value) {
        return Err(Error::invalid(0, "CAPI iHeading is invalid"));
      }
      CaptionHeading::Heading(heading_value)
    } else {
      CaptionHeading::Ignored(heading_value)
    };
    let number_format = NumberingFormat::from_u16(input.u16()?)?;
    let separator_value = input.u16()?;
    let separator = if include_chapter_number {
      CaptionSeparator::from_u16(separator_value)?
    } else {
      CaptionSeparator::Ignored(separator_value)
    };
    Ok(Self {
      location,
      include_chapter_number,
      heading,
      ignored: ((flags >> 7) & 0xff) as u8,
      no_label: flags & 0x8000 != 0,
      number_format,
      separator,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    let heading = match (self.include_chapter_number, self.heading) {
      (true, CaptionHeading::Heading(value)) if (1..=9).contains(&value) => value,
      (false, CaptionHeading::Ignored(value)) if value <= 0x0f => value,
      _ => return Err(Error::invalid(0, "CAPI heading disagrees with fChapNum")),
    };
    let separator = match (self.include_chapter_number, self.separator) {
      (true, value) => value.to_u16(true)?,
      (false, CaptionSeparator::Ignored(value)) => value,
      _ => return Err(Error::invalid(0, "CAPI separator disagrees with fChapNum")),
    };
    let location = match self.location {
      CaptionLocation::Below => 0,
      CaptionLocation::Above => 1,
    };
    push_u16(
      bytes,
      location
        | (u16::from(self.include_chapter_number) << 2)
        | (u16::from(heading) << 3)
        | (u16::from(self.ignored) << 7)
        | (u16::from(self.no_label) << 15),
    );
    push_u16(bytes, u16::from(self.number_format.code()));
    push_u16(bytes, separator);
    Ok(())
  }
}

impl CaptionSeparator {
  fn from_u16(value: u16) -> Result<Self> {
    Ok(match value {
      0x001e => Self::Hyphen,
      0x002e => Self::Period,
      0x003a => Self::Colon,
      0x2013 => Self::EnDash,
      0x2014 => Self::EmDash,
      _ => return Err(Error::invalid(0, "CAPI chapter separator is invalid")),
    })
  }

  fn to_u16(self, interpreted: bool) -> Result<u16> {
    Ok(match self {
      Self::Hyphen => 0x001e,
      Self::Period => 0x002e,
      Self::Colon => 0x003a,
      Self::EnDash => 0x2013,
      Self::EmDash => 0x2014,
      Self::Ignored(value) if !interpreted => value,
      Self::Ignored(_) => {
        return Err(Error::invalid(0, "CAPI interpreted separator is ignored"));
      }
    })
  }
}

impl AutoCaptionDefinitions {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0xffff {
      return Err(Error::invalid(0, "SttbfAutoCaption is not extended"));
    }
    let count = usize::from(input.u16()?);
    if input.u16()? != 2 || count > bytes.len().saturating_sub(6) / 4 {
      return Err(Error::invalid(
        0,
        "SttbfAutoCaption header or count is invalid",
      ));
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
      let length = usize::from(input.u16()?);
      let mut program_id = Vec::with_capacity(length);
      for _ in 0..length {
        program_id.push(input.u16()?);
      }
      entries.push(AutoCaptionDefinition {
        program_id,
        caption_index: input.u16()?,
      });
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(0, "trailing bytes after SttbfAutoCaption"));
    }
    Ok(Self { entries })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let count = u16::try_from(self.entries.len())
      .map_err(|_| Error::Limit("SttbfAutoCaption count exceeds u16".into()))?;
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0xffff);
    push_u16(&mut bytes, count);
    push_u16(&mut bytes, 2);
    for entry in &self.entries {
      push_u16(
        &mut bytes,
        u16::try_from(entry.program_id.len())
          .map_err(|_| Error::Limit("AutoCaption ProgID exceeds u16".into()))?,
      );
      write_u16_array(&mut bytes, &entry.program_id);
      push_u16(&mut bytes, entry.caption_index);
    }
    Ok(bytes)
  }

  pub fn validate_against(&self, captions: &CaptionDefinitions) -> Result<()> {
    if self
      .entries
      .iter()
      .any(|entry| usize::from(entry.caption_index) >= captions.captions.len())
    {
      return Err(Error::invalid(
        0,
        "AutoCaption index is outside SttbfCaption",
      ));
    }
    Ok(())
  }
}

impl RevisionAuthors {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes == [0; 22] {
      return Ok(Self::CompatibilityZeroPlaceholder);
    }
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0xffff {
      return Err(Error::invalid(0, "SttbfRMark is not an extended STTB"));
    }
    let count = usize::from(input.u16()?);
    if input.u16()? != 0 {
      return Err(Error::invalid(4, "SttbfRMark cbExtra is not zero"));
    }
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
      let length = usize::from(input.u16()?);
      let mut name = Vec::with_capacity(length);
      for _ in 0..length {
        name.push(input.u16()?);
      }
      names.push(name);
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after SttbfRMark",
      ));
    }
    Self::validate_names(&names)?;
    Ok(Self::Standard { names })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let Self::Standard { names } = self else {
      return Ok(vec![0; 22]);
    };
    Self::validate_names(names)?;
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0xffff);
    push_u16(
      &mut bytes,
      u16::try_from(names.len())
        .map_err(|_| Error::Limit("SttbfRMark count exceeds u16".into()))?,
    );
    push_u16(&mut bytes, 0);
    for name in names {
      push_u16(
        &mut bytes,
        u16::try_from(name.len())
          .map_err(|_| Error::Limit("revision author name exceeds u16".into()))?,
      );
      write_u16_array(&mut bytes, name);
    }
    Ok(bytes)
  }

  pub fn names(&self) -> &[Vec<u16>] {
    match self {
      Self::Standard { names } => names,
      Self::CompatibilityZeroPlaceholder => &[],
    }
  }

  fn validate_names(names: &[Vec<u16>]) -> Result<()> {
    if names.first().map(Vec::as_slice) != Some(UNKNOWN_REVISION_AUTHOR.as_slice()) {
      return Err(Error::invalid(
        0,
        "SttbfRMark first author is not the required Unknown entry",
      ));
    }
    Ok(())
  }
}

impl SpellingStateTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(6) {
      return Err(Error::invalid(
        0,
        "Plcfspl length does not match 2-byte SpellingSpls records",
      ));
    }
    let count = (bytes.len() - 4) / 6;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut states = Vec::with_capacity(count);
    for _ in 0..count {
      states.push(SpellingState::from_u16(input.u16()?)?);
    }
    require_nondecreasing(&positions, "Plcfspl CP")?;
    Ok(Self { positions, states })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.states.len().saturating_add(1) {
      return Err(Error::invalid(
        0,
        "Plcfspl CP/SpellingSpls cardinality changed",
      ));
    }
    require_nondecreasing(&self.positions, "Plcfspl CP")?;
    let mut bytes = Vec::with_capacity(self.positions.len() * 4 + self.states.len() * 2);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for state in &self.states {
      push_u16(&mut bytes, state.to_u16()?);
    }
    Ok(bytes)
  }
}

impl SpellingState {
  fn from_u16(value: u16) -> Result<Self> {
    if value & 0xffe0 != 0 {
      return Err(Error::invalid(
        0,
        "SpellingSpls has nonzero fExtend, fTypo, or unused bits",
      ));
    }
    let kind = SpellingStateKind::from_u8((value & 0x000f) as u8)?;
    let state = Self {
      kind,
      error: value & 0x0010 != 0,
    };
    state.validate()?;
    Ok(state)
  }

  fn to_u16(self) -> Result<u16> {
    self.validate()?;
    Ok(u16::from(self.kind.to_u8()) | (u16::from(self.error) << 4))
  }

  fn validate(self) -> Result<()> {
    let error_is_valid = match self.kind {
      SpellingStateKind::RepeatWord
      | SpellingStateKind::UnknownWord
      | SpellingStateKind::Compatibility13 => self.error,
      SpellingStateKind::Dirty | SpellingStateKind::Edit => true,
      SpellingStateKind::MaybeDirty | SpellingStateKind::Foreign | SpellingStateKind::Clean => {
        !self.error
      }
    };
    if !error_is_valid {
      return Err(Error::invalid(0, "SpellingSpls fError does not match splf"));
    }
    Ok(())
  }
}

impl SpellingStateKind {
  fn from_u8(value: u8) -> Result<Self> {
    Ok(match value {
      0x02 => Self::MaybeDirty,
      0x03 => Self::Dirty,
      0x04 => Self::Edit,
      0x05 => Self::Foreign,
      0x07 => Self::Clean,
      0x0b => Self::RepeatWord,
      0x0c => Self::UnknownWord,
      0x0d => Self::Compatibility13,
      _ => {
        return Err(Error::invalid(
          0,
          format!("invalid SpellingSpls splf 0x{value:x}"),
        ));
      }
    })
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::MaybeDirty => 0x02,
      Self::Dirty => 0x03,
      Self::Edit => 0x04,
      Self::Foreign => 0x05,
      Self::Clean => 0x07,
      Self::RepeatWord => 0x0b,
      Self::UnknownWord => 0x0c,
      Self::Compatibility13 => 0x0d,
    }
  }
}

impl GrammarStateTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(6) {
      return Err(Error::invalid(
        0,
        "Plcfgram length does not match 2-byte GrammarSpls records",
      ));
    }
    let count = (bytes.len() - 4) / 6;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut states = Vec::with_capacity(count);
    for _ in 0..count {
      states.push(GrammarState::from_u16(input.u16()?)?);
    }
    require_nondecreasing(&positions, "Plcfgram CP")?;
    Ok(Self { positions, states })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.states.len().saturating_add(1) {
      return Err(Error::invalid(
        0,
        "Plcfgram CP/GrammarSpls cardinality changed",
      ));
    }
    require_nondecreasing(&self.positions, "Plcfgram CP")?;
    let mut bytes = Vec::with_capacity(self.positions.len() * 4 + self.states.len() * 2);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for state in &self.states {
      push_u16(&mut bytes, state.to_u16()?);
    }
    Ok(bytes)
  }
}

impl LanguageDetectionStateTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(6) {
      return Err(Error::invalid(
        0,
        "Plcflad length does not match 2-byte LadSpls records",
      ));
    }
    let count = (bytes.len() - 4) / 6;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut states = Vec::with_capacity(count);
    for _ in 0..count {
      states.push(LanguageDetectionState::from_u16(input.u16()?)?);
    }
    require_nondecreasing(&positions, "Plcflad CP")?;
    Ok(Self { positions, states })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.states.len().saturating_add(1) {
      return Err(Error::invalid(0, "Plcflad CP/LadSpls cardinality changed"));
    }
    require_nondecreasing(&self.positions, "Plcflad CP")?;
    let mut bytes = Vec::with_capacity(self.positions.len() * 4 + self.states.len() * 2);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for state in &self.states {
      push_u16(&mut bytes, state.to_u16()?);
    }
    Ok(bytes)
  }
}

impl LanguageDetectionState {
  fn from_u16(value: u16) -> Result<Self> {
    if value & 0xffe0 != 0 {
      return Err(Error::invalid(
        0,
        "LadSpls has nonzero fExtend, fTypo, or unused bits",
      ));
    }
    let state = Self {
      kind: LanguageDetectionStateKind::from_u8((value & 0x000f) as u8)?,
      error: value & 0x0010 != 0,
    };
    state.validate()?;
    Ok(state)
  }

  fn to_u16(self) -> Result<u16> {
    self.validate()?;
    Ok(u16::from(self.kind.to_u8()) | (u16::from(self.error) << 4))
  }

  fn validate(self) -> Result<()> {
    let error_is_valid = match self.kind {
      LanguageDetectionStateKind::Dirty | LanguageDetectionStateKind::Edit => true,
      LanguageDetectionStateKind::MaybeDirty
      | LanguageDetectionStateKind::Foreign
      | LanguageDetectionStateKind::Clean
      | LanguageDetectionStateKind::NoLanguageDetection => !self.error,
    };
    if !error_is_valid {
      return Err(Error::invalid(0, "LadSpls fError does not match splf"));
    }
    Ok(())
  }
}

impl LanguageDetectionStateKind {
  fn from_u8(value: u8) -> Result<Self> {
    Ok(match value {
      0x02 => Self::MaybeDirty,
      0x03 => Self::Dirty,
      0x04 => Self::Edit,
      0x05 => Self::Foreign,
      0x07 => Self::Clean,
      0x08 => Self::NoLanguageDetection,
      _ => {
        return Err(Error::invalid(
          0,
          format!("invalid LadSpls splf 0x{value:x}"),
        ));
      }
    })
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::MaybeDirty => 0x02,
      Self::Dirty => 0x03,
      Self::Edit => 0x04,
      Self::Foreign => 0x05,
      Self::Clean => 0x07,
      Self::NoLanguageDetection => 0x08,
    }
  }
}

impl GrammarState {
  fn from_u16(value: u16) -> Result<Self> {
    if value & 0xff80 != 0 {
      return Err(Error::invalid(0, "GrammarSpls has nonzero unused bits"));
    }
    let state = Self {
      kind: GrammarStateKind::from_u8((value & 0x000f) as u8)?,
      error: value & 0x0010 != 0,
      extend: value & 0x0020 != 0,
      typo: value & 0x0040 != 0,
    };
    state.validate()?;
    Ok(state)
  }

  fn to_u16(self) -> Result<u16> {
    self.validate()?;
    Ok(
      u16::from(self.kind.to_u8())
        | (u16::from(self.error) << 4)
        | (u16::from(self.extend) << 5)
        | (u16::from(self.typo) << 6),
    )
  }

  fn validate(self) -> Result<()> {
    let error_is_valid = match self.kind {
      GrammarStateKind::ErrorMin | GrammarStateKind::RepeatWord | GrammarStateKind::UnknownWord => {
        self.error
      }
      GrammarStateKind::Dirty | GrammarStateKind::Edit => true,
      GrammarStateKind::MaybeDirty | GrammarStateKind::Foreign | GrammarStateKind::Clean => {
        !self.error
      }
    };
    if !error_is_valid {
      return Err(Error::invalid(0, "GrammarSpls fError does not match splf"));
    }
    if self.extend && !self.error {
      return Err(Error::invalid(
        0,
        "GrammarSpls fExtend is set without fError",
      ));
    }
    Ok(())
  }
}

impl GrammarStateKind {
  fn from_u8(value: u8) -> Result<Self> {
    Ok(match value {
      0x02 => Self::MaybeDirty,
      0x03 => Self::Dirty,
      0x04 => Self::Edit,
      0x05 => Self::Foreign,
      0x07 => Self::Clean,
      0x0a => Self::ErrorMin,
      0x0b => Self::RepeatWord,
      0x0c => Self::UnknownWord,
      _ => {
        return Err(Error::invalid(
          0,
          format!("invalid GrammarSpls splf 0x{value:x}"),
        ));
      }
    })
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::MaybeDirty => 0x02,
      Self::Dirty => 0x03,
      Self::Edit => 0x04,
      Self::Foreign => 0x05,
      Self::Clean => 0x07,
      Self::ErrorMin => 0x0a,
      Self::RepeatWord => 0x0b,
      Self::UnknownWord => 0x0c,
    }
  }
}

impl ListStyleTemplates {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0xffff {
      return Err(Error::invalid(0, "SttbRgtplc fExtend is not 0xffff"));
    }
    let count = usize::from(input.u16()?);
    if count > 0x7ff0 {
      return Err(Error::invalid(2, "SttbRgtplc cData exceeds 0x7ff0"));
    }
    if input.u16()? != 0 {
      return Err(Error::invalid(4, "SttbRgtplc cbExtra is not zero"));
    }
    let mut lists = Vec::with_capacity(count);
    for _ in 0..count {
      match input.u16()? {
        0 => lists.push(None),
        0x12 => {
          let mut levels = [ListLevelTemplateCode::UserDefined { random: 0 }; 9];
          for level in &mut levels {
            *level = ListLevelTemplateCode::from_u32(input.u32()?)?;
          }
          lists.push(Some(levels));
        }
        length => {
          return Err(Error::invalid(
            input.offset.saturating_sub(2) as u64,
            format!("SttbRgtplc cchData is {length:#x}, expected 0 or 0x12"),
          ));
        }
      }
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "SttbRgtplc has trailing bytes",
      ));
    }
    Ok(Self { lists })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.lists.len() > 0x7ff0 {
      return Err(Error::Limit("SttbRgtplc cData exceeds 0x7ff0".into()));
    }
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0xffff);
    push_u16(&mut bytes, self.lists.len() as u16);
    push_u16(&mut bytes, 0);
    for list in &self.lists {
      if let Some(levels) = list {
        push_u16(&mut bytes, 0x12);
        for level in levels {
          push_u32(&mut bytes, level.to_u32()?);
        }
      } else {
        push_u16(&mut bytes, 0);
      }
    }
    Ok(bytes)
  }
}

impl ListLevelTemplateCode {
  fn from_u32(value: u32) -> Result<Self> {
    if value & 1 == 0 {
      return Ok(Self::UserDefined { random: value >> 1 });
    }
    let format = BuiltInListFormat::from_u16(((value >> 1) & 0x7fff) as u16)?;
    Ok(Self::BuiltIn {
      format,
      lid: (value >> 16) as u16,
    })
  }

  fn to_u32(self) -> Result<u32> {
    Ok(match self {
      Self::BuiltIn { format, lid } => {
        1 | (u32::from(format.to_u16()) << 1) | (u32::from(lid) << 16)
      }
      Self::UserDefined { random } => {
        if random > 0x7fff_ffff {
          return Err(Error::invalid(0, "TplcUser random exceeds 31 bits"));
        }
        random << 1
      }
    })
  }
}

impl BuiltInListFormat {
  fn from_u16(value: u16) -> Result<Self> {
    match value {
      0..=0x0d => Ok(Self::Format(value as u8)),
      0x7fff => Ok(Self::None),
      _ => Err(Error::invalid(
        0,
        format!("invalid TplcBuildIn ilgpdM1 {value:#x}"),
      )),
    }
  }

  fn to_u16(self) -> u16 {
    match self {
      Self::Format(value) => u16::from(value),
      Self::None => 0x7fff,
    }
  }
}

impl FrameAndListRecords {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let mut records = Vec::new();
    while input.offset < bytes.len() {
      let start = input.offset;
      let size =
        usize::try_from(input.u32()?).map_err(|_| Error::Limit("Dofrh cb exceeds usize".into()))?;
      if size < 8 {
        return Err(Error::invalid(
          start as u64,
          "Dofrh cb is smaller than header",
        ));
      }
      let kind = input.u32()?;
      let body = input.bytes(size - 8)?;
      records.push(FrameAndListRecord::from_body(kind, body)?);
    }
    Ok(Self { records })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for record in &self.records {
      let (kind, body) = record.to_body()?;
      let size = body
        .len()
        .checked_add(8)
        .ok_or_else(|| Error::Limit("Dofrh cb overflow".into()))?;
      push_u32(
        &mut bytes,
        u32::try_from(size).map_err(|_| Error::Limit("Dofrh cb exceeds u32".into()))?,
      );
      push_u32(&mut bytes, kind);
      bytes.extend_from_slice(&body);
    }
    Ok(bytes)
  }
}

impl FrameAndListRecord {
  fn from_body(kind: u32, body: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(body);
    let record = match kind {
      0 => Self::FrameSet,
      1 => {
        let divider_units = FrameDividerUnits::from_u32(input.u32()?)?;
        let divider_value = input.u32()?;
        let child_layout = FrameChildLayout::from_i32(input.i32()?)?;
        let kind = FrameRecordKind::from_u32(input.u32()?)?;
        let horizontal_margin = input.i32()?;
        let vertical_margin = input.i32()?;
        let scroll = FrameScroll::from_u32(input.u32()?)?;
        let flags = input.u32()?;
        Self::Frame(FrameRecord {
          divider_units,
          divider_value,
          child_layout,
          kind,
          horizontal_margin,
          vertical_margin,
          scroll,
          linked: flags & 1 != 0,
          no_resize: flags & 2 != 0,
          unused_flags: flags >> 2,
          unused: input.u32()?,
        })
      }
      2 => {
        let value = input.u32()?;
        Self::ChildMarker {
          push: value & 1 != 0,
          unused: value >> 1,
        }
      }
      3 | 4 => {
        let value = Xstz::read(&mut input)?;
        let maximum = if kind == 3 { 255 } else { 258 };
        if value.characters.len() > maximum || value.terminator != 0 {
          return Err(Error::invalid(
            0,
            "frame Xstz violates length or terminator",
          ));
        }
        if kind == 3 {
          Self::FrameName(value)
        } else {
          Self::FrameFilePath(value)
        }
      }
      5 => {
        let width_twips = input.i32()?;
        if !(0..=31_680).contains(&width_twips) {
          return Err(Error::invalid(0, "DofrFsnSpbd width is out of range"));
        }
        let color = ColorRef::read(&mut input)?;
        let flags = input.u32()?;
        if flags & !3 != 0 {
          return Err(Error::invalid(8, "DofrFsnSpbd unused flags are nonzero"));
        }
        Self::FrameBorder(FrameBorder {
          width_twips,
          color,
          no_border: flags & 1 != 0,
          three_dimensional: flags & 2 != 0,
        })
      }
      6 => {
        let count = input.i32()?;
        let count = usize::try_from(count)
          .map_err(|_| Error::invalid(0, "DofrRglstsf clstsf is negative"))?;
        let mut styles = Vec::with_capacity(count);
        for _ in 0..count {
          let list_index = input.u16()?;
          let flags = input.u16()?;
          if flags & 0xe000 != 0 {
            return Err(Error::invalid(
              input.offset as u64 - 2,
              "Lstsf unused bits are nonzero",
            ));
          }
          styles.push(ListStyleReference {
            list_index,
            style_index: flags & 0x0fff,
            style_definition: flags & 0x1000 != 0,
          });
        }
        Self::ListStyles(styles)
      }
      _ => return Err(Error::invalid(4, format!("invalid Dofrt {kind:#x}"))),
    };
    if input.offset != body.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "Dofrh body length mismatch",
      ));
    }
    Ok(record)
  }

  fn to_body(&self) -> Result<(u32, Vec<u8>)> {
    let mut body = Vec::new();
    let kind = match self {
      Self::FrameSet => 0,
      Self::Frame(value) => {
        push_u32(&mut body, value.divider_units.to_u32());
        push_u32(&mut body, value.divider_value);
        push_i32(&mut body, value.child_layout.to_i32());
        push_u32(&mut body, value.kind.to_u32());
        push_i32(&mut body, value.horizontal_margin);
        push_i32(&mut body, value.vertical_margin);
        push_u32(&mut body, value.scroll.to_u32());
        if value.unused_flags > 0x3fff_ffff {
          return Err(Error::invalid(0, "DofrFsn unused flags exceed 30 bits"));
        }
        push_u32(
          &mut body,
          u32::from(value.linked) | (u32::from(value.no_resize) << 1) | (value.unused_flags << 2),
        );
        push_u32(&mut body, value.unused);
        1
      }
      Self::ChildMarker { push, unused } => {
        if *unused > 0x7fff_ffff {
          return Err(Error::invalid(0, "DofrFsnp unused exceeds 31 bits"));
        }
        push_u32(&mut body, u32::from(*push) | (*unused << 1));
        2
      }
      Self::FrameName(value) | Self::FrameFilePath(value) => {
        let maximum = if matches!(self, Self::FrameName(_)) {
          255
        } else {
          258
        };
        if value.characters.len() > maximum || value.terminator != 0 {
          return Err(Error::invalid(
            0,
            "frame Xstz violates length or terminator",
          ));
        }
        value.write(&mut body)?;
        if matches!(self, Self::FrameName(_)) {
          3
        } else {
          4
        }
      }
      Self::FrameBorder(value) => {
        if !(0..=31_680).contains(&value.width_twips) {
          return Err(Error::invalid(0, "DofrFsnSpbd width is out of range"));
        }
        push_i32(&mut body, value.width_twips);
        value.color.write(&mut body);
        push_u32(
          &mut body,
          u32::from(value.no_border) | (u32::from(value.three_dimensional) << 1),
        );
        5
      }
      Self::ListStyles(styles) => {
        push_i32(
          &mut body,
          i32::try_from(styles.len())
            .map_err(|_| Error::Limit("DofrRglstsf count exceeds i32".into()))?,
        );
        for style in styles {
          if style.style_index > 0x0fff {
            return Err(Error::invalid(0, "Lstsf style index exceeds 12 bits"));
          }
          push_u16(&mut body, style.list_index);
          push_u16(
            &mut body,
            style.style_index | (u16::from(style.style_definition) << 12),
          );
        }
        6
      }
    };
    Ok((kind, body))
  }
}

impl FrameDividerUnits {
  fn from_u32(value: u32) -> Result<Self> {
    Ok(match value {
      0 => Self::None,
      1 => Self::Pixels,
      2 => Self::Percent,
      3 => Self::Relative,
      _ => return Err(Error::invalid(0, "invalid FssUnits")),
    })
  }
  fn to_u32(self) -> u32 {
    match self {
      Self::None => 0,
      Self::Pixels => 1,
      Self::Percent => 2,
      Self::Relative => 3,
    }
  }
}
impl FrameChildLayout {
  fn from_i32(value: i32) -> Result<Self> {
    Ok(match value {
      -1 => Self::None,
      0 => Self::Rows,
      1 => Self::Columns,
      _ => return Err(Error::invalid(0, "invalid frame child layout")),
    })
  }
  fn to_i32(self) -> i32 {
    match self {
      Self::None => -1,
      Self::Rows => 0,
      Self::Columns => 1,
    }
  }
}
impl FrameRecordKind {
  fn from_u32(value: u32) -> Result<Self> {
    Ok(match value {
      0 => Self::Nil,
      1 => Self::FrameSet,
      2 => Self::Frame,
      _ => return Err(Error::invalid(0, "invalid Fsnk")),
    })
  }
  fn to_u32(self) -> u32 {
    match self {
      Self::Nil => 0,
      Self::FrameSet => 1,
      Self::Frame => 2,
    }
  }
}
impl FrameScroll {
  fn from_u32(value: u32) -> Result<Self> {
    Ok(match value {
      0 => Self::Auto,
      1 => Self::Yes,
      2 => Self::No,
      _ => return Err(Error::invalid(0, "invalid IScrollType")),
    })
  }
  fn to_u32(self) -> u32 {
    match self {
      Self::Auto => 0,
      Self::Yes => 1,
      Self::No => 2,
    }
  }
}

impl GrammarOptionSets {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let count = input.i32()?;
    let count =
      usize::try_from(count).map_err(|_| Error::invalid(0, "PlfCosl iMac is negative"))?;
    let expected = count
      .checked_mul(10)
      .and_then(|length| length.checked_add(4))
      .ok_or_else(|| Error::Limit("PlfCosl encoded length overflow".into()))?;
    if expected != bytes.len() {
      return Err(Error::invalid(
        0,
        "PlfCosl count does not match its bounded length",
      ));
    }
    let mut options = Vec::with_capacity(count);
    for _ in 0..count {
      options.push(GrammarOptionSet {
        option_set: input.u16()?,
        language_id: input.u16()?,
        checker_version: input.u32()?,
        company_id: input.u16()?,
      });
    }
    Ok(Self { options })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let capacity = self
      .options
      .len()
      .checked_mul(10)
      .and_then(|length| length.checked_add(4))
      .ok_or_else(|| Error::Limit("PlfCosl encoded length overflow".into()))?;
    let mut bytes = Vec::with_capacity(capacity);
    push_i32(
      &mut bytes,
      i32::try_from(self.options.len())
        .map_err(|_| Error::Limit("PlfCosl iMac exceeds i32".into()))?,
    );
    for option in &self.options {
      push_u16(&mut bytes, option.option_set);
      push_u16(&mut bytes, option.language_id);
      push_u32(&mut bytes, option.checker_version);
      push_u16(&mut bytes, option.company_id);
    }
    Ok(bytes)
  }
}

impl LegacyGrammarOptionSets {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let count = input.i32()?;
    let count =
      usize::try_from(count).map_err(|_| Error::invalid(0, "PlfGosl iMac is negative"))?;
    let expected = count
      .checked_mul(8)
      .and_then(|length| length.checked_add(4))
      .ok_or_else(|| Error::Limit("PlfGosl encoded length overflow".into()))?;
    if expected != bytes.len() {
      return Err(Error::invalid(
        0,
        "PlfGosl count does not match its bounded length",
      ));
    }
    let mut options = Vec::with_capacity(count);
    for _ in 0..count {
      options.push(LegacyGrammarOptionSet {
        option_set: input.u16()?,
        language_id: input.u16()?,
        checker_version: input.u16()?,
        company_id: input.u16()?,
      });
    }
    Ok(Self { options })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let capacity = self
      .options
      .len()
      .checked_mul(8)
      .and_then(|length| length.checked_add(4))
      .ok_or_else(|| Error::Limit("PlfGosl encoded length overflow".into()))?;
    let mut bytes = Vec::with_capacity(capacity);
    push_i32(
      &mut bytes,
      i32::try_from(self.options.len())
        .map_err(|_| Error::Limit("PlfGosl iMac exceeds i32".into()))?,
    );
    for option in &self.options {
      push_u16(&mut bytes, option.option_set);
      push_u16(&mut bytes, option.language_id);
      push_u16(&mut bytes, option.checker_version);
      push_u16(&mut bytes, option.company_id);
    }
    Ok(bytes)
  }
}

impl AutoSummaryRangeTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(8) {
      return Err(Error::invalid(
        0,
        "PlcfAsumy length does not match 4-byte ASUMY records",
      ));
    }
    let count = (bytes.len() - 4) / 8;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut priorities = Vec::with_capacity(count);
    for _ in 0..count {
      let level = input.i32()?;
      if level <= 0 {
        return Err(Error::invalid(0, "ASUMY lLevel is not positive"));
      }
      priorities.push(AutoSummaryPriority { level });
    }
    require_strictly_increasing(&positions, "PlcfAsumy CP")?;
    Ok(Self {
      positions,
      priorities,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.priorities.len().saturating_add(1) {
      return Err(Error::invalid(0, "PlcfAsumy CP/ASUMY cardinality changed"));
    }
    require_strictly_increasing(&self.positions, "PlcfAsumy CP")?;
    let capacity = self
      .priorities
      .len()
      .checked_mul(8)
      .and_then(|length| length.checked_add(4))
      .ok_or_else(|| Error::Limit("PlcfAsumy encoded length overflow".into()))?;
    let mut bytes = Vec::with_capacity(capacity);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for priority in &self.priorities {
      if priority.level <= 0 {
        return Err(Error::invalid(0, "ASUMY lLevel is not positive"));
      }
      push_i32(&mut bytes, priority.level);
    }
    Ok(bytes)
  }

  pub fn validate_against(&self, info: &AutoSummaryInfo) -> Result<()> {
    if info.valid
      && self
        .priorities
        .iter()
        .any(|priority| priority.level > info.highest_level)
    {
      return Err(Error::invalid(
        0,
        "ASUMY lLevel exceeds valid Asumyi lHighestLevel",
      ));
    }
    Ok(())
  }
}

impl AutoSummaryInfo {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let flags = input.u16()?;
    if flags & 0xffe0 != 0 {
      return Err(Error::invalid(0, "Asumyi has nonzero reserved bits"));
    }
    let info = Self {
      valid: flags & 0x0001 != 0,
      view_active: flags & 0x0002 != 0,
      view_by: AutoSummaryView::from_u16((flags >> 2) & 0x0003),
      update_properties: flags & 0x0010 != 0,
      desired_size: AutoSummaryDesiredSize::from_u16(input.u16()?),
      highest_level: input.i32()?,
      current_level: input.i32()?,
    };
    info.validate()?;
    Ok(info)
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    let flags = u16::from(self.valid)
      | (u16::from(self.view_active) << 1)
      | (self.view_by.to_u16() << 2)
      | (u16::from(self.update_properties) << 4);
    push_u16(bytes, flags);
    push_u16(bytes, self.desired_size.to_u16()?);
    push_i32(bytes, self.highest_level);
    push_i32(bytes, self.current_level);
    Ok(())
  }

  fn validate(self) -> Result<()> {
    if !self.valid {
      return Ok(());
    }
    let percentage = match self.desired_size {
      AutoSummaryDesiredSize::Percentage(value) if value <= 100 => Some(value),
      AutoSummaryDesiredSize::TenPercent => Some(10),
      AutoSummaryDesiredSize::TwentyFivePercent => Some(25),
      AutoSummaryDesiredSize::FiftyPercent => Some(50),
      AutoSummaryDesiredSize::SeventyFivePercent => Some(75),
      _ => None,
    };
    if let Some(percentage) = percentage {
      let expected = (i64::from(percentage) * i64::from(self.highest_level) + 50) / 100;
      if i64::from(self.current_level) != expected {
        return Err(Error::invalid(
          0,
          "valid Asumyi lCurrentLevel does not match its percentage",
        ));
      }
    }
    Ok(())
  }
}

impl AutoSummaryView {
  fn from_u16(value: u16) -> Self {
    match value {
      0 => Self::Highlight,
      1 => Self::HideNonSummaryText,
      2 => Self::InsertAtDocumentStart,
      3 => Self::CreateDocument,
      _ => unreachable!("AutoSummary iViewBy is two bits"),
    }
  }

  fn to_u16(self) -> u16 {
    match self {
      Self::Highlight => 0,
      Self::HideNonSummaryText => 1,
      Self::InsertAtDocumentStart => 2,
      Self::CreateDocument => 3,
    }
  }
}

impl AutoSummaryDesiredSize {
  fn from_u16(value: u16) -> Self {
    match value {
      0..=100 => Self::Percentage(value),
      0xfffe => Self::TenSentences,
      0xfffd => Self::TwentySentences,
      0xfffc => Self::HundredWords,
      0xfffb => Self::FiveHundredWords,
      0xfffa => Self::TenPercent,
      0xfff9 => Self::TwentyFivePercent,
      0xfff8 => Self::FiftyPercent,
      0xfff7 => Self::SeventyFivePercent,
      value => Self::Compatibility(value),
    }
  }

  fn to_u16(self) -> Result<u16> {
    Ok(match self {
      Self::Percentage(value) if value <= 100 => value,
      Self::Percentage(_) => {
        return Err(Error::invalid(0, "Asumyi percentage exceeds 100"));
      }
      Self::TenSentences => 0xfffe,
      Self::TwentySentences => 0xfffd,
      Self::HundredWords => 0xfffc,
      Self::FiveHundredWords => 0xfffb,
      Self::TenPercent => 0xfffa,
      Self::TwentyFivePercent => 0xfff9,
      Self::FiftyPercent => 0xfff8,
      Self::SeventyFivePercent => 0xfff7,
      Self::Compatibility(value) if !matches!(value, 0..=100 | 0xfff7..=0xfffe) => value,
      Self::Compatibility(_) => {
        return Err(Error::invalid(
          0,
          "Asumyi compatibility size has a canonical variant",
        ));
      }
    })
  }
}

impl SmartTagRecognizerStateTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(6) {
      return Err(Error::invalid(
        0,
        "Plcffactoid length does not match 2-byte FactoidSpls records",
      ));
    }
    let count = (bytes.len() - 4) / 6;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut states = Vec::with_capacity(count);
    for _ in 0..count {
      states.push(SmartTagRecognizerState::from_u16(input.u16()?)?);
    }
    require_nondecreasing(&positions, "Plcffactoid CP")?;
    Ok(Self { positions, states })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.states.len().saturating_add(1) {
      return Err(Error::invalid(
        0,
        "Plcffactoid CP/FactoidSpls cardinality changed",
      ));
    }
    require_nondecreasing(&self.positions, "Plcffactoid CP")?;
    let mut bytes = Vec::with_capacity(self.positions.len() * 4 + self.states.len() * 2);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for state in &self.states {
      push_u16(&mut bytes, state.to_u16());
    }
    Ok(bytes)
  }
}

impl SmartTagRecognizerState {
  fn from_u16(value: u16) -> Result<Self> {
    if value & 0xfff0 != 0 {
      return Err(Error::invalid(
        0,
        "FactoidSpls has nonzero flags or unused bits",
      ));
    }
    Ok(Self {
      kind: SmartTagRecognizerStateKind::from_u8(value as u8)?,
    })
  }

  fn to_u16(self) -> u16 {
    u16::from(self.kind.to_u8())
  }
}

impl SmartTagRecognizerStateKind {
  fn from_u8(value: u8) -> Result<Self> {
    Ok(match value {
      0x01 => Self::Pending,
      0x02 => Self::MaybeDirty,
      0x03 => Self::Dirty,
      0x04 => Self::Edit,
      0x07 => Self::Clean,
      _ => {
        return Err(Error::invalid(
          0,
          format!("invalid FactoidSpls splf {value:#x}"),
        ));
      }
    })
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::Pending => 0x01,
      Self::MaybeDirty => 0x02,
      Self::Dirty => 0x03,
      Self::Edit => 0x04,
      Self::Clean => 0x07,
    }
  }
}

impl ParagraphGroupProperties {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let count = usize::from(input.u16()?);
    let mut entries = Vec::with_capacity(count);
    let mut ids = BTreeSet::new();
    for _ in 0..count {
      let id = input.u32()?;
      if id == 0 {
        return Err(Error::invalid(
          input.offset as u64 - 4,
          "PGPInfo id is zero",
        ));
      }
      if !ids.insert(id) {
        return Err(Error::invalid(
          input.offset as u64 - 4,
          "PGPInfo id is duplicated",
        ));
      }
      let parent_id = input.u32()?;
      let table_depth = input.u32()?;
      let flags = input.u16()?;
      if flags & !0x01ff != 0 {
        return Err(Error::invalid(
          input.offset as u64 - 2,
          "PGPInfo grfElements has unknown bits",
        ));
      }
      let options = ParagraphGroupOptions::read(&mut input, flags)?;
      entries.push(ParagraphGroupProperty {
        id,
        parent_id,
        table_depth,
        options,
      });
    }
    if entries
      .iter()
      .any(|entry| entry.parent_id != 0 && !ids.contains(&entry.parent_id))
    {
      return Err(Error::invalid(0, "PGPInfo parent id is missing"));
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "PGPArray has trailing bytes",
      ));
    }
    Ok(Self { entries })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    push_u16(
      &mut bytes,
      u16::try_from(self.entries.len())
        .map_err(|_| Error::Limit("PGPArray count exceeds u16".into()))?,
    );
    let mut ids = BTreeSet::new();
    for entry in &self.entries {
      if entry.id == 0 {
        return Err(Error::invalid(0, "PGPInfo id is zero"));
      }
      if !ids.insert(entry.id) {
        return Err(Error::invalid(0, "PGPInfo id is duplicated"));
      }
      push_u32(&mut bytes, entry.id);
      push_u32(&mut bytes, entry.parent_id);
      push_u32(&mut bytes, entry.table_depth);
      let flags = entry.options.flags();
      push_u16(&mut bytes, flags);
      entry.options.write(&mut bytes, flags)?;
    }
    if self
      .entries
      .iter()
      .any(|entry| entry.parent_id != 0 && !ids.contains(&entry.parent_id))
    {
      return Err(Error::invalid(0, "PGPInfo parent id is missing"));
    }
    Ok(bytes)
  }
}

impl ParagraphGroupOptions {
  fn read(input: &mut SliceReader<'_>, flags: u16) -> Result<Self> {
    if flags == 0 {
      return Ok(Self::default());
    }
    let declared_length = usize::from(input.u16()?);
    let expected_length = Self::encoded_body_len(flags);
    if declared_length != expected_length {
      return Err(Error::invalid(
        input.offset as u64 - 2,
        "PGPOptions cbOption does not match grfElements",
      ));
    }
    Ok(Self {
      left_margin: (flags & 0x0001 != 0).then(|| input.i32()).transpose()?,
      right_margin: (flags & 0x0002 != 0).then(|| input.i32()).transpose()?,
      top_margin: (flags & 0x0004 != 0).then(|| input.i32()).transpose()?,
      bottom_margin: (flags & 0x0008 != 0).then(|| input.i32()).transpose()?,
      left_border: (flags & 0x0010 != 0)
        .then(|| Brc::read(input))
        .transpose()?,
      right_border: (flags & 0x0020 != 0)
        .then(|| Brc::read(input))
        .transpose()?,
      top_border: (flags & 0x0040 != 0)
        .then(|| Brc::read(input))
        .transpose()?,
      bottom_border: (flags & 0x0080 != 0)
        .then(|| Brc::read(input))
        .transpose()?,
      html_block_type: (flags & 0x0100 != 0)
        .then(|| HtmlBlockType::from_u16(input.u16()?))
        .transpose()?,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>, flags: u16) -> Result<()> {
    if flags == 0 {
      return Ok(());
    }
    push_u16(bytes, Self::encoded_body_len(flags) as u16);
    for value in [
      self.left_margin,
      self.right_margin,
      self.top_margin,
      self.bottom_margin,
    ]
    .into_iter()
    .flatten()
    {
      push_i32(bytes, value);
    }
    for border in [
      self.left_border,
      self.right_border,
      self.top_border,
      self.bottom_border,
    ]
    .into_iter()
    .flatten()
    {
      if border.spacing > 0x1f || border.reserved > 0x01ff {
        return Err(Error::invalid(0, "PGPOptions Brc fields exceed bit width"));
      }
      border.write(bytes);
    }
    if let Some(value) = self.html_block_type {
      push_u16(bytes, value.to_u16());
    }
    Ok(())
  }

  fn flags(&self) -> u16 {
    u16::from(self.left_margin.is_some())
      | (u16::from(self.right_margin.is_some()) << 1)
      | (u16::from(self.top_margin.is_some()) << 2)
      | (u16::from(self.bottom_margin.is_some()) << 3)
      | (u16::from(self.left_border.is_some()) << 4)
      | (u16::from(self.right_border.is_some()) << 5)
      | (u16::from(self.top_border.is_some()) << 6)
      | (u16::from(self.bottom_border.is_some()) << 7)
      | (u16::from(self.html_block_type.is_some()) << 8)
  }

  fn encoded_body_len(flags: u16) -> usize {
    usize::from((flags & 0x000f).count_ones() as u8) * 4
      + usize::from(((flags >> 4) & 0x000f).count_ones() as u8) * 8
      + usize::from(flags & 0x0100 != 0) * 2
  }
}

impl HtmlBlockType {
  fn from_u16(value: u16) -> Result<Self> {
    Ok(match value {
      0 => Self::Division,
      1 => Self::BlockQuote,
      2 => Self::Body,
      _ => return Err(Error::invalid(0, "invalid PGPOptions HTML block type")),
    })
  }

  fn to_u16(self) -> u16 {
    match self {
      Self::Division => 0,
      Self::BlockQuote => 1,
      Self::Body => 2,
    }
  }
}

impl SaveHistory {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0xffff {
      return Err(Error::invalid(0, "SttbSavedBy fExtend is not 0xffff"));
    }
    let string_count = usize::from(input.u16()?);
    if string_count > 20 || !string_count.is_multiple_of(2) {
      return Err(Error::invalid(2, "SttbSavedBy cData is odd or exceeds 20"));
    }
    if input.u16()? != 0 {
      return Err(Error::invalid(4, "SttbSavedBy cbExtra is not zero"));
    }
    let mut entries = Vec::with_capacity(string_count / 2);
    for _ in 0..string_count / 2 {
      let author_count = usize::from(input.u16()?);
      let mut author = Vec::with_capacity(author_count);
      for _ in 0..author_count {
        author.push(input.u16()?);
      }
      let path_count = usize::from(input.u16()?);
      let mut path = Vec::with_capacity(path_count);
      for _ in 0..path_count {
        path.push(input.u16()?);
      }
      entries.push(SaveHistoryEntry { author, path });
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "SttbSavedBy has trailing bytes",
      ));
    }
    Ok(Self { entries })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.entries.len() > 10 {
      return Err(Error::Limit(
        "SttbSavedBy contains more than 10 pairs".into(),
      ));
    }
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0xffff);
    push_u16(&mut bytes, (self.entries.len() * 2) as u16);
    push_u16(&mut bytes, 0);
    for entry in &self.entries {
      for value in [&entry.author, &entry.path] {
        push_u16(
          &mut bytes,
          u16::try_from(value.len())
            .map_err(|_| Error::Limit("SttbSavedBy string exceeds u16".into()))?,
        );
        write_u16_array(&mut bytes, value);
      }
    }
    Ok(bytes)
  }
}

impl SmartTagBookmarks {
  pub fn from_bytes(infos: &[u8], starts: &[u8], ends: &[u8]) -> Result<Self> {
    let infos = SmartTagBookmarkInfo::array_from_bytes(infos)?;
    let starts = SmartTagBookmarkStartTable::from_bytes(starts)?;
    let ends = SmartTagBookmarkEndTable::from_bytes(ends)?;
    let count = infos.len();
    if starts.bookmarks.len() != count || ends.bookmarks.len() != count {
      return Err(Error::invalid(
        0,
        "parallel smart-tag bookmark table cardinality differs",
      ));
    }
    let mut start_targets = BTreeSet::new();
    let mut end_targets = BTreeSet::new();
    for (start_index, start) in starts.bookmarks.iter().enumerate() {
      let end_index = usize::from(start.bookmark.end_index);
      if end_index >= count || !start_targets.insert(end_index) {
        return Err(Error::invalid(
          0,
          "FBKFD end index is invalid or duplicated",
        ));
      }
      let end = &ends.bookmarks[end_index];
      if usize::from(end.start_index) != start_index
        || ends.positions[end_index] < starts.positions[start_index]
      {
        return Err(Error::invalid(0, "FBKFD/FBKLD mapping is not reciprocal"));
      }
    }
    for end in &ends.bookmarks {
      let start_index = usize::from(end.start_index);
      if start_index >= count || !end_targets.insert(start_index) {
        return Err(Error::invalid(
          0,
          "FBKLD start index is invalid or duplicated",
        ));
      }
    }
    Ok(Self {
      infos,
      starts,
      ends,
    })
  }

  pub fn to_bytes(&self) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let infos = SmartTagBookmarkInfo::array_to_bytes(&self.infos)?;
    let starts = self.starts.to_bytes()?;
    let ends = self.ends.to_bytes()?;
    Self::from_bytes(&infos, &starts, &ends)?;
    Ok((infos, starts, ends))
  }
}

impl SmartTagBookmarkInfo {
  fn array_from_bytes(bytes: &[u8]) -> Result<Vec<Self>> {
    let mut input = SliceReader::new(bytes);
    if input.u16()? != 0xffff {
      return Err(Error::invalid(0, "SttbfBkmkFactoid fExtend is not 0xffff"));
    }
    let count = usize::from(input.u16()?);
    if count > 0x7ff0 || input.u16()? != 0 {
      return Err(Error::invalid(2, "SttbfBkmkFactoid header is invalid"));
    }
    let mut infos = Vec::with_capacity(count);
    let mut ids = BTreeSet::new();
    for _ in 0..count {
      if input.u16()? != 6 {
        return Err(Error::invalid(0, "FACTOIDINFO cchData is not 6"));
      }
      let id = input.u32()?;
      if !ids.insert(id) {
        return Err(Error::invalid(0, "FACTOIDINFO id is duplicated"));
      }
      let flags = input.u16()?;
      infos.push(Self {
        id,
        sub_entity: flags & 1 != 0,
        unused: flags >> 1,
        source: SmartTagSource::from_u16(input.u16()?)?,
        ignored_property_bag_pointer: input.u32()?,
      });
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(0, "SttbfBkmkFactoid has trailing bytes"));
    }
    Ok(infos)
  }

  fn array_to_bytes(infos: &[Self]) -> Result<Vec<u8>> {
    if infos.len() > 0x7ff0 {
      return Err(Error::Limit("SttbfBkmkFactoid count exceeds 0x7ff0".into()));
    }
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0xffff);
    push_u16(&mut bytes, infos.len() as u16);
    push_u16(&mut bytes, 0);
    let mut ids = BTreeSet::new();
    for info in infos {
      if !ids.insert(info.id) || info.unused > 0x7fff {
        return Err(Error::invalid(
          0,
          "FACTOIDINFO id or unused field is invalid",
        ));
      }
      push_u16(&mut bytes, 6);
      push_u32(&mut bytes, info.id);
      push_u16(&mut bytes, u16::from(info.sub_entity) | (info.unused << 1));
      push_u16(&mut bytes, info.source.to_u16());
      push_u32(&mut bytes, info.ignored_property_bag_pointer);
    }
    Ok(bytes)
  }
}

impl SmartTagSource {
  fn from_u16(value: u16) -> Result<Self> {
    Ok(match value {
      0 => Self::Unknown,
      1 => Self::Grammar,
      2 => Self::ScanDll,
      3 => Self::VisualBasic,
      _ => return Err(Error::invalid(0, "invalid FACTOIDINFO FTO")),
    })
  }
  fn to_u16(self) -> u16 {
    match self {
      Self::Unknown => 0,
      Self::Grammar => 1,
      Self::ScanDll => 2,
      Self::VisualBasic => 3,
    }
  }
}

impl SmartTagBookmarkStartTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(10) {
      return Err(Error::invalid(0, "Plcfbkfd length is invalid"));
    }
    let count = (bytes.len() - 4) / 10;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut bookmarks = Vec::with_capacity(count);
    for _ in 0..count {
      let end_index = input.u16()?;
      let value = input.u16()?;
      bookmarks.push(SmartTagBookmarkStart {
        bookmark: BookmarkStart {
          end_index,
          column_start: (value & 0x007f) as u8,
          published: value & 0x0080 != 0,
          column_limit: ((value >> 8) & 0x003f) as u8,
          native: value & 0x4000 != 0,
          column: value & 0x8000 != 0,
        },
        depth: input.u16()?,
      });
    }
    require_nondecreasing(&positions, "Plcfbkfd CP")?;
    Ok(Self {
      positions,
      bookmarks,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.bookmarks.len().saturating_add(1) {
      return Err(Error::invalid(0, "Plcfbkfd cardinality changed"));
    }
    require_nondecreasing(&self.positions, "Plcfbkfd CP")?;
    let mut bytes = Vec::new();
    for value in &self.positions {
      push_u32(&mut bytes, *value);
    }
    for value in &self.bookmarks {
      let bookmark = value.bookmark;
      if bookmark.column_start > 0x7f || bookmark.column_limit > 0x3f {
        return Err(Error::invalid(0, "FBKFD BKC exceeds bit width"));
      }
      push_u16(&mut bytes, bookmark.end_index);
      push_u16(
        &mut bytes,
        u16::from(bookmark.column_start)
          | (u16::from(bookmark.published) << 7)
          | (u16::from(bookmark.column_limit) << 8)
          | (u16::from(bookmark.native) << 14)
          | (u16::from(bookmark.column) << 15),
      );
      push_u16(&mut bytes, value.depth);
    }
    Ok(bytes)
  }
}

impl SmartTagBookmarkEndTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(8) {
      return Err(Error::invalid(0, "Plcfbkld length is invalid"));
    }
    let count = (bytes.len() - 4) / 8;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut bookmarks = Vec::with_capacity(count);
    for _ in 0..count {
      bookmarks.push(SmartTagBookmarkEnd {
        start_index: input.u16()?,
        depth: input.u16()?,
      });
    }
    require_nondecreasing(&positions, "Plcfbkld CP")?;
    Ok(Self {
      positions,
      bookmarks,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.bookmarks.len().saturating_add(1) {
      return Err(Error::invalid(0, "Plcfbkld cardinality changed"));
    }
    require_nondecreasing(&self.positions, "Plcfbkld CP")?;
    let mut bytes = Vec::new();
    for value in &self.positions {
      push_u32(&mut bytes, *value);
    }
    for value in &self.bookmarks {
      push_u16(&mut bytes, value.start_index);
      push_u16(&mut bytes, value.depth);
    }
    Ok(bytes)
  }
}

impl GrammarCookieStore {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let total = usize::try_from(input.u32()?)
      .map_err(|_| Error::Limit("RgCdb cbTotal exceeds usize".into()))?;
    if total != bytes.len() || total < 8 {
      return Err(Error::invalid(
        0,
        "RgCdb cbTotal does not match its bounded length",
      ));
    }
    let count =
      usize::try_from(input.u32()?).map_err(|_| Error::Limit("RgCdb ccdb exceeds usize".into()))?;
    if count > (bytes.len() - 8) / 4 {
      return Err(Error::invalid(4, "RgCdb ccdb exceeds its bounded length"));
    }
    let mut cookies = Vec::with_capacity(count);
    for _ in 0..count {
      let length = usize::try_from(input.u32()?)
        .map_err(|_| Error::Limit("CDB cbData exceeds usize".into()))?;
      cookies.push(GrammarCookieData {
        provider_data: input.bytes(length)?.to_vec(),
      });
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after RgCdb",
      ));
    }
    Ok(Self { cookies })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let count = u32::try_from(self.cookies.len())
      .map_err(|_| Error::Limit("RgCdb ccdb exceeds u32".into()))?;
    let mut total = 8usize;
    for cookie in &self.cookies {
      u32::try_from(cookie.provider_data.len())
        .map_err(|_| Error::Limit("CDB cbData exceeds u32".into()))?;
      total = total
        .checked_add(4)
        .and_then(|total| total.checked_add(cookie.provider_data.len()))
        .ok_or_else(|| Error::Limit("RgCdb encoded length overflow".into()))?;
    }
    let total_u32 =
      u32::try_from(total).map_err(|_| Error::Limit("RgCdb cbTotal exceeds u32".into()))?;
    let mut bytes = Vec::with_capacity(total);
    push_u32(&mut bytes, total_u32);
    push_u32(&mut bytes, count);
    for cookie in &self.cookies {
      push_u32(
        &mut bytes,
        u32::try_from(cookie.provider_data.len()).expect("CDB length was checked above"),
      );
      bytes.extend_from_slice(&cookie.provider_data);
    }
    Ok(bytes)
  }

  pub fn entry_offsets(&self) -> Result<Vec<u32>> {
    let mut offset = 8usize;
    let mut offsets = Vec::with_capacity(self.cookies.len());
    for cookie in &self.cookies {
      offsets.push(
        u32::try_from(offset).map_err(|_| Error::Limit("RgCdb entry offset exceeds u32".into()))?,
      );
      offset = offset
        .checked_add(4)
        .and_then(|offset| offset.checked_add(cookie.provider_data.len()))
        .ok_or_else(|| Error::Limit("RgCdb entry offset overflow".into()))?;
    }
    Ok(offsets)
  }

  pub fn cookie_at_offset(&self, offset: u32) -> Result<Option<&GrammarCookieData>> {
    Ok(
      self
        .entry_offsets()?
        .into_iter()
        .position(|entry_offset| entry_offset == offset)
        .map(|index| &self.cookies[index]),
    )
  }

  pub fn validate_references(&self, table: &GrammarCheckerCookieTable) -> Result<()> {
    let offsets = self.entry_offsets()?.into_iter().collect::<BTreeSet<_>>();
    if table
      .cookies
      .iter()
      .any(|cookie| !offsets.contains(&cookie.data_offset))
    {
      return Err(Error::invalid(
        0,
        "FCKS icdb does not point to a CDB entry boundary",
      ));
    }
    Ok(())
  }

  pub fn validate_legacy_references(&self, table: &LegacyGrammarCheckerCookieTable) -> Result<()> {
    let offsets = self.entry_offsets()?.into_iter().collect::<BTreeSet<_>>();
    if table
      .cookies
      .iter()
      .any(|cookie| !offsets.contains(&cookie.data_offset))
    {
      return Err(Error::invalid(
        0,
        "FCKSOLD icdb does not point to a CDB entry boundary",
      ));
    }
    Ok(())
  }
}

impl LegacyGrammarCheckerCookieTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(20) {
      return Err(Error::invalid(0, "PlcfcookieOld length is invalid"));
    }
    let count = (bytes.len() - 4) / 20;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut cookies = Vec::with_capacity(count);
    for _ in 0..count {
      let record_offset = input.offset as u64;
      let language_id = input.u16()?;
      let character_count = input.i16()?;
      if character_count < 0 {
        return Err(Error::invalid(record_offset + 2, "FCKSOLD dcp is negative"));
      }
      let sentence_offset = input.i16()?;
      if sentence_offset > 0 {
        return Err(Error::invalid(
          record_offset + 4,
          "FCKSOLD dcpSent is positive",
        ));
      }
      let padding1 = input.u16()?;
      let flags = input.u16()?;
      let padding2 = input.u16()?;
      let data_offset = input.u32()?;
      cookies.push(LegacyGrammarCheckerCookie {
        language_id,
        character_count,
        sentence_offset,
        padding1,
        error_type: GrammarCookieErrorType::from_u8((flags & 3) as u8),
        spare: (flags >> 2) & 0x1fff,
        error: flags & 0x8000 != 0,
        padding2,
        data_offset,
      });
    }
    require_nondecreasing(&positions, "PlcfcookieOld CP")?;
    Ok(Self { positions, cookies })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.cookies.len().saturating_add(1) {
      return Err(Error::invalid(0, "PlcfcookieOld cardinality changed"));
    }
    require_nondecreasing(&self.positions, "PlcfcookieOld CP")?;
    let mut bytes = Vec::with_capacity(
      self
        .cookies
        .len()
        .checked_mul(20)
        .and_then(|length| length.checked_add(4))
        .ok_or_else(|| Error::Limit("PlcfcookieOld encoded length overflow".into()))?,
    );
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for cookie in &self.cookies {
      if cookie.character_count < 0 {
        return Err(Error::invalid(0, "FCKSOLD dcp is negative"));
      }
      if cookie.sentence_offset > 0 {
        return Err(Error::invalid(0, "FCKSOLD dcpSent is positive"));
      }
      if cookie.spare > 0x1fff {
        return Err(Error::invalid(0, "FCKSOLD spare exceeds 13 bits"));
      }
      push_u16(&mut bytes, cookie.language_id);
      bytes.extend_from_slice(&cookie.character_count.to_le_bytes());
      bytes.extend_from_slice(&cookie.sentence_offset.to_le_bytes());
      push_u16(&mut bytes, cookie.padding1);
      push_u16(
        &mut bytes,
        u16::from(cookie.error_type.to_u8())
          | (cookie.spare << 2)
          | (u16::from(cookie.error) << 15),
      );
      push_u16(&mut bytes, cookie.padding2);
      push_u32(&mut bytes, cookie.data_offset);
    }
    Ok(bytes)
  }
}

impl GrammarCheckerCookieTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(14) {
      return Err(Error::invalid(0, "Plcfcookie length is invalid"));
    }
    let count = (bytes.len() - 4) / 14;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut cookies = Vec::with_capacity(count);
    let mut header_languages = BTreeSet::new();
    for _ in 0..count {
      let character_count = input.i16()?;
      let sentence_offset = input.i16()?;
      let data_offset = input.u32()?;
      let flags = input.u16()?;
      let cookie = GrammarCheckerCookie {
        character_count,
        sentence_offset,
        data_offset,
        error_type: GrammarCookieErrorType::from_u8((flags & 3) as u8),
        error: flags & 0x0004 != 0,
        language_sub: ((flags >> 3) & 0x001f) as u8,
        language_primary: ((flags >> 8) & 0x007f) as u8,
        header: flags & 0x8000 != 0,
      };
      if cookie.header && !header_languages.insert((cookie.language_sub, cookie.language_primary)) {
        return Err(Error::invalid(
          input.offset as u64 - 2,
          "Plcfcookie has duplicate grammar-checker header",
        ));
      }
      cookies.push(cookie);
    }
    require_nondecreasing(&positions, "Plcfcookie CP")?;
    Ok(Self { positions, cookies })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.cookies.len().saturating_add(1) {
      return Err(Error::invalid(0, "Plcfcookie cardinality changed"));
    }
    require_nondecreasing(&self.positions, "Plcfcookie CP")?;
    let mut bytes = Vec::new();
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    let mut header_languages = BTreeSet::new();
    for cookie in &self.cookies {
      if cookie.language_sub > 0x1f || cookie.language_primary > 0x7f {
        return Err(Error::invalid(0, "FCKS language field exceeds bit width"));
      }
      if cookie.header && !header_languages.insert((cookie.language_sub, cookie.language_primary)) {
        return Err(Error::invalid(
          0,
          "Plcfcookie has duplicate grammar-checker header",
        ));
      }
      bytes.extend_from_slice(&cookie.character_count.to_le_bytes());
      bytes.extend_from_slice(&cookie.sentence_offset.to_le_bytes());
      push_u32(&mut bytes, cookie.data_offset);
      push_u16(
        &mut bytes,
        u16::from(cookie.error_type.to_u8())
          | (u16::from(cookie.error) << 2)
          | (u16::from(cookie.language_sub) << 3)
          | (u16::from(cookie.language_primary) << 8)
          | (u16::from(cookie.header) << 15),
      );
    }
    Ok(bytes)
  }
}

impl GrammarCookieErrorType {
  fn from_u8(value: u8) -> Self {
    match value {
      0 => Self::Default,
      1 => Self::Typo,
      2 => Self::Homonym,
      _ => Self::Consistency,
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::Default => 0,
      Self::Typo => 1,
      Self::Homonym => 2,
      Self::Consistency => 3,
    }
  }
}

impl SmartTagData {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let factoid_count = usize::try_from(input.u32()?)
      .map_err(|_| Error::Limit("PropertyBagStore factoid count exceeds usize".into()))?;
    if factoid_count > bytes.len() {
      return Err(Error::invalid(
        0,
        "PropertyBagStore factoid count is excessive",
      ));
    }
    let mut factoid_types = Vec::with_capacity(factoid_count);
    let mut factoid_ids = BTreeSet::new();
    let mut property_bag_factoid_ids = BTreeSet::new();
    for _ in 0..factoid_count {
      let value = SmartTagFactoidType::read(&mut input)?;
      if !factoid_ids.insert(value.id) {
        return Err(Error::invalid(0, "FactoidType id is duplicated"));
      }
      if !property_bag_factoid_ids.insert(value.id.property_bag_id()) {
        return Err(Error::invalid(
          0,
          "FactoidType ids have an ambiguous PropertyBag reference",
        ));
      }
      factoid_types.push(value);
    }
    if input.u16()? != 0x000c || input.u16()? != 0x0100 {
      return Err(Error::invalid(
        0,
        "PropertyBagStore header or version is invalid",
      ));
    }
    let reserved_factoid_count = input.u32()?;
    let string_count = usize::try_from(input.u32()?)
      .map_err(|_| Error::Limit("PropertyBagStore string count exceeds usize".into()))?;
    if string_count > bytes.len() {
      return Err(Error::invalid(
        0,
        "PropertyBagStore string count is excessive",
      ));
    }
    let mut strings = Vec::with_capacity(string_count);
    for _ in 0..string_count {
      strings.push(PropertyBagString::read(&mut input)?);
    }
    let mut property_bags = Vec::new();
    while input.offset < bytes.len() {
      let factoid_type_id = input.u16()?;
      let property_count = usize::from(input.u16()?);
      if input.u16()? != 0 {
        return Err(Error::invalid(
          input.offset as u64 - 2,
          "PropertyBag cbUnknown is nonzero",
        ));
      }
      if !property_bag_factoid_ids.contains(&factoid_type_id) {
        return Err(Error::invalid(
          0,
          "PropertyBag references unknown FactoidType",
        ));
      }
      let available = bytes.len().saturating_sub(input.offset) / 8;
      if property_count > available {
        return Err(Error::invalid(0, "PropertyBag property count is truncated"));
      }
      let mut properties = Vec::with_capacity(property_count);
      for _ in 0..property_count {
        let property = SmartTagProperty {
          key_index: input.u32()?,
          value_index: input.u32()?,
        };
        if usize::try_from(property.key_index).unwrap_or(usize::MAX) >= strings.len()
          || usize::try_from(property.value_index).unwrap_or(usize::MAX) >= strings.len()
        {
          return Err(Error::invalid(0, "Property index is outside stringTable"));
        }
        properties.push(property);
      }
      property_bags.push(SmartTagPropertyBag {
        factoid_type_id,
        properties,
      });
    }
    Ok(Self {
      factoid_types,
      reserved_factoid_count,
      strings,
      property_bags,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    push_u32(
      &mut bytes,
      u32::try_from(self.factoid_types.len())
        .map_err(|_| Error::Limit("PropertyBagStore factoid count exceeds u32".into()))?,
    );
    let mut factoid_ids = BTreeSet::new();
    let mut property_bag_factoid_ids = BTreeSet::new();
    for value in &self.factoid_types {
      if !factoid_ids.insert(value.id) {
        return Err(Error::invalid(0, "FactoidType id is duplicated"));
      }
      if !property_bag_factoid_ids.insert(value.id.property_bag_id()) {
        return Err(Error::invalid(
          0,
          "FactoidType ids have an ambiguous PropertyBag reference",
        ));
      }
      value.write(&mut bytes)?;
    }
    push_u16(&mut bytes, 0x000c);
    push_u16(&mut bytes, 0x0100);
    push_u32(&mut bytes, self.reserved_factoid_count);
    push_u32(
      &mut bytes,
      u32::try_from(self.strings.len())
        .map_err(|_| Error::Limit("PropertyBagStore string count exceeds u32".into()))?,
    );
    for value in &self.strings {
      value.write(&mut bytes)?;
    }
    for bag in &self.property_bags {
      if !property_bag_factoid_ids.contains(&bag.factoid_type_id) {
        return Err(Error::invalid(
          0,
          "PropertyBag references unknown FactoidType",
        ));
      }
      push_u16(&mut bytes, bag.factoid_type_id);
      push_u16(
        &mut bytes,
        u16::try_from(bag.properties.len())
          .map_err(|_| Error::Limit("PropertyBag property count exceeds u16".into()))?,
      );
      push_u16(&mut bytes, 0);
      for property in &bag.properties {
        if usize::try_from(property.key_index).unwrap_or(usize::MAX) >= self.strings.len()
          || usize::try_from(property.value_index).unwrap_or(usize::MAX) >= self.strings.len()
        {
          return Err(Error::invalid(0, "Property index is outside stringTable"));
        }
        push_u32(&mut bytes, property.key_index);
        push_u32(&mut bytes, property.value_index);
      }
    }
    Ok(bytes)
  }
}

impl SmartTagFactoidType {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let size = usize::try_from(input.u32()?)
      .map_err(|_| Error::Limit("FactoidType size exceeds usize".into()))?;
    let body = input.bytes(size)?;
    let mut body = SliceReader::new(body);
    let id = SmartTagFactoidTypeId::from_u32(body.u32()?)?;
    let value = Self {
      id,
      uri: PropertyBagString::read(&mut body)?,
      tag: PropertyBagString::read(&mut body)?,
      download_url: PropertyBagString::read(&mut body)?,
    };
    if body.offset != body.bytes.len() {
      return Err(Error::invalid(
        body.offset as u64,
        "FactoidType size mismatch",
      ));
    }
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    let mut body = Vec::new();
    push_u32(&mut body, self.id.to_u32());
    self.uri.write(&mut body)?;
    self.tag.write(&mut body)?;
    self.download_url.write(&mut body)?;
    push_u32(
      bytes,
      u32::try_from(body.len()).map_err(|_| Error::Limit("FactoidType size exceeds u32".into()))?,
    );
    bytes.extend_from_slice(&body);
    Ok(())
  }
}

impl PropertyBagString {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let header = input.u16()?;
    let count = usize::from(header & 0x7fff);
    if header & 0x8000 != 0 {
      Ok(Self::Ansi(input.bytes(count)?.to_vec()))
    } else {
      let mut value = Vec::with_capacity(count);
      for _ in 0..count {
        value.push(input.u16()?);
      }
      Ok(Self::Unicode(value))
    }
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    match self {
      Self::Ansi(value) => {
        if value.len() > 0x7fff {
          return Err(Error::Limit("ANSI PBString exceeds 0x7fff".into()));
        }
        push_u16(bytes, 0x8000 | value.len() as u16);
        bytes.extend_from_slice(value);
      }
      Self::Unicode(value) => {
        if value.len() > 0x7fff {
          return Err(Error::Limit("Unicode PBString exceeds 0x7fff".into()));
        }
        push_u16(bytes, value.len() as u16);
        write_u16_array(bytes, value);
      }
    }
    Ok(())
  }
}

impl TableCharacterCacheTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(8) {
      return Err(Error::invalid(
        0,
        "PlcfTch length does not match 4-byte Tch records",
      ));
    }
    let count = (bytes.len() - 4) / 8;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(count + 1);
    for _ in 0..=count {
      positions.push(input.u32()?);
    }
    let mut caches = Vec::with_capacity(count);
    for _ in 0..count {
      let value = input.u32()?;
      caches.push(TableCharacterCache {
        unknown: value & 1 != 0,
        unused: value >> 1,
      });
    }
    require_strictly_increasing(&positions, "PlcfTch CP")?;
    Ok(Self { positions, caches })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.caches.len().saturating_add(1) {
      return Err(Error::invalid(0, "PlcfTch CP/Tch cardinality changed"));
    }
    require_strictly_increasing(&self.positions, "PlcfTch CP")?;
    let mut bytes = Vec::with_capacity(self.positions.len() * 4 + self.caches.len() * 4);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for cache in &self.caches {
      if cache.unused > 0x7fff_ffff {
        return Err(Error::invalid(0, "Tch unused field exceeds 31 bits"));
      }
      push_u32(&mut bytes, u32::from(cache.unknown) | (cache.unused << 1));
    }
    Ok(bytes)
  }
}

impl RevisionMessageThreading {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);

    let message_count = read_threading_sttb_header(&mut input, 8, "SttbMessage")?;
    let mut messages = Vec::with_capacity(message_count);
    for _ in 0..message_count {
      messages.push(RevisionThreadMessage {
        identifier: read_threading_string(&mut input)?,
        display: MessageDisplayProperties {
          created: Dttm::from_u32(input.u32()?)?,
          reserved: input.u16()?,
          author_index: input.i16()?,
        },
      });
    }

    let style_count = read_threading_sttb_header(&mut input, 0, "SttbStyle")?;
    let mut styles = Vec::with_capacity(style_count);
    for _ in 0..style_count {
      styles.push(read_threading_string(&mut input)?);
    }

    let author_attribute_count = read_threading_sttb_header(&mut input, 2, "SttbAuthorAttrib")?;
    let mut author_attributes = Vec::with_capacity(author_attribute_count);
    for _ in 0..author_attribute_count {
      author_attributes.push(RevisionThreadAttribute {
        name: read_threading_string(&mut input)?,
        target_index: input.i16()?,
      });
    }

    let author_value_count = read_threading_sttb_header(&mut input, 0, "SttbAuthorValue")?;
    let mut author_values = Vec::with_capacity(author_value_count);
    for _ in 0..author_value_count {
      author_values.push(read_threading_string(&mut input)?);
    }

    let message_attribute_count = read_threading_sttb_header(&mut input, 2, "SttbMessageAttrib")?;
    let mut message_attributes = Vec::with_capacity(message_attribute_count);
    for _ in 0..message_attribute_count {
      message_attributes.push(RevisionThreadAttribute {
        name: read_threading_string(&mut input)?,
        target_index: input.i16()?,
      });
    }

    let message_value_count = read_threading_sttb_header(&mut input, 0, "SttbMessageValue")?;
    let mut message_values = Vec::with_capacity(message_value_count);
    for _ in 0..message_value_count {
      message_values.push(read_threading_string(&mut input)?);
    }

    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after RmdThreading",
      ));
    }
    let value = Self {
      messages,
      styles,
      author_attributes,
      author_values,
      message_attributes,
      message_values,
    };
    value.validate()?;
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.validate()?;
    let mut bytes = Vec::new();

    write_threading_sttb_header(&mut bytes, self.messages.len(), 8, "SttbMessage")?;
    for message in &self.messages {
      write_threading_string(&mut bytes, &message.identifier)?;
      push_u32(&mut bytes, message.display.created.to_u32()?);
      push_u16(&mut bytes, message.display.reserved);
      bytes.extend_from_slice(&message.display.author_index.to_le_bytes());
    }

    write_threading_string_table(&mut bytes, &self.styles, "SttbStyle")?;

    write_threading_sttb_header(
      &mut bytes,
      self.author_attributes.len(),
      2,
      "SttbAuthorAttrib",
    )?;
    for attribute in &self.author_attributes {
      write_threading_string(&mut bytes, &attribute.name)?;
      bytes.extend_from_slice(&attribute.target_index.to_le_bytes());
    }

    write_threading_string_table(&mut bytes, &self.author_values, "SttbAuthorValue")?;

    write_threading_sttb_header(
      &mut bytes,
      self.message_attributes.len(),
      2,
      "SttbMessageAttrib",
    )?;
    for attribute in &self.message_attributes {
      write_threading_string(&mut bytes, &attribute.name)?;
      bytes.extend_from_slice(&attribute.target_index.to_le_bytes());
    }

    write_threading_string_table(&mut bytes, &self.message_values, "SttbMessageValue")?;
    Ok(bytes)
  }

  fn validate(&self) -> Result<()> {
    if self.messages.len() != self.styles.len() {
      return Err(Error::invalid(
        0,
        "RmdThreading message/style cardinality differs",
      ));
    }
    if self.author_attributes.len() != self.author_values.len() {
      return Err(Error::invalid(
        0,
        "RmdThreading author attribute/value cardinality differs",
      ));
    }
    if self.message_attributes.len() != self.message_values.len() {
      return Err(Error::invalid(
        0,
        "RmdThreading message attribute/value cardinality differs",
      ));
    }
    for message in &self.messages {
      if !message.identifier.is_empty() && message.display.reserved != 0 {
        return Err(Error::invalid(
          0,
          "nonempty SttbMessage entry has nonzero MDP reserved field",
        ));
      }
    }
    for attribute in &self.author_attributes {
      if !attribute.name.is_empty() && attribute.target_index < 0 {
        return Err(Error::invalid(
          0,
          "nonempty author attribute has a negative author index",
        ));
      }
    }
    for attribute in &self.message_attributes {
      if !attribute.name.is_empty()
        && usize::try_from(attribute.target_index)
          .map_or(true, |index| index >= self.messages.len())
      {
        return Err(Error::invalid(
          0,
          "nonempty message attribute index exceeds SttbMessage",
        ));
      }
    }
    Ok(())
  }
}

impl RevisionSaveIdTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 24 || !(bytes.len() - 24).is_multiple_of(4) {
      return Err(Error::invalid(0, "PLRSID physical length is invalid"));
    }
    let mut input = SliceReader::new(bytes);
    let count = usize::try_from(input.u32()?)
      .map_err(|_| Error::Limit("PLRSID count exceeds usize".into()))?;
    if input.u32()? != 4 || input.u32()? != 8 || input.u32()? != 229 {
      return Err(Error::invalid(4, "PLRSID fixed header values are invalid"));
    }
    let reserved2 = input.u32()?;
    if reserved2 >= 32 {
      return Err(Error::invalid(16, "PLRSID reserved2 is not less than 32"));
    }
    let reserved3 = input.u32()?;
    if count != (bytes.len() - 24) / 4 {
      return Err(Error::invalid(0, "PLRSID count does not match its length"));
    }
    let mut ids = Vec::with_capacity(count);
    for _ in 0..count {
      ids.push(RevisionSaveId(input.u32()?));
    }
    Ok(Self {
      reserved2,
      reserved3,
      ids,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.reserved2 >= 32 {
      return Err(Error::invalid(0, "PLRSID reserved2 is not less than 32"));
    }
    let capacity = self
      .ids
      .len()
      .checked_mul(4)
      .and_then(|length| length.checked_add(24))
      .ok_or_else(|| Error::Limit("PLRSID encoded length overflow".into()))?;
    let mut bytes = Vec::with_capacity(capacity);
    push_u32(
      &mut bytes,
      u32::try_from(self.ids.len()).map_err(|_| Error::Limit("PLRSID count exceeds u32".into()))?,
    );
    push_u32(&mut bytes, 4);
    push_u32(&mut bytes, 8);
    push_u32(&mut bytes, 229);
    push_u32(&mut bytes, self.reserved2);
    push_u32(&mut bytes, self.reserved3);
    for id in &self.ids {
      push_u32(&mut bytes, id.0);
    }
    Ok(bytes)
  }
}

impl Dttm {
  pub fn is_ignored(self) -> bool {
    self.day == 0 || self.month == 0
  }

  pub fn from_u32(value: u32) -> Result<Self> {
    let result = Self {
      minute: (value & 0x3f) as u8,
      hour: ((value >> 6) & 0x1f) as u8,
      day: ((value >> 11) & 0x1f) as u8,
      month: ((value >> 16) & 0x0f) as u8,
      year_offset: ((value >> 20) & 0x01ff) as u16,
      weekday: ((value >> 29) & 0x07) as u8,
    };
    result.validate()?;
    Ok(result)
  }

  pub fn to_u32(self) -> Result<u32> {
    self.validate()?;
    Ok(
      u32::from(self.minute)
        | (u32::from(self.hour) << 6)
        | (u32::from(self.day) << 11)
        | (u32::from(self.month) << 16)
        | (u32::from(self.year_offset) << 20)
        | (u32::from(self.weekday) << 29),
    )
  }

  pub fn validate(self) -> Result<()> {
    if self.minute > 59
      || self.hour > 23
      || self.day > 31
      || self.month > 12
      || self.year_offset > 0x01ff
      || self.weekday > 6
    {
      return Err(Error::invalid(0, "DTTM field is outside its valid range"));
    }
    Ok(())
  }
}

fn read_threading_sttb_header(
  input: &mut SliceReader<'_>,
  expected_extra: u16,
  name: &str,
) -> Result<usize> {
  if input.u16()? != 0xffff {
    return Err(Error::invalid(
      input.offset.saturating_sub(2) as u64,
      format!("{name} is not an extended STTB"),
    ));
  }
  let count = usize::from(input.u16()?);
  if input.u16()? != expected_extra {
    return Err(Error::invalid(
      input.offset.saturating_sub(2) as u64,
      format!("{name} cbExtra is not {expected_extra}"),
    ));
  }
  Ok(count)
}

fn read_threading_string(input: &mut SliceReader<'_>) -> Result<Vec<u16>> {
  let length = usize::from(input.u16()?);
  let mut value = Vec::with_capacity(length);
  for _ in 0..length {
    value.push(input.u16()?);
  }
  Ok(value)
}

fn write_threading_sttb_header(
  bytes: &mut Vec<u8>,
  count: usize,
  extra: u16,
  name: &str,
) -> Result<()> {
  push_u16(bytes, 0xffff);
  push_u16(
    bytes,
    u16::try_from(count).map_err(|_| Error::Limit(format!("{name} count exceeds u16")))?,
  );
  push_u16(bytes, extra);
  Ok(())
}

fn write_threading_string(bytes: &mut Vec<u8>, value: &[u16]) -> Result<()> {
  push_u16(
    bytes,
    u16::try_from(value.len())
      .map_err(|_| Error::Limit("RmdThreading string exceeds u16".into()))?,
  );
  write_u16_array(bytes, value);
  Ok(())
}

fn write_threading_string_table(
  bytes: &mut Vec<u8>,
  values: &[Vec<u16>],
  name: &str,
) -> Result<()> {
  write_threading_sttb_header(bytes, values.len(), 0, name)?;
  for value in values {
    write_threading_string(bytes, value)?;
  }
  Ok(())
}

impl SelectionState {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if !matches!(bytes.len(), 36 | 44) {
      return Err(Error::invalid(
        0,
        format!("unsupported Selsf length {}", bytes.len()),
      ));
    }
    let mut input = SliceReader::new(bytes);
    let flags = SelectionFlags::from_u32(input.u32()?);
    let first_character = input.i32()?;
    let character_limit = input.i32()?;
    let unused4 = input.u32()?;
    let range_value = input.u32()?;
    let range = if flags.table {
      SelectionRange::Table {
        first_cell: range_value as u16 as i16,
        limit_cell: (range_value >> 16) as u16 as i16,
      }
    } else if flags.block {
      SelectionRange::Block {
        first_pixel: range_value as u16 as i16,
        limit_pixel: (range_value >> 16) as u16 as i16,
      }
    } else {
      SelectionRange::Unused(range_value)
    };
    let anchor_character = input.i32()?;
    let style = SelectionStyle::from_u16(input.u16()?);
    let unused5 = input.u16()?;
    let shrink_anchor_character = input.i32()?;
    let table_left = input.i16()?;
    let table_right = input.i16()?;
    let extension = if bytes.len() == 44 {
      SelectionStateExtension::Compatibility([input.u32()?, input.u32()?])
    } else {
      SelectionStateExtension::None
    };
    Ok(Self {
      flags,
      first_character,
      character_limit,
      unused4,
      range,
      anchor_character,
      style,
      unused5,
      shrink_anchor_character,
      table_left,
      table_right,
      extension,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let range = match (self.flags.table, self.flags.block, self.range) {
      (
        true,
        _,
        SelectionRange::Table {
          first_cell,
          limit_cell,
        },
      ) => u32::from(first_cell as u16) | (u32::from(limit_cell as u16) << 16),
      (
        false,
        true,
        SelectionRange::Block {
          first_pixel,
          limit_pixel,
        },
      ) => u32::from(first_pixel as u16) | (u32::from(limit_pixel as u16) << 16),
      (false, false, SelectionRange::Unused(value)) => value,
      _ => {
        return Err(Error::invalid(
          16,
          "Selsf range variant disagrees with flags",
        ));
      }
    };
    let mut bytes = Vec::with_capacity(match self.extension {
      SelectionStateExtension::None => 36,
      SelectionStateExtension::Compatibility(_) => 44,
    });
    push_u32(&mut bytes, self.flags.to_u32());
    push_i32(&mut bytes, self.first_character);
    push_i32(&mut bytes, self.character_limit);
    push_u32(&mut bytes, self.unused4);
    push_u32(&mut bytes, range);
    push_i32(&mut bytes, self.anchor_character);
    push_u16(&mut bytes, self.style.to_u16());
    push_u16(&mut bytes, self.unused5);
    push_i32(&mut bytes, self.shrink_anchor_character);
    bytes.extend_from_slice(&self.table_left.to_le_bytes());
    bytes.extend_from_slice(&self.table_right.to_le_bytes());
    if let SelectionStateExtension::Compatibility(words) = self.extension {
      for word in words {
        push_u32(&mut bytes, word);
      }
    }
    Ok(bytes)
  }
}

impl SelectionFlags {
  fn from_u32(value: u32) -> Self {
    Self {
      rightward: value & (1 << 0) != 0,
      unused1: value & (1 << 1) != 0,
      within_cell: value & (1 << 2) != 0,
      table_anchor: value & (1 << 3) != 0,
      table_selection_non_shrink: value & (1 << 4) != 0,
      unused2: value & (1 << 5) != 0,
      discontiguous: value & (1 << 6) != 0,
      prefix: value & (1 << 7) != 0,
      shape: value & (1 << 8) != 0,
      frame: value & (1 << 9) != 0,
      column: value & (1 << 10) != 0,
      table: value & (1 << 11) != 0,
      graphics: value & (1 << 12) != 0,
      block: value & (1 << 13) != 0,
      unused3: value & (1 << 14) != 0,
      insertion_point: value & (1 << 15) != 0,
      forward: ((value >> 16) & 0x7f) as u8,
      prefix_word2007: value & (1 << 23) != 0,
      insertion_at_line_end: (value >> 24) as u8,
    }
  }

  fn to_u32(self) -> u32 {
    u32::from(self.rightward)
      | (u32::from(self.unused1) << 1)
      | (u32::from(self.within_cell) << 2)
      | (u32::from(self.table_anchor) << 3)
      | (u32::from(self.table_selection_non_shrink) << 4)
      | (u32::from(self.unused2) << 5)
      | (u32::from(self.discontiguous) << 6)
      | (u32::from(self.prefix) << 7)
      | (u32::from(self.shape) << 8)
      | (u32::from(self.frame) << 9)
      | (u32::from(self.column) << 10)
      | (u32::from(self.table) << 11)
      | (u32::from(self.graphics) << 12)
      | (u32::from(self.block) << 13)
      | (u32::from(self.unused3) << 14)
      | (u32::from(self.insertion_point) << 15)
      | (u32::from(self.forward & 0x7f) << 16)
      | (u32::from(self.prefix_word2007) << 23)
      | (u32::from(self.insertion_at_line_end) << 24)
  }
}

impl SelectionStyle {
  fn from_u16(value: u16) -> Self {
    match value {
      0x0000 => Self::Undefined,
      0x0001 => Self::Character,
      0x0002 => Self::Word,
      0x0003 => Self::Sentence,
      0x0004 => Self::Paragraph,
      0x0005 => Self::Line,
      0x000c => Self::Column,
      0x000d => Self::Row,
      0x000e => Self::AllColumns,
      0x000f => Self::WholeTable,
      0x001b => Self::Prefix,
      value => Self::Compatibility(value),
    }
  }

  fn to_u16(self) -> u16 {
    match self {
      Self::Undefined => 0x0000,
      Self::Character => 0x0001,
      Self::Word => 0x0002,
      Self::Sentence => 0x0003,
      Self::Paragraph => 0x0004,
      Self::Line => 0x0005,
      Self::Column => 0x000c,
      Self::Row => 0x000d,
      Self::AllColumns => 0x000e,
      Self::WholeTable => 0x000f,
      Self::Prefix => 0x001b,
      Self::Compatibility(value) => value,
    }
  }
}

impl CommandCustomizations {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    if input.u8()? != 0xff {
      return Err(Error::invalid(0, "Tcg.nTcgVer is not 255"));
    }
    let mut records = Vec::new();
    loop {
      let id = input.u8()?;
      match id {
        0x40 => break,
        0x01 => {
          let count = read_nonnegative_i32_count(&mut input, "PlfMcd")?;
          let mut values = Vec::with_capacity(count);
          for _ in 0..count {
            values.push(MacroCommandDescriptor {
              reserved1: input.u8()? as i8,
              reserved2: input.u8()?,
              macro_name_index: input.u16()?,
              command_string_index: input.u16()?,
              reserved3: input.u16()?,
              reserved4: input.u32()?,
              reserved5: input.u32()?,
              reserved6: input.u32()?,
              reserved7: input.u32()?,
            });
          }
          records.push(CommandCustomizationRecord::MacroCommands(values));
        }
        0x10 => {
          if input.u16()? != 0xffff {
            return Err(Error::invalid(
              input.offset as u64 - 2,
              "TcgSttbf is not UTF-16",
            ));
          }
          let count = usize::from(input.u16()?);
          if input.u16()? != 2 {
            return Err(Error::invalid(
              input.offset as u64 - 2,
              "TcgSttbf cbExtra is not 2",
            ));
          }
          let mut values = Vec::with_capacity(count);
          for _ in 0..count {
            let length = usize::from(input.u16()?);
            let mut value = Vec::with_capacity(length);
            for _ in 0..length {
              value.push(input.u16()?);
            }
            values.push(CommandString {
              value,
              reference_count: input.u16()?,
            });
          }
          records.push(CommandCustomizationRecord::CommandStrings(values));
        }
        0x11 => {
          let count = usize::from(input.u16()?);
          let mut values = Vec::with_capacity(count);
          for _ in 0..count {
            values.push(MacroName {
              index: input.u16()?,
              value: Xstz::read(&mut input)?,
            });
          }
          records.push(CommandCustomizationRecord::MacroNames(values));
        }
        0x12 => records.push(CommandCustomizationRecord::Toolbar(ToolbarWrapper::read(
          &mut input,
        )?)),
        value => {
          return Err(Error::invalid(
            input.offset as u64 - 1,
            format!("unsupported Tcg255 record {value:#04x}"),
          ));
        }
      }
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after Tcg255 terminator",
      ));
    }
    Ok(Self { records })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = vec![0xff];
    for record in &self.records {
      match record {
        CommandCustomizationRecord::MacroCommands(values) => {
          bytes.push(0x01);
          push_i32(
            &mut bytes,
            i32::try_from(values.len())
              .map_err(|_| Error::Limit("PlfMcd count exceeds i32".into()))?,
          );
          for value in values {
            bytes.extend_from_slice(&[value.reserved1 as u8, value.reserved2]);
            push_u16(&mut bytes, value.macro_name_index);
            push_u16(&mut bytes, value.command_string_index);
            push_u16(&mut bytes, value.reserved3);
            for word in [
              value.reserved4,
              value.reserved5,
              value.reserved6,
              value.reserved7,
            ] {
              push_u32(&mut bytes, word);
            }
          }
        }
        CommandCustomizationRecord::CommandStrings(values) => {
          bytes.push(0x10);
          push_u16(&mut bytes, 0xffff);
          push_u16(
            &mut bytes,
            u16::try_from(values.len())
              .map_err(|_| Error::Limit("TcgSttbf count exceeds u16".into()))?,
          );
          push_u16(&mut bytes, 2);
          for value in values {
            push_u16(
              &mut bytes,
              u16::try_from(value.value.len())
                .map_err(|_| Error::Limit("Tcg command string exceeds u16".into()))?,
            );
            write_u16_array(&mut bytes, &value.value);
            push_u16(&mut bytes, value.reference_count);
          }
        }
        CommandCustomizationRecord::MacroNames(values) => {
          bytes.push(0x11);
          push_u16(
            &mut bytes,
            u16::try_from(values.len())
              .map_err(|_| Error::Limit("MacroNames count exceeds u16".into()))?,
          );
          for value in values {
            push_u16(&mut bytes, value.index);
            value.value.write(&mut bytes)?;
          }
        }
        CommandCustomizationRecord::Toolbar(value) => {
          bytes.push(0x12);
          value.write(&mut bytes)?;
        }
      }
    }
    bytes.push(0x40);
    Ok(bytes)
  }
}

impl ToolbarWrapper {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let reserved2 = input.u16()?;
    let reserved3 = input.u8()?;
    let reserved4 = input.u16()?;
    let reserved5 = input.u16()?;
    let toolbar_delta_size = input.i16()?;
    let customization_count = usize::from(input.u16()?);
    let control_bytes = read_nonnegative_i32_count(input, "CTBWRAPPER.cbDTBC")?;
    let physical_controls = input.bytes(control_bytes)?.to_vec();
    let mut customizations = Vec::with_capacity(customization_count);
    for _ in 0..customization_count {
      let toolbar_id = input.i32()?;
      let reserved = input.u16()?;
      let delta_count = usize::from(input.u16()?);
      if toolbar_id == 0 {
        if delta_count != 0 {
          return Err(Error::invalid(
            input.offset as u64 - 2,
            "custom CTB has nonzero ctbds",
          ));
        }
        customizations.push(ToolbarCustomization {
          toolbar_id,
          reserved,
          deltas: Vec::new(),
          custom_toolbar: Some(CustomToolbar::read(input)?),
        });
        continue;
      }
      let mut deltas = Vec::with_capacity(delta_count);
      for _ in 0..delta_count {
        let flags = input.u8()?;
        deltas.push(ToolbarDelta {
          operation: flags & 0x03,
          at_end: flags & 0x04 != 0,
          reserved: flags >> 3,
          control_index: input.u8()?,
          next_command_id: input.i32()?,
          command_id: input.i32()?,
          file_offset: input.i32()?,
          toolbar_index_flags: input.u16()?,
          control_byte_count: input.u16()?,
        });
      }
      customizations.push(ToolbarCustomization {
        toolbar_id,
        reserved,
        deltas,
        custom_toolbar: None,
      });
    }
    let mut controls = Vec::new();
    let mut offset = 0usize;
    for delta in customizations.iter().flat_map(|value| &value.deltas) {
      let end = offset
        .checked_add(usize::from(delta.control_byte_count))
        .ok_or_else(|| Error::invalid(0, "CTBWRAPPER control size overflow"))?;
      let bytes = physical_controls
        .get(offset..end)
        .ok_or_else(|| Error::invalid(offset as u64, "TBDelta.cbTBC exceeds rtbdc"))?;
      controls.push(ToolbarControl::from_bytes(bytes)?);
      offset = end;
    }
    if offset != physical_controls.len() {
      return Err(Error::invalid(
        offset as u64,
        "TBDelta.cbTBC values do not cover rtbdc",
      ));
    }
    Ok(Self {
      reserved2,
      reserved3,
      reserved4,
      reserved5,
      toolbar_delta_size,
      controls,
      customizations,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    let deltas = self
      .customizations
      .iter()
      .flat_map(|value| &value.deltas)
      .collect::<Vec<_>>();
    if deltas.len() != self.controls.len() {
      return Err(Error::invalid(0, "CTBWRAPPER control/delta count mismatch"));
    }
    push_u16(bytes, self.reserved2);
    bytes.push(self.reserved3);
    push_u16(bytes, self.reserved4);
    push_u16(bytes, self.reserved5);
    bytes.extend_from_slice(&self.toolbar_delta_size.to_le_bytes());
    push_u16(
      bytes,
      u16::try_from(self.customizations.len())
        .map_err(|_| Error::Limit("CTBWRAPPER customization count exceeds u16".into()))?,
    );
    let encoded_controls = self
      .controls
      .iter()
      .map(ToolbarControl::to_bytes)
      .collect::<Result<Vec<_>>>()?;
    let control_byte_count = encoded_controls.iter().try_fold(0usize, |total, value| {
      total
        .checked_add(value.len())
        .ok_or_else(|| Error::invalid(0, "CTBWRAPPER control size overflow"))
    })?;
    push_i32(
      bytes,
      i32::try_from(control_byte_count)
        .map_err(|_| Error::Limit("CTBWRAPPER controls exceed i32".into()))?,
    );
    for (control, delta) in encoded_controls.iter().zip(deltas) {
      if control.len() != usize::from(delta.control_byte_count) {
        return Err(Error::invalid(
          0,
          "Toolbar control length disagrees with TBDelta",
        ));
      }
      bytes.extend_from_slice(control);
    }
    for customization in &self.customizations {
      push_i32(bytes, customization.toolbar_id);
      push_u16(bytes, customization.reserved);
      if customization.toolbar_id == 0 {
        if !customization.deltas.is_empty() {
          return Err(Error::invalid(0, "custom CTB contains toolbar deltas"));
        }
        push_u16(bytes, 0);
        customization
          .custom_toolbar
          .as_ref()
          .ok_or_else(|| Error::invalid(0, "custom CTB data is missing"))?
          .write(bytes)?;
        continue;
      }
      if customization.custom_toolbar.is_some() {
        return Err(Error::invalid(
          0,
          "toolbar delta customization contains CTB data",
        ));
      }
      push_u16(
        bytes,
        u16::try_from(customization.deltas.len())
          .map_err(|_| Error::Limit("Toolbar delta count exceeds u16".into()))?,
      );
      for delta in &customization.deltas {
        bytes
          .push((delta.operation & 0x03) | (u8::from(delta.at_end) << 2) | (delta.reserved << 3));
        bytes.push(delta.control_index);
        push_i32(bytes, delta.next_command_id);
        push_i32(bytes, delta.command_id);
        push_i32(bytes, delta.file_offset);
        push_u16(bytes, delta.toolbar_index_flags);
        push_u16(bytes, delta.control_byte_count);
      }
    }
    Ok(())
  }
}

impl CustomToolbar {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let name_length = usize::from(input.u16()?);
    let mut name = Vec::with_capacity(name_length);
    for _ in 0..name_length {
      name.push(input.u16()?);
    }
    let declared_toolbar_data_size = input.i32()?;
    let toolbar = ToolbarData::read(input)?;
    let mut visuals = Vec::with_capacity(5);
    for _ in 0..5 {
      visuals.push(ToolbarVisualData::read(input)?);
    }
    let visual_data: [ToolbarVisualData; 5] = visuals
      .try_into()
      .expect("exactly five toolbar visual records");
    let customization_index = input.i32()?;
    let reserved = input.u16()?;
    let unused = input.u16()?;
    let control_count = read_nonnegative_i32_count(input, "CTB.cCtls")?;
    let mut controls = Vec::with_capacity(control_count);
    for _ in 0..control_count {
      controls.push(ToolbarControl::read(input)?);
    }
    let value = Self {
      name,
      declared_toolbar_data_size,
      toolbar,
      visual_data,
      customization_index,
      reserved,
      unused,
      controls,
    };
    value.validate_declared_size()?;
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate_declared_size()?;
    push_u16(
      bytes,
      u16::try_from(self.name.len()).map_err(|_| Error::Limit("CTB name exceeds u16".into()))?,
    );
    write_u16_array(bytes, &self.name);
    push_i32(bytes, self.declared_toolbar_data_size);
    self.toolbar.write(bytes)?;
    for visual in self.visual_data {
      visual.write(bytes);
    }
    push_i32(bytes, self.customization_index);
    push_u16(bytes, self.reserved);
    push_u16(bytes, self.unused);
    push_i32(
      bytes,
      i32::try_from(self.controls.len())
        .map_err(|_| Error::Limit("CTB controls exceed i32".into()))?,
    );
    for control in &self.controls {
      bytes.extend_from_slice(&control.to_bytes()?);
    }
    Ok(())
  }

  fn validate_declared_size(&self) -> Result<()> {
    let toolbar_size = self.toolbar.encoded_len()?;
    let expected = 112usize
      .checked_add(toolbar_size)
      .ok_or_else(|| Error::Limit("CTB toolbar data size overflow".into()))?;
    if usize::try_from(self.declared_toolbar_data_size).ok() != Some(expected) {
      return Err(Error::invalid(
        0,
        format!(
          "CTB cbTBData is {}, expected {expected}",
          self.declared_toolbar_data_size
        ),
      ));
    }
    Ok(())
  }
}

impl ToolbarData {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    Ok(Self {
      signature: input.u8()? as i8,
      version: input.u8()? as i8,
      declared_control_count: input.i16()?,
      toolbar_id: input.i32()?,
      type_restrictions: input.u32()?,
      default_rows: input.u16()?,
      flags: input.u16()?,
      name: read_toolbar_string(input)?,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    bytes.extend_from_slice(&[self.signature as u8, self.version as u8]);
    bytes.extend_from_slice(&self.declared_control_count.to_le_bytes());
    push_i32(bytes, self.toolbar_id);
    push_u32(bytes, self.type_restrictions);
    push_u16(bytes, self.default_rows);
    push_u16(bytes, self.flags);
    write_toolbar_string(bytes, &self.name)
  }

  fn encoded_len(&self) -> Result<usize> {
    let name_bytes = self
      .name
      .len()
      .checked_mul(2)
      .and_then(|length| length.checked_add(1))
      .ok_or_else(|| Error::Limit("toolbar name length overflow".into()))?;
    16usize
      .checked_add(name_bytes)
      .ok_or_else(|| Error::Limit("toolbar data length overflow".into()))
  }
}

impl ToolbarVisualData {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    Ok(Self {
      dock_state: input.u8()? as i8,
      visibility: input.u8()? as i8,
      last_dock_state: input.u8()? as i8,
      row: input.u8()? as i8,
      docked: ToolbarRectangle::read(input)?,
      floating: ToolbarRectangle::read(input)?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&[
      self.dock_state as u8,
      self.visibility as u8,
      self.last_dock_state as u8,
      self.row as u8,
    ]);
    self.docked.write(bytes);
    self.floating.write(bytes);
  }
}

impl ToolbarRectangle {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    Ok(Self {
      left: input.i16()?,
      top: input.i16()?,
      right: input.i16()?,
      bottom: input.i16()?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) {
    for value in [self.left, self.top, self.right, self.bottom] {
      bytes.extend_from_slice(&value.to_le_bytes());
    }
  }
}

impl ToolbarControl {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let value = Self::read(&mut input)?;
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after toolbar control",
      ));
    }
    Ok(value)
  }

  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let flags = input.u8()? as i8;
    let version = input.u8()? as i8;
    let header_flags = input.u8()?;
    let control_type = input.u8()?;
    let control_id = input.u16()?;
    let specific_flags = input.u32()?;
    let priority = input.u8()?;
    let size = if header_flags & 0x10 != 0 {
      Some((input.u16()?, input.u16()?))
    } else {
      None
    };
    let header = ToolbarControlHeader {
      signature: flags,
      version,
      flags: header_flags,
      control_type,
      control_id,
      specific_flags,
      priority,
      size,
    };
    let command_id = if !matches!(control_id, 0x0001 | 0x1051) {
      Some(input.u32()?)
    } else {
      None
    };
    let data = if control_type == 0x16 {
      None
    } else {
      let general_flags = input.u8()?;
      let custom_text = if general_flags & 0x01 != 0 {
        Some(read_toolbar_string(input)?)
      } else {
        None
      };
      let (description, tooltip) = if general_flags & 0x02 != 0 {
        (
          Some(read_toolbar_string(input)?),
          Some(read_toolbar_string(input)?),
        )
      } else {
        (None, None)
      };
      let extra = if general_flags & 0x04 != 0 {
        Some(ToolbarControlExtraInfo {
          help_file: read_toolbar_string(input)?,
          help_context_id: input.i32()?,
          tag: read_toolbar_string(input)?,
          on_action: read_toolbar_string(input)?,
          parameter: read_toolbar_string(input)?,
          toolbar_control_user: input.u8()? as i8,
          toolbar_control_modified: input.u8()? as i8,
        })
      } else {
        None
      };
      let specific = match control_type {
        0x0a | 0x0c | 0x0d | 0x0e => {
          let toolbar_id = input.i32()?;
          let name = if toolbar_id == 1 {
            Some(read_toolbar_string(input)?)
          } else {
            None
          };
          ToolbarControlSpecific::Menu { toolbar_id, name }
        }
        value => {
          return Err(Error::invalid(
            input.offset as u64,
            format!("unsupported toolbar control type {value:#04x}"),
          ));
        }
      };
      Some(ToolbarControlData {
        general: ToolbarControlGeneralInfo {
          flags: general_flags,
          custom_text,
          description,
          tooltip,
          extra,
        },
        specific,
      })
    };
    Ok(Self {
      header,
      command_id,
      data,
    })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = vec![
      self.header.signature as u8,
      self.header.version as u8,
      self.header.flags,
      self.header.control_type,
    ];
    push_u16(&mut bytes, self.header.control_id);
    push_u32(&mut bytes, self.header.specific_flags);
    bytes.push(self.header.priority);
    if let Some((width, height)) = self.header.size {
      if self.header.flags & 0x10 == 0 {
        return Err(Error::invalid(2, "toolbar size exists without fSaveDxy"));
      }
      push_u16(&mut bytes, width);
      push_u16(&mut bytes, height);
    } else if self.header.flags & 0x10 != 0 {
      return Err(Error::invalid(2, "toolbar fSaveDxy has no size"));
    }
    match (self.header.control_id, self.command_id) {
      (0x0001 | 0x1051, None) => {}
      (0x0001 | 0x1051, Some(_)) => {
        return Err(Error::invalid(0, "custom toolbar control has a Cid"));
      }
      (_, Some(value)) => push_u32(&mut bytes, value),
      (_, None) => return Err(Error::invalid(0, "toolbar control is missing Cid")),
    }
    if self.header.control_type == 0x16 {
      if self.data.is_some() {
        return Err(Error::invalid(3, "ActiveX toolbar control has TBCData"));
      }
      return Ok(bytes);
    }
    let data = self
      .data
      .as_ref()
      .ok_or_else(|| Error::invalid(3, "toolbar control is missing TBCData"))?;
    bytes.push(data.general.flags);
    write_optional_toolbar_string(
      &mut bytes,
      data.general.flags & 0x01 != 0,
      &data.general.custom_text,
    )?;
    if data.general.flags & 0x02 != 0 {
      write_toolbar_string(
        &mut bytes,
        data
          .general
          .description
          .as_deref()
          .ok_or_else(|| Error::invalid(0, "missing toolbar description"))?,
      )?;
      write_toolbar_string(
        &mut bytes,
        data
          .general
          .tooltip
          .as_deref()
          .ok_or_else(|| Error::invalid(0, "missing toolbar tooltip"))?,
      )?;
    } else if data.general.description.is_some() || data.general.tooltip.is_some() {
      return Err(Error::invalid(0, "toolbar UI strings disagree with flags"));
    }
    if data.general.flags & 0x04 != 0 {
      let extra = data
        .general
        .extra
        .as_ref()
        .ok_or_else(|| Error::invalid(0, "missing toolbar extra info"))?;
      write_toolbar_string(&mut bytes, &extra.help_file)?;
      push_i32(&mut bytes, extra.help_context_id);
      write_toolbar_string(&mut bytes, &extra.tag)?;
      write_toolbar_string(&mut bytes, &extra.on_action)?;
      write_toolbar_string(&mut bytes, &extra.parameter)?;
      bytes.extend_from_slice(&[
        extra.toolbar_control_user as u8,
        extra.toolbar_control_modified as u8,
      ]);
    } else if data.general.extra.is_some() {
      return Err(Error::invalid(0, "toolbar extra info disagrees with flags"));
    }
    match &data.specific {
      ToolbarControlSpecific::Menu { toolbar_id, name } => {
        if !matches!(self.header.control_type, 0x0a | 0x0c | 0x0d | 0x0e) {
          return Err(Error::invalid(3, "menu-specific data disagrees with tct"));
        }
        push_i32(&mut bytes, *toolbar_id);
        if *toolbar_id == 1 {
          write_toolbar_string(
            &mut bytes,
            name
              .as_deref()
              .ok_or_else(|| Error::invalid(0, "menu toolbar is missing name"))?,
          )?;
        } else if name.is_some() {
          return Err(Error::invalid(0, "noncustom menu toolbar has a name"));
        }
      }
    }
    Ok(bytes)
  }
}

fn read_toolbar_string(input: &mut SliceReader<'_>) -> Result<Vec<u16>> {
  let length = usize::from(input.u8()?);
  let mut value = Vec::with_capacity(length);
  for _ in 0..length {
    value.push(input.u16()?);
  }
  Ok(value)
}

fn write_toolbar_string(bytes: &mut Vec<u8>, value: &[u16]) -> Result<()> {
  bytes.push(
    u8::try_from(value.len()).map_err(|_| Error::Limit("toolbar WString exceeds u8".into()))?,
  );
  write_u16_array(bytes, value);
  Ok(())
}

fn write_optional_toolbar_string(
  bytes: &mut Vec<u8>,
  present: bool,
  value: &Option<Vec<u16>>,
) -> Result<()> {
  match (present, value) {
    (true, Some(value)) => write_toolbar_string(bytes, value),
    (false, None) => Ok(()),
    _ => Err(Error::invalid(0, "toolbar string disagrees with flags")),
  }
}

fn read_nonnegative_i32_count(input: &mut SliceReader<'_>, name: &str) -> Result<usize> {
  let offset = input.offset;
  let count = input.i32()?;
  if !(0..=1_000_000).contains(&count) {
    return Err(Error::invalid(
      offset as u64,
      format!("{name} count {count} is invalid"),
    ));
  }
  Ok(count as usize)
}

impl DocumentProperties {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 500 {
      return Err(Error::invalid(0, "Dop is shorter than Dop97"));
    }
    let mut input = SliceReader::new(bytes);
    let word97 = DocumentProperties97::read(&mut input)?;
    let extension = match bytes.len() {
      500 => DocumentPropertiesExtension::None,
      544 => DocumentPropertiesExtension::Word2000(DocumentProperties2000::read(&mut input)?),
      594 => DocumentPropertiesExtension::Word2002(DocumentProperties2002::read(&mut input)?),
      600 => DocumentPropertiesExtension::Compatibility600 {
        word2002: DocumentProperties2002::read(&mut input)?,
        words: read_u16_array(&mut input)?,
      },
      610 => DocumentPropertiesExtension::Compatibility610 {
        word2002: DocumentProperties2002::read(&mut input)?,
        words: read_u16_array(&mut input)?,
      },
      616 => DocumentPropertiesExtension::Word2003(DocumentProperties2003::read(&mut input)?),
      617 => DocumentPropertiesExtension::Word2003WithTrailingByte {
        word2003: DocumentProperties2003::read(&mut input)?,
        trailing: input.u8()?,
      },
      674 => DocumentPropertiesExtension::Word2007(DocumentProperties2007::read(&mut input)?),
      690 => DocumentPropertiesExtension::Word2010(DocumentProperties2010::read(&mut input)?),
      694 => DocumentPropertiesExtension::Word2013(DocumentProperties2013::read(&mut input)?),
      length => {
        return Err(Error::invalid(
          0,
          format!("unsupported Dop length {length}"),
        ));
      }
    };
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after Dop",
      ));
    }
    Ok(Self { word97, extension })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(self.encoded_len());
    self.word97.write(&mut bytes)?;
    match &self.extension {
      DocumentPropertiesExtension::None => {}
      DocumentPropertiesExtension::Word2000(word2000) => word2000.write(&mut bytes)?,
      DocumentPropertiesExtension::Word2002(word2002) => word2002.write(&mut bytes)?,
      DocumentPropertiesExtension::Compatibility600 { word2002, words } => {
        word2002.write(&mut bytes)?;
        write_u16_array(&mut bytes, words)
      }
      DocumentPropertiesExtension::Compatibility610 { word2002, words } => {
        word2002.write(&mut bytes)?;
        write_u16_array(&mut bytes, words)
      }
      DocumentPropertiesExtension::Word2003(word2003) => word2003.write(&mut bytes)?,
      DocumentPropertiesExtension::Word2007(word2007) => word2007.write(&mut bytes)?,
      DocumentPropertiesExtension::Word2010(word2010) => word2010.write(&mut bytes)?,
      DocumentPropertiesExtension::Word2013(word2013) => word2013.write(&mut bytes)?,
      DocumentPropertiesExtension::Word2003WithTrailingByte { word2003, trailing } => {
        word2003.write(&mut bytes)?;
        bytes.push(*trailing);
      }
    }
    Ok(bytes)
  }

  pub fn encoded_len(&self) -> usize {
    500
      + match &self.extension {
        DocumentPropertiesExtension::None => 0,
        DocumentPropertiesExtension::Word2000(_) => 44,
        DocumentPropertiesExtension::Word2002(_) => 94,
        DocumentPropertiesExtension::Compatibility600 { .. } => 100,
        DocumentPropertiesExtension::Compatibility610 { .. } => 110,
        DocumentPropertiesExtension::Word2003(_) => 116,
        DocumentPropertiesExtension::Word2003WithTrailingByte { .. } => 117,
        DocumentPropertiesExtension::Word2007(_) => 174,
        DocumentPropertiesExtension::Word2010(_) => 190,
        DocumentPropertiesExtension::Word2013(_) => 194,
      }
  }
}

impl DocumentPropertiesExtension {
  pub fn word2000(&self) -> Option<&DocumentProperties2000> {
    match self {
      Self::None => None,
      Self::Word2000(value) => Some(value),
      Self::Word2002(value) => Some(&value.word2000),
      Self::Compatibility600 { word2002, .. } | Self::Compatibility610 { word2002, .. } => {
        Some(&word2002.word2000)
      }
      Self::Word2003(value) => Some(&value.word2002.word2000),
      Self::Word2003WithTrailingByte { word2003, .. } => Some(&word2003.word2002.word2000),
      Self::Word2007(value) => Some(&value.word2003.word2002.word2000),
      Self::Word2010(value) => Some(&value.word2007.word2003.word2002.word2000),
      Self::Word2013(value) => Some(&value.word2010.word2007.word2003.word2002.word2000),
    }
  }

  pub fn word2000_mut(&mut self) -> Option<&mut DocumentProperties2000> {
    match self {
      Self::None => None,
      Self::Word2000(value) => Some(value),
      Self::Word2002(value) => Some(&mut value.word2000),
      Self::Compatibility600 { word2002, .. } | Self::Compatibility610 { word2002, .. } => {
        Some(&mut word2002.word2000)
      }
      Self::Word2003(value) => Some(&mut value.word2002.word2000),
      Self::Word2003WithTrailingByte { word2003, .. } => Some(&mut word2003.word2002.word2000),
      Self::Word2007(value) => Some(&mut value.word2003.word2002.word2000),
      Self::Word2010(value) => Some(&mut value.word2007.word2003.word2002.word2000),
      Self::Word2013(value) => Some(&mut value.word2010.word2007.word2003.word2002.word2000),
    }
  }

  pub fn word2002(&self) -> Option<&DocumentProperties2002> {
    match self {
      Self::None | Self::Word2000(_) => None,
      Self::Word2002(value) => Some(value),
      Self::Compatibility600 { word2002, .. } | Self::Compatibility610 { word2002, .. } => {
        Some(word2002)
      }
      Self::Word2003(value) => Some(&value.word2002),
      Self::Word2003WithTrailingByte { word2003, .. } => Some(&word2003.word2002),
      Self::Word2007(value) => Some(&value.word2003.word2002),
      Self::Word2010(value) => Some(&value.word2007.word2003.word2002),
      Self::Word2013(value) => Some(&value.word2010.word2007.word2003.word2002),
    }
  }

  pub fn word2003(&self) -> Option<&DocumentProperties2003> {
    match self {
      Self::None
      | Self::Word2000(_)
      | Self::Word2002(_)
      | Self::Compatibility600 { .. }
      | Self::Compatibility610 { .. } => None,
      Self::Word2003(value) => Some(value),
      Self::Word2003WithTrailingByte { word2003, .. } => Some(word2003),
      Self::Word2007(value) => Some(&value.word2003),
      Self::Word2010(value) => Some(&value.word2007.word2003),
      Self::Word2013(value) => Some(&value.word2010.word2007.word2003),
    }
  }

  pub fn word2007(&self) -> Option<&DocumentProperties2007> {
    match self {
      Self::None
      | Self::Word2000(_)
      | Self::Word2002(_)
      | Self::Compatibility600 { .. }
      | Self::Compatibility610 { .. }
      | Self::Word2003(_)
      | Self::Word2003WithTrailingByte { .. } => None,
      Self::Word2007(value) => Some(value),
      Self::Word2010(value) => Some(&value.word2007),
      Self::Word2013(value) => Some(&value.word2010.word2007),
    }
  }

  pub fn word2010(&self) -> Option<&DocumentProperties2010> {
    match self {
      Self::None
      | Self::Word2000(_)
      | Self::Word2002(_)
      | Self::Compatibility600 { .. }
      | Self::Compatibility610 { .. }
      | Self::Word2003(_)
      | Self::Word2003WithTrailingByte { .. }
      | Self::Word2007(_) => None,
      Self::Word2010(value) => Some(value),
      Self::Word2013(value) => Some(&value.word2010),
    }
  }

  pub fn word2013(&self) -> Option<&DocumentProperties2013> {
    match self {
      Self::Word2013(value) => Some(value),
      _ => None,
    }
  }
}

impl CompatibilityOptions60 {
  pub fn from_bits(value: u16) -> Self {
    Self {
      no_tab_for_hanging_indent: value & (1 << 0) != 0,
      no_space_for_raised_or_lowered_text: value & (1 << 1) != 0,
      suppress_space_before_after_page_break: value & (1 << 2) != 0,
      wrap_trailing_spaces: value & (1 << 3) != 0,
      map_print_text_color: value & (1 << 4) != 0,
      no_column_balance: value & (1 << 5) != 0,
      convert_mail_merge_escapes: value & (1 << 6) != 0,
      suppress_top_spacing: value & (1 << 7) != 0,
      original_word_table_rules: value & (1 << 8) != 0,
      unused: value & (1 << 9) != 0,
      show_breaks_in_frames: value & (1 << 10) != 0,
      swap_borders_on_facing_pages: value & (1 << 11) != 0,
      leave_backslash_alone: value & (1 << 12) != 0,
      expand_shift_return: value & (1 << 13) != 0,
      do_not_underline_trailing_space: value & (1 << 14) != 0,
      do_not_balance_single_double_byte_width: value & (1 << 15) != 0,
    }
  }

  pub fn bits(self) -> u16 {
    u16::from(self.no_tab_for_hanging_indent)
      | (u16::from(self.no_space_for_raised_or_lowered_text) << 1)
      | (u16::from(self.suppress_space_before_after_page_break) << 2)
      | (u16::from(self.wrap_trailing_spaces) << 3)
      | (u16::from(self.map_print_text_color) << 4)
      | (u16::from(self.no_column_balance) << 5)
      | (u16::from(self.convert_mail_merge_escapes) << 6)
      | (u16::from(self.suppress_top_spacing) << 7)
      | (u16::from(self.original_word_table_rules) << 8)
      | (u16::from(self.unused) << 9)
      | (u16::from(self.show_breaks_in_frames) << 10)
      | (u16::from(self.swap_borders_on_facing_pages) << 11)
      | (u16::from(self.leave_backslash_alone) << 12)
      | (u16::from(self.expand_shift_return) << 13)
      | (u16::from(self.do_not_underline_trailing_space) << 14)
      | (u16::from(self.do_not_balance_single_double_byte_width) << 15)
  }
}

impl CompatibilityOptions80 {
  pub fn from_bits(value: u32) -> Self {
    Self {
      word6: CompatibilityOptions60::from_bits(value as u16),
      suppress_top_spacing_mac5: value & (1 << 16) != 0,
      truncate_expanded_spacing: value & (1 << 17) != 0,
      print_body_before_header: value & (1 << 18) != 0,
      no_external_leading: value & (1 << 19) != 0,
      do_not_make_space_for_underline: value & (1 << 20) != 0,
      mac_word_small_caps: value & (1 << 21) != 0,
      two_point_external_leading_only: value & (1 << 22) != 0,
      truncate_font_height: value & (1 << 23) != 0,
      substitute_font_by_size: value & (1 << 24) != 0,
      line_wrap_like_word6: value & (1 << 25) != 0,
      word6_border_rules: value & (1 << 26) != 0,
      exact_line_height_on_top: value & (1 << 27) != 0,
      extra_after: value & (1 << 28) != 0,
      wordperfect_space_width: value & (1 << 29) != 0,
      wordperfect_justification: value & (1 << 30) != 0,
      use_printer_metrics: value & (1 << 31) != 0,
    }
  }

  pub fn bits(self) -> u32 {
    u32::from(self.word6.bits())
      | (u32::from(self.suppress_top_spacing_mac5) << 16)
      | (u32::from(self.truncate_expanded_spacing) << 17)
      | (u32::from(self.print_body_before_header) << 18)
      | (u32::from(self.no_external_leading) << 19)
      | (u32::from(self.do_not_make_space_for_underline) << 20)
      | (u32::from(self.mac_word_small_caps) << 21)
      | (u32::from(self.two_point_external_leading_only) << 22)
      | (u32::from(self.truncate_font_height) << 23)
      | (u32::from(self.substitute_font_by_size) << 24)
      | (u32::from(self.line_wrap_like_word6) << 25)
      | (u32::from(self.word6_border_rules) << 26)
      | (u32::from(self.exact_line_height_on_top) << 27)
      | (u32::from(self.extra_after) << 28)
      | (u32::from(self.wordperfect_space_width) << 29)
      | (u32::from(self.wordperfect_justification) << 30)
      | (u32::from(self.use_printer_metrics) << 31)
  }
}

impl CompatibilityOptions {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let word8 = CompatibilityOptions80::from_bits(input.u32()?);
    let flags = input.u32()?;
    let cached_and_empty = input.u32()?;
    let mut empty = [0; 5];
    for value in &mut empty {
      *value = input.u32()?;
    }
    Ok(Self {
      word8,
      shape_layout_like_word8: flags & (1 << 0) != 0,
      footnote_layout_like_word8: flags & (1 << 1) != 0,
      do_not_use_html_paragraph_auto_spacing: flags & (1 << 2) != 0,
      do_not_adjust_line_height_in_table: flags & (1 << 3) != 0,
      forget_last_tab_alignment: flags & (1 << 4) != 0,
      use_autospace_for_full_width_alpha: flags & (1 << 5) != 0,
      align_tables_row_by_row: flags & (1 << 6) != 0,
      layout_raw_table_width: flags & (1 << 7) != 0,
      layout_table_rows_apart: flags & (1 << 8) != 0,
      use_word97_line_breaking_rules: flags & (1 << 9) != 0,
      do_not_break_wrapped_tables: flags & (1 << 10) != 0,
      do_not_snap_to_grid_in_cell: flags & (1 << 11) != 0,
      do_not_allow_field_end_select: flags & (1 << 12) != 0,
      apply_breaking_rules: flags & (1 << 13) != 0,
      do_not_wrap_text_with_punctuation: flags & (1 << 14) != 0,
      do_not_use_asian_break_rules: flags & (1 << 15) != 0,
      use_word2002_table_style_rules: flags & (1 << 16) != 0,
      grow_autofit: flags & (1 << 17) != 0,
      use_normal_style_for_list: flags & (1 << 18) != 0,
      do_not_use_indent_as_numbering_tab_stop: flags & (1 << 19) != 0,
      far_east_line_break11: flags & (1 << 20) != 0,
      allow_same_style_spacing_in_table: flags & (1 << 21) != 0,
      word11_indent_rules: flags & (1 << 22) != 0,
      do_not_autofit_constrained_tables: flags & (1 << 23) != 0,
      autofit_like_word11: flags & (1 << 24) != 0,
      underline_tab_in_numbered_list: flags & (1 << 25) != 0,
      hangul_width_like_word11: flags & (1 << 26) != 0,
      split_page_break_and_paragraph_mark: flags & (1 << 27) != 0,
      do_not_vertically_align_cell_with_shape: flags & (1 << 28) != 0,
      do_not_break_constrained_forced_tables: flags & (1 << 29) != 0,
      do_not_vertically_align_in_textbox: flags & (1 << 30) != 0,
      word11_kerning_pairs: flags & (1 << 31) != 0,
      cached_column_balance: cached_and_empty & 1 != 0,
      empty1: cached_and_empty >> 1,
      empty,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.empty1 > 0x7fff_ffff {
      return Err(Error::invalid(0, "Copts empty1 exceeds 31 bits"));
    }
    push_u32(bytes, self.word8.bits());
    push_u32(bytes, self.named_bits());
    push_u32(
      bytes,
      u32::from(self.cached_column_balance) | (self.empty1 << 1),
    );
    for value in self.empty {
      push_u32(bytes, value);
    }
    Ok(())
  }

  pub fn named_bits(self) -> u32 {
    u32::from(self.shape_layout_like_word8)
      | (u32::from(self.footnote_layout_like_word8) << 1)
      | (u32::from(self.do_not_use_html_paragraph_auto_spacing) << 2)
      | (u32::from(self.do_not_adjust_line_height_in_table) << 3)
      | (u32::from(self.forget_last_tab_alignment) << 4)
      | (u32::from(self.use_autospace_for_full_width_alpha) << 5)
      | (u32::from(self.align_tables_row_by_row) << 6)
      | (u32::from(self.layout_raw_table_width) << 7)
      | (u32::from(self.layout_table_rows_apart) << 8)
      | (u32::from(self.use_word97_line_breaking_rules) << 9)
      | (u32::from(self.do_not_break_wrapped_tables) << 10)
      | (u32::from(self.do_not_snap_to_grid_in_cell) << 11)
      | (u32::from(self.do_not_allow_field_end_select) << 12)
      | (u32::from(self.apply_breaking_rules) << 13)
      | (u32::from(self.do_not_wrap_text_with_punctuation) << 14)
      | (u32::from(self.do_not_use_asian_break_rules) << 15)
      | (u32::from(self.use_word2002_table_style_rules) << 16)
      | (u32::from(self.grow_autofit) << 17)
      | (u32::from(self.use_normal_style_for_list) << 18)
      | (u32::from(self.do_not_use_indent_as_numbering_tab_stop) << 19)
      | (u32::from(self.far_east_line_break11) << 20)
      | (u32::from(self.allow_same_style_spacing_in_table) << 21)
      | (u32::from(self.word11_indent_rules) << 22)
      | (u32::from(self.do_not_autofit_constrained_tables) << 23)
      | (u32::from(self.autofit_like_word11) << 24)
      | (u32::from(self.underline_tab_in_numbered_list) << 25)
      | (u32::from(self.hangul_width_like_word11) << 26)
      | (u32::from(self.split_page_break_and_paragraph_mark) << 27)
      | (u32::from(self.do_not_vertically_align_cell_with_shape) << 28)
      | (u32::from(self.do_not_break_constrained_forced_tables) << 29)
      | (u32::from(self.do_not_vertically_align_in_textbox) << 30)
      | (u32::from(self.word11_kerning_pairs) << 31)
  }
}

impl DocumentProperties2000 {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let last_bullet_level = input.u8()?;
    let last_numbering_level = input.u8()?;
    if last_bullet_level > 9 || last_numbering_level > 9 {
      return Err(Error::invalid(0, "Dop2000 list toolbar level exceeds 9"));
    }
    Ok(Self {
      last_bullet_level,
      last_numbering_level,
      click_and_type_style: input.u16()?,
      flags: DocumentProperties2000Flags::from_bits(input.u32()?)?,
      compatibility_options: CompatibilityOptions::read(input)?,
      pre_word10_features: PreWord10Features::from_bits(input.u16()?),
      flags2: DocumentProperties2000Flags2::from_bits(input.u16()?)?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.last_bullet_level > 9 || self.last_numbering_level > 9 {
      return Err(Error::invalid(0, "Dop2000 list toolbar level exceeds 9"));
    }
    bytes.extend_from_slice(&[self.last_bullet_level, self.last_numbering_level]);
    push_u16(bytes, self.click_and_type_style);
    push_u32(bytes, self.flags.bits()?);
    self.compatibility_options.write(bytes)?;
    push_u16(bytes, self.pre_word10_features.bits()?);
    push_u16(bytes, self.flags2.bits()?);
    Ok(())
  }

  pub fn compatibility_options_match(self, word97: &DocumentProperties97) -> bool {
    self.compatibility_options.word8 == word97.compatibility_options_80
  }

  pub fn validate_compatibility_options(self, word97: &DocumentProperties97) -> Result<()> {
    if !self.compatibility_options_match(word97) {
      return Err(Error::invalid(
        508,
        "Copts.copts80 differs from Dop97.copts80",
      ));
    }
    Ok(())
  }
}

impl DocumentProperties2000Flags {
  fn from_bits(value: u32) -> Result<Self> {
    if value & 0x0000_00f0 != 0 {
      return Err(Error::invalid(0, "Dop2000 empty1 bits are nonzero"));
    }
    let web_options_initialized = value & (1 << 28) != 0;
    let target_screen_size = WebTargetScreenSize::from_u8(((value >> 12) & 0x0f) as u8);
    let pixels_per_inch = ((value >> 18) & 0x03ff) as u16;
    if web_options_initialized
      && (!target_screen_size.is_standard() || !(19..=480).contains(&pixels_per_inch))
    {
      return Err(Error::invalid(
        0,
        "Dop2000 initialized Web options are invalid",
      ));
    }
    Ok(Self {
      language_detection_all_done: value & (1 << 0) != 0,
      envelope_visible: value & (1 << 1) != 0,
      maybe_tentative_list: value & (1 << 2) != 0,
      maybe_fit_text: value & (1 << 3) != 0,
      format_consistency_all_done: value & (1 << 8) != 0,
      rely_on_css: value & (1 << 9) != 0,
      rely_on_vml: value & (1 << 10) != 0,
      allow_png: value & (1 << 11) != 0,
      target_screen_size,
      organize_in_folder: value & (1 << 16) != 0,
      use_long_file_names: value & (1 << 17) != 0,
      pixels_per_inch,
      web_options_initialized,
      maybe_east_asian_layout: value & (1 << 29) != 0,
      character_line_units: value & (1 << 30) != 0,
      unused1: value & (1 << 31) != 0,
    })
  }

  pub fn bits(self) -> Result<u32> {
    if self.pixels_per_inch > 0x03ff {
      return Err(Error::invalid(0, "Dop2000 pixels-per-inch exceeds 10 bits"));
    }
    if self.web_options_initialized
      && (!self.target_screen_size.is_standard() || !(19..=480).contains(&self.pixels_per_inch))
    {
      return Err(Error::invalid(
        0,
        "Dop2000 initialized Web options are invalid",
      ));
    }
    Ok(
      u32::from(self.language_detection_all_done)
        | (u32::from(self.envelope_visible) << 1)
        | (u32::from(self.maybe_tentative_list) << 2)
        | (u32::from(self.maybe_fit_text) << 3)
        | (u32::from(self.format_consistency_all_done) << 8)
        | (u32::from(self.rely_on_css) << 9)
        | (u32::from(self.rely_on_vml) << 10)
        | (u32::from(self.allow_png) << 11)
        | (u32::from(self.target_screen_size.to_u8()) << 12)
        | (u32::from(self.organize_in_folder) << 16)
        | (u32::from(self.use_long_file_names) << 17)
        | (u32::from(self.pixels_per_inch) << 18)
        | (u32::from(self.web_options_initialized) << 28)
        | (u32::from(self.maybe_east_asian_layout) << 29)
        | (u32::from(self.character_line_units) << 30)
        | (u32::from(self.unused1) << 31),
    )
  }
}

impl WebTargetScreenSize {
  fn from_u8(value: u8) -> Self {
    match value {
      0 => Self::Size544x376,
      1 => Self::Size640x480,
      2 => Self::Size720x512,
      3 => Self::Size800x600,
      4 => Self::Size1024x768,
      5 => Self::Size1152x882,
      6 => Self::Size1152x900,
      7 => Self::Size1280x1024,
      8 => Self::Size1600x1200,
      9 => Self::Size1800x1440,
      10 => Self::Size1920x1200,
      11 => Self::Compatibility11,
      12 => Self::Compatibility12,
      13 => Self::Compatibility13,
      14 => Self::Compatibility14,
      _ => Self::Compatibility15,
    }
  }

  fn to_u8(self) -> u8 {
    self as u8
  }

  pub fn is_standard(self) -> bool {
    (self as u8) <= 10
  }
}

impl PreWord10Features {
  const KNOWN_MASK: u16 = 0x084c;

  fn from_bits(value: u16) -> Self {
    Self {
      word95: value & 0x0004 != 0,
      word97: value & 0x0008 != 0,
      east_asian_word95: value & 0x0040 != 0,
      word2003: value & 0x0800 != 0,
      unused: value & !Self::KNOWN_MASK,
    }
  }

  pub fn bits(self) -> Result<u16> {
    if self.unused & Self::KNOWN_MASK != 0 {
      return Err(Error::invalid(
        0,
        "Dop2000 verCompatPre10 unused bits overlap",
      ));
    }
    Ok(
      self.unused
        | (u16::from(self.word95) << 2)
        | (u16::from(self.word97) << 3)
        | (u16::from(self.east_asian_word95) << 6)
        | (u16::from(self.word2003) << 11),
    )
  }
}

impl DocumentProperties2000Flags2 {
  fn from_bits(value: u16) -> Result<Self> {
    if value & 0x0120 != 0 {
      return Err(Error::invalid(0, "Dop2000 empty flag is nonzero"));
    }
    Ok(Self {
      suppress_page_boundaries: value & (1 << 0) != 0,
      unused2_to_4: ((value >> 1) & 7) as u8,
      bullet_proofed: value & (1 << 4) != 0,
      save_uim: value & (1 << 6) != 0,
      filter_privacy: value & (1 << 7) != 0,
      seen_repairs: value & (1 << 9) != 0,
      has_xml: value & (1 << 10) != 0,
      unused5: value & (1 << 11) != 0,
      validate_xml: value & (1 << 12) != 0,
      save_invalid_xml: value & (1 << 13) != 0,
      show_xml_errors: value & (1 << 14) != 0,
      always_merge_empty_namespace: value & (1 << 15) != 0,
    })
  }

  pub fn bits(self) -> Result<u16> {
    if self.unused2_to_4 > 7 {
      return Err(Error::invalid(0, "Dop2000 unused2..4 exceed 3 bits"));
    }
    Ok(
      u16::from(self.suppress_page_boundaries)
        | (u16::from(self.unused2_to_4) << 1)
        | (u16::from(self.bullet_proofed) << 4)
        | (u16::from(self.save_uim) << 6)
        | (u16::from(self.filter_privacy) << 7)
        | (u16::from(self.seen_repairs) << 9)
        | (u16::from(self.has_xml) << 10)
        | (u16::from(self.unused5) << 11)
        | (u16::from(self.validate_xml) << 12)
        | (u16::from(self.save_invalid_xml) << 13)
        | (u16::from(self.show_xml_errors) << 14)
        | (u16::from(self.always_merge_empty_namespace) << 15),
    )
  }
}

impl DocumentProperties2002 {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    Ok(Self {
      word2000: DocumentProperties2000::read(input)?,
      unused: input.u32()?,
      flags: DocumentProperties2002Flags::from_bits(input.u16()?)?,
      default_table_style: input.u16()?,
      feature_compatibility: FeatureCompatibility::from_bits(input.u16()?),
      style_filter: input.u16()?,
      booklet_pages: input.u16()?,
      text_code_page: input.u32()?,
      minimum_revision_positions: RevisionMinimumPositions::read(input)?,
      root_revision_save_id: input.u32()?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    self.word2000.write(bytes)?;
    push_u32(bytes, self.unused);
    push_u16(bytes, self.flags.bits()?);
    push_u16(bytes, self.default_table_style);
    push_u16(bytes, self.feature_compatibility.bits()?);
    push_u16(bytes, self.style_filter);
    push_u16(bytes, self.booklet_pages);
    push_u32(bytes, self.text_code_page);
    self.minimum_revision_positions.write(bytes);
    push_u32(bytes, self.root_revision_save_id);
    Ok(())
  }
}

impl DocumentProperties2002Flags {
  fn from_bits(value: u16) -> Result<Self> {
    let folio_print = value & (1 << 6) != 0;
    let reverse_folio = value & (1 << 7) != 0;
    if reverse_folio && !folio_print {
      return Err(Error::invalid(
        0,
        "Dop2002 reverse folio is set without folio printing",
      ));
    }
    Ok(Self {
      do_not_embed_system_font: value & (1 << 0) != 0,
      word_compatibility: value & (1 << 1) != 0,
      live_recover: value & (1 << 2) != 0,
      embed_factoids: value & (1 << 3) != 0,
      factoid_xml: value & (1 << 4) != 0,
      factoid_all_done: value & (1 << 5) != 0,
      folio_print,
      reverse_folio,
      text_line_ending: TextLineEnding::from_u8(((value >> 8) & 7) as u8)?,
      hide_format_consistency: value & (1 << 11) != 0,
      show_markup: value & (1 << 12) != 0,
      show_comments: value & (1 << 13) != 0,
      show_insertions_deletions: value & (1 << 14) != 0,
      show_property_changes: value & (1 << 15) != 0,
    })
  }

  pub fn bits(self) -> Result<u16> {
    if self.reverse_folio && !self.folio_print {
      return Err(Error::invalid(
        0,
        "Dop2002 reverse folio is set without folio printing",
      ));
    }
    Ok(
      u16::from(self.do_not_embed_system_font)
        | (u16::from(self.word_compatibility) << 1)
        | (u16::from(self.live_recover) << 2)
        | (u16::from(self.embed_factoids) << 3)
        | (u16::from(self.factoid_xml) << 4)
        | (u16::from(self.factoid_all_done) << 5)
        | (u16::from(self.folio_print) << 6)
        | (u16::from(self.reverse_folio) << 7)
        | (u16::from(self.text_line_ending.to_u8()) << 8)
        | (u16::from(self.hide_format_consistency) << 11)
        | (u16::from(self.show_markup) << 12)
        | (u16::from(self.show_comments) << 13)
        | (u16::from(self.show_insertions_deletions) << 14)
        | (u16::from(self.show_property_changes) << 15),
    )
  }
}

impl TextLineEnding {
  fn from_u8(value: u8) -> Result<Self> {
    match value {
      0 => Ok(Self::CrLf),
      1 => Ok(Self::Cr),
      2 => Ok(Self::Lf),
      3 => Ok(Self::LfCr),
      4 => Ok(Self::UnicodeSeparator),
      _ => Err(Error::invalid(0, "Dop2002 line-ending mode is invalid")),
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::CrLf => 0,
      Self::Cr => 1,
      Self::Lf => 2,
      Self::LfCr => 3,
      Self::UnicodeSeparator => 4,
    }
  }
}

impl FeatureCompatibility {
  const KNOWN_MASK: u16 = 0x1fff;

  fn from_bits(value: u16) -> Self {
    Self {
      internet_explorer4: value & (1 << 0) != 0,
      internet_explorer5: value & (1 << 1) != 0,
      word95: value & (1 << 2) != 0,
      word97: value & (1 << 3) != 0,
      word_html: value & (1 << 4) != 0,
      word_rtf: value & (1 << 5) != 0,
      east_asian_word95: value & (1 << 6) != 0,
      plain_text_email: value & (1 << 7) != 0,
      internet_explorer6: value & (1 << 8) != 0,
      word_xml: value & (1 << 9) != 0,
      rtf_email: value & (1 << 10) != 0,
      no_word2007_features: value & (1 << 11) != 0,
      plain_text: value & (1 << 12) != 0,
      unused: value & !Self::KNOWN_MASK,
    }
  }

  pub fn bits(self) -> Result<u16> {
    if self.unused & Self::KNOWN_MASK != 0 {
      return Err(Error::invalid(
        0,
        "Dop2002 verCompat unused bits overlap known flags",
      ));
    }
    Ok(
      self.unused
        | u16::from(self.internet_explorer4)
        | (u16::from(self.internet_explorer5) << 1)
        | (u16::from(self.word95) << 2)
        | (u16::from(self.word97) << 3)
        | (u16::from(self.word_html) << 4)
        | (u16::from(self.word_rtf) << 5)
        | (u16::from(self.east_asian_word95) << 6)
        | (u16::from(self.plain_text_email) << 7)
        | (u16::from(self.internet_explorer6) << 8)
        | (u16::from(self.word_xml) << 9)
        | (u16::from(self.rtf_email) << 10)
        | (u16::from(self.no_word2007_features) << 11)
        | (u16::from(self.plain_text) << 12),
    )
  }
}

impl RevisionMinimumPositions {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    Ok(Self {
      main: input.u32()?,
      footnote: input.u32()?,
      header: input.u32()?,
      comment: input.u32()?,
      endnote: input.u32()?,
      textbox: input.u32()?,
      header_textbox: input.u32()?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) {
    push_u32(bytes, self.main);
    push_u32(bytes, self.footnote);
    push_u32(bytes, self.header);
    push_u32(bytes, self.comment);
    push_u32(bytes, self.endnote);
    push_u32(bytes, self.textbox);
    push_u32(bytes, self.header_textbox);
  }
}

impl DocumentProperties2003 {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let value = Self {
      word2002: DocumentProperties2002::read(input)?,
      flags: DocumentProperties2003Flags::from_bits(input.u32()?)?,
      protection: DocumentProtectionSettings::from_bits(input.u16()?)?,
      page_lock_width: input.u32()?,
      page_lock_height: input.u32()?,
      locked_font_percentage: input.u32()?,
      state_toolbars: DocumentStateToolbars::from_bits(input.u8()?)?,
      list_override_cleanup_limit: {
        let empty = input.u8()?;
        if empty != 0 {
          return Err(Error::invalid(0, "Dop2003 empty3 byte is nonzero"));
        }
        input.u16()?
      },
    };
    Ok(value)
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    self.word2002.write(bytes)?;
    push_u32(bytes, self.flags.bits()?);
    push_u16(bytes, self.protection.bits());
    push_u32(bytes, self.page_lock_width);
    push_u32(bytes, self.page_lock_height);
    push_u32(bytes, self.locked_font_percentage);
    bytes.extend_from_slice(&[self.state_toolbars.bits(), 0]);
    push_u16(bytes, self.list_override_cleanup_limit);
    Ok(())
  }

  pub fn cleanup_limit_matches_override_count(self, override_count: usize) -> bool {
    if override_count == 0 {
      self.list_override_cleanup_limit == 0
    } else {
      usize::from(self.list_override_cleanup_limit) < override_count
    }
  }

  pub fn validate_cleanup_limit(self, override_count: usize) -> Result<()> {
    if !self.cleanup_limit_matches_override_count(override_count) {
      return Err(Error::invalid(
        614,
        format!(
          "Dop2003 cleanup limit {} exceeds PlfLfo count {override_count}",
          self.list_override_cleanup_limit
        ),
      ));
    }
    Ok(())
  }
}

impl DocumentProperties2003Flags {
  const KNOWN_MASK: u32 = 0x0000_1fff;

  fn from_bits(value: u32) -> Result<Self> {
    if value & !Self::KNOWN_MASK != 0 {
      return Err(Error::invalid(0, "Dop2003 empty1 bits are nonzero"));
    }
    let style_lock = value & (1 << 1) != 0;
    let style_lock_enforced = value & (1 << 5) != 0;
    if style_lock_enforced && !style_lock {
      return Err(Error::invalid(
        0,
        "Dop2003 style lock is enforced without being enabled",
      ));
    }
    Ok(Self {
      treat_comment_lock_as_read_only: value & (1 << 0) != 0,
      style_lock,
      auto_format_override: value & (1 << 2) != 0,
      remove_wordml: value & (1 << 3) != 0,
      apply_custom_xml_transform: value & (1 << 4) != 0,
      style_lock_enforced,
      compatibility_comment_lock: value & (1 << 6) != 0,
      ignore_mixed_content: value & (1 << 7) != 0,
      show_placeholder_text: value & (1 << 8) != 0,
      unused: value & (1 << 9) != 0,
      word97_document: value & (1 << 10) != 0,
      lock_theme: value & (1 << 11) != 0,
      lock_quick_format_style_set: value & (1 << 12) != 0,
    })
  }

  pub fn bits(self) -> Result<u32> {
    if self.style_lock_enforced && !self.style_lock {
      return Err(Error::invalid(
        0,
        "Dop2003 style lock is enforced without being enabled",
      ));
    }
    Ok(
      u32::from(self.treat_comment_lock_as_read_only)
        | (u32::from(self.style_lock) << 1)
        | (u32::from(self.auto_format_override) << 2)
        | (u32::from(self.remove_wordml) << 3)
        | (u32::from(self.apply_custom_xml_transform) << 4)
        | (u32::from(self.style_lock_enforced) << 5)
        | (u32::from(self.compatibility_comment_lock) << 6)
        | (u32::from(self.ignore_mixed_content) << 7)
        | (u32::from(self.show_placeholder_text) << 8)
        | (u32::from(self.unused) << 9)
        | (u32::from(self.word97_document) << 10)
        | (u32::from(self.lock_theme) << 11)
        | (u32::from(self.lock_quick_format_style_set) << 12),
    )
  }
}

impl DocumentProtectionSettings {
  fn from_bits(value: u16) -> Result<Self> {
    if value & 0xff00 != 0 {
      return Err(Error::invalid(0, "Dop2003 empty2 bits are nonzero"));
    }
    Ok(Self {
      reading_mode_ink_lockdown: value & (1 << 0) != 0,
      show_ink_annotations: value & (1 << 1) != 0,
      remove_annotation_date_time: value & (1 << 2) != 0,
      enforce: value & (1 << 3) != 0,
      mode: DocumentProtectionMode::from_u8(((value >> 4) & 7) as u8)?,
      display_background_shapes: value & (1 << 7) != 0,
    })
  }

  pub fn bits(self) -> u16 {
    u16::from(self.reading_mode_ink_lockdown)
      | (u16::from(self.show_ink_annotations) << 1)
      | (u16::from(self.remove_annotation_date_time) << 2)
      | (u16::from(self.enforce) << 3)
      | (u16::from(self.mode.to_u8()) << 4)
      | (u16::from(self.display_background_shapes) << 7)
  }
}

impl DocumentProtectionMode {
  fn from_u8(value: u8) -> Result<Self> {
    match value {
      0 => Ok(Self::TrackedChanges),
      1 => Ok(Self::CommentsAndRangePermissions),
      2 => Ok(Self::Forms),
      3 => Ok(Self::RangePermissions),
      7 => Ok(Self::None),
      _ => Err(Error::invalid(
        0,
        "Dop2003 document protection mode is invalid",
      )),
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::TrackedChanges => 0,
      Self::CommentsAndRangePermissions => 1,
      Self::Forms => 2,
      Self::RangePermissions => 3,
      Self::None => 7,
    }
  }
}

impl DocumentStateToolbars {
  fn from_bits(value: u8) -> Result<Self> {
    if value & !0x07 != 0 {
      return Err(Error::invalid(0, "Dop2003 grfitbid has reserved bits"));
    }
    Ok(Self {
      reviewing: value & 1 != 0,
      web: value & 2 != 0,
      mail_merge: value & 4 != 0,
    })
  }

  pub fn bits(self) -> u8 {
    u8::from(self.reviewing) | (u8::from(self.web) << 1) | (u8::from(self.mail_merge) << 2)
  }
}

impl DocumentProperties2007 {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let word2003 = DocumentProperties2003::read(input)?;
    let reserved = input.u32()?;
    let flags = DocumentProperties2007Flags::from_bits(input.u32()?)?;
    for _ in 0..4 {
      if input.u32()? != 0 {
        return Err(Error::invalid(0, "Dop2007 empty dword is nonzero"));
      }
    }
    Ok(Self {
      word2003,
      reserved,
      flags,
      math: DocumentMathProperties::read(input)?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    self.word2003.write(bytes)?;
    push_u32(bytes, self.reserved);
    push_u32(bytes, self.flags.bits());
    for _ in 0..4 {
      push_u32(bytes, 0);
    }
    self.math.write(bytes)
  }
}

impl DocumentProperties2007Flags {
  const ALLOWED_MASK: u32 = 0x0000_07e3;

  fn from_bits(value: u32) -> Result<Self> {
    if value & !Self::ALLOWED_MASK != 0 {
      return Err(Error::invalid(0, "Dop2007 reserved flag is nonzero"));
    }
    Ok(Self {
      track_formatting: value & 1 != 0,
      track_moves: value & 2 != 0,
      style_sort_method: StyleSortMethod::from_u8(((value >> 5) & 0x0f) as u8)?,
      reading_mode_actual_pages: value & (1 << 9) != 0,
      auto_compress_pictures: value & (1 << 10) != 0,
    })
  }

  pub fn bits(self) -> u32 {
    u32::from(self.track_formatting)
      | (u32::from(self.track_moves) << 1)
      | (u32::from(self.style_sort_method.to_u8()) << 5)
      | (u32::from(self.reading_mode_actual_pages) << 9)
      | (u32::from(self.auto_compress_pictures) << 10)
  }
}

impl StyleSortMethod {
  fn from_u8(value: u8) -> Result<Self> {
    match value {
      0 => Ok(Self::Name),
      1 => Ok(Self::ApplicationDefault),
      2 => Ok(Self::Font),
      3 => Ok(Self::BasedOn),
      4 => Ok(Self::StyleType),
      _ => Err(Error::invalid(0, "Dop2007 style sort method is invalid")),
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::Name => 0,
      Self::ApplicationDefault => 1,
      Self::Font => 2,
      Self::BasedOn => 3,
      Self::StyleType => 4,
    }
  }
}

impl DocumentMathProperties {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let flags = input.u32()?;
    if flags & !0x0000_1fff != 0 {
      return Err(Error::invalid(0, "DopMth reserved2 bits are nonzero"));
    }
    let value = Self {
      binary_operator_break: MathBinaryOperatorBreak::from_u8((flags & 3) as u8)?,
      binary_subtraction_break: MathBinarySubtractionBreak::from_u8(((flags >> 2) & 3) as u8)?,
      justification: MathJustification::from_u8(((flags >> 4) & 7) as u8)?,
      reserved: flags & (1 << 7) != 0,
      small_fraction: flags & (1 << 8) != 0,
      integral_limits_above_below: flags & (1 << 9) != 0,
      nary_limits_above_below: flags & (1 << 10) != 0,
      wrapped_line_align_left: flags & (1 << 11) != 0,
      use_display_defaults: flags & (1 << 12) != 0,
      font_index: input.u16()?,
      left_margin: input.i32()?,
      right_margin: input.i32()?,
      fixed_constants: MathFixedConstants::Standard120,
      wrapped_line_indent: 0,
    };
    let fixed1 = input.u32()?;
    let fixed2 = input.u32()?;
    let fixed_constants = MathFixedConstants::from_values(fixed1, fixed2)?;
    if input.u32()? != 0 || input.u32()? != 0 {
      return Err(Error::invalid(0, "DopMth empty dword is nonzero"));
    }
    let value = Self {
      fixed_constants,
      wrapped_line_indent: input.i32()?,
      ..value
    };
    value.validate()?;
    Ok(value)
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    push_u32(bytes, self.flag_bits());
    push_u16(bytes, self.font_index);
    push_i32(bytes, self.left_margin);
    push_i32(bytes, self.right_margin);
    let (fixed1, fixed2) = self.fixed_constants.values();
    push_u32(bytes, fixed1);
    push_u32(bytes, fixed2);
    push_u32(bytes, 0);
    push_u32(bytes, 0);
    push_i32(bytes, self.wrapped_line_indent);
    Ok(())
  }

  pub fn flag_bits(self) -> u32 {
    u32::from(self.binary_operator_break.to_u8())
      | (u32::from(self.binary_subtraction_break.to_u8()) << 2)
      | (u32::from(self.justification.to_u8()) << 4)
      | (u32::from(self.reserved) << 7)
      | (u32::from(self.small_fraction) << 8)
      | (u32::from(self.integral_limits_above_below) << 9)
      | (u32::from(self.nary_limits_above_below) << 10)
      | (u32::from(self.wrapped_line_align_left) << 11)
      | (u32::from(self.use_display_defaults) << 12)
  }

  pub fn validate_standard(self) -> Result<()> {
    if !self.justification.is_standard() {
      return Err(Error::invalid(
        0,
        "DopMth justification uses producer compatibility value 0",
      ));
    }
    if !self.fixed_constants.is_standard() {
      return Err(Error::invalid(
        0,
        "DopMth fixed dwords use producer compatibility value 0/0",
      ));
    }
    self.validate()
  }

  fn validate(self) -> Result<()> {
    for (name, value) in [
      ("left margin", self.left_margin),
      ("right margin", self.right_margin),
      ("wrapped-line indent", self.wrapped_line_indent),
    ] {
      if !(0..=31_680).contains(&value) {
        return Err(Error::invalid(
          0,
          format!("DopMth {name} is outside 0..=31680"),
        ));
      }
    }
    Ok(())
  }
}

impl MathBinaryOperatorBreak {
  fn from_u8(value: u8) -> Result<Self> {
    match value {
      0 => Ok(Self::Before),
      1 => Ok(Self::After),
      2 => Ok(Self::Repeat),
      _ => Err(Error::invalid(0, "DopMth binary-operator break is invalid")),
    }
  }

  fn to_u8(self) -> u8 {
    self as u8
  }
}

impl MathBinarySubtractionBreak {
  fn from_u8(value: u8) -> Result<Self> {
    match value {
      0 => Ok(Self::MinusMinus),
      1 => Ok(Self::PlusMinus),
      2 => Ok(Self::MinusPlus),
      _ => Err(Error::invalid(0, "DopMth subtraction break is invalid")),
    }
  }

  fn to_u8(self) -> u8 {
    self as u8
  }
}

impl MathJustification {
  fn from_u8(value: u8) -> Result<Self> {
    match value {
      0 => Ok(Self::ProducerCompatibilityZero),
      1 => Ok(Self::CenteredAsGroup),
      2 => Ok(Self::Center),
      3 => Ok(Self::Left),
      4 => Ok(Self::Right),
      _ => Err(Error::invalid(
        0,
        format!("DopMth justification {value} is invalid"),
      )),
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::ProducerCompatibilityZero => 0,
      Self::CenteredAsGroup => 1,
      Self::Center => 2,
      Self::Left => 3,
      Self::Right => 4,
    }
  }

  pub fn is_standard(self) -> bool {
    !matches!(self, Self::ProducerCompatibilityZero)
  }
}

impl MathFixedConstants {
  fn from_values(first: u32, second: u32) -> Result<Self> {
    match (first, second) {
      (120, 120) => Ok(Self::Standard120),
      (0, 0) => Ok(Self::ProducerCompatibilityZero),
      _ => Err(Error::invalid(
        0,
        format!("DopMth fixed dwords are {first}/{second}, not 120/120 or 0/0"),
      )),
    }
  }

  fn values(self) -> (u32, u32) {
    match self {
      Self::Standard120 => (120, 120),
      Self::ProducerCompatibilityZero => (0, 0),
    }
  }

  pub fn is_standard(self) -> bool {
    matches!(self, Self::Standard120)
  }
}

impl DocumentProperties2010 {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let value = Self {
      word2007: DocumentProperties2007::read(input)?,
      paragraph_identifier_context: ParagraphIdentifierContext::from_u32(input.u32()?)?,
      reserved: input.u32()?,
      discard_image_editing_data: {
        let flags = input.u32()?;
        if flags & !1 != 0 {
          return Err(Error::invalid(0, "Dop2010 empty flag is nonzero"));
        }
        flags & 1 != 0
      },
      image_resolution_dpi: input.u32()?,
    };
    value.validate()?;
    Ok(value)
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    self.word2007.write(bytes)?;
    push_u32(bytes, self.paragraph_identifier_context.value()?);
    push_u32(bytes, self.reserved);
    push_u32(bytes, u32::from(self.discard_image_editing_data));
    push_u32(bytes, self.image_resolution_dpi);
    Ok(())
  }

  fn validate(self) -> Result<()> {
    self.paragraph_identifier_context.value()?;
    Ok(())
  }
}

impl ParagraphIdentifierContext {
  fn from_u32(value: u32) -> Result<Self> {
    match value {
      0 => Ok(Self::ProducerCompatibilityZero),
      1..=0x7fff_ffff => Ok(Self::Standard(value)),
      _ => Err(Error::invalid(
        0,
        "Dop2010 paragraph identifier context exceeds 31 bits",
      )),
    }
  }

  pub fn value(self) -> Result<u32> {
    match self {
      Self::Standard(value) if (1..0x8000_0000).contains(&value) => Ok(value),
      Self::Standard(_) => Err(Error::invalid(
        0,
        "Dop2010 standard paragraph identifier context is outside 1..0x80000000",
      )),
      Self::ProducerCompatibilityZero => Ok(0),
    }
  }

  pub fn is_standard(self) -> bool {
    matches!(self, Self::Standard(1..=0x7fff_ffff))
  }
}

impl DocumentProperties2013 {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let word2010 = DocumentProperties2010::read(input)?;
    let flags = input.u32()?;
    if flags & !1 != 0 {
      return Err(Error::invalid(0, "Dop2013 empty flag is nonzero"));
    }
    Ok(Self {
      word2010,
      chart_tracking_reference_based: flags & 1 != 0,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    self.word2010.write(bytes)?;
    push_u32(bytes, u32::from(self.chart_tracking_reference_based));
    Ok(())
  }
}

impl DocumentFormatFlags {
  pub fn from_bits(value: u8) -> Result<Self> {
    Ok(Self {
      facing_pages: value & 0x01 != 0,
      unused1: value & 0x02 != 0,
      mail_merge_main_document: value & 0x04 != 0,
      unused2: (value >> 3) & 0x03,
      footnote_placement: FootnotePlacement::from_u8((value >> 5) & 0x03)?,
      unused3: value & 0x80 != 0,
    })
  }

  pub fn bits(self) -> Result<u8> {
    if self.unused2 > 3 {
      return Err(Error::invalid(0, "DopBase unused2 exceeds 2 bits"));
    }
    Ok(
      u8::from(self.facing_pages)
        | (u8::from(self.unused1) << 1)
        | (u8::from(self.mail_merge_main_document) << 2)
        | (self.unused2 << 3)
        | (self.footnote_placement.to_u8() << 5)
        | (u8::from(self.unused3) << 7),
    )
  }
}

impl FootnotePlacement {
  fn from_u8(value: u8) -> Result<Self> {
    match value {
      0 => Ok(Self::EndOfSection),
      1 => Ok(Self::BottomOfPage),
      2 => Ok(Self::BeneathText),
      _ => Err(Error::invalid(0, "DopBase fpc is invalid")),
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::EndOfSection => 0,
      Self::BottomOfPage => 1,
      Self::BeneathText => 2,
    }
  }
}

impl NoteNumbering {
  pub fn from_bits(value: u16) -> Result<Self> {
    Ok(Self {
      restart: NoteNumberingRestart::from_u8((value & 3) as u8)?,
      starting_number: value >> 2,
    })
  }

  pub fn bits(self) -> Result<u16> {
    if self.starting_number > 0x3fff {
      return Err(Error::invalid(0, "note starting number exceeds 14 bits"));
    }
    Ok((self.starting_number << 2) | u16::from(self.restart.to_u8()))
  }
}

impl NoteNumberingRestart {
  fn from_u8(value: u8) -> Result<Self> {
    match value {
      0 => Ok(Self::Continuous),
      1 => Ok(Self::EachSection),
      2 => Ok(Self::EachPage),
      _ => Err(Error::invalid(0, "note restart mode is invalid")),
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::Continuous => 0,
      Self::EachSection => 1,
      Self::EachPage => 2,
    }
  }
}

impl DocumentStateFlags {
  pub fn from_bits(value: u32) -> Self {
    Self {
      unused5_to_10: (value & 0x3f) as u8,
      spelling_all_done: value & (1 << 6) != 0,
      spelling_all_clean: value & (1 << 7) != 0,
      hide_spelling_errors: value & (1 << 8) != 0,
      hide_grammar_errors: value & (1 << 9) != 0,
      labels_document: value & (1 << 10) != 0,
      hyphenate_capitals: value & (1 << 11) != 0,
      auto_hyphenate: value & (1 << 12) != 0,
      form_has_no_fields: value & (1 << 13) != 0,
      link_styles: value & (1 << 14) != 0,
      revision_marking: value & (1 << 15) != 0,
      unused11: value & (1 << 16) != 0,
      exact_statistics: value & (1 << 17) != 0,
      unused_page_hidden: value & (1 << 18) != 0,
      unused_page_results: value & (1 << 19) != 0,
      lock_annotations: value & (1 << 20) != 0,
      mirror_margins: value & (1 << 21) != 0,
      word97_compatibility: value & (1 << 22) != 0,
      unused12: value & (1 << 23) != 0,
      unused13: value & (1 << 24) != 0,
      form_protection: value & (1 << 25) != 0,
      display_form_field_selection: value & (1 << 26) != 0,
      revision_mark_view: value & (1 << 27) != 0,
      revision_mark_print: value & (1 << 28) != 0,
      lock_vba_project: value & (1 << 29) != 0,
      lock_revisions: value & (1 << 30) != 0,
      embed_fonts: value & (1 << 31) != 0,
    }
  }

  pub fn bits(self) -> Result<u32> {
    self.validate_protection_relations()?;
    if self.unused5_to_10 > 0x3f {
      return Err(Error::invalid(0, "DopBase unused5..unused10 exceed 6 bits"));
    }
    Ok(
      u32::from(self.unused5_to_10)
        | (u32::from(self.spelling_all_done) << 6)
        | (u32::from(self.spelling_all_clean) << 7)
        | (u32::from(self.hide_spelling_errors) << 8)
        | (u32::from(self.hide_grammar_errors) << 9)
        | (u32::from(self.labels_document) << 10)
        | (u32::from(self.hyphenate_capitals) << 11)
        | (u32::from(self.auto_hyphenate) << 12)
        | (u32::from(self.form_has_no_fields) << 13)
        | (u32::from(self.link_styles) << 14)
        | (u32::from(self.revision_marking) << 15)
        | (u32::from(self.unused11) << 16)
        | (u32::from(self.exact_statistics) << 17)
        | (u32::from(self.unused_page_hidden) << 18)
        | (u32::from(self.unused_page_results) << 19)
        | (u32::from(self.lock_annotations) << 20)
        | (u32::from(self.mirror_margins) << 21)
        | (u32::from(self.word97_compatibility) << 22)
        | (u32::from(self.unused12) << 23)
        | (u32::from(self.unused13) << 24)
        | (u32::from(self.form_protection) << 25)
        | (u32::from(self.display_form_field_selection) << 26)
        | (u32::from(self.revision_mark_view) << 27)
        | (u32::from(self.revision_mark_print) << 28)
        | (u32::from(self.lock_vba_project) << 29)
        | (u32::from(self.lock_revisions) << 30)
        | (u32::from(self.embed_fonts) << 31),
    )
  }

  pub fn validate_protection_relations(self) -> Result<()> {
    if self.lock_revisions && !self.revision_marking {
      return Err(Error::invalid(4, "DopBase fLockRev requires fRevMarking"));
    }
    if self.lock_revisions && self.lock_annotations {
      return Err(Error::invalid(
        4,
        "DopBase fLockRev and fLockAtn are mutually exclusive",
      ));
    }
    Ok(())
  }
}

impl EndnoteOptions {
  pub fn from_bits(value: u16) -> Result<Self> {
    if value & 0x4000 != 0 {
      return Err(Error::invalid(
        0,
        "DopBase endnote reserved2 bit is nonzero",
      ));
    }
    Ok(Self {
      placement: EndnotePlacement::from_u8((value & 3) as u8)?,
      unused14: ((value >> 2) & 0x0f) as u8,
      unused15: ((value >> 6) & 0x0f) as u8,
      print_form_data: value & 0x0400 != 0,
      save_form_data: value & 0x0800 != 0,
      shade_form_data: value & 0x1000 != 0,
      shade_merge_fields: value & 0x2000 != 0,
      include_subdocuments_in_statistics: value & 0x8000 != 0,
    })
  }

  pub fn bits(self) -> Result<u16> {
    if self.unused14 > 0x0f || self.unused15 > 0x0f {
      return Err(Error::invalid(
        0,
        "DopBase endnote unused field exceeds 4 bits",
      ));
    }
    Ok(
      u16::from(self.placement.to_u8())
        | (u16::from(self.unused14) << 2)
        | (u16::from(self.unused15) << 6)
        | (u16::from(self.print_form_data) << 10)
        | (u16::from(self.save_form_data) << 11)
        | (u16::from(self.shade_form_data) << 12)
        | (u16::from(self.shade_merge_fields) << 13)
        | (u16::from(self.include_subdocuments_in_statistics) << 15),
    )
  }
}

impl EndnotePlacement {
  fn from_u8(value: u8) -> Result<Self> {
    match value {
      0 => Ok(Self::EndOfSection),
      3 => Ok(Self::EndOfDocument),
      _ => Err(Error::invalid(0, "DopBase epc is invalid")),
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::EndOfSection => 0,
      Self::EndOfDocument => 3,
    }
  }
}

impl SavedView {
  pub fn from_bits(value: u16) -> Result<Self> {
    let zoom_percentage = (value >> 3) & 0x01ff;
    if zoom_percentage != 0 && !(10..=500).contains(&zoom_percentage) {
      return Err(Error::invalid(
        0,
        "DopBase pctWwdSaved is outside 0 or 10..=500",
      ));
    }
    Ok(Self {
      kind: SavedViewKind::from_u8((value & 7) as u8),
      zoom_percentage,
      zoom_kind: SavedZoomKind::from_u8(((value >> 12) & 3) as u8),
      unused: value & 0x4000 != 0,
      gutter_at_top: value & 0x8000 != 0,
    })
  }

  pub fn bits(self) -> Result<u16> {
    if self.zoom_percentage != 0 && !(10..=500).contains(&self.zoom_percentage) {
      return Err(Error::invalid(
        0,
        "DopBase pctWwdSaved is outside 0 or 10..=500",
      ));
    }
    Ok(
      u16::from(self.kind.to_u8())
        | (self.zoom_percentage << 3)
        | (u16::from(self.zoom_kind.to_u8()) << 12)
        | (u16::from(self.unused) << 14)
        | (u16::from(self.gutter_at_top) << 15),
    )
  }
}

impl SavedViewKind {
  fn from_u8(value: u8) -> Self {
    match value {
      0 => Self::None,
      1 => Self::Print,
      2 => Self::Outline,
      3 => Self::MasterPages,
      4 => Self::Normal,
      5 => Self::Web,
      6 => Self::Compatibility6,
      _ => Self::Compatibility7,
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::None => 0,
      Self::Print => 1,
      Self::Outline => 2,
      Self::MasterPages => 3,
      Self::Normal => 4,
      Self::Web => 5,
      Self::Compatibility6 => 6,
      Self::Compatibility7 => 7,
    }
  }

  pub fn is_standard(self) -> bool {
    !matches!(self, Self::Compatibility6 | Self::Compatibility7)
  }
}

impl SavedZoomKind {
  fn from_u8(value: u8) -> Self {
    match value {
      0 => Self::None,
      1 => Self::FullPage,
      2 => Self::BestFit,
      _ => Self::TextFit,
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::None => 0,
      Self::FullPage => 1,
      Self::BestFit => 2,
      Self::TextFit => 3,
    }
  }
}

impl DocumentDisplayFlags {
  pub fn from_bits(value: u16) -> Result<Self> {
    Ok(Self {
      unused1: value & 0x0001 != 0,
      outline_level: SavedOutlineLevel::from_u8(((value >> 1) & 0x0f) as u8)?,
      grammar_all_done: value & 0x0020 != 0,
      grammar_all_clean: value & 0x0040 != 0,
      subset_fonts: value & 0x0080 != 0,
      unused2: value & 0x0100 != 0,
      html_document: value & 0x0200 != 0,
      list_cache_invalid: value & 0x0400 != 0,
      snap_border: value & 0x0800 != 0,
      include_header: value & 0x1000 != 0,
      include_footer: value & 0x2000 != 0,
      unused3: value & 0x4000 != 0,
      unused4: value & 0x8000 != 0,
    })
  }

  pub fn bits(self) -> u16 {
    u16::from(self.unused1)
      | (u16::from(self.outline_level.to_u8()) << 1)
      | (u16::from(self.grammar_all_done) << 5)
      | (u16::from(self.grammar_all_clean) << 6)
      | (u16::from(self.subset_fonts) << 7)
      | (u16::from(self.unused2) << 8)
      | (u16::from(self.html_document) << 9)
      | (u16::from(self.list_cache_invalid) << 10)
      | (u16::from(self.snap_border) << 11)
      | (u16::from(self.include_header) << 12)
      | (u16::from(self.include_footer) << 13)
      | (u16::from(self.unused3) << 14)
      | (u16::from(self.unused4) << 15)
  }
}

impl SavedOutlineLevel {
  fn from_u8(value: u8) -> Result<Self> {
    match value {
      0 => Ok(Self::Heading1),
      1 => Ok(Self::Heading2),
      2 => Ok(Self::Heading3),
      3 => Ok(Self::Heading4),
      4 => Ok(Self::Heading5),
      5 => Ok(Self::Heading6),
      6 => Ok(Self::Heading7),
      7 => Ok(Self::Heading8),
      8 => Ok(Self::Heading9),
      9 => Ok(Self::All9),
      15 => Ok(Self::All15),
      _ => Err(Error::invalid(0, "Dop97 lvlDop is invalid")),
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::Heading1 => 0,
      Self::Heading2 => 1,
      Self::Heading3 => 2,
      Self::Heading4 => 3,
      Self::Heading5 => 4,
      Self::Heading6 => 5,
      Self::Heading7 => 6,
      Self::Heading8 => 7,
      Self::Heading9 => 8,
      Self::All9 => 9,
      Self::All15 => 15,
    }
  }
}

impl DocumentVersionFlags {
  pub fn from_bits(value: u16) -> Self {
    Self { unused: value }
  }

  pub fn bits(self) -> u16 {
    self.unused
  }
}

impl DocumentEvents {
  const ALLOWED_BITS: u32 = 0x0000_7f3f;

  pub fn from_bits(value: u32) -> Result<Self> {
    if value & !Self::ALLOWED_BITS != 0 {
      return Err(Error::invalid(0, "Dop97 grfDocEvents has reserved bits"));
    }
    Ok(Self {
      new: value & (1 << 0) != 0,
      open: value & (1 << 1) != 0,
      close: value & (1 << 2) != 0,
      sync: value & (1 << 3) != 0,
      xml_after_insert: value & (1 << 4) != 0,
      xml_before_delete: value & (1 << 5) != 0,
      building_block_after_insert: value & (1 << 8) != 0,
      building_block_before_delete: value & (1 << 9) != 0,
      building_block_on_exit: value & (1 << 10) != 0,
      building_block_on_enter: value & (1 << 11) != 0,
      store_update: value & (1 << 12) != 0,
      building_block_content_update: value & (1 << 13) != 0,
      lego_after_insert: value & (1 << 14) != 0,
    })
  }

  pub fn bits(self) -> u32 {
    u32::from(self.new)
      | (u32::from(self.open) << 1)
      | (u32::from(self.close) << 2)
      | (u32::from(self.sync) << 3)
      | (u32::from(self.xml_after_insert) << 4)
      | (u32::from(self.xml_before_delete) << 5)
      | (u32::from(self.building_block_after_insert) << 8)
      | (u32::from(self.building_block_before_delete) << 9)
      | (u32::from(self.building_block_on_exit) << 10)
      | (u32::from(self.building_block_on_enter) << 11)
      | (u32::from(self.store_update) << 12)
      | (u32::from(self.building_block_content_update) << 13)
      | (u32::from(self.lego_after_insert) << 14)
  }
}

impl VirusSessionInfo {
  pub fn from_bits(value: u32) -> Self {
    Self {
      prompted: value & 1 != 0,
      load_safe: value & 2 != 0,
      session_key: value >> 2,
    }
  }

  pub fn bits(self) -> Result<u32> {
    if self.session_key > 0x3fff_ffff {
      return Err(Error::invalid(0, "Dop97 virus session key exceeds 30 bits"));
    }
    Ok(u32::from(self.prompted) | (u32::from(self.load_safe) << 1) | (self.session_key << 2))
  }
}

impl DocumentDrawingGrid {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() != 10 {
      return Err(Error::invalid(0, "Dogrid length is not 10 bytes"));
    }
    Self::read(&mut SliceReader::new(bytes))
  }

  pub fn to_bytes(self) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(10);
    self.write(&mut bytes)?;
    Ok(bytes)
  }

  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let horizontal_origin = input.u16()?;
    let vertical_origin = input.u16()?;
    let horizontal_spacing = input.u16()?;
    let vertical_spacing = input.u16()?;
    let vertical_flags = input.u8()?;
    let horizontal_flags = input.u8()?;
    let value = Self {
      horizontal_origin,
      vertical_origin,
      horizontal_spacing,
      vertical_spacing,
      vertical_display_frequency: GridDisplayFrequency::from_bits(vertical_flags & 0x7f),
      unused: vertical_flags & 0x80 != 0,
      horizontal_display_frequency: GridDisplayFrequency::from_bits(horizontal_flags & 0x7f),
      follow_margins: horizontal_flags & 0x80 != 0,
    };
    value.validate()?;
    Ok(value)
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    for value in [
      self.horizontal_origin,
      self.vertical_origin,
      self.horizontal_spacing,
      self.vertical_spacing,
    ] {
      push_u16(bytes, value);
    }
    bytes.push(self.vertical_display_frequency.bits()? | (u8::from(self.unused) << 7));
    bytes.push(self.horizontal_display_frequency.bits()? | (u8::from(self.follow_margins) << 7));
    Ok(())
  }

  fn validate(self) -> Result<()> {
    if [
      self.horizontal_origin,
      self.vertical_origin,
      self.horizontal_spacing,
      self.vertical_spacing,
    ]
    .into_iter()
    .any(|value| value > 31_680)
    {
      return Err(Error::invalid(0, "Dogrid distance exceeds 31,680 twips"));
    }
    Ok(())
  }
}

impl GridDisplayFrequency {
  fn from_bits(value: u8) -> Self {
    if value == 0 {
      Self::DisabledCompatibility
    } else {
      Self::Every(value)
    }
  }

  fn bits(self) -> Result<u8> {
    match self {
      Self::DisabledCompatibility => Ok(0),
      Self::Every(value) if (1..=0x7f).contains(&value) => Ok(value),
      Self::Every(_) => Err(Error::invalid(0, "Dogrid display frequency exceeds 7 bits")),
    }
  }
}

impl DocumentTypography {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let flags_offset = input.offset as u64;
    let flags = input.u16()?;
    if flags & 0xf800 != 0 {
      return Err(Error::invalid(
        flags_offset,
        "DopTypography reserved flags are nonzero",
      ));
    }
    let following_punctuation_count = input.u16()?;
    if following_punctuation_count > 100 {
      return Err(Error::invalid(
        flags_offset + 2,
        "DopTypography cchFollowingPunct exceeds 100",
      ));
    }
    let leading_punctuation_count = input.u16()?;
    if leading_punctuation_count > 50 {
      return Err(Error::invalid(
        flags_offset + 4,
        "DopTypography cchLeadingPunct exceeds 50",
      ));
    }
    Ok(Self {
      kern_punctuation: flags & 0x0001 != 0,
      justification: TypographyJustification::from_u8(((flags >> 1) & 3) as u8)
        .ok_or_else(|| Error::invalid(flags_offset, "DopTypography iJustification is invalid"))?,
      kinsoku_level: KinsokuLevel::from_u8(((flags >> 3) & 3) as u8)
        .ok_or_else(|| Error::invalid(flags_offset, "DopTypography iLevelOfKinsoku is invalid"))?,
      print_two_on_one: flags & 0x0020 != 0,
      unused: flags & 0x0040 != 0,
      custom_kinsoku_language: CustomKinsokuLanguage::from_u8(((flags >> 7) & 7) as u8)
        .ok_or_else(|| Error::invalid(flags_offset, "DopTypography iCustomKsu is invalid"))?,
      japanese_use_level2: flags & 0x0400 != 0,
      following_punctuation_count,
      leading_punctuation_count,
      following_punctuation_slots: read_u16_array(input)?,
      leading_punctuation_slots: read_u16_array(input)?,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.following_punctuation_count > 100 {
      return Err(Error::invalid(
        0,
        "DopTypography cchFollowingPunct exceeds 100",
      ));
    }
    if self.leading_punctuation_count > 50 {
      return Err(Error::invalid(
        0,
        "DopTypography cchLeadingPunct exceeds 50",
      ));
    }
    push_u16(
      bytes,
      u16::from(self.kern_punctuation)
        | (u16::from(self.justification.to_u8()) << 1)
        | (u16::from(self.kinsoku_level.to_u8()) << 3)
        | (u16::from(self.print_two_on_one) << 5)
        | (u16::from(self.unused) << 6)
        | (u16::from(self.custom_kinsoku_language.to_u8()) << 7)
        | (u16::from(self.japanese_use_level2) << 10),
    );
    push_u16(bytes, self.following_punctuation_count);
    push_u16(bytes, self.leading_punctuation_count);
    write_u16_array(bytes, &self.following_punctuation_slots);
    write_u16_array(bytes, &self.leading_punctuation_slots);
    Ok(())
  }

  pub fn following_punctuation(&self) -> Result<&[u16]> {
    if self.following_punctuation_count > 100 {
      return Err(Error::invalid(
        0,
        "DopTypography cchFollowingPunct exceeds 100",
      ));
    }
    self
      .following_punctuation_slots
      .get(..usize::from(self.following_punctuation_count))
      .ok_or_else(|| Error::invalid(0, "DopTypography cchFollowingPunct exceeds slot count"))
  }

  pub fn leading_punctuation(&self) -> Result<&[u16]> {
    if self.leading_punctuation_count > 50 {
      return Err(Error::invalid(
        0,
        "DopTypography cchLeadingPunct exceeds 50",
      ));
    }
    self
      .leading_punctuation_slots
      .get(..usize::from(self.leading_punctuation_count))
      .ok_or_else(|| Error::invalid(0, "DopTypography cchLeadingPunct exceeds slot count"))
  }
}

impl TypographyJustification {
  fn from_u8(value: u8) -> Option<Self> {
    match value {
      0 => Some(Self::DoNotCompress),
      1 => Some(Self::CompressPunctuation),
      2 => Some(Self::CompressPunctuationAndJapaneseKana),
      _ => None,
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::DoNotCompress => 0,
      Self::CompressPunctuation => 1,
      Self::CompressPunctuationAndJapaneseKana => 2,
    }
  }
}

impl KinsokuLevel {
  fn from_u8(value: u8) -> Option<Self> {
    match value {
      0 => Some(Self::LanguageDefault),
      1 => Some(Self::JapaneseLevel2),
      2 => Some(Self::Custom),
      _ => None,
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::LanguageDefault => 0,
      Self::JapaneseLevel2 => 1,
      Self::Custom => 2,
    }
  }
}

impl CustomKinsokuLanguage {
  fn from_u8(value: u8) -> Option<Self> {
    match value {
      0 => Some(Self::None),
      1 => Some(Self::Japanese),
      2 => Some(Self::ChineseSimplified),
      3 => Some(Self::Korean),
      4 => Some(Self::ChineseTraditional),
      _ => None,
    }
  }

  fn to_u8(self) -> u8 {
    match self {
      Self::None => 0,
      Self::Japanese => 1,
      Self::ChineseSimplified => 2,
      Self::Korean => 3,
      Self::ChineseTraditional => 4,
    }
  }
}

impl DocumentClassification {
  fn from_i16(value: i16) -> Result<Self> {
    match value {
      0 => Ok(Self::NotSpecified),
      1 => Ok(Self::Letter),
      2 => Ok(Self::Email),
      _ => Err(Error::invalid(
        0,
        "Dop97 document classification is invalid",
      )),
    }
  }

  fn to_i16(self) -> i16 {
    match self {
      Self::NotSpecified => 0,
      Self::Letter => 1,
      Self::Email => 2,
    }
  }
}

impl DocumentProperties97Space {
  pub const fn new(bytes: [u8; 30]) -> Self {
    Self(bytes)
  }

  pub const fn bytes(&self) -> &[u8; 30] {
    &self.0
  }

  pub fn bytes_mut(&mut self) -> &mut [u8; 30] {
    &mut self.0
  }

  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    Ok(Self(input.take()?))
  }

  fn write(self, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&self.0);
  }
}

impl LastListIndexes {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    Ok(Self {
      bullet: input.u16()?,
      numbering: input.u16()?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) {
    push_u16(bytes, self.bullet);
    push_u16(bytes, self.numbering);
  }

  pub fn matches_override_count(self, override_count: usize) -> bool {
    if override_count == 0 {
      self.bullet == 0 && self.numbering == 0
    } else {
      usize::from(self.bullet) < override_count && usize::from(self.numbering) < override_count
    }
  }

  pub fn validate_override_count(self, override_count: usize) -> Result<()> {
    if !self.matches_override_count(override_count) {
      return Err(Error::invalid(
        476,
        format!(
          "Dop97 last list indexes {}/{} exceed PlfLfo count {override_count}",
          self.bullet, self.numbering
        ),
      ));
    }
    Ok(())
  }
}

impl DocumentCharacterCountPair {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    Ok(Self {
      main: input.i32()?,
      with_subdocuments: input.i32()?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) {
    push_i32(bytes, self.main);
    push_i32(bytes, self.with_subdocuments);
  }

  pub fn is_nonnegative(self) -> bool {
    self.main >= 0 && self.with_subdocuments >= 0
  }

  pub fn includes_main(self) -> bool {
    self.with_subdocuments >= self.main
  }

  pub fn validate_count_relation(self, name: &str) -> Result<()> {
    if !self.is_nonnegative() {
      return Err(Error::invalid(0, format!("Dop97 {name} count is negative")));
    }
    if !self.includes_main() {
      return Err(Error::invalid(
        0,
        format!("Dop97 {name} with-subdocuments count is below main count"),
      ));
    }
    Ok(())
  }
}

impl DocumentProperties97 {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let base = DocumentPropertiesBase::read(input)?;
    let compatibility_options_80 = CompatibilityOptions80::from_bits(input.u32()?);
    Ok(Self {
      base,
      compatibility_options_80,
      document_classification: DocumentClassification::from_i16(input.i16()?)?,
      typography: DocumentTypography::read(input)?,
      drawing_grid: DocumentDrawingGrid::read(input)?,
      display_flags: DocumentDisplayFlags::from_bits(input.u16()?)?,
      version_flags: DocumentVersionFlags::from_bits(input.u16()?),
      auto_summary: AutoSummaryInfo::read(input)?,
      characters_with_spaces: DocumentCharacterCountPair::read(input)?,
      document_events: DocumentEvents::from_bits(input.u32()?)?,
      virus_info: VirusSessionInfo::from_bits(input.u32()?),
      undefined_space: DocumentProperties97Space::read(input)?,
      maximum_list_cache_position: input.i32()?,
      last_list_indexes: LastListIndexes::read(input)?,
      double_byte_characters: DocumentCharacterCountPair::read(input)?,
      reserved3a: input.u32()?,
      footnote_number_format: NumberingFormat::from_u16(input.u16()?)?,
      endnote_number_format: NumberingFormat::from_u16(input.u16()?)?,
      pagination_zoom_font_size: input.u16()?,
      pagination_screen_height: input.u16()?,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.base.write(bytes)?;
    push_u32(bytes, self.compatibility_options_80.bits());
    bytes.extend_from_slice(&self.document_classification.to_i16().to_le_bytes());
    self.typography.write(bytes)?;
    self.drawing_grid.write(bytes)?;
    push_u16(bytes, self.display_flags.bits());
    push_u16(bytes, self.version_flags.bits());
    self.auto_summary.write(bytes)?;
    self.characters_with_spaces.write(bytes);
    push_u32(bytes, self.document_events.bits());
    push_u32(bytes, self.virus_info.bits()?);
    self.undefined_space.write(bytes);
    bytes.extend_from_slice(&self.maximum_list_cache_position.to_le_bytes());
    self.last_list_indexes.write(bytes);
    self.double_byte_characters.write(bytes);
    push_u32(bytes, self.reserved3a);
    push_u16(bytes, u16::from(self.footnote_number_format.code()));
    push_u16(bytes, u16::from(self.endnote_number_format.code()));
    push_u16(bytes, self.pagination_zoom_font_size);
    push_u16(bytes, self.pagination_screen_height);
    Ok(())
  }

  pub fn compatibility_options_match(&self) -> bool {
    self.compatibility_options_80.word6 == self.base.compatibility_options_60
  }

  pub fn validate_compatibility_options(&self) -> Result<()> {
    if !self.compatibility_options_match() {
      return Err(Error::invalid(
        84,
        "Copts80.copts60 differs from DopBase.copts60",
      ));
    }
    Ok(())
  }

  pub fn deprecated_numbering_field_cache_metadata(
    &self,
    location: Option<FibFcLcb>,
  ) -> DeprecatedNumberingFieldCacheMetadata {
    DeprecatedNumberingFieldCacheMetadata {
      location,
      maximum_valid_position: self.maximum_list_cache_position,
      invalid: self.display_flags.list_cache_invalid,
    }
  }

  pub fn validate_character_count_relations(&self) -> Result<()> {
    self
      .characters_with_spaces
      .validate_count_relation("characters-with-spaces")?;
    self
      .double_byte_characters
      .validate_count_relation("double-byte characters")?;
    Ok(())
  }
}

impl DocumentStoryStatistics {
  pub fn is_nonnegative(self) -> bool {
    self.words >= 0
      && self.characters >= 0
      && self.pages >= 0
      && self.paragraphs >= 0
      && self.lines >= 0
  }
}

impl DocumentStatistics {
  pub fn is_nonnegative(self) -> bool {
    self.main.is_nonnegative() && self.with_subdocuments.is_nonnegative()
  }

  pub fn includes_main(self) -> bool {
    self.with_subdocuments.words >= self.main.words
      && self.with_subdocuments.characters >= self.main.characters
      && self.with_subdocuments.pages >= self.main.pages
      && self.with_subdocuments.paragraphs >= self.main.paragraphs
      && self.with_subdocuments.lines >= self.main.lines
  }

  pub fn validate_count_relations(self) -> Result<()> {
    if !self.is_nonnegative() {
      return Err(Error::invalid(0, "DopBase document statistic is negative"));
    }
    if !self.includes_main() {
      return Err(Error::invalid(
        0,
        "DopBase with-subdocuments statistic is below main statistic",
      ));
    }
    Ok(())
  }

  pub fn exact(
    self,
    statistics_are_exact: bool,
    include_subdocuments: bool,
  ) -> Option<DocumentStoryStatistics> {
    statistics_are_exact.then_some(if include_subdocuments {
      self.with_subdocuments
    } else {
      self.main
    })
  }
}

impl DocumentPropertiesBase {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let format_flags = DocumentFormatFlags::from_bits(input.u8()?)?;
    let unused4 = input.u8()?;
    let footnote_numbering = NoteNumbering::from_bits(input.u16()?)?;
    let document_flags = DocumentStateFlags::from_bits(input.u32()?);
    document_flags.validate_protection_relations()?;
    let compatibility_options_60 = CompatibilityOptions60::from_bits(input.u16()?);
    let default_tab_width = input.i16()?;
    let web_code_page = CodePage(input.u16()?);
    let hyphenation_zone = input.u16()?;
    let consecutive_hyphen_limit = input.u16()?;
    let reserved2 = input.u16()?;
    if reserved2 != 0 {
      return Err(Error::invalid(18, "DopBase wSpare2 MUST be zero"));
    }
    let created = Dttm::from_u32(input.u32()?)?;
    let revised = Dttm::from_u32(input.u32()?)?;
    let last_printed = Dttm::from_u32(input.u32()?)?;
    let revision_count = input.i16()?;
    if revision_count < 0 {
      return Err(Error::invalid(
        32,
        "DopBase revision count is outside 0..=0x7fff",
      ));
    }
    let editing_time = input.i32()?;
    let words = input.i32()?;
    let characters = input.i32()?;
    let pages = input.i16()?;
    let paragraphs = input.i32()?;
    let endnote_numbering = NoteNumbering::from_bits(input.u16()?)?;
    let endnote_options = EndnoteOptions::from_bits(input.u16()?)?;
    let lines = input.i32()?;
    let words_with_subdocuments = input.i32()?;
    let characters_with_subdocuments = input.i32()?;
    let pages_with_subdocuments = input.i16()?;
    let paragraphs_with_subdocuments = input.i32()?;
    let lines_with_subdocuments = input.i32()?;
    let protection_password_hash = DocumentProtectionPasswordHash(input.i32()?);
    let saved_view = SavedView::from_bits(input.u16()?)?;
    let result = Self {
      format_flags,
      unused4,
      footnote_numbering,
      document_flags,
      compatibility_options_60,
      default_tab_width,
      web_code_page,
      hyphenation_zone,
      consecutive_hyphen_limit,
      reserved2,
      created,
      revised,
      last_printed,
      revision_count,
      editing_time,
      statistics: DocumentStatistics {
        main: DocumentStoryStatistics {
          words,
          characters,
          pages,
          paragraphs,
          lines,
        },
        with_subdocuments: DocumentStoryStatistics {
          words: words_with_subdocuments,
          characters: characters_with_subdocuments,
          pages: pages_with_subdocuments,
          paragraphs: paragraphs_with_subdocuments,
          lines: lines_with_subdocuments,
        },
      },
      endnote_numbering,
      endnote_options,
      protection_password_hash,
      saved_view,
    };
    result.validate()?;
    Ok(result)
  }

  pub fn exact_statistics(&self) -> Option<DocumentStoryStatistics> {
    self.statistics.exact(
      self.document_flags.exact_statistics,
      self.endnote_options.include_subdocuments_in_statistics,
    )
  }

  pub fn validate_revision_count(&self) -> Result<()> {
    if self.revision_count < 0 {
      return Err(Error::invalid(
        32,
        "DopBase revision count is outside 0..=0x7fff",
      ));
    }
    Ok(())
  }

  pub fn validate_reserved_fields(&self) -> Result<()> {
    if self.reserved2 != 0 {
      return Err(Error::invalid(18, "DopBase wSpare2 MUST be zero"));
    }
    Ok(())
  }

  pub fn validate(&self) -> Result<()> {
    self.document_flags.validate_protection_relations()?;
    self.validate_revision_count()?;
    self.validate_reserved_fields()?;
    self.created.validate()?;
    self.revised.validate()?;
    self.last_printed.validate()?;
    if let Some(statistics) = self.exact_statistics()
      && !statistics.is_nonnegative()
    {
      return Err(Error::invalid(
        38,
        "DopBase exact document statistic is negative",
      ));
    }
    Ok(())
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    bytes.extend_from_slice(&[self.format_flags.bits()?, self.unused4]);
    push_u16(bytes, self.footnote_numbering.bits()?);
    bytes.extend_from_slice(&self.document_flags.bits()?.to_le_bytes());
    push_u16(bytes, self.compatibility_options_60.bits());
    bytes.extend_from_slice(&self.default_tab_width.to_le_bytes());
    push_u16(bytes, self.web_code_page.0);
    push_u16(bytes, self.hyphenation_zone);
    push_u16(bytes, self.consecutive_hyphen_limit);
    push_u16(bytes, self.reserved2);
    push_u32(bytes, self.created.to_u32()?);
    push_u32(bytes, self.revised.to_u32()?);
    push_u32(bytes, self.last_printed.to_u32()?);
    bytes.extend_from_slice(&self.revision_count.to_le_bytes());
    for value in [
      self.editing_time,
      self.statistics.main.words,
      self.statistics.main.characters,
    ] {
      bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&self.statistics.main.pages.to_le_bytes());
    bytes.extend_from_slice(&self.statistics.main.paragraphs.to_le_bytes());
    push_u16(bytes, self.endnote_numbering.bits()?);
    push_u16(bytes, self.endnote_options.bits()?);
    for value in [
      self.statistics.main.lines,
      self.statistics.with_subdocuments.words,
      self.statistics.with_subdocuments.characters,
    ] {
      bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&self.statistics.with_subdocuments.pages.to_le_bytes());
    for value in [
      self.statistics.with_subdocuments.paragraphs,
      self.statistics.with_subdocuments.lines,
    ] {
      bytes.extend_from_slice(&value.to_le_bytes());
    }
    push_i32(bytes, self.protection_password_hash.0);
    push_u16(bytes, self.saved_view.bits()?);
    Ok(())
  }
}

fn read_u16_array<const N: usize>(input: &mut SliceReader<'_>) -> Result<[u16; N]> {
  let mut values = [0; N];
  for value in &mut values {
    *value = input.u16()?;
  }
  Ok(values)
}

fn write_u16_array(bytes: &mut Vec<u8>, values: &[u16]) {
  for value in values {
    push_u16(bytes, *value);
  }
}

impl FieldTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_bytes_with_compatibility(bytes, false)
  }

  pub(crate) fn from_bytes_with_compatibility(
    bytes: &[u8],
    preserve_separator_flag_mismatch: bool,
  ) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(6) {
      return Err(Error::invalid(
        0,
        "Plcfld length does not match 2-byte Fld records",
      ));
    }
    let field_count = (bytes.len() - 4) / 6;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(field_count + 1);
    for _ in 0..=field_count {
      positions.push(input.u32()?);
    }
    let mut descriptors = Vec::with_capacity(field_count);
    for _ in 0..field_count {
      descriptors.push(FieldDescriptor::read(&mut input)?);
    }
    require_strictly_increasing(&positions, "Plcfld CP")?;
    let terminal_position = positions[field_count];
    let positions = &positions[..field_count];
    let mut index = 0usize;
    let mut fields = Vec::new();
    while index < descriptors.len() {
      fields.push(Field::from_flat(positions, &descriptors, &mut index)?);
    }
    let value = Self {
      fields,
      terminal_position,
    };
    if !preserve_separator_flag_mismatch
      && let Some(position) = value.separator_flag_mismatches().next()
    {
      return Err(Error::invalid(
        u64::from(position),
        "Plcfld field separator and grffldEnd.fHasSep disagree",
      ));
    }
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut positions = Vec::new();
    let mut descriptors = Vec::new();
    for field in &self.fields {
      field.append_flat(&mut positions, &mut descriptors)?;
    }
    positions.push(self.terminal_position);
    require_strictly_increasing(&positions, "Plcfld CP")?;
    let mut bytes = Vec::with_capacity(positions.len() * 4 + descriptors.len() * 2);
    for position in &positions {
      push_u32(&mut bytes, *position);
    }
    for descriptor in descriptors {
      descriptor.write(&mut bytes)?;
    }
    Ok(bytes)
  }
}

impl Field {
  fn from_flat(
    positions: &[u32],
    descriptors: &[FieldDescriptor],
    index: &mut usize,
  ) -> Result<Self> {
    let begin_index = *index;
    let begin = match descriptors.get(begin_index).map(|value| value.character) {
      Some(FieldCharacter::Begin {
        reserved,
        field_type,
      }) => FieldBegin {
        position: positions[begin_index],
        reserved,
        field_type,
      },
      _ => {
        return Err(Error::invalid(
          positions
            .get(begin_index)
            .copied()
            .unwrap_or_default()
            .into(),
          "Plcfld FieldList entry does not begin with a field-begin Fld",
        ));
      }
    };
    *index += 1;

    let mut instruction_fields = Vec::new();
    while matches!(
      descriptors.get(*index).map(|value| value.character),
      Some(FieldCharacter::Begin { .. })
    ) {
      instruction_fields.push(Self::from_flat(positions, descriptors, index)?);
    }

    let separator = match descriptors.get(*index).map(|value| value.character) {
      Some(FieldCharacter::Separator { reserved, value }) => {
        let value = Some(FieldSeparator {
          position: positions[*index],
          reserved,
          value,
        });
        *index += 1;
        value
      }
      _ => None,
    };

    let mut result_fields = Vec::new();
    if separator.is_some() {
      while matches!(
        descriptors.get(*index).map(|value| value.character),
        Some(FieldCharacter::Begin { .. })
      ) {
        result_fields.push(Self::from_flat(positions, descriptors, index)?);
      }
    }

    let end_index = *index;
    let end = match descriptors.get(end_index).map(|value| value.character) {
      Some(FieldCharacter::End { reserved, flags }) => FieldEnd {
        position: positions[end_index],
        reserved,
        flags,
      },
      _ => {
        return Err(Error::invalid(
          positions.get(end_index).copied().unwrap_or_default().into(),
          "Plcfld FieldList field does not end with a field-end Fld",
        ));
      }
    };
    *index += 1;
    Ok(Self {
      begin,
      instruction_fields,
      separator,
      result_fields,
      end,
    })
  }

  fn append_flat(
    &self,
    positions: &mut Vec<u32>,
    descriptors: &mut Vec<FieldDescriptor>,
  ) -> Result<()> {
    positions.push(self.begin.position);
    descriptors.push(FieldDescriptor {
      character: FieldCharacter::Begin {
        reserved: self.begin.reserved,
        field_type: self.begin.field_type,
      },
    });
    for field in &self.instruction_fields {
      field.append_flat(positions, descriptors)?;
    }
    if let Some(separator) = self.separator {
      positions.push(separator.position);
      descriptors.push(FieldDescriptor {
        character: FieldCharacter::Separator {
          reserved: separator.reserved,
          value: separator.value,
        },
      });
    }
    for field in &self.result_fields {
      field.append_flat(positions, descriptors)?;
    }
    positions.push(self.end.position);
    descriptors.push(FieldDescriptor {
      character: FieldCharacter::End {
        reserved: self.end.reserved,
        flags: self.end.flags,
      },
    });
    Ok(())
  }

  pub fn contains_position(&self, position: u32) -> bool {
    self.begin.position < position && position < self.end.position
  }

  /// Returns the innermost field containing a document-part-relative CP.
  pub fn innermost_at(&self, position: u32) -> Option<&Self> {
    if !self.contains_position(position) {
      return None;
    }
    self
      .instruction_fields
      .iter()
      .chain(&self.result_fields)
      .find_map(|field| field.innermost_at(position))
      .or(Some(self))
  }
}

impl FieldTable {
  pub fn innermost_at(&self, position: u32) -> Option<&Field> {
    self
      .fields
      .iter()
      .find_map(|field| field.innermost_at(position))
  }

  pub fn separator_flag_mismatches(&self) -> impl Iterator<Item = u32> + '_ {
    self
      .fields
      .iter()
      .flat_map(Field::separator_flag_mismatches)
  }
}

impl Field {
  fn separator_flag_mismatches(&self) -> Box<dyn Iterator<Item = u32> + '_> {
    let own = (self.end.flags.contains(FieldEndFlags::HAS_SEPARATOR) != self.separator.is_some())
      .then_some(self.end.position)
      .into_iter();
    Box::new(
      own.chain(
        self
          .instruction_fields
          .iter()
          .chain(&self.result_fields)
          .flat_map(Self::separator_flag_mismatches),
      ),
    )
  }
}

impl FieldDescriptor {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let raw_character = input.u8()?;
    let reserved = raw_character >> 5;
    let value = input.u8()?;
    let character = match raw_character & 0x1f {
      0x13 => FieldCharacter::Begin {
        reserved,
        field_type: value,
      },
      0x14 => FieldCharacter::Separator { reserved, value },
      0x15 => FieldCharacter::End {
        reserved,
        flags: FieldEndFlags::from_bits_retain(value),
      },
      value => {
        return Err(Error::invalid(
          input.offset as u64 - 2,
          format!("invalid Fld character 0x{value:02x}"),
        ));
      }
    };
    Ok(Self { character })
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    let (character, reserved, value) = match self.character {
      FieldCharacter::Begin {
        reserved,
        field_type,
      } => (0x13, reserved, field_type),
      FieldCharacter::Separator { reserved, value } => (0x14, reserved, value),
      FieldCharacter::End { reserved, flags } => (0x15, reserved, flags.bits()),
    };
    if reserved > 0x07 {
      return Err(Error::invalid(0, "Fld reserved bits exceed three bits"));
    }
    bytes.extend_from_slice(&[character | (reserved << 5), value]);
    Ok(())
  }
}

impl BookmarkNames {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let extended_marker = input.u16()?;
    let count = usize::from(input.u16()?);
    let extra_data_size = input.u16()?;
    if extended_marker != 0xffff || extra_data_size != 0 {
      return Err(Error::invalid(
        0,
        "SttbfBkmk header is not UTF-16/no-extra-data",
      ));
    }
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
      let length = usize::from(input.u16()?);
      let mut name = Vec::with_capacity(length);
      for _ in 0..length {
        name.push(input.u16()?);
      }
      names.push(name);
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after SttbfBkmk",
      ));
    }
    Ok(Self {
      extended_marker,
      extra_data_size,
      names,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.extended_marker != 0xffff || self.extra_data_size != 0 {
      return Err(Error::invalid(0, "SttbfBkmk header changed"));
    }
    let mut bytes = Vec::new();
    push_u16(&mut bytes, self.extended_marker);
    push_u16(
      &mut bytes,
      u16::try_from(self.names.len())
        .map_err(|_| Error::Limit("SttbfBkmk count exceeds u16".into()))?,
    );
    push_u16(&mut bytes, self.extra_data_size);
    for name in &self.names {
      push_u16(
        &mut bytes,
        u16::try_from(name.len()).map_err(|_| Error::Limit("bookmark name exceeds u16".into()))?,
      );
      for character in name {
        push_u16(&mut bytes, *character);
      }
    }
    Ok(bytes)
  }
}

impl BookmarkStartTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(8) {
      return Err(Error::invalid(
        0,
        "Plcfbkf length does not match 4-byte FBKF records",
      ));
    }
    let bookmark_count = (bytes.len() - 4) / 8;
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(bookmark_count + 1);
    for _ in 0..=bookmark_count {
      positions.push(input.u32()?);
    }
    let mut bookmarks = Vec::with_capacity(bookmark_count);
    for _ in 0..bookmark_count {
      let end_index = input.u16()?;
      let value = input.u16()?;
      bookmarks.push(BookmarkStart {
        end_index,
        column_start: (value & 0x007f) as u8,
        published: value & 0x0080 != 0,
        column_limit: ((value >> 8) & 0x003f) as u8,
        native: value & 0x4000 != 0,
        column: value & 0x8000 != 0,
      });
    }
    require_nondecreasing(&positions, "Plcfbkf CP")?;
    Ok(Self {
      positions,
      bookmarks,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.len() != self.bookmarks.len().saturating_add(1) {
      return Err(Error::invalid(0, "Plcfbkf CP/FBKF cardinality changed"));
    }
    require_nondecreasing(&self.positions, "Plcfbkf CP")?;
    let mut bytes = Vec::with_capacity(self.positions.len() * 4 + self.bookmarks.len() * 4);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    for bookmark in &self.bookmarks {
      if bookmark.column_start > 0x7f || bookmark.column_limit > 0x3f {
        return Err(Error::invalid(0, "BKC bit field exceeds its width"));
      }
      push_u16(&mut bytes, bookmark.end_index);
      push_u16(
        &mut bytes,
        u16::from(bookmark.column_start)
          | (u16::from(bookmark.published) << 7)
          | (u16::from(bookmark.column_limit) << 8)
          | (u16::from(bookmark.native) << 14)
          | (u16::from(bookmark.column) << 15),
      );
    }
    Ok(bytes)
  }
}

impl BookmarkEndTable {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(4) {
      return Err(Error::invalid(0, "Plcfbkl is not an array of CPs"));
    }
    let mut input = SliceReader::new(bytes);
    let mut positions = Vec::with_capacity(bytes.len() / 4);
    while input.offset < bytes.len() {
      positions.push(input.u32()?);
    }
    require_nondecreasing(&positions, "Plcfbkl CP")?;
    Ok(Self { positions })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.positions.is_empty() {
      return Err(Error::invalid(0, "Plcfbkl has no terminal CP"));
    }
    require_nondecreasing(&self.positions, "Plcfbkl CP")?;
    let mut bytes = Vec::with_capacity(self.positions.len() * 4);
    for position in &self.positions {
      push_u32(&mut bytes, *position);
    }
    Ok(bytes)
  }
}

impl Bookmarks {
  pub fn from_bytes(names: &[u8], starts: &[u8], ends: &[u8]) -> Result<Self> {
    let names = BookmarkNames::from_bytes(names)?;
    let starts = BookmarkStartTable::from_bytes(starts)?;
    let ends = BookmarkEndTable::from_bytes(ends)?;
    let end_count = ends.positions.len().saturating_sub(1);
    if names.names.len() != starts.bookmarks.len() || starts.bookmarks.len() != end_count {
      return Err(Error::invalid(
        0,
        "parallel bookmark table cardinality differs",
      ));
    }
    for bookmark in &starts.bookmarks {
      if usize::from(bookmark.end_index) >= end_count {
        return Err(Error::invalid(0, "FBKF.ibkl is outside Plcfbkl"));
      }
    }
    Ok(Self {
      names,
      starts,
      ends,
    })
  }

  pub fn to_bytes(&self) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let end_count = self.ends.positions.len().saturating_sub(1);
    if self.names.names.len() != self.starts.bookmarks.len()
      || self.starts.bookmarks.len() != end_count
    {
      return Err(Error::invalid(
        0,
        "parallel bookmark table cardinality differs",
      ));
    }
    for bookmark in &self.starts.bookmarks {
      if usize::from(bookmark.end_index) >= end_count {
        return Err(Error::invalid(0, "FBKF.ibkl is outside Plcfbkl"));
      }
    }
    Ok((
      self.names.to_bytes()?,
      self.starts.to_bytes()?,
      self.ends.to_bytes()?,
    ))
  }
}

impl Sepx {
  pub fn from_word_document(word_document: &[u8], offset: i32) -> Result<Option<Self>> {
    if offset == -1 {
      return Ok(None);
    }
    let offset = usize::try_from(offset)
      .map_err(|_| Error::invalid(0, "negative Sepx offset other than -1"))?;
    let header = word_document
      .get(offset..offset + 2)
      .ok_or_else(|| Error::invalid(offset as u64, "Sepx length exceeds WordDocument"))?;
    let length = i16::from_le_bytes(header.try_into().unwrap());
    let length = usize::try_from(length)
      .map_err(|_| Error::invalid(offset as u64, "negative Sepx grpprl length"))?;
    let start = offset + 2;
    let end = start
      .checked_add(length)
      .ok_or_else(|| Error::invalid(offset as u64, "Sepx bounds overflow"))?;
    let body = word_document
      .get(start..end)
      .ok_or_else(|| Error::invalid(offset as u64, "Sepx exceeds WordDocument"))?;
    let (properties, trailing_byte) = match GrpPrl::from_bytes(body) {
      Ok(properties) => (properties, None),
      Err(original_error) => {
        let Some((&trailing_byte, prefix)) = body.split_last() else {
          return Err(original_error);
        };
        match GrpPrl::from_bytes(prefix) {
          Ok(properties) => (properties, Some(trailing_byte)),
          Err(_) => return Err(original_error),
        }
      }
    };
    Ok(Some(Self {
      properties,
      trailing_byte,
    }))
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut body = self.properties.to_bytes()?;
    if let Some(value) = self.trailing_byte {
      body.push(value);
    }
    let length =
      i16::try_from(body.len()).map_err(|_| Error::Limit("Sepx grpprl exceeds i16".into()))?;
    let mut bytes = Vec::with_capacity(body.len() + 2);
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(&body);
    Ok(bytes)
  }
}

impl StyleSheet {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let info_length = usize::from(input.u16()?);
    let info_bytes = input.bytes(info_length)?;
    let info = StyleSheetInfo::from_bytes(info_bytes)?;
    let mut styles = Vec::with_capacity(usize::from(info.header.style_count));
    for style_index in 0..info.header.style_count {
      if bytes.len() - input.offset < 2 {
        return Err(Error::invalid(
          input.offset as u64,
          format!("STSH ends before LPStd {style_index} length"),
        ));
      }
      let length = input.i16()?;
      let length = usize::try_from(length)
        .map_err(|_| Error::invalid(input.offset as u64 - 2, "negative LPStd length"))?;
      let definition = if length == 0 {
        None
      } else {
        if length > bytes.len() - input.offset {
          return Err(Error::invalid(
            input.offset as u64,
            format!(
              "LPStd {style_index} declares {length} bytes with {} remaining",
              bytes.len() - input.offset
            ),
          ));
        }
        let definition_offset = input.offset;
        let payload = input.bytes(length)?;
        Some(
          StyleDefinition::from_bytes(payload, info.header.std_base_size).map_err(|error| {
            Error::invalid(
              definition_offset as u64,
              format!("LPStd {style_index} ({length} bytes): {error}"),
            )
          })?,
        )
      };
      let alignment_padding = if length % 2 == 1 {
        Some(input.u8()?)
      } else {
        None
      };
      styles.push(LengthPrefixedStyle {
        definition,
        alignment_padding,
      });
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after STSH style array",
      ));
    }
    Ok(Self { info, styles })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.styles.len() != usize::from(self.info.header.style_count) {
      return Err(Error::invalid(
        0,
        "STSH style count does not match Stshif.cstd",
      ));
    }
    let info = self.info.to_bytes()?;
    let info_length =
      u16::try_from(info.len()).map_err(|_| Error::Limit("STSHI exceeds u16".into()))?;
    let mut bytes = Vec::new();
    push_u16(&mut bytes, info_length);
    bytes.extend_from_slice(&info);
    for style in &self.styles {
      let definition = style
        .definition
        .as_ref()
        .map(StyleDefinition::to_bytes)
        .transpose()?
        .unwrap_or_default();
      let length =
        i16::try_from(definition.len()).map_err(|_| Error::Limit("STD exceeds i16".into()))?;
      bytes.extend_from_slice(&length.to_le_bytes());
      bytes.extend_from_slice(&definition);
      match (definition.len() % 2, style.alignment_padding) {
        (1, Some(value)) => bytes.push(value),
        (1, None) => {
          return Err(Error::invalid(0, "odd LPStd is missing alignment padding"));
        }
        (0, None) => {}
        (0, Some(_)) => {
          return Err(Error::invalid(0, "even LPStd has alignment padding"));
        }
        _ => unreachable!(),
      }
    }
    Ok(bytes)
  }
}

impl StyleDefinition {
  pub fn from_bytes(bytes: &[u8], std_base_size: u16) -> Result<Self> {
    let std_base_size = usize::from(std_base_size);
    if !matches!(std_base_size, 10 | 18) {
      return Err(Error::invalid(
        0,
        format!("unsupported cbSTDBaseInFile {std_base_size}"),
      ));
    }
    if bytes.len() < std_base_size {
      return Err(Error::invalid(0, "STD is shorter than Stdf"));
    }
    let mut input = SliceReader::new(bytes);
    let base = StdfBase::read(&mut input)?;
    let post_2000 = if std_base_size == 18 {
      Some(StdfPost2000::read(&mut input)?)
    } else {
      None
    };
    let name = Xstz::read(&mut input)?;
    let formatting = StyleFormatting::read(&mut input, &base, post_2000)?;
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after GrLPUpxSw",
      ));
    }
    Ok(Self {
      base,
      post_2000,
      name,
      formatting,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    self.base.write(&mut bytes)?;
    if let Some(post_2000) = self.post_2000 {
      post_2000.write(&mut bytes)?;
    }
    self.name.write(&mut bytes)?;
    let revision_marked = matches!(
      self.formatting,
      StyleFormatting::RevisionParagraph { .. } | StyleFormatting::RevisionCharacter { .. }
    );
    if self.post_2000.is_some_and(|post| post.has_original_style) != revision_marked {
      return Err(Error::invalid(
        0,
        "StdfPost2000 original-style flag and formatting mismatch",
      ));
    }
    self.formatting.write(&mut bytes, &self.base)?;
    if usize::from(self.base.byte_count) != bytes.len() {
      return Err(Error::invalid(6, "StdfBase.bchUpe does not match STD size"));
    }
    Ok(bytes)
  }
}

impl StyleFormatting {
  fn read(
    input: &mut SliceReader<'_>,
    base: &StdfBase,
    post_2000: Option<StdfPost2000>,
  ) -> Result<Self> {
    let revision_marked = post_2000.is_some_and(|post| post.has_original_style);
    match (base.style_kind, base.formatting_count, revision_marked) {
      (StyleKind::Paragraph, 2, false) => Ok(Self::Paragraph {
        paragraph: StylePapx::read(input)?,
        character: StyleGrpPrl::read(input)?,
      }),
      (StyleKind::Character, 1, false) => Ok(Self::Character {
        character: StyleGrpPrl::read(input)?,
      }),
      (StyleKind::Paragraph, 3, true) => {
        let paragraph = StylePapx::read(input)?;
        let character = StyleGrpPrl::read(input)?;
        let (revision, original_paragraph, original_character) =
          StyleRevision::read_paragraph(input)?;
        Ok(Self::RevisionParagraph {
          paragraph,
          character,
          revision,
          original_paragraph,
          original_character,
        })
      }
      (StyleKind::Character, 2, true) => {
        let character = StyleGrpPrl::read(input)?;
        let (revision, original_character) = StyleRevision::read_character(input)?;
        Ok(Self::RevisionCharacter {
          character,
          revision,
          original_character,
        })
      }
      (StyleKind::Table, 3, false) => Ok(Self::Table {
        table: StyleGrpPrl::read(input)?,
        paragraph: StylePapx::read(input)?,
        character: StyleGrpPrl::read(input)?,
      }),
      (StyleKind::Numbering, 1, false) => Ok(Self::Numbering {
        paragraph: StylePapx::read(input)?,
      }),
      (kind, count, marked) => Err(Error::invalid(
        input.offset as u64,
        format!("invalid GrLPUpxSw shape {kind:?}/cupx={count}/revision={marked}"),
      )),
    }
  }

  fn write(&self, bytes: &mut Vec<u8>, base: &StdfBase) -> Result<()> {
    match (self, base.style_kind, base.formatting_count) {
      (
        Self::Paragraph {
          paragraph,
          character,
        },
        StyleKind::Paragraph,
        2,
      ) => {
        paragraph.write(bytes)?;
        character.write(bytes)?;
      }
      (Self::Character { character }, StyleKind::Character, 1) => {
        character.write(bytes)?;
      }
      (
        Self::RevisionParagraph {
          paragraph,
          character,
          revision,
          original_paragraph,
          original_character,
        },
        StyleKind::Paragraph,
        3,
      ) => {
        paragraph.write(bytes)?;
        character.write(bytes)?;
        revision.write_paragraph(bytes, original_paragraph, original_character)?;
      }
      (
        Self::RevisionCharacter {
          character,
          revision,
          original_character,
        },
        StyleKind::Character,
        2,
      ) => {
        character.write(bytes)?;
        revision.write_character(bytes, original_character)?;
      }
      (
        Self::Table {
          table,
          paragraph,
          character,
        },
        StyleKind::Table,
        3,
      ) => {
        table.write(bytes)?;
        paragraph.write(bytes)?;
        character.write(bytes)?;
      }
      (Self::Numbering { paragraph }, StyleKind::Numbering, 1) => {
        paragraph.write(bytes)?;
      }
      _ => return Err(Error::invalid(0, "GrLPUpxSw does not match StdfBase")),
    }
    Ok(())
  }

  pub fn property_count(&self) -> usize {
    match self {
      Self::Paragraph {
        paragraph,
        character,
      } => paragraph.properties.properties.len() + character.properties.properties.len(),
      Self::Character { character } => character.properties.properties.len(),
      Self::RevisionParagraph {
        paragraph,
        character,
        original_paragraph,
        original_character,
        ..
      } => {
        paragraph.properties.properties.len()
          + character.properties.properties.len()
          + original_paragraph.properties.properties.len()
          + original_character.properties.properties.len()
      }
      Self::RevisionCharacter {
        character,
        original_character,
        ..
      } => character.properties.properties.len() + original_character.properties.properties.len(),
      Self::Table {
        table,
        paragraph,
        character,
      } => {
        table.properties.properties.len()
          + paragraph.properties.properties.len()
          + character.properties.properties.len()
      }
      Self::Numbering { paragraph } => paragraph.properties.properties.len(),
    }
  }
}

impl StyleRevision {
  fn read_header(input: &mut SliceReader<'_>) -> Result<Self> {
    let length = input.u16()?;
    if length != 6 {
      return Err(Error::invalid(
        input.offset as u64 - 2,
        format!("LPUpxRm has {length} bytes, expected 6"),
      ));
    }
    Ok(Self {
      modified: Dttm::from_u32(input.u32()?)?,
      author_index: input.i16()?,
    })
  }

  fn read_paragraph(input: &mut SliceReader<'_>) -> Result<(Self, StylePapx, StyleGrpPrl)> {
    let length = usize::from(input.u16()?);
    let mut body = SliceReader::new(input.bytes(length)?);
    let revision = Self::read_header(&mut body)?;
    let paragraph = StylePapx::read(&mut body)?;
    let character = StyleGrpPrl::read(&mut body)?;
    if body.offset != body.bytes.len() {
      return Err(Error::invalid(
        body.offset as u64,
        "trailing bytes in StkParaUpxGrLPUpxRM",
      ));
    }
    Ok((revision, paragraph, character))
  }

  fn read_character(input: &mut SliceReader<'_>) -> Result<(Self, StyleGrpPrl)> {
    let length = usize::from(input.u16()?);
    let mut body = SliceReader::new(input.bytes(length)?);
    let revision = Self::read_header(&mut body)?;
    let character = StyleGrpPrl::read(&mut body)?;
    if body.offset != body.bytes.len() {
      return Err(Error::invalid(
        body.offset as u64,
        "trailing bytes in StkCharUpxGrLPUpxRM",
      ));
    }
    Ok((revision, character))
  }

  fn write_header(self, bytes: &mut Vec<u8>) -> Result<()> {
    push_u16(bytes, 6);
    push_u32(bytes, self.modified.to_u32()?);
    bytes.extend_from_slice(&self.author_index.to_le_bytes());
    Ok(())
  }

  fn write_paragraph(
    self,
    bytes: &mut Vec<u8>,
    paragraph: &StylePapx,
    character: &StyleGrpPrl,
  ) -> Result<()> {
    let mut body = Vec::new();
    self.write_header(&mut body)?;
    paragraph.write(&mut body)?;
    character.write(&mut body)?;
    write_style_revision_body(bytes, &body)
  }

  fn write_character(self, bytes: &mut Vec<u8>, character: &StyleGrpPrl) -> Result<()> {
    let mut body = Vec::new();
    self.write_header(&mut body)?;
    character.write(&mut body)?;
    write_style_revision_body(bytes, &body)
  }
}

fn write_style_revision_body(bytes: &mut Vec<u8>, body: &[u8]) -> Result<()> {
  if !body.len().is_multiple_of(2) {
    return Err(Error::invalid(
      0,
      "revision style wrapper is not even-sized",
    ));
  }
  push_u16(
    bytes,
    u16::try_from(body.len())
      .map_err(|_| Error::Limit("revision style wrapper exceeds u16".into()))?,
  );
  bytes.extend_from_slice(body);
  Ok(())
}

impl StyleGrpPrl {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let length = usize::from(input.u16()?);
    let properties = GrpPrl::from_bytes(input.bytes(length)?)?;
    let padding = if length % 2 == 1 {
      Some(input.u8()?)
    } else {
      None
    };
    Ok(Self {
      properties,
      padding,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    let body = self.properties.to_bytes()?;
    write_style_upx(bytes, &body, self.padding)
  }
}

impl StylePapx {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let length = usize::from(input.u16()?);
    if length < 2 {
      return Err(Error::invalid(
        input.offset as u64 - 2,
        "LPUpxPapx is shorter than istd",
      ));
    }
    let body = input.bytes(length)?;
    let style_index = u16::from_le_bytes([body[0], body[1]]);
    let properties = GrpPrl::from_bytes(&body[2..])?;
    let padding = if length % 2 == 1 {
      Some(input.u8()?)
    } else {
      None
    };
    Ok(Self {
      style_index,
      properties,
      padding,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    let properties = self.properties.to_bytes()?;
    let mut body = Vec::with_capacity(properties.len() + 2);
    push_u16(&mut body, self.style_index);
    body.extend_from_slice(&properties);
    write_style_upx(bytes, &body, self.padding)
  }
}

fn write_style_upx(bytes: &mut Vec<u8>, body: &[u8], padding: Option<u8>) -> Result<()> {
  push_u16(
    bytes,
    u16::try_from(body.len()).map_err(|_| Error::Limit("UPX exceeds u16".into()))?,
  );
  bytes.extend_from_slice(body);
  match (body.len() % 2, padding) {
    (1, Some(value)) => bytes.push(value),
    (0, None) => {}
    (1, None) => return Err(Error::invalid(0, "odd UPX is missing padding")),
    (0, Some(_)) => return Err(Error::invalid(0, "even UPX has padding")),
    _ => unreachable!(),
  }
  Ok(())
}

fn require_nondecreasing(values: &[u32], name: &str) -> Result<()> {
  if values.windows(2).any(|pair| pair[0] > pair[1]) {
    return Err(Error::invalid(
      0,
      format!("{name} values are not nondecreasing"),
    ));
  }
  Ok(())
}

fn require_strictly_increasing(values: &[u32], name: &str) -> Result<()> {
  if values.windows(2).any(|pair| pair[0] >= pair[1]) {
    return Err(Error::invalid(
      0,
      format!("{name} values are not strictly increasing"),
    ));
  }
  Ok(())
}

impl StdfBase {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let identity = input.u16()?;
    let inheritance = input.u16()?;
    let following = input.u16()?;
    Ok(Self {
      invariant_style_id: identity & 0x0fff,
      flags: StdfBaseFlags::from_bits_retain((identity >> 12) as u8),
      style_kind: StyleKind::from_raw((inheritance & 0x000f) as u8),
      base_style_index: inheritance >> 4,
      formatting_count: (following & 0x000f) as u8,
      next_style_index: following >> 4,
      byte_count: input.u16()?,
      general_flags: StyleGeneralFlags::from_bits_retain(input.u16()?),
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.invariant_style_id > 0x0fff
      || self.base_style_index > 0x0fff
      || self.next_style_index > 0x0fff
      || self.formatting_count > 0x0f
    {
      return Err(Error::invalid(0, "StdfBase bit field exceeds its width"));
    }
    push_u16(
      bytes,
      self.invariant_style_id | (u16::from(self.flags.bits()) << 12),
    );
    push_u16(
      bytes,
      u16::from(self.style_kind.raw()) | (self.base_style_index << 4),
    );
    push_u16(
      bytes,
      u16::from(self.formatting_count) | (self.next_style_index << 4),
    );
    push_u16(bytes, self.byte_count);
    push_u16(bytes, self.general_flags.bits());
    Ok(())
  }
}

impl StyleKind {
  fn from_raw(value: u8) -> Self {
    match value {
      1 => Self::Paragraph,
      2 => Self::Character,
      3 => Self::Table,
      4 => Self::Numbering,
      value => Self::Compatibility(value),
    }
  }

  fn raw(self) -> u8 {
    match self {
      Self::Paragraph => 1,
      Self::Character => 2,
      Self::Table => 3,
      Self::Numbering => 4,
      Self::Compatibility(value) => value,
    }
  }
}

impl StdfPost2000 {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let link = input.u16()?;
    let revision_save_id = input.u32()?;
    let priority = input.u16()?;
    Ok(Self {
      linked_style_index: link & 0x0fff,
      has_original_style: link & 0x1000 != 0,
      spare: ((link >> 13) & 0x07) as u8,
      revision_save_id,
      html_font_index: (priority & 0x0007) as u8,
      unused: priority & 0x0008 != 0,
      priority: priority >> 4,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.linked_style_index > 0x0fff
      || self.spare > 0x07
      || self.html_font_index > 0x07
      || self.priority > 0x0fff
    {
      return Err(Error::invalid(
        0,
        "StdfPost2000 bit field exceeds its width",
      ));
    }
    push_u16(
      bytes,
      self.linked_style_index
        | (u16::from(self.has_original_style) << 12)
        | (u16::from(self.spare) << 13),
    );
    push_u32(bytes, self.revision_save_id);
    push_u16(
      bytes,
      u16::from(self.html_font_index) | (u16::from(self.unused) << 3) | (self.priority << 4),
    );
    Ok(())
  }
}

impl Xstz {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    input.sdk_object()
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    write_sdk_object(bytes, self)
  }
}

fn validate_xstz_encoding(value: &Xstz) -> Result<()> {
  if value.terminator != 0 {
    return Err(Error::invalid(0, "Xstz terminator must be zero"));
  }
  Ok(())
}

impl StyleSheetInfo {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 18 {
      return Err(Error::invalid(0, "STSHI is shorter than Stshif"));
    }
    let mut input = SliceReader::new(bytes);
    let header = Stshif::read(&mut input)?;
    let bidi_font_index = if bytes.len() >= 20 {
      Some(input.i16()?)
    } else {
      None
    };
    let (latent_styles, standard_character_properties, standard_paragraph_properties) =
      if input.offset == bytes.len() {
        (None, None, None)
      } else {
        let entry_size = input.u16()?;
        if !matches!(entry_size, 0 | 4) {
          return Err(Error::invalid(
            input.offset as u64 - 2,
            "StshiLsd.cbLSD is neither 4 nor the known zero compatibility value",
          ));
        }
        let entry_count = usize::from(header.max_builtin_style);
        let mut entries = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
          entries.push(LatentStyleData::from_raw(input.u32()?));
        }
        // One Word 2007 producer shape declares cbLSD=0 and emits five
        // additional zero LSD slots before the two STSHIB property runs.
        // Preserve those slots as typed LSD values rather than opaque tail bytes.
        if entry_size == 0 && bytes.len().saturating_sub(input.offset) == 28 {
          for _ in 0..5 {
            entries.push(LatentStyleData::from_raw(input.u32()?));
          }
        }
        let character = if input.offset < bytes.len() {
          Some(read_stshi_grpprl(&mut input)?)
        } else {
          None
        };
        let paragraph = if input.offset < bytes.len() {
          Some(read_stshi_grpprl(&mut input)?)
        } else {
          None
        };
        if input.offset != bytes.len() {
          return Err(Error::invalid(
            input.offset as u64,
            "trailing bytes after STSHIB",
          ));
        }
        (
          Some(StshiLsd {
            entry_size,
            entries,
          }),
          character,
          paragraph,
        )
      };
    Ok(Self {
      header,
      bidi_font_index,
      latent_styles,
      standard_character_properties,
      standard_paragraph_properties,
    })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    self.header.write(&mut bytes);
    if let Some(value) = self.bidi_font_index {
      bytes.extend_from_slice(&value.to_le_bytes());
    }
    match (
      &self.latent_styles,
      &self.standard_character_properties,
      &self.standard_paragraph_properties,
    ) {
      (None, None, None) => {}
      (Some(latent), character, paragraph) if paragraph.is_none() || character.is_some() => {
        if self.bidi_font_index.is_none() {
          return Err(Error::invalid(0, "extended STSHI is missing ftcBi"));
        }
        let declared_entries = usize::from(self.header.max_builtin_style);
        let expected_entries = if latent.entry_size == 0 {
          declared_entries + 5
        } else {
          declared_entries
        };
        if !matches!(latent.entry_size, 0 | 4) || latent.entries.len() != expected_entries {
          return Err(Error::invalid(0, "StshiLsd shape changed"));
        }
        push_u16(&mut bytes, latent.entry_size);
        for entry in &latent.entries {
          push_u32(&mut bytes, entry.raw());
        }
        if let Some(character) = character {
          write_stshi_grpprl(&mut bytes, character)?;
        }
        if let Some(paragraph) = paragraph {
          write_stshi_grpprl(&mut bytes, paragraph)?;
        }
      }
      _ => return Err(Error::invalid(0, "partial STSHI extension")),
    }
    Ok(bytes)
  }
}

impl LatentStyleData {
  fn from_raw(value: u32) -> Self {
    Self {
      locked: value & 0x0001 != 0,
      semi_hidden: value & 0x0002 != 0,
      unhide_when_used: value & 0x0004 != 0,
      quick_format: value & 0x0008 != 0,
      priority: ((value >> 4) & 0x0fff) as u16,
      reserved: (value >> 16) as u16,
    }
  }

  fn raw(self) -> u32 {
    u32::from(self.locked)
      | (u32::from(self.semi_hidden) << 1)
      | (u32::from(self.unhide_when_used) << 2)
      | (u32::from(self.quick_format) << 3)
      | (u32::from(self.priority & 0x0fff) << 4)
      | (u32::from(self.reserved) << 16)
  }
}

fn read_stshi_grpprl(input: &mut SliceReader<'_>) -> Result<GrpPrl> {
  let length = input.i32()?;
  let length = usize::try_from(length)
    .map_err(|_| Error::invalid(input.offset as u64 - 4, "negative LPStshiGrpPrl length"))?;
  GrpPrl::from_bytes(input.bytes(length)?)
}

fn write_stshi_grpprl(bytes: &mut Vec<u8>, properties: &GrpPrl) -> Result<()> {
  let body = properties.to_bytes()?;
  let length =
    i32::try_from(body.len()).map_err(|_| Error::Limit("LPStshiGrpPrl exceeds i32".into()))?;
  bytes.extend_from_slice(&length.to_le_bytes());
  bytes.extend_from_slice(&body);
  Ok(())
}

impl Stshif {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let style_count = input.u16()?;
    let std_base_size = input.u16()?;
    let flags = input.u16()?;
    Ok(Self {
      style_count,
      std_base_size,
      style_names_written: flags & 1 != 0,
      reserved: flags >> 1,
      max_builtin_style: input.u16()?,
      fixed_style_count: input.u16()?,
      builtin_name_version: input.u16()?,
      ascii_font_index: input.i16()?,
      east_asian_font_index: input.i16()?,
      other_font_index: input.i16()?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) {
    push_u16(bytes, self.style_count);
    push_u16(bytes, self.std_base_size);
    push_u16(
      bytes,
      u16::from(self.style_names_written) | ((self.reserved & 0x7fff) << 1),
    );
    push_u16(bytes, self.max_builtin_style);
    push_u16(bytes, self.fixed_style_count);
    push_u16(bytes, self.builtin_name_version);
    bytes.extend_from_slice(&self.ascii_font_index.to_le_bytes());
    bytes.extend_from_slice(&self.east_asian_font_index.to_le_bytes());
    bytes.extend_from_slice(&self.other_font_index.to_le_bytes());
  }
}

impl PlcBte {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(8) {
      return Err(Error::invalid(
        0,
        "PlcBte size does not contain whole entries",
      ));
    }
    let page_count = (bytes.len() - 4) / 8;
    let mut input = SliceReader::new(bytes);
    let mut file_positions = Vec::with_capacity(page_count + 1);
    for _ in 0..=page_count {
      file_positions.push(input.u32()?);
    }
    let mut pages = Vec::with_capacity(page_count);
    for _ in 0..page_count {
      let raw = input.u32()?;
      pages.push(FkpPageNumber {
        page_number: raw & 0x003f_ffff,
        unused: (raw >> 22) as u16,
      });
    }
    Ok(Self {
      file_positions,
      pages,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.file_positions.len() != self.pages.len().saturating_add(1) {
      return Err(Error::invalid(0, "PlcBte FC/page cardinality changed"));
    }
    let mut bytes = Vec::with_capacity(self.file_positions.len() * 4 + self.pages.len() * 4);
    for position in &self.file_positions {
      push_u32(&mut bytes, *position);
    }
    for page in &self.pages {
      if page.page_number > 0x003f_ffff || page.unused > 0x03ff {
        return Err(Error::invalid(0, "PnFkp field exceeds its bit width"));
      }
      push_u32(
        &mut bytes,
        page.page_number | (u32::from(page.unused) << 22),
      );
    }
    Ok(bytes)
  }
}

impl FkpPageNumber {
  pub fn byte_offset(self) -> Result<usize> {
    usize::try_from(self.page_number)
      .ok()
      .and_then(|page| page.checked_mul(512))
      .ok_or_else(|| Error::Limit("FKP page offset exceeds usize".into()))
  }
}

impl ChpxFkp {
  /// Builds a canonical 512-byte CHPX FKP layout from typed runs.
  ///
  /// Existing `property_offset` values are physical source coordinates and
  /// are ignored. Equal property blocks share one canonical allocation.
  pub fn with_canonical_layout(
    file_positions: Vec<u32>,
    mut runs: Vec<ChpxFkpRun>,
  ) -> Result<Self> {
    let run_count = runs.len();
    if file_positions.len() != run_count.saturating_add(1) || !(1..=0x65).contains(&run_count) {
      return Err(Error::invalid(0, "ChpxFkp run cardinality is invalid"));
    }
    require_strictly_increasing_u32(&file_positions, "ChpxFkp rgfc")?;
    let table_end = (run_count + 1)
      .checked_mul(4)
      .and_then(|value| value.checked_add(run_count))
      .ok_or_else(|| Error::Limit("ChpxFkp table size overflow".into()))?;
    let mut cursor = 511usize;
    let mut allocated = BTreeMap::<Vec<u8>, u16>::new();
    for run in &mut runs {
      let Some(properties) = &run.properties else {
        run.property_offset = None;
        continue;
      };
      let grpprl = properties.to_bytes()?;
      let length =
        u8::try_from(grpprl.len()).map_err(|_| Error::Limit("Chpx grpprl exceeds u8".into()))?;
      let mut block = Vec::with_capacity(grpprl.len() + 1);
      block.push(length);
      block.extend_from_slice(&grpprl);
      let offset = if let Some(offset) = allocated.get(&block) {
        *offset
      } else {
        let start = cursor
          .checked_sub(block.len())
          .map(|value| value & !1)
          .filter(|value| *value >= table_end)
          .ok_or_else(|| Error::Limit("ChpxFkp typed runs exceed one page".into()))?;
        cursor = start;
        let offset = u16::try_from(start)
          .map_err(|_| Error::Limit("Chpx property offset exceeds u16".into()))?;
        allocated.insert(block, offset);
        offset
      };
      run.property_offset = Some(offset);
    }
    let page = Self {
      file_positions,
      runs,
      unused_regions: Vec::new(),
    };
    page.to_bytes()?;
    Ok(page)
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() != 512 {
      return Err(Error::invalid(0, "ChpxFkp is not 512 bytes"));
    }
    let run_count = usize::from(bytes[511]);
    if !(1..=0x65).contains(&run_count) {
      return Err(Error::invalid(511, "ChpxFkp crun is outside 1..=0x65"));
    }
    let positions_end = (run_count + 1)
      .checked_mul(4)
      .ok_or_else(|| Error::Limit("ChpxFkp position table overflow".into()))?;
    let offsets_end = positions_end
      .checked_add(run_count)
      .ok_or_else(|| Error::Limit("ChpxFkp offset table overflow".into()))?;
    if offsets_end > 511 {
      return Err(Error::invalid(0, "ChpxFkp tables overlap crun"));
    }

    let mut input = SliceReader::new(bytes);
    let mut file_positions = Vec::with_capacity(run_count + 1);
    for _ in 0..=run_count {
      file_positions.push(input.u32()?);
    }
    let raw_offsets = input.bytes(run_count)?.to_vec();
    let mut used = [false; 512];
    used[..offsets_end].fill(true);
    used[511] = true;
    let mut runs = Vec::with_capacity(run_count);
    for raw_offset in raw_offsets {
      if raw_offset == 0 {
        runs.push(ChpxFkpRun {
          property_offset: None,
          properties: None,
        });
        continue;
      }
      let offset = usize::from(raw_offset) * 2;
      if offset < offsets_end || offset >= 511 {
        return Err(Error::invalid(
          offset as u64,
          "Chpx offset is outside property area",
        ));
      }
      let length = usize::from(bytes[offset]);
      let end = offset
        .checked_add(1 + length)
        .filter(|end| *end <= 511)
        .ok_or_else(|| Error::invalid(offset as u64, "Chpx exceeds FKP page"))?;
      used[offset..end].fill(true);
      runs.push(ChpxFkpRun {
        property_offset: Some(offset as u16),
        properties: Some(GrpPrl::from_bytes(&bytes[offset + 1..end])?.into()),
      });
    }
    let unused_regions = collect_unused_regions(bytes, &used);
    Ok(Self {
      file_positions,
      runs,
      unused_regions,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let run_count = self.runs.len();
    if self.file_positions.len() != run_count.saturating_add(1) || !(1..=0x65).contains(&run_count)
    {
      return Err(Error::invalid(0, "ChpxFkp run cardinality changed"));
    }
    let positions_end = (run_count + 1) * 4;
    let offsets_end = positions_end + run_count;
    let mut bytes = vec![0; 512];
    for region in &self.unused_regions {
      let start = usize::from(region.offset);
      let end = start
        .checked_add(region.bytes.len())
        .filter(|end| *end <= 512)
        .ok_or_else(|| Error::invalid(start as u64, "FKP unused region exceeds page"))?;
      bytes[start..end].copy_from_slice(&region.bytes);
    }
    for (index, position) in self.file_positions.iter().enumerate() {
      let start = index * 4;
      bytes[start..start + 4].copy_from_slice(&position.to_le_bytes());
    }
    let mut property_blocks = BTreeMap::<u16, Vec<u8>>::new();
    for (index, run) in self.runs.iter().enumerate() {
      match (run.property_offset, &run.properties) {
        (None, None) => bytes[positions_end + index] = 0,
        (Some(offset), Some(properties)) if offset % 2 == 0 => {
          let raw_offset = u8::try_from(offset / 2)
            .map_err(|_| Error::invalid(u64::from(offset), "Chpx offset exceeds u8"))?;
          bytes[positions_end + index] = raw_offset;
          let grpprl = properties.to_bytes()?;
          let length = u8::try_from(grpprl.len())
            .map_err(|_| Error::Limit("Chpx grpprl exceeds u8".into()))?;
          let mut block = Vec::with_capacity(grpprl.len() + 1);
          block.push(length);
          block.extend_from_slice(&grpprl);
          if let Some(existing) = property_blocks.insert(offset, block.clone())
            && existing != block
          {
            return Err(Error::invalid(
              u64::from(offset),
              "shared Chpx offset has conflicting properties",
            ));
          }
        }
        _ => return Err(Error::invalid(0, "Chpx offset/property presence changed")),
      }
    }
    let mut occupied = [false; 512];
    occupied[..offsets_end].fill(true);
    occupied[511] = true;
    for (offset, block) in property_blocks {
      let start = usize::from(offset);
      let end = start
        .checked_add(block.len())
        .filter(|end| *end <= 511)
        .ok_or_else(|| Error::invalid(start as u64, "Chpx property exceeds page"))?;
      if start < offsets_end || occupied[start..end].iter().any(|value| *value) {
        return Err(Error::invalid(start as u64, "Chpx property blocks overlap"));
      }
      occupied[start..end].fill(true);
      bytes[start..end].copy_from_slice(&block);
    }
    bytes[511] = run_count as u8;
    Ok(bytes)
  }
}

impl PapxFkp {
  /// Builds a canonical 512-byte PAPX FKP layout from typed runs.
  ///
  /// Existing `property_offset` values are physical source coordinates and
  /// are ignored. Equal PapxInFkp blocks share one canonical allocation.
  pub fn with_canonical_layout(
    file_positions: Vec<u32>,
    mut runs: Vec<PapxFkpRun>,
  ) -> Result<Self> {
    let run_count = runs.len();
    if file_positions.len() != run_count.saturating_add(1) || !(1..=0x1d).contains(&run_count) {
      return Err(Error::invalid(0, "PapxFkp run cardinality is invalid"));
    }
    require_strictly_increasing_u32(&file_positions, "PapxFkp rgfc")?;
    let table_end = (run_count + 1)
      .checked_mul(4)
      .and_then(|value| value.checked_add(run_count.checked_mul(13)?))
      .ok_or_else(|| Error::Limit("PapxFkp table size overflow".into()))?;
    let mut cursor = 511usize;
    let mut allocated = BTreeMap::<Vec<u8>, u16>::new();
    for run in &mut runs {
      let Some(properties) = &run.properties else {
        run.property_offset = None;
        continue;
      };
      let block = properties.to_block()?;
      let offset = if let Some(offset) = allocated.get(&block) {
        *offset
      } else {
        let start = cursor
          .checked_sub(block.len())
          .map(|value| value & !1)
          .filter(|value| *value >= table_end)
          .ok_or_else(|| Error::Limit("PapxFkp typed runs exceed one page".into()))?;
        cursor = start;
        let offset = u16::try_from(start)
          .map_err(|_| Error::Limit("Papx property offset exceeds u16".into()))?;
        allocated.insert(block, offset);
        offset
      };
      run.property_offset = Some(offset);
    }
    let page = Self {
      file_positions,
      runs,
      unused_regions: Vec::new(),
    };
    page.to_bytes()?;
    Ok(page)
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() != 512 {
      return Err(Error::invalid(0, "PapxFkp is not 512 bytes"));
    }
    let run_count = usize::from(bytes[511]);
    if !(1..=0x1d).contains(&run_count) {
      return Err(Error::invalid(511, "PapxFkp cpara is outside 1..=0x1d"));
    }
    let positions_end = (run_count + 1)
      .checked_mul(4)
      .ok_or_else(|| Error::Limit("PapxFkp position table overflow".into()))?;
    let bx_end = positions_end
      .checked_add(run_count * 13)
      .ok_or_else(|| Error::Limit("PapxFkp BxPap table overflow".into()))?;
    if bx_end > 511 {
      return Err(Error::invalid(0, "PapxFkp tables overlap cpara"));
    }

    let mut input = SliceReader::new(bytes);
    let mut file_positions = Vec::with_capacity(run_count + 1);
    for _ in 0..=run_count {
      file_positions.push(input.u32()?);
    }
    let mut bx_entries = Vec::with_capacity(run_count);
    for _ in 0..run_count {
      bx_entries.push((input.u8()?, input.take::<12>()?));
    }
    let mut used = [false; 512];
    used[..bx_end].fill(true);
    used[511] = true;
    let mut runs = Vec::with_capacity(run_count);
    for (raw_offset, paragraph_height_info) in bx_entries {
      if raw_offset == 0 {
        runs.push(PapxFkpRun {
          property_offset: None,
          paragraph_height_info,
          properties: None,
        });
        continue;
      }
      let offset = usize::from(raw_offset) * 2;
      if offset < bx_end || offset >= 511 {
        return Err(Error::invalid(
          offset as u64,
          "Papx offset is outside property area",
        ));
      }
      let cb = bytes[offset];
      let (length_encoding, body_start, body_length) = if cb == 0 {
        let cb_prime = *bytes
          .get(offset + 1)
          .ok_or_else(|| Error::invalid(offset as u64, "truncated extended Papx length"))?;
        if cb_prime == 0 {
          return Err(Error::invalid(
            offset as u64,
            "extended Papx length is zero",
          ));
        }
        (
          PapxLengthEncoding::ExtendedHalfWords,
          offset + 2,
          usize::from(cb_prime) * 2,
        )
      } else {
        (
          PapxLengthEncoding::HalfWordsMinusOne,
          offset + 1,
          usize::from(cb) * 2 - 1,
        )
      };
      let end = body_start
        .checked_add(body_length)
        .filter(|end| *end <= 511)
        .ok_or_else(|| Error::invalid(offset as u64, "PapxInFkp exceeds page"))?;
      if body_length < 2 {
        return Err(Error::invalid(offset as u64, "PapxInFkp lacks istd"));
      }
      used[offset..end].fill(true);
      let style_index = u16::from_le_bytes(
        bytes[body_start..body_start + 2]
          .try_into()
          .expect("two bytes were checked"),
      );
      let grpprl_bytes = &bytes[body_start + 2..end];
      let (properties, trailing_byte) = match GrpPrl::from_bytes(grpprl_bytes) {
        Ok(properties) => (properties, None),
        Err(original_error) if !grpprl_bytes.is_empty() => {
          match GrpPrl::from_bytes(&grpprl_bytes[..grpprl_bytes.len() - 1]) {
            Ok(properties) => (
              properties,
              Some(*grpprl_bytes.last().expect("non-empty bytes were checked")),
            ),
            Err(_) => {
              return Err(Error::invalid(
                offset as u64,
                format!(
                  "invalid Papx grpprl ({original_error}); body {:02x?}",
                  &bytes[body_start..end][..body_length.min(96)]
                ),
              ));
            }
          }
        }
        Err(error) => return Err(error),
      };
      runs.push(PapxFkpRun {
        property_offset: Some(offset as u16),
        paragraph_height_info,
        properties: Some(
          PapxInFkp {
            length_encoding,
            style_index,
            properties,
            trailing_byte,
          }
          .into(),
        ),
      });
    }
    Ok(Self {
      file_positions,
      runs,
      unused_regions: collect_unused_regions(bytes, &used),
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let run_count = self.runs.len();
    if self.file_positions.len() != run_count.saturating_add(1) || !(1..=0x1d).contains(&run_count)
    {
      return Err(Error::invalid(0, "PapxFkp run cardinality changed"));
    }
    let positions_end = (run_count + 1) * 4;
    let bx_end = positions_end + run_count * 13;
    let mut bytes = vec![0; 512];
    for region in &self.unused_regions {
      let start = usize::from(region.offset);
      let end = start
        .checked_add(region.bytes.len())
        .filter(|end| *end <= 512)
        .ok_or_else(|| Error::invalid(start as u64, "FKP unused region exceeds page"))?;
      bytes[start..end].copy_from_slice(&region.bytes);
    }
    for (index, position) in self.file_positions.iter().enumerate() {
      let start = index * 4;
      bytes[start..start + 4].copy_from_slice(&position.to_le_bytes());
    }
    let mut property_blocks = BTreeMap::<u16, Vec<u8>>::new();
    for (index, run) in self.runs.iter().enumerate() {
      let bx_offset = positions_end + index * 13;
      bytes[bx_offset + 1..bx_offset + 13].copy_from_slice(&run.paragraph_height_info);
      match (run.property_offset, &run.properties) {
        (None, None) => bytes[bx_offset] = 0,
        (Some(offset), Some(properties)) if offset % 2 == 0 => {
          bytes[bx_offset] = u8::try_from(offset / 2)
            .map_err(|_| Error::invalid(u64::from(offset), "Papx offset exceeds u8"))?;
          let block = properties.to_block()?;
          if let Some(existing) = property_blocks.insert(offset, block.clone())
            && existing != block
          {
            return Err(Error::invalid(
              u64::from(offset),
              "shared Papx offset has conflicting properties",
            ));
          }
        }
        _ => return Err(Error::invalid(0, "Papx offset/property presence changed")),
      }
    }
    let mut occupied = [false; 512];
    occupied[..bx_end].fill(true);
    occupied[511] = true;
    for (offset, block) in property_blocks {
      let start = usize::from(offset);
      let end = start
        .checked_add(block.len())
        .filter(|end| *end <= 511)
        .ok_or_else(|| Error::invalid(start as u64, "Papx property exceeds page"))?;
      if start < bx_end || occupied[start..end].iter().any(|value| *value) {
        return Err(Error::invalid(start as u64, "Papx property blocks overlap"));
      }
      occupied[start..end].fill(true);
      bytes[start..end].copy_from_slice(&block);
    }
    bytes[511] = run_count as u8;
    Ok(bytes)
  }
}

impl PapxInFkp {
  fn to_block(&self) -> Result<Vec<u8>> {
    let grpprl = self.properties.to_bytes()?;
    let mut body = Vec::with_capacity(grpprl.len() + 2);
    push_u16(&mut body, self.style_index);
    body.extend_from_slice(&grpprl);
    if let Some(value) = self.trailing_byte {
      body.push(value);
    }
    let mut block = Vec::new();
    match self.length_encoding {
      PapxLengthEncoding::HalfWordsMinusOne if body.len() % 2 == 1 => {
        let cb = body
          .len()
          .checked_add(1)
          .map(|length| length / 2)
          .and_then(|length| u8::try_from(length).ok())
          .filter(|value| *value != 0)
          .ok_or_else(|| Error::Limit("PapxInFkp short length exceeds u8".into()))?;
        block.push(cb);
      }
      PapxLengthEncoding::ExtendedHalfWords if body.len() % 2 == 0 => {
        let cb_prime = u8::try_from(body.len() / 2)
          .ok()
          .filter(|value| *value != 0)
          .ok_or_else(|| Error::Limit("PapxInFkp extended length exceeds u8".into()))?;
        block.push(0);
        block.push(cb_prime);
      }
      _ => {
        return Err(Error::invalid(
          0,
          "PapxInFkp body parity does not match its length encoding",
        ));
      }
    }
    block.extend_from_slice(&body);
    Ok(block)
  }
}

fn collect_unused_regions(bytes: &[u8], used: &[bool; 512]) -> Vec<FkpUnusedRegion> {
  let mut regions = Vec::new();
  let mut offset = 0usize;
  while offset < 512 {
    if used[offset] {
      offset += 1;
      continue;
    }
    let start = offset;
    while offset < 512 && !used[offset] {
      offset += 1;
    }
    regions.push(FkpUnusedRegion {
      offset: start as u16,
      bytes: bytes[start..offset].to_vec(),
    });
  }
  regions
}

fn require_strictly_increasing_u32(values: &[u32], label: &str) -> Result<()> {
  if let Some(pair) = values.windows(2).find(|pair| pair[0] >= pair[1]) {
    return Err(Error::invalid(
      u64::from(pair[1]),
      format!("{label} values are not strictly increasing"),
    ));
  }
  Ok(())
}

fn validate_spp_operand(value: &SppOperand) -> Result<()> {
  if value.ignored_long != 0 {
    return Err(Error::invalid(0, "SPPOperand fLong must be zero"));
  }
  let expected = value
    .last_style_index
    .checked_sub(value.first_style_index)
    .and_then(|distance| usize::from(distance).checked_add(1))
    .ok_or_else(|| {
      Error::invalid(
        0,
        "SPPOperand last style index precedes its first style index",
      )
    })?;
  if value.remapped_style_indices.len() != expected {
    return Err(Error::invalid(
      0,
      "SPPOperand remapping count does not match its inclusive style range",
    ));
  }
  Ok(())
}

impl GrpPrl {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let mut properties = Vec::new();
    while input.offset < bytes.len() {
      properties.push(Prl::read(&mut input)?);
    }
    Ok(Self { properties })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for property in &self.properties {
      property.write(&mut bytes)?;
    }
    Ok(bytes)
  }
}

impl Prl {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let opcode = input.u16()?;
    let sprm = Sprm::from_opcode(opcode);
    let operand = match sprm.operand_size {
      SprmOperandSize::Toggle => SprmOperand::Toggle(input.u8()?),
      SprmOperandSize::Byte if opcode == 0x3014 => {
        SprmOperand::SectionHeaderFooterFlags(SectionHeaderFooterFlags::from_bits(input.u8()?))
      }
      SprmOperandSize::Byte => SprmOperand::Byte(input.u8()?),
      SprmOperandSize::Word => SprmOperand::Word(input.take()?),
      SprmOperandSize::Dword => SprmOperand::Dword(input.take()?),
      SprmOperandSize::Word4 => SprmOperand::Word4(input.take()?),
      SprmOperandSize::Word5 => SprmOperand::Word5(input.take()?),
      SprmOperandSize::Variable if opcode == SPRM_T_DEF_TABLE => {
        let stored_length = usize::from(input.u16()?);
        let length = stored_length.checked_sub(1).ok_or_else(|| {
          Error::invalid(
            input.offset.saturating_sub(2) as u64,
            "zero long SPRM length",
          )
        })?;
        SprmOperand::TableDefinition(TDefTableOperand::from_bytes(input.bytes(length)?)?)
      }
      SprmOperandSize::Variable => {
        let length = usize::from(input.u8()?);
        let body = input.bytes(length)?;
        match opcode {
          SPRM_P_CHG_TABS => SprmOperand::ParagraphChangeTabs(PChgTabsOperand::from_bytes(body)?),
          0xc60d => SprmOperand::ParagraphChangeTabsPapx(PChgTabsPapxOperand::from_bytes(body)?),
          0xca71 | 0xc64d | 0xd687 => SprmOperand::Shading(Shd::from_bytes(body)?),
          0xca72 | 0xc64e..=0xc653 | 0xd234..=0xd237 | 0xd47f | 0xd680..=0xd686 => {
            SprmOperand::Border(Brc::from_bytes(body)?)
          }
          0xca57 | 0xca89 | 0xc66f | 0xd243 | 0xd667 => {
            SprmOperand::PropertyRevisionMark(PropRMark::from_bytes(body)?)
          }
          0xca76 => SprmOperand::CharacterFitText(CFitTextOperand::from_bytes(body)?),
          0xd605 => SprmOperand::TableBorders80(TableBordersOperand80::from_bytes(body)?),
          0xd613 => SprmOperand::TableBorders(TableBordersOperand::from_bytes(body)?),
          0xd620 => SprmOperand::TableBorder80(TableBrcOperand80::from_bytes(body)?),
          0xd62f => SprmOperand::TableBorder(TableBrcOperand::from_bytes(body)?),
          0xd632..=0xd634 | 0xd63e => SprmOperand::TableCellSpacing(Cssa::from_bytes(body)?),
          0xd61a..=0xd61d => SprmOperand::TableBorderColors(ColorRef::array_from_bytes(body)?),
          0xd609 => SprmOperand::TableShading80(Shd80::array_from_bytes(body)?),
          0xd612 | 0xd616 | 0xd670..=0xd672 => {
            SprmOperand::TableShading(Shd::array_from_bytes(body)?)
          }
          0xd660 => SprmOperand::TableShading(vec![Shd::from_bytes(body)?]),
          0xd642 => SprmOperand::TableCellHideMark(CellHideMarkOperand::from_bytes(body)?),
          0xd635 => SprmOperand::TableCellWidth(TableCellWidthOperand::from_bytes(body)?),
          0xc66c => {
            require_operand_len(body, 16, "PTIstdInfo")?;
            SprmOperand::ParagraphTableStyleInfo(body.try_into().unwrap())
          }
          0xc645 => SprmOperand::ParagraphNumberRevisionMark(NumRmOperand::from_bytes(body)?),
          0xca47 => SprmOperand::CharacterMajority(Box::new(GrpPrl::from_bytes(body)?)),
          0xca62 => {
            SprmOperand::CharacterDisplayFieldRevisionMark(DispFldRmOperand::from_bytes(body)?)
          }
          0xc601 | 0xca31 => {
            let mut body = SliceReader::new(body);
            SprmOperand::StylePermutation(body.sdk_object()?)
          }
          0xc666 | 0xca85 | 0xd66a => {
            SprmOperand::ConditionalFormatting(CnfOperand::from_bytes(body)?)
          }
          0xc63e => SprmOperand::AutoNumberedListData(AnldOperand::from_bytes(body)?),
          0xd202 => SprmOperand::OutlineListData(Box::new(OlstOperand::from_bytes(body)?)),
          _ => SprmOperand::Variable8(body.to_vec()),
        }
      }
      SprmOperandSize::ThreeBytes => SprmOperand::ThreeBytes(input.take()?),
    };
    Ok(Self { sprm, operand })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    let opcode = self.sprm.opcode()?;
    push_u16(bytes, opcode);
    match (&self.operand, self.sprm.operand_size) {
      (SprmOperand::Toggle(value), SprmOperandSize::Toggle) => bytes.push(*value),
      (SprmOperand::Byte(value), SprmOperandSize::Byte) if opcode != 0x3014 => bytes.push(*value),
      (SprmOperand::SectionHeaderFooterFlags(value), SprmOperandSize::Byte) if opcode == 0x3014 => {
        bytes.push(value.bits()?);
      }
      (SprmOperand::Word(value), SprmOperandSize::Word)
      | (SprmOperand::Word4(value), SprmOperandSize::Word4)
      | (SprmOperand::Word5(value), SprmOperandSize::Word5) => bytes.extend_from_slice(value),
      (SprmOperand::Dword(value), SprmOperandSize::Dword) => bytes.extend_from_slice(value),
      (SprmOperand::ThreeBytes(value), SprmOperandSize::ThreeBytes) => {
        bytes.extend_from_slice(value)
      }
      (SprmOperand::Variable8(value), SprmOperandSize::Variable) if opcode != SPRM_T_DEF_TABLE => {
        bytes.push(
          u8::try_from(value.len())
            .map_err(|_| Error::Limit("variable SPRM operand exceeds u8".into()))?,
        );
        bytes.extend_from_slice(value);
      }
      (SprmOperand::ParagraphChangeTabs(value), SprmOperandSize::Variable)
        if opcode == SPRM_P_CHG_TABS =>
      {
        write_variable8(bytes, &value.to_bytes()?)?;
      }
      (SprmOperand::ParagraphChangeTabsPapx(value), SprmOperandSize::Variable)
        if opcode == 0xc60d =>
      {
        write_variable8(bytes, &value.to_bytes()?)?;
      }
      (SprmOperand::Shading(value), SprmOperandSize::Variable)
        if matches!(opcode, 0xca71 | 0xc64d | 0xd687) =>
      {
        write_variable8(bytes, &value.to_bytes())?;
      }
      (SprmOperand::Border(value), SprmOperandSize::Variable)
        if opcode == 0xca72
          || (0xc64e..=0xc653).contains(&opcode)
          || (0xd234..=0xd237).contains(&opcode)
          || opcode == 0xd47f
          || (0xd680..=0xd686).contains(&opcode) =>
      {
        write_variable8(bytes, &value.to_bytes())?;
      }
      (SprmOperand::PropertyRevisionMark(value), SprmOperandSize::Variable)
        if matches!(opcode, 0xca57 | 0xca89 | 0xc66f | 0xd243 | 0xd667) =>
      {
        write_variable8(bytes, &value.to_bytes())?;
      }
      (SprmOperand::CharacterFitText(value), SprmOperandSize::Variable) if opcode == 0xca76 => {
        write_variable8(bytes, &value.to_bytes())?;
      }
      (SprmOperand::TableBorders80(value), SprmOperandSize::Variable) if opcode == 0xd605 => {
        write_variable8(bytes, &value.to_bytes()?)?;
      }
      (SprmOperand::TableBorders(value), SprmOperandSize::Variable) if opcode == 0xd613 => {
        write_variable8(bytes, &value.to_bytes())?;
      }
      (SprmOperand::TableBorder80(value), SprmOperandSize::Variable) if opcode == 0xd620 => {
        write_variable8(bytes, &value.to_bytes()?)?;
      }
      (SprmOperand::TableBorder(value), SprmOperandSize::Variable) if opcode == 0xd62f => {
        write_variable8(bytes, &value.to_bytes())?;
      }
      (SprmOperand::TableCellSpacing(value), SprmOperandSize::Variable)
        if matches!(opcode, 0xd632..=0xd634 | 0xd63e) =>
      {
        write_variable8(bytes, &value.to_bytes())?;
      }
      (SprmOperand::TableBorderColors(value), SprmOperandSize::Variable)
        if matches!(opcode, 0xd61a..=0xd61d) =>
      {
        write_variable8(bytes, &ColorRef::array_to_bytes(value))?;
      }
      (SprmOperand::TableShading80(value), SprmOperandSize::Variable) if opcode == 0xd609 => {
        write_variable8(bytes, &Shd80::array_to_bytes(value))?;
      }
      (SprmOperand::TableShading(value), SprmOperandSize::Variable)
        if matches!(opcode, 0xd612 | 0xd616 | 0xd670..=0xd672 | 0xd660) =>
      {
        write_variable8(bytes, &Shd::array_to_bytes(value))?;
      }
      (SprmOperand::TableCellHideMark(value), SprmOperandSize::Variable) if opcode == 0xd642 => {
        write_variable8(bytes, &value.to_bytes())?;
      }
      (SprmOperand::TableCellWidth(value), SprmOperandSize::Variable) if opcode == 0xd635 => {
        write_variable8(bytes, &value.to_bytes())?;
      }
      (SprmOperand::ParagraphTableStyleInfo(value), SprmOperandSize::Variable)
        if opcode == 0xc66c =>
      {
        write_variable8(bytes, value)?;
      }
      (SprmOperand::ParagraphNumberRevisionMark(value), SprmOperandSize::Variable)
        if opcode == 0xc645 =>
      {
        write_variable8(bytes, &value.to_bytes())?;
      }
      (SprmOperand::CharacterMajority(value), SprmOperandSize::Variable) if opcode == 0xca47 => {
        write_variable8(bytes, &value.to_bytes()?)?;
      }
      (SprmOperand::CharacterDisplayFieldRevisionMark(value), SprmOperandSize::Variable)
        if opcode == 0xca62 =>
      {
        write_variable8(bytes, &value.to_bytes())?;
      }
      (SprmOperand::StylePermutation(value), SprmOperandSize::Variable)
        if matches!(opcode, 0xc601 | 0xca31) =>
      {
        let mut body = Vec::new();
        write_sdk_object(&mut body, value)?;
        write_variable8(bytes, &body)?;
      }
      (SprmOperand::ConditionalFormatting(value), SprmOperandSize::Variable)
        if matches!(opcode, 0xc666 | 0xca85 | 0xd66a) =>
      {
        write_variable8(bytes, &value.to_bytes()?)?;
      }
      (SprmOperand::AutoNumberedListData(value), SprmOperandSize::Variable) if opcode == 0xc63e => {
        write_variable8(bytes, &value.to_bytes()?)?;
      }
      (SprmOperand::OutlineListData(value), SprmOperandSize::Variable) if opcode == 0xd202 => {
        write_variable8(bytes, &value.to_bytes()?)?;
      }
      (SprmOperand::Variable16PlusOne(value), SprmOperandSize::Variable)
        if opcode == SPRM_T_DEF_TABLE =>
      {
        let stored_length = value
          .len()
          .checked_add(1)
          .and_then(|length| u16::try_from(length).ok())
          .ok_or_else(|| Error::Limit("long SPRM operand exceeds u16".into()))?;
        push_u16(bytes, stored_length);
        bytes.extend_from_slice(value);
      }
      (SprmOperand::TableDefinition(value), SprmOperandSize::Variable)
        if opcode == SPRM_T_DEF_TABLE =>
      {
        let value = value.to_bytes()?;
        let stored_length = value
          .len()
          .checked_add(1)
          .and_then(|length| u16::try_from(length).ok())
          .ok_or_else(|| Error::Limit("TDefTable operand exceeds u16".into()))?;
        push_u16(bytes, stored_length);
        bytes.extend_from_slice(&value);
      }
      _ => return Err(Error::invalid(0, "SPRM operand shape does not match spra")),
    }
    Ok(())
  }
}

impl Sprm {
  pub fn from_opcode(opcode: u16) -> Self {
    Self {
      property_id: opcode & 0x01ff,
      special: opcode & 0x0200 != 0,
      group: SprmGroup::from_raw(((opcode >> 10) & 0x07) as u8),
      operand_size: SprmOperandSize::from_raw(((opcode >> 13) & 0x07) as u8),
    }
  }

  pub fn opcode(self) -> Result<u16> {
    if self.property_id > 0x01ff {
      return Err(Error::invalid(0, "SPRM property id exceeds nine bits"));
    }
    Ok(
      self.property_id
        | (u16::from(self.special) << 9)
        | (u16::from(self.group.raw()) << 10)
        | (u16::from(self.operand_size.raw()) << 13),
    )
  }

  pub fn kind(self) -> SprmKind {
    let opcode = self.opcode().unwrap_or(u16::MAX);
    match KnownSprm::from_opcode(opcode) {
      Some(value) => SprmKind::Known(value),
      None => SprmKind::Other(opcode),
    }
  }
}

impl SprmGroup {
  fn from_raw(value: u8) -> Self {
    match value {
      1 => Self::Paragraph,
      2 => Self::Character,
      3 => Self::Picture,
      4 => Self::Section,
      5 => Self::Table,
      value => Self::Compatibility(value),
    }
  }

  fn raw(self) -> u8 {
    match self {
      Self::Paragraph => 1,
      Self::Character => 2,
      Self::Picture => 3,
      Self::Section => 4,
      Self::Table => 5,
      Self::Compatibility(value) => value & 0x07,
    }
  }
}

impl SprmOperandSize {
  fn from_raw(value: u8) -> Self {
    match value {
      0 => Self::Toggle,
      1 => Self::Byte,
      2 => Self::Word,
      3 => Self::Dword,
      4 => Self::Word4,
      5 => Self::Word5,
      6 => Self::Variable,
      _ => Self::ThreeBytes,
    }
  }

  fn raw(self) -> u8 {
    match self {
      Self::Toggle => 0,
      Self::Byte => 1,
      Self::Word => 2,
      Self::Dword => 3,
      Self::Word4 => 4,
      Self::Word5 => 5,
      Self::Variable => 6,
      Self::ThreeBytes => 7,
    }
  }
}

impl PChgTabsOperand {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let deleted_count = usize::from(input.u8()?);
    if deleted_count > 64 {
      return Err(Error::invalid(0, "PChgTabs deleted count exceeds 64"));
    }
    let mut deleted_positions = Vec::with_capacity(deleted_count);
    for _ in 0..deleted_count {
      deleted_positions.push(input.i16()?);
    }
    let mut deleted = Vec::with_capacity(deleted_count);
    for position in deleted_positions {
      deleted.push(DeletedTabStop {
        position,
        close_distance: input.i16()?,
      });
    }

    let added_count = usize::from(input.u8()?);
    if added_count > 64 {
      return Err(Error::invalid(
        input.offset as u64,
        "PChgTabs added count exceeds 64",
      ));
    }
    let mut added_positions = Vec::with_capacity(added_count);
    for _ in 0..added_count {
      added_positions.push(input.i16()?);
    }
    let mut added = Vec::with_capacity(added_count);
    for position in added_positions {
      added.push(AddedTabStop {
        position,
        descriptor: input.sdk_object()?,
      });
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing PChgTabs bytes",
      ));
    }
    Ok(Self { deleted, added })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let deleted_count = u8::try_from(self.deleted.len())
      .map_err(|_| Error::Limit("PChgTabs deleted count exceeds u8".into()))?;
    let added_count = u8::try_from(self.added.len())
      .map_err(|_| Error::Limit("PChgTabs added count exceeds u8".into()))?;
    if deleted_count > 64 || added_count > 64 {
      return Err(Error::invalid(0, "PChgTabs count exceeds 64"));
    }
    let mut bytes = Vec::new();
    bytes.push(deleted_count);
    for tab in &self.deleted {
      bytes.extend_from_slice(&tab.position.to_le_bytes());
    }
    for tab in &self.deleted {
      bytes.extend_from_slice(&tab.close_distance.to_le_bytes());
    }
    bytes.push(added_count);
    for tab in &self.added {
      bytes.extend_from_slice(&tab.position.to_le_bytes());
    }
    for tab in &self.added {
      write_sdk_object(&mut bytes, &tab.descriptor)?;
    }
    Ok(bytes)
  }
}

impl PChgTabsPapxOperand {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let deleted_count = usize::from(input.u8()?);
    if deleted_count > 64 {
      return Err(Error::invalid(0, "PChgTabsPapx deleted count exceeds 64"));
    }
    let mut deleted_positions = Vec::with_capacity(deleted_count);
    for _ in 0..deleted_count {
      deleted_positions.push(input.i16()?);
    }

    let added_count = usize::from(input.u8()?);
    if added_count > 64 {
      return Err(Error::invalid(
        input.offset as u64,
        "PChgTabsPapx added count exceeds 64",
      ));
    }
    let mut added_positions = Vec::with_capacity(added_count);
    for _ in 0..added_count {
      added_positions.push(input.i16()?);
    }
    let mut added = Vec::with_capacity(added_count);
    for position in added_positions {
      added.push(AddedTabStop {
        position,
        descriptor: input.sdk_object()?,
      });
    }
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing PChgTabsPapx bytes",
      ));
    }
    Ok(Self {
      deleted_positions,
      added,
    })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let deleted_count = u8::try_from(self.deleted_positions.len())
      .map_err(|_| Error::Limit("PChgTabsPapx deleted count exceeds u8".into()))?;
    let added_count = u8::try_from(self.added.len())
      .map_err(|_| Error::Limit("PChgTabsPapx added count exceeds u8".into()))?;
    if deleted_count > 64 || added_count > 64 {
      return Err(Error::invalid(0, "PChgTabsPapx count exceeds 64"));
    }
    let mut bytes = Vec::new();
    bytes.push(deleted_count);
    for position in &self.deleted_positions {
      bytes.extend_from_slice(&position.to_le_bytes());
    }
    bytes.push(added_count);
    for tab in &self.added {
      bytes.extend_from_slice(&tab.position.to_le_bytes());
    }
    for tab in &self.added {
      write_sdk_object(&mut bytes, &tab.descriptor)?;
    }
    Ok(bytes)
  }
}

impl ColorRef {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    Ok(Self {
      red: input.u8()?,
      green: input.u8()?,
      blue: input.u8()?,
      auto: input.u8()?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&[self.red, self.green, self.blue, self.auto]);
  }

  fn array_from_bytes(bytes: &[u8]) -> Result<Vec<Self>> {
    if !bytes.len().is_multiple_of(4) {
      return Err(Error::invalid(
        0,
        "BrcCv array is not a multiple of 4 bytes",
      ));
    }
    let mut input = SliceReader::new(bytes);
    let mut values = Vec::with_capacity(bytes.len() / 4);
    while input.offset < bytes.len() {
      values.push(Self::read(&mut input)?);
    }
    Ok(values)
  }

  fn array_to_bytes(values: &[Self]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
      value.write(&mut bytes);
    }
    bytes
  }
}

impl Shd {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    require_operand_len(bytes, 10, "Shd")?;
    let mut input = SliceReader::new(bytes);
    Ok(Self {
      foreground: ColorRef::read(&mut input)?,
      background: ColorRef::read(&mut input)?,
      pattern: input.u16()?,
    })
  }

  fn to_bytes(self) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(10);
    self.foreground.write(&mut bytes);
    self.background.write(&mut bytes);
    push_u16(&mut bytes, self.pattern);
    bytes
  }

  fn array_from_bytes(bytes: &[u8]) -> Result<Vec<Self>> {
    if !bytes.len().is_multiple_of(10) || bytes.len() > 220 {
      return Err(Error::invalid(
        0,
        "DefTableShd array has an invalid byte count",
      ));
    }
    bytes.chunks_exact(10).map(Self::from_bytes).collect()
  }

  fn array_to_bytes(values: &[Self]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 10);
    for value in values {
      bytes.extend_from_slice(&value.to_bytes());
    }
    bytes
  }
}

impl Brc {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    require_operand_len(bytes, 8, "Brc")?;
    let mut input = SliceReader::new(bytes);
    Self::read(&mut input)
  }

  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let color = ColorRef::read(input)?;
    let line_width = input.u8()?;
    let border_type = input.u8()?;
    let flags = input.u16()?;
    Ok(Self {
      color,
      line_width,
      border_type,
      spacing: (flags & 0x001f) as u8,
      shadow: flags & 0x0020 != 0,
      frame: flags & 0x0040 != 0,
      reserved: flags >> 7,
    })
  }

  fn to_bytes(self) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    self.write(&mut bytes);
    bytes
  }

  fn write(self, bytes: &mut Vec<u8>) {
    self.color.write(bytes);
    bytes.push(self.line_width);
    bytes.push(self.border_type);
    let flags = u16::from(self.spacing & 0x1f)
      | (u16::from(self.shadow) << 5)
      | (u16::from(self.frame) << 6)
      | ((self.reserved & 0x01ff) << 7);
    push_u16(bytes, flags);
  }
}

impl PropRMark {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    require_operand_len(bytes, 7, "PropRMark")?;
    let mut input = SliceReader::new(bytes);
    Ok(Self {
      has_revision: input.u8()?,
      author_index: input.i16()?,
      timestamp: input.u32()?,
    })
  }

  fn to_bytes(self) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(7);
    bytes.push(self.has_revision);
    bytes.extend_from_slice(&self.author_index.to_le_bytes());
    bytes.extend_from_slice(&self.timestamp.to_le_bytes());
    bytes
  }
}

impl CFitTextOperand {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    require_operand_len(bytes, 8, "CFitText")?;
    let mut input = SliceReader::new(bytes);
    Ok(Self {
      width_twips: input.i32()?,
      fit_text_id: input.i32()?,
    })
  }

  fn to_bytes(self) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&self.width_twips.to_le_bytes());
    bytes.extend_from_slice(&self.fit_text_id.to_le_bytes());
    bytes
  }
}

impl CnfOperand {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 2 {
      return Err(Error::invalid(0, "CNFOperand is shorter than cnfc"));
    }
    let condition = i16::from_le_bytes([bytes[0], bytes[1]]);
    Ok(Self {
      condition,
      properties: Box::new(GrpPrl::from_bytes(&bytes[2..])?),
    })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let properties = self.properties.to_bytes()?;
    let mut bytes = Vec::with_capacity(properties.len() + 2);
    bytes.extend_from_slice(&self.condition.to_le_bytes());
    bytes.extend_from_slice(&properties);
    Ok(bytes)
  }
}

impl Anlv {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let number_format = input.u8()?;
    let text_before = input.u8()?;
    let text_after = input.u8()?;
    let flags1 = input.u8()?;
    let flags2 = input.u8()?;
    let appearance = input.u8()?;
    Ok(Self {
      number_format,
      text_before,
      text_after,
      justification: flags1 & 0x03,
      include_previous_levels: flags1 & 0x04 != 0,
      hanging_indent: flags1 & 0x08 != 0,
      set_bold: flags1 & 0x10 != 0,
      set_italic: flags1 & 0x20 != 0,
      set_small_caps: flags1 & 0x40 != 0,
      set_caps: flags1 & 0x80 != 0,
      set_strike: flags2 & 0x01 != 0,
      set_underline: flags2 & 0x02 != 0,
      previous_space: flags2 & 0x04 != 0,
      bold: flags2 & 0x08 != 0,
      italic: flags2 & 0x10 != 0,
      small_caps: flags2 & 0x20 != 0,
      caps: flags2 & 0x40 != 0,
      strike: flags2 & 0x80 != 0,
      underline: appearance & 0x07,
      color: appearance >> 3,
      font_index: input.u16()?,
      font_size_half_points: input.u16()?,
      start_at: input.u16()?,
      indent_twips: input.i16()?,
      space_twips: input.i16()?,
    })
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.justification > 0x03 || self.underline > 0x07 || self.color > 0x1f {
      return Err(Error::invalid(0, "ANLV bit field exceeds its width"));
    }
    let flags1 = self.justification
      | (u8::from(self.include_previous_levels) << 2)
      | (u8::from(self.hanging_indent) << 3)
      | (u8::from(self.set_bold) << 4)
      | (u8::from(self.set_italic) << 5)
      | (u8::from(self.set_small_caps) << 6)
      | (u8::from(self.set_caps) << 7);
    let flags2 = u8::from(self.set_strike)
      | (u8::from(self.set_underline) << 1)
      | (u8::from(self.previous_space) << 2)
      | (u8::from(self.bold) << 3)
      | (u8::from(self.italic) << 4)
      | (u8::from(self.small_caps) << 5)
      | (u8::from(self.caps) << 6)
      | (u8::from(self.strike) << 7);
    bytes.extend_from_slice(&[
      self.number_format,
      self.text_before,
      self.text_after,
      flags1,
      flags2,
      self.underline | (self.color << 3),
    ]);
    push_u16(bytes, self.font_index);
    push_u16(bytes, self.font_size_half_points);
    push_u16(bytes, self.start_at);
    bytes.extend_from_slice(&self.indent_twips.to_le_bytes());
    bytes.extend_from_slice(&self.space_twips.to_le_bytes());
    Ok(())
  }
}

impl SectionHeaderFooterFlags {
  pub fn from_bits(value: u8) -> Self {
    Self {
      even_header: value & 0x01 != 0,
      odd_header: value & 0x02 != 0,
      even_footer: value & 0x04 != 0,
      odd_footer: value & 0x08 != 0,
      first_header: value & 0x10 != 0,
      first_footer: value & 0x20 != 0,
      reserved: value >> 6,
    }
  }

  pub fn bits(self) -> Result<u8> {
    if self.reserved > 0x03 {
      return Err(Error::invalid(
        0,
        "section header/footer reserved value exceeds two bits",
      ));
    }
    Ok(
      u8::from(self.even_header)
        | (u8::from(self.odd_header) << 1)
        | (u8::from(self.even_footer) << 2)
        | (u8::from(self.odd_footer) << 3)
        | (u8::from(self.first_header) << 4)
        | (u8::from(self.first_footer) << 5)
        | (self.reserved << 6),
    )
  }
}

impl AnldOperand {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    require_operand_len(bytes, 84, "ANLD")?;
    let mut input = SliceReader::new(bytes);
    let level = Anlv::read(&mut input)?;
    let number_one_per_cell = input.u8()?;
    let number_across_cells = input.u8()?;
    let restart_heading = input.u8()?;
    let spare = input.u8()?;
    let mut display_text = [0u16; 32];
    for character in &mut display_text {
      *character = input.u16()?;
    }
    Ok(Self {
      level,
      number_one_per_cell,
      number_across_cells,
      restart_heading,
      spare,
      display_text,
    })
  }

  fn to_bytes(self) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(84);
    self.level.write(&mut bytes)?;
    bytes.extend_from_slice(&[
      self.number_one_per_cell,
      self.number_across_cells,
      self.restart_heading,
      self.spare,
    ]);
    for character in self.display_text {
      push_u16(&mut bytes, character);
    }
    Ok(bytes)
  }
}

impl OlstOperand {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    require_operand_len(bytes, 212, "OLST")?;
    let mut input = SliceReader::new(bytes);
    let mut levels = Vec::with_capacity(9);
    for _ in 0..9 {
      levels.push(Anlv::read(&mut input)?);
    }
    let restart_heading = input.u8()?;
    let reserved = input.take()?;
    let mut display_text = [0u16; 32];
    for character in &mut display_text {
      *character = input.u16()?;
    }
    Ok(Self {
      levels: levels.try_into().unwrap(),
      restart_heading,
      reserved,
      display_text,
    })
  }

  fn to_bytes(self) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(212);
    for level in self.levels {
      level.write(&mut bytes)?;
    }
    bytes.push(self.restart_heading);
    bytes.extend_from_slice(&self.reserved);
    for character in self.display_text {
      push_u16(&mut bytes, character);
    }
    Ok(bytes)
  }
}

impl Cssa {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    require_operand_len(bytes, 6, "CSSA")?;
    let mut input = SliceReader::new(bytes);
    Ok(Self {
      cells: CellRange {
        first: input.u8()?,
        limit: input.u8()?,
      },
      border_sides: input.u8()?,
      width_type: input.u8()?,
      width: input.u16()?,
    })
  }

  fn to_bytes(self) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(6);
    bytes.extend_from_slice(&[
      self.cells.first,
      self.cells.limit,
      self.border_sides,
      self.width_type,
    ]);
    push_u16(&mut bytes, self.width);
    bytes
  }
}

impl Shd80 {
  fn from_raw(value: u16) -> Self {
    Self {
      foreground_color_index: (value & 0x001f) as u8,
      background_color_index: ((value >> 5) & 0x001f) as u8,
      pattern: (value >> 10) as u8,
    }
  }

  fn raw(self) -> u16 {
    u16::from(self.foreground_color_index & 0x1f)
      | (u16::from(self.background_color_index & 0x1f) << 5)
      | (u16::from(self.pattern & 0x3f) << 10)
  }

  fn array_from_bytes(bytes: &[u8]) -> Result<Vec<Self>> {
    if !bytes.len().is_multiple_of(2) {
      return Err(Error::invalid(
        0,
        "DefTableShd80 array has an odd byte count",
      ));
    }
    let mut input = SliceReader::new(bytes);
    let mut values = Vec::with_capacity(bytes.len() / 2);
    while input.offset < bytes.len() {
      values.push(Self::from_raw(input.u16()?));
    }
    Ok(values)
  }

  fn array_to_bytes(values: &[Self]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 2);
    for value in values {
      push_u16(&mut bytes, value.raw());
    }
    bytes
  }
}

impl CellHideMarkOperand {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    require_operand_len(bytes, 3, "CellHideMark")?;
    Ok(Self {
      cells: CellRange {
        first: bytes[0],
        limit: bytes[1],
      },
      hide_when_empty: bytes[2],
    })
  }

  fn to_bytes(self) -> [u8; 3] {
    [self.cells.first, self.cells.limit, self.hide_when_empty]
  }
}

impl TableCellWidthOperand {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    require_operand_len(bytes, 5, "TableCellWidth")?;
    let mut input = SliceReader::new(bytes);
    Ok(Self {
      cells: CellRange {
        first: input.u8()?,
        limit: input.u8()?,
      },
      width_type: input.u8()?,
      width: input.u16()?,
    })
  }

  fn to_bytes(self) -> [u8; 5] {
    let width = self.width.to_le_bytes();
    [
      self.cells.first,
      self.cells.limit,
      self.width_type,
      width[0],
      width[1],
    ]
  }
}

impl Brc80 {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    input.sdk_object()
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    write_sdk_object(bytes, &self)
  }
}

impl Picf {
  pub const ENCODED_LEN: usize = 68;

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() != Self::ENCODED_LEN {
      return Err(Error::invalid(0, "PICF must contain exactly 68 bytes"));
    }
    let mut reader = Reader::new(Cursor::new(bytes))?;
    Self::read_from(&mut reader)
  }

  pub fn to_bytes(self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(Self::ENCODED_LEN)));
    self.write_to(&mut writer)?;
    Ok(writer.into_inner().into_inner())
  }
}

fn validate_picf(value: &Picf) -> Result<()> {
  if value.header_length != Picf::ENCODED_LEN as u16 {
    return Err(Error::invalid(4, "PICF cbHeader must be 0x44"));
  }
  Ok(())
}

impl PicfAndOfficeArtData {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < Picf::ENCODED_LEN {
      return Err(Error::invalid(0, "PICFAndOfficeArtData is truncated"));
    }
    let picf = Picf::from_bytes(&bytes[..Picf::ENCODED_LEN])?;
    if usize::try_from(picf.total_length).ok() != Some(bytes.len()) {
      return Err(Error::invalid(
        0,
        "PICF lcb does not match PICFAndOfficeArtData length",
      ));
    }
    let mut offset = Picf::ENCODED_LEN;
    let shape_file_name = match picf.storage.format {
      PictureStorageFormat::Shape => None,
      PictureStorageFormat::ShapeFile => {
        let length = usize::from(*bytes.get(offset).ok_or_else(|| {
          Error::invalid(offset as u64, "PICF shape-file name length is missing")
        })?);
        offset += 1;
        let end = offset
          .checked_add(length)
          .filter(|end| *end <= bytes.len())
          .ok_or_else(|| Error::invalid(offset as u64, "PICF shape-file name is truncated"))?;
        let name = bytes[offset..end].to_vec();
        offset = end;
        Some(name)
      }
    };
    Ok(Self {
      picf,
      shape_file_name,
      picture: OfficeArtStream::from_bytes(&bytes[offset..])?,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let picture = self.picture.to_bytes()?;
    self.to_bytes_with_parts(self.picf, &picture)
  }

  pub(crate) fn to_bytes_with_computed_length(&self) -> Result<Vec<u8>> {
    let picture = self.picture.to_bytes()?;
    let name_len = self
      .shape_file_name
      .as_ref()
      .map_or(0usize, |name| name.len() + 1);
    let total_len = Picf::ENCODED_LEN
      .checked_add(name_len)
      .and_then(|length| length.checked_add(picture.len()))
      .ok_or_else(|| Error::Limit("PICFAndOfficeArtData length overflow".into()))?;
    let mut picf = self.picf;
    picf.total_length = i32::try_from(total_len)
      .map_err(|_| Error::Limit("PICFAndOfficeArtData length exceeds i32".into()))?;
    self.to_bytes_with_parts(picf, &picture)
  }

  fn to_bytes_with_parts(&self, picf: Picf, picture: &[u8]) -> Result<Vec<u8>> {
    if matches!(self.picf.storage.format, PictureStorageFormat::Shape)
      != self.shape_file_name.is_none()
    {
      return Err(Error::invalid(
        0,
        "PICF shape-file name presence does not match MFPF",
      ));
    }
    let mut bytes = Vec::with_capacity(
      usize::try_from(picf.total_length).map_err(|_| Error::invalid(0, "PICF lcb is negative"))?,
    );
    bytes.extend_from_slice(&picf.to_bytes()?);
    if let Some(name) = &self.shape_file_name {
      bytes.push(
        u8::try_from(name.len())
          .map_err(|_| Error::Limit("PICF shape-file name exceeds u8".into()))?,
      );
      bytes.extend_from_slice(name);
    }
    bytes.extend_from_slice(picture);
    if usize::try_from(picf.total_length).ok() != Some(bytes.len()) {
      return Err(Error::invalid(
        0,
        "PICF lcb does not match encoded PICFAndOfficeArtData length",
      ));
    }
    Ok(bytes)
  }
}

impl NilPicfAndBinData {
  pub const HEADER_LEN: usize = 68;

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < Self::HEADER_LEN {
      return Err(Error::invalid(0, "NilPICFAndBinData is truncated"));
    }
    let mut reader = Reader::new(Cursor::new(bytes))?;
    let value = NilPicfWire::read_from(&mut reader)?;
    Ok(Self {
      total_length: value.total_length,
      header_length: value.header_length,
      ignored_header: value.ignored_header,
      binary_data: NilPicfBinaryData::Unresolved(value.binary_data),
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let binary_data = self.binary_data.to_bytes()?;
    self.to_bytes_with_binary_data(self.total_length, binary_data)
  }

  pub(crate) fn to_bytes_with_computed_length(&self) -> Result<Vec<u8>> {
    let binary_data = self.binary_data.to_bytes()?;
    let total_len = Self::HEADER_LEN
      .checked_add(binary_data.len())
      .ok_or_else(|| Error::Limit("NilPICFAndBinData length overflow".into()))?;
    let total_length = i32::try_from(total_len)
      .map_err(|_| Error::Limit("NilPICFAndBinData length exceeds i32".into()))?;
    self.to_bytes_with_binary_data(total_length, binary_data)
  }

  fn to_bytes_with_binary_data(&self, total_length: i32, binary_data: Vec<u8>) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      Self::HEADER_LEN.saturating_add(binary_data.len()),
    )));
    NilPicfWire {
      total_length,
      header_length: self.header_length,
      ignored_header: self.ignored_header,
      binary_data,
    }
    .write_to(&mut writer)?;
    Ok(writer.into_inner().into_inner())
  }

  pub fn interpret(&mut self, field_type: NilPicfFieldType) {
    let bytes = match &self.binary_data {
      NilPicfBinaryData::Unresolved(bytes) => bytes.clone(),
      _ => return,
    };
    self.binary_data = match field_type {
      NilPicfFieldType::Hyperlink(field_type) => match Hfd::from_bytes(&bytes) {
        Ok(value) => NilPicfBinaryData::Hyperlink { field_type, value },
        Err(_) => NilPicfBinaryData::Invalid {
          field_type: NilPicfFieldType::Hyperlink(field_type),
          bytes,
        },
      },
      NilPicfFieldType::Form(field_type) => match FfData::from_bytes(&bytes) {
        Ok(value) if field_type.matches(value.bits.field_kind) => {
          NilPicfBinaryData::Form { field_type, value }
        }
        Ok(_) | Err(_) => NilPicfBinaryData::Invalid {
          field_type: NilPicfFieldType::Form(field_type),
          bytes,
        },
      },
      NilPicfFieldType::Private(field_type) => NilPicfBinaryData::Private { field_type, bytes },
    };
  }

  pub(crate) fn mark_invalid_context(&mut self) {
    let NilPicfBinaryData::Unresolved(bytes) = &self.binary_data else {
      return;
    };
    self.binary_data = NilPicfBinaryData::InvalidContext(bytes.clone());
  }

  pub fn binary_len(&self) -> Result<usize> {
    Ok(self.binary_data.to_bytes()?.len())
  }
}

impl NilPicfBinaryData {
  fn to_bytes(&self) -> Result<Vec<u8>> {
    match self {
      Self::Unresolved(bytes)
      | Self::InvalidContext(bytes)
      | Self::Private { bytes, .. }
      | Self::Invalid { bytes, .. } => Ok(bytes.clone()),
      Self::Hyperlink { value, .. } => value.to_bytes(),
      Self::Form { field_type, value } if field_type.matches(value.bits.field_kind) => {
        value.to_bytes()
      }
      Self::Form { .. } => Err(Error::invalid(
        0,
        "NilPICF form field type does not match FFDataBits.iType",
      )),
    }
  }
}

impl FormFieldType {
  fn matches(self, kind: FormFieldKind) -> bool {
    matches!(
      (self, kind),
      (Self::Text, FormFieldKind::Text)
        | (Self::CheckBox, FormFieldKind::CheckBox)
        | (Self::DropDown, FormFieldKind::DropDown)
    )
  }
}

impl NilPicfFieldType {
  pub const fn from_field_type(value: u8) -> Option<Self> {
    match value {
      0x03 => Some(Self::Hyperlink(HyperlinkFieldType::Ref)),
      0x25 => Some(Self::Hyperlink(HyperlinkFieldType::PageRef)),
      0x46 => Some(Self::Form(FormFieldType::Text)),
      0x47 => Some(Self::Form(FormFieldType::CheckBox)),
      0x48 => Some(Self::Hyperlink(HyperlinkFieldType::NoteRef)),
      0x4d => Some(Self::Private(PrivateFieldType::Private)),
      0x51 => Some(Self::Private(PrivateFieldType::AddIn)),
      0x53 => Some(Self::Form(FormFieldType::DropDown)),
      0x58 => Some(Self::Hyperlink(HyperlinkFieldType::Hyperlink)),
      _ => None,
    }
  }
}

fn validate_nil_picf_wire(value: &NilPicfWire) -> Result<()> {
  if value.header_length != NilPicfAndBinData::HEADER_LEN as u16 {
    return Err(Error::invalid(4, "NilPICFAndBinData cbHeader must be 0x44"));
  }
  let length = NilPicfAndBinData::HEADER_LEN
    .checked_add(value.binary_data.len())
    .ok_or_else(|| Error::Limit("NilPICFAndBinData length overflow".into()))?;
  if usize::try_from(value.total_length).ok() != Some(length) {
    return Err(Error::invalid(
      0,
      "NilPICFAndBinData lcb does not match encoded length",
    ));
  }
  Ok(())
}

impl FfDataBits {
  pub fn from_u16(value: u16) -> Result<Self> {
    let field_kind = match value & 0x0003 {
      0 => FormFieldKind::Text,
      1 => FormFieldKind::CheckBox,
      2 => FormFieldKind::DropDown,
      _ => return Err(Error::invalid(0, "FFDataBits iType is reserved")),
    };
    let text_kind = match (value >> 11) & 0x0007 {
      0 => TextFormFieldKind::Regular,
      1 => TextFormFieldKind::Number,
      2 => TextFormFieldKind::DateOrTime,
      3 => TextFormFieldKind::CurrentDate,
      4 => TextFormFieldKind::CurrentTime,
      5 => TextFormFieldKind::Calculated,
      _ => return Err(Error::invalid(0, "FFDataBits iTypeTxt is reserved")),
    };
    let result = ((value >> 2) & 0x001f) as u8;
    let automatic_size = value & 0x0400 != 0;
    let has_list_box = value & 0x8000 != 0;
    if field_kind == FormFieldKind::Text && result != 0 {
      return Err(Error::invalid(0, "text FFDataBits iRes must be zero"));
    }
    if field_kind == FormFieldKind::CheckBox && !matches!(result, 0 | 1 | 25) {
      return Err(Error::invalid(0, "checkbox FFDataBits iRes is invalid"));
    }
    if field_kind != FormFieldKind::CheckBox && automatic_size {
      return Err(Error::invalid(
        0,
        "non-checkbox FFDataBits iSize must be zero",
      ));
    }
    if field_kind != FormFieldKind::Text && text_kind != TextFormFieldKind::Regular {
      return Err(Error::invalid(
        0,
        "non-text FFDataBits iTypeTxt must be zero",
      ));
    }
    if has_list_box != (field_kind == FormFieldKind::DropDown) {
      return Err(Error::invalid(
        0,
        "FFDataBits fHasListBox does not match iType",
      ));
    }
    Ok(Self {
      field_kind,
      result,
      own_help: value & 0x0080 != 0,
      own_status: value & 0x0100 != 0,
      protected: value & 0x0200 != 0,
      automatic_size,
      text_kind,
      recalculate: value & 0x4000 != 0,
      has_list_box,
    })
  }

  pub fn to_u16(self) -> Result<u16> {
    let field_kind = match self.field_kind {
      FormFieldKind::Text => 0,
      FormFieldKind::CheckBox => 1,
      FormFieldKind::DropDown => 2,
    };
    let text_kind = match self.text_kind {
      TextFormFieldKind::Regular => 0,
      TextFormFieldKind::Number => 1,
      TextFormFieldKind::DateOrTime => 2,
      TextFormFieldKind::CurrentDate => 3,
      TextFormFieldKind::CurrentTime => 4,
      TextFormFieldKind::Calculated => 5,
    };
    let value = field_kind
      | (u16::from(self.result) << 2)
      | (u16::from(self.own_help) << 7)
      | (u16::from(self.own_status) << 8)
      | (u16::from(self.protected) << 9)
      | (u16::from(self.automatic_size) << 10)
      | (text_kind << 11)
      | (u16::from(self.recalculate) << 14)
      | (u16::from(self.has_list_box) << 15);
    if self.result > 0x1f || Self::from_u16(value)? != self {
      return Err(Error::invalid(0, "FFDataBits fields are inconsistent"));
    }
    Ok(value)
  }
}

impl HsttbDropList {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    if input.u16()? != 0xffff {
      return Err(Error::invalid(
        input.offset.saturating_sub(2) as u64,
        "FFData dropdown list is not an extended STTB",
      ));
    }
    let count = usize::from(input.u16()?);
    if count > 25 {
      return Err(Error::invalid(
        input.offset.saturating_sub(2) as u64,
        "FFData dropdown list exceeds 25 entries",
      ));
    }
    if input.u16()? != 0 {
      return Err(Error::invalid(
        input.offset.saturating_sub(2) as u64,
        "FFData dropdown STTB cbExtra is not zero",
      ));
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
      let length = usize::from(input.u16()?);
      let mut entry = Vec::with_capacity(length);
      for _ in 0..length {
        entry.push(input.u16()?);
      }
      entries.push(entry);
    }
    Ok(Self { entries })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.entries.len() > 25 {
      return Err(Error::invalid(0, "FFData dropdown list exceeds 25 entries"));
    }
    push_u16(bytes, 0xffff);
    push_u16(
      bytes,
      u16::try_from(self.entries.len())
        .map_err(|_| Error::Limit("FFData dropdown count exceeds u16".into()))?,
    );
    push_u16(bytes, 0);
    for entry in &self.entries {
      push_u16(
        bytes,
        u16::try_from(entry.len())
          .map_err(|_| Error::Limit("FFData dropdown entry exceeds u16".into()))?,
      );
      write_u16_array(bytes, entry);
    }
    Ok(())
  }
}

impl FfData {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let version = input.u32()?;
    let bits = FfDataBits::from_u16(input.u16()?)?;
    let maximum_text_length = input.u16()?;
    let check_box_size_half_points = input.u16()?;
    let name = Xstz::read(&mut input)?;
    let (default_text, default_selection) = match bits.field_kind {
      FormFieldKind::Text => (Some(Xstz::read(&mut input)?), None),
      FormFieldKind::CheckBox | FormFieldKind::DropDown => (None, Some(input.u16()?)),
    };
    let text_format = Xstz::read(&mut input)?;
    let help_text = Xstz::read(&mut input)?;
    let status_text = Xstz::read(&mut input)?;
    let entry_macro = Xstz::read(&mut input)?;
    let exit_macro = Xstz::read(&mut input)?;
    let drop_down_list = (bits.field_kind == FormFieldKind::DropDown)
      .then(|| HsttbDropList::read(&mut input))
      .transpose()?;
    if input.offset != bytes.len() {
      return Err(Error::invalid(
        input.offset as u64,
        "trailing bytes after FFData",
      ));
    }
    let value = Self {
      version,
      bits,
      maximum_text_length,
      check_box_size_half_points,
      name,
      default_text,
      default_selection,
      text_format,
      help_text,
      status_text,
      entry_macro,
      exit_macro,
      drop_down_list,
    };
    value.validate()?;
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.validate()?;
    let mut bytes = Vec::new();
    push_u32(&mut bytes, self.version);
    push_u16(&mut bytes, self.bits.to_u16()?);
    push_u16(&mut bytes, self.maximum_text_length);
    push_u16(&mut bytes, self.check_box_size_half_points);
    self.name.write(&mut bytes)?;
    match self.bits.field_kind {
      FormFieldKind::Text => self
        .default_text
        .as_ref()
        .expect("validated")
        .write(&mut bytes)?,
      FormFieldKind::CheckBox | FormFieldKind::DropDown => {
        push_u16(&mut bytes, self.default_selection.expect("validated"))
      }
    }
    self.text_format.write(&mut bytes)?;
    self.help_text.write(&mut bytes)?;
    self.status_text.write(&mut bytes)?;
    self.entry_macro.write(&mut bytes)?;
    self.exit_macro.write(&mut bytes)?;
    if let Some(drop_down_list) = &self.drop_down_list {
      drop_down_list.write(&mut bytes)?;
    }
    Ok(bytes)
  }

  fn validate(&self) -> Result<()> {
    if self.version != 0xffff_ffff {
      return Err(Error::invalid(0, "FFData version must be 0xFFFFFFFF"));
    }
    self.bits.to_u16()?;
    if self.maximum_text_length > 32767
      || (self.bits.field_kind != FormFieldKind::Text && self.maximum_text_length != 0)
    {
      return Err(Error::invalid(
        0,
        "FFData cch is invalid for its field type",
      ));
    }
    if self.bits.field_kind == FormFieldKind::CheckBox
      && !(2..=3168).contains(&self.check_box_size_half_points)
    {
      return Err(Error::invalid(0, "FFData checkbox hps is outside 2..=3168"));
    }
    validate_xstz(&self.name, 20, "FFData name")?;
    validate_xstz(&self.text_format, 64, "FFData text format")?;
    validate_xstz(&self.help_text, 255, "FFData help text")?;
    validate_xstz(&self.status_text, 138, "FFData status text")?;
    validate_xstz(&self.entry_macro, 32, "FFData entry macro")?;
    validate_xstz(&self.exit_macro, 32, "FFData exit macro")?;
    match self.bits.field_kind {
      FormFieldKind::Text => {
        let default = self
          .default_text
          .as_ref()
          .ok_or_else(|| Error::invalid(0, "text FFData is missing xstzTextDef"))?;
        validate_xstz(default, 255, "FFData default text")?;
        if matches!(
          self.bits.text_kind,
          TextFormFieldKind::CurrentDate | TextFormFieldKind::CurrentTime
        ) && !default.characters.is_empty()
        {
          return Err(Error::invalid(
            0,
            "current date/time FFData default text must be empty",
          ));
        }
        if self.default_selection.is_some() || self.drop_down_list.is_some() {
          return Err(Error::invalid(
            0,
            "text FFData has non-text optional fields",
          ));
        }
      }
      FormFieldKind::CheckBox => {
        if !matches!(self.default_selection, Some(0 | 1))
          || self.default_text.is_some()
          || self.drop_down_list.is_some()
        {
          return Err(Error::invalid(
            0,
            "checkbox FFData optional fields are invalid",
          ));
        }
      }
      FormFieldKind::DropDown => {
        let list = self
          .drop_down_list
          .as_ref()
          .ok_or_else(|| Error::invalid(0, "dropdown FFData is missing its string table"))?;
        let selection = usize::from(
          self
            .default_selection
            .ok_or_else(|| Error::invalid(0, "dropdown FFData is missing wDef"))?,
        );
        if selection >= list.entries.len()
          || (self.bits.result != 25 && usize::from(self.bits.result) >= list.entries.len())
          || self.default_text.is_some()
        {
          return Err(Error::invalid(
            0,
            "dropdown FFData selection is out of bounds",
          ));
        }
      }
    }
    if self.bits.field_kind != FormFieldKind::Text && !self.text_format.characters.is_empty() {
      return Err(Error::invalid(
        0,
        "non-text FFData text format must be empty",
      ));
    }
    Ok(())
  }
}

fn validate_xstz(value: &Xstz, maximum: usize, label: &str) -> Result<()> {
  if value.characters.len() > maximum || value.terminator != 0 {
    return Err(Error::invalid(
      0,
      format!("{label} length or terminator is invalid"),
    ));
  }
  Ok(())
}

impl HfdBits {
  pub fn from_u8(value: u8) -> Result<Self> {
    let result = Self {
      open_in_new_window: value & 0x01 != 0,
      do_not_preserve_history: value & 0x02 != 0,
      image_map: value & 0x04 != 0,
      has_location: value & 0x08 != 0,
      has_tooltip: value & 0x10 != 0,
      unused: value >> 5,
    };
    if result.unused != 0 {
      return Err(Error::invalid(0, "HFDBits unused bits must be zero"));
    }
    Ok(result)
  }

  pub fn to_u8(self) -> Result<u8> {
    if self.unused != 0 {
      return Err(Error::invalid(0, "HFDBits unused bits must be zero"));
    }
    Ok(
      u8::from(self.open_in_new_window)
        | (u8::from(self.do_not_preserve_history) << 1)
        | (u8::from(self.image_map) << 2)
        | (u8::from(self.has_location) << 3)
        | (u8::from(self.has_tooltip) << 4),
    )
  }
}

impl Hfd {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 25 {
      return Err(Error::invalid(0, "HFD is shorter than its fixed prefix"));
    }
    let mut input = SliceReader::new(bytes);
    let bits = HfdBits::from_u8(input.u8()?)?;
    let class_id = Guid {
      data1: input.u32()?,
      data2: input.u16()?,
      data3: input.u16()?,
      data4: input.take()?,
    };
    let hyperlink = crate::xls::HyperlinkObject::parse(&bytes[input.offset..])?;
    Ok(Self {
      bits,
      class_id,
      hyperlink,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.push(self.bits.to_u8()?);
    push_u32(&mut bytes, self.class_id.data1);
    push_u16(&mut bytes, self.class_id.data2);
    push_u16(&mut bytes, self.class_id.data3);
    bytes.extend_from_slice(&self.class_id.data4);
    bytes.extend_from_slice(&self.hyperlink.to_bytes()?);
    Ok(bytes)
  }
}

impl PrcData {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let length = input.i16()?;
    if !(0..=0x3fa2).contains(&length) {
      return Err(Error::invalid(0, "PrcData cbGrpprl is outside 0..=0x3FA2"));
    }
    let length = length as usize;
    if bytes.len() != length + 2 {
      return Err(Error::invalid(
        0,
        "PrcData cbGrpprl does not match its length",
      ));
    }
    Ok(Self {
      properties: GrpPrl::from_bytes(input.bytes(length)?)?,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let properties = self.properties.to_bytes()?;
    if properties.len() > 0x3fa2 {
      return Err(Error::Limit("PrcData GrpPrl exceeds 0x3FA2 bytes".into()));
    }
    let mut bytes = Vec::with_capacity(properties.len() + 2);
    bytes.extend_from_slice(&(properties.len() as i16).to_le_bytes());
    bytes.extend_from_slice(&properties);
    Ok(bytes)
  }
}

impl TableBordersOperand {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    require_operand_len(bytes, 48, "TableBorders")?;
    let mut input = SliceReader::new(bytes);
    let mut borders = Vec::with_capacity(6);
    for _ in 0..6 {
      borders.push(Brc::read(&mut input)?);
    }
    Ok(Self {
      borders: borders.try_into().unwrap(),
    })
  }

  fn to_bytes(self) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(48);
    for border in self.borders {
      border.write(&mut bytes);
    }
    bytes
  }
}

impl TableBordersOperand80 {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    require_operand_len(bytes, 24, "TableBorders80")?;
    let mut input = SliceReader::new(bytes);
    let mut borders = Vec::with_capacity(6);
    for _ in 0..6 {
      borders.push(Brc80::read(&mut input)?);
    }
    Ok(Self {
      borders: borders.try_into().unwrap(),
    })
  }

  fn to_bytes(self) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(24);
    for border in self.borders {
      border.write(&mut bytes)?;
    }
    Ok(bytes)
  }
}

impl TableBrcOperand {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    require_operand_len(bytes, 11, "TableBrc")?;
    let mut input = SliceReader::new(bytes);
    Ok(Self {
      cells: CellRange {
        first: input.u8()?,
        limit: input.u8()?,
      },
      borders_to_apply: input.u8()?,
      border: Brc::read(&mut input)?,
    })
  }

  fn to_bytes(self) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(11);
    bytes.extend_from_slice(&[self.cells.first, self.cells.limit, self.borders_to_apply]);
    self.border.write(&mut bytes);
    bytes
  }
}

impl TableBrcOperand80 {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    require_operand_len(bytes, 7, "TableBrc80")?;
    let mut input = SliceReader::new(bytes);
    Ok(Self {
      cells: CellRange {
        first: input.u8()?,
        limit: input.u8()?,
      },
      borders_to_apply: input.u8()?,
      border: Brc80::read(&mut input)?,
    })
  }

  fn to_bytes(self) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(7);
    bytes.extend_from_slice(&[self.cells.first, self.cells.limit, self.borders_to_apply]);
    self.border.write(&mut bytes)?;
    Ok(bytes)
  }
}

impl Tc80 {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let formatting = input.sdk_object()?;
    let preferred_width = input.u16()?;
    let mut borders = Vec::with_capacity(4);
    for _ in 0..4 {
      borders.push(Brc80::read(input)?);
    }
    Ok(Self {
      formatting,
      preferred_width,
      borders: borders.try_into().unwrap(),
    })
  }

  fn write(self, bytes: &mut Vec<u8>) -> Result<()> {
    write_sdk_object(bytes, &self.formatting)?;
    push_u16(bytes, self.preferred_width);
    for border in self.borders {
      border.write(bytes)?;
    }
    Ok(())
  }
}

impl TDefTableOperand {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut input = SliceReader::new(bytes);
    let column_count = usize::from(input.u8()?);
    if column_count > 63 {
      return Err(Error::invalid(0, "TDefTable column count exceeds 63"));
    }
    let mut column_boundaries = Vec::with_capacity(column_count + 1);
    for _ in 0..=column_count {
      column_boundaries.push(input.i16()?);
    }
    let remaining = bytes.len() - input.offset;
    if !remaining.is_multiple_of(20) {
      return Err(Error::invalid(
        input.offset as u64,
        "TDefTable TC80 region is not a multiple of 20 bytes",
      ));
    }
    let mut cells = Vec::with_capacity(remaining / 20);
    while input.offset < bytes.len() {
      cells.push(Tc80::read(&mut input)?);
    }
    Ok(Self {
      column_boundaries,
      cells,
    })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let column_count = self
      .column_boundaries
      .len()
      .checked_sub(1)
      .ok_or_else(|| Error::invalid(0, "TDefTable has no column boundaries"))?;
    if column_count > 63 {
      return Err(Error::invalid(0, "TDefTable column count exceeds 63"));
    }
    let mut bytes =
      Vec::with_capacity(1 + self.column_boundaries.len() * 2 + self.cells.len() * 20);
    bytes.push(column_count as u8);
    for boundary in &self.column_boundaries {
      bytes.extend_from_slice(&boundary.to_le_bytes());
    }
    for cell in &self.cells {
      cell.write(&mut bytes)?;
    }
    Ok(bytes)
  }
}

fn require_operand_len(bytes: &[u8], expected: usize, name: &str) -> Result<()> {
  if bytes.len() != expected {
    return Err(Error::invalid(
      0,
      format!(
        "{name} operand is {} bytes instead of {expected}",
        bytes.len()
      ),
    ));
  }
  Ok(())
}

impl NumRmOperand {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() != 128 {
      return Err(Error::invalid(0, "NumRM operand is not 128 bytes"));
    }
    let mut input = SliceReader::new(bytes);
    let numbered_before_tracking = input.u8()?;
    let ignored_flag = input.u8()?;
    let author_index = input.u16()?;
    let timestamp = input.u32()?;
    let placeholder_indices = input.take()?;
    let number_formats = input.take()?;
    let ignored = input.u16()?;
    let mut number_values = [0; 9];
    for value in &mut number_values {
      *value = input.u32()?;
    }
    let mut format_string = [0; 32];
    for value in &mut format_string {
      *value = input.u16()?;
    }
    Ok(Self {
      numbered_before_tracking,
      ignored_flag,
      author_index,
      timestamp,
      placeholder_indices,
      number_formats,
      ignored,
      number_values,
      format_string,
    })
  }

  fn to_bytes(&self) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(128);
    bytes.push(self.numbered_before_tracking);
    bytes.push(self.ignored_flag);
    push_u16(&mut bytes, self.author_index);
    push_u32(&mut bytes, self.timestamp);
    bytes.extend_from_slice(&self.placeholder_indices);
    bytes.extend_from_slice(&self.number_formats);
    push_u16(&mut bytes, self.ignored);
    for value in self.number_values {
      push_u32(&mut bytes, value);
    }
    for value in self.format_string {
      push_u16(&mut bytes, value);
    }
    bytes
  }
}

impl DispFldRmOperand {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() != 39 {
      return Err(Error::invalid(0, "DispFldRm operand is not 39 bytes"));
    }
    let mut input = SliceReader::new(bytes);
    let has_revision = input.u8()?;
    let author_index = input.u16()?;
    let timestamp = input.u32()?;
    let mut previous_result = [0; 16];
    for value in &mut previous_result {
      *value = input.u16()?;
    }
    Ok(Self {
      has_revision,
      author_index,
      timestamp,
      previous_result,
    })
  }

  fn to_bytes(&self) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(39);
    bytes.push(self.has_revision);
    push_u16(&mut bytes, self.author_index);
    push_u32(&mut bytes, self.timestamp);
    for value in self.previous_result {
      push_u16(&mut bytes, value);
    }
    bytes
  }
}

impl PlcPcd {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(12) {
      return Err(Error::invalid(
        0,
        "PlcPcd size does not contain whole Pcd elements",
      ));
    }
    let piece_count = (bytes.len() - 4) / 12;
    let mut input = SliceReader::new(bytes);
    let mut character_positions = Vec::with_capacity(piece_count + 1);
    for _ in 0..=piece_count {
      character_positions.push(input.i32()?);
    }
    let mut pieces = Vec::with_capacity(piece_count);
    for _ in 0..piece_count {
      pieces.push(Pcd::read(&mut input)?);
    }
    Ok(Self {
      character_positions,
      pieces,
    })
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.character_positions.len() != self.pieces.len().saturating_add(1) {
      return Err(Error::invalid(0, "PlcPcd CP/Pcd cardinality changed"));
    }
    let mut bytes = Vec::with_capacity(self.character_positions.len() * 4 + self.pieces.len() * 8);
    for position in &self.character_positions {
      push_i32(&mut bytes, *position);
    }
    for piece in &self.pieces {
      piece.write(&mut bytes)?;
    }
    Ok(bytes)
  }
}

impl Pcd {
  pub fn text_piece(&self, word_document: &[u8], cp_start: i32, cp_end: i32) -> Result<TextPiece> {
    let character_count = cp_end
      .checked_sub(cp_start)
      .filter(|count| *count > 0)
      .ok_or_else(|| Error::invalid(0, "Pcd character range is not strictly increasing"))?;
    let character_count = usize::try_from(character_count)
      .map_err(|_| Error::Limit("Pcd character count exceeds usize".into()))?;
    let file_offset = self.file_position.byte_offset();
    let start = usize::try_from(file_offset)
      .map_err(|_| Error::Limit("Pcd file offset exceeds usize".into()))?;
    let byte_count = if self.file_position.compressed {
      character_count
    } else {
      character_count
        .checked_mul(2)
        .ok_or_else(|| Error::Limit("UTF-16 Pcd byte count overflow".into()))?
    };
    let end = start
      .checked_add(byte_count)
      .ok_or_else(|| Error::Limit("Pcd text bounds overflow".into()))?;
    let bytes = word_document
      .get(start..end)
      .ok_or_else(|| Error::invalid(u64::from(file_offset), "Pcd text exceeds WordDocument"))?;
    let characters = if self.file_position.compressed {
      TextPieceCharacters::from_compressed_bytes(bytes)
    } else {
      TextPieceCharacters::from_utf16_units(
        bytes
          .chunks_exact(2)
          .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
          .collect(),
      )
    };
    Ok(TextPiece {
      cp_start,
      cp_end,
      file_offset,
      characters,
    })
  }

  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let flags = input.u16()?;
    let file_position = input.sdk_object()?;
    let raw_prm = input.u16()?;
    Ok(Self {
      no_paragraph_mark_at_end: flags & 0x0001 != 0,
      reserved1: flags & 0x0002 != 0,
      dirty: flags & 0x0004 != 0,
      reserved2: flags >> 3,
      file_position,
      property_modifier: if raw_prm & 1 == 0 {
        Prm::Simple {
          isprm: ((raw_prm >> 1) & 0x7f) as u8,
          value: (raw_prm >> 8) as u8,
        }
      } else {
        Prm::Complex {
          property_run_index: raw_prm >> 1,
        }
      },
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.reserved2 > 0x1fff {
      return Err(Error::invalid(0, "Pcd reserved field exceeds 13 bits"));
    }
    let flags = u16::from(self.no_paragraph_mark_at_end)
      | (u16::from(self.reserved1) << 1)
      | (u16::from(self.dirty) << 2)
      | (self.reserved2 << 3);
    push_u16(bytes, flags);
    write_sdk_object(bytes, &self.file_position)?;
    let raw_prm = match self.property_modifier {
      Prm::Simple { isprm, value } if isprm <= 0x7f => {
        (u16::from(isprm) << 1) | (u16::from(value) << 8)
      }
      Prm::Simple { .. } => {
        return Err(Error::invalid(0, "Prm0 isprm exceeds seven bits"));
      }
      Prm::Complex { property_run_index } if property_run_index <= 0x7fff => {
        (property_run_index << 1) | 1
      }
      Prm::Complex { .. } => {
        return Err(Error::invalid(0, "Prm1 property-run index exceeds 15 bits"));
      }
    };
    push_u16(bytes, raw_prm);
    Ok(())
  }
}

impl TextPiece {
  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.characters.to_bytes()
  }

  pub fn character_count(&self) -> usize {
    self.characters.character_count()
  }
}

impl FibBase {
  pub fn from_word_document(bytes: &[u8]) -> Result<Self> {
    Self::read(&mut SliceReader::new(bytes))
  }

  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let file_identifier = input.u16()?;
    let n_fib = input.u16()?;
    let unused = input.u16()?;
    let language_id = input.u16()?;
    let next_fib_page = input.u16()?;
    let raw_flags = input.u16()?;
    let flags = FibBaseFlags::from_bits_retain(raw_flags & !0x00f0);
    let quick_save_count = ((raw_flags >> 4) & 0x0f) as u8;
    Ok(Self {
      file_identifier,
      n_fib,
      unused,
      language_id,
      next_fib_page,
      flags,
      quick_save_count,
      n_fib_back: input.u16()?,
      encryption_key_or_header_size: input.u32()?,
      environment: input.u8()?,
      environment_flags: FibBaseEnvironmentFlags::from_bits_retain(input.u8()?),
      reserved3: input.u16()?,
      reserved4: input.u16()?,
      reserved5: input.u32()?,
      reserved6: input.u32()?,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) {
    push_u16(bytes, self.file_identifier);
    push_u16(bytes, self.n_fib);
    push_u16(bytes, self.unused);
    push_u16(bytes, self.language_id);
    push_u16(bytes, self.next_fib_page);
    push_u16(
      bytes,
      self.flags.bits() | (u16::from(self.quick_save_count & 0x0f) << 4),
    );
    push_u16(bytes, self.n_fib_back);
    push_u32(bytes, self.encryption_key_or_header_size);
    bytes.push(self.environment);
    bytes.push(self.environment_flags.bits());
    push_u16(bytes, self.reserved3);
    push_u16(bytes, self.reserved4);
    push_u32(bytes, self.reserved5);
    push_u32(bytes, self.reserved6);
  }
}

impl FibRgW97 {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let mut reserved = [0; 13];
    for value in &mut reserved {
      *value = input.u16()?;
    }
    Ok(Self {
      reserved,
      far_east_language_id: input.u16()?,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) {
    for value in self.reserved {
      push_u16(bytes, value);
    }
    push_u16(bytes, self.far_east_language_id);
  }
}

impl FibRgLw97 {
  fn read(input: &mut SliceReader<'_>) -> Result<Self> {
    let cb_mac = input.u32()?;
    let reserved1 = input.u32()?;
    let reserved2 = input.u32()?;
    let ccp_text = input.i32()?;
    let ccp_footnote = input.i32()?;
    let ccp_header = input.i32()?;
    let reserved3 = input.u32()?;
    let ccp_comment = input.i32()?;
    let ccp_endnote = input.i32()?;
    let ccp_textbox = input.i32()?;
    let ccp_header_textbox = input.i32()?;
    let mut reserved = [0; 11];
    for value in &mut reserved {
      *value = input.u32()?;
    }
    Ok(Self {
      cb_mac,
      reserved1,
      reserved2,
      ccp_text,
      ccp_footnote,
      ccp_header,
      reserved3,
      ccp_comment,
      ccp_endnote,
      ccp_textbox,
      ccp_header_textbox,
      reserved,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) {
    push_u32(bytes, self.cb_mac);
    push_u32(bytes, self.reserved1);
    push_u32(bytes, self.reserved2);
    push_i32(bytes, self.ccp_text);
    push_i32(bytes, self.ccp_footnote);
    push_i32(bytes, self.ccp_header);
    push_u32(bytes, self.reserved3);
    push_i32(bytes, self.ccp_comment);
    push_i32(bytes, self.ccp_endnote);
    push_i32(bytes, self.ccp_textbox);
    push_i32(bytes, self.ccp_header_textbox);
    for value in self.reserved {
      push_u32(bytes, value);
    }
  }
}

struct SliceReader<'a> {
  bytes: &'a [u8],
  offset: usize,
}

impl<'a> SliceReader<'a> {
  fn new(bytes: &'a [u8]) -> Self {
    Self { bytes, offset: 0 }
  }

  fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
    let end = self
      .offset
      .checked_add(N)
      .ok_or_else(|| Error::invalid(self.offset as u64, "FIB offset overflow"))?;
    let value = self
      .bytes
      .get(self.offset..end)
      .ok_or_else(|| Error::invalid(self.offset as u64, "truncated FIB"))?;
    self.offset = end;
    Ok(
      value
        .try_into()
        .expect("bounded slice has the requested size"),
    )
  }

  fn u8(&mut self) -> Result<u8> {
    Ok(self.take::<1>()?[0])
  }

  fn u16(&mut self) -> Result<u16> {
    Ok(u16::from_le_bytes(self.take()?))
  }

  fn i16(&mut self) -> Result<i16> {
    Ok(i16::from_le_bytes(self.take()?))
  }

  fn u32(&mut self) -> Result<u32> {
    Ok(u32::from_le_bytes(self.take()?))
  }

  fn i32(&mut self) -> Result<i32> {
    Ok(i32::from_le_bytes(self.take()?))
  }

  fn bytes(&mut self, length: usize) -> Result<&'a [u8]> {
    let end = self
      .offset
      .checked_add(length)
      .ok_or_else(|| Error::invalid(self.offset as u64, "FIB offset overflow"))?;
    let value = self
      .bytes
      .get(self.offset..end)
      .ok_or_else(|| Error::invalid(self.offset as u64, "truncated bounded structure"))?;
    self.offset = end;
    Ok(value)
  }

  fn sdk_object<T: SdkRead>(&mut self) -> Result<T> {
    let mut reader = Reader::new(Cursor::new(&self.bytes[self.offset..]))?;
    let value = T::read_from(&mut reader)?;
    let consumed = usize::try_from(reader.position()?)
      .map_err(|_| Error::Limit("SDK object length does not fit usize".into()))?;
    self.bytes(consumed)?;
    Ok(value)
  }
}

fn write_sdk_object<T: SdkWrite>(bytes: &mut Vec<u8>, value: &T) -> Result<()> {
  let mut writer = Writer::new(Cursor::new(Vec::new()));
  value.write_to(&mut writer)?;
  bytes.extend_from_slice(&writer.into_inner().into_inner());
  Ok(())
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
  bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
  bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_i32(bytes: &mut Vec<u8>, value: i32) {
  bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_variable8(bytes: &mut Vec<u8>, value: &[u8]) -> Result<()> {
  bytes.push(
    u8::try_from(value.len())
      .map_err(|_| Error::Limit("variable SPRM operand exceeds u8".into()))?,
  );
  bytes.extend_from_slice(value);
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn fixture(version: FibVersion) -> Fib {
    let csw_new = match version {
      FibVersion::Word97 => FibRgCswNew::None,
      FibVersion::Word2000 | FibVersion::Word2002 | FibVersion::Word2003 => FibRgCswNew::Word2000 {
        n_fib_new: version.n_fib(),
        quick_save_count: 3,
      },
      FibVersion::Word2007 => FibRgCswNew::Word2007 {
        n_fib_new: version.n_fib(),
        quick_save_count: 3,
        theme_language_other: 0x0409,
        theme_language_far_east: 0x0411,
        theme_language_complex_script: 0x0401,
      },
      FibVersion::Compatibility(_) => unreachable!("fixtures use documented versions"),
    };
    Fib {
      base: FibBase {
        file_identifier: WORD97_FILE_IDENTIFIER,
        n_fib: if version == FibVersion::Word97 {
          version.n_fib()
        } else {
          FibVersion::Word97.n_fib()
        },
        unused: 0,
        language_id: 0x0409,
        next_fib_page: 0,
        flags: FibBaseFlags::COMPLEX
          | FibBaseFlags::USE_1_TABLE
          | FibBaseFlags::EXTENDED_CHARACTERS,
        quick_save_count: 0x0f,
        n_fib_back: 0x00bf,
        encryption_key_or_header_size: 0,
        environment: 0,
        environment_flags: FibBaseEnvironmentFlags::empty(),
        reserved3: 0,
        reserved4: 0,
        reserved5: 0,
        reserved6: 0,
      },
      rg_w: FibRgW97 {
        reserved: [0; 13],
        far_east_language_id: 0x0409,
      },
      rg_lw: FibRgLw97 {
        cb_mac: 4096,
        reserved1: 0,
        reserved2: 0,
        ccp_text: 42,
        ccp_footnote: 0,
        ccp_header: 0,
        reserved3: 0,
        ccp_comment: 0,
        ccp_endnote: 0,
        ccp_textbox: 0,
        ccp_header_textbox: 0,
        reserved: [0; 11],
      },
      fc_lcb: vec![FibFcLcb { fc: 0, lcb: 0 }; version.documented_fc_lcb_count().unwrap()],
      csw_new,
    }
  }

  #[test]
  fn all_documented_fib_versions_round_trip() {
    for version in [
      FibVersion::Word97,
      FibVersion::Word2000,
      FibVersion::Word2002,
      FibVersion::Word2003,
      FibVersion::Word2007,
    ] {
      let expected = fixture(version);
      let bytes = expected.to_bytes().unwrap();
      let parsed = Fib::from_word_document(&bytes).unwrap();
      assert_eq!(parsed, expected);
      assert_eq!(parsed.to_bytes().unwrap(), bytes);
      assert_eq!(parsed.encoded_len(), bytes.len());
    }
  }

  #[test]
  fn fib_last_saved_time_uses_filetime_pair_instead_of_fc_lcb_semantics() {
    let mut fib = fixture(FibVersion::Word97);
    fib.fc_lcb[FIB_LAST_SAVED_FILETIME_INDEX] = FibFcLcb {
      fc: 0x89ab_cdef,
      lcb: 0x0123_4567,
    };
    let expected = FileTime::from_parts(0x89ab_cdef, 0x0123_4567);
    assert_eq!(fib.last_saved_file_time(), Some(expected));

    let reparsed = Fib::from_word_document(&fib.to_bytes().unwrap()).unwrap();
    assert_eq!(reparsed.last_saved_file_time(), Some(expected));
  }

  #[test]
  fn invalid_counts_are_rejected_without_unbounded_reads() {
    let mut bytes = fixture(FibVersion::Word97).to_bytes().unwrap();
    bytes[32..34].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(Fib::from_word_document(&bytes).is_err());
  }

  #[test]
  fn clx_piece_table_round_trips_static_fields() {
    let value = Clx {
      property_runs: vec![Prc {
        properties: GrpPrl {
          properties: vec![Prl {
            sprm: Sprm::from_opcode(0x0835),
            operand: SprmOperand::Toggle(1),
          }],
        },
      }],
      piece_table: PlcPcd {
        character_positions: vec![0, 5],
        pieces: vec![Pcd {
          no_paragraph_mark_at_end: false,
          reserved1: true,
          dirty: false,
          reserved2: 3,
          file_position: FcCompressed {
            fc: 2048,
            compressed: true,
            reserved: false,
          },
          property_modifier: Prm::Complex {
            property_run_index: 0,
          },
        }],
      },
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(Clx::from_bytes(&bytes).unwrap(), value);
    assert_eq!(Clx::from_bytes(&bytes).unwrap().to_bytes().unwrap(), bytes);
  }

  #[test]
  fn prm_resolves_the_normative_simple_table_and_complex_prc_reference() {
    let clx = Clx {
      property_runs: vec![Prc {
        properties: GrpPrl {
          properties: vec![Prl {
            sprm: Sprm::from_opcode(KnownSprm::CFItalic.opcode()),
            operand: SprmOperand::Toggle(1),
          }],
        },
      }],
      piece_table: PlcPcd {
        character_positions: vec![0],
        pieces: Vec::new(),
      },
    };

    let no_effect = Prm::Simple { isprm: 0, value: 0 }
      .property_modifications(&clx)
      .unwrap();
    assert!(no_effect.properties.is_empty());
    assert_eq!(
      Prm::Simple { isprm: 0, value: 0 }
        .property_modifications_ref(&clx)
        .unwrap(),
      PrmPropertiesRef::Empty
    );

    let simple = Prm::Simple {
      isprm: 0x75,
      value: 1,
    }
    .property_modifications(&clx)
    .unwrap();
    assert_eq!(simple.properties.len(), 1);
    assert_eq!(
      simple.properties[0].sprm.kind(),
      SprmKind::Known(KnownSprm::CFSpec)
    );
    assert_eq!(simple.properties[0].operand, SprmOperand::Toggle(1));
    assert_eq!(
      Prm::Simple {
        isprm: 0x75,
        value: 1,
      }
      .property_modifications_ref(&clx)
      .unwrap(),
      PrmPropertiesRef::Simple {
        sprm: KnownSprm::CFSpec,
        value: 1,
      }
    );

    let line_break = Prm::Simple { isprm: 0, value: 2 }
      .property_modifications(&clx)
      .unwrap();
    assert_eq!(
      line_break.properties[0].sprm.kind(),
      SprmKind::Known(KnownSprm::CLbcCRJ)
    );
    assert_eq!(line_break.properties[0].operand, SprmOperand::Byte(2));

    let complex = Prm::Complex {
      property_run_index: 0,
    }
    .property_modifications(&clx)
    .unwrap();
    assert_eq!(complex, clx.property_runs[0].properties);
    assert_eq!(
      Prm::Complex {
        property_run_index: 0,
      }
      .property_modifications_ref(&clx)
      .unwrap(),
      PrmPropertiesRef::Complex(&clx.property_runs[0].properties)
    );
    assert!(
      Prm::Simple {
        isprm: 0x01,
        value: 1,
      }
      .property_modifications(&clx)
      .is_err()
    );
    assert!(
      Prm::Complex {
        property_run_index: 1,
      }
      .property_modifications(&clx)
      .is_err()
    );
  }

  #[test]
  fn section_table_and_sepx_round_trip_static_fields() {
    let table = PlcfSed {
      character_positions: vec![0, 42],
      sections: vec![Sed {
        file_number: 0,
        sepx_offset: 16,
        mpr_file_number: 0,
        mpr_offset: -1,
      }],
    };
    let table_bytes = table.to_bytes().unwrap();
    assert_eq!(PlcfSed::from_bytes(&table_bytes).unwrap(), table);

    let sepx = Sepx {
      properties: GrpPrl {
        properties: vec![Prl {
          sprm: Sprm::from_opcode(0x3009),
          operand: SprmOperand::Byte(2),
        }],
      },
      trailing_byte: Some(0),
    };
    let sepx_bytes = sepx.to_bytes().unwrap();
    let mut word_document = vec![0; 16];
    word_document.extend_from_slice(&sepx_bytes);
    assert_eq!(
      Sepx::from_word_document(&word_document, 16).unwrap(),
      Some(sepx)
    );
  }

  #[test]
  fn style_sheet_framing_round_trips_bounded_records() {
    let style_sheet = StyleSheet {
      info: StyleSheetInfo {
        header: Stshif {
          style_count: 2,
          std_base_size: 18,
          style_names_written: true,
          reserved: 0,
          max_builtin_style: 1,
          fixed_style_count: 15,
          builtin_name_version: 0,
          ascii_font_index: 0,
          east_asian_font_index: 1,
          other_font_index: 2,
        },
        bidi_font_index: Some(3),
        latent_styles: Some(StshiLsd {
          entry_size: 4,
          entries: vec![LatentStyleData {
            locked: false,
            semi_hidden: true,
            unhide_when_used: false,
            quick_format: true,
            priority: 7,
            reserved: 0,
          }],
        }),
        standard_character_properties: Some(GrpPrl { properties: vec![] }),
        standard_paragraph_properties: None,
      },
      styles: vec![
        LengthPrefixedStyle {
          definition: Some(StyleDefinition {
            base: StdfBase {
              invariant_style_id: 0,
              flags: StdfBaseFlags::empty(),
              style_kind: StyleKind::Paragraph,
              base_style_index: 0x0fff,
              formatting_count: 2,
              next_style_index: 0,
              byte_count: 28,
              general_flags: StyleGeneralFlags::empty(),
            },
            post_2000: Some(StdfPost2000 {
              linked_style_index: 0,
              has_original_style: false,
              spare: 0,
              revision_save_id: 0,
              html_font_index: 0,
              unused: false,
              priority: 0,
            }),
            name: Xstz {
              characters: vec![],
              terminator: 0,
            },
            formatting: StyleFormatting::Paragraph {
              paragraph: StylePapx {
                style_index: 0,
                properties: GrpPrl { properties: vec![] },
                padding: None,
              },
              character: StyleGrpPrl {
                properties: GrpPrl { properties: vec![] },
                padding: None,
              },
            },
          }),
          alignment_padding: None,
        },
        LengthPrefixedStyle {
          definition: None,
          alignment_padding: None,
        },
      ],
    };
    let bytes = style_sheet.to_bytes().unwrap();
    assert_eq!(StyleSheet::from_bytes(&bytes).unwrap(), style_sheet);
  }

  #[test]
  fn revision_marked_styles_round_trip_original_and_current_formatting() {
    let post_2000 = StdfPost2000 {
      linked_style_index: 0,
      has_original_style: true,
      spare: 0,
      revision_save_id: 7,
      html_font_index: 0,
      unused: false,
      priority: 1,
    };
    let revision = StyleRevision {
      modified: Dttm {
        minute: 30,
        hour: 14,
        day: 12,
        month: 6,
        year_offset: 126,
        weekday: 5,
      },
      author_index: 2,
    };
    let empty_character = || StyleGrpPrl {
      properties: GrpPrl { properties: vec![] },
      padding: None,
    };
    let paragraph = StyleDefinition {
      base: StdfBase {
        invariant_style_id: 0x0ffe,
        flags: StdfBaseFlags::empty(),
        style_kind: StyleKind::Paragraph,
        base_style_index: 0x0fff,
        formatting_count: 3,
        next_style_index: 0,
        byte_count: 44,
        general_flags: StyleGeneralFlags::empty(),
      },
      post_2000: Some(post_2000),
      name: Xstz {
        characters: vec![],
        terminator: 0,
      },
      formatting: StyleFormatting::RevisionParagraph {
        paragraph: StylePapx {
          style_index: 0,
          properties: GrpPrl { properties: vec![] },
          padding: None,
        },
        character: empty_character(),
        revision,
        original_paragraph: StylePapx {
          style_index: 0,
          properties: GrpPrl { properties: vec![] },
          padding: None,
        },
        original_character: empty_character(),
      },
    };
    let character = StyleDefinition {
      base: StdfBase {
        invariant_style_id: 0x0ffe,
        flags: StdfBaseFlags::empty(),
        style_kind: StyleKind::Character,
        base_style_index: 0x0fff,
        formatting_count: 2,
        next_style_index: 0,
        byte_count: 36,
        general_flags: StyleGeneralFlags::empty(),
      },
      post_2000: Some(post_2000),
      name: Xstz {
        characters: vec![],
        terminator: 0,
      },
      formatting: StyleFormatting::RevisionCharacter {
        character: empty_character(),
        revision,
        original_character: empty_character(),
      },
    };
    for value in [paragraph, character] {
      let bytes = value.to_bytes().unwrap();
      assert_eq!(bytes.len(), usize::from(value.base.byte_count));
      assert_eq!(StyleDefinition::from_bytes(&bytes, 18).unwrap(), value);
    }
  }

  #[test]
  fn field_and_bookmark_plcs_round_trip_static_records() {
    let fields = FieldTable {
      fields: vec![Field {
        begin: FieldBegin {
          position: 3,
          reserved: 0,
          field_type: 13,
        },
        instruction_fields: Vec::new(),
        separator: Some(FieldSeparator {
          position: 8,
          reserved: 1,
          value: 0,
        }),
        result_fields: Vec::new(),
        end: FieldEnd {
          position: 12,
          reserved: 0,
          flags: FieldEndFlags::HAS_SEPARATOR | FieldEndFlags::RESULTS_DIRTY,
        },
      }],
      terminal_position: 13,
    };
    let field_bytes = fields.to_bytes().unwrap();
    assert_eq!(FieldTable::from_bytes(&field_bytes).unwrap(), fields);
    assert_eq!(fields.innermost_at(9).unwrap().begin.field_type, 13);

    let nested = FieldTable {
      fields: vec![Field {
        begin: FieldBegin {
          position: 1,
          reserved: 0,
          field_type: 0x0d,
        },
        instruction_fields: vec![Field {
          begin: FieldBegin {
            position: 3,
            reserved: 0,
            field_type: 0x25,
          },
          instruction_fields: Vec::new(),
          separator: None,
          result_fields: Vec::new(),
          end: FieldEnd {
            position: 5,
            reserved: 0,
            flags: FieldEndFlags::empty(),
          },
        }],
        separator: Some(FieldSeparator {
          position: 7,
          reserved: 0,
          value: 0,
        }),
        result_fields: vec![Field {
          begin: FieldBegin {
            position: 9,
            reserved: 0,
            field_type: 0x58,
          },
          instruction_fields: Vec::new(),
          separator: None,
          result_fields: Vec::new(),
          end: FieldEnd {
            position: 11,
            reserved: 0,
            flags: FieldEndFlags::empty(),
          },
        }],
        end: FieldEnd {
          position: 13,
          reserved: 0,
          flags: FieldEndFlags::HAS_SEPARATOR,
        },
      }],
      terminal_position: 14,
    };
    let nested_bytes = nested.to_bytes().unwrap();
    assert_eq!(FieldTable::from_bytes(&nested_bytes).unwrap(), nested);
    assert_eq!(nested.innermost_at(4).unwrap().begin.field_type, 0x25);
    assert_eq!(nested.innermost_at(10).unwrap().begin.field_type, 0x58);

    let mut mismatched = nested;
    mismatched.fields[0]
      .end
      .flags
      .remove(FieldEndFlags::HAS_SEPARATOR);
    let mismatched_bytes = mismatched.to_bytes().unwrap();
    assert!(FieldTable::from_bytes(&mismatched_bytes).is_err());
    let compatible = FieldTable::from_bytes_with_compatibility(&mismatched_bytes, true).unwrap();
    assert_eq!(
      compatible.separator_flag_mismatches().collect::<Vec<_>>(),
      vec![13]
    );

    let bookmarks = Bookmarks {
      names: BookmarkNames {
        extended_marker: 0xffff,
        extra_data_size: 0,
        names: vec![vec![b'a' as u16], vec![b'b' as u16]],
      },
      starts: BookmarkStartTable {
        positions: vec![1, 4, 20],
        bookmarks: vec![
          BookmarkStart {
            end_index: 1,
            column_start: 0,
            published: false,
            column_limit: 0,
            native: true,
            column: false,
          },
          BookmarkStart {
            end_index: 0,
            column_start: 2,
            published: false,
            column_limit: 3,
            native: true,
            column: true,
          },
        ],
      },
      ends: BookmarkEndTable {
        positions: vec![7, 9, 21],
      },
    };
    let (names, starts, ends) = bookmarks.to_bytes().unwrap();
    assert_eq!(
      Bookmarks::from_bytes(&names, &starts, &ends).unwrap(),
      bookmarks
    );
  }

  #[test]
  fn note_header_and_annotation_plcs_round_trip_static_records() {
    let header = HeaderTextTable {
      boundaries: vec![
        HeaderStoryBoundary::Position(0),
        HeaderStoryBoundary::Missing,
        HeaderStoryBoundary::Position(9),
      ],
    };
    assert_eq!(
      HeaderTextTable::from_bytes(&header.to_bytes().unwrap()).unwrap(),
      header
    );
    let text = CpOnlyTable {
      positions: vec![0, 4, 9],
    };
    assert_eq!(
      CpOnlyTable::from_bytes(&text.to_bytes().unwrap()).unwrap(),
      text
    );
    let notes = NoteReferenceTable {
      positions: vec![5, 10, 20],
      indices: vec![1, 0],
    };
    assert_eq!(
      NoteReferenceTable::from_bytes(&notes.to_bytes().unwrap()).unwrap(),
      notes
    );
    let annotations = AnnotationReferenceTable {
      positions: vec![7, 8],
      annotations: vec![AnnotationReference {
        initials_length: 2,
        initials_buffer: [b'A' as u16, b'B' as u16, 0, 0, 0, 0, 0, 0, 0],
        author_index: 3,
        bits_not_used: 0,
        flags_not_used: 0,
        bookmark_tag: -1,
      }],
    };
    assert_eq!(
      AnnotationReferenceTable::from_bytes(&annotations.to_bytes().unwrap()).unwrap(),
      annotations
    );
    let extended = AnnotationExtendedData {
      comments: vec![
        AnnotationPost10 {
          modified: Dttm {
            minute: 30,
            hour: 14,
            day: 12,
            month: 7,
            year_offset: 126,
            weekday: 0,
          },
          padding1: 0,
          depth: 0,
          parent_offset: 0,
          ows_discussion_item: false,
          ink: false,
          padding2: 0,
        },
        AnnotationPost10 {
          modified: Dttm {
            minute: 31,
            hour: 14,
            day: 12,
            month: 7,
            year_offset: 126,
            weekday: 0,
          },
          padding1: 0,
          depth: 1,
          parent_offset: -1,
          ows_discussion_item: false,
          ink: true,
          padding2: 0,
        },
      ],
    };
    assert_eq!(
      AnnotationExtendedData::from_bytes(&extended.to_bytes().unwrap()).unwrap(),
      extended
    );
    let mut invalid_tree = extended;
    invalid_tree.comments[1].parent_offset = 1;
    assert!(invalid_tree.to_bytes().is_err());

    let owners = AnnotationOwners {
      names: vec![vec![b'A' as u16, b'd' as u16, b'a' as u16]],
    };
    assert_eq!(
      AnnotationOwners::from_bytes(&owners.to_bytes().unwrap()).unwrap(),
      owners
    );
    let annotation_bookmarks = AnnotationBookmarks {
      infos: AnnotationBookmarkInfos {
        present: true,
        extended_marker: 0xffff,
        extra_data_size: 10,
        entries: vec![AnnotationBookmarkInfo {
          bookmark_class: 0x0100,
          tag: 42,
          old_tag: -1,
        }],
      },
      starts: BookmarkStartTable {
        positions: vec![7, 20],
        bookmarks: vec![BookmarkStart {
          end_index: 0,
          column_start: 0,
          published: false,
          column_limit: 0,
          native: true,
          column: false,
        }],
      },
      ends: BookmarkEndTable {
        positions: vec![12, 20],
      },
    };
    let (infos, starts, ends) = annotation_bookmarks.to_bytes().unwrap();
    assert_eq!(
      AnnotationBookmarks::from_bytes(&infos, &starts, &ends).unwrap(),
      annotation_bookmarks
    );
    let empty_annotation_bookmarks =
      AnnotationBookmarks::from_bytes(&[], &0u32.to_le_bytes(), &0u32.to_le_bytes()).unwrap();
    assert!(!empty_annotation_bookmarks.infos.present);
    assert_eq!(
      empty_annotation_bookmarks.to_bytes().unwrap(),
      (Vec::new(), vec![0; 4], vec![0; 4])
    );

    let textbox_stories = TextboxStoryTable {
      positions: vec![0, 5, 6],
      stories: vec![
        TextboxStory {
          chain: TextboxStoryChain::NonReusable {
            textbox_count: 1,
            edited_textbox_count: 0,
          },
          reusable_flags: 0,
          destination_index: 0,
          shape_id: 1025,
          undo_transaction_id: 0,
        },
        TextboxStory {
          chain: TextboxStoryChain::Reusable {
            next_reusable_index: -1,
            reusable_count: 0,
          },
          reusable_flags: 1,
          destination_index: 0,
          shape_id: 0,
          undo_transaction_id: 0,
        },
      ],
    };
    assert_eq!(
      TextboxStoryTable::from_bytes(&textbox_stories.to_bytes().unwrap()).unwrap(),
      textbox_stories
    );
    let textbox_breaks = TextboxBreakTable {
      positions: vec![0, 5, 6],
      breaks: vec![
        TextboxBreak {
          story_index: 0,
          dependent_character_count: 0,
          reserved1: 0,
          mark_delete: false,
          unused: false,
          text_overflow: true,
          reserved2: 0,
        },
        TextboxBreak {
          story_index: -1,
          dependent_character_count: 0,
          reserved1: 0,
          mark_delete: false,
          unused: false,
          text_overflow: false,
          reserved2: 0,
        },
      ],
    };
    assert_eq!(
      TextboxBreakTable::from_bytes(&textbox_breaks.to_bytes().unwrap()).unwrap(),
      textbox_breaks
    );
    let shape_anchors = ShapeAnchorTable {
      positions: vec![4, 9],
      anchors: vec![ShapeAnchor {
        shape_id: 1025,
        rectangle: ShapeAnchorRectangle {
          left: -120,
          top: 240,
          right: 1_440,
          bottom: 2_880,
        },
        header: false,
        horizontal_origin: 2,
        vertical_origin: 1,
        wrap_style: 3,
        wrap_side: 0,
        simple_rectangle: false,
        below_text: true,
        anchor_locked: false,
        textbox_count: 0,
      }],
    };
    assert_eq!(
      ShapeAnchorTable::from_bytes(&shape_anchors.to_bytes().unwrap()).unwrap(),
      shape_anchors
    );
    let office_art_bytes = [
      0x0f, 0x00, 0x00, 0xf0, 0, 0, 0, 0, 0, 0x0f, 0x00, 0x02, 0xf0, 12, 0, 0, 0, 0, 0, 0x10, 0xf0,
      4, 0, 0, 0, 0, 0, 0, 0,
    ];
    let office_art = DocOfficeArtContent::from_bytes(&office_art_bytes).unwrap();
    let mut word_anchors = 0;
    office_art.drawings[0].container.visit_complete(|record| {
      word_anchors += usize::from(matches!(
        record.data,
        OfficeArtRecordData::WordClientAnchor(0)
      ));
    });
    assert_eq!(word_anchors, 1);
    assert_eq!(office_art.to_bytes().unwrap(), office_art_bytes);
  }

  #[test]
  fn user_input_methods_round_trip_guid_references_and_service_data() {
    let methods = UserInputMethods {
      positions: vec![9, 3, 3],
      methods: vec![
        UserInputMethod {
          service_category_index: 0,
          service_clsid_index: 1,
          service_data_offset: 2,
          character_count: 4,
          service_data_size: 3,
          private_data: 0x1234_5678,
        },
        UserInputMethod {
          service_category_index: 1,
          service_clsid_index: 0,
          service_data_offset: -1,
          character_count: 0,
          service_data_size: 0,
          private_data: 0,
        },
      ],
      service_guids: vec![[0x11; 16], [0x22; 16]],
    };
    let (method_bytes, guid_bytes) = methods.to_bytes().unwrap();
    assert_eq!(
      UserInputMethods::from_bytes(&method_bytes, &guid_bytes).unwrap(),
      methods
    );
    assert_eq!(methods.methods[0].service_data(b"abcdef").unwrap(), b"cde");
    assert_eq!(methods.methods[1].service_data(b"abcdef").unwrap(), b"");

    let mut invalid = methods;
    invalid.methods[0].service_clsid_index = 2;
    assert!(invalid.to_bytes().is_err());
  }

  #[test]
  fn printer_driver_info_round_trips_four_terminated_ansi_strings() {
    let info = PrinterDriverInfo {
      printer_name: b"Network Printer".to_vec(),
      port_name: b"LPT1:".to_vec(),
      driver_name: b"driver.dll".to_vec(),
      product_name: b"Printer Model".to_vec(),
    };
    let bytes = info.to_bytes().unwrap();
    assert_eq!(PrinterDriverInfo::from_bytes(&bytes).unwrap(), info);

    let mut unterminated = bytes;
    unterminated.pop();
    assert!(PrinterDriverInfo::from_bytes(&unterminated).is_err());
  }

  #[test]
  fn ole_object_descriptor_round_trips_control_storage_flags() {
    let value = OleObjectDescriptor {
      persist1: OleObjectPersist1Flags::DEFAULT_HANDLER
        | OleObjectPersist1Flags::OCX
        | OleObjectPersist1Flags::STREAM
        | OleObjectPersist1Flags::VIEW_OBJECT,
      clipboard_format: OleObjectClipboardFormat::Metafile,
      persist2: Some(
        OleObjectPersist2Flags::EMF_PRESENTATION
          | OleObjectPersist2Flags::QUERIED_EMF
          | OleObjectPersist2Flags::STORED_AS_EMF,
      ),
    };
    let bytes = value.to_bytes();
    assert_eq!(OleObjectDescriptor::from_bytes(&bytes).unwrap(), value);
    assert!(value.is_ole_control());
    assert!(value.control_uses_stream());
    assert!(OleObjectDescriptor::from_bytes(&bytes[..5]).is_err());

    let compatibility = OleObjectDescriptor::from_bytes(&[0, 0, 0x34, 0x12]).unwrap();
    assert_eq!(
      compatibility.clipboard_format,
      OleObjectClipboardFormat::Compatibility(0x1234)
    );
    assert_eq!(compatibility.to_bytes(), [0, 0, 0x34, 0x12]);
  }

  #[test]
  fn ole_control_info_round_trips_static_field_links_and_flags() {
    let infos = OleControlInfos {
      controls: vec![OleControlInfo {
        cookie: 3,
        field_index: 7,
        ignored_accelerator_handle: 0x1234,
        accelerator_count: 2,
        field_linked: true,
        eats_return: true,
        eats_escape: false,
        default_button: true,
        cancel_button: false,
        failed_load: false,
        right_to_left: true,
        corrupt: false,
        reserved1: 0x80,
        document_part: OleControlDocumentPart::Textbox,
        reserved2: 0x5678,
      }],
    };
    let bytes = infos.to_bytes().unwrap();
    assert_eq!(OleControlInfos::from_bytes(&bytes).unwrap(), infos);

    let mut duplicate = infos;
    duplicate.controls.push(duplicate.controls[0]);
    let bytes = duplicate.to_bytes().unwrap();
    assert_eq!(OleControlInfos::from_bytes(&bytes).unwrap(), duplicate);
  }

  #[test]
  fn list_names_round_trip_unique_utf16_names() {
    let names = ListNamesTable {
      names: vec![
        Vec::new(),
        "Legal".encode_utf16().collect(),
        "章节".encode_utf16().collect(),
      ],
    };
    let bytes = names.to_bytes().unwrap();
    assert_eq!(ListNamesTable::from_bytes(&bytes).unwrap(), names);

    let duplicate = ListNamesTable {
      names: vec![
        "same".encode_utf16().collect(),
        "same".encode_utf16().collect(),
      ],
    };
    assert!(duplicate.to_bytes().is_err());
  }

  #[test]
  fn list_definition_and_override_tables_round_trip_static_levels() {
    let level = ListLevel {
      info: ListLevelInfo {
        start_at: 1,
        number_format: 0,
        justification: 0,
        legal: false,
        no_restart: false,
        indent_saved: false,
        converted: false,
        unused1: false,
        tentative: false,
        placeholder_offsets: [1, 0, 0, 0, 0, 0, 0, 0, 0],
        follow_character: 0,
        saved_indent: 360,
        unused2: 0,
        restart_limit: 0,
        html_incompatibilities: 0,
      },
      paragraph_properties: GrpPrl { properties: vec![] },
      paragraph_incomplete_prl_tail: vec![],
      number_properties: GrpPrl { properties: vec![] },
      number_incomplete_prl_tail: vec![],
      number_text: vec![0, b'.' as u16],
    };
    let definitions = ListDefinitions {
      levels_in_declared_length: false,
      definitions: vec![ListDefinition {
        info: ListDefinitionInfo {
          list_id: 42,
          template_code: 7,
          paragraph_style_indexes: [0x0fff; 9],
          simple: true,
          unused1: false,
          auto_number: false,
          unused2: false,
          hybrid: false,
          reserved: 0,
          html_incompatibilities: 0,
        },
        levels: vec![level.clone()],
      }],
    };
    let (base, levels) = definitions.to_bytes().unwrap();
    let mut table = base.clone();
    table.extend_from_slice(&levels);
    assert_eq!(
      ListDefinitions::from_table_stream(
        &table,
        FibFcLcb {
          fc: 0,
          lcb: base.len() as u32,
        },
      )
      .unwrap(),
      definitions
    );

    let overrides = ListOverrides {
      overrides: vec![ListOverride {
        info: ListOverrideInfo {
          list_id: 42,
          unused1: 0,
          unused2: 0,
          field_type: 0,
          html_incompatibilities: 0,
          unused3: 0,
        },
        data: ListOverrideData {
          first_paragraph_position: 10,
          levels: vec![ListLevelOverride {
            start_at: 3,
            level_index: 0,
            overrides_start: true,
            overrides_formatting: true,
            html_incompatibilities: 0,
            unused1: 0,
            unused2: 0,
            level: Some(level),
          }],
        },
      }],
    };
    assert_eq!(
      ListOverrides::from_bytes(&overrides.to_bytes().unwrap()).unwrap(),
      overrides
    );
  }

  #[test]
  fn document_properties_round_trip_all_supported_physical_lengths() {
    for length in [500, 544, 594, 600, 610, 616, 617, 674, 690, 694] {
      let mut physical = vec![0; length];
      if length >= 674 {
        physical[620..624].copy_from_slice(&0x0020_u32.to_le_bytes());
        physical[640..644].copy_from_slice(&0x0010_u32.to_le_bytes());
        physical[654..658].copy_from_slice(&120_u32.to_le_bytes());
        physical[658..662].copy_from_slice(&120_u32.to_le_bytes());
      }
      if length >= 690 {
        physical[674..678].copy_from_slice(&1_u32.to_le_bytes());
      }
      let mut properties = DocumentProperties::from_bytes(&physical).unwrap();
      assert_eq!(properties.encoded_len(), length);
      assert_eq!(properties.to_bytes().unwrap(), physical);

      properties.word97.base.statistics.main.words = 42;
      let encoded = properties.to_bytes().unwrap();
      let reparsed = DocumentProperties::from_bytes(&encoded).unwrap();
      assert_eq!(reparsed.word97.base.statistics.main.words, 42);
      assert_eq!(reparsed, properties);
    }
  }

  #[test]
  fn document_properties_97_round_trips_static_classification_space_lists_and_formats() {
    let mut physical = vec![0; 500];
    let created = Dttm {
      minute: 1,
      hour: 2,
      day: 3,
      month: 4,
      year_offset: 125,
      weekday: 5,
    };
    let last_printed = Dttm {
      minute: 59,
      hour: 23,
      day: 31,
      month: 12,
      year_offset: 124,
      weekday: 6,
    };
    physical[10..12].copy_from_slice(&720_i16.to_le_bytes());
    physical[12..14].copy_from_slice(&1_252_u16.to_le_bytes());
    physical[14..16].copy_from_slice(&360_u16.to_le_bytes());
    physical[16..18].copy_from_slice(&2_u16.to_le_bytes());
    physical[20..24].copy_from_slice(&created.to_u32().unwrap().to_le_bytes());
    physical[28..32].copy_from_slice(&last_printed.to_u32().unwrap().to_le_bytes());
    physical[32..34].copy_from_slice(&7_i16.to_le_bytes());
    physical[34..38].copy_from_slice(&(-12_i32).to_le_bytes());
    physical[38..42].copy_from_slice(&10_i32.to_le_bytes());
    physical[42..46].copy_from_slice(&20_i32.to_le_bytes());
    physical[46..48].copy_from_slice(&1_i16.to_le_bytes());
    physical[48..52].copy_from_slice(&2_i32.to_le_bytes());
    physical[56..60].copy_from_slice(&3_i32.to_le_bytes());
    physical[60..64].copy_from_slice(&11_i32.to_le_bytes());
    physical[64..68].copy_from_slice(&21_i32.to_le_bytes());
    physical[68..70].copy_from_slice(&1_i16.to_le_bytes());
    physical[70..74].copy_from_slice(&3_i32.to_le_bytes());
    physical[74..78].copy_from_slice(&4_i32.to_le_bytes());
    physical[78..82].copy_from_slice(&(-123_i32).to_le_bytes());
    physical[88..90].copy_from_slice(&2_i16.to_le_bytes());
    for (index, byte) in physical[442..472].iter_mut().enumerate() {
      *byte = index as u8;
    }
    physical[426..430].copy_from_slice(&10_i32.to_le_bytes());
    physical[430..434].copy_from_slice(&20_i32.to_le_bytes());
    physical[476..478].copy_from_slice(&0x1234_u16.to_le_bytes());
    physical[478..480].copy_from_slice(&0x5678_u16.to_le_bytes());
    physical[480..484].copy_from_slice(&3_i32.to_le_bytes());
    physical[484..488].copy_from_slice(&4_i32.to_le_bytes());
    physical[488..492].copy_from_slice(&0x1122_3344_u32.to_le_bytes());
    physical[492..494].copy_from_slice(&u16::from(NumberingFormat::BULLET.code()).to_le_bytes());
    physical[494..496].copy_from_slice(&u16::from(NumberingFormat::NONE.code()).to_le_bytes());
    physical[496..498].copy_from_slice(&0xfedc_u16.to_le_bytes());
    physical[498..500].copy_from_slice(&0xffff_u16.to_le_bytes());

    let properties = DocumentProperties::from_bytes(&physical).unwrap();
    assert_eq!(properties.word97.base.default_tab_width, 720);
    assert_eq!(properties.word97.base.web_code_page, CodePage(1_252));
    assert_eq!(properties.word97.base.hyphenation_zone, 360);
    assert_eq!(properties.word97.base.consecutive_hyphen_limit, 2);
    assert_eq!(properties.word97.base.reserved2, 0);
    assert_eq!(properties.word97.base.created, created);
    assert_eq!(properties.word97.base.revised.to_u32().unwrap(), 0);
    assert_eq!(properties.word97.base.last_printed, last_printed);
    assert_eq!(properties.word97.base.revision_count, 7);
    assert_eq!(properties.word97.base.editing_time, -12);
    assert_eq!(
      properties.word97.base.protection_password_hash,
      DocumentProtectionPasswordHash(-123)
    );
    assert_eq!(
      properties.word97.base.statistics,
      DocumentStatistics {
        main: DocumentStoryStatistics {
          words: 10,
          characters: 20,
          pages: 1,
          paragraphs: 2,
          lines: 3,
        },
        with_subdocuments: DocumentStoryStatistics {
          words: 11,
          characters: 21,
          pages: 1,
          paragraphs: 3,
          lines: 4,
        },
      }
    );
    properties
      .word97
      .base
      .statistics
      .validate_count_relations()
      .unwrap();
    assert_eq!(properties.word97.base.exact_statistics(), None);
    assert_eq!(
      properties.word97.base.statistics.exact(true, false),
      Some(properties.word97.base.statistics.main)
    );
    assert_eq!(
      properties.word97.base.statistics.exact(true, true),
      Some(properties.word97.base.statistics.with_subdocuments)
    );
    assert_eq!(
      properties.word97.document_classification,
      DocumentClassification::Email
    );
    assert_eq!(
      properties.word97.undefined_space.bytes(),
      &physical[442..472]
    );
    assert_eq!(
      properties.word97.last_list_indexes,
      LastListIndexes {
        bullet: 0x1234,
        numbering: 0x5678,
      }
    );
    assert_eq!(
      properties.word97.characters_with_spaces,
      DocumentCharacterCountPair {
        main: 10,
        with_subdocuments: 20,
      }
    );
    assert_eq!(
      properties.word97.double_byte_characters,
      DocumentCharacterCountPair {
        main: 3,
        with_subdocuments: 4,
      }
    );
    properties
      .word97
      .validate_character_count_relations()
      .unwrap();
    assert_eq!(properties.word97.reserved3a, 0x1122_3344);
    assert_eq!(
      properties.word97.footnote_number_format,
      NumberingFormat::BULLET
    );
    assert_eq!(
      properties.word97.endnote_number_format,
      NumberingFormat::NONE
    );
    assert_eq!(properties.word97.pagination_zoom_font_size, 0xfedc);
    assert_eq!(properties.word97.pagination_screen_height, 0xffff);
    assert_eq!(properties.to_bytes().unwrap(), physical);
    let mut invalid_revision = properties.clone();
    invalid_revision.word97.base.revision_count = -1;
    assert!(invalid_revision.to_bytes().is_err());
    let mut invalid_reserved = properties.clone();
    invalid_reserved.word97.base.reserved2 = 1;
    assert!(invalid_reserved.to_bytes().is_err());
    let mut invalid_exact_statistics = properties.clone();
    invalid_exact_statistics
      .word97
      .base
      .document_flags
      .exact_statistics = true;
    invalid_exact_statistics.word97.base.statistics.main.words = -1;
    assert!(invalid_exact_statistics.to_bytes().is_err());
    let cache = properties
      .word97
      .deprecated_numbering_field_cache_metadata(Some(FibFcLcb { fc: 4, lcb: 12 }));
    assert!(cache.is_present());
    assert_eq!(cache.maximum_valid_position, 0);
    assert!(!cache.invalid);
    assert!(
      !properties
        .word97
        .deprecated_numbering_field_cache_metadata(None)
        .is_present()
    );
    assert!(
      LastListIndexes {
        bullet: 0,
        numbering: 0
      }
      .validate_override_count(0)
      .is_ok()
    );
    assert!(
      LastListIndexes {
        bullet: 0,
        numbering: 0
      }
      .validate_override_count(1)
      .is_ok()
    );
    assert!(
      LastListIndexes {
        bullet: 1,
        numbering: 0
      }
      .validate_override_count(1)
      .is_err()
    );
    assert!(
      LastListIndexes {
        bullet: 0,
        numbering: 1
      }
      .validate_override_count(1)
      .is_err()
    );
    assert!(
      DocumentCharacterCountPair {
        main: -1,
        with_subdocuments: 0,
      }
      .validate_count_relation("test")
      .is_err()
    );
    assert!(
      DocumentCharacterCountPair {
        main: 2,
        with_subdocuments: 1,
      }
      .validate_count_relation("test")
      .is_err()
    );

    let mut invalid_classification = vec![0; 500];
    invalid_classification[88..90].copy_from_slice(&3_i16.to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid_classification).is_err());

    for value in [0x0040_u16, 0x0100] {
      let mut invalid_number_format = vec![0; 500];
      invalid_number_format[492..494].copy_from_slice(&value.to_le_bytes());
      assert!(DocumentProperties::from_bytes(&invalid_number_format).is_err());
    }
  }

  #[test]
  fn document_properties_2002_round_trips_static_tail_and_rejects_invalid_flags() {
    let mut physical = vec![0; 594];
    physical[544..548].copy_from_slice(&0x1122_3344_u32.to_le_bytes());
    physical[548..550].copy_from_slice(&0xfcff_u16.to_le_bytes());
    physical[550..552].copy_from_slice(&0x1234_u16.to_le_bytes());
    physical[552..554].copy_from_slice(&0xffff_u16.to_le_bytes());
    physical[554..556].copy_from_slice(&0x5678_u16.to_le_bytes());
    physical[556..558].copy_from_slice(&0x009a_u16.to_le_bytes());
    physical[558..562].copy_from_slice(&1252_u32.to_le_bytes());
    for (index, value) in [10_u32, 20, 30, 40, 50, 60, 70].into_iter().enumerate() {
      let offset = 562 + index * 4;
      physical[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    physical[590..594].copy_from_slice(&0xaabb_ccdd_u32.to_le_bytes());

    let properties = DocumentProperties::from_bytes(&physical).unwrap();
    let word2002 = properties.extension.word2002().unwrap();
    assert_eq!(word2002.unused, 0x1122_3344);
    assert!(word2002.flags.do_not_embed_system_font);
    assert!(word2002.flags.word_compatibility);
    assert!(word2002.flags.live_recover);
    assert!(word2002.flags.embed_factoids);
    assert!(word2002.flags.factoid_xml);
    assert!(word2002.flags.factoid_all_done);
    assert!(word2002.flags.folio_print);
    assert!(word2002.flags.reverse_folio);
    assert_eq!(
      word2002.flags.text_line_ending,
      TextLineEnding::UnicodeSeparator
    );
    assert!(word2002.flags.hide_format_consistency);
    assert!(word2002.flags.show_markup);
    assert!(word2002.flags.show_comments);
    assert!(word2002.flags.show_insertions_deletions);
    assert!(word2002.flags.show_property_changes);
    assert_eq!(word2002.default_table_style, 0x1234);
    assert_eq!(word2002.feature_compatibility.bits().unwrap(), 0xffff);
    assert_eq!(word2002.style_filter, 0x5678);
    assert_eq!(word2002.booklet_pages, 0x009a);
    assert_eq!(word2002.text_code_page, 1252);
    assert_eq!(
      word2002.minimum_revision_positions,
      RevisionMinimumPositions {
        main: 10,
        footnote: 20,
        header: 30,
        comment: 40,
        endnote: 50,
        textbox: 60,
        header_textbox: 70,
      }
    );
    assert_eq!(word2002.root_revision_save_id, 0xaabb_ccdd);
    assert_eq!(properties.to_bytes().unwrap(), physical);

    let mut invalid_line_ending = vec![0; 594];
    invalid_line_ending[548..550].copy_from_slice(&0x0500_u16.to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid_line_ending).is_err());

    let mut invalid_folio = vec![0; 594];
    invalid_folio[548..550].copy_from_slice(&0x0080_u16.to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid_folio).is_err());

    let mut invalid_writer = properties;
    if let DocumentPropertiesExtension::Word2002(word2002) = &mut invalid_writer.extension {
      word2002.feature_compatibility.unused |= 1;
    }
    assert!(invalid_writer.to_bytes().is_err());
  }

  #[test]
  fn document_properties_2003_round_trips_static_tail_and_rejects_reserved_values() {
    let mut physical = vec![0; 616];
    physical[594..598].copy_from_slice(&0x1fff_u32.to_le_bytes());
    physical[598..600].copy_from_slice(&0x00ff_u16.to_le_bytes());
    physical[600..604].copy_from_slice(&12_240_u32.to_le_bytes());
    physical[604..608].copy_from_slice(&15_840_u32.to_le_bytes());
    physical[608..612].copy_from_slice(&125_u32.to_le_bytes());
    physical[612] = 0x07;
    physical[614..616].copy_from_slice(&0x1234_u16.to_le_bytes());

    let properties = DocumentProperties::from_bytes(&physical).unwrap();
    let word2003 = properties.extension.word2003().unwrap();
    assert!(word2003.flags.treat_comment_lock_as_read_only);
    assert!(word2003.flags.style_lock);
    assert!(word2003.flags.auto_format_override);
    assert!(word2003.flags.remove_wordml);
    assert!(word2003.flags.apply_custom_xml_transform);
    assert!(word2003.flags.style_lock_enforced);
    assert!(word2003.flags.compatibility_comment_lock);
    assert!(word2003.flags.ignore_mixed_content);
    assert!(word2003.flags.show_placeholder_text);
    assert!(word2003.flags.unused);
    assert!(word2003.flags.word97_document);
    assert!(word2003.flags.lock_theme);
    assert!(word2003.flags.lock_quick_format_style_set);
    assert!(word2003.protection.reading_mode_ink_lockdown);
    assert!(word2003.protection.show_ink_annotations);
    assert!(word2003.protection.remove_annotation_date_time);
    assert!(word2003.protection.enforce);
    assert_eq!(word2003.protection.mode, DocumentProtectionMode::None);
    assert!(word2003.protection.display_background_shapes);
    assert_eq!(word2003.page_lock_width, 12_240);
    assert_eq!(word2003.page_lock_height, 15_840);
    assert_eq!(word2003.locked_font_percentage, 125);
    assert_eq!(word2003.state_toolbars.bits(), 0x07);
    assert_eq!(word2003.list_override_cleanup_limit, 0x1234);
    assert_eq!(properties.to_bytes().unwrap(), physical);

    let mut cleanup = *word2003;
    cleanup.list_override_cleanup_limit = 0;
    assert!(cleanup.validate_cleanup_limit(0).is_ok());
    assert!(cleanup.validate_cleanup_limit(1).is_ok());
    cleanup.list_override_cleanup_limit = 1;
    assert!(cleanup.validate_cleanup_limit(1).is_err());

    for (offset, value, width) in [
      (594, 0x0000_2000_u32, 4),
      (594, 0x0000_0020, 4),
      (598, 0x0000_0040, 2),
      (598, 0x0000_0100, 2),
      (612, 0x0000_0008, 1),
      (613, 0x0000_0001, 1),
    ] {
      let mut invalid = vec![0; 616];
      invalid[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
      assert!(DocumentProperties::from_bytes(&invalid).is_err());
    }

    let mut invalid_writer = properties;
    if let DocumentPropertiesExtension::Word2003(word2003) = &mut invalid_writer.extension {
      word2003.flags.style_lock = false;
    }
    assert!(invalid_writer.to_bytes().is_err());
  }

  #[test]
  fn document_properties_2007_round_trips_static_math_and_rejects_reserved_values() {
    let mut physical = vec![0; 674];
    physical[616..620].copy_from_slice(&0x1122_3344_u32.to_le_bytes());
    physical[620..624].copy_from_slice(&0x0683_u32.to_le_bytes());
    physical[640..644].copy_from_slice(&0x1fca_u32.to_le_bytes());
    physical[644..646].copy_from_slice(&42_u16.to_le_bytes());
    physical[646..650].copy_from_slice(&100_i32.to_le_bytes());
    physical[650..654].copy_from_slice(&200_i32.to_le_bytes());
    physical[654..658].copy_from_slice(&120_u32.to_le_bytes());
    physical[658..662].copy_from_slice(&120_u32.to_le_bytes());
    physical[670..674].copy_from_slice(&300_i32.to_le_bytes());

    let properties = DocumentProperties::from_bytes(&physical).unwrap();
    let word2007 = properties.extension.word2007().unwrap();
    assert_eq!(word2007.reserved, 0x1122_3344);
    assert!(word2007.flags.track_formatting);
    assert!(word2007.flags.track_moves);
    assert_eq!(word2007.flags.style_sort_method, StyleSortMethod::StyleType);
    assert!(word2007.flags.reading_mode_actual_pages);
    assert!(word2007.flags.auto_compress_pictures);
    assert_eq!(
      word2007.math.binary_operator_break,
      MathBinaryOperatorBreak::Repeat
    );
    assert_eq!(
      word2007.math.binary_subtraction_break,
      MathBinarySubtractionBreak::MinusPlus
    );
    assert_eq!(word2007.math.justification, MathJustification::Right);
    assert!(word2007.math.reserved);
    assert!(word2007.math.small_fraction);
    assert!(word2007.math.integral_limits_above_below);
    assert!(word2007.math.nary_limits_above_below);
    assert!(word2007.math.wrapped_line_align_left);
    assert!(word2007.math.use_display_defaults);
    assert_eq!(word2007.math.font_index, 42);
    assert_eq!(word2007.math.left_margin, 100);
    assert_eq!(word2007.math.right_margin, 200);
    assert_eq!(
      word2007.math.fixed_constants,
      MathFixedConstants::Standard120
    );
    assert_eq!(word2007.math.wrapped_line_indent, 300);
    word2007.math.validate_standard().unwrap();
    assert_eq!(properties.to_bytes().unwrap(), physical);

    let mut compatibility = physical.clone();
    compatibility[640..644].copy_from_slice(&0x1f8a_u32.to_le_bytes());
    compatibility[654..662].fill(0);
    let compatibility_properties = DocumentProperties::from_bytes(&compatibility).unwrap();
    let compatibility_math = &compatibility_properties.extension.word2007().unwrap().math;
    assert_eq!(
      compatibility_math.justification,
      MathJustification::ProducerCompatibilityZero
    );
    assert_eq!(
      compatibility_math.fixed_constants,
      MathFixedConstants::ProducerCompatibilityZero
    );
    assert!(compatibility_math.validate_standard().is_err());
    assert_eq!(compatibility_properties.to_bytes().unwrap(), compatibility);

    for style_sort_method in [
      StyleSortMethod::Name,
      StyleSortMethod::ApplicationDefault,
      StyleSortMethod::Font,
      StyleSortMethod::BasedOn,
      StyleSortMethod::StyleType,
    ] {
      let mut variant = properties.clone();
      if let DocumentPropertiesExtension::Word2007(word2007) = &mut variant.extension {
        word2007.flags.style_sort_method = style_sort_method;
      }
      let encoded = variant.to_bytes().unwrap();
      assert_eq!(DocumentProperties::from_bytes(&encoded).unwrap(), variant);
    }

    for (operator, subtraction, justification) in [
      (
        MathBinaryOperatorBreak::Before,
        MathBinarySubtractionBreak::MinusMinus,
        MathJustification::CenteredAsGroup,
      ),
      (
        MathBinaryOperatorBreak::After,
        MathBinarySubtractionBreak::PlusMinus,
        MathJustification::Center,
      ),
      (
        MathBinaryOperatorBreak::Repeat,
        MathBinarySubtractionBreak::MinusPlus,
        MathJustification::Left,
      ),
      (
        MathBinaryOperatorBreak::Repeat,
        MathBinarySubtractionBreak::MinusPlus,
        MathJustification::Right,
      ),
    ] {
      let mut variant = properties.clone();
      if let DocumentPropertiesExtension::Word2007(word2007) = &mut variant.extension {
        word2007.math.binary_operator_break = operator;
        word2007.math.binary_subtraction_break = subtraction;
        word2007.math.justification = justification;
      }
      let encoded = variant.to_bytes().unwrap();
      assert_eq!(DocumentProperties::from_bytes(&encoded).unwrap(), variant);
    }

    for (offset, value) in [
      (620, 0x0000_0004_u32),
      (620, 0x0000_00a0),
      (624, 0x0000_0001),
      (640, 0x0000_0013),
      (640, 0x0000_001c),
      (640, 0x0000_0050),
      (640, 0x0000_2010),
      (654, 0x0000_0000),
      (658, 0x0000_0000),
      (662, 0x0000_0001),
    ] {
      let mut invalid = physical.clone();
      invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
      assert!(DocumentProperties::from_bytes(&invalid).is_err());
    }

    for (offset, value) in [(646, -1_i32), (650, 31_681), (670, 31_681)] {
      let mut invalid = physical.clone();
      invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
      assert!(DocumentProperties::from_bytes(&invalid).is_err());
    }

    let mut invalid_writer = properties;
    if let DocumentPropertiesExtension::Word2007(word2007) = &mut invalid_writer.extension {
      word2007.math.wrapped_line_indent = -1;
    }
    assert!(invalid_writer.to_bytes().is_err());
  }

  #[test]
  fn document_properties_2010_round_trips_static_image_and_identifier_settings() {
    let mut physical = vec![0; 690];
    physical[620..624].copy_from_slice(&0x0020_u32.to_le_bytes());
    physical[640..644].copy_from_slice(&0x0010_u32.to_le_bytes());
    physical[654..658].copy_from_slice(&120_u32.to_le_bytes());
    physical[658..662].copy_from_slice(&120_u32.to_le_bytes());
    physical[674..678].copy_from_slice(&0x7fff_ffff_u32.to_le_bytes());
    physical[678..682].copy_from_slice(&0x1122_3344_u32.to_le_bytes());
    physical[682..686].copy_from_slice(&1_u32.to_le_bytes());
    physical[686..690].copy_from_slice(&330_u32.to_le_bytes());

    let properties = DocumentProperties::from_bytes(&physical).unwrap();
    let word2010 = properties.extension.word2010().unwrap();
    assert_eq!(
      word2010.paragraph_identifier_context,
      ParagraphIdentifierContext::Standard(0x7fff_ffff)
    );
    assert!(word2010.paragraph_identifier_context.is_standard());
    assert_eq!(word2010.reserved, 0x1122_3344);
    assert!(word2010.discard_image_editing_data);
    assert_eq!(word2010.image_resolution_dpi, 330);
    assert_eq!(properties.to_bytes().unwrap(), physical);

    let mut compatibility = physical.clone();
    compatibility[674..678].fill(0);
    let compatibility_properties = DocumentProperties::from_bytes(&compatibility).unwrap();
    let compatibility_context = compatibility_properties
      .extension
      .word2010()
      .unwrap()
      .paragraph_identifier_context;
    assert_eq!(
      compatibility_context,
      ParagraphIdentifierContext::ProducerCompatibilityZero
    );
    assert!(!compatibility_context.is_standard());
    assert_eq!(compatibility_properties.to_bytes().unwrap(), compatibility);

    for paragraph_identifier_context in [0x8000_0000, u32::MAX] {
      let mut invalid = physical.clone();
      invalid[674..678].copy_from_slice(&paragraph_identifier_context.to_le_bytes());
      assert!(DocumentProperties::from_bytes(&invalid).is_err());
    }
    let mut invalid_flags = physical.clone();
    invalid_flags[682..686].copy_from_slice(&2_u32.to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid_flags).is_err());

    let mut invalid_writer = properties;
    if let DocumentPropertiesExtension::Word2010(word2010) = &mut invalid_writer.extension {
      word2010.paragraph_identifier_context = ParagraphIdentifierContext::Standard(0);
    }
    assert!(invalid_writer.to_bytes().is_err());
  }

  #[test]
  fn document_properties_2013_round_trips_static_chart_tracking_flag() {
    let mut physical = vec![0; 694];
    physical[620..624].copy_from_slice(&0x0020_u32.to_le_bytes());
    physical[640..644].copy_from_slice(&0x0010_u32.to_le_bytes());
    physical[654..658].copy_from_slice(&120_u32.to_le_bytes());
    physical[658..662].copy_from_slice(&120_u32.to_le_bytes());
    physical[674..678].copy_from_slice(&1_u32.to_le_bytes());
    physical[690..694].copy_from_slice(&1_u32.to_le_bytes());

    let properties = DocumentProperties::from_bytes(&physical).unwrap();
    assert!(
      properties
        .extension
        .word2013()
        .unwrap()
        .chart_tracking_reference_based
    );
    assert_eq!(properties.to_bytes().unwrap(), physical);

    let mut disabled = physical.clone();
    disabled[690..694].fill(0);
    let disabled_properties = DocumentProperties::from_bytes(&disabled).unwrap();
    assert!(
      !disabled_properties
        .extension
        .word2013()
        .unwrap()
        .chart_tracking_reference_based
    );
    assert_eq!(disabled_properties.to_bytes().unwrap(), disabled);

    let mut invalid = physical;
    invalid[690..694].copy_from_slice(&2_u32.to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid).is_err());
  }

  #[test]
  fn document_typography_round_trips_static_flags_counts_and_slots() {
    let mut properties = DocumentProperties::from_bytes(&vec![0; 500]).unwrap();
    let typography = &mut properties.word97.typography;
    typography.kern_punctuation = true;
    typography.justification = TypographyJustification::CompressPunctuationAndJapaneseKana;
    typography.kinsoku_level = KinsokuLevel::Custom;
    typography.print_two_on_one = true;
    typography.unused = true;
    typography.custom_kinsoku_language = CustomKinsokuLanguage::ChineseTraditional;
    typography.japanese_use_level2 = true;
    typography.following_punctuation_count = 3;
    typography.leading_punctuation_count = 2;
    typography.following_punctuation_slots[..4].copy_from_slice(&[
      b'!' as u16,
      0x3001,
      0x3002,
      0xaaaa,
    ]);
    typography.leading_punctuation_slots[..3].copy_from_slice(&[b'(' as u16, 0x3008, 0xbbbb]);

    let bytes = properties.to_bytes().unwrap();
    let reparsed = DocumentProperties::from_bytes(&bytes).unwrap();
    assert_eq!(reparsed, properties);
    reparsed.word97.validate_compatibility_options().unwrap();
    assert_eq!(
      reparsed.word97.typography.following_punctuation().unwrap(),
      [b'!' as u16, 0x3001, 0x3002]
    );
    assert_eq!(
      reparsed.word97.typography.leading_punctuation().unwrap(),
      [b'(' as u16, 0x3008]
    );
    assert_eq!(
      reparsed.word97.typography.following_punctuation_slots[3],
      0xaaaa
    );
    assert_eq!(
      reparsed.word97.typography.leading_punctuation_slots[2],
      0xbbbb
    );

    let mut invalid_flags = bytes.clone();
    invalid_flags[90..92].copy_from_slice(&0x1800_u16.to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid_flags).is_err());
    let mut invalid_following_count = bytes.clone();
    invalid_following_count[92..94].copy_from_slice(&101_u16.to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid_following_count).is_err());
    let mut invalid_leading_count = bytes;
    invalid_leading_count[94..96].copy_from_slice(&51_u16.to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid_leading_count).is_err());

    let mut invalid_writer = properties;
    invalid_writer.word97.typography.following_punctuation_count = 101;
    assert!(invalid_writer.to_bytes().is_err());
    assert!(
      invalid_writer
        .word97
        .typography
        .following_punctuation()
        .is_err()
    );
  }

  #[test]
  fn document_compatibility_options_round_trip_all_named_bits_and_match() {
    let mut properties = DocumentProperties::from_bytes(&vec![0; 500]).unwrap();
    let word6 = CompatibilityOptions60::from_bits(u16::MAX);
    let mut word8 = CompatibilityOptions80::from_bits(u32::MAX);
    word8.word6 = word6;
    properties.word97.base.compatibility_options_60 = word6;
    properties.word97.compatibility_options_80 = word8;

    let bytes = properties.to_bytes().unwrap();
    assert_eq!(&bytes[8..10], &u16::MAX.to_le_bytes());
    assert_eq!(&bytes[84..88], &u32::MAX.to_le_bytes());
    let reparsed = DocumentProperties::from_bytes(&bytes).unwrap();
    assert_eq!(reparsed, properties);
    reparsed.word97.validate_compatibility_options().unwrap();
    assert!(reparsed.word97.compatibility_options_80.use_printer_metrics);
    assert!(reparsed.word97.base.compatibility_options_60.unused);

    let mut mismatched_bytes = bytes;
    mismatched_bytes[84..88].copy_from_slice(&0xffff_fffe_u32.to_le_bytes());
    let mismatched = DocumentProperties::from_bytes(&mismatched_bytes).unwrap();
    assert!(!mismatched.word97.compatibility_options_match());
    assert!(mismatched.word97.validate_compatibility_options().is_err());
    assert_eq!(mismatched.to_bytes().unwrap(), mismatched_bytes);

    let mut mismatched_writer = properties;
    mismatched_writer
      .word97
      .compatibility_options_80
      .word6
      .no_tab_for_hanging_indent = false;
    assert!(!mismatched_writer.word97.compatibility_options_match());
    assert_eq!(mismatched_writer.to_bytes().unwrap()[84] & 1, 0);
  }

  #[test]
  fn document_base_packed_fields_round_trip_static_bits_and_enums() {
    let mut properties = DocumentProperties::from_bytes(&vec![0; 500]).unwrap();
    properties.word97.base.format_flags = DocumentFormatFlags::from_bits(0xdf).unwrap();
    properties.word97.base.unused4 = 0xaa;
    properties.word97.base.footnote_numbering = NoteNumbering::from_bits(0x48d2).unwrap();
    let document_flag_bits = !(1_u32 << 20);
    properties.word97.base.document_flags = DocumentStateFlags::from_bits(document_flag_bits);
    properties.word97.base.endnote_numbering = NoteNumbering::from_bits(0x8d15).unwrap();
    properties.word97.base.endnote_options = EndnoteOptions::from_bits(0xbfff).unwrap();
    properties.word97.base.saved_view = SavedView::from_bits(0xffa5).unwrap();

    let bytes = properties.to_bytes().unwrap();
    assert_eq!(bytes[0], 0xdf);
    assert_eq!(bytes[1], 0xaa);
    assert_eq!(&bytes[2..4], &0x48d2_u16.to_le_bytes());
    assert_eq!(&bytes[4..8], &document_flag_bits.to_le_bytes());
    assert_eq!(&bytes[52..54], &0x8d15_u16.to_le_bytes());
    assert_eq!(&bytes[54..56], &0xbfff_u16.to_le_bytes());
    assert_eq!(&bytes[82..84], &0xffa5_u16.to_le_bytes());
    assert_eq!(DocumentProperties::from_bytes(&bytes).unwrap(), properties);

    let compatibility_view = SavedView::from_bits(7 | (100 << 3)).unwrap();
    assert_eq!(compatibility_view.kind, SavedViewKind::Compatibility7);
    assert!(!compatibility_view.kind.is_standard());
    assert_eq!(compatibility_view.bits().unwrap(), 7 | (100 << 3));

    let mut invalid = bytes.clone();
    invalid[0] = 0x60;
    assert!(DocumentProperties::from_bytes(&invalid).is_err());
    invalid = bytes.clone();
    invalid[2..4].copy_from_slice(&3_u16.to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid).is_err());
    invalid = bytes.clone();
    invalid[52..54].copy_from_slice(&3_u16.to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid).is_err());
    invalid = bytes.clone();
    invalid[54..56].copy_from_slice(&0x4000_u16.to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid).is_err());
    invalid = bytes;
    invalid[82..84].copy_from_slice(&(6_u16 | (5_u16 << 3)).to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid).is_err());

    let mut invalid_writer = properties;
    invalid_writer.word97.base.document_flags.unused5_to_10 = 0x40;
    assert!(invalid_writer.to_bytes().is_err());
    invalid_writer.word97.base.document_flags.unused5_to_10 = 0;
    invalid_writer.word97.base.document_flags.revision_marking = false;
    assert!(invalid_writer.to_bytes().is_err());
    invalid_writer.word97.base.document_flags.revision_marking = true;
    invalid_writer.word97.base.document_flags.lock_annotations = true;
    assert!(invalid_writer.to_bytes().is_err());
    invalid_writer.word97.base.document_flags.lock_annotations = false;
    invalid_writer.word97.base.saved_view.zoom_percentage = 9;
    assert!(invalid_writer.to_bytes().is_err());
  }

  #[test]
  fn document_97_packed_fields_round_trip_display_events_and_virus_info() {
    let mut properties = DocumentProperties::from_bytes(&vec![0; 500]).unwrap();
    properties.word97.display_flags = DocumentDisplayFlags::from_bits(u16::MAX).unwrap();
    properties.word97.version_flags = DocumentVersionFlags::from_bits(0xa5a5);
    properties.word97.document_events = DocumentEvents::from_bits(0x0000_7f3f).unwrap();
    properties.word97.virus_info = VirusSessionInfo::from_bits(u32::MAX);

    let bytes = properties.to_bytes().unwrap();
    assert_eq!(&bytes[410..412], &u16::MAX.to_le_bytes());
    assert_eq!(&bytes[412..414], &0xa5a5_u16.to_le_bytes());
    assert_eq!(&bytes[434..438], &0x0000_7f3f_u32.to_le_bytes());
    assert_eq!(&bytes[438..442], &u32::MAX.to_le_bytes());
    let reparsed = DocumentProperties::from_bytes(&bytes).unwrap();
    assert_eq!(reparsed, properties);
    assert_eq!(
      reparsed.word97.display_flags.outline_level,
      SavedOutlineLevel::All15
    );
    assert_eq!(reparsed.word97.virus_info.session_key, 0x3fff_ffff);

    let mut invalid_display = bytes.clone();
    invalid_display[410..412].copy_from_slice(&(10_u16 << 1).to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid_display).is_err());
    let mut invalid_events = bytes;
    invalid_events[434..438].copy_from_slice(&(1_u32 << 6).to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid_events).is_err());

    let mut invalid_writer = properties;
    invalid_writer.word97.virus_info.session_key = 0x4000_0000;
    assert!(invalid_writer.to_bytes().is_err());
  }

  #[test]
  fn document_drawing_grid_round_trips_static_distances_frequencies_and_flags() {
    let grid = DocumentDrawingGrid {
      horizontal_origin: 1_701,
      vertical_origin: 1_984,
      horizontal_spacing: 180,
      vertical_spacing: 180,
      vertical_display_frequency: GridDisplayFrequency::Every(3),
      unused: true,
      horizontal_display_frequency: GridDisplayFrequency::Every(127),
      follow_margins: true,
    };
    let bytes = grid.to_bytes().unwrap();
    assert_eq!(bytes.len(), 10);
    assert_eq!(bytes[8], 0x83);
    assert_eq!(bytes[9], 0xff);
    assert_eq!(DocumentDrawingGrid::from_bytes(&bytes).unwrap(), grid);

    let disabled = DocumentDrawingGrid::from_bytes(&[0; 10]).unwrap();
    assert_eq!(
      disabled.vertical_display_frequency,
      GridDisplayFrequency::DisabledCompatibility
    );
    assert_eq!(disabled.to_bytes().unwrap(), [0; 10]);
    assert!(DocumentDrawingGrid::from_bytes(&[0; 9]).is_err());

    let mut invalid = grid;
    invalid.horizontal_origin = 31_681;
    assert!(invalid.to_bytes().is_err());
    invalid.horizontal_origin = 0;
    invalid.vertical_display_frequency = GridDisplayFrequency::Every(0);
    assert!(invalid.to_bytes().is_err());
    invalid.vertical_display_frequency = GridDisplayFrequency::Every(128);
    assert!(invalid.to_bytes().is_err());
  }

  #[test]
  fn document_2000_extension_round_trips_static_flags_and_copts() {
    let mut physical = vec![0; 544];
    physical[500] = 9;
    physical[501] = 8;
    physical[502..504].copy_from_slice(&0x1234_u16.to_le_bytes());
    let web_flags =
      0x0000_000f | 0x0000_0f00 | (10_u32 << 12) | (3_u32 << 16) | (480_u32 << 18) | 0xf000_0000;
    physical[504..508].copy_from_slice(&web_flags.to_le_bytes());
    physical[512..516].copy_from_slice(&u32::MAX.to_le_bytes());
    physical[516..520].copy_from_slice(&u32::MAX.to_le_bytes());
    for (index, value) in [1_u32, 2, 3, 4, 5].into_iter().enumerate() {
      let offset = 520 + index * 4;
      physical[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    physical[540..542].copy_from_slice(&u16::MAX.to_le_bytes());
    physical[542..544].copy_from_slice(&0xfedf_u16.to_le_bytes());

    let properties = DocumentProperties::from_bytes(&physical).unwrap();
    let word2000 = properties.extension.word2000().unwrap();
    assert_eq!(word2000.last_bullet_level, 9);
    assert_eq!(word2000.last_numbering_level, 8);
    assert_eq!(
      word2000.flags.target_screen_size,
      WebTargetScreenSize::Size1920x1200
    );
    assert_eq!(word2000.flags.pixels_per_inch, 480);
    assert_eq!(word2000.compatibility_options.named_bits(), u32::MAX);
    assert!(word2000.compatibility_options.cached_column_balance);
    assert_eq!(word2000.compatibility_options.empty1, 0x7fff_ffff);
    word2000
      .validate_compatibility_options(&properties.word97)
      .unwrap();
    assert_eq!(properties.to_bytes().unwrap(), physical);

    let mut compatibility_screen = vec![0; 544];
    compatibility_screen[504..508].copy_from_slice(&(15_u32 << 12).to_le_bytes());
    let compatibility = DocumentProperties::from_bytes(&compatibility_screen).unwrap();
    assert_eq!(
      compatibility
        .extension
        .word2000()
        .unwrap()
        .flags
        .target_screen_size,
      WebTargetScreenSize::Compatibility15
    );
    assert_eq!(compatibility.to_bytes().unwrap(), compatibility_screen);

    let mut invalid = vec![0; 544];
    invalid[500] = 10;
    assert!(DocumentProperties::from_bytes(&invalid).is_err());
    invalid[500] = 0;
    invalid[504..508].copy_from_slice(&(1_u32 << 4).to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid).is_err());
    invalid[504..508].copy_from_slice(&((1_u32 << 28) | (15_u32 << 12)).to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid).is_err());
    invalid[504..508].copy_from_slice(&0_u32.to_le_bytes());
    invalid[542..544].copy_from_slice(&(1_u16 << 5).to_le_bytes());
    assert!(DocumentProperties::from_bytes(&invalid).is_err());

    let mut invalid_writer = properties.clone();
    invalid_writer
      .extension
      .word2000_mut()
      .unwrap()
      .compatibility_options
      .empty1 = 0x8000_0000;
    assert!(invalid_writer.to_bytes().is_err());

    let mut mismatched = properties;
    mismatched
      .extension
      .word2000_mut()
      .unwrap()
      .compatibility_options
      .word8 = CompatibilityOptions80::from_bits(1);
    let word2000 = *mismatched.extension.word2000().unwrap();
    assert!(!word2000.compatibility_options_match(&mismatched.word97));
    assert!(
      word2000
        .validate_compatibility_options(&mismatched.word97)
        .is_err()
    );
    assert_eq!(mismatched.to_bytes().unwrap()[508] & 1, 1);
  }

  #[test]
  fn font_table_round_trips_static_font_metadata_and_names() {
    let table = FontTable {
      fonts: vec![FontFamilyName {
        family: FontFamilyIdentifier {
          pitch: 2,
          true_type: true,
          unused1: false,
          family: 2,
          unused2: false,
        },
        weight: 400,
        character_set: 0,
        alternate_name_index: 6,
        panose: Panose {
          family_type: 2,
          serif_style: 11,
          weight: 6,
          proportion: 4,
          contrast: 2,
          stroke_variation: 2,
          arm_style: 2,
          letterform: 2,
          midline: 2,
          height: 4,
        },
        signature: FontSignature {
          unicode_subsets: [1, 2, 3, 4],
          code_pages: [5, 6],
        },
        name_units: "Arial\0Helvetica\0".encode_utf16().collect(),
        trailing_name_nulls: 0,
      }],
    };
    let bytes = table.to_bytes().unwrap();
    let reparsed = FontTable::from_bytes(&bytes).unwrap();
    assert_eq!(reparsed, table);
    assert_eq!(
      reparsed.fonts[0].primary_name(),
      "Arial".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
      reparsed.fonts[0].alternate_name(),
      Some("Helvetica".encode_utf16().collect::<Vec<_>>().as_slice())
    );
  }

  #[test]
  fn associated_strings_round_trip_named_document_metadata() {
    let strings = AssociatedStrings {
      unused0: vec![],
      template_path: "Normal.dot".encode_utf16().collect(),
      title: "Title".encode_utf16().collect(),
      subject: "Subject".encode_utf16().collect(),
      keywords: "rust,office".encode_utf16().collect(),
      unused5: vec![],
      author: "Author".encode_utf16().collect(),
      last_revised_by: "Editor".encode_utf16().collect(),
      mail_merge_data_source: vec![],
      mail_merge_header: vec![],
      unused10: vec![],
      unused11: vec![],
      unused12: vec![],
      unused13: vec![],
      unused14: vec![],
      unused15: vec![],
      unused16: vec![],
      write_reservation_password: "secret".encode_utf16().collect(),
      trailing_zero_words: 0,
    };
    let bytes = strings.to_bytes().unwrap();
    assert_eq!(AssociatedStrings::from_bytes(&bytes).unwrap(), strings);
  }

  #[test]
  fn user_variables_round_trip_parallel_names_and_values() {
    let variables = UserVariables {
      variables: vec![
        UserVariable {
          name: "ProjectName".encode_utf16().collect(),
          ignored_name_metadata: 0x1234_5678,
          value: "olecfsdk".encode_utf16().collect(),
        },
        UserVariable {
          name: "Sign".encode_utf16().collect(),
          ignored_name_metadata: 0,
          value: vec![0x1234, 0xabcd],
        },
        UserVariable {
          name: "SigV3".encode_utf16().collect(),
          ignored_name_metadata: 7,
          value: vec![1, 2, 3],
        },
      ],
    };
    let bytes = variables.to_bytes().unwrap();
    assert_eq!(UserVariables::from_bytes(&bytes).unwrap(), variables);
    assert_eq!(variables.variables[0].kind(), UserVariableKind::Ordinary);
    assert_eq!(
      variables.variables[1].kind(),
      UserVariableKind::LegacyVbaSignature
    );
    assert_eq!(
      variables.variables[2].kind(),
      UserVariableKind::VbaSignatureV3
    );
    let mut unsigned = variables.clone();
    assert_eq!(unsigned.remove_vba_signatures(), 2);
    assert_eq!(unsigned.variables, variables.variables[..1]);
    assert_eq!(
      UserVariables::from_bytes(&unsigned.to_bytes().unwrap()).unwrap(),
      unsigned
    );

    let duplicate = UserVariables {
      variables: vec![
        variables.variables[0].clone(),
        variables.variables[0].clone(),
      ],
    };
    assert!(duplicate.to_bytes().is_err());

    let mut trailing = bytes;
    trailing.push(0);
    assert!(UserVariables::from_bytes(&trailing).is_err());
  }

  #[test]
  fn mail_merge_state_round_trips_conditional_pms_sections() {
    let state = MailMergeState {
      status: MailMergeStatus {
        main_document_selected: true,
        data_source_selected: true,
        header_source_selected: false,
        document_type: MailMergeDocumentType::Letters,
        ignored1: true,
        automatic: false,
        suppress_blank_lines: true,
        record_selection: false,
        destination: MailMergeDestination::Email,
      },
      header_source_index: 0,
      fetch_source_index: 1,
      current_record: None,
      sources: [
        MailMergeSource {
          kind: MailMergeSourceKind::DataFile,
          link_to_filename: true,
          link_to_connection: false,
          no_prompt_query_tool: false,
          query: true,
          ignored_flags: 2,
          field_separator: MailMergeSeparator::Token(MailMergeToken::Tab),
          record_separator: MailMergeSeparator::Token(MailMergeToken::Enter),
          file: MailMergeFileReference::Identifier(7),
        },
        MailMergeSource {
          kind: MailMergeSourceKind::Odbc,
          link_to_filename: false,
          link_to_connection: true,
          no_prompt_query_tool: true,
          query: true,
          ignored_flags: 1,
          field_separator: MailMergeSeparator::Ignored(-1),
          record_separator: MailMergeSeparator::Ignored(123),
          file: MailMergeFileReference::Identifier(9),
        },
      ],
      filter: MailMergeFilter {
        show_data: true,
        error_handling: MailMergeErrorHandling::CompleteAndReport,
        main_document_setup: true,
        mail_as_text: false,
        ignored1: true,
        default_sql: false,
        mail_as_html: true,
        ignored2: 0x5a,
        string_table_handle: 4,
      },
      sql_query: Some("SELECT * FROM data".encode_utf16().collect()),
      strings: Some(MailMergeStrings {
        connection: "connection".encode_utf16().collect(),
        header_connection: vec![],
        subject: "subject".encode_utf16().collect(),
        recipient_column: "email".encode_utf16().collect(),
        ignored: Some("ignored".encode_utf16().collect()),
      }),
      document_type: Some(MailMergeDocumentTypeInfo {
        document_type: MailMergeDocumentType::Email,
        ignored: 7,
      }),
    };
    let bytes = state.to_bytes().unwrap();
    assert_eq!(MailMergeState::from_bytes(&bytes).unwrap(), state);
    let referenced_files = ExternalFileNameTable {
      files: [7, 9]
        .into_iter()
        .map(|identifier| ExternalFileName {
          path: format!(r"C:\data\source{identifier}.csv")
            .encode_utf16()
            .collect(),
          file_type: ExternalFileType::MailMergeDataSource,
          identifier,
          relative_path: ExternalRelativePath::None,
          file_systems: ExternalFileSystems {
            fat: true,
            ignored1: false,
            ignored2: false,
            ntfs: true,
            non_file_system: false,
            ignored3: 0,
            ignored4: false,
          },
          ignored: 0,
        })
        .collect(),
    };
    state.validate_file_references(&referenced_files).unwrap();
    let mut missing_file = referenced_files;
    missing_file.files.pop();
    assert!(state.validate_file_references(&missing_file).is_err());

    let compatibility_source = MailMergeSource {
      kind: MailMergeSourceKind::DataFile,
      link_to_filename: false,
      link_to_connection: false,
      no_prompt_query_tool: false,
      query: false,
      ignored_flags: 0,
      field_separator: MailMergeSeparator::Token(MailMergeToken::None),
      record_separator: MailMergeSeparator::Token(MailMergeToken::None),
      file: MailMergeFileReference::NilCompatibility,
    };
    let compatibility = MailMergeState {
      status: MailMergeStatus {
        main_document_selected: true,
        data_source_selected: false,
        header_source_selected: false,
        document_type: MailMergeDocumentType::Letters,
        ignored1: false,
        automatic: false,
        suppress_blank_lines: true,
        record_selection: false,
        destination: MailMergeDestination::None,
      },
      header_source_index: 0,
      fetch_source_index: 0,
      current_record: Some(1),
      sources: [compatibility_source; 2],
      filter: MailMergeFilter {
        show_data: false,
        error_handling: MailMergeErrorHandling::CompleteAndPause,
        main_document_setup: false,
        mail_as_text: false,
        ignored1: false,
        default_sql: false,
        mail_as_html: true,
        ignored2: 0,
        string_table_handle: 0,
      },
      sql_query: Some("SELECT * FROM source".encode_utf16().collect()),
      strings: None,
      document_type: Some(MailMergeDocumentTypeInfo {
        document_type: MailMergeDocumentType::Letters,
        ignored: 0,
      }),
    };
    let bytes = compatibility.to_bytes().unwrap();
    assert_eq!(MailMergeState::from_bytes(&bytes).unwrap(), compatibility);

    let mut invalid = compatibility;
    invalid.filter.string_table_handle = 1;
    assert!(invalid.to_bytes().is_err());
  }

  #[test]
  fn odso_filter_and_sort_properties_round_trip_static_items() {
    let filter = RecipientFilter {
      items: vec![
        RecipientFilterItem {
          column: 3,
          operator: FilterComparison::Equal,
          condition: FilterCondition::And,
          value: "London".encode_utf16().collect(),
        },
        RecipientFilterItem {
          column: 254,
          operator: FilterComparison::NotEmpty,
          condition: FilterCondition::Or,
          value: vec![],
        },
      ],
    };
    let filter_bytes = filter.to_bytes().unwrap();
    assert_eq!(RecipientFilter::from_bytes(&filter_bytes).unwrap(), filter);

    let sort = RecipientSort {
      columns: vec![
        SortColumn {
          column: 1,
          direction: SortDirection::Ascending,
        },
        SortColumn {
          column: 254,
          direction: SortDirection::Descending,
        },
      ],
    };
    let sort_bytes = sort.to_bytes().unwrap();
    assert_eq!(RecipientSort::from_bytes(&sort_bytes).unwrap(), sort);

    let mut invalid_length = filter_bytes.clone();
    invalid_length[0] = 17;
    assert!(RecipientFilter::from_bytes(&invalid_length).is_err());

    let mut invalid_terminator = filter_bytes;
    let first_length = 16 + ("London".encode_utf16().count() + 1) * 2;
    invalid_terminator[first_length - 1] = 1;
    assert!(RecipientFilter::from_bytes(&invalid_terminator).is_err());

    let mut too_many_sorts = sort;
    too_many_sorts.columns.extend([
      SortColumn {
        column: 2,
        direction: SortDirection::Ascending,
      },
      SortColumn {
        column: 3,
        direction: SortDirection::Ascending,
      },
    ]);
    assert!(too_many_sorts.to_bytes().is_err());

    let mut invalid_direction = sort_bytes;
    invalid_direction[4] = 2;
    assert!(RecipientSort::from_bytes(&invalid_direction).is_err());
  }

  #[test]
  fn odso_recipient_info_round_trips_marker_lists_and_large_framing() {
    let info = RecipientInfo {
      recipients: vec![
        Recipient {
          items: vec![
            RecipientData::Included(false),
            RecipientData::Hash(0x1234_5678),
          ],
        },
        Recipient {
          items: vec![
            RecipientData::UniqueColumn(7),
            RecipientData::UniqueValue("alice@example.test".encode_utf16().collect()),
            RecipientData::Included(true),
          ],
        },
      ],
    };
    let bytes = info.to_bytes().unwrap();
    assert_eq!(RecipientInfo::from_bytes(&bytes).unwrap(), info);

    let large = RecipientInfo {
      recipients: (0..6_000)
        .map(|hash| Recipient {
          items: vec![RecipientData::Hash(hash)],
        })
        .collect(),
    };
    let large_bytes = large.to_bytes().unwrap();
    assert_eq!(
      u16::from_le_bytes([large_bytes[10], large_bytes[11]]),
      u16::MAX
    );
    assert_eq!(RecipientInfo::from_bytes(&large_bytes).unwrap(), large);

    let missing_identity = RecipientInfo {
      recipients: vec![Recipient {
        items: vec![RecipientData::Included(true)],
      }],
    };
    assert!(missing_identity.to_bytes().is_err());

    let mut invalid_status = bytes.clone();
    invalid_status[16] = 2;
    assert!(RecipientInfo::from_bytes(&invalid_status).is_err());

    let mut invalid_terminator = bytes;
    let first_terminator = 12 + 8 + 8;
    invalid_terminator[first_terminator + 2] = 1;
    assert!(RecipientInfo::from_bytes(&invalid_terminator).is_err());
  }

  #[test]
  fn odso_field_map_info_round_trips_thirty_marker_lists() {
    let mut fields = vec![FieldMap { items: vec![] }; 30];
    fields[0].items = vec![
      FieldMapData::Mapped,
      FieldMapData::DataSourceColumnName("CustomerId".encode_utf16().collect()),
      FieldMapData::StandardFieldName("Unique Identifier".encode_utf16().collect()),
      FieldMapData::ColumnIndex(Some(4)),
    ];
    fields[1].items = vec![FieldMapData::ColumnIndex(None)];
    let info = FieldMapInfo { fields };
    let bytes = info.to_bytes().unwrap();
    assert_eq!(FieldMapInfo::from_bytes(&bytes).unwrap(), info);

    let large = FieldMapInfo {
      fields: (0..30)
        .map(|_| FieldMap {
          items: vec![FieldMapData::DataSourceColumnName(vec![0x61; 1_100])],
        })
        .collect(),
    };
    let large_bytes = large.to_bytes().unwrap();
    assert_eq!(
      u16::from_le_bytes([large_bytes[10], large_bytes[11]]),
      u16::MAX
    );
    assert_eq!(FieldMapInfo::from_bytes(&large_bytes).unwrap(), large);

    let mut missing = info.clone();
    missing.fields.pop();
    assert!(missing.to_bytes().is_err());

    let mut invalid_mapped = bytes.clone();
    invalid_mapped[16] = 2;
    assert!(FieldMapInfo::from_bytes(&invalid_mapped).is_err());

    let mut invalid_terminator = bytes;
    let first_field_length = 8
      + 4
      + "CustomerId".encode_utf16().count() * 2
      + 4
      + "Unique Identifier".encode_utf16().count() * 2
      + 8;
    invalid_terminator[12 + first_field_length + 2] = 1;
    assert!(FieldMapInfo::from_bytes(&invalid_terminator).is_err());
  }

  #[test]
  fn office_data_source_round_trips_all_static_property_variants() {
    let source = OfficeDataSource {
      properties: vec![
        OfficeDataSourceProperty::ConnectionString("Provider=Example".encode_utf16().collect()),
        OfficeDataSourceProperty::DataSet("Customers".encode_utf16().collect()),
        OfficeDataSourceProperty::FileName("customers.csv".encode_utf16().collect()),
        OfficeDataSourceProperty::ConnectionType(3),
        OfficeDataSourceProperty::ColumnDelimiter(u16::from(b',')),
        OfficeDataSourceProperty::FirstRowIsHeader(true),
        OfficeDataSourceProperty::Filter(RecipientFilter {
          items: vec![RecipientFilterItem {
            column: 2,
            operator: FilterComparison::NotEqual,
            condition: FilterCondition::And,
            value: "inactive".encode_utf16().collect(),
          }],
        }),
        OfficeDataSourceProperty::Sort(RecipientSort {
          columns: vec![SortColumn {
            column: 1,
            direction: SortDirection::Ascending,
          }],
        }),
        OfficeDataSourceProperty::Recipients(RecipientInfo {
          recipients: vec![Recipient {
            items: vec![RecipientData::Hash(0x1234_5678)],
          }],
        }),
        OfficeDataSourceProperty::FieldMap(FieldMapInfo {
          fields: vec![FieldMap { items: vec![] }; 30],
        }),
        OfficeDataSourceProperty::WizardStep(4),
      ],
    };
    let bytes = source.to_bytes().unwrap();
    assert_eq!(OfficeDataSource::from_bytes(&bytes).unwrap(), source);

    let large = OfficeDataSource {
      properties: vec![OfficeDataSourceProperty::FileName(vec![0x61; 40_000])],
    };
    let large_bytes = large.to_bytes().unwrap();
    assert_eq!(
      u16::from_le_bytes([large_bytes[2], large_bytes[3]]),
      u16::MAX
    );
    assert_eq!(OfficeDataSource::from_bytes(&large_bytes).unwrap(), large);

    let duplicate = OfficeDataSource {
      properties: vec![
        OfficeDataSourceProperty::WizardStep(1),
        OfficeDataSourceProperty::WizardStep(2),
      ],
    };
    assert!(duplicate.to_bytes().is_err());

    let mut invalid_id = bytes;
    invalid_id[0] = 3;
    assert!(OfficeDataSource::from_bytes(&invalid_id).is_err());

    let invalid_step = OfficeDataSource {
      properties: vec![OfficeDataSourceProperty::WizardStep(7)],
    };
    assert!(invalid_step.to_bytes().is_err());
  }

  #[test]
  fn subdocument_table_round_trips_static_wkb_records() {
    let table = SubdocumentTable {
      positions: vec![0, 10, 42],
      subdocuments: vec![
        SubdocumentReference {
          ignored_flag3: false,
          ignored_flag8: true,
          file_identifier: 7,
        },
        SubdocumentReference {
          ignored_flag3: true,
          ignored_flag8: false,
          file_identifier: 11,
        },
      ],
    };
    let bytes = table.to_bytes().unwrap();
    assert_eq!(SubdocumentTable::from_bytes(&bytes).unwrap(), table);
    table.validate_main_document_length(40).unwrap();
    let files = ExternalFileNameTable {
      files: [7, 11]
        .into_iter()
        .map(|identifier| ExternalFileName {
          path: format!(r"C:\master\part{identifier}.doc")
            .encode_utf16()
            .collect(),
          file_type: ExternalFileType::Subdocument,
          identifier,
          relative_path: ExternalRelativePath::Offset(10),
          file_systems: ExternalFileSystems {
            fat: false,
            ignored1: false,
            ignored2: false,
            ntfs: true,
            non_file_system: false,
            ignored3: 0,
            ignored4: false,
          },
          ignored: 0,
        })
        .collect(),
    };
    table.validate_file_references(&files).unwrap();

    let mut invalid_flags = bytes;
    let flags_offset = table.positions.len() * 4 + 2;
    invalid_flags[flags_offset] &= !0x20;
    assert!(SubdocumentTable::from_bytes(&invalid_flags).is_err());
    assert!(SubdocumentTable::from_bytes(&[0; 66]).is_err());

    let mut invalid_id = table;
    invalid_id.subdocuments[0].file_identifier = 0x0fff;
    assert!(invalid_id.to_bytes().is_err());
  }

  #[test]
  fn external_file_name_table_round_trips_fnif_and_fnpi() {
    let table = ExternalFileNameTable {
      files: vec![
        ExternalFileName {
          path: r"C:\merge\data.csv".encode_utf16().collect(),
          file_type: ExternalFileType::MailMergeDataSource,
          identifier: 7,
          relative_path: ExternalRelativePath::Offset(9),
          file_systems: ExternalFileSystems {
            fat: true,
            ignored1: true,
            ignored2: false,
            ntfs: true,
            non_file_system: false,
            ignored3: 2,
            ignored4: true,
          },
          ignored: 0x1234_5678,
        },
        ExternalFileName {
          path: "https://example.test/part.doc".encode_utf16().collect(),
          file_type: ExternalFileType::Subdocument,
          identifier: 7,
          relative_path: ExternalRelativePath::None,
          file_systems: ExternalFileSystems {
            fat: false,
            ignored1: false,
            ignored2: true,
            ntfs: false,
            non_file_system: true,
            ignored3: 1,
            ignored4: false,
          },
          ignored: u32::MAX,
        },
      ],
    };
    let bytes = table.to_bytes().unwrap();
    assert_eq!(ExternalFileNameTable::from_bytes(&bytes).unwrap(), table);

    let mut duplicate = table.clone();
    duplicate.files.push(duplicate.files[0].clone());
    assert!(duplicate.to_bytes().is_err());

    let mut invalid_id = table.clone();
    invalid_id.files[0].identifier = 0x0fff;
    assert!(invalid_id.to_bytes().is_err());

    let mut invalid_relative = table.clone();
    invalid_relative.files[0].relative_path = ExternalRelativePath::Offset(0xff);
    assert!(invalid_relative.to_bytes().is_err());

    let mut invalid_systems = table;
    invalid_systems.files[1].file_systems.fat = true;
    assert!(invalid_systems.to_bytes().is_err());

    let mut invalid_type = bytes;
    let first_fnpi = 6 + 2 + r"C:\merge\data.csv".encode_utf16().count() * 2;
    invalid_type[first_fnpi] = (invalid_type[first_fnpi] & 0xf0) | 4;
    assert!(ExternalFileNameTable::from_bytes(&invalid_type).is_err());
  }

  #[test]
  fn xml_schema_references_round_trip_xsdr_string_tables() {
    let references = XmlSchemaReferences {
      schemas: vec![
        XmlSchemaReference {
          uri: "urn:example:invoice".encode_utf16().collect(),
          manifest_location: "https://example.test/manifest.xml".encode_utf16().collect(),
          elements: XmlSchemaStringTable::Utf16(vec![
            "invoice".encode_utf16().collect(),
            "line".encode_utf16().collect(),
          ]),
          attributes: XmlSchemaStringTable::Utf16(vec!["currency".encode_utf16().collect()]),
        },
        XmlSchemaReference {
          uri: "urn:example:legacy".encode_utf16().collect(),
          manifest_location: vec![],
          elements: XmlSchemaStringTable::Ansi(vec![b"root".to_vec(), b"item".to_vec()]),
          attributes: XmlSchemaStringTable::Ansi(vec![b"id".to_vec()]),
        },
      ],
    };
    let bytes = references.to_bytes().unwrap();
    assert_eq!(XmlSchemaReferences::from_bytes(&bytes).unwrap(), references);

    let empty = XmlSchemaReferences { schemas: vec![] };
    assert_eq!(empty.to_bytes().unwrap(), [0; 4]);
    assert_eq!(XmlSchemaReferences::from_bytes(&[0; 4]).unwrap(), empty);

    let mut empty_table = references.clone();
    empty_table.schemas[0].attributes = XmlSchemaStringTable::Utf16(vec![]);
    assert!(empty_table.to_bytes().is_err());

    let mut oversized_ansi = references.clone();
    oversized_ansi.schemas[1].attributes = XmlSchemaStringTable::Ansi(vec![vec![0; 256]]);
    assert!(oversized_ansi.to_bytes().is_err());

    assert!(XmlSchemaReferences::from_bytes(&(-1i32).to_le_bytes()).is_err());
    let mut trailing = bytes;
    trailing.push(0);
    assert!(XmlSchemaReferences::from_bytes(&trailing).is_err());
  }

  #[test]
  fn xml_transform_path_round_trips_bounded_utf16_array() {
    let transform = XmlTransformPath {
      path: r"C:\Transforms\document.xsl".encode_utf16().collect(),
    };
    let bytes = transform.to_bytes().unwrap();
    assert_eq!(XmlTransformPath::from_bytes(&bytes).unwrap(), transform);

    assert!(XmlTransformPath::from_bytes(&[]).is_err());
    assert!(XmlTransformPath::from_bytes(&[0]).is_err());
    assert!(XmlTransformPath::from_bytes(&vec![0; 4170]).is_err());
    assert!(XmlTransformPath { path: vec![] }.to_bytes().is_err());
    assert!(
      XmlTransformPath {
        path: vec![0; 2085]
      }
      .to_bytes()
      .is_err()
    );
  }

  #[test]
  fn range_protection_round_trips_prti_users_and_bookmarks() {
    let protection = RangeProtection {
      permissions: vec![
        RangePermission {
          editors: PermittedEditors::UserIndex(1),
          ignored_index: 0x1234,
          ignored_use: 0x5678,
        },
        RangePermission {
          editors: PermittedEditors::Editors,
          ignored_index: 0,
          ignored_use: 1,
        },
        RangePermission {
          editors: PermittedEditors::Everyone,
          ignored_index: u16::MAX,
          ignored_use: 2,
        },
      ],
      starts: BookmarkStartTable {
        positions: vec![0, 10, 20, 30],
        bookmarks: vec![
          BookmarkStart {
            end_index: 0,
            column_start: 0,
            published: false,
            column_limit: 0,
            native: true,
            column: false,
          },
          BookmarkStart {
            end_index: 1,
            column_start: 1,
            published: true,
            column_limit: 2,
            native: false,
            column: true,
          },
          BookmarkStart {
            end_index: 2,
            column_start: 0,
            published: false,
            column_limit: 0,
            native: false,
            column: false,
          },
        ],
      },
      ends: BookmarkEndTable {
        positions: vec![5, 15, 25, 30],
      },
      users: ProtectedUsers {
        users: vec![
          ProtectedUser {
            name: r"DOMAIN\alice".encode_utf16().collect(),
            role: ProtectedUserRole::Owner,
          },
          ProtectedUser {
            name: "editor@example.test".encode_utf16().collect(),
            role: ProtectedUserRole::Editor,
          },
        ],
      },
    };
    let bytes = protection.to_bytes().unwrap();
    assert_eq!(
      RangeProtection::from_bytes(&bytes.permissions, &bytes.starts, &bytes.ends, &bytes.users,)
        .unwrap(),
      protection
    );

    let mut missing_user = protection.clone();
    missing_user.permissions[0].editors = PermittedEditors::UserIndex(3);
    assert!(missing_user.to_bytes().is_err());

    let mut duplicate_user = protection.clone();
    duplicate_user
      .users
      .users
      .push(duplicate_user.users.users[0].clone());
    assert!(duplicate_user.to_bytes().is_err());

    let mut bad_end_index = protection.clone();
    bad_end_index.starts.bookmarks[0].end_index = 3;
    assert!(bad_end_index.to_bytes().is_err());

    let mut invalid_cch = bytes.permissions.clone();
    invalid_cch[8] = 1;
    assert!(
      RangeProtection::from_bytes(&invalid_cch, &bytes.starts, &bytes.ends, &bytes.users).is_err()
    );

    let mut invalid_protection_type = bytes.permissions;
    invalid_protection_type[12] = 2;
    assert!(
      RangeProtection::from_bytes(
        &invalid_protection_type,
        &bytes.starts,
        &bytes.ends,
        &bytes.users,
      )
      .is_err()
    );
  }

  #[test]
  fn structured_tag_bookmarks_round_trip_sdti_and_fsdap() {
    let bookmarks = StructuredTagBookmarks {
      tags: vec![
        StructuredTagInfo {
          id: 10,
          name: TagQualifiedName {
            schema_index: 0,
            name_index: 0,
          },
          tag_type: StructuredTagType::Characters,
          attributes: vec![StructuredTagAttribute {
            name: TagQualifiedName {
              schema_index: 0,
              name_index: 1,
            },
            value: "blue".encode_utf16().collect(),
          }],
          placeholder: "enter value".encode_utf16().collect(),
        },
        StructuredTagInfo {
          id: 11,
          name: TagQualifiedName {
            schema_index: 0,
            name_index: 1,
          },
          tag_type: StructuredTagType::TableRows,
          attributes: vec![],
          placeholder: vec![],
        },
      ],
      starts: BookmarkStartTable {
        positions: vec![0, 10, 20],
        bookmarks: vec![
          BookmarkStart {
            end_index: 0,
            column_start: 0,
            published: false,
            column_limit: 0,
            native: false,
            column: false,
          },
          BookmarkStart {
            end_index: 1,
            column_start: 2,
            published: true,
            column_limit: 4,
            native: true,
            column: true,
          },
        ],
      },
      ends: BookmarkEndTable {
        positions: vec![5, 15, 20],
      },
    };
    let bytes = bookmarks.to_bytes().unwrap();
    assert_eq!(
      StructuredTagBookmarks::from_bytes(&bytes.tags, &bytes.starts, &bytes.ends).unwrap(),
      bookmarks
    );

    let schemas = XmlSchemaReferences {
      schemas: vec![XmlSchemaReference {
        uri: vec![],
        manifest_location: vec![],
        elements: XmlSchemaStringTable::Utf16(vec![
          "color".encode_utf16().collect(),
          "shade".encode_utf16().collect(),
        ]),
        attributes: XmlSchemaStringTable::Utf16(vec![
          "text".encode_utf16().collect(),
          "row".encode_utf16().collect(),
        ]),
      }],
    };
    bookmarks.validate_schema_references(&schemas).unwrap();

    let mut duplicate_id = bookmarks.clone();
    duplicate_id.tags[1].id = 10;
    assert!(duplicate_id.to_bytes().is_err());

    let mut bad_schema = bookmarks.clone();
    bad_schema.tags[0].name.schema_index = 1;
    assert!(bad_schema.validate_schema_references(&schemas).is_err());

    let mut bad_cch = bytes.tags.clone();
    bad_cch[8] = 11;
    assert!(StructuredTagBookmarks::from_bytes(&bad_cch, &bytes.starts, &bytes.ends).is_err());

    let mut bad_placeholder_terminator = bytes.tags;
    let last = bad_placeholder_terminator.len() - 1;
    bad_placeholder_terminator[last] = 1;
    assert!(
      StructuredTagBookmarks::from_bytes(&bad_placeholder_terminator, &bytes.starts, &bytes.ends,)
        .is_err()
    );
  }

  #[test]
  fn consistency_and_repair_bookmarks_round_trip_specialized_sttbs() {
    let starts = BookmarkStartTable {
      positions: vec![0, 10, 20],
      bookmarks: vec![
        BookmarkStart {
          end_index: 0,
          column_start: 0,
          published: false,
          column_limit: 0,
          native: false,
          column: false,
        },
        BookmarkStart {
          end_index: 1,
          column_start: 1,
          published: true,
          column_limit: 2,
          native: true,
          column: true,
        },
      ],
    };
    let ends = BookmarkEndTable {
      positions: vec![5, 15, 20],
    };
    let consistency = FormatConsistencyBookmarks {
      records: vec![
        FormatConsistencyBookmark {
          padding1: 0x1234,
          squiggle: false,
          ignored: true,
          squiggle_changed: true,
          kind: FormatConsistencyKind::CharacterFormatting,
          ignored_data: 0x5678_9abc,
          properties: FormatConsistencyProperties {
            character: true,
            table: false,
            line_separation: true,
          },
          id: 7,
          padding2: 0xde,
        },
        FormatConsistencyBookmark {
          padding1: 0,
          squiggle: false,
          ignored: true,
          squiggle_changed: true,
          kind: FormatConsistencyKind::ListLevelFormatting,
          ignored_data: 0,
          properties: FormatConsistencyProperties {
            character: false,
            table: true,
            line_separation: false,
          },
          id: 8,
          padding2: 0,
        },
      ],
      starts: starts.clone(),
      ends: ends.clone(),
    };
    let bytes = consistency.to_bytes().unwrap();
    assert_eq!(
      FormatConsistencyBookmarks::from_bytes(&bytes.metadata, &bytes.starts, &bytes.ends,).unwrap(),
      consistency
    );
    consistency.validate_main_document(20).unwrap();

    let repairs = RepairBookmarks {
      descriptions: vec!["repaired paragraph".encode_utf16().collect(), vec![]],
      starts,
      ends,
    };
    let repair_bytes = repairs.to_bytes().unwrap();
    assert_eq!(
      RepairBookmarks::from_bytes(
        &repair_bytes.metadata,
        &repair_bytes.starts,
        &repair_bytes.ends,
      )
      .unwrap(),
      repairs
    );
    repairs.validate_main_document(20).unwrap();

    let mut duplicate = consistency.clone();
    duplicate.records[1].id = 7;
    assert!(duplicate.to_bytes().is_err());

    let mut not_main_document = consistency.clone();
    not_main_document.records[0].squiggle = true;
    assert!(not_main_document.validate_main_document(20).is_err());

    let mut invalid_fcct = bytes.metadata;
    invalid_fcct[6 + 2 + 2 + 4 + 4 + 4] = 2;
    assert!(
      FormatConsistencyBookmarks::from_bytes(&invalid_fcct, &bytes.starts, &bytes.ends,).is_err()
    );

    let mut outside = repairs;
    outside.ends.positions[1] = 21;
    assert!(outside.validate_main_document(20).is_err());
  }

  #[test]
  fn caption_tables_round_trip_capi_and_autocaption_references() {
    let captions = CaptionDefinitions {
      captions: vec![
        CaptionDefinition {
          label: "Figure".encode_utf16().collect(),
          properties: CaptionProperties {
            location: CaptionLocation::Below,
            include_chapter_number: true,
            heading: CaptionHeading::Heading(2),
            ignored: 0x2a,
            no_label: false,
            number_format: NumberingFormat::ARABIC,
            separator: CaptionSeparator::Period,
          },
        },
        CaptionDefinition {
          label: "Table".encode_utf16().collect(),
          properties: CaptionProperties {
            location: CaptionLocation::Above,
            include_chapter_number: false,
            heading: CaptionHeading::Ignored(15),
            ignored: 0,
            no_label: true,
            number_format: NumberingFormat::UPPER_ROMAN,
            separator: CaptionSeparator::Ignored(0xffff),
          },
        },
      ],
    };
    let caption_bytes = captions.to_bytes().unwrap();
    assert_eq!(
      CaptionDefinitions::from_bytes(&caption_bytes).unwrap(),
      captions
    );

    let automatic = AutoCaptionDefinitions {
      entries: vec![
        AutoCaptionDefinition {
          program_id: "Word.Picture".encode_utf16().collect(),
          caption_index: 0,
        },
        AutoCaptionDefinition {
          program_id: "Excel.Sheet".encode_utf16().collect(),
          caption_index: 1,
        },
      ],
    };
    let automatic_bytes = automatic.to_bytes().unwrap();
    assert_eq!(
      AutoCaptionDefinitions::from_bytes(&automatic_bytes).unwrap(),
      automatic
    );
    automatic.validate_against(&captions).unwrap();

    let mut invalid = automatic;
    invalid.entries[1].caption_index = 2;
    assert!(invalid.validate_against(&captions).is_err());

    let mut invalid_extra = automatic_bytes;
    invalid_extra[4..6].copy_from_slice(&0_u16.to_le_bytes());
    assert!(AutoCaptionDefinitions::from_bytes(&invalid_extra).is_err());
    assert!(NumberingFormat::from_u16(0x0100).is_err());
    assert!(NumberingFormat::from_code(0x40).is_err());
  }

  #[test]
  fn embedded_font_table_round_trips_header_and_typed_ttmbd_records() {
    let empty = EmbeddedFontTable {
      producer_offset: EmbeddedFontTableOffset::Word97Compatibility,
      fonts: vec![],
    };
    let bytes = empty.to_bytes().unwrap();
    assert_eq!(bytes, [0, 0, 0, 0, 64, 0, 0, 0, 26, 0]);
    assert_eq!(EmbeddedFontTable::from_bytes(&bytes).unwrap(), empty);

    let table = EmbeddedFontTable {
      producer_offset: EmbeddedFontTableOffset::Standard,
      fonts: vec![
        EmbeddedFontReference {
          word_document_offset: 0x1_0000,
          font_index: 2,
          bold: true,
          italic: false,
          ignored_flags: 0x123,
          subset: EmbeddedFontSubset::UsageOrder(0),
        },
        EmbeddedFontReference {
          word_document_offset: 0x2_0000,
          font_index: 3,
          bold: false,
          italic: true,
          ignored_flags: 0,
          subset: EmbeddedFontSubset::EntireFont,
        },
      ],
    };
    let bytes = table.to_bytes().unwrap();
    assert_eq!(EmbeddedFontTable::from_bytes(&bytes).unwrap(), table);
    table.validate_against_font_table(4).unwrap();

    let mut mismatched_count = bytes;
    mismatched_count[2..4].copy_from_slice(&3_i16.to_le_bytes());
    assert!(EmbeddedFontTable::from_bytes(&mismatched_count).is_err());

    let duplicate_subset = EmbeddedFontTable {
      fonts: vec![
        table.fonts[0],
        EmbeddedFontReference {
          font_index: 1,
          subset: EmbeddedFontSubset::UsageOrder(0),
          ..table.fonts[1]
        },
      ],
      ..table
    };
    assert!(duplicate_subset.validate_against_font_table(4).is_err());
  }

  #[test]
  fn revision_authors_round_trip_extended_utf16_string_table() {
    let authors = RevisionAuthors::Standard {
      names: ["Unknown", "Alice", "编辑者"]
        .into_iter()
        .map(|name| name.encode_utf16().collect())
        .collect(),
    };
    let bytes = authors.to_bytes().unwrap();
    assert_eq!(RevisionAuthors::from_bytes(&bytes).unwrap(), authors);

    let invalid = RevisionAuthors::Standard {
      names: vec!["Anonymous".encode_utf16().collect()],
    };
    assert!(invalid.to_bytes().is_err());
    let placeholder = RevisionAuthors::CompatibilityZeroPlaceholder;
    assert_eq!(
      RevisionAuthors::from_bytes(&placeholder.to_bytes().unwrap()).unwrap(),
      placeholder
    );
  }

  #[test]
  fn spelling_state_table_round_trips_static_spls_fields() {
    let table = SpellingStateTable {
      positions: vec![0, 4, 4, 10],
      states: vec![
        SpellingState {
          kind: SpellingStateKind::Clean,
          error: false,
        },
        SpellingState {
          kind: SpellingStateKind::Dirty,
          error: true,
        },
        SpellingState {
          kind: SpellingStateKind::UnknownWord,
          error: true,
        },
      ],
    };
    let bytes = table.to_bytes().unwrap();
    assert_eq!(SpellingStateTable::from_bytes(&bytes).unwrap(), table);

    let mut invalid_bits = bytes.clone();
    let state_offset = table.positions.len() * 4;
    invalid_bits[state_offset + 1] = 0x01;
    assert!(SpellingStateTable::from_bytes(&invalid_bits).is_err());

    let mut missing_error = bytes;
    let last_state_offset = state_offset + 4;
    missing_error[last_state_offset] &= !0x10;
    assert!(SpellingStateTable::from_bytes(&missing_error).is_err());

    let compatibility = SpellingState {
      kind: SpellingStateKind::Compatibility13,
      error: true,
    };
    assert_eq!(SpellingState::from_u16(0x001d).unwrap(), compatibility);
    assert_eq!(compatibility.to_u16().unwrap(), 0x001d);
    assert!(SpellingState::from_u16(0x000d).is_err());
  }

  #[test]
  fn grammar_state_table_round_trips_grammar_specific_spls_fields() {
    let table = GrammarStateTable {
      positions: vec![0, 5, 5, 12],
      states: vec![
        GrammarState {
          kind: GrammarStateKind::Clean,
          error: false,
          extend: false,
          typo: false,
        },
        GrammarState {
          kind: GrammarStateKind::Dirty,
          error: true,
          extend: true,
          typo: false,
        },
        GrammarState {
          kind: GrammarStateKind::ErrorMin,
          error: true,
          extend: false,
          typo: true,
        },
      ],
    };
    let bytes = table.to_bytes().unwrap();
    assert_eq!(GrammarStateTable::from_bytes(&bytes).unwrap(), table);

    let invalid = GrammarState {
      kind: GrammarStateKind::Clean,
      error: false,
      extend: true,
      typo: false,
    };
    assert!(invalid.to_u16().is_err());

    let mut unused = bytes;
    let first_state_offset = table.positions.len() * 4;
    unused[first_state_offset + 1] = 0x01;
    assert!(GrammarStateTable::from_bytes(&unused).is_err());
  }

  #[test]
  fn language_detection_state_table_round_trips_lad_spls_fields() {
    let table = LanguageDetectionStateTable {
      positions: vec![0, 6, 6, 14],
      states: vec![
        LanguageDetectionState {
          kind: LanguageDetectionStateKind::Clean,
          error: false,
        },
        LanguageDetectionState {
          kind: LanguageDetectionStateKind::Dirty,
          error: true,
        },
        LanguageDetectionState {
          kind: LanguageDetectionStateKind::NoLanguageDetection,
          error: false,
        },
      ],
    };
    let bytes = table.to_bytes().unwrap();
    assert_eq!(
      LanguageDetectionStateTable::from_bytes(&bytes).unwrap(),
      table
    );

    let state_offset = table.positions.len() * 4;
    for invalid_mask in [0x0020_u16, 0x0040, 0x0080] {
      let mut invalid = bytes.clone();
      let encoded =
        u16::from_le_bytes([invalid[state_offset], invalid[state_offset + 1]]) | invalid_mask;
      invalid[state_offset..state_offset + 2].copy_from_slice(&encoded.to_le_bytes());
      assert!(LanguageDetectionStateTable::from_bytes(&invalid).is_err());
    }

    let invalid_error = LanguageDetectionState {
      kind: LanguageDetectionStateKind::NoLanguageDetection,
      error: true,
    };
    assert!(invalid_error.to_u16().is_err());
  }

  #[test]
  fn list_style_templates_round_trip_static_tplc_variants() {
    let mut levels = [ListLevelTemplateCode::UserDefined { random: 0 }; 9];
    levels[0] = ListLevelTemplateCode::BuiltIn {
      format: BuiltInListFormat::Format(7),
      lid: 0x0409,
    };
    levels[1] = ListLevelTemplateCode::BuiltIn {
      format: BuiltInListFormat::None,
      lid: 0,
    };
    levels[2] = ListLevelTemplateCode::UserDefined { random: 0x1234567 };
    let table = ListStyleTemplates {
      lists: vec![Some(levels), None],
    };
    let bytes = table.to_bytes().unwrap();
    assert_eq!(ListStyleTemplates::from_bytes(&bytes).unwrap(), table);

    let mut invalid_length = bytes.clone();
    invalid_length[6..8].copy_from_slice(&1_u16.to_le_bytes());
    assert!(ListStyleTemplates::from_bytes(&invalid_length).is_err());
    assert!(
      ListLevelTemplateCode::UserDefined {
        random: 0x8000_0000
      }
      .to_u32()
      .is_err()
    );
    assert!(ListLevelTemplateCode::from_u32(0x0000_001d).is_err());
  }

  #[test]
  fn frame_and_list_records_round_trip_all_dofrt_variants() {
    let records = FrameAndListRecords {
      records: vec![
        FrameAndListRecord::FrameSet,
        FrameAndListRecord::Frame(FrameRecord {
          divider_units: FrameDividerUnits::Percent,
          divider_value: 50,
          child_layout: FrameChildLayout::Columns,
          kind: FrameRecordKind::Frame,
          horizontal_margin: 2,
          vertical_margin: 3,
          scroll: FrameScroll::Auto,
          linked: true,
          no_resize: false,
          unused_flags: 7,
          unused: 9,
        }),
        FrameAndListRecord::ChildMarker {
          push: true,
          unused: 5,
        },
        FrameAndListRecord::FrameName(Xstz {
          characters: "main".encode_utf16().collect(),
          terminator: 0,
        }),
        FrameAndListRecord::FrameFilePath(Xstz {
          characters: "frame.html".encode_utf16().collect(),
          terminator: 0,
        }),
        FrameAndListRecord::FrameBorder(FrameBorder {
          width_twips: 120,
          color: ColorRef {
            red: 1,
            green: 2,
            blue: 3,
            auto: 0,
          },
          no_border: false,
          three_dimensional: true,
        }),
        FrameAndListRecord::ListStyles(vec![ListStyleReference {
          list_index: 2,
          style_index: 0x123,
          style_definition: true,
        }]),
      ],
    };
    let bytes = records.to_bytes().unwrap();
    assert_eq!(FrameAndListRecords::from_bytes(&bytes).unwrap(), records);

    let invalid = FrameAndListRecords {
      records: vec![FrameAndListRecord::ListStyles(vec![ListStyleReference {
        list_index: 0,
        style_index: 0x1000,
        style_definition: false,
      }])],
    };
    assert!(invalid.to_bytes().is_err());
  }

  #[test]
  fn grammar_option_sets_round_trip_fixed_cosl_fields() {
    let sets = GrammarOptionSets {
      options: vec![
        GrammarOptionSet {
          option_set: 1,
          language_id: 0x0409,
          checker_version: 0x10203,
          company_id: 7,
        },
        GrammarOptionSet {
          option_set: 6,
          language_id: 0x0411,
          checker_version: 0,
          company_id: 0,
        },
      ],
    };
    let bytes = sets.to_bytes().unwrap();
    assert_eq!(GrammarOptionSets::from_bytes(&bytes).unwrap(), sets);

    let mut mismatched_count = bytes;
    mismatched_count[..4].copy_from_slice(&3_i32.to_le_bytes());
    assert!(GrammarOptionSets::from_bytes(&mismatched_count).is_err());
    assert!(GrammarOptionSets::from_bytes(&(-1_i32).to_le_bytes()).is_err());

    let legacy = LegacyGrammarOptionSets {
      options: vec![LegacyGrammarOptionSet {
        option_set: 1,
        language_id: 0x0409,
        checker_version: 3,
        company_id: 64,
      }],
    };
    let bytes = legacy.to_bytes().unwrap();
    assert_eq!(LegacyGrammarOptionSets::from_bytes(&bytes).unwrap(), legacy);

    let mut mismatched_count = bytes;
    mismatched_count[..4].copy_from_slice(&2_i32.to_le_bytes());
    assert!(LegacyGrammarOptionSets::from_bytes(&mismatched_count).is_err());
  }

  #[test]
  fn auto_summary_ranges_round_trip_with_typed_dop_info() {
    let info = AutoSummaryInfo {
      valid: true,
      view_active: true,
      view_by: AutoSummaryView::HideNonSummaryText,
      update_properties: true,
      desired_size: AutoSummaryDesiredSize::TwentyFivePercent,
      highest_level: 12,
      current_level: 3,
    };
    let mut info_bytes = Vec::new();
    info.write(&mut info_bytes).unwrap();
    assert_eq!(info_bytes.len(), 12);
    let mut input = SliceReader::new(&info_bytes);
    assert_eq!(AutoSummaryInfo::read(&mut input).unwrap(), info);

    let table = AutoSummaryRangeTable {
      positions: vec![0, 4, 9],
      priorities: vec![
        AutoSummaryPriority { level: 2 },
        AutoSummaryPriority { level: 12 },
      ],
    };
    let bytes = table.to_bytes().unwrap();
    assert_eq!(AutoSummaryRangeTable::from_bytes(&bytes).unwrap(), table);
    table.validate_against(&info).unwrap();

    let mut duplicate_cp = bytes.clone();
    duplicate_cp[4..8].copy_from_slice(&0_u32.to_le_bytes());
    assert!(AutoSummaryRangeTable::from_bytes(&duplicate_cp).is_err());

    let mut nonpositive_level = bytes;
    nonpositive_level[12..16].copy_from_slice(&0_i32.to_le_bytes());
    assert!(AutoSummaryRangeTable::from_bytes(&nonpositive_level).is_err());

    let invalid_info = AutoSummaryInfo {
      highest_level: 11,
      ..info
    };
    assert!(table.validate_against(&invalid_info).is_err());
    let mut reserved = info_bytes;
    reserved[1] |= 0x80;
    assert!(AutoSummaryInfo::read(&mut SliceReader::new(&reserved)).is_err());
  }

  #[test]
  fn smart_tag_recognizer_states_round_trip_factoid_spls() {
    let table = SmartTagRecognizerStateTable {
      positions: vec![0, 3, 3, 9],
      states: vec![
        SmartTagRecognizerState {
          kind: SmartTagRecognizerStateKind::Pending,
        },
        SmartTagRecognizerState {
          kind: SmartTagRecognizerStateKind::Dirty,
        },
        SmartTagRecognizerState {
          kind: SmartTagRecognizerStateKind::Clean,
        },
      ],
    };
    let bytes = table.to_bytes().unwrap();
    assert_eq!(
      SmartTagRecognizerStateTable::from_bytes(&bytes).unwrap(),
      table
    );

    let state_offset = table.positions.len() * 4;
    let mut invalid_flag = bytes.clone();
    invalid_flag[state_offset] |= 0x10;
    assert!(SmartTagRecognizerStateTable::from_bytes(&invalid_flag).is_err());

    let mut invalid_kind = bytes;
    invalid_kind[state_offset] = 0x05;
    assert!(SmartTagRecognizerStateTable::from_bytes(&invalid_kind).is_err());
  }

  #[test]
  fn paragraph_group_properties_round_trip_masked_pgp_options() {
    let border = Brc {
      color: ColorRef {
        red: 10,
        green: 20,
        blue: 30,
        auto: 0,
      },
      line_width: 4,
      border_type: 1,
      spacing: 2,
      shadow: true,
      frame: false,
      reserved: 0,
    };
    let properties = ParagraphGroupProperties {
      entries: vec![
        ParagraphGroupProperty {
          id: 1,
          parent_id: 0,
          table_depth: 0,
          options: ParagraphGroupOptions::default(),
        },
        ParagraphGroupProperty {
          id: 2,
          parent_id: 1,
          table_depth: 1,
          options: ParagraphGroupOptions {
            left_margin: Some(120),
            right_margin: Some(-20),
            top_border: Some(border),
            html_block_type: Some(HtmlBlockType::BlockQuote),
            ..ParagraphGroupOptions::default()
          },
        },
      ],
    };
    let bytes = properties.to_bytes().unwrap();
    assert_eq!(
      ParagraphGroupProperties::from_bytes(&bytes).unwrap(),
      properties
    );

    let mut zero_id = bytes.clone();
    zero_id[2..6].fill(0);
    assert!(ParagraphGroupProperties::from_bytes(&zero_id).is_err());

    let second_flags_offset = 2 + 14 + 12;
    let mut invalid_flags = bytes;
    invalid_flags[second_flags_offset..second_flags_offset + 2]
      .copy_from_slice(&0x0200_u16.to_le_bytes());
    assert!(ParagraphGroupProperties::from_bytes(&invalid_flags).is_err());
  }

  #[test]
  fn save_history_round_trips_paired_utf16_strings() {
    let history = SaveHistory {
      entries: vec![
        SaveHistoryEntry {
          author: "Alice".encode_utf16().collect(),
          path: r"C:\draft.doc".encode_utf16().collect(),
        },
        SaveHistoryEntry {
          author: "编辑者".encode_utf16().collect(),
          path: r"D:\final.doc".encode_utf16().collect(),
        },
      ],
    };
    let bytes = history.to_bytes().unwrap();
    assert_eq!(SaveHistory::from_bytes(&bytes).unwrap(), history);

    let mut odd_count = bytes.clone();
    odd_count[2..4].copy_from_slice(&3_u16.to_le_bytes());
    assert!(SaveHistory::from_bytes(&odd_count).is_err());

    let too_many = SaveHistory {
      entries: vec![
        SaveHistoryEntry {
          author: Vec::new(),
          path: Vec::new(),
        };
        11
      ],
    };
    assert!(too_many.to_bytes().is_err());
  }

  #[test]
  fn smart_tag_bookmarks_round_trip_parallel_typed_tables() {
    let bookmarks = SmartTagBookmarks {
      infos: vec![
        SmartTagBookmarkInfo {
          id: 10,
          sub_entity: false,
          unused: 0,
          source: SmartTagSource::Grammar,
          ignored_property_bag_pointer: 0,
        },
        SmartTagBookmarkInfo {
          id: 11,
          sub_entity: true,
          unused: 2,
          source: SmartTagSource::ScanDll,
          ignored_property_bag_pointer: 7,
        },
      ],
      starts: SmartTagBookmarkStartTable {
        positions: vec![0, 2, 10],
        bookmarks: vec![
          SmartTagBookmarkStart {
            bookmark: BookmarkStart {
              end_index: 1,
              column_start: 0,
              published: false,
              column_limit: 0,
              native: false,
              column: false,
            },
            depth: 0,
          },
          SmartTagBookmarkStart {
            bookmark: BookmarkStart {
              end_index: 0,
              column_start: 0,
              published: false,
              column_limit: 0,
              native: false,
              column: false,
            },
            depth: 1,
          },
        ],
      },
      ends: SmartTagBookmarkEndTable {
        positions: vec![5, 8, 10],
        bookmarks: vec![
          SmartTagBookmarkEnd {
            start_index: 1,
            depth: 1,
          },
          SmartTagBookmarkEnd {
            start_index: 0,
            depth: 0,
          },
        ],
      },
    };
    let (infos, starts, ends) = bookmarks.to_bytes().unwrap();
    assert_eq!(
      SmartTagBookmarks::from_bytes(&infos, &starts, &ends).unwrap(),
      bookmarks
    );

    let mut invalid = bookmarks;
    invalid.ends.bookmarks[0].start_index = 0;
    assert!(invalid.to_bytes().is_err());
  }

  #[test]
  fn grammar_checker_cookies_round_trip_fcks_bit_fields() {
    let table = GrammarCheckerCookieTable {
      positions: vec![0, 4, 4, 12],
      cookies: vec![
        GrammarCheckerCookie {
          character_count: 4,
          sentence_offset: 1,
          data_offset: 20,
          error_type: GrammarCookieErrorType::Typo,
          error: true,
          language_sub: 1,
          language_primary: 9,
          header: false,
        },
        GrammarCheckerCookie {
          character_count: 0,
          sentence_offset: 0,
          data_offset: 40,
          error_type: GrammarCookieErrorType::Default,
          error: false,
          language_sub: 1,
          language_primary: 9,
          header: true,
        },
        GrammarCheckerCookie {
          character_count: 8,
          sentence_offset: -2,
          data_offset: 80,
          error_type: GrammarCookieErrorType::Consistency,
          error: false,
          language_sub: 2,
          language_primary: 10,
          header: false,
        },
      ],
    };
    let bytes = table.to_bytes().unwrap();
    assert_eq!(
      GrammarCheckerCookieTable::from_bytes(&bytes).unwrap(),
      table
    );

    let store = GrammarCookieStore {
      cookies: vec![
        GrammarCookieData {
          provider_data: vec![1; 8],
        },
        GrammarCookieData {
          provider_data: vec![2; 16],
        },
        GrammarCookieData {
          provider_data: vec![3; 36],
        },
        GrammarCookieData {
          provider_data: vec![],
        },
      ],
    };
    let store_bytes = store.to_bytes().unwrap();
    assert_eq!(GrammarCookieStore::from_bytes(&store_bytes).unwrap(), store);
    assert_eq!(store.entry_offsets().unwrap(), [8, 20, 40, 80]);
    assert_eq!(
      store
        .cookie_at_offset(20)
        .unwrap()
        .unwrap()
        .provider_data
        .len(),
      16
    );
    store.validate_references(&table).unwrap();

    let mut duplicate_header = table;
    duplicate_header.cookies[0].header = true;
    assert!(duplicate_header.to_bytes().is_err());

    let mut invalid_reference = duplicate_header;
    invalid_reference.cookies[0].header = false;
    invalid_reference.cookies[0].data_offset = 21;
    assert!(store.validate_references(&invalid_reference).is_err());

    let mut wrong_total = store_bytes;
    wrong_total[..4].copy_from_slice(&7_u32.to_le_bytes());
    assert!(GrammarCookieStore::from_bytes(&wrong_total).is_err());
  }

  #[test]
  fn legacy_grammar_checker_cookies_round_trip_fcksold_bit_fields() {
    let table = LegacyGrammarCheckerCookieTable {
      positions: vec![0, 4, 4],
      cookies: vec![
        LegacyGrammarCheckerCookie {
          language_id: 0x0409,
          character_count: 4,
          sentence_offset: 0,
          padding1: 0xa5a5,
          error_type: GrammarCookieErrorType::Typo,
          spare: 0x1234,
          error: true,
          padding2: 0x5a5a,
          data_offset: 8,
        },
        LegacyGrammarCheckerCookie {
          language_id: 0x0411,
          character_count: 0,
          sentence_offset: -3,
          padding1: 0,
          error_type: GrammarCookieErrorType::Homonym,
          spare: 0,
          error: false,
          padding2: u16::MAX,
          data_offset: 16,
        },
      ],
    };
    let bytes = table.to_bytes().unwrap();
    assert_eq!(bytes.len(), 44);
    assert_eq!(
      LegacyGrammarCheckerCookieTable::from_bytes(&bytes).unwrap(),
      table
    );

    let store = GrammarCookieStore {
      cookies: vec![
        GrammarCookieData {
          provider_data: vec![1, 2, 3, 4],
        },
        GrammarCookieData {
          provider_data: vec![],
        },
      ],
    };
    assert_eq!(store.entry_offsets().unwrap(), [8, 16]);
    store.validate_legacy_references(&table).unwrap();

    let mut invalid = table.clone();
    invalid.cookies[0].character_count = -1;
    assert!(invalid.to_bytes().is_err());
    invalid.cookies[0].character_count = 1;
    invalid.cookies[0].sentence_offset = 1;
    assert!(invalid.to_bytes().is_err());
    invalid.cookies[0].sentence_offset = 0;
    invalid.cookies[0].spare = 0x2000;
    assert!(invalid.to_bytes().is_err());
    invalid.cookies[0].spare = 0;
    invalid.cookies[0].data_offset = 9;
    assert!(store.validate_legacy_references(&invalid).is_err());

    let mut negative_dcp = bytes.clone();
    negative_dcp[14..16].copy_from_slice(&(-1_i16).to_le_bytes());
    assert!(LegacyGrammarCheckerCookieTable::from_bytes(&negative_dcp).is_err());
    let mut positive_sentence_offset = bytes;
    positive_sentence_offset[16..18].copy_from_slice(&1_i16.to_le_bytes());
    assert!(LegacyGrammarCheckerCookieTable::from_bytes(&positive_sentence_offset).is_err());
  }

  #[test]
  fn smart_tag_data_round_trips_property_bag_store_statically() {
    let data = SmartTagData {
      factoid_types: vec![SmartTagFactoidType {
        id: SmartTagFactoidTypeId::Standard(7),
        uri: PropertyBagString::Ansi(b"urn:test".to_vec()),
        tag: PropertyBagString::Unicode("Person".encode_utf16().collect()),
        download_url: PropertyBagString::Ansi(Vec::new()),
      }],
      reserved_factoid_count: 3,
      strings: vec![
        PropertyBagString::Unicode("name".encode_utf16().collect()),
        PropertyBagString::Unicode("Alice".encode_utf16().collect()),
        PropertyBagString::Ansi(b"city".to_vec()),
        PropertyBagString::Ansi(b"Paris".to_vec()),
      ],
      property_bags: vec![SmartTagPropertyBag {
        factoid_type_id: 7,
        properties: vec![
          SmartTagProperty {
            key_index: 0,
            value_index: 1,
          },
          SmartTagProperty {
            key_index: 2,
            value_index: 3,
          },
        ],
      }],
    };
    let bytes = data.to_bytes().unwrap();
    assert_eq!(SmartTagData::from_bytes(&bytes).unwrap(), data);

    let mut invalid = data;
    invalid.property_bags[0].properties[0].value_index = 4;
    assert!(invalid.to_bytes().is_err());
  }

  #[test]
  fn smart_tag_data_only_accepts_the_known_malformed_cve_factoid_id() {
    let data = SmartTagData {
      factoid_types: vec![SmartTagFactoidType {
        id: SmartTagFactoidTypeId::MalformedCve20163133,
        uri: PropertyBagString::Ansi(Vec::new()),
        tag: PropertyBagString::Ansi(Vec::new()),
        download_url: PropertyBagString::Ansi(Vec::new()),
      }],
      reserved_factoid_count: 0,
      strings: Vec::new(),
      property_bags: vec![SmartTagPropertyBag {
        factoid_type_id: 4,
        properties: Vec::new(),
      }],
    };
    let bytes = data.to_bytes().unwrap();
    assert_eq!(SmartTagData::from_bytes(&bytes).unwrap(), data);

    let mut unknown = bytes;
    unknown[8..12].copy_from_slice(&0x0005_0005_u32.to_le_bytes());
    assert!(SmartTagData::from_bytes(&unknown).is_err());
  }

  #[test]
  fn table_character_cache_round_trips_tch_bit_fields() {
    let table = TableCharacterCacheTable {
      positions: vec![0, 8, 10, 24],
      caches: vec![
        TableCharacterCache {
          unknown: false,
          unused: 0,
        },
        TableCharacterCache {
          unknown: true,
          unused: 0,
        },
        TableCharacterCache {
          unknown: false,
          unused: 0x1234,
        },
      ],
    };
    let bytes = table.to_bytes().unwrap();
    assert_eq!(TableCharacterCacheTable::from_bytes(&bytes).unwrap(), table);

    let mut duplicate = table.clone();
    duplicate.positions[2] = duplicate.positions[1];
    assert!(duplicate.to_bytes().is_err());

    let mut overflow = table;
    overflow.caches[0].unused = 0x8000_0000;
    assert!(overflow.to_bytes().is_err());
  }

  #[test]
  fn revision_message_threading_round_trips_six_static_string_tables() {
    let threading = RevisionMessageThreading {
      messages: vec![RevisionThreadMessage {
        identifier: "message-id".encode_utf16().collect(),
        display: MessageDisplayProperties {
          created: Dttm {
            minute: 30,
            hour: 14,
            day: 12,
            month: 7,
            year_offset: 125,
            weekday: 6,
          },
          reserved: 0,
          author_index: 1,
        },
      }],
      styles: vec!["PersonalStyle".encode_utf16().collect()],
      author_attributes: vec![RevisionThreadAttribute {
        name: "role".encode_utf16().collect(),
        target_index: 1,
      }],
      author_values: vec!["reviewer".encode_utf16().collect()],
      message_attributes: vec![RevisionThreadAttribute {
        name: "priority".encode_utf16().collect(),
        target_index: 0,
      }],
      message_values: vec!["high".encode_utf16().collect()],
    };
    let bytes = threading.to_bytes().unwrap();
    assert_eq!(
      RevisionMessageThreading::from_bytes(&bytes).unwrap(),
      threading
    );

    let mut invalid = threading;
    invalid.message_values.clear();
    assert!(invalid.to_bytes().is_err());
  }

  #[test]
  fn revision_save_ids_round_trip_fixed_header_and_typed_ids() {
    let ids = RevisionSaveIdTable {
      reserved2: 7,
      reserved3: 0x1234_5678,
      ids: vec![RevisionSaveId(0), RevisionSaveId(0x89ab_cdef)],
    };
    let bytes = ids.to_bytes().unwrap();
    assert_eq!(RevisionSaveIdTable::from_bytes(&bytes).unwrap(), ids);

    let mut invalid = ids;
    invalid.reserved2 = 32;
    assert!(invalid.to_bytes().is_err());

    let mut bad_header = bytes;
    bad_header[4..8].copy_from_slice(&8u32.to_le_bytes());
    assert!(RevisionSaveIdTable::from_bytes(&bad_header).is_err());
  }

  #[test]
  fn selection_state_round_trips_flag_selected_range_variants() {
    let mut selection = SelectionState::from_bytes(&[0; 36]).unwrap();
    selection.flags.block = true;
    selection.range = SelectionRange::Block {
      first_pixel: -20,
      limit_pixel: 240,
    };
    selection.style = SelectionStyle::Line;
    let encoded = selection.to_bytes().unwrap();
    assert_eq!(SelectionState::from_bytes(&encoded).unwrap(), selection);

    let extended = SelectionState::from_bytes(&[0; 44]).unwrap();
    assert_eq!(extended.to_bytes().unwrap(), [0; 44]);
  }

  #[test]
  fn command_customizations_round_trip_macro_chain_statically() {
    let value = CommandCustomizations {
      records: vec![
        CommandCustomizationRecord::MacroCommands(vec![MacroCommandDescriptor {
          reserved1: 0x56,
          reserved2: 0,
          macro_name_index: 1,
          command_string_index: 0,
          reserved3: 0xffff,
          reserved4: 0,
          reserved5: 0,
          reserved6: 0,
          reserved7: 0,
        }]),
        CommandCustomizationRecord::CommandStrings(vec![CommandString {
          value: "Project.Module.Macro".encode_utf16().collect(),
          reference_count: 1,
        }]),
        CommandCustomizationRecord::MacroNames(vec![MacroName {
          index: 1,
          value: Xstz {
            characters: "PROJECT.MODULE.MACRO".encode_utf16().collect(),
            terminator: 0,
          },
        }]),
      ],
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(CommandCustomizations::from_bytes(&bytes).unwrap(), value);
  }

  #[test]
  fn toolbar_wrapper_round_trips_static_header_customization_and_delta() {
    let value = CommandCustomizations {
      records: vec![CommandCustomizationRecord::Toolbar(ToolbarWrapper {
        reserved2: 0,
        reserved3: 7,
        reserved4: 6,
        reserved5: 12,
        toolbar_delta_size: 18,
        controls: vec![ToolbarControl {
          header: ToolbarControlHeader {
            signature: 3,
            version: 1,
            flags: 0,
            control_type: 0x0a,
            control_id: 1,
            specific_flags: 0,
            priority: 0,
            size: None,
          },
          command_id: None,
          data: Some(ToolbarControlData {
            general: ToolbarControlGeneralInfo {
              flags: 0,
              custom_text: None,
              description: None,
              tooltip: None,
              extra: None,
            },
            specific: ToolbarControlSpecific::Menu {
              toolbar_id: 0,
              name: None,
            },
          }),
        }],
        customizations: vec![ToolbarCustomization {
          toolbar_id: 0x25,
          reserved: 0,
          deltas: vec![ToolbarDelta {
            operation: 1,
            at_end: true,
            reserved: 0,
            control_index: 2,
            next_command_id: -1,
            command_id: 0x1ef9,
            file_offset: 42,
            toolbar_index_flags: 1,
            control_byte_count: 16,
          }],
          custom_toolbar: None,
        }],
      })],
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(CommandCustomizations::from_bytes(&bytes).unwrap(), value);
  }

  #[test]
  fn custom_toolbar_round_trips_tb_visual_data_and_controls() {
    let rectangle = ToolbarRectangle {
      left: 1,
      top: 2,
      right: 101,
      bottom: 42,
    };
    let visual = ToolbarVisualData {
      dock_state: 4,
      visibility: 1,
      last_dock_state: 1,
      row: -2,
      docked: rectangle,
      floating: rectangle,
    };
    let value = CommandCustomizations {
      records: vec![CommandCustomizationRecord::Toolbar(ToolbarWrapper {
        reserved2: 0,
        reserved3: 7,
        reserved4: 6,
        reserved5: 12,
        toolbar_delta_size: 18,
        controls: Vec::new(),
        customizations: vec![ToolbarCustomization {
          toolbar_id: 0,
          reserved: 0,
          deltas: Vec::new(),
          custom_toolbar: Some(CustomToolbar {
            name: "Custom".encode_utf16().collect(),
            declared_toolbar_data_size: 129,
            toolbar: ToolbarData {
              signature: 2,
              version: 1,
              declared_control_count: 1,
              toolbar_id: 1,
              type_restrictions: 0,
              default_rows: 1,
              flags: 0,
              name: Vec::new(),
            },
            visual_data: [visual; 5],
            customization_index: 0,
            reserved: 0,
            unused: 0xa5a5,
            controls: vec![ToolbarControl {
              header: ToolbarControlHeader {
                signature: 3,
                version: 1,
                flags: 0,
                control_type: 0x16,
                control_id: 1,
                specific_flags: 0,
                priority: 0,
                size: None,
              },
              command_id: None,
              data: None,
            }],
          }),
        }],
      })],
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(CommandCustomizations::from_bytes(&bytes).unwrap(), value);
  }

  #[test]
  fn variable_sprm_operands_round_trip_as_static_structures() {
    let outline_level = Anlv {
      number_format: 1,
      text_before: 2,
      text_after: 3,
      justification: 2,
      include_previous_levels: true,
      hanging_indent: true,
      set_bold: true,
      set_italic: false,
      set_small_caps: true,
      set_caps: false,
      set_strike: true,
      set_underline: true,
      previous_space: true,
      bold: true,
      italic: false,
      small_caps: true,
      caps: false,
      strike: true,
      underline: 4,
      color: 17,
      font_index: 5,
      font_size_half_points: 24,
      start_at: 1,
      indent_twips: 720,
      space_twips: 360,
    };
    let value = GrpPrl {
      properties: vec![
        Prl {
          sprm: Sprm::from_opcode(0xd202),
          operand: SprmOperand::OutlineListData(Box::new(OlstOperand {
            levels: [outline_level; 9],
            restart_heading: 1,
            reserved: [2, 3, 4],
            display_text: std::array::from_fn(|index| index as u16),
          })),
        },
        Prl {
          sprm: Sprm::from_opcode(0x3014),
          operand: SprmOperand::SectionHeaderFooterFlags(SectionHeaderFooterFlags {
            even_header: true,
            odd_header: false,
            even_footer: true,
            odd_footer: false,
            first_header: true,
            first_footer: true,
            reserved: 2,
          }),
        },
        Prl {
          sprm: Sprm::from_opcode(SPRM_P_CHG_TABS),
          operand: SprmOperand::ParagraphChangeTabs(PChgTabsOperand {
            deleted: vec![DeletedTabStop {
              position: 720,
              close_distance: 25,
            }],
            added: vec![AddedTabStop {
              position: 1440,
              descriptor: TabDescriptor {
                alignment: 1,
                leader: 2,
                reserved: 0,
              },
            }],
          }),
        },
        Prl {
          sprm: Sprm::from_opcode(0xc60d),
          operand: SprmOperand::ParagraphChangeTabsPapx(PChgTabsPapxOperand {
            deleted_positions: vec![360],
            added: vec![AddedTabStop {
              position: 1080,
              descriptor: TabDescriptor {
                alignment: 2,
                leader: 1,
                reserved: 0,
              },
            }],
          }),
        },
        Prl {
          sprm: Sprm::from_opcode(0xc645),
          operand: SprmOperand::ParagraphNumberRevisionMark(NumRmOperand {
            numbered_before_tracking: 1,
            ignored_flag: 0,
            author_index: 2,
            timestamp: 0x1234_5678,
            placeholder_indices: [0; 9],
            number_formats: [0; 9],
            ignored: 0,
            number_values: [0; 9],
            format_string: [0; 32],
          }),
        },
        Prl {
          sprm: Sprm::from_opcode(0xca47),
          operand: SprmOperand::CharacterMajority(Box::new(GrpPrl {
            properties: vec![Prl {
              sprm: Sprm::from_opcode(0x0835),
              operand: SprmOperand::Toggle(1),
            }],
          })),
        },
        Prl {
          sprm: Sprm::from_opcode(0xca62),
          operand: SprmOperand::CharacterDisplayFieldRevisionMark(DispFldRmOperand {
            has_revision: 1,
            author_index: 3,
            timestamp: 0x8765_4321,
            previous_result: [0; 16],
          }),
        },
        Prl {
          sprm: Sprm::from_opcode(KnownSprm::PIstdPermute.opcode()),
          operand: SprmOperand::StylePermutation(SppOperand {
            ignored_long: 0,
            first_style_index: 3,
            last_style_index: 5,
            remapped_style_indices: vec![7, 8, 9],
          }),
        },
      ],
    };
    let bytes = value.to_bytes().unwrap();
    let parsed = GrpPrl::from_bytes(&bytes).unwrap();
    assert_eq!(parsed, value);
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
    let SprmOperand::StylePermutation(permutation) = &parsed.properties.last().unwrap().operand
    else {
      panic!("sprmPIstdPermute was not parsed as SPPOperand")
    };
    assert_eq!(permutation.remap(2), None);
    assert_eq!(permutation.remap(4), Some(8));
    let mut invalid_permutation = permutation.clone();
    invalid_permutation.remapped_style_indices.pop();
    assert!(
      GrpPrl {
        properties: vec![Prl {
          sprm: Sprm::from_opcode(KnownSprm::PIstdPermute.opcode()),
          operand: SprmOperand::StylePermutation(invalid_permutation),
        }],
      }
      .to_bytes()
      .is_err()
    );
    let mut invalid_flags = SectionHeaderFooterFlags::from_bits(0);
    invalid_flags.reserved = 4;
    assert!(invalid_flags.bits().is_err());
  }

  #[test]
  fn documented_sprm_opcode_has_static_identity_and_name() {
    let known = KnownSprm::from_opcode(0xd608).unwrap();
    assert_eq!(known, KnownSprm::TDefTable);
    assert_eq!(known.opcode(), 0xd608);
    assert_eq!(known.name(), "sprmTDefTable");
    assert_eq!(KnownSprm::from_opcode(0xffff), None);
  }

  #[test]
  fn data_stream_headers_and_prc_data_round_trip_statically() {
    let border = Brc80 {
      line_width: 1,
      border_type: 2,
      color_index: 3,
      spacing: 4,
      shadow: true,
      frame: false,
      reserved: false,
    };
    let mut border_writer = Writer::new(Cursor::new(Vec::new()));
    border.write_to(&mut border_writer).unwrap();
    assert_eq!(border_writer.into_inner().into_inner(), [1, 2, 3, 0x24]);
    let mut invalid_border = border;
    invalid_border.spacing = 0x20;
    assert!(
      invalid_border
        .write_to(&mut Writer::new(Cursor::new(Vec::new())))
        .is_err()
    );
    let picf = Picf {
      total_length: Picf::ENCODED_LEN as i32,
      header_length: Picf::ENCODED_LEN as u16,
      storage: Mfpf {
        format: PictureStorageFormat::Shape,
        unused_x_extent: 0,
        unused_y_extent: 0,
        ignored_handle: 0,
      },
      shape: PicfShape {
        ignored_flags: 0x1234,
        padding1: 0,
        ignored_mapping_mode: 0,
        padding2: 0,
      },
      picture: Picmid {
        goal_width_twips: 720,
        goal_height_twips: 360,
        horizontal_scale_tenths_percent: 1000,
        vertical_scale_tenths_percent: 1000,
        reserved_width1: 0,
        reserved_height1: 0,
        reserved_width2: 0,
        reserved_height2: 0,
        reserved_flags: 0,
        bits_per_pixel: 24,
        top_border: border,
        left_border: border,
        bottom_border: border,
        right_border: border,
        reserved_width3: 0,
        reserved_height3: 0,
      },
      property_count: 0,
    };
    let picf_bytes = picf.to_bytes().unwrap();
    assert_eq!(picf_bytes.len(), Picf::ENCODED_LEN);
    assert_eq!(Picf::from_bytes(&picf_bytes).unwrap(), picf);

    let mut stale_picture = PicfAndOfficeArtData {
      picf,
      shape_file_name: None,
      picture: OfficeArtStream {
        records: Vec::new(),
      },
    };
    stale_picture.picf.total_length = 0;
    assert!(stale_picture.to_bytes().is_err());
    let canonical_picture_bytes = stale_picture.to_bytes_with_computed_length().unwrap();
    let canonical_picture = PicfAndOfficeArtData::from_bytes(&canonical_picture_bytes).unwrap();
    assert_eq!(
      canonical_picture.picf.total_length,
      i32::try_from(canonical_picture_bytes.len()).unwrap()
    );
    assert_eq!(canonical_picture.picture.records, Vec::new());

    let binary = NilPicfAndBinData {
      total_length: 71,
      header_length: 68,
      ignored_header: [0; 62],
      binary_data: NilPicfBinaryData::Unresolved(vec![1, 2, 3]),
    };
    let binary_bytes = binary.to_bytes().unwrap();
    assert_eq!(
      NilPicfAndBinData::from_bytes(&binary_bytes).unwrap(),
      binary
    );
    let mut stale_binary = binary;
    stale_binary.total_length = 0;
    assert!(stale_binary.to_bytes().is_err());
    let canonical_binary_bytes = stale_binary.to_bytes_with_computed_length().unwrap();
    let canonical_binary = NilPicfAndBinData::from_bytes(&canonical_binary_bytes).unwrap();
    assert_eq!(
      canonical_binary.total_length,
      i32::try_from(canonical_binary_bytes.len()).unwrap()
    );
    assert_eq!(canonical_binary.binary_data, stale_binary.binary_data);

    let properties = PrcData {
      properties: GrpPrl {
        properties: vec![Prl {
          sprm: Sprm::from_opcode(0x0835),
          operand: SprmOperand::Toggle(1),
        }],
      },
    };
    let property_bytes = properties.to_bytes().unwrap();
    assert_eq!(PrcData::from_bytes(&property_bytes).unwrap(), properties);
    assert!(PrcData::from_bytes(&[0xff, 0xff]).is_err());
  }

  #[test]
  fn canonical_fkp_layout_packs_typed_runs_and_rejects_duplicate_boundaries() {
    let properties = GrpPrl {
      properties: vec![Prl {
        sprm: Sprm::from_opcode(0x0835),
        operand: SprmOperand::Toggle(1),
      }],
    };
    let chpx = ChpxFkp::with_canonical_layout(
      vec![10, 20, 30],
      vec![
        ChpxFkpRun {
          property_offset: None,
          properties: Some(properties.clone().into()),
        },
        ChpxFkpRun {
          property_offset: Some(42),
          properties: Some(properties.clone().into()),
        },
      ],
    )
    .unwrap();
    let mut cloned_chpx = chpx.clone();
    assert!(Arc::ptr_eq(
      chpx.runs[0].properties.as_ref().unwrap(),
      cloned_chpx.runs[0].properties.as_ref().unwrap(),
    ));
    Arc::make_mut(cloned_chpx.runs[0].properties.as_mut().unwrap())
      .properties
      .clear();
    assert!(
      !chpx.runs[0]
        .properties
        .as_ref()
        .unwrap()
        .properties
        .is_empty()
    );
    assert!(
      cloned_chpx.runs[0]
        .properties
        .as_ref()
        .unwrap()
        .properties
        .is_empty()
    );
    assert_eq!(chpx.runs[0].property_offset, chpx.runs[1].property_offset);
    let chpx_bytes = chpx.to_bytes().unwrap();
    let reopened_chpx = ChpxFkp::from_bytes(&chpx_bytes).unwrap();
    assert_eq!(reopened_chpx.file_positions, chpx.file_positions);
    assert_eq!(reopened_chpx.runs, chpx.runs);
    assert_eq!(reopened_chpx.to_bytes().unwrap(), chpx_bytes);

    let papx_properties = PapxInFkp {
      length_encoding: PapxLengthEncoding::HalfWordsMinusOne,
      style_index: 3,
      properties,
      trailing_byte: None,
    };
    let papx = PapxFkp::with_canonical_layout(
      vec![100, 120, 140],
      vec![
        PapxFkpRun {
          property_offset: None,
          paragraph_height_info: [1; 12],
          properties: Some(papx_properties.clone().into()),
        },
        PapxFkpRun {
          property_offset: Some(64),
          paragraph_height_info: [2; 12],
          properties: Some(papx_properties.into()),
        },
      ],
    )
    .unwrap();
    assert_eq!(papx.runs[0].property_offset, papx.runs[1].property_offset);
    let papx_bytes = papx.to_bytes().unwrap();
    let reopened_papx = PapxFkp::from_bytes(&papx_bytes).unwrap();
    assert_eq!(reopened_papx.file_positions, papx.file_positions);
    assert_eq!(reopened_papx.runs, papx.runs);
    assert_eq!(reopened_papx.to_bytes().unwrap(), papx_bytes);

    assert!(
      ChpxFkp::with_canonical_layout(
        vec![10, 10],
        vec![ChpxFkpRun {
          property_offset: None,
          properties: None,
        }],
      )
      .is_err()
    );
  }

  #[test]
  fn ffdata_and_hfd_round_trip_conditional_fields_statically() {
    let xstz = |characters: &[u16]| Xstz {
      characters: characters.to_vec(),
      terminator: 0,
    };
    let dropdown = FfData {
      version: 0xffff_ffff,
      bits: FfDataBits {
        field_kind: FormFieldKind::DropDown,
        result: 1,
        own_help: true,
        own_status: false,
        protected: true,
        automatic_size: false,
        text_kind: TextFormFieldKind::Regular,
        recalculate: false,
        has_list_box: true,
      },
      maximum_text_length: 0,
      check_box_size_half_points: 0,
      name: xstz(&[b'f' as u16]),
      default_text: None,
      default_selection: Some(0),
      text_format: xstz(&[]),
      help_text: xstz(&[b'h' as u16]),
      status_text: xstz(&[]),
      entry_macro: xstz(&[]),
      exit_macro: xstz(&[]),
      drop_down_list: Some(HsttbDropList {
        entries: vec![vec![b'A' as u16], vec![b'B' as u16]],
      }),
    };
    let dropdown_bytes = dropdown.to_bytes().unwrap();
    assert_eq!(FfData::from_bytes(&dropdown_bytes).unwrap(), dropdown);

    let hfd = Hfd {
      bits: HfdBits {
        open_in_new_window: true,
        do_not_preserve_history: false,
        image_map: false,
        has_location: false,
        has_tooltip: false,
        unused: 0,
      },
      class_id: Guid::ZERO,
      hyperlink: crate::xls::HyperlinkObject::Parsed {
        stream_version: 2,
        flags: crate::xls::HyperlinkFlags::empty(),
        display_name: None,
        target_frame_name: None,
        moniker: None,
        location: None,
        guid: None,
        creation_time: None,
        trailing: Vec::new(),
      },
    };
    let hfd_bytes = hfd.to_bytes().unwrap();
    assert_eq!(Hfd::from_bytes(&hfd_bytes).unwrap(), hfd);

    let mut form_container = NilPicfAndBinData {
      total_length: i32::try_from(NilPicfAndBinData::HEADER_LEN + dropdown_bytes.len()).unwrap(),
      header_length: NilPicfAndBinData::HEADER_LEN as u16,
      ignored_header: [0; 62],
      binary_data: NilPicfBinaryData::Unresolved(dropdown_bytes),
    };
    form_container.interpret(NilPicfFieldType::Form(FormFieldType::DropDown));
    assert!(matches!(
        form_container.binary_data,
        NilPicfBinaryData::Form {
            field_type: FormFieldType::DropDown,
            ref value,
        } if value == &dropdown
    ));

    let mut hyperlink_container = NilPicfAndBinData {
      total_length: i32::try_from(NilPicfAndBinData::HEADER_LEN + hfd_bytes.len()).unwrap(),
      header_length: NilPicfAndBinData::HEADER_LEN as u16,
      ignored_header: [0; 62],
      binary_data: NilPicfBinaryData::Unresolved(hfd_bytes),
    };
    hyperlink_container.interpret(NilPicfFieldType::Hyperlink(HyperlinkFieldType::Hyperlink));
    assert!(matches!(
        hyperlink_container.binary_data,
        NilPicfBinaryData::Hyperlink {
            field_type: HyperlinkFieldType::Hyperlink,
            ref value,
        } if value == &hfd
    ));

    let mut invalid = dropdown;
    invalid.bits.has_list_box = false;
    assert!(invalid.to_bytes().is_err());
    assert!(HfdBits::from_u8(0xe0).is_err());
  }
}
