//! MS-OFORMS control persistence structures.

use std::path::{Path, PathBuf};

use bitflags::bitflags;

use crate::{Error, Result, cfb::CompoundFile, common::Guid};

/// Fixed leaf name of the MS-OFORMS Form stream in a parent-control storage.
pub const FORM_STREAM_NAME: &str = "f";
/// Fixed leaf name of the MS-OFORMS Object stream in a parent-control storage.
pub const OBJECT_STREAM_NAME: &str = "o";
/// Fixed leaf name of the MS-OFORMS MultiPage extension stream.
pub const MULTIPAGE_STREAM_NAME: &str = "x";

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct MorphDataPropertyMask: u64 {
        const VARIOUS_PROPERTY_BITS = 1 << 0;
        const BACK_COLOR = 1 << 1;
        const FORE_COLOR = 1 << 2;
        const MAX_LENGTH = 1 << 3;
        const BORDER_STYLE = 1 << 4;
        const SCROLL_BARS = 1 << 5;
        const DISPLAY_STYLE = 1 << 6;
        const MOUSE_POINTER = 1 << 7;
        const SIZE = 1 << 8;
        const PASSWORD_CHAR = 1 << 9;
        const LIST_WIDTH = 1 << 10;
        const BOUND_COLUMN = 1 << 11;
        const TEXT_COLUMN = 1 << 12;
        const COLUMN_COUNT = 1 << 13;
        const LIST_ROWS = 1 << 14;
        const COLUMN_INFO_COUNT = 1 << 15;
        const MATCH_ENTRY = 1 << 16;
        const LIST_STYLE = 1 << 17;
        const SHOW_DROP_BUTTON_WHEN = 1 << 18;
        const UNUSED1 = 1 << 19;
        const DROP_BUTTON_STYLE = 1 << 20;
        const MULTI_SELECT = 1 << 21;
        const VALUE = 1 << 22;
        const CAPTION = 1 << 23;
        const PICTURE_POSITION = 1 << 24;
        const BORDER_COLOR = 1 << 25;
        const SPECIAL_EFFECT = 1 << 26;
        const MOUSE_ICON = 1 << 27;
        const PICTURE = 1 << 28;
        const ACCELERATOR = 1 << 29;
        const UNUSED2 = 1 << 30;
        const RESERVED = 1 << 31;
        const GROUP_NAME = 1 << 32;
        const UNUSED3 = 0xffff_fffe_0000_0000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FormScrollBarFlags: u8 {
        const HORIZONTAL = 1 << 0;
        const VERTICAL = 1 << 1;
        const KEEP_HORIZONTAL = 1 << 2;
        const KEEP_VERTICAL = 1 << 3;
        const KEEP_LEFT = 1 << 4;
        const UNUSED = 0xe0;
    }
}

impl FormScrollBarFlags {
  fn validate(self) -> Result<()> {
    if self.intersects(Self::UNUSED) {
      return Err(Error::invalid(0, "FormScrollBarFlags has unused bits set"));
    }
    Ok(())
  }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FormPropertyMask: u32 {
        const UNUSED1 = 1 << 0;
        const BACK_COLOR = 1 << 1;
        const FORE_COLOR = 1 << 2;
        const NEXT_AVAILABLE_ID = 1 << 3;
        const UNUSED2 = 0b11 << 4;
        const BOOLEAN_PROPERTIES = 1 << 6;
        const BORDER_STYLE = 1 << 7;
        const MOUSE_POINTER = 1 << 8;
        const SCROLL_BARS = 1 << 9;
        const DISPLAYED_SIZE = 1 << 10;
        const LOGICAL_SIZE = 1 << 11;
        const SCROLL_POSITION = 1 << 12;
        const GROUP_COUNT = 1 << 13;
        const RESERVED = 1 << 14;
        const MOUSE_ICON = 1 << 15;
        const CYCLE = 1 << 16;
        const SPECIAL_EFFECT = 1 << 17;
        const BORDER_COLOR = 1 << 18;
        const CAPTION = 1 << 19;
        const FONT = 1 << 20;
        const PICTURE = 1 << 21;
        const ZOOM = 1 << 22;
        const PICTURE_ALIGNMENT = 1 << 23;
        const PICTURE_TILING = 1 << 24;
        const PICTURE_SIZE_MODE = 1 << 25;
        const SHAPE_COOKIE = 1 << 26;
        const DRAW_BUFFER = 1 << 27;
        const UNUSED3 = 0xf000_0000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FormFlags: u32 {
        const UNUSED1 = 0b11;
        const ENABLED = 1 << 2;
        const UNUSED2 = 0x0000_3ff8;
        const DESIGN_EXTENDER_PERSISTED = 1 << 14;
        const DONT_SAVE_CLASS_TABLE = 1 << 15;
        const UNUSED3 = 0xffff_0000;
    }
}

impl FormFlags {
  fn validate(self) -> Result<()> {
    if self.intersects(Self::UNUSED1 | Self::UNUSED2 | Self::UNUSED3) {
      return Err(Error::invalid(
        0,
        "Form BooleanProperties has unused bits set",
      ));
    }
    Ok(())
  }
}

bitflags! {
    /// MS-OFORMS `SITE_FLAG` values persisted for an embedded control.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SiteFlags: u32 {
        const TAB_STOP = 1 << 0;
        const VISIBLE = 1 << 1;
        const DEFAULT = 1 << 2;
        const CANCEL = 1 << 3;
        const STREAMED = 1 << 4;
        const AUTO_SIZE = 1 << 5;
        const UNUSED1 = 0b11 << 6;
        const PRESERVE_HEIGHT = 1 << 8;
        const FIT_TO_PARENT = 1 << 9;
        const RESERVED1 = 0b111 << 10;
        const SELECT_CHILD = 1 << 13;
        const UNUSED2 = 0b1111 << 14;
        const PROMOTE_CONTROLS = 1 << 18;
        const UNUSED3 = 0xfff8_0000;
    }
}

impl SiteFlags {
  fn validate(self) -> Result<()> {
    if self.intersects(Self::UNUSED1 | Self::RESERVED1 | Self::UNUSED2 | Self::UNUSED3) {
      return Err(Error::invalid(
        0,
        "SITE_FLAG has unused or reserved bits set",
      ));
    }
    Ok(())
  }
}

bitflags! {
    /// MS-OFORMS `DX_MODE` design-surface properties.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct DesignExtenderFlags: u32 {
        const INHERIT_DESIGN = 1 << 0;
        const DESIGN = 1 << 1;
        const INHERIT_SHOW_TOOLBOX = 1 << 2;
        const SHOW_TOOLBOX = 1 << 3;
        const INHERIT_SHOW_GRID = 1 << 4;
        const SHOW_GRID = 1 << 5;
        const INHERIT_SNAP_TO_GRID = 1 << 6;
        const SNAP_TO_GRID = 1 << 7;
        const INHERIT_GRID_X = 1 << 8;
        const INHERIT_GRID_Y = 1 << 9;
        const INHERIT_CLICK_CONTROL = 1 << 10;
        const INHERIT_DOUBLE_CLICK_CONTROL = 1 << 11;
        const INHERIT_SHOW_INVISIBLE = 1 << 12;
        const SHOW_INVISIBLE = 1 << 13;
        const INHERIT_SHOW_TOOLTIPS = 1 << 14;
        const SHOW_TOOLTIPS = 1 << 15;
        const INHERIT_LAYOUT_IMMEDIATE = 1 << 16;
        const LAYOUT_IMMEDIATE = 1 << 17;
        const UNUSED = 0xfffc_0000;
    }
}

impl DesignExtenderFlags {
  fn validate(self) -> Result<()> {
    if self.intersects(Self::UNUSED) {
      return Err(Error::invalid(0, "DX_MODE has unused bits set"));
    }
    Ok(())
  }
}

bitflags! {
    /// MS-OFORMS `CLSTABLE_FLAGS` type-information properties.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ClassTableFlags: u16 {
        const EXCLUSIVE_VALUE = 1 << 0;
        const DUAL_INTERFACE = 1 << 1;
        const NO_AGGREGATION = 1 << 2;
        const UNUSED = 0xfff8;
    }
}

impl ClassTableFlags {
  fn validate(self) -> Result<()> {
    if self.intersects(Self::UNUSED) {
      return Err(Error::invalid(0, "CLSTABLE_FLAGS has unused bits set"));
    }
    Ok(())
  }
}

bitflags! {
    /// MS-OAUT `VARFLAGS` values used by a SiteClassInfo.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct VariableFlags: u16 {
        const READ_ONLY = 0x0001;
        const SOURCE = 0x0002;
        const BINDABLE = 0x0004;
        const REQUEST_EDIT = 0x0008;
        const DISPLAY_BIND = 0x0010;
        const DEFAULT_BIND = 0x0020;
        const HIDDEN = 0x0040;
        const RESTRICTED = 0x0080;
        const DEFAULT_COLLECTION_ELEMENT = 0x0100;
        const UI_DEFAULT = 0x0200;
        const NON_BROWSABLE = 0x0400;
        const REPLACEABLE = 0x0800;
        const IMMEDIATE_BIND = 0x1000;
        const UNUSED = 0xe000;
    }
}

bitflags! {
    /// MS-OFORMS `FONTFLAGS` used by the standard OLE font persistence format.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct StdFontFlags: u8 {
        const BOLD_RESERVED = 1 << 0;
        const ITALIC = 1 << 1;
        const UNDERLINE = 1 << 2;
        const STRIKETHROUGH = 1 << 3;
        const UNUSED = 0xf0;
    }
}

impl StdFontFlags {
  fn validate(self) -> Result<()> {
    if self.intersects(Self::BOLD_RESERVED | Self::UNUSED) {
      return Err(Error::invalid(0, "FONTFLAGS has a forbidden bit set"));
    }
    Ok(())
  }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TabStripTabFlags: u32 {
        const VISIBLE = 1 << 0;
        const ENABLED = 1 << 1;
        const UNUSED = 0xffff_fffc;
    }
}

impl TabStripTabFlags {
  fn validate(self) -> Result<()> {
    if self.intersects(Self::UNUSED) {
      return Err(Error::invalid(0, "TabStripTabFlag has unused bits set"));
    }
    Ok(())
  }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct MorphDataColumnInfoPropertyMask: u32 {
        const COLUMN_WIDTH = 1 << 0;
        const UNUSED = 0xffff_fffe;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnabledState {
  Disabled,
  Enabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CachedControlClass {
  Form,
  Image,
  Frame,
  MorphDataLegacy,
  SpinButton,
  CommandButton,
  TabStrip,
  Label,
  TextBox,
  ListBox,
  ComboBox,
  CheckBox,
  OptionButton,
  ToggleButton,
  ScrollBar,
  MultiPage,
  Compatibility(u16),
}

impl CachedControlClass {
  pub fn from_raw(value: u16) -> Self {
    match value {
      7 => Self::Form,
      12 => Self::Image,
      14 => Self::Frame,
      15 => Self::MorphDataLegacy,
      16 => Self::SpinButton,
      17 => Self::CommandButton,
      18 => Self::TabStrip,
      21 => Self::Label,
      23 => Self::TextBox,
      24 => Self::ListBox,
      25 => Self::ComboBox,
      26 => Self::CheckBox,
      27 => Self::OptionButton,
      28 => Self::ToggleButton,
      47 => Self::ScrollBar,
      57 => Self::MultiPage,
      value => Self::Compatibility(value),
    }
  }

  pub fn raw(self) -> u16 {
    match self {
      Self::Form => 7,
      Self::Image => 12,
      Self::Frame => 14,
      Self::MorphDataLegacy => 15,
      Self::SpinButton => 16,
      Self::CommandButton => 17,
      Self::TabStrip => 18,
      Self::Label => 21,
      Self::TextBox => 23,
      Self::ListBox => 24,
      Self::ComboBox => 25,
      Self::CheckBox => 26,
      Self::OptionButton => 27,
      Self::ToggleButton => 28,
      Self::ScrollBar => 47,
      Self::MultiPage => 57,
      Self::Compatibility(value) => value,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SiteClassIndex {
  Cached(CachedControlClass),
  Invalid,
  ClassTable(u16),
}

impl SiteClassIndex {
  pub fn from_raw(value: u16) -> Self {
    match value {
      0x7fff => Self::Invalid,
      0x8000..=u16::MAX => Self::ClassTable(value - 0x8000),
      value => Self::Cached(CachedControlClass::from_raw(value)),
    }
  }

  pub fn to_raw(self) -> Result<u16> {
    match self {
      Self::Cached(value) if value.raw() < 0x7fff => Ok(value.raw()),
      Self::Cached(_) => Err(Error::invalid(
        0,
        "cached control class index must be below 0x7fff",
      )),
      Self::Invalid => Ok(0x7fff),
      Self::ClassTable(index) if index < 0x8000 => Ok(0x8000 | index),
      Self::ClassTable(_) => Err(Error::invalid(0, "class-table index must be below 0x8000")),
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PersistenceMarker;

impl PersistenceMarker {
  pub const RAW: u16 = 0xffff;

  pub fn from_raw(value: u16) -> Result<Self> {
    if value != Self::RAW {
      return Err(Error::invalid(0, "persistence marker must be 0xffff"));
    }
    Ok(Self)
  }
}

impl EnabledState {
  pub fn from_raw(value: i32) -> Result<Self> {
    match value {
      0 => Ok(Self::Disabled),
      1 => Ok(Self::Enabled),
      _ => Err(Error::invalid(0, "enabled state must be zero or one")),
    }
  }

  pub fn raw(self) -> i32 {
    match self {
      Self::Disabled => 0,
      Self::Enabled => 1,
    }
  }

  pub fn is_enabled(self) -> bool {
    self == Self::Enabled
  }
}

impl VariableFlags {
  fn validate(self) -> Result<()> {
    if self.intersects(Self::UNUSED) {
      return Err(Error::invalid(0, "VARFLAGS has unused bits set"));
    }
    Ok(())
  }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SitePropertyMask: u32 {
        const NAME = 1 << 0;
        const TAG = 1 << 1;
        const ID = 1 << 2;
        const HELP_CONTEXT_ID = 1 << 3;
        const BIT_FLAGS = 1 << 4;
        const OBJECT_STREAM_SIZE = 1 << 5;
        const TAB_INDEX = 1 << 6;
        const CLSID_CACHE_INDEX = 1 << 7;
        const POSITION = 1 << 8;
        const GROUP_ID = 1 << 9;
        const UNUSED1 = 1 << 10;
        const CONTROL_TIP_TEXT = 1 << 11;
        const RUNTIME_LICENSE_KEY = 1 << 12;
        const CONTROL_SOURCE = 1 << 13;
        const ROW_SOURCE = 1 << 14;
        const UNUSED2 = 0xffff_8000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ClassInfoPropertyMask: u32 {
        const CLSID = 1 << 0;
        const DISPATCH_EVENT = 1 << 1;
        const UNUSED1 = 1 << 2;
        const DEFAULT_PROGRAM = 1 << 3;
        const CLASS_FLAGS = 1 << 4;
        const COUNT_OF_METHODS = 1 << 5;
        const DISPID_BIND = 1 << 6;
        const GET_BIND_INDEX = 1 << 7;
        const PUT_BIND_INDEX = 1 << 8;
        const BIND_TYPE = 1 << 9;
        const GET_VALUE_INDEX = 1 << 10;
        const PUT_VALUE_INDEX = 1 << 11;
        const VALUE_TYPE = 1 << 12;
        const DISPID_ROWSET = 1 << 13;
        const SET_ROWSET = 1 << 14;
        const UNUSED2 = 0xffff_8000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct DesignExtenderPropertyMask: u32 {
        const BIT_FLAGS = 1 << 0;
        const GRID_X = 1 << 1;
        const GRID_Y = 1 << 2;
        const CLICK_CONTROL_MODE = 1 << 3;
        const DOUBLE_CLICK_CONTROL_MODE = 1 << 4;
        const UNUSED = 0xffff_ffe0;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct PagePropertyMask: u32 {
        const UNUSED1 = 1 << 0;
        const TRANSITION_EFFECT = 1 << 1;
        const TRANSITION_PERIOD = 1 << 2;
        const UNUSED = 0xffff_fff8;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct MultiPagePropertyMask: u32 {
        const UNUSED1 = 1 << 0;
        const PAGE_COUNT = 1 << 1;
        const ID = 1 << 2;
        const FLAGS = 1 << 3;
        const UNUSED = 0xffff_fff0;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TabStripPropertyMask: u32 {
        const LIST_INDEX = 1 << 0;
        const BACK_COLOR = 1 << 1;
        const FORE_COLOR = 1 << 2;
        const UNUSED1 = 1 << 3;
        const SIZE = 1 << 4;
        const ITEMS = 1 << 5;
        const MOUSE_POINTER = 1 << 6;
        const UNUSED2 = 1 << 7;
        const TAB_ORIENTATION = 1 << 8;
        const TAB_STYLE = 1 << 9;
        const MULTI_ROW = 1 << 10;
        const TAB_FIXED_WIDTH = 1 << 11;
        const TAB_FIXED_HEIGHT = 1 << 12;
        const TOOLTIPS = 1 << 13;
        const UNUSED3 = 1 << 14;
        const TIP_STRINGS = 1 << 15;
        const UNUSED4 = 1 << 16;
        const NAMES = 1 << 17;
        const VARIOUS_PROPERTY_BITS = 1 << 18;
        const NEW_VERSION = 1 << 19;
        const TABS_ALLOCATED = 1 << 20;
        const TAGS = 1 << 21;
        const TAB_DATA = 1 << 22;
        const ACCELERATORS = 1 << 23;
        const MOUSE_ICON = 1 << 24;
        const UNUSED = 0xfe00_0000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ImagePropertyMask: u32 {
        const UNUSED1 = 0b11;
        const AUTO_SIZE = 1 << 2;
        const BORDER_COLOR = 1 << 3;
        const BACK_COLOR = 1 << 4;
        const BORDER_STYLE = 1 << 5;
        const MOUSE_POINTER = 1 << 6;
        const PICTURE_SIZE_MODE = 1 << 7;
        const SPECIAL_EFFECT = 1 << 8;
        const SIZE = 1 << 9;
        const PICTURE = 1 << 10;
        const PICTURE_ALIGNMENT = 1 << 11;
        const PICTURE_TILING = 1 << 12;
        const VARIOUS_PROPERTY_BITS = 1 << 13;
        const MOUSE_ICON = 1 << 14;
        const UNUSED2 = 0xffff_8000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct LabelPropertyMask: u32 {
        const FORE_COLOR = 1 << 0;
        const BACK_COLOR = 1 << 1;
        const VARIOUS_PROPERTY_BITS = 1 << 2;
        const CAPTION = 1 << 3;
        const PICTURE_POSITION = 1 << 4;
        const SIZE = 1 << 5;
        const MOUSE_POINTER = 1 << 6;
        const BORDER_COLOR = 1 << 7;
        const BORDER_STYLE = 1 << 8;
        const SPECIAL_EFFECT = 1 << 9;
        const PICTURE = 1 << 10;
        const ACCELERATOR = 1 << 11;
        const MOUSE_ICON = 1 << 12;
        const UNUSED = 0xffff_e000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SpinButtonPropertyMask: u32 {
        const FORE_COLOR = 1 << 0;
        const BACK_COLOR = 1 << 1;
        const VARIOUS_PROPERTY_BITS = 1 << 2;
        const SIZE = 1 << 3;
        const UNUSED1 = 1 << 4;
        const MIN = 1 << 5;
        const MAX = 1 << 6;
        const POSITION = 1 << 7;
        const PREV_ENABLED = 1 << 8;
        const NEXT_ENABLED = 1 << 9;
        const SMALL_CHANGE = 1 << 10;
        const ORIENTATION = 1 << 11;
        const DELAY = 1 << 12;
        const MOUSE_ICON = 1 << 13;
        const MOUSE_POINTER = 1 << 14;
        const UNUSED2 = 0xffff_8000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ScrollBarPropertyMask: u32 {
        const FORE_COLOR = 1 << 0;
        const BACK_COLOR = 1 << 1;
        const VARIOUS_PROPERTY_BITS = 1 << 2;
        const SIZE = 1 << 3;
        const MOUSE_POINTER = 1 << 4;
        const MIN = 1 << 5;
        const MAX = 1 << 6;
        const POSITION = 1 << 7;
        const UNUSED1 = 1 << 8;
        const PREV_ENABLED = 1 << 9;
        const NEXT_ENABLED = 1 << 10;
        const SMALL_CHANGE = 1 << 11;
        const LARGE_CHANGE = 1 << 12;
        const ORIENTATION = 1 << 13;
        const PROPORTIONAL_THUMB = 1 << 14;
        const DELAY = 1 << 15;
        const MOUSE_ICON = 1 << 16;
        const UNUSED2 = 0xfffe_0000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct CommandButtonPropertyMask: u32 {
        const FORE_COLOR = 1 << 0;
        const BACK_COLOR = 1 << 1;
        const VARIOUS_PROPERTY_BITS = 1 << 2;
        const CAPTION = 1 << 3;
        const PICTURE_POSITION = 1 << 4;
        const SIZE = 1 << 5;
        const MOUSE_POINTER = 1 << 6;
        const PICTURE = 1 << 7;
        const ACCELERATOR = 1 << 8;
        const TAKE_FOCUS_ON_CLICK = 1 << 9;
        const MOUSE_ICON = 1 << 10;
        const UNUSED = 0xffff_f800;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TextPropsPropertyMask: u32 {
        const FONT_NAME = 1 << 0;
        const FONT_EFFECTS = 1 << 1;
        const FONT_HEIGHT = 1 << 2;
        const UNUSED1 = 1 << 3;
        const FONT_CHAR_SET = 1 << 4;
        const FONT_PITCH_AND_FAMILY = 1 << 5;
        const PARAGRAPH_ALIGN = 1 << 6;
        const FONT_WEIGHT = 1 << 7;
        const UNUSED2 = 0xffff_ff00;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct VariousPropertiesBitfield: u32 {
        const RESERVED1 = 1 << 0;
        const ENABLED = 1 << 1;
        const LOCKED = 1 << 2;
        const BACK_STYLE = 1 << 3;
        const RESERVED2 = 1 << 4;
        const UNUSED1 = 0b1_1111 << 5;
        const COLUMN_HEADS = 1 << 10;
        const INTEGRAL_HEIGHT = 1 << 11;
        const MATCH_REQUIRED = 1 << 12;
        const ALIGNMENT = 1 << 13;
        const EDITABLE = 1 << 14;
        const IME_MODE = 0b1111 << 15;
        const DRAG_BEHAVIOR = 1 << 19;
        const ENTER_KEY_BEHAVIOR = 1 << 20;
        const ENTER_FIELD_BEHAVIOR = 1 << 21;
        const TAB_KEY_BEHAVIOR = 1 << 22;
        const WORD_WRAP = 1 << 23;
        const UNUSED2 = 1 << 24;
        const BORDERS_SUPPRESS = 1 << 25;
        const SELECTION_MARGIN = 1 << 26;
        const AUTO_WORD_SELECT = 1 << 27;
        const AUTO_SIZE = 1 << 28;
        const HIDE_SELECTION = 1 << 29;
        const AUTO_TAB = 1 << 30;
        const MULTI_LINE = 1 << 31;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FmImeMode {
  NoControl,
  On,
  Off,
  Disable,
  Hiragana,
  Katakana,
  KatakanaHalf,
  AlphaFull,
  Alpha,
  HangulFull,
  Hangul,
  HanziFull,
  Hanzi,
  Compatibility(u8),
}

impl FmImeMode {
  pub fn from_raw(value: u8) -> Self {
    match value {
      0x0 => Self::NoControl,
      0x1 => Self::On,
      0x2 => Self::Off,
      0x3 => Self::Disable,
      0x4 => Self::Hiragana,
      0x5 => Self::Katakana,
      0x6 => Self::KatakanaHalf,
      0x7 => Self::AlphaFull,
      0x8 => Self::Alpha,
      0x9 => Self::HangulFull,
      0xa => Self::Hangul,
      0xb => Self::HanziFull,
      0xc => Self::Hanzi,
      value => Self::Compatibility(value & 0x0f),
    }
  }

  pub fn raw(self) -> u8 {
    match self {
      Self::NoControl => 0x0,
      Self::On => 0x1,
      Self::Off => 0x2,
      Self::Disable => 0x3,
      Self::Hiragana => 0x4,
      Self::Katakana => 0x5,
      Self::KatakanaHalf => 0x6,
      Self::AlphaFull => 0x7,
      Self::Alpha => 0x8,
      Self::HangulFull => 0x9,
      Self::Hangul => 0xa,
      Self::HanziFull => 0xb,
      Self::Hanzi => 0xc,
      Self::Compatibility(value) => value & 0x0f,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FmMousePointer {
  Default,
  Arrow,
  Cross,
  IBeam,
  SizeNorthEastSouthWest,
  SizeNorthSouth,
  SizeNorthWestSouthEast,
  SizeWestEast,
  UpArrow,
  HourGlass,
  NoDrop,
  AppStarting,
  Help,
  SizeAll,
  Custom,
  Compatibility(u8),
}

/// The base MS-OAUT `VARENUM` value used by an OFORMS class-table TYPEDESC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariantBaseType {
  Empty,
  Null,
  I16,
  I32,
  F32,
  F64,
  Currency,
  Date,
  Bstr,
  Dispatch,
  Error,
  Bool,
  Variant,
  Unknown,
  Decimal,
  I8,
  U8,
  U16,
  U32,
  I64,
  U64,
  Int,
  UInt,
  Void,
  HResult,
  Pointer,
  SafeArray,
  CArray,
  UserDefined,
  LpStr,
  LpWStr,
  Record,
  IntPtr,
  UIntPtr,
  Compatibility(u16),
}

impl VariantBaseType {
  pub fn from_raw(value: u16) -> Self {
    match value {
      0x0000 => Self::Empty,
      0x0001 => Self::Null,
      0x0002 => Self::I16,
      0x0003 => Self::I32,
      0x0004 => Self::F32,
      0x0005 => Self::F64,
      0x0006 => Self::Currency,
      0x0007 => Self::Date,
      0x0008 => Self::Bstr,
      0x0009 => Self::Dispatch,
      0x000a => Self::Error,
      0x000b => Self::Bool,
      0x000c => Self::Variant,
      0x000d => Self::Unknown,
      0x000e => Self::Decimal,
      0x0010 => Self::I8,
      0x0011 => Self::U8,
      0x0012 => Self::U16,
      0x0013 => Self::U32,
      0x0014 => Self::I64,
      0x0015 => Self::U64,
      0x0016 => Self::Int,
      0x0017 => Self::UInt,
      0x0018 => Self::Void,
      0x0019 => Self::HResult,
      0x001a => Self::Pointer,
      0x001b => Self::SafeArray,
      0x001c => Self::CArray,
      0x001d => Self::UserDefined,
      0x001e => Self::LpStr,
      0x001f => Self::LpWStr,
      0x0024 => Self::Record,
      0x0025 => Self::IntPtr,
      0x0026 => Self::UIntPtr,
      value => Self::Compatibility(value),
    }
  }

  pub fn raw(self) -> u16 {
    match self {
      Self::Empty => 0x0000,
      Self::Null => 0x0001,
      Self::I16 => 0x0002,
      Self::I32 => 0x0003,
      Self::F32 => 0x0004,
      Self::F64 => 0x0005,
      Self::Currency => 0x0006,
      Self::Date => 0x0007,
      Self::Bstr => 0x0008,
      Self::Dispatch => 0x0009,
      Self::Error => 0x000a,
      Self::Bool => 0x000b,
      Self::Variant => 0x000c,
      Self::Unknown => 0x000d,
      Self::Decimal => 0x000e,
      Self::I8 => 0x0010,
      Self::U8 => 0x0011,
      Self::U16 => 0x0012,
      Self::U32 => 0x0013,
      Self::I64 => 0x0014,
      Self::U64 => 0x0015,
      Self::Int => 0x0016,
      Self::UInt => 0x0017,
      Self::Void => 0x0018,
      Self::HResult => 0x0019,
      Self::Pointer => 0x001a,
      Self::SafeArray => 0x001b,
      Self::CArray => 0x001c,
      Self::UserDefined => 0x001d,
      Self::LpStr => 0x001e,
      Self::LpWStr => 0x001f,
      Self::Record => 0x0024,
      Self::IntPtr => 0x0025,
      Self::UIntPtr => 0x0026,
      Self::Compatibility(value) => value,
    }
  }
}

/// A lossless MS-OAUT variant type with the standard array/by-reference modifiers split out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VariantType {
  pub base: VariantBaseType,
  pub array: bool,
  pub by_reference: bool,
}

impl VariantType {
  const ARRAY: u16 = 0x2000;
  const BY_REFERENCE: u16 = 0x4000;

  pub fn from_raw(value: u16) -> Self {
    Self {
      base: VariantBaseType::from_raw(value & !(Self::ARRAY | Self::BY_REFERENCE)),
      array: value & Self::ARRAY != 0,
      by_reference: value & Self::BY_REFERENCE != 0,
    }
  }

  pub fn raw(self) -> u16 {
    self.base.raw()
      | if self.array { Self::ARRAY } else { 0 }
      | if self.by_reference {
        Self::BY_REFERENCE
      } else {
        0
      }
  }

  fn validate(self) -> Result<()> {
    if self.by_reference && matches!(self.base, VariantBaseType::Empty | VariantBaseType::Null) {
      return Err(Error::invalid(
        0,
        "VT_EMPTY and VT_NULL cannot use VT_BYREF",
      ));
    }
    Ok(())
  }
}

/// The two legal MS-OFORMS values of ScrollBar.ProportionalThumb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProportionalThumb {
  Fixed,
  Proportional,
}

impl ProportionalThumb {
  pub fn from_raw(value: i16) -> Result<Self> {
    match value {
      0 => Ok(Self::Fixed),
      -1 => Ok(Self::Proportional),
      _ => Err(Error::invalid(
        0,
        "ProportionalThumb must be 0x0000 or 0xffff",
      )),
    }
  }

  pub fn raw(self) -> i16 {
    match self {
      Self::Fixed => 0,
      Self::Proportional => -1,
    }
  }
}

impl FmMousePointer {
  pub fn from_raw(value: u8) -> Self {
    match value {
      0x00 => Self::Default,
      0x01 => Self::Arrow,
      0x02 => Self::Cross,
      0x03 => Self::IBeam,
      0x06 => Self::SizeNorthEastSouthWest,
      0x07 => Self::SizeNorthSouth,
      0x08 => Self::SizeNorthWestSouthEast,
      0x09 => Self::SizeWestEast,
      0x0a => Self::UpArrow,
      0x0b => Self::HourGlass,
      0x0c => Self::NoDrop,
      0x0d => Self::AppStarting,
      0x0e => Self::Help,
      0x0f => Self::SizeAll,
      0x63 => Self::Custom,
      value => Self::Compatibility(value),
    }
  }

  pub fn raw(self) -> u8 {
    match self {
      Self::Default => 0x00,
      Self::Arrow => 0x01,
      Self::Cross => 0x02,
      Self::IBeam => 0x03,
      Self::SizeNorthEastSouthWest => 0x06,
      Self::SizeNorthSouth => 0x07,
      Self::SizeNorthWestSouthEast => 0x08,
      Self::SizeWestEast => 0x09,
      Self::UpArrow => 0x0a,
      Self::HourGlass => 0x0b,
      Self::NoDrop => 0x0c,
      Self::AppStarting => 0x0d,
      Self::Help => 0x0e,
      Self::SizeAll => 0x0f,
      Self::Custom => 0x63,
      Self::Compatibility(value) => value,
    }
  }
}

macro_rules! form_u32_enum {
    ($name:ident { $($variant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($variant,)+
            Compatibility(u32),
        }

        impl $name {
            pub fn from_raw(value: u32) -> Self {
                match value {
                    $($value => Self::$variant,)+
                    value => Self::Compatibility(value),
                }
            }

            pub fn raw(self) -> u32 {
                match self {
                    $(Self::$variant => $value,)+
                    Self::Compatibility(value) => value,
                }
            }
        }
    };
}

form_u32_enum!(FmBorderStyle {
    None = 0x00,
    Single = 0x01,
});

form_u32_enum!(FmPictureAlignment {
    TopLeft = 0x00,
    TopRight = 0x01,
    Center = 0x02,
    BottomLeft = 0x03,
    BottomRight = 0x04,
});

form_u32_enum!(FmPictureSizeMode {
    Clip = 0x00,
    Stretch = 0x01,
    Zoom = 0x03,
});

form_u32_enum!(FmPicturePosition {
    LeftTop = 0x0002_0000,
    LeftCenter = 0x0005_0003,
    LeftBottom = 0x0008_0006,
    RightTop = 0x0000_0002,
    RightCenter = 0x0003_0005,
    RightBottom = 0x0006_0008,
    AboveLeft = 0x0006_0000,
    AboveCenter = 0x0007_0001,
    AboveRight = 0x0008_0002,
    BelowLeft = 0x0000_0006,
    BelowCenter = 0x0001_0007,
    BelowRight = 0x0002_0008,
    Center = 0x0004_0004,
});

form_u32_enum!(FmSpecialEffect {
    Flat = 0x00,
    Raised = 0x01,
    Sunken = 0x02,
    Etched = 0x03,
    Bump = 0x06,
});

form_u32_enum!(FmOrientation {
    Auto = 0xffff_ffff,
    Vertical = 0x0000_0000,
    Horizontal = 0x0000_0001,
});

form_u32_enum!(FmScrollBars {
    None = 0x00,
    Horizontal = 0x01,
    Vertical = 0x02,
    Both = 0x03,
});

form_u32_enum!(FmDisplayStyle {
    Text = 0x01,
    List = 0x02,
    Combo = 0x03,
    CheckBox = 0x04,
    OptionButton = 0x05,
    Toggle = 0x06,
    DropList = 0x07,
});

form_u32_enum!(FmListStyle {
    Plain = 0x00,
    Option = 0x01,
});

form_u32_enum!(FmMatchEntry {
    FirstLetter = 0x00,
    Complete = 0x01,
    None = 0x02,
});

form_u32_enum!(FmShowDropButtonWhen {
    Never = 0x00,
    Focus = 0x01,
    Always = 0x02,
});

form_u32_enum!(FmDropButtonStyle {
    Plain = 0x00,
    Arrow = 0x01,
    Ellipsis = 0x02,
    Reduce = 0x03,
});

form_u32_enum!(FmMultiSelect {
    Single = 0x00,
    Multi = 0x01,
    Extended = 0x02,
});

form_u32_enum!(FmCycle {
    AllForms = 0x00,
    CurrentForm = 0x02,
});

form_u32_enum!(FmTabOrientation {
    Top = 0x0000_0000,
    Bottom = 0x0000_0001,
    Left = 0x0000_0002,
    Right = 0x0000_0003,
});

form_u32_enum!(FmTabStyle {
    Tabs = 0x0000_0000,
    Buttons = 0x0000_0001,
    None = 0x0000_0002,
});

form_u32_enum!(FmClickControlMode {
    InsertionPoint = 0x00,
    SelectThenInsert = 0x01,
    Inherit = 0xfe,
    Default = 0xff,
});

form_u32_enum!(FmDoubleClickControlMode {
    SelectText = 0x00,
    EditCode = 0x01,
    EditProperties = 0x02,
    Inherit = 0xfe,
});

form_u32_enum!(FmParagraphAlignment {
    Left = 0x01,
    Right = 0x02,
    Center = 0x03,
});

form_u32_enum!(FmTransitionEffect {
    None = 0x00,
    CoverUp = 0x01,
    CoverRightUp = 0x02,
    CoverRight = 0x03,
    CoverRightDown = 0x04,
    CoverDown = 0x05,
    CoverLeftDown = 0x06,
    CoverLeft = 0x07,
    CoverLeftUp = 0x08,
    PushUp = 0x09,
    PushRight = 0x0a,
    PushDown = 0x0b,
    PushLeft = 0x0c,
});

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FmFontEffects: u32 {
        const BOLD = 1 << 0;
        const ITALIC = 1 << 1;
        const UNDERLINE = 1 << 2;
        const STRIKEOUT = 1 << 3;
        const UNUSED1 = 0x0000_1ff0;
        const DISABLED = 1 << 13;
        const UNUSED2 = 0x3fff_c000;
        const AUTO_COLOR = 1 << 30;
        const UNUSED3 = 1 << 31;
    }
}

impl FmFontEffects {
  fn validate(self) -> Result<()> {
    if self.intersects(Self::UNUSED1 | Self::UNUSED2 | Self::UNUSED3) {
      return Err(Error::invalid(0, "fmFontEffects has unused bits set"));
    }
    Ok(())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FmFontPitch {
  Default,
  Fixed,
  Variable,
  Compatibility(u8),
}

impl FmFontPitch {
  pub fn from_raw(value: u8) -> Self {
    match value {
      0 => Self::Default,
      1 => Self::Fixed,
      2 => Self::Variable,
      value => Self::Compatibility(value & 0x0f),
    }
  }

  pub fn raw(self) -> u8 {
    match self {
      Self::Default => 0,
      Self::Fixed => 1,
      Self::Variable => 2,
      Self::Compatibility(value) => value & 0x0f,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FmFontFamily {
  DontCare,
  Roman,
  Swiss,
  Modern,
  Script,
  Decorative,
  Compatibility(u8),
}

impl FmFontFamily {
  pub fn from_raw(value: u8) -> Self {
    match value {
      0 => Self::DontCare,
      1 => Self::Roman,
      2 => Self::Swiss,
      3 => Self::Modern,
      4 => Self::Script,
      5 => Self::Decorative,
      value => Self::Compatibility(value & 0x0f),
    }
  }

  pub fn raw(self) -> u8 {
    match self {
      Self::DontCare => 0,
      Self::Roman => 1,
      Self::Swiss => 2,
      Self::Modern => 3,
      Self::Script => 4,
      Self::Decorative => 5,
      Self::Compatibility(value) => value & 0x0f,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FmFontPitchAndFamily {
  pub pitch: FmFontPitch,
  pub family: FmFontFamily,
}

impl FmFontPitchAndFamily {
  pub fn from_raw(value: u8) -> Self {
    Self {
      pitch: FmFontPitch::from_raw(value & 0x0f),
      family: FmFontFamily::from_raw(value >> 4),
    }
  }

  pub fn raw(self) -> u8 {
    self.pitch.raw() | (self.family.raw() << 4)
  }

  fn validate(self) -> Result<()> {
    if matches!(self.pitch, FmFontPitch::Compatibility(_)) {
      return Err(Error::invalid(0, "fmFontPitch has an invalid value"));
    }
    if matches!(self.family, FmFontFamily::Compatibility(_)) {
      return Err(Error::invalid(0, "fmFontFamily has an invalid value"));
    }
    Ok(())
  }
}

impl VariousPropertiesBitfield {
  pub fn ime_mode(self) -> FmImeMode {
    FmImeMode::from_raw(((self.bits() & Self::IME_MODE.bits()) >> 15) as u8)
  }

  pub fn with_ime_mode(self, value: FmImeMode) -> Self {
    Self::from_bits_retain((self.bits() & !Self::IME_MODE.bits()) | (u32::from(value.raw()) << 15))
  }

  fn validate(self) -> Result<()> {
    if !self.contains(Self::RESERVED1 | Self::RESERVED2) {
      return Err(Error::invalid(
        0,
        "VariousPropertiesBitfield reserved bits must be set",
      ));
    }
    if self.intersects(Self::UNUSED1 | Self::UNUSED2) {
      return Err(Error::invalid(
        0,
        "VariousPropertiesBitfield unused bits must be zero",
      ));
    }
    Ok(())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OleColorType {
  Default,
  PaletteEntry,
  RgbColor,
  SystemPalette,
  Compatibility(u8),
}

impl OleColorType {
  pub fn from_raw(value: u8) -> Self {
    match value {
      0x00 => Self::Default,
      0x01 => Self::PaletteEntry,
      0x02 => Self::RgbColor,
      0x80 => Self::SystemPalette,
      value => Self::Compatibility(value),
    }
  }

  pub fn raw(self) -> u8 {
    match self {
      Self::Default => 0x00,
      Self::PaletteEntry => 0x01,
      Self::RgbColor => 0x02,
      Self::SystemPalette => 0x80,
      Self::Compatibility(value) => value,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RgbColorOrPaletteEntry {
  pub red_and_green_or_palette_index: u16,
  pub blue: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OleColor {
  pub entry: RgbColorOrPaletteEntry,
  pub color_type: OleColorType,
}

impl OleColor {
  pub fn from_raw(value: u32) -> Result<Self> {
    let result = Self {
      entry: RgbColorOrPaletteEntry {
        red_and_green_or_palette_index: value as u16,
        blue: (value >> 16) as u8,
      },
      color_type: OleColorType::from_raw((value >> 24) as u8),
    };
    result.validate()?;
    Ok(result)
  }

  pub fn raw(self) -> u32 {
    u32::from(self.entry.red_and_green_or_palette_index)
      | (u32::from(self.entry.blue) << 16)
      | (u32::from(self.color_type.raw()) << 24)
  }

  pub fn rgb_components(self) -> Option<(u8, u8, u8)> {
    matches!(
      self.color_type,
      OleColorType::Default | OleColorType::RgbColor
    )
    .then(|| {
      (
        self.entry.red_and_green_or_palette_index as u8,
        (self.entry.red_and_green_or_palette_index >> 8) as u8,
        self.entry.blue,
      )
    })
  }

  pub fn palette_index(self) -> Option<u16> {
    matches!(
      self.color_type,
      OleColorType::PaletteEntry | OleColorType::SystemPalette
    )
    .then_some(self.entry.red_and_green_or_palette_index)
  }

  fn validate(self) -> Result<()> {
    if matches!(
      self.color_type,
      OleColorType::PaletteEntry | OleColorType::SystemPalette
    ) && self.entry.blue != 0
    {
      return Err(Error::invalid(
        0,
        "OLE_COLOR palette entry has a nonzero Blue field",
      ));
    }
    Ok(())
  }
}

/// A mask-controlled scalar together with the undefined alignment bytes that
/// physically precede it. Retaining these bytes makes byte-exact round trips
/// possible without treating the complete property block as opaque data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignedValue<T> {
  pub padding_before: Vec<u8>,
  pub value: T,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CountOfBytesWithCompressionFlag {
  pub byte_count: u32,
  pub compressed: bool,
}

impl CountOfBytesWithCompressionFlag {
  const COMPRESSED: u32 = 0x8000_0000;

  fn from_raw(raw: u32) -> Self {
    Self {
      byte_count: raw & !Self::COMPRESSED,
      compressed: raw & Self::COMPRESSED != 0,
    }
  }

  fn to_raw(self) -> Result<u32> {
    if self.byte_count & Self::COMPRESSED != 0 {
      return Err(Error::Limit(
        "MS-OFORMS string byte count exceeds 31 bits".into(),
      ));
    }
    Ok(self.byte_count | if self.compressed { Self::COMPRESSED } else { 0 })
  }
}

/// Exact persisted fmString bytes and their property-level alignment padding.
/// Compressed strings contain one byte per Unicode character; uncompressed
/// strings contain little-endian UTF-16 code units.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FmString {
  pub bytes: Vec<u8>,
  pub padding_after: Vec<u8>,
  pub length_mode: FmStringLengthMode,
}

/// How an fmString byte boundary was obtained. `LowWordCompatibility` retains
/// malformed legacy controls whose 31-bit declared size exceeds cbTextProps,
/// while their low 16 bits describe the physically present string exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FmStringLengthMode {
  #[default]
  Declared,
  LowWordCompatibility,
}

impl FmString {
  /// Decodes this persisted `fmString` according to its property descriptor.
  ///
  /// MS-OFORMS compressed strings store the low byte of each Unicode scalar;
  /// uncompressed strings store little-endian UTF-16 code units.
  pub fn decode(&self, descriptor: CountOfBytesWithCompressionFlag) -> Result<String> {
    self.validate(descriptor)?;
    if descriptor.compressed {
      return Ok(self.bytes.iter().map(|&value| char::from(value)).collect());
    }

    let code_units = self
      .bytes
      .chunks_exact(2)
      .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
      .collect::<Vec<_>>();
    String::from_utf16(&code_units)
      .map_err(|_| Error::invalid(0, "uncompressed MS-OFORMS string is not valid UTF-16"))
  }

  fn validate(&self, descriptor: CountOfBytesWithCompressionFlag) -> Result<()> {
    let actual = u32::try_from(self.bytes.len())
      .map_err(|_| Error::Limit("MS-OFORMS string exceeds u32".into()))?;
    let expected = match self.length_mode {
      FmStringLengthMode::Declared => descriptor.byte_count,
      FmStringLengthMode::LowWordCompatibility => descriptor.byte_count & 0xffff,
    };
    if actual != expected {
      return Err(Error::invalid(
        0,
        "MS-OFORMS string byte count does not match its descriptor",
      ));
    }
    if !descriptor.compressed && !self.bytes.len().is_multiple_of(2) {
      return Err(Error::invalid(
        0,
        "uncompressed MS-OFORMS string has an odd byte count",
      ));
    }
    Ok(())
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FmSize {
  pub width: i32,
  pub height: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FmPosition {
  pub left: i32,
  pub top: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StdFont {
  pub version: u8,
  pub character_set: i16,
  pub flags: StdFontFlags,
  pub weight: i16,
  pub height: u32,
  pub face: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormFont {
  StdFont(StdFont),
  TextProps(Box<TextProps>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuidAndFont {
  pub class_id: Guid,
  pub font: FormFont,
}

impl GuidAndFont {
  pub const STD_FONT_CLASS_ID: Guid = Guid::from_fields(
    0x0be3_5203,
    0x8f91,
    0x11ce,
    [0x9d, 0xe3, 0x00, 0xaa, 0x00, 0x4b, 0xb8, 0x51],
  );
  pub const TEXT_PROPS_CLASS_ID: Guid = Guid::from_fields(
    0xafc2_0920,
    0xda4e,
    0x11ce,
    [0xb9, 0x43, 0x00, 0xaa, 0x00, 0x68, 0x87, 0xb4],
  );
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormDataBlock {
  pub back_color: Option<AlignedValue<OleColor>>,
  pub fore_color: Option<AlignedValue<OleColor>>,
  pub next_available_id: Option<AlignedValue<u32>>,
  pub boolean_properties: Option<AlignedValue<FormFlags>>,
  pub border_style: Option<AlignedValue<FmBorderStyle>>,
  pub mouse_pointer: Option<AlignedValue<FmMousePointer>>,
  pub scroll_bars: Option<AlignedValue<FormScrollBarFlags>>,
  pub group_count: Option<AlignedValue<i32>>,
  pub mouse_icon_marker: Option<AlignedValue<PersistenceMarker>>,
  pub cycle: Option<AlignedValue<FmCycle>>,
  pub special_effect: Option<AlignedValue<FmSpecialEffect>>,
  pub border_color: Option<AlignedValue<OleColor>>,
  pub caption: Option<AlignedValue<CountOfBytesWithCompressionFlag>>,
  pub font_marker: Option<AlignedValue<PersistenceMarker>>,
  pub picture_marker: Option<AlignedValue<PersistenceMarker>>,
  pub zoom: Option<AlignedValue<u32>>,
  pub picture_alignment: Option<AlignedValue<FmPictureAlignment>>,
  pub picture_size_mode: Option<AlignedValue<FmPictureSizeMode>>,
  pub shape_cookie: Option<AlignedValue<u32>>,
  pub draw_buffer: Option<AlignedValue<u32>>,
  pub trailing_padding: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormExtraDataBlock {
  pub displayed_size: Option<FmSize>,
  pub logical_size: Option<FmSize>,
  pub scroll_position: Option<FmPosition>,
  pub caption: Option<FmString>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormStreamData {
  pub mouse_icon: Option<GuidAndPicture>,
  pub font: Option<GuidAndFont>,
  pub picture: Option<GuidAndPicture>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassInfoDataBlock {
  pub class_table_flags: Option<AlignedValue<ClassTableFlags>>,
  pub variable_flags: Option<AlignedValue<VariableFlags>>,
  pub count_of_methods: Option<AlignedValue<u32>>,
  pub dispid_bind: Option<AlignedValue<u32>>,
  pub get_bind_index: Option<AlignedValue<u16>>,
  pub put_bind_index: Option<AlignedValue<u16>>,
  pub bind_type: Option<AlignedValue<VariantType>>,
  pub get_value_index: Option<AlignedValue<u16>>,
  pub put_value_index: Option<AlignedValue<u16>>,
  pub value_type: Option<AlignedValue<VariantType>>,
  pub dispid_rowset: Option<AlignedValue<u32>>,
  pub set_rowset: Option<AlignedValue<u16>>,
  pub trailing_padding: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassInfoExtraDataBlock {
  pub class_id: Option<Guid>,
  pub dispatch_event: Option<Guid>,
  pub default_program: Option<Guid>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteClassInfo {
  pub version: u16,
  pub property_mask: ClassInfoPropertyMask,
  pub data_block: ClassInfoDataBlock,
  pub extra_data_block: ClassInfoExtraDataBlock,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SiteDataBlock {
  pub name: Option<AlignedValue<CountOfBytesWithCompressionFlag>>,
  pub tag: Option<AlignedValue<CountOfBytesWithCompressionFlag>>,
  pub id: Option<AlignedValue<i32>>,
  pub help_context_id: Option<AlignedValue<i32>>,
  pub bit_flags: Option<AlignedValue<SiteFlags>>,
  pub object_stream_size: Option<AlignedValue<u32>>,
  pub tab_index: Option<AlignedValue<i16>>,
  pub clsid_cache_index: Option<AlignedValue<SiteClassIndex>>,
  pub group_id: Option<AlignedValue<u16>>,
  pub control_tip_text: Option<AlignedValue<CountOfBytesWithCompressionFlag>>,
  pub runtime_license_key: Option<AlignedValue<CountOfBytesWithCompressionFlag>>,
  pub control_source: Option<AlignedValue<CountOfBytesWithCompressionFlag>>,
  pub row_source: Option<AlignedValue<CountOfBytesWithCompressionFlag>>,
  pub trailing_padding: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SiteExtraDataBlock {
  pub name: Option<FmString>,
  pub tag: Option<FmString>,
  pub position: Option<FmPosition>,
  pub control_tip_text: Option<FmString>,
  pub runtime_license_key: Option<FmString>,
  pub control_source: Option<FmString>,
  pub row_source: Option<FmString>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OleSiteConcreteControl {
  pub version: u16,
  pub property_mask: SitePropertyMask,
  pub data_block: SiteDataBlock,
  pub extra_data_block: SiteExtraDataBlock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormObjectDepthTypeCount {
  pub depth: u8,
  pub count: u8,
  pub site_type: u8,
  pub compressed_count: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormSiteData {
  pub count_of_site_class_info: Option<u16>,
  pub class_table: Vec<SiteClassInfo>,
  pub count_of_sites: u32,
  pub count_of_bytes: u32,
  pub depths_and_types: Vec<FormObjectDepthTypeCount>,
  pub array_padding: Vec<u8>,
  pub sites: Vec<OleSiteConcreteControl>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesignExtenderDataBlock {
  pub bit_flags: Option<AlignedValue<DesignExtenderFlags>>,
  pub grid_x: Option<AlignedValue<i32>>,
  pub grid_y: Option<AlignedValue<i32>>,
  pub click_control_mode: Option<AlignedValue<FmClickControlMode>>,
  pub double_click_control_mode: Option<AlignedValue<FmDoubleClickControlMode>>,
  pub trailing_padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesignExtender {
  pub minor_version: u8,
  pub major_version: u8,
  pub property_mask: DesignExtenderPropertyMask,
  pub data_block: DesignExtenderDataBlock,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormControl {
  pub minor_version: u8,
  pub major_version: u8,
  pub property_mask: FormPropertyMask,
  pub picture_tiling: bool,
  pub data_block: FormDataBlock,
  pub extra_data_block: FormExtraDataBlock,
  pub stream_data: FormStreamData,
  pub site_data: FormSiteData,
  pub design_extender: Option<DesignExtender>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageDataBlock {
  pub transition_effect: Option<AlignedValue<FmTransitionEffect>>,
  pub transition_period: Option<AlignedValue<u32>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageProperties {
  pub minor_version: u8,
  pub major_version: u8,
  pub property_mask: PagePropertyMask,
  pub data_block: PageDataBlock,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultiPagePropertiesDataBlock {
  pub page_count: Option<AlignedValue<i32>>,
  pub id: Option<AlignedValue<i32>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiPageProperties {
  pub minor_version: u8,
  pub major_version: u8,
  pub property_mask: MultiPagePropertyMask,
  pub flags: bool,
  pub data_block: MultiPagePropertiesDataBlock,
  pub page_ids: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiPageXStream {
  pub pages: Vec<PageProperties>,
  pub multi_page: MultiPageProperties,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabStripDataBlock {
  pub list_index: Option<AlignedValue<i32>>,
  pub back_color: Option<AlignedValue<OleColor>>,
  pub fore_color: Option<AlignedValue<OleColor>>,
  pub items_size: Option<AlignedValue<u32>>,
  pub mouse_pointer: Option<AlignedValue<FmMousePointer>>,
  pub tab_orientation: Option<AlignedValue<FmTabOrientation>>,
  pub tab_style: Option<AlignedValue<FmTabStyle>>,
  pub tab_fixed_width: Option<AlignedValue<u32>>,
  pub tab_fixed_height: Option<AlignedValue<u32>>,
  pub tip_strings_size: Option<AlignedValue<u32>>,
  pub names_size: Option<AlignedValue<u32>>,
  pub various_property_bits: Option<AlignedValue<VariousPropertiesBitfield>>,
  pub tabs_allocated: Option<AlignedValue<u32>>,
  pub tags_size: Option<AlignedValue<u32>>,
  pub tab_data_count: Option<AlignedValue<u32>>,
  pub accelerators_size: Option<AlignedValue<u32>>,
  pub mouse_icon_marker: Option<AlignedValue<PersistenceMarker>>,
  pub trailing_padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArrayString {
  pub character_count: u32,
  pub compressed: bool,
  pub bytes: Vec<u8>,
  pub padding_after: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabStripExtraDataBlock {
  pub size: Option<FmSize>,
  pub items: Option<Vec<ArrayString>>,
  pub tip_strings: Option<Vec<ArrayString>>,
  pub tab_names: Option<Vec<ArrayString>>,
  pub tags: Option<Vec<ArrayString>>,
  pub accelerators: Option<Vec<ArrayString>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabStripStreamData {
  pub mouse_icon: Option<GuidAndPicture>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabStripControl {
  pub minor_version: u8,
  pub major_version: u8,
  pub property_mask: TabStripPropertyMask,
  pub multi_row: bool,
  pub tooltips: bool,
  pub new_version: bool,
  pub data_block: TabStripDataBlock,
  pub extra_data_block: TabStripExtraDataBlock,
  pub stream_data: TabStripStreamData,
  pub text_props: TextProps,
  pub tab_flags: Vec<TabStripTabFlags>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageDataBlock {
  pub border_color: Option<AlignedValue<OleColor>>,
  pub back_color: Option<AlignedValue<OleColor>>,
  pub border_style: Option<AlignedValue<FmBorderStyle>>,
  pub mouse_pointer: Option<AlignedValue<FmMousePointer>>,
  pub picture_size_mode: Option<AlignedValue<FmPictureSizeMode>>,
  pub special_effect: Option<AlignedValue<FmSpecialEffect>>,
  pub picture_marker: Option<AlignedValue<PersistenceMarker>>,
  pub picture_alignment: Option<AlignedValue<FmPictureAlignment>>,
  pub various_property_bits: Option<AlignedValue<VariousPropertiesBitfield>>,
  pub mouse_icon_marker: Option<AlignedValue<PersistenceMarker>>,
  pub trailing_padding: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PictureStreamData {
  pub picture: Option<GuidAndPicture>,
  pub mouse_icon: Option<GuidAndPicture>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageControl {
  pub minor_version: u8,
  pub major_version: u8,
  pub property_mask: ImagePropertyMask,
  pub auto_size: bool,
  pub picture_tiling: bool,
  pub data_block: ImageDataBlock,
  pub size: FmSize,
  pub stream_data: PictureStreamData,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelDataBlock {
  pub fore_color: Option<AlignedValue<OleColor>>,
  pub back_color: Option<AlignedValue<OleColor>>,
  pub various_property_bits: Option<AlignedValue<VariousPropertiesBitfield>>,
  pub caption: Option<AlignedValue<CountOfBytesWithCompressionFlag>>,
  pub picture_position: Option<AlignedValue<FmPicturePosition>>,
  pub mouse_pointer: Option<AlignedValue<FmMousePointer>>,
  pub border_color: Option<AlignedValue<OleColor>>,
  pub border_style: Option<AlignedValue<FmBorderStyle>>,
  pub special_effect: Option<AlignedValue<FmSpecialEffect>>,
  pub picture_marker: Option<AlignedValue<PersistenceMarker>>,
  pub accelerator: Option<AlignedValue<u16>>,
  pub mouse_icon_marker: Option<AlignedValue<PersistenceMarker>>,
  pub trailing_padding: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelExtraDataBlock {
  pub caption: Option<FmString>,
  pub size: Option<FmSize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelControl {
  pub minor_version: u8,
  pub major_version: u8,
  pub property_mask: LabelPropertyMask,
  pub data_block: LabelDataBlock,
  pub extra_data_block: LabelExtraDataBlock,
  pub stream_data: PictureStreamData,
  pub text_props: TextProps,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpinButtonDataBlock {
  pub fore_color: Option<AlignedValue<OleColor>>,
  pub back_color: Option<AlignedValue<OleColor>>,
  pub various_property_bits: Option<AlignedValue<VariousPropertiesBitfield>>,
  pub min: Option<AlignedValue<i32>>,
  pub max: Option<AlignedValue<i32>>,
  pub position: Option<AlignedValue<i32>>,
  pub prev_enabled: Option<AlignedValue<EnabledState>>,
  pub next_enabled: Option<AlignedValue<EnabledState>>,
  pub small_change: Option<AlignedValue<i32>>,
  pub orientation: Option<AlignedValue<FmOrientation>>,
  pub delay: Option<AlignedValue<u32>>,
  pub mouse_icon_marker: Option<AlignedValue<PersistenceMarker>>,
  pub mouse_pointer: Option<AlignedValue<FmMousePointer>>,
  pub trailing_padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpinButtonControl {
  pub minor_version: u8,
  pub major_version: u8,
  pub property_mask: SpinButtonPropertyMask,
  pub data_block: SpinButtonDataBlock,
  pub size: FmSize,
  pub mouse_icon: Option<GuidAndPicture>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrollBarDataBlock {
  pub fore_color: Option<AlignedValue<OleColor>>,
  pub back_color: Option<AlignedValue<OleColor>>,
  pub various_property_bits: Option<AlignedValue<VariousPropertiesBitfield>>,
  pub mouse_pointer: Option<AlignedValue<FmMousePointer>>,
  pub min: Option<AlignedValue<i32>>,
  pub max: Option<AlignedValue<i32>>,
  pub position: Option<AlignedValue<i32>>,
  pub prev_enabled: Option<AlignedValue<EnabledState>>,
  pub next_enabled: Option<AlignedValue<EnabledState>>,
  pub small_change: Option<AlignedValue<i32>>,
  pub large_change: Option<AlignedValue<i32>>,
  pub orientation: Option<AlignedValue<FmOrientation>>,
  pub proportional_thumb: Option<AlignedValue<ProportionalThumb>>,
  pub delay: Option<AlignedValue<u32>>,
  pub mouse_icon_marker: Option<AlignedValue<PersistenceMarker>>,
  pub trailing_padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrollBarControl {
  pub minor_version: u8,
  pub major_version: u8,
  pub property_mask: ScrollBarPropertyMask,
  pub data_block: ScrollBarDataBlock,
  pub size: FmSize,
  pub mouse_icon: Option<GuidAndPicture>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormControlPersistence {
  Image(Box<ImageControl>),
  Label(Box<LabelControl>),
  SpinButton(Box<SpinButtonControl>),
  ScrollBar(Box<ScrollBarControl>),
  CommandButton(Box<CommandButtonControl>),
  TabStrip(Box<TabStripControl>),
  MorphData(Box<MorphDataControl>),
  ExternalClass(ExternalComPersistStream),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormObjectControl {
  pub site_index: usize,
  pub class_index: SiteClassIndex,
  pub persistence: FormControlPersistence,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormObjectStream {
  pub controls: Vec<FormObjectControl>,
}

/// The statically parsed MS-OFORMS streams and embedded parent storages below one CFB storage.
///
/// Entries other than the format-defined `f`, `o`, and MultiPage `x` streams remain owned by the
/// [`CompoundFile`] and are left untouched by [`Self::write_to_compound`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentControlStorage {
  pub path: PathBuf,
  pub class_id: Guid,
  pub form: FormControl,
  pub object_stream: FormObjectStream,
  pub multi_page_x: Option<MultiPageXStream>,
  pub children: Vec<EmbeddedParentControl>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedParentControl {
  pub site_index: usize,
  pub storage_name: String,
  pub storage: Box<ParentControlStorage>,
}

/// Storage-neutral recursive MS-OFORMS parent-control model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentControlStorageModel {
  pub class_id: Guid,
  pub form: FormControl,
  pub object_stream: FormObjectStream,
  pub multi_page_x: Option<MultiPageXStream>,
  pub children: Vec<EmbeddedParentControlModel>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedParentControlModel {
  pub site_index: usize,
  pub storage_name: String,
  pub storage: Box<ParentControlStorageModel>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentControlStorageCfbIdentity {
  pub path: PathBuf,
  pub class_id: Guid,
  pub children: Vec<ParentControlStorageCfbIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedParentControlStorage {
  identity: ParentControlStorageCfbIdentity,
  model: ParentControlStorageModel,
  source_model: ParentControlStorageModel,
}

impl LocatedParentControlStorage {
  pub fn from_compound(compound: &CompoundFile, path: impl AsRef<Path>) -> Result<Self> {
    Ok(ParentControlStorage::from_compound(compound, path)?.into())
  }

  pub const fn model(&self) -> &ParentControlStorageModel {
    &self.model
  }

  pub const fn identity(&self) -> &ParentControlStorageCfbIdentity {
    &self.identity
  }

  pub fn is_modified(&self) -> bool {
    self.model != self.source_model
  }

  pub fn edit<T>(
    &mut self,
    edit: impl FnOnce(&mut ParentControlStorageModel) -> Result<T>,
  ) -> Result<T> {
    let mut candidate = self.model.clone();
    let result = edit(&mut candidate)?;
    let legacy = parent_storage_from_model(&self.identity, &candidate)?;
    legacy.validate_tree(0)?;
    self.model = candidate;
    Ok(result)
  }

  pub(crate) fn write_if_modified(&self, compound: &mut CompoundFile) -> Result<()> {
    if !self.is_modified() {
      return Ok(());
    }
    let legacy = parent_storage_from_model(&self.identity, &self.model)?;
    let mut candidate = compound.clone();
    legacy.write_to_compound(&mut candidate)?;
    *compound = candidate;
    Ok(())
  }

  /// Discovers the outermost MS-OFORMS parent-control storage roots below a
  /// CFB storage.
  ///
  /// A root is identified by its directly owned `f` and `o` streams rather
  /// than by its CFB CLSID: UserForm storages commonly use a zero CLSID.
  /// Embedded Page, Frame, and MultiPage storages are owned recursively by
  /// their outer root and therefore are not returned a second time.
  pub fn discover_root_paths_below(compound: &CompoundFile, project_root: &Path) -> Vec<PathBuf> {
    let mut candidates = compound
      .entries()
      .iter()
      .filter(|entry| {
        entry.is_storage()
          && entry.path != project_root
          && entry.path.starts_with(project_root)
          && compound.entries().iter().any(|child| {
            child.is_stream()
              && child.path.parent() == Some(entry.path.as_path())
              && child.name.eq_ignore_ascii_case(FORM_STREAM_NAME)
          })
          && compound.entries().iter().any(|child| {
            child.is_stream()
              && child.path.parent() == Some(entry.path.as_path())
              && child.name.eq_ignore_ascii_case(OBJECT_STREAM_NAME)
          })
      })
      .map(|entry| entry.path.clone())
      .collect::<Vec<_>>();
    candidates.sort_by_key(|path| path.components().count());

    let mut roots = Vec::<PathBuf>::new();
    for path in candidates {
      if roots
        .iter()
        .any(|root| path != *root && path.starts_with(root))
      {
        continue;
      }
      roots.push(path);
    }
    roots.sort();
    roots
  }

  /// Parses every root returned by [`Self::discover_root_paths_below`].
  pub fn discover_below(compound: &CompoundFile, project_root: &Path) -> Result<Vec<Self>> {
    Self::discover_root_paths_below(compound, project_root)
      .into_iter()
      .map(|path| Self::from_compound(compound, path))
      .collect()
  }
}

impl From<ParentControlStorage> for LocatedParentControlStorage {
  fn from(storage: ParentControlStorage) -> Self {
    let (identity, model) = split_parent_storage(storage);
    Self {
      identity,
      source_model: model.clone(),
      model,
    }
  }
}

fn split_parent_storage(
  storage: ParentControlStorage,
) -> (ParentControlStorageCfbIdentity, ParentControlStorageModel) {
  let mut child_identities = Vec::with_capacity(storage.children.len());
  let mut child_models = Vec::with_capacity(storage.children.len());
  for child in storage.children {
    let (identity, model) = split_parent_storage(*child.storage);
    child_identities.push(identity);
    child_models.push(EmbeddedParentControlModel {
      site_index: child.site_index,
      storage_name: child.storage_name,
      storage: Box::new(model),
    });
  }
  (
    ParentControlStorageCfbIdentity {
      path: storage.path,
      class_id: storage.class_id,
      children: child_identities,
    },
    ParentControlStorageModel {
      class_id: storage.class_id,
      form: storage.form,
      object_stream: storage.object_stream,
      multi_page_x: storage.multi_page_x,
      children: child_models,
    },
  )
}

fn parent_storage_from_model(
  identity: &ParentControlStorageCfbIdentity,
  model: &ParentControlStorageModel,
) -> Result<ParentControlStorage> {
  if identity.class_id != model.class_id {
    return Err(Error::invalid(
      0,
      "Forms storage CLSID does not match its stable CFB identity",
    ));
  }
  if identity.children.len() != model.children.len() {
    return Err(Error::invalid(
      0,
      "Forms child model and CFB identity counts differ",
    ));
  }
  let children = model
    .children
    .iter()
    .zip(&identity.children)
    .map(|(child, child_identity)| {
      let expected_path = identity.path.join(&child.storage_name);
      if child_identity.path != expected_path {
        return Err(Error::invalid(
          0,
          "Forms child storage name does not match its stable CFB identity",
        ));
      }
      Ok(EmbeddedParentControl {
        site_index: child.site_index,
        storage_name: child.storage_name.clone(),
        storage: Box::new(parent_storage_from_model(child_identity, &child.storage)?),
      })
    })
    .collect::<Result<Vec<_>>>()?;
  Ok(ParentControlStorage {
    path: identity.path.clone(),
    class_id: model.class_id,
    form: model.form.clone(),
    object_stream: model.object_stream.clone(),
    multi_page_x: model.multi_page_x.clone(),
    children,
  })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MorphDataDataBlock {
  pub various_property_bits: Option<AlignedValue<VariousPropertiesBitfield>>,
  pub back_color: Option<AlignedValue<OleColor>>,
  pub fore_color: Option<AlignedValue<OleColor>>,
  pub max_length: Option<AlignedValue<u32>>,
  pub border_style: Option<AlignedValue<FmBorderStyle>>,
  pub scroll_bars: Option<AlignedValue<FmScrollBars>>,
  pub display_style: Option<AlignedValue<FmDisplayStyle>>,
  pub mouse_pointer: Option<AlignedValue<FmMousePointer>>,
  pub password_char: Option<AlignedValue<u16>>,
  pub list_width: Option<AlignedValue<u32>>,
  pub bound_column: Option<AlignedValue<u16>>,
  pub text_column: Option<AlignedValue<i16>>,
  pub column_count: Option<AlignedValue<i16>>,
  pub list_rows: Option<AlignedValue<u16>>,
  pub column_info_count: Option<AlignedValue<u16>>,
  pub match_entry: Option<AlignedValue<FmMatchEntry>>,
  pub list_style: Option<AlignedValue<FmListStyle>>,
  pub show_drop_button_when: Option<AlignedValue<FmShowDropButtonWhen>>,
  pub drop_button_style: Option<AlignedValue<FmDropButtonStyle>>,
  pub multi_select: Option<AlignedValue<FmMultiSelect>>,
  pub value: Option<AlignedValue<CountOfBytesWithCompressionFlag>>,
  pub caption: Option<AlignedValue<CountOfBytesWithCompressionFlag>>,
  pub picture_position: Option<AlignedValue<FmPicturePosition>>,
  pub border_color: Option<AlignedValue<OleColor>>,
  pub special_effect: Option<AlignedValue<FmSpecialEffect>>,
  pub mouse_icon_marker: Option<AlignedValue<PersistenceMarker>>,
  pub picture_marker: Option<AlignedValue<PersistenceMarker>>,
  pub accelerator: Option<AlignedValue<u16>>,
  pub group_name: Option<AlignedValue<CountOfBytesWithCompressionFlag>>,
  pub trailing_padding: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MorphDataExtraDataBlock {
  pub size: Option<FmSize>,
  pub value: Option<FmString>,
  pub caption: Option<FmString>,
  pub group_name: Option<FmString>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuidAndPicture {
  pub class_id: Guid,
  pub preamble: u32,
  pub picture: Vec<u8>,
}

impl GuidAndPicture {
  pub const STD_PICTURE_CLASS_ID: Guid = Guid::from_fields(
    0x0be3_5204,
    0x8f91,
    0x11ce,
    [0x9d, 0xe3, 0x00, 0xaa, 0x00, 0x4b, 0xb8, 0x51],
  );
  pub const PREAMBLE: u32 = 0x0000_746c;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MorphDataStreamData {
  pub mouse_icon: Option<GuidAndPicture>,
  pub picture: Option<GuidAndPicture>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandButtonDataBlock {
  pub fore_color: Option<AlignedValue<OleColor>>,
  pub back_color: Option<AlignedValue<OleColor>>,
  pub various_property_bits: Option<AlignedValue<VariousPropertiesBitfield>>,
  pub caption: Option<AlignedValue<CountOfBytesWithCompressionFlag>>,
  pub picture_position: Option<AlignedValue<FmPicturePosition>>,
  pub mouse_pointer: Option<AlignedValue<FmMousePointer>>,
  pub picture_marker: Option<AlignedValue<PersistenceMarker>>,
  pub accelerator: Option<AlignedValue<u16>>,
  pub mouse_icon_marker: Option<AlignedValue<PersistenceMarker>>,
  pub trailing_padding: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandButtonExtraDataBlock {
  pub caption: Option<FmString>,
  pub size: Option<FmSize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandButtonStreamData {
  pub picture: Option<GuidAndPicture>,
  pub mouse_icon: Option<GuidAndPicture>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandButtonControl {
  pub minor_version: u8,
  pub major_version: u8,
  pub property_mask: CommandButtonPropertyMask,
  pub take_focus_on_click: bool,
  pub data_block: CommandButtonDataBlock,
  pub extra_data_block: CommandButtonExtraDataBlock,
  pub stream_data: CommandButtonStreamData,
  pub text_props: TextProps,
}

/// Persistence bytes owned by an external COM class rather than a Microsoft
/// Office binary file-format specification. The containing Office framing and
/// class identity remain statically typed; applications may dispatch these
/// bytes to a class-specific parser without olecfsdk loading or invoking COM.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExternalComPersistStream {
  pub bytes: Vec<u8>,
}

/// MS-DOC single-stream OLE control framing used by the `\x03OCXDATA` stream.
/// Office writes the class identifier before the bytes passed to the control's
/// implementation-specific persistence interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SingleStreamOleControl {
  pub class_id: Guid,
  pub persistence: ExternalComPersistStream,
}

impl SingleStreamOleControl {
  /// Microsoft Scriptlet Component (`ScriptBridge.ScriptBridge.1`). This
  /// class is kill-bitted on supported Windows versions and occurs in the
  /// corpus only in the crafted CVE-2015-0097 fixture.
  pub const SCRIPTLET_COMPONENT_CLASS_ID: Guid = Guid::from_fields(
    0xae24_fdae,
    0x03c6,
    0x11d1,
    [0x8b, 0x76, 0x00, 0x80, 0xc7, 0x44, 0xf3, 0x89],
  );

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 16 {
      return Err(Error::invalid(
        0,
        "single-stream OLE control is missing its CLSID",
      ));
    }
    let mut cursor = SliceCursor::new(bytes);
    let class_id = cursor.read_guid()?;
    Ok(Self {
      class_id,
      persistence: ExternalComPersistStream {
        bytes: cursor.read_vec(cursor.end - cursor.position)?,
      },
    })
  }

  pub fn to_bytes(&self) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + self.persistence.bytes.len());
    write_guid(&mut bytes, self.class_id);
    bytes.extend_from_slice(&self.persistence.bytes);
    bytes
  }

  pub fn is_scriptlet_component(&self) -> bool {
    self.class_id == Self::SCRIPTLET_COMPONENT_CLASS_ID
  }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextPropsDataBlock {
  pub font_name: Option<AlignedValue<CountOfBytesWithCompressionFlag>>,
  pub font_effects: Option<AlignedValue<FmFontEffects>>,
  pub font_height: Option<AlignedValue<u32>>,
  pub font_char_set: Option<AlignedValue<u8>>,
  pub font_pitch_and_family: Option<AlignedValue<FmFontPitchAndFamily>>,
  pub paragraph_align: Option<AlignedValue<FmParagraphAlignment>>,
  pub font_weight: Option<AlignedValue<u16>>,
  pub trailing_padding: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextPropsExtraDataBlock {
  pub font_name: Option<FmString>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextProps {
  pub minor_version: u8,
  pub major_version: u8,
  pub property_mask: TextPropsPropertyMask,
  pub data_block: TextPropsDataBlock,
  pub extra_data_block: TextPropsExtraDataBlock,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MorphDataColumnInfo {
  pub minor_version: u8,
  pub major_version: u8,
  pub property_mask: MorphDataColumnInfoPropertyMask,
  pub column_width: Option<AlignedValue<i32>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MorphDataControl {
  pub minor_version: u8,
  pub major_version: u8,
  pub property_mask: MorphDataPropertyMask,
  pub data_block: MorphDataDataBlock,
  pub extra_data_block: MorphDataExtraDataBlock,
  pub stream_data: MorphDataStreamData,
  pub text_props: TextProps,
  pub column_info: Vec<MorphDataColumnInfo>,
}

trait FormScalar<Repr> {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()>;
}

macro_rules! impl_form_scalar {
    ($($type:ty),+ $(,)?) => {
        $(
            impl FormScalar<$type> for $type {
                fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
                    bytes.extend_from_slice(&self.to_le_bytes());
                    Ok(())
                }
            }
        )+
    };
}

impl_form_scalar!(u8, u16, u32, i16, i32);

impl FormScalar<u32> for OleColor {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    bytes.extend_from_slice(&self.raw().to_le_bytes());
    Ok(())
  }
}

impl FormScalar<u32> for VariousPropertiesBitfield {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    bytes.extend_from_slice(&self.bits().to_le_bytes());
    Ok(())
  }
}

impl FormScalar<u32> for FormFlags {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    bytes.extend_from_slice(&self.bits().to_le_bytes());
    Ok(())
  }
}

impl FormScalar<u32> for SiteFlags {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    bytes.extend_from_slice(&self.bits().to_le_bytes());
    Ok(())
  }
}

impl FormScalar<u32> for DesignExtenderFlags {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    bytes.extend_from_slice(&self.bits().to_le_bytes());
    Ok(())
  }
}

impl FormScalar<u16> for ClassTableFlags {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    bytes.extend_from_slice(&self.bits().to_le_bytes());
    Ok(())
  }
}

impl FormScalar<u16> for VariableFlags {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    bytes.extend_from_slice(&self.bits().to_le_bytes());
    Ok(())
  }
}

impl FormScalar<u16> for VariantType {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    bytes.extend_from_slice(&self.raw().to_le_bytes());
    Ok(())
  }
}

impl FormScalar<i16> for ProportionalThumb {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    bytes.extend_from_slice(&self.raw().to_le_bytes());
    Ok(())
  }
}

impl FormScalar<i32> for EnabledState {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    bytes.extend_from_slice(&self.raw().to_le_bytes());
    Ok(())
  }
}

impl FormScalar<u16> for SiteClassIndex {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    bytes.extend_from_slice(&self.to_raw()?.to_le_bytes());
    Ok(())
  }
}

impl FormScalar<u16> for PersistenceMarker {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    bytes.extend_from_slice(&PersistenceMarker::RAW.to_le_bytes());
    Ok(())
  }
}

impl FormScalar<u8> for FmMousePointer {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    bytes.push(self.raw());
    Ok(())
  }
}

impl FormScalar<u8> for FormScrollBarFlags {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    bytes.push(self.bits());
    Ok(())
  }
}

impl FormScalar<u32> for FmFontEffects {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    bytes.extend_from_slice(&self.bits().to_le_bytes());
    Ok(())
  }
}

impl FormScalar<u8> for FmFontPitchAndFamily {
  fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
    self.validate()?;
    bytes.push(self.raw());
    Ok(())
  }
}

macro_rules! impl_form_enum_scalar {
  ($type:ty, $repr:ty) => {
    impl FormScalar<$repr> for $type {
      fn write_le(&self, bytes: &mut Vec<u8>) -> Result<()> {
        let raw = <$repr>::try_from(self.raw()).map_err(|_| {
          Error::Limit(format!(
            "{} value does not fit {}",
            stringify!($type),
            stringify!($repr)
          ))
        })?;
        bytes.extend_from_slice(&raw.to_le_bytes());
        Ok(())
      }
    }
  };
}

impl_form_enum_scalar!(FmBorderStyle, u8);
impl_form_enum_scalar!(FmBorderStyle, u16);
impl_form_enum_scalar!(FmPictureAlignment, u8);
impl_form_enum_scalar!(FmPictureSizeMode, u8);
impl_form_enum_scalar!(FmPicturePosition, u32);
impl_form_enum_scalar!(FmSpecialEffect, u8);
impl_form_enum_scalar!(FmSpecialEffect, u16);
impl_form_enum_scalar!(FmSpecialEffect, u32);
impl_form_enum_scalar!(FmOrientation, u32);
impl_form_enum_scalar!(FmScrollBars, u8);
impl_form_enum_scalar!(FmDisplayStyle, u8);
impl_form_enum_scalar!(FmListStyle, u8);
impl_form_enum_scalar!(FmMatchEntry, u8);
impl_form_enum_scalar!(FmShowDropButtonWhen, u8);
impl_form_enum_scalar!(FmDropButtonStyle, u8);
impl_form_enum_scalar!(FmMultiSelect, u8);
impl_form_enum_scalar!(FmCycle, u8);
impl_form_enum_scalar!(FmTabOrientation, u32);
impl_form_enum_scalar!(FmTabStyle, u32);
impl_form_enum_scalar!(FmClickControlMode, u8);
impl_form_enum_scalar!(FmDoubleClickControlMode, u8);
impl_form_enum_scalar!(FmParagraphAlignment, u8);
impl_form_enum_scalar!(FmTransitionEffect, u32);

macro_rules! read_optional {
  ($target:ident, $cursor:ident, $mask:ident, $flag:ident, $field:ident, $alignment:expr, $method:ident) => {
    if $mask.contains(Mask::$flag) {
      $target.$field = Some(AlignedValue {
        padding_before: $cursor.read_alignment($alignment)?,
        value: $cursor.$method()?,
      });
    }
  };
}

macro_rules! read_descriptor {
  ($target:ident, $cursor:ident, $mask:ident, $flag:ident, $field:ident) => {
    if $mask.contains(MorphDataPropertyMask::$flag) {
      $target.$field = Some(AlignedValue {
        padding_before: $cursor.read_alignment(4)?,
        value: CountOfBytesWithCompressionFlag::from_raw($cursor.read_u32()?),
      });
    }
  };
}

macro_rules! write_optional {
  ($target:expr, $bytes:ident, $mask:expr, $flag:ident, $field:ident, $type:ty) => {
    match ($mask.contains(Mask::$flag), ($target).$field.as_ref()) {
      (true, Some(value)) => {
        append_padding(
          $bytes,
          &value.padding_before,
          std::mem::size_of::<$type>(),
          stringify!($field),
        )?;
        <_ as FormScalar<$type>>::write_le(&value.value, $bytes)?;
      }
      (false, None) => {}
      _ => return Err(mask_field_mismatch("property block", stringify!($field))),
    }
  };
}

macro_rules! write_descriptor {
  ($target:ident, $bytes:ident, $mask:ident, $flag:ident, $field:ident) => {
    match (
      $mask.contains(MorphDataPropertyMask::$flag),
      $target.$field.as_ref(),
    ) {
      (true, Some(value)) => {
        append_padding($bytes, &value.padding_before, 4, stringify!($field))?;
        $bytes.extend_from_slice(&value.value.to_raw()?.to_le_bytes());
      }
      (false, None) => {}
      _ => return Err(mask_field_mismatch("MorphData", stringify!($field))),
    }
  };
}

impl FormControl {
  const MASK_SIZE: usize = 4;

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let (value, consumed) = Self::from_prefix(bytes)?;
    if consumed != bytes.len() {
      return Err(Error::invalid(
        consumed as u64,
        "unexpected bytes after MS-OFORMS FormControl",
      ));
    }
    Ok(value)
  }

  fn from_prefix(bytes: &[u8]) -> Result<(Self, usize)> {
    if bytes.len() < 4 + Self::MASK_SIZE {
      return Err(Error::invalid(0, "truncated MS-OFORMS FormControl"));
    }
    let mut cursor = SliceCursor::new(bytes);
    let minor_version = cursor.read_u8()?;
    let major_version = cursor.read_u8()?;
    if minor_version != 0 || major_version != 4 {
      return Err(Error::invalid(0, "FormControl version must be 0.4"));
    }
    let property_size = usize::from(cursor.read_u16()?);
    if property_size < Self::MASK_SIZE {
      return Err(Error::invalid(2, "cbForm is smaller than FormPropMask"));
    }
    let property_end = 4usize
      .checked_add(property_size)
      .ok_or_else(|| Error::Limit("FormControl property boundary overflow".into()))?;
    if property_end > bytes.len() {
      return Err(Error::invalid(2, "cbForm exceeds the form stream"));
    }
    cursor.end = property_end;
    let property_mask = FormPropertyMask::from_bits_retain(cursor.read_u32()?);
    validate_form_mask(property_mask)?;
    let data_block = FormDataBlock::read(&mut cursor, property_mask)?;
    let extra_data_block = FormExtraDataBlock::read(&mut cursor, property_mask, &data_block)?;
    if cursor.position != property_end {
      return Err(Error::invalid(
        cursor.position as u64,
        "Form property fields do not consume cbForm",
      ));
    }

    cursor.end = bytes.len();
    let stream_data = FormStreamData::read(&mut cursor, property_mask)?;
    let flags = data_block.form_flags()?;
    let (site_data, site_size) = FormSiteData::from_prefix(
      &bytes[cursor.position..],
      flags.contains(FormFlags::DONT_SAVE_CLASS_TABLE),
    )?;
    cursor.position += site_size;
    let design_extender = if flags.contains(FormFlags::DESIGN_EXTENDER_PERSISTED) {
      let (value, size) = DesignExtender::from_prefix(&bytes[cursor.position..])?;
      cursor.position += size;
      Some(value)
    } else {
      None
    };
    Ok((
      Self {
        minor_version,
        major_version,
        property_mask,
        picture_tiling: property_mask.contains(FormPropertyMask::PICTURE_TILING),
        data_block,
        extra_data_block,
        stream_data,
        site_data,
        design_extender,
      },
      cursor.position,
    ))
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.minor_version != 0 || self.major_version != 4 {
      return Err(Error::invalid(0, "FormControl version must be 0.4"));
    }
    validate_form_mask(self.property_mask)?;
    if self.picture_tiling
      != self
        .property_mask
        .contains(FormPropertyMask::PICTURE_TILING)
    {
      return Err(Error::invalid(
        0,
        "Form PictureTiling does not match its property mask bit",
      ));
    }
    let flags = self.data_block.form_flags()?;
    if flags.contains(FormFlags::DESIGN_EXTENDER_PERSISTED) != self.design_extender.is_some() {
      return Err(mask_field_mismatch("FormControl", "DesignExtender"));
    }
    let mut bytes = vec![self.minor_version, self.major_version, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    self.data_block.write(&mut bytes, self.property_mask)?;
    self
      .extra_data_block
      .write(&mut bytes, self.property_mask, &self.data_block)?;
    let property_size =
      u16::try_from(bytes.len() - 4).map_err(|_| Error::Limit("cbForm exceeds u16".into()))?;
    bytes[2..4].copy_from_slice(&property_size.to_le_bytes());
    self.stream_data.write(&mut bytes, self.property_mask)?;
    bytes.extend_from_slice(
      &self
        .site_data
        .to_bytes(flags.contains(FormFlags::DONT_SAVE_CLASS_TABLE))?,
    );
    if let Some(design_extender) = &self.design_extender {
      bytes.extend_from_slice(&design_extender.to_bytes()?);
    }
    Ok(bytes)
  }
}

impl PageProperties {
  fn from_prefix(bytes: &[u8]) -> Result<(Self, usize)> {
    if bytes.len() < 8 {
      return Err(Error::invalid(0, "truncated PageProperties"));
    }
    let mut cursor = SliceCursor::new(bytes);
    let minor_version = cursor.read_u8()?;
    let major_version = cursor.read_u8()?;
    validate_version(minor_version, major_version, "PageProperties")?;
    let size = usize::from(cursor.read_u16()?);
    if size < 4 {
      return Err(Error::invalid(2, "cbPage is smaller than PagePropMask"));
    }
    let end = 4usize
      .checked_add(size)
      .ok_or_else(|| Error::Limit("PageProperties boundary overflow".into()))?;
    if end > bytes.len() {
      return Err(Error::invalid(2, "cbPage exceeds the x stream"));
    }
    cursor.end = end;
    let property_mask = PagePropertyMask::from_bits_retain(cursor.read_u32()?);
    if property_mask.intersects(PagePropertyMask::UNUSED1 | PagePropertyMask::UNUSED) {
      return Err(Error::invalid(4, "PageProperties has unused mask bits set"));
    }
    use PagePropertyMask as Mask;
    let mut data_block = PageDataBlock::default();
    read_optional!(
      data_block,
      cursor,
      property_mask,
      TRANSITION_EFFECT,
      transition_effect,
      4,
      read_transition_effect
    );
    read_optional!(
      data_block,
      cursor,
      property_mask,
      TRANSITION_PERIOD,
      transition_period,
      4,
      read_u32
    );
    validate_page_data(&data_block)?;
    if cursor.position != end {
      return Err(Error::invalid(
        cursor.position as u64,
        "PageProperties fields do not consume cbPage",
      ));
    }
    Ok((
      Self {
        minor_version,
        major_version,
        property_mask,
        data_block,
      },
      end,
    ))
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_version(self.minor_version, self.major_version, "PageProperties")?;
    validate_page_data(&self.data_block)?;
    if self
      .property_mask
      .intersects(PagePropertyMask::UNUSED1 | PagePropertyMask::UNUSED)
    {
      return Err(Error::invalid(4, "PageProperties has unused mask bits set"));
    }
    use PagePropertyMask as Mask;
    let mut bytes = vec![self.minor_version, self.major_version, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    let output = &mut bytes;
    write_optional!(
      self.data_block,
      output,
      self.property_mask,
      TRANSITION_EFFECT,
      transition_effect,
      u32
    );
    write_optional!(
      self.data_block,
      output,
      self.property_mask,
      TRANSITION_PERIOD,
      transition_period,
      u32
    );
    let size =
      u16::try_from(bytes.len() - 4).map_err(|_| Error::Limit("cbPage exceeds u16".into()))?;
    bytes[2..4].copy_from_slice(&size.to_le_bytes());
    Ok(bytes)
  }
}

impl MultiPageProperties {
  fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 8 {
      return Err(Error::invalid(0, "truncated MultiPageProperties"));
    }
    let mut cursor = SliceCursor::new(bytes);
    let minor_version = cursor.read_u8()?;
    let major_version = cursor.read_u8()?;
    validate_version(minor_version, major_version, "MultiPageProperties")?;
    let size = usize::from(cursor.read_u16()?);
    if size < 4 {
      return Err(Error::invalid(
        2,
        "cbMultiPageControlProperties is smaller than its mask",
      ));
    }
    let property_end = 4usize
      .checked_add(size)
      .ok_or_else(|| Error::Limit("MultiPageProperties boundary overflow".into()))?;
    if property_end > bytes.len() {
      return Err(Error::invalid(
        2,
        "cbMultiPageControlProperties exceeds the x stream",
      ));
    }
    cursor.end = property_end;
    let property_mask = MultiPagePropertyMask::from_bits_retain(cursor.read_u32()?);
    if property_mask.intersects(MultiPagePropertyMask::UNUSED1 | MultiPagePropertyMask::UNUSED) {
      return Err(Error::invalid(
        4,
        "MultiPageProperties has unused mask bits set",
      ));
    }
    use MultiPagePropertyMask as Mask;
    let mut data_block = MultiPagePropertiesDataBlock::default();
    read_optional!(
      data_block,
      cursor,
      property_mask,
      PAGE_COUNT,
      page_count,
      4,
      read_i32
    );
    read_optional!(data_block, cursor, property_mask, ID, id, 4, read_i32);
    if cursor.position != property_end {
      return Err(Error::invalid(
        cursor.position as u64,
        "MultiPageProperties fields do not consume its property block",
      ));
    }
    cursor.end = bytes.len();
    let page_count = data_block
      .page_count
      .as_ref()
      .map_or(0, |value| value.value);
    let page_count = usize::try_from(page_count)
      .map_err(|_| Error::invalid(0, "MultiPage PageCount is negative"))?;
    let mut page_ids = Vec::with_capacity(page_count);
    for _ in 0..page_count {
      page_ids.push(cursor.read_i32()?);
    }
    if cursor.position != bytes.len() {
      return Err(Error::invalid(
        cursor.position as u64,
        "unexpected bytes after MultiPage PageIDs",
      ));
    }
    Ok(Self {
      minor_version,
      major_version,
      property_mask,
      flags: property_mask.contains(MultiPagePropertyMask::FLAGS),
      data_block,
      page_ids,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_version(
      self.minor_version,
      self.major_version,
      "MultiPageProperties",
    )?;
    if self
      .property_mask
      .intersects(MultiPagePropertyMask::UNUSED1 | MultiPagePropertyMask::UNUSED)
    {
      return Err(Error::invalid(
        4,
        "MultiPageProperties has unused mask bits set",
      ));
    }
    if self.flags != self.property_mask.contains(MultiPagePropertyMask::FLAGS) {
      return Err(Error::invalid(
        0,
        "MultiPage Flags does not match its property mask bit",
      ));
    }
    let page_count = self
      .data_block
      .page_count
      .as_ref()
      .map_or(0, |value| value.value);
    if usize::try_from(page_count).ok() != Some(self.page_ids.len()) {
      return Err(Error::invalid(
        0,
        "MultiPage PageCount does not match PageIDs",
      ));
    }
    use MultiPagePropertyMask as Mask;
    let mut bytes = vec![self.minor_version, self.major_version, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    let output = &mut bytes;
    write_optional!(
      self.data_block,
      output,
      self.property_mask,
      PAGE_COUNT,
      page_count,
      i32
    );
    write_optional!(self.data_block, output, self.property_mask, ID, id, i32);
    let size = u16::try_from(bytes.len() - 4)
      .map_err(|_| Error::Limit("cbMultiPageControlProperties exceeds u16".into()))?;
    bytes[2..4].copy_from_slice(&size.to_le_bytes());
    for id in &self.page_ids {
      bytes.extend_from_slice(&id.to_le_bytes());
    }
    Ok(bytes)
  }
}

impl MultiPageXStream {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut pages = Vec::new();
    let mut position = 0usize;
    while position < bytes.len() {
      if !pages.is_empty()
        && let Ok(multi_page) = MultiPageProperties::from_bytes(&bytes[position..])
      {
        let page_count = multi_page
          .data_block
          .page_count
          .as_ref()
          .map_or(0, |value| value.value);
        if usize::try_from(page_count)
          .ok()
          .and_then(|count| count.checked_add(1))
          == Some(pages.len())
        {
          return Ok(Self { pages, multi_page });
        }
      }
      let (page, size) = PageProperties::from_prefix(&bytes[position..])?;
      position += size;
      pages.push(page);
    }
    Err(Error::invalid(
      position as u64,
      "x stream does not end with matching MultiPageProperties",
    ))
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let page_count = self
      .multi_page
      .data_block
      .page_count
      .as_ref()
      .map_or(0, |value| value.value);
    if usize::try_from(page_count)
      .ok()
      .and_then(|count| count.checked_add(1))
      != Some(self.pages.len())
    {
      return Err(Error::invalid(
        0,
        "MultiPage x stream PageProperties count is not PageCount plus one",
      ));
    }
    let mut bytes = Vec::new();
    for page in &self.pages {
      bytes.extend_from_slice(&page.to_bytes()?);
    }
    bytes.extend_from_slice(&self.multi_page.to_bytes()?);
    Ok(bytes)
  }
}

impl TabStripControl {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 8 {
      return Err(Error::invalid(0, "truncated TabStripControl"));
    }
    let mut cursor = SliceCursor::new(bytes);
    let minor_version = cursor.read_u8()?;
    let major_version = cursor.read_u8()?;
    validate_version(minor_version, major_version, "TabStripControl")?;
    let size = usize::from(cursor.read_u16()?);
    if size < 4 {
      return Err(Error::invalid(2, "cbTabStrip is smaller than its mask"));
    }
    let property_end = 4usize
      .checked_add(size)
      .ok_or_else(|| Error::Limit("TabStrip property boundary overflow".into()))?;
    if property_end > bytes.len() {
      return Err(Error::invalid(2, "cbTabStrip exceeds the object stream"));
    }
    cursor.end = property_end;
    let property_mask = TabStripPropertyMask::from_bits_retain(cursor.read_u32()?);
    validate_tab_strip_mask(property_mask)?;
    let data_block = TabStripDataBlock::read(&mut cursor, property_mask)?;
    validate_control_various(
      data_block.various_property_bits.as_ref(),
      CachedControlClass::TabStrip,
    )?;
    let extra_data_block = TabStripExtraDataBlock::read(&mut cursor, property_mask, &data_block)?;
    validate_tab_strip_data(&data_block, &extra_data_block)?;
    if cursor.position != property_end {
      return Err(Error::invalid(
        cursor.position as u64,
        "TabStrip properties do not consume cbTabStrip",
      ));
    }
    cursor.end = bytes.len();
    let stream_data = TabStripStreamData {
      mouse_icon: if property_mask.contains(TabStripPropertyMask::MOUSE_ICON) {
        Some(cursor.read_picture()?)
      } else {
        None
      },
    };
    let (text_props, text_props_size) = TextProps::from_prefix(&bytes[cursor.position..])?;
    cursor.position += text_props_size;
    let tab_count = data_block
      .tab_data_count
      .as_ref()
      .map_or(0, |value| value.value);
    let tab_count = usize::try_from(tab_count)
      .map_err(|_| Error::Limit("TabStrip TabData count does not fit usize".into()))?;
    let mut tab_flags = Vec::with_capacity(tab_count);
    for _ in 0..tab_count {
      let flags = TabStripTabFlags::from_bits_retain(cursor.read_u32()?);
      flags.validate()?;
      tab_flags.push(flags);
    }
    if cursor.position != bytes.len() {
      return Err(Error::invalid(
        cursor.position as u64,
        "unexpected bytes after TabStripTabFlags",
      ));
    }
    Ok(Self {
      minor_version,
      major_version,
      property_mask,
      multi_row: property_mask.contains(TabStripPropertyMask::MULTI_ROW),
      tooltips: !property_mask.contains(TabStripPropertyMask::TOOLTIPS),
      new_version: property_mask.contains(TabStripPropertyMask::NEW_VERSION),
      data_block,
      extra_data_block,
      stream_data,
      text_props,
      tab_flags,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_version(self.minor_version, self.major_version, "TabStripControl")?;
    validate_tab_strip_mask(self.property_mask)?;
    validate_control_various(
      self.data_block.various_property_bits.as_ref(),
      CachedControlClass::TabStrip,
    )?;
    validate_tab_strip_data(&self.data_block, &self.extra_data_block)?;
    if self.multi_row != self.property_mask.contains(TabStripPropertyMask::MULTI_ROW)
      || self.tooltips == self.property_mask.contains(TabStripPropertyMask::TOOLTIPS)
      || self.new_version
        != self
          .property_mask
          .contains(TabStripPropertyMask::NEW_VERSION)
    {
      return Err(Error::invalid(
        0,
        "TabStrip Boolean properties do not match their mask bits",
      ));
    }
    let expected_tab_count = usize::try_from(
      self
        .data_block
        .tab_data_count
        .as_ref()
        .map_or(0, |value| value.value),
    )
    .map_err(|_| Error::Limit("TabStrip TabData count does not fit usize".into()))?;
    if expected_tab_count != self.tab_flags.len()
      || self.tab_flags.iter().any(|value| value.validate().is_err())
    {
      return Err(Error::invalid(
        0,
        "TabStrip TabData count or flags are invalid",
      ));
    }
    let mut bytes = vec![self.minor_version, self.major_version, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    self.data_block.write(&mut bytes, self.property_mask)?;
    self
      .extra_data_block
      .write(&mut bytes, self.property_mask, &self.data_block)?;
    let size =
      u16::try_from(bytes.len() - 4).map_err(|_| Error::Limit("cbTabStrip exceeds u16".into()))?;
    bytes[2..4].copy_from_slice(&size.to_le_bytes());
    write_picture(
      &mut bytes,
      self
        .property_mask
        .contains(TabStripPropertyMask::MOUSE_ICON),
      self.stream_data.mouse_icon.as_ref(),
      "TabStrip.MouseIcon",
    )?;
    bytes.extend_from_slice(&self.text_props.to_bytes()?);
    for flags in &self.tab_flags {
      bytes.extend_from_slice(&flags.bits().to_le_bytes());
    }
    Ok(bytes)
  }
}

impl ImageControl {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let (minor_version, major_version, property_end, mut cursor) =
      property_control_cursor(bytes, "ImageControl")?;
    let property_mask = ImagePropertyMask::from_bits_retain(cursor.read_u32()?);
    validate_image_mask(property_mask)?;
    let data_block = ImageDataBlock::read(&mut cursor, property_mask)?;
    validate_control_various(
      data_block.various_property_bits.as_ref(),
      CachedControlClass::Image,
    )?;
    let size = read_fm_size(&mut cursor, true)?.expect("required Image.Size");
    if cursor.position != property_end {
      return Err(Error::invalid(
        cursor.position as u64,
        "Image properties do not consume cbImage",
      ));
    }
    cursor.end = bytes.len();
    let stream_data = PictureStreamData::read(
      &mut cursor,
      property_mask.contains(ImagePropertyMask::PICTURE),
      property_mask.contains(ImagePropertyMask::MOUSE_ICON),
    )?;
    if cursor.position != bytes.len() {
      return Err(Error::invalid(
        cursor.position as u64,
        "unexpected bytes after Image StreamData",
      ));
    }
    Ok(Self {
      minor_version,
      major_version,
      property_mask,
      auto_size: property_mask.contains(ImagePropertyMask::AUTO_SIZE),
      picture_tiling: property_mask.contains(ImagePropertyMask::PICTURE_TILING),
      data_block,
      size,
      stream_data,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_version(self.minor_version, self.major_version, "ImageControl")?;
    validate_image_mask(self.property_mask)?;
    validate_control_various(
      self.data_block.various_property_bits.as_ref(),
      CachedControlClass::Image,
    )?;
    if self.auto_size != self.property_mask.contains(ImagePropertyMask::AUTO_SIZE)
      || self.picture_tiling
        != self
          .property_mask
          .contains(ImagePropertyMask::PICTURE_TILING)
    {
      return Err(Error::invalid(
        0,
        "Image Boolean properties do not match their mask bits",
      ));
    }
    let mut bytes = vec![self.minor_version, self.major_version, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    self.data_block.write(&mut bytes, self.property_mask)?;
    write_fm_size(&mut bytes, true, Some(self.size), "Image.Size")?;
    finalize_property_control(&mut bytes, "cbImage")?;
    self.stream_data.write(
      &mut bytes,
      self.property_mask.contains(ImagePropertyMask::PICTURE),
      self.property_mask.contains(ImagePropertyMask::MOUSE_ICON),
      "Image",
    )?;
    Ok(bytes)
  }
}

impl ImageDataBlock {
  fn read(cursor: &mut SliceCursor<'_>, mask: ImagePropertyMask) -> Result<Self> {
    use ImagePropertyMask as Mask;
    let mut value = Self::default();
    read_optional!(
      value,
      cursor,
      mask,
      BORDER_COLOR,
      border_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      BACK_COLOR,
      back_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      BORDER_STYLE,
      border_style,
      1,
      read_border_style_u8
    );
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_POINTER,
      mouse_pointer,
      1,
      read_mouse_pointer
    );
    read_optional!(
      value,
      cursor,
      mask,
      PICTURE_SIZE_MODE,
      picture_size_mode,
      1,
      read_picture_size_mode
    );
    read_optional!(
      value,
      cursor,
      mask,
      SPECIAL_EFFECT,
      special_effect,
      1,
      read_special_effect_u8
    );
    read_optional!(
      value,
      cursor,
      mask,
      PICTURE,
      picture_marker,
      2,
      read_persistence_marker
    );
    read_optional!(
      value,
      cursor,
      mask,
      PICTURE_ALIGNMENT,
      picture_alignment,
      1,
      read_picture_alignment
    );
    read_optional!(
      value,
      cursor,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      4,
      read_various_properties
    );
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_ICON,
      mouse_icon_marker,
      2,
      read_persistence_marker
    );
    value.trailing_padding = cursor.read_alignment(4)?;
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: ImagePropertyMask) -> Result<()> {
    use ImagePropertyMask as Mask;
    write_optional!(self, bytes, mask, BORDER_COLOR, border_color, u32);
    write_optional!(self, bytes, mask, BACK_COLOR, back_color, u32);
    write_optional!(self, bytes, mask, BORDER_STYLE, border_style, u8);
    write_optional!(self, bytes, mask, MOUSE_POINTER, mouse_pointer, u8);
    write_optional!(self, bytes, mask, PICTURE_SIZE_MODE, picture_size_mode, u8);
    write_optional!(self, bytes, mask, SPECIAL_EFFECT, special_effect, u8);
    write_optional!(self, bytes, mask, PICTURE, picture_marker, u16);
    write_optional!(self, bytes, mask, PICTURE_ALIGNMENT, picture_alignment, u8);
    write_optional!(
      self,
      bytes,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      u32
    );
    write_optional!(self, bytes, mask, MOUSE_ICON, mouse_icon_marker, u16);
    append_padding(bytes, &self.trailing_padding, 4, "Image DataBlock")
  }
}

impl PictureStreamData {
  fn read(cursor: &mut SliceCursor<'_>, picture: bool, mouse_icon: bool) -> Result<Self> {
    Ok(Self {
      picture: if picture {
        Some(cursor.read_picture()?)
      } else {
        None
      },
      mouse_icon: if mouse_icon {
        Some(cursor.read_picture()?)
      } else {
        None
      },
    })
  }

  fn write(&self, bytes: &mut Vec<u8>, picture: bool, mouse_icon: bool, owner: &str) -> Result<()> {
    write_picture(
      bytes,
      picture,
      self.picture.as_ref(),
      &format!("{owner}.Picture"),
    )?;
    write_picture(
      bytes,
      mouse_icon,
      self.mouse_icon.as_ref(),
      &format!("{owner}.MouseIcon"),
    )
  }
}

impl LabelControl {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let (minor_version, major_version, property_end, mut cursor) =
      property_control_cursor(bytes, "LabelControl")?;
    let property_mask = LabelPropertyMask::from_bits_retain(cursor.read_u32()?);
    validate_label_mask(property_mask)?;
    let data_block = LabelDataBlock::read(&mut cursor, property_mask)?;
    validate_control_various(
      data_block.various_property_bits.as_ref(),
      CachedControlClass::Label,
    )?;
    let extra_data_block = LabelExtraDataBlock {
      caption: read_fm_string(&mut cursor, data_block.caption.as_ref())?,
      size: read_fm_size(&mut cursor, true)?,
    };
    if cursor.position != property_end {
      return Err(Error::invalid(
        cursor.position as u64,
        "Label properties do not consume cbLabel",
      ));
    }
    cursor.end = bytes.len();
    let stream_data = PictureStreamData::read(
      &mut cursor,
      property_mask.contains(LabelPropertyMask::PICTURE),
      property_mask.contains(LabelPropertyMask::MOUSE_ICON),
    )?;
    let (text_props, text_size) = TextProps::from_prefix(&bytes[cursor.position..])?;
    cursor.position += text_size;
    if cursor.position != bytes.len() {
      return Err(Error::invalid(
        cursor.position as u64,
        "unexpected bytes after Label TextProps",
      ));
    }
    Ok(Self {
      minor_version,
      major_version,
      property_mask,
      data_block,
      extra_data_block,
      stream_data,
      text_props,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_version(self.minor_version, self.major_version, "LabelControl")?;
    validate_label_mask(self.property_mask)?;
    validate_control_various(
      self.data_block.various_property_bits.as_ref(),
      CachedControlClass::Label,
    )?;
    let mut bytes = vec![self.minor_version, self.major_version, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    self.data_block.write(&mut bytes, self.property_mask)?;
    write_fm_string(
      &mut bytes,
      self.data_block.caption.as_ref(),
      self.extra_data_block.caption.as_ref(),
      "Label.Caption",
    )?;
    write_fm_size(&mut bytes, true, self.extra_data_block.size, "Label.Size")?;
    finalize_property_control(&mut bytes, "cbLabel")?;
    self.stream_data.write(
      &mut bytes,
      self.property_mask.contains(LabelPropertyMask::PICTURE),
      self.property_mask.contains(LabelPropertyMask::MOUSE_ICON),
      "Label",
    )?;
    bytes.extend_from_slice(&self.text_props.to_bytes()?);
    Ok(bytes)
  }
}

impl LabelDataBlock {
  fn read(cursor: &mut SliceCursor<'_>, mask: LabelPropertyMask) -> Result<Self> {
    use LabelPropertyMask as Mask;
    let mut value = Self::default();
    read_optional!(
      value,
      cursor,
      mask,
      FORE_COLOR,
      fore_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      BACK_COLOR,
      back_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      4,
      read_various_properties
    );
    value.caption = read_count_descriptor(cursor, mask.contains(Mask::CAPTION))?;
    read_optional!(
      value,
      cursor,
      mask,
      PICTURE_POSITION,
      picture_position,
      4,
      read_picture_position
    );
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_POINTER,
      mouse_pointer,
      1,
      read_mouse_pointer
    );
    read_optional!(
      value,
      cursor,
      mask,
      BORDER_COLOR,
      border_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      BORDER_STYLE,
      border_style,
      2,
      read_border_style_u16
    );
    read_optional!(
      value,
      cursor,
      mask,
      SPECIAL_EFFECT,
      special_effect,
      2,
      read_special_effect_u16
    );
    read_optional!(
      value,
      cursor,
      mask,
      PICTURE,
      picture_marker,
      2,
      read_persistence_marker
    );
    read_optional!(value, cursor, mask, ACCELERATOR, accelerator, 2, read_u16);
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_ICON,
      mouse_icon_marker,
      2,
      read_persistence_marker
    );
    value.trailing_padding = cursor.read_alignment(4)?;
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: LabelPropertyMask) -> Result<()> {
    use LabelPropertyMask as Mask;
    write_optional!(self, bytes, mask, FORE_COLOR, fore_color, u32);
    write_optional!(self, bytes, mask, BACK_COLOR, back_color, u32);
    write_optional!(
      self,
      bytes,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      u32
    );
    write_count_descriptor(
      bytes,
      mask.contains(Mask::CAPTION),
      self.caption.as_ref(),
      "Label.Caption",
    )?;
    write_optional!(self, bytes, mask, PICTURE_POSITION, picture_position, u32);
    write_optional!(self, bytes, mask, MOUSE_POINTER, mouse_pointer, u8);
    write_optional!(self, bytes, mask, BORDER_COLOR, border_color, u32);
    write_optional!(self, bytes, mask, BORDER_STYLE, border_style, u16);
    write_optional!(self, bytes, mask, SPECIAL_EFFECT, special_effect, u16);
    write_optional!(self, bytes, mask, PICTURE, picture_marker, u16);
    write_optional!(self, bytes, mask, ACCELERATOR, accelerator, u16);
    write_optional!(self, bytes, mask, MOUSE_ICON, mouse_icon_marker, u16);
    append_padding(bytes, &self.trailing_padding, 4, "Label DataBlock")
  }
}

impl SpinButtonControl {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let (minor_version, major_version, property_end, mut cursor) =
      property_control_cursor(bytes, "SpinButtonControl")?;
    let property_mask = SpinButtonPropertyMask::from_bits_retain(cursor.read_u32()?);
    validate_spin_button_mask(property_mask)?;
    let data_block = SpinButtonDataBlock::read(&mut cursor, property_mask)?;
    validate_control_various(
      data_block.various_property_bits.as_ref(),
      CachedControlClass::SpinButton,
    )?;
    validate_spin_button_enabled_mask(property_mask, &data_block)?;
    let size = read_fm_size(&mut cursor, true)?.expect("required SpinButton.Size");
    if cursor.position != property_end {
      return Err(Error::invalid(
        cursor.position as u64,
        "SpinButton properties do not consume cbSpinButton",
      ));
    }
    cursor.end = bytes.len();
    let mouse_icon = if property_mask.contains(SpinButtonPropertyMask::MOUSE_ICON) {
      Some(cursor.read_picture()?)
    } else {
      None
    };
    if cursor.position != bytes.len() {
      return Err(Error::invalid(
        cursor.position as u64,
        "unexpected bytes after SpinButton StreamData",
      ));
    }
    Ok(Self {
      minor_version,
      major_version,
      property_mask,
      data_block,
      size,
      mouse_icon,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_version(self.minor_version, self.major_version, "SpinButtonControl")?;
    validate_spin_button_mask(self.property_mask)?;
    validate_control_various(
      self.data_block.various_property_bits.as_ref(),
      CachedControlClass::SpinButton,
    )?;
    validate_spin_button_enabled_mask(self.property_mask, &self.data_block)?;
    let mut bytes = vec![self.minor_version, self.major_version, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    self.data_block.write(&mut bytes, self.property_mask)?;
    write_fm_size(&mut bytes, true, Some(self.size), "SpinButton.Size")?;
    finalize_property_control(&mut bytes, "cbSpinButton")?;
    write_picture(
      &mut bytes,
      self
        .property_mask
        .contains(SpinButtonPropertyMask::MOUSE_ICON),
      self.mouse_icon.as_ref(),
      "SpinButton.MouseIcon",
    )?;
    Ok(bytes)
  }
}

impl SpinButtonDataBlock {
  fn read(cursor: &mut SliceCursor<'_>, mask: SpinButtonPropertyMask) -> Result<Self> {
    use SpinButtonPropertyMask as Mask;
    let mut value = Self::default();
    read_optional!(
      value,
      cursor,
      mask,
      FORE_COLOR,
      fore_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      BACK_COLOR,
      back_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      4,
      read_various_properties
    );
    read_optional!(value, cursor, mask, MIN, min, 4, read_i32);
    read_optional!(value, cursor, mask, MAX, max, 4, read_i32);
    read_optional!(value, cursor, mask, POSITION, position, 4, read_i32);
    read_optional!(
      value,
      cursor,
      mask,
      PREV_ENABLED,
      prev_enabled,
      4,
      read_enabled_state
    );
    read_optional!(
      value,
      cursor,
      mask,
      NEXT_ENABLED,
      next_enabled,
      4,
      read_enabled_state
    );
    read_optional!(value, cursor, mask, SMALL_CHANGE, small_change, 4, read_i32);
    read_optional!(
      value,
      cursor,
      mask,
      ORIENTATION,
      orientation,
      4,
      read_orientation
    );
    read_optional!(value, cursor, mask, DELAY, delay, 4, read_u32);
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_ICON,
      mouse_icon_marker,
      2,
      read_persistence_marker
    );
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_POINTER,
      mouse_pointer,
      1,
      read_mouse_pointer
    );
    value.trailing_padding = cursor.read_alignment(4)?;
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: SpinButtonPropertyMask) -> Result<()> {
    use SpinButtonPropertyMask as Mask;
    write_optional!(self, bytes, mask, FORE_COLOR, fore_color, u32);
    write_optional!(self, bytes, mask, BACK_COLOR, back_color, u32);
    write_optional!(
      self,
      bytes,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      u32
    );
    write_optional!(self, bytes, mask, MIN, min, i32);
    write_optional!(self, bytes, mask, MAX, max, i32);
    write_optional!(self, bytes, mask, POSITION, position, i32);
    write_optional!(self, bytes, mask, PREV_ENABLED, prev_enabled, i32);
    write_optional!(self, bytes, mask, NEXT_ENABLED, next_enabled, i32);
    write_optional!(self, bytes, mask, SMALL_CHANGE, small_change, i32);
    write_optional!(self, bytes, mask, ORIENTATION, orientation, u32);
    write_optional!(self, bytes, mask, DELAY, delay, u32);
    write_optional!(self, bytes, mask, MOUSE_ICON, mouse_icon_marker, u16);
    write_optional!(self, bytes, mask, MOUSE_POINTER, mouse_pointer, u8);
    append_padding(bytes, &self.trailing_padding, 4, "SpinButton DataBlock")
  }
}

impl ScrollBarControl {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let (minor_version, major_version, property_end, mut cursor) =
      property_control_cursor(bytes, "ScrollBarControl")?;
    let property_mask = ScrollBarPropertyMask::from_bits_retain(cursor.read_u32()?);
    validate_scroll_bar_mask(property_mask)?;
    let data_block = ScrollBarDataBlock::read(&mut cursor, property_mask)?;
    validate_control_various(
      data_block.various_property_bits.as_ref(),
      CachedControlClass::ScrollBar,
    )?;
    validate_scroll_bar_enabled_mask(property_mask, &data_block)?;
    let size = read_fm_size(&mut cursor, true)?.expect("required ScrollBar.Size");
    if cursor.position != property_end {
      return Err(Error::invalid(
        cursor.position as u64,
        "ScrollBar properties do not consume cbScrollBar",
      ));
    }
    cursor.end = bytes.len();
    let mouse_icon = if property_mask.contains(ScrollBarPropertyMask::MOUSE_ICON) {
      Some(cursor.read_picture()?)
    } else {
      None
    };
    if cursor.position != bytes.len() {
      return Err(Error::invalid(
        cursor.position as u64,
        "unexpected bytes after ScrollBar StreamData",
      ));
    }
    Ok(Self {
      minor_version,
      major_version,
      property_mask,
      data_block,
      size,
      mouse_icon,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_version(self.minor_version, self.major_version, "ScrollBarControl")?;
    validate_scroll_bar_mask(self.property_mask)?;
    validate_control_various(
      self.data_block.various_property_bits.as_ref(),
      CachedControlClass::ScrollBar,
    )?;
    validate_scroll_bar_enabled_mask(self.property_mask, &self.data_block)?;
    let mut bytes = vec![self.minor_version, self.major_version, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    self.data_block.write(&mut bytes, self.property_mask)?;
    write_fm_size(&mut bytes, true, Some(self.size), "ScrollBar.Size")?;
    finalize_property_control(&mut bytes, "cbScrollBar")?;
    write_picture(
      &mut bytes,
      self
        .property_mask
        .contains(ScrollBarPropertyMask::MOUSE_ICON),
      self.mouse_icon.as_ref(),
      "ScrollBar.MouseIcon",
    )?;
    Ok(bytes)
  }
}

impl ScrollBarDataBlock {
  fn read(cursor: &mut SliceCursor<'_>, mask: ScrollBarPropertyMask) -> Result<Self> {
    use ScrollBarPropertyMask as Mask;
    let mut value = Self::default();
    read_optional!(
      value,
      cursor,
      mask,
      FORE_COLOR,
      fore_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      BACK_COLOR,
      back_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      4,
      read_various_properties
    );
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_POINTER,
      mouse_pointer,
      1,
      read_mouse_pointer
    );
    read_optional!(value, cursor, mask, MIN, min, 4, read_i32);
    read_optional!(value, cursor, mask, MAX, max, 4, read_i32);
    read_optional!(value, cursor, mask, POSITION, position, 4, read_i32);
    read_optional!(
      value,
      cursor,
      mask,
      PREV_ENABLED,
      prev_enabled,
      4,
      read_enabled_state
    );
    read_optional!(
      value,
      cursor,
      mask,
      NEXT_ENABLED,
      next_enabled,
      4,
      read_enabled_state
    );
    read_optional!(value, cursor, mask, SMALL_CHANGE, small_change, 4, read_i32);
    read_optional!(value, cursor, mask, LARGE_CHANGE, large_change, 4, read_i32);
    read_optional!(
      value,
      cursor,
      mask,
      ORIENTATION,
      orientation,
      4,
      read_orientation
    );
    read_optional!(
      value,
      cursor,
      mask,
      PROPORTIONAL_THUMB,
      proportional_thumb,
      2,
      read_proportional_thumb
    );
    read_optional!(value, cursor, mask, DELAY, delay, 4, read_u32);
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_ICON,
      mouse_icon_marker,
      2,
      read_persistence_marker
    );
    value.trailing_padding = cursor.read_alignment(4)?;
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: ScrollBarPropertyMask) -> Result<()> {
    use ScrollBarPropertyMask as Mask;
    write_optional!(self, bytes, mask, FORE_COLOR, fore_color, u32);
    write_optional!(self, bytes, mask, BACK_COLOR, back_color, u32);
    write_optional!(
      self,
      bytes,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      u32
    );
    write_optional!(self, bytes, mask, MOUSE_POINTER, mouse_pointer, u8);
    write_optional!(self, bytes, mask, MIN, min, i32);
    write_optional!(self, bytes, mask, MAX, max, i32);
    write_optional!(self, bytes, mask, POSITION, position, i32);
    write_optional!(self, bytes, mask, PREV_ENABLED, prev_enabled, i32);
    write_optional!(self, bytes, mask, NEXT_ENABLED, next_enabled, i32);
    write_optional!(self, bytes, mask, SMALL_CHANGE, small_change, i32);
    write_optional!(self, bytes, mask, LARGE_CHANGE, large_change, i32);
    write_optional!(self, bytes, mask, ORIENTATION, orientation, u32);
    write_optional!(
      self,
      bytes,
      mask,
      PROPORTIONAL_THUMB,
      proportional_thumb,
      i16
    );
    write_optional!(self, bytes, mask, DELAY, delay, u32);
    write_optional!(self, bytes, mask, MOUSE_ICON, mouse_icon_marker, u16);
    append_padding(bytes, &self.trailing_padding, 4, "ScrollBar DataBlock")
  }
}

impl FormObjectStream {
  pub fn from_form(form: &FormControl, bytes: &[u8]) -> Result<Self> {
    let mut position = 0usize;
    let mut controls = Vec::new();
    for (site_index, site) in form.site_data.sites.iter().enumerate() {
      if !site_flags(site).contains(SiteFlags::STREAMED) {
        continue;
      }
      let size = usize::try_from(site_object_stream_size(site))
        .map_err(|_| Error::Limit("Form object stream size does not fit usize".into()))?;
      let end = position
        .checked_add(size)
        .ok_or_else(|| Error::Limit("Form object stream boundary overflow".into()))?;
      if end > bytes.len() {
        return Err(Error::invalid(
          position as u64,
          "Site ObjectStreamSize exceeds the o stream",
        ));
      }
      let class_index = site_class_index(site);
      let object = &bytes[position..end];
      let persistence = match class_index {
        SiteClassIndex::ClassTable(table_index) => {
          let table_index = usize::from(table_index);
          if table_index >= form.site_data.class_table.len() {
            return Err(Error::invalid(
              position as u64,
              "Site class-table index is out of bounds",
            ));
          }
          FormControlPersistence::ExternalClass(ExternalComPersistStream {
            bytes: object.to_vec(),
          })
        }
        SiteClassIndex::Cached(class) => read_cached_form_control(class, object, position)?,
        SiteClassIndex::Invalid => {
          return Err(Error::invalid(
            position as u64,
            "streamed control has an invalid ClsidCacheIndex",
          ));
        }
      };
      controls.push(FormObjectControl {
        site_index,
        class_index,
        persistence,
      });
      position = end;
    }
    if position != bytes.len() {
      return Err(Error::invalid(
        position as u64,
        "unexpected bytes after Form object controls",
      ));
    }
    Ok(Self { controls })
  }

  pub fn to_bytes(&self, form: &FormControl) -> Result<Vec<u8>> {
    let expected_sites = form
      .site_data
      .sites
      .iter()
      .enumerate()
      .filter(|(_, site)| site_flags(site).contains(SiteFlags::STREAMED))
      .collect::<Vec<_>>();
    if expected_sites.len() != self.controls.len() {
      return Err(Error::invalid(
        0,
        "FormObjectStream control count does not match o-stream Sites",
      ));
    }
    let mut bytes = Vec::new();
    for ((expected_site_index, site), control) in expected_sites.into_iter().zip(&self.controls) {
      let expected_class_index = site_class_index(site);
      if control.site_index != expected_site_index || control.class_index != expected_class_index {
        return Err(Error::invalid(
          0,
          "FormObjectControl does not match its Site index or class index",
        ));
      }
      let object = match (&control.persistence, control.class_index) {
        (FormControlPersistence::ExternalClass(value), SiteClassIndex::ClassTable(_)) => {
          value.bytes.clone()
        }
        (value, SiteClassIndex::Cached(class)) => write_cached_form_control(value, class)?,
        _ => return Err(Error::invalid(0, "cached/external class index mismatch")),
      };
      if u32::try_from(object.len()).ok() != Some(site_object_stream_size(site)) {
        return Err(Error::invalid(
          0,
          "FormObjectControl size does not match Site.ObjectStreamSize",
        ));
      }
      bytes.extend_from_slice(&object);
    }
    Ok(bytes)
  }
}

impl ParentControlStorage {
  pub const MULTI_PAGE_CLASS_ID: Guid = Guid::from_fields(
    0x46e3_1370,
    0x3f7a,
    0x11ce,
    [0xbe, 0xd6, 0x00, 0xaa, 0x00, 0x61, 0x10, 0x80],
  );
  pub const FRAME_CLASS_ID: Guid = Guid::from_fields(
    0x6e18_2020,
    0xf460,
    0x11ce,
    [0x9b, 0xcd, 0x00, 0xaa, 0x00, 0x60, 0x8e, 0x01],
  );
  pub const PAGE_CLASS_ID: Guid = Guid::from_fields(
    0xc62a_69f0,
    0x16dc,
    0x11ce,
    [0x9e, 0x98, 0x00, 0xaa, 0x00, 0x57, 0x4a, 0x4f],
  );

  const MAX_PARENT_DEPTH: usize = 256;

  pub fn is_parent_class_id(class_id: Guid) -> bool {
    matches!(
      class_id,
      Self::MULTI_PAGE_CLASS_ID | Self::FRAME_CLASS_ID | Self::PAGE_CLASS_ID
    )
  }

  pub fn from_compound(compound: &CompoundFile, path: impl AsRef<Path>) -> Result<Self> {
    Self::from_compound_at_depth(compound, path.as_ref(), 0)
  }

  fn validate_tree(&self, depth: usize) -> Result<()> {
    if depth > Self::MAX_PARENT_DEPTH {
      return Err(Error::Limit(format!(
        "MS-OFORMS parent storage nesting exceeds {}",
        Self::MAX_PARENT_DEPTH
      )));
    }
    validate_parent_children(&self.path, &self.form, &self.children)?;
    match (
      self.class_id == Self::MULTI_PAGE_CLASS_ID,
      &self.multi_page_x,
    ) {
      (true, Some(value)) => validate_multi_page_children(&self.form, &self.children, value)?,
      (false, None) => {}
      _ => {
        return Err(mask_field_mismatch(
          "ParentControlStorage",
          "MultiPage x stream",
        ));
      }
    }
    self.form.to_bytes()?;
    self.object_stream.to_bytes(&self.form)?;
    if let Some(value) = &self.multi_page_x {
      value.to_bytes()?;
    }
    for child in &self.children {
      child.storage.validate_tree(depth + 1)?;
    }
    Ok(())
  }

  fn from_compound_at_depth(compound: &CompoundFile, path: &Path, depth: usize) -> Result<Self> {
    if depth > Self::MAX_PARENT_DEPTH {
      return Err(Error::Limit(format!(
        "MS-OFORMS parent storage nesting exceeds {}",
        Self::MAX_PARENT_DEPTH
      )));
    }
    let entry = compound.entry(path).ok_or_else(|| {
      Error::invalid(
        0,
        format!("MS-OFORMS storage {} does not exist", path.display()),
      )
    })?;
    if !entry.is_storage() {
      return Err(Error::invalid(
        0,
        format!("MS-OFORMS path {} is not a storage", path.display()),
      ));
    }
    let form_bytes = required_parent_stream(compound, path, FORM_STREAM_NAME)?;
    let object_bytes = required_parent_stream(compound, path, OBJECT_STREAM_NAME)?;
    let form = FormControl::from_bytes(form_bytes)?;
    let object_stream = FormObjectStream::from_form(&form, object_bytes)?;

    let mut children = Vec::new();
    for (site_index, site) in form.site_data.sites.iter().enumerate() {
      if site_flags(site).contains(SiteFlags::STREAMED) {
        continue;
      }
      let expected_class_id = embedded_parent_class_id(site)?;
      let storage_name = site_storage_name(site)?;
      let storage_path = path.join(&storage_name);
      let storage = Self::from_compound_at_depth(compound, &storage_path, depth + 1)?;
      if storage.class_id != expected_class_id {
        return Err(Error::invalid(
          0,
          "embedded parent storage CLSID does not match ClsidCacheIndex",
        ));
      }
      children.push(EmbeddedParentControl {
        site_index,
        storage_name,
        storage: Box::new(storage),
      });
    }

    let multi_page_x = if entry.clsid == Self::MULTI_PAGE_CLASS_ID {
      let bytes = required_parent_stream(compound, path, MULTIPAGE_STREAM_NAME)?;
      let value = MultiPageXStream::from_bytes(bytes)?;
      validate_multi_page_children(&form, &children, &value)?;
      Some(value)
    } else {
      None
    };

    Ok(Self {
      path: path.to_path_buf(),
      class_id: entry.clsid,
      form,
      object_stream,
      multi_page_x,
      children,
    })
  }

  /// Writes edited typed streams back into an existing compound file.
  ///
  /// Structural CFB edits (moving/adding parent storages) remain explicit `CompoundFile`
  /// operations; this method verifies that the current Site.ID hierarchy still matches the
  /// parsed paths before replacing stream data.
  pub fn write_to_compound(&self, compound: &mut CompoundFile) -> Result<()> {
    self.write_to_compound_at_depth(compound, 0)
  }

  fn write_to_compound_at_depth(&self, compound: &mut CompoundFile, depth: usize) -> Result<()> {
    if depth > Self::MAX_PARENT_DEPTH {
      return Err(Error::Limit(format!(
        "MS-OFORMS parent storage nesting exceeds {}",
        Self::MAX_PARENT_DEPTH
      )));
    }
    let entry = compound.entry(&self.path).ok_or_else(|| {
      Error::invalid(
        0,
        format!("MS-OFORMS storage {} does not exist", self.path.display()),
      )
    })?;
    if !entry.is_storage() || entry.clsid != self.class_id {
      return Err(Error::invalid(
        0,
        format!(
          "MS-OFORMS storage {} kind or CLSID changed",
          self.path.display()
        ),
      ));
    }

    validate_parent_children(&self.path, &self.form, &self.children)?;
    match (
      self.class_id == Self::MULTI_PAGE_CLASS_ID,
      &self.multi_page_x,
    ) {
      (true, Some(value)) => validate_multi_page_children(&self.form, &self.children, value)?,
      (false, None) => {}
      _ => {
        return Err(mask_field_mismatch(
          "ParentControlStorage",
          "MultiPage x stream",
        ));
      }
    }

    let form_bytes = self.form.to_bytes()?;
    let object_bytes = self.object_stream.to_bytes(&self.form)?;
    compound.overwrite_stream(self.path.join(FORM_STREAM_NAME), form_bytes)?;
    compound.overwrite_stream(self.path.join(OBJECT_STREAM_NAME), object_bytes)?;
    if let Some(value) = &self.multi_page_x {
      compound.overwrite_stream(self.path.join(MULTIPAGE_STREAM_NAME), value.to_bytes()?)?;
    }
    for child in &self.children {
      child
        .storage
        .write_to_compound_at_depth(compound, depth + 1)?;
    }
    Ok(())
  }
}

fn required_parent_stream<'a>(
  compound: &'a CompoundFile,
  storage_path: &Path,
  name: &str,
) -> Result<&'a [u8]> {
  let path = storage_path.join(name);
  compound.stream(&path).ok_or_else(|| {
    Error::invalid(
      0,
      format!("MS-OFORMS stream {} does not exist", path.display()),
    )
  })
}

fn site_storage_name(site: &OleSiteConcreteControl) -> Result<String> {
  let id = site.data_block.id.as_ref().map_or(0, |value| value.value);
  if id < 0 {
    return Err(Error::invalid(
      0,
      "embedded parent control has a negative Site.ID",
    ));
  }
  Ok(if id < 10 {
    format!("i{id:02}")
  } else {
    format!("i{id}")
  })
}

fn embedded_parent_class_id(site: &OleSiteConcreteControl) -> Result<Guid> {
  match site_class_index(site) {
    SiteClassIndex::Cached(CachedControlClass::Form) => Ok(ParentControlStorage::PAGE_CLASS_ID),
    SiteClassIndex::Cached(CachedControlClass::Frame) => Ok(ParentControlStorage::FRAME_CLASS_ID),
    SiteClassIndex::Cached(CachedControlClass::MultiPage) => {
      Ok(ParentControlStorage::MULTI_PAGE_CLASS_ID)
    }
    _ => Err(Error::invalid(
      0,
      "non-streamed Site must identify Form/Page, Frame, or MultiPage",
    )),
  }
}

fn validate_parent_children(
  path: &Path,
  form: &FormControl,
  children: &[EmbeddedParentControl],
) -> Result<()> {
  let expected = form
    .site_data
    .sites
    .iter()
    .enumerate()
    .filter(|(_, site)| !site_flags(site).contains(SiteFlags::STREAMED));
  let mut actual = children.iter();
  for (site_index, site) in expected {
    let child = actual
      .next()
      .ok_or_else(|| Error::invalid(0, "ParentControlStorage is missing an embedded parent"))?;
    let storage_name = site_storage_name(site)?;
    let expected_class_id = embedded_parent_class_id(site)?;
    if child.site_index != site_index
      || child.storage_name != storage_name
      || child.storage.path != path.join(&storage_name)
      || child.storage.class_id != expected_class_id
    {
      return Err(Error::invalid(
        0,
        "embedded parent storage does not match its OleSiteConcrete",
      ));
    }
  }
  if actual.next().is_some() {
    return Err(Error::invalid(
      0,
      "ParentControlStorage has an extra embedded parent",
    ));
  }
  Ok(())
}

fn validate_multi_page_children(
  form: &FormControl,
  children: &[EmbeddedParentControl],
  stream: &MultiPageXStream,
) -> Result<()> {
  if stream.pages.len() != children.len().saturating_add(1) {
    return Err(Error::invalid(
      0,
      "MultiPage x stream PageProperties count does not match child Page storages",
    ));
  }
  let expected_ids = form
    .site_data
    .sites
    .iter()
    .enumerate()
    .filter(|(_, site)| !site_flags(site).contains(SiteFlags::STREAMED))
    .map(|(_, site)| site.data_block.id.as_ref().map_or(0, |value| value.value))
    .collect::<Vec<_>>();
  if stream.multi_page.page_ids != expected_ids {
    return Err(Error::invalid(
      0,
      "MultiPage x stream PageIDs do not match child Site.ID values",
    ));
  }
  Ok(())
}

fn site_flags(site: &OleSiteConcreteControl) -> SiteFlags {
  site.data_block.bit_flags.as_ref().map_or_else(
    || SiteFlags::from_bits_retain(0x0000_0033),
    |value| value.value,
  )
}

fn site_object_stream_size(site: &OleSiteConcreteControl) -> u32 {
  site
    .data_block
    .object_stream_size
    .as_ref()
    .map_or(0, |value| value.value)
}

fn site_class_index(site: &OleSiteConcreteControl) -> SiteClassIndex {
  site
    .data_block
    .clsid_cache_index
    .as_ref()
    .map_or(SiteClassIndex::Invalid, |value| value.value)
}

impl TabStripDataBlock {
  fn read(cursor: &mut SliceCursor<'_>, mask: TabStripPropertyMask) -> Result<Self> {
    use TabStripPropertyMask as Mask;
    let mut value = Self::default();
    read_optional!(value, cursor, mask, LIST_INDEX, list_index, 4, read_i32);
    read_optional!(
      value,
      cursor,
      mask,
      BACK_COLOR,
      back_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      FORE_COLOR,
      fore_color,
      4,
      read_ole_color
    );
    read_optional!(value, cursor, mask, ITEMS, items_size, 4, read_u32);
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_POINTER,
      mouse_pointer,
      1,
      read_mouse_pointer
    );
    read_optional!(
      value,
      cursor,
      mask,
      TAB_ORIENTATION,
      tab_orientation,
      4,
      read_tab_orientation
    );
    read_optional!(value, cursor, mask, TAB_STYLE, tab_style, 4, read_tab_style);
    read_optional!(
      value,
      cursor,
      mask,
      TAB_FIXED_WIDTH,
      tab_fixed_width,
      4,
      read_u32
    );
    read_optional!(
      value,
      cursor,
      mask,
      TAB_FIXED_HEIGHT,
      tab_fixed_height,
      4,
      read_u32
    );
    read_optional!(
      value,
      cursor,
      mask,
      TIP_STRINGS,
      tip_strings_size,
      4,
      read_u32
    );
    read_optional!(value, cursor, mask, NAMES, names_size, 4, read_u32);
    read_optional!(
      value,
      cursor,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      4,
      read_various_properties
    );
    read_optional!(
      value,
      cursor,
      mask,
      TABS_ALLOCATED,
      tabs_allocated,
      4,
      read_u32
    );
    read_optional!(value, cursor, mask, TAGS, tags_size, 4, read_u32);
    read_optional!(value, cursor, mask, TAB_DATA, tab_data_count, 4, read_u32);
    read_optional!(
      value,
      cursor,
      mask,
      ACCELERATORS,
      accelerators_size,
      4,
      read_u32
    );
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_ICON,
      mouse_icon_marker,
      2,
      read_persistence_marker
    );
    value.trailing_padding = cursor.read_alignment(4)?;
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: TabStripPropertyMask) -> Result<()> {
    use TabStripPropertyMask as Mask;
    write_optional!(self, bytes, mask, LIST_INDEX, list_index, i32);
    write_optional!(self, bytes, mask, BACK_COLOR, back_color, u32);
    write_optional!(self, bytes, mask, FORE_COLOR, fore_color, u32);
    write_optional!(self, bytes, mask, ITEMS, items_size, u32);
    write_optional!(self, bytes, mask, MOUSE_POINTER, mouse_pointer, u8);
    write_optional!(self, bytes, mask, TAB_ORIENTATION, tab_orientation, u32);
    write_optional!(self, bytes, mask, TAB_STYLE, tab_style, u32);
    write_optional!(self, bytes, mask, TAB_FIXED_WIDTH, tab_fixed_width, u32);
    write_optional!(self, bytes, mask, TAB_FIXED_HEIGHT, tab_fixed_height, u32);
    write_optional!(self, bytes, mask, TIP_STRINGS, tip_strings_size, u32);
    write_optional!(self, bytes, mask, NAMES, names_size, u32);
    write_optional!(
      self,
      bytes,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      u32
    );
    write_optional!(self, bytes, mask, TABS_ALLOCATED, tabs_allocated, u32);
    write_optional!(self, bytes, mask, TAGS, tags_size, u32);
    write_optional!(self, bytes, mask, TAB_DATA, tab_data_count, u32);
    write_optional!(self, bytes, mask, ACCELERATORS, accelerators_size, u32);
    write_optional!(self, bytes, mask, MOUSE_ICON, mouse_icon_marker, u16);
    append_padding(bytes, &self.trailing_padding, 4, "TabStrip DataBlock")
  }
}

impl TabStripExtraDataBlock {
  fn read(
    cursor: &mut SliceCursor<'_>,
    mask: TabStripPropertyMask,
    data: &TabStripDataBlock,
  ) -> Result<Self> {
    Ok(Self {
      size: read_fm_size(cursor, mask.contains(TabStripPropertyMask::SIZE))?,
      items: read_sized_array_strings(cursor, data.items_size.as_ref(), "Items")?,
      tip_strings: read_sized_array_strings(cursor, data.tip_strings_size.as_ref(), "TipStrings")?,
      tab_names: read_sized_array_strings(cursor, data.names_size.as_ref(), "TabNames")?,
      tags: read_sized_array_strings(cursor, data.tags_size.as_ref(), "Tags")?,
      accelerators: read_sized_array_strings(
        cursor,
        data.accelerators_size.as_ref(),
        "Accelerators",
      )?,
    })
  }

  fn write(
    &self,
    bytes: &mut Vec<u8>,
    mask: TabStripPropertyMask,
    data: &TabStripDataBlock,
  ) -> Result<()> {
    write_fm_size(
      bytes,
      mask.contains(TabStripPropertyMask::SIZE),
      self.size,
      "TabStrip.Size",
    )?;
    write_sized_array_strings(
      bytes,
      data.items_size.as_ref(),
      self.items.as_deref(),
      "Items",
    )?;
    write_sized_array_strings(
      bytes,
      data.tip_strings_size.as_ref(),
      self.tip_strings.as_deref(),
      "TipStrings",
    )?;
    write_sized_array_strings(
      bytes,
      data.names_size.as_ref(),
      self.tab_names.as_deref(),
      "TabNames",
    )?;
    write_sized_array_strings(bytes, data.tags_size.as_ref(), self.tags.as_deref(), "Tags")?;
    write_sized_array_strings(
      bytes,
      data.accelerators_size.as_ref(),
      self.accelerators.as_deref(),
      "Accelerators",
    )
  }
}

impl FormDataBlock {
  fn form_flags(&self) -> Result<FormFlags> {
    let value = self
      .boolean_properties
      .as_ref()
      .map_or(FormFlags::ENABLED, |value| value.value);
    value.validate()?;
    Ok(value)
  }

  fn validate(&self) -> Result<()> {
    self.form_flags()?;
    if let Some(zoom) = &self.zoom
      && !(10..=400).contains(&zoom.value)
    {
      return Err(Error::invalid(
        0,
        "Form Zoom must be in the range 10 through 400",
      ));
    }
    let draw_buffer = self
      .draw_buffer
      .as_ref()
      .map_or(32_000, |value| value.value);
    if !(16_000..=1_048_576).contains(&draw_buffer) {
      return Err(Error::invalid(
        0,
        "Form DrawBuffer must be in the range 16000 through 1048576",
      ));
    }
    Ok(())
  }

  fn read(cursor: &mut SliceCursor<'_>, mask: FormPropertyMask) -> Result<Self> {
    use FormPropertyMask as Mask;
    let mut value = Self::default();
    read_optional!(
      value,
      cursor,
      mask,
      BACK_COLOR,
      back_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      FORE_COLOR,
      fore_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      NEXT_AVAILABLE_ID,
      next_available_id,
      4,
      read_u32
    );
    read_optional!(
      value,
      cursor,
      mask,
      BOOLEAN_PROPERTIES,
      boolean_properties,
      4,
      read_form_flags
    );
    read_optional!(
      value,
      cursor,
      mask,
      BORDER_STYLE,
      border_style,
      1,
      read_border_style_u8
    );
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_POINTER,
      mouse_pointer,
      1,
      read_mouse_pointer
    );
    read_optional!(
      value,
      cursor,
      mask,
      SCROLL_BARS,
      scroll_bars,
      1,
      read_form_scroll_bars
    );
    read_optional!(value, cursor, mask, GROUP_COUNT, group_count, 4, read_i32);
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_ICON,
      mouse_icon_marker,
      2,
      read_persistence_marker
    );
    read_optional!(value, cursor, mask, CYCLE, cycle, 1, read_cycle);
    read_optional!(
      value,
      cursor,
      mask,
      SPECIAL_EFFECT,
      special_effect,
      1,
      read_special_effect_u8
    );
    read_optional!(
      value,
      cursor,
      mask,
      BORDER_COLOR,
      border_color,
      4,
      read_ole_color
    );
    if mask.contains(FormPropertyMask::CAPTION) {
      value.caption = Some(AlignedValue {
        padding_before: cursor.read_alignment(4)?,
        value: CountOfBytesWithCompressionFlag::from_raw(cursor.read_u32()?),
      });
    }
    read_optional!(
      value,
      cursor,
      mask,
      FONT,
      font_marker,
      2,
      read_persistence_marker
    );
    read_optional!(
      value,
      cursor,
      mask,
      PICTURE,
      picture_marker,
      2,
      read_persistence_marker
    );
    read_optional!(value, cursor, mask, ZOOM, zoom, 4, read_u32);
    read_optional!(
      value,
      cursor,
      mask,
      PICTURE_ALIGNMENT,
      picture_alignment,
      1,
      read_picture_alignment
    );
    read_optional!(
      value,
      cursor,
      mask,
      PICTURE_SIZE_MODE,
      picture_size_mode,
      1,
      read_picture_size_mode
    );
    read_optional!(value, cursor, mask, SHAPE_COOKIE, shape_cookie, 4, read_u32);
    read_optional!(value, cursor, mask, DRAW_BUFFER, draw_buffer, 4, read_u32);
    value.trailing_padding = cursor.read_alignment(4)?;
    value.validate()?;
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: FormPropertyMask) -> Result<()> {
    use FormPropertyMask as Mask;
    self.validate()?;
    write_optional!(self, bytes, mask, BACK_COLOR, back_color, u32);
    write_optional!(self, bytes, mask, FORE_COLOR, fore_color, u32);
    write_optional!(self, bytes, mask, NEXT_AVAILABLE_ID, next_available_id, u32);
    write_optional!(
      self,
      bytes,
      mask,
      BOOLEAN_PROPERTIES,
      boolean_properties,
      u32
    );
    write_optional!(self, bytes, mask, BORDER_STYLE, border_style, u8);
    write_optional!(self, bytes, mask, MOUSE_POINTER, mouse_pointer, u8);
    write_optional!(self, bytes, mask, SCROLL_BARS, scroll_bars, u8);
    write_optional!(self, bytes, mask, GROUP_COUNT, group_count, i32);
    write_optional!(self, bytes, mask, MOUSE_ICON, mouse_icon_marker, u16);
    write_optional!(self, bytes, mask, CYCLE, cycle, u8);
    write_optional!(self, bytes, mask, SPECIAL_EFFECT, special_effect, u8);
    write_optional!(self, bytes, mask, BORDER_COLOR, border_color, u32);
    write_count_descriptor(
      bytes,
      mask.contains(FormPropertyMask::CAPTION),
      self.caption.as_ref(),
      "Form.Caption",
    )?;
    write_optional!(self, bytes, mask, FONT, font_marker, u16);
    write_optional!(self, bytes, mask, PICTURE, picture_marker, u16);
    write_optional!(self, bytes, mask, ZOOM, zoom, u32);
    write_optional!(self, bytes, mask, PICTURE_ALIGNMENT, picture_alignment, u8);
    write_optional!(self, bytes, mask, PICTURE_SIZE_MODE, picture_size_mode, u8);
    write_optional!(self, bytes, mask, SHAPE_COOKIE, shape_cookie, u32);
    write_optional!(self, bytes, mask, DRAW_BUFFER, draw_buffer, u32);
    append_padding(bytes, &self.trailing_padding, 4, "Form DataBlock")
  }
}

impl FormExtraDataBlock {
  fn read(
    cursor: &mut SliceCursor<'_>,
    mask: FormPropertyMask,
    data: &FormDataBlock,
  ) -> Result<Self> {
    Ok(Self {
      displayed_size: read_fm_size(cursor, mask.contains(FormPropertyMask::DISPLAYED_SIZE))?,
      logical_size: read_fm_size(cursor, mask.contains(FormPropertyMask::LOGICAL_SIZE))?,
      scroll_position: read_fm_position(cursor, mask.contains(FormPropertyMask::SCROLL_POSITION))?,
      caption: read_fm_string(cursor, data.caption.as_ref())?,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: FormPropertyMask, data: &FormDataBlock) -> Result<()> {
    write_fm_size(
      bytes,
      mask.contains(FormPropertyMask::DISPLAYED_SIZE),
      self.displayed_size,
      "Form.DisplayedSize",
    )?;
    write_fm_size(
      bytes,
      mask.contains(FormPropertyMask::LOGICAL_SIZE),
      self.logical_size,
      "Form.LogicalSize",
    )?;
    write_fm_position(
      bytes,
      mask.contains(FormPropertyMask::SCROLL_POSITION),
      self.scroll_position,
      "Form.ScrollPosition",
    )?;
    write_fm_string(
      bytes,
      data.caption.as_ref(),
      self.caption.as_ref(),
      "Form.Caption",
    )
  }
}

impl FormStreamData {
  fn read(cursor: &mut SliceCursor<'_>, mask: FormPropertyMask) -> Result<Self> {
    Ok(Self {
      mouse_icon: if mask.contains(FormPropertyMask::MOUSE_ICON) {
        Some(cursor.read_picture()?)
      } else {
        None
      },
      font: if mask.contains(FormPropertyMask::FONT) {
        Some(GuidAndFont::read(cursor)?)
      } else {
        None
      },
      picture: if mask.contains(FormPropertyMask::PICTURE) {
        Some(cursor.read_picture()?)
      } else {
        None
      },
    })
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: FormPropertyMask) -> Result<()> {
    write_picture(
      bytes,
      mask.contains(FormPropertyMask::MOUSE_ICON),
      self.mouse_icon.as_ref(),
      "Form.MouseIcon",
    )?;
    match (mask.contains(FormPropertyMask::FONT), self.font.as_ref()) {
      (true, Some(font)) => font.write(bytes)?,
      (false, None) => {}
      _ => return Err(mask_field_mismatch("Form StreamData", "Font")),
    }
    write_picture(
      bytes,
      mask.contains(FormPropertyMask::PICTURE),
      self.picture.as_ref(),
      "Form.Picture",
    )
  }
}

impl GuidAndFont {
  fn read(cursor: &mut SliceCursor<'_>) -> Result<Self> {
    let class_id = cursor.read_guid()?;
    let font = if class_id == Self::STD_FONT_CLASS_ID {
      FormFont::StdFont(StdFont::read(cursor)?)
    } else if class_id == Self::TEXT_PROPS_CLASS_ID {
      let (value, size) = TextProps::from_prefix(&cursor.bytes[cursor.position..cursor.end])?;
      cursor.position += size;
      FormFont::TextProps(Box::new(value))
    } else {
      return Err(Error::invalid(
        cursor.position.saturating_sub(16) as u64,
        "GuidAndFont has an unsupported FormFont CLSID",
      ));
    };
    Ok(Self { class_id, font })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    write_guid(bytes, self.class_id);
    match (&self.font, self.class_id) {
      (FormFont::StdFont(value), Self::STD_FONT_CLASS_ID) => value.write(bytes),
      (FormFont::TextProps(value), Self::TEXT_PROPS_CLASS_ID) => {
        bytes.extend_from_slice(&value.to_bytes()?);
        Ok(())
      }
      _ => Err(Error::invalid(
        0,
        "GuidAndFont CLSID does not match its FormFont variant",
      )),
    }
  }
}

impl StdFont {
  fn read(cursor: &mut SliceCursor<'_>) -> Result<Self> {
    let version = cursor.read_u8()?;
    if version != 1 {
      return Err(Error::invalid(
        cursor.position as u64 - 1,
        "StdFont version must be 1",
      ));
    }
    let character_set = cursor.read_i16()?;
    let flags = StdFontFlags::from_bits_retain(cursor.read_u8()?);
    flags.validate()?;
    let weight = cursor.read_i16()?;
    if !(0..=1000).contains(&weight) {
      return Err(Error::invalid(
        cursor.position as u64 - 2,
        "StdFont weight exceeds 1000",
      ));
    }
    let height = cursor.read_u32()?;
    if height == 0 || height > 655_350_000 {
      return Err(Error::invalid(
        cursor.position as u64 - 4,
        "StdFont height is out of range",
      ));
    }
    let face_len = usize::from(cursor.read_u8()?);
    if face_len >= 32 {
      return Err(Error::invalid(
        cursor.position as u64 - 1,
        "StdFont face exceeds 31 bytes",
      ));
    }
    Ok(Self {
      version,
      character_set,
      flags,
      weight,
      height,
      face: cursor.read_vec(face_len)?,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.version != 1 || !(0..=1000).contains(&self.weight) {
      return Err(Error::invalid(0, "invalid StdFont version or weight"));
    }
    if self.height == 0 || self.height > 655_350_000 || self.face.len() >= 32 {
      return Err(Error::invalid(0, "invalid StdFont height or face length"));
    }
    bytes.push(self.version);
    bytes.extend_from_slice(&self.character_set.to_le_bytes());
    self.flags.validate()?;
    bytes.push(self.flags.bits());
    bytes.extend_from_slice(&self.weight.to_le_bytes());
    bytes.extend_from_slice(&self.height.to_le_bytes());
    bytes.push(self.face.len() as u8);
    bytes.extend_from_slice(&self.face);
    Ok(())
  }
}

impl DesignExtender {
  fn from_prefix(bytes: &[u8]) -> Result<(Self, usize)> {
    if bytes.len() < 8 {
      return Err(Error::invalid(0, "truncated DesignExtender"));
    }
    let mut cursor = SliceCursor::new(bytes);
    let minor_version = cursor.read_u8()?;
    let major_version = cursor.read_u8()?;
    validate_version(minor_version, major_version, "DesignExtender")?;
    let size = usize::from(cursor.read_u16()?);
    if size < 4 {
      return Err(Error::invalid(
        2,
        "cbDesignExtender is smaller than its mask",
      ));
    }
    let end = 4usize
      .checked_add(size)
      .ok_or_else(|| Error::Limit("DesignExtender boundary overflow".into()))?;
    if end > bytes.len() {
      return Err(Error::invalid(2, "cbDesignExtender exceeds the stream"));
    }
    cursor.end = end;
    let property_mask = DesignExtenderPropertyMask::from_bits_retain(cursor.read_u32()?);
    if property_mask.intersects(DesignExtenderPropertyMask::UNUSED) {
      return Err(Error::invalid(4, "DesignExtender has unused mask bits set"));
    }
    use DesignExtenderPropertyMask as Mask;
    let mut data_block = DesignExtenderDataBlock::default();
    read_optional!(
      data_block,
      cursor,
      property_mask,
      BIT_FLAGS,
      bit_flags,
      4,
      read_design_extender_flags
    );
    read_optional!(
      data_block,
      cursor,
      property_mask,
      GRID_X,
      grid_x,
      4,
      read_i32
    );
    read_optional!(
      data_block,
      cursor,
      property_mask,
      GRID_Y,
      grid_y,
      4,
      read_i32
    );
    read_optional!(
      data_block,
      cursor,
      property_mask,
      CLICK_CONTROL_MODE,
      click_control_mode,
      1,
      read_click_control_mode
    );
    read_optional!(
      data_block,
      cursor,
      property_mask,
      DOUBLE_CLICK_CONTROL_MODE,
      double_click_control_mode,
      1,
      read_double_click_control_mode
    );
    data_block.trailing_padding = cursor.read_alignment(4)?;
    if cursor.position != end {
      return Err(Error::invalid(
        cursor.position as u64,
        "DesignExtender fields do not consume cbDesignExtender",
      ));
    }
    Ok((
      Self {
        minor_version,
        major_version,
        property_mask,
        data_block,
      },
      end,
    ))
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_version(self.minor_version, self.major_version, "DesignExtender")?;
    if self
      .property_mask
      .intersects(DesignExtenderPropertyMask::UNUSED)
    {
      return Err(Error::invalid(4, "DesignExtender has unused mask bits set"));
    }
    use DesignExtenderPropertyMask as Mask;
    let mut bytes = vec![self.minor_version, self.major_version, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    let output = &mut bytes;
    write_optional!(
      self.data_block,
      output,
      self.property_mask,
      BIT_FLAGS,
      bit_flags,
      u32
    );
    write_optional!(
      self.data_block,
      output,
      self.property_mask,
      GRID_X,
      grid_x,
      i32
    );
    write_optional!(
      self.data_block,
      output,
      self.property_mask,
      GRID_Y,
      grid_y,
      i32
    );
    write_optional!(
      self.data_block,
      output,
      self.property_mask,
      CLICK_CONTROL_MODE,
      click_control_mode,
      u8
    );
    write_optional!(
      self.data_block,
      output,
      self.property_mask,
      DOUBLE_CLICK_CONTROL_MODE,
      double_click_control_mode,
      u8
    );
    append_padding(
      output,
      &self.data_block.trailing_padding,
      4,
      "DesignExtender DataBlock",
    )?;
    let size = u16::try_from(bytes.len() - 4)
      .map_err(|_| Error::Limit("cbDesignExtender exceeds u16".into()))?;
    bytes[2..4].copy_from_slice(&size.to_le_bytes());
    Ok(bytes)
  }
}

impl FormSiteData {
  fn from_prefix(bytes: &[u8], class_table_omitted: bool) -> Result<(Self, usize)> {
    let mut cursor = SliceCursor::new(bytes);
    let count_of_site_class_info = if class_table_omitted {
      None
    } else {
      Some(cursor.read_u16()?)
    };
    let mut class_table = Vec::with_capacity(usize::from(count_of_site_class_info.unwrap_or(0)));
    for _ in 0..count_of_site_class_info.unwrap_or(0) {
      let (value, size) = SiteClassInfo::from_prefix(&bytes[cursor.position..cursor.end])?;
      cursor.position += size;
      class_table.push(value);
    }
    let count_of_sites = cursor.read_u32()?;
    let count_of_bytes = cursor.read_u32()?;
    let bounded_size = usize::try_from(count_of_bytes)
      .map_err(|_| Error::Limit("FormSiteData CountOfBytes does not fit usize".into()))?;
    let bounded_end = cursor
      .position
      .checked_add(bounded_size)
      .ok_or_else(|| Error::Limit("FormSiteData boundary overflow".into()))?;
    if bounded_end > bytes.len() {
      return Err(Error::invalid(
        cursor.position as u64 - 4,
        "FormSiteData CountOfBytes exceeds the stream",
      ));
    }
    cursor.end = bounded_end;
    let depth_start = cursor.position;
    let mut expanded_count = 0u32;
    let mut depths_and_types = Vec::new();
    while expanded_count < count_of_sites {
      let depth = cursor.read_u8()?;
      let type_or_count = cursor.read_u8()?;
      let compressed_count = type_or_count & 0x80 != 0;
      let count = type_or_count & 0x7f;
      let site_type = if compressed_count {
        cursor.read_u8()?
      } else {
        count
      };
      if site_type != 1 || (compressed_count && count == 0) {
        return Err(Error::invalid(
          cursor.position as u64,
          "FormObjectDepthTypeCount has an invalid site type or count",
        ));
      }
      expanded_count = expanded_count
        .checked_add(if compressed_count {
          u32::from(count)
        } else {
          1
        })
        .ok_or_else(|| Error::Limit("Form site count overflow".into()))?;
      if expanded_count > count_of_sites {
        return Err(Error::invalid(
          cursor.position as u64,
          "Form depth/type array expands past CountOfSites",
        ));
      }
      depths_and_types.push(FormObjectDepthTypeCount {
        depth,
        count: if compressed_count { count } else { 1 },
        site_type,
        compressed_count,
      });
    }
    let depth_size = cursor.position - depth_start;
    let padding_size = depth_size.next_multiple_of(4) - depth_size;
    let array_padding = cursor.read_vec(padding_size)?;
    let mut sites = Vec::with_capacity(
      usize::try_from(count_of_sites)
        .map_err(|_| Error::Limit("Form CountOfSites does not fit usize".into()))?,
    );
    for _ in 0..count_of_sites {
      let (value, size) = OleSiteConcreteControl::from_prefix(&bytes[cursor.position..cursor.end])?;
      cursor.position += size;
      sites.push(value);
    }
    if cursor.position != bounded_end {
      return Err(Error::invalid(
        cursor.position as u64,
        "FormSiteData fields do not consume CountOfBytes",
      ));
    }
    Ok((
      Self {
        count_of_site_class_info,
        class_table,
        count_of_sites,
        count_of_bytes,
        depths_and_types,
        array_padding,
        sites,
      },
      bounded_end,
    ))
  }

  fn to_bytes(&self, class_table_omitted: bool) -> Result<Vec<u8>> {
    let expected_class_count = u16::try_from(self.class_table.len())
      .map_err(|_| Error::Limit("Form class table exceeds u16".into()))?;
    match (class_table_omitted, self.count_of_site_class_info) {
      (true, None) if self.class_table.is_empty() => {}
      (false, Some(count)) if count == expected_class_count => {}
      _ => return Err(mask_field_mismatch("FormSiteData", "ClassTable")),
    }
    let expected_site_count = u32::try_from(self.sites.len())
      .map_err(|_| Error::Limit("Form site array exceeds u32".into()))?;
    if self.count_of_sites != expected_site_count {
      return Err(Error::invalid(
        0,
        "FormSiteData CountOfSites does not match Sites",
      ));
    }
    let expanded_count = self.depths_and_types.iter().try_fold(0u32, |total, item| {
      if item.site_type != 1
        || item.count == 0
        || (!item.compressed_count && item.count != 1)
        || item.count > 0x7f
      {
        return Err(Error::invalid(0, "invalid FormObjectDepthTypeCount"));
      }
      total
        .checked_add(u32::from(item.count))
        .ok_or_else(|| Error::Limit("Form site count overflow".into()))
    })?;
    if expanded_count != self.count_of_sites {
      return Err(Error::invalid(
        0,
        "Form depth/type array does not expand to CountOfSites",
      ));
    }

    let mut bytes = Vec::new();
    if let Some(count) = self.count_of_site_class_info {
      bytes.extend_from_slice(&count.to_le_bytes());
    }
    for class_info in &self.class_table {
      bytes.extend_from_slice(&class_info.to_bytes()?);
    }
    bytes.extend_from_slice(&self.count_of_sites.to_le_bytes());
    let count_offset = bytes.len();
    bytes.extend_from_slice(&[0; 4]);
    let bounded_start = bytes.len();
    let depth_start = bytes.len();
    for item in &self.depths_and_types {
      bytes.push(item.depth);
      if item.compressed_count {
        bytes.push(item.count | 0x80);
        bytes.push(item.site_type);
      } else {
        bytes.push(item.site_type);
      }
    }
    let expected_padding =
      (bytes.len() - depth_start).next_multiple_of(4) - (bytes.len() - depth_start);
    if self.array_padding.len() != expected_padding {
      return Err(Error::invalid(
        bytes.len() as u64,
        "Form depth/type array padding has the wrong length",
      ));
    }
    bytes.extend_from_slice(&self.array_padding);
    for site in &self.sites {
      bytes.extend_from_slice(&site.to_bytes()?);
    }
    let actual_count = u32::try_from(bytes.len() - bounded_start)
      .map_err(|_| Error::Limit("FormSiteData CountOfBytes exceeds u32".into()))?;
    if actual_count != self.count_of_bytes {
      return Err(Error::invalid(
        count_offset as u64,
        "FormSiteData CountOfBytes does not match its fields",
      ));
    }
    bytes[count_offset..count_offset + 4].copy_from_slice(&actual_count.to_le_bytes());
    Ok(bytes)
  }
}

impl OleSiteConcreteControl {
  fn from_prefix(bytes: &[u8]) -> Result<(Self, usize)> {
    if bytes.len() < 8 {
      return Err(Error::invalid(0, "truncated OleSiteConcreteControl"));
    }
    let mut cursor = SliceCursor::new(bytes);
    let version = cursor.read_u16()?;
    if version != 0 {
      return Err(Error::invalid(
        0,
        "OleSiteConcreteControl version must be 0",
      ));
    }
    let size = usize::from(cursor.read_u16()?);
    if size < 4 {
      return Err(Error::invalid(2, "cbSite is smaller than SitePropMask"));
    }
    let end = 4usize
      .checked_add(size)
      .ok_or_else(|| Error::Limit("OleSite boundary overflow".into()))?;
    if end > bytes.len() {
      return Err(Error::invalid(2, "cbSite exceeds FormSiteData"));
    }
    cursor.end = end;
    let property_mask = SitePropertyMask::from_bits_retain(cursor.read_u32()?);
    validate_site_mask(property_mask)?;
    let data_block = SiteDataBlock::read(&mut cursor, property_mask)?;
    validate_site_class_flags(&data_block)?;
    let extra_data_block = SiteExtraDataBlock::read(&mut cursor, property_mask, &data_block)?;
    if cursor.position != end {
      return Err(Error::invalid(
        cursor.position as u64,
        "OleSite fields do not consume cbSite",
      ));
    }
    Ok((
      Self {
        version,
        property_mask,
        data_block,
        extra_data_block,
      },
      end,
    ))
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.version != 0 {
      return Err(Error::invalid(
        0,
        "OleSiteConcreteControl version must be 0",
      ));
    }
    validate_site_mask(self.property_mask)?;
    validate_site_class_flags(&self.data_block)?;
    let mut bytes = vec![0, 0, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    self.data_block.write(&mut bytes, self.property_mask)?;
    self
      .extra_data_block
      .write(&mut bytes, self.property_mask, &self.data_block)?;
    let size =
      u16::try_from(bytes.len() - 4).map_err(|_| Error::Limit("cbSite exceeds u16".into()))?;
    bytes[2..4].copy_from_slice(&size.to_le_bytes());
    Ok(bytes)
  }
}

impl SiteDataBlock {
  fn read(cursor: &mut SliceCursor<'_>, mask: SitePropertyMask) -> Result<Self> {
    use SitePropertyMask as Mask;
    let mut value = Self {
      name: read_count_descriptor(cursor, mask.contains(Mask::NAME))?,
      tag: read_count_descriptor(cursor, mask.contains(Mask::TAG))?,
      ..Self::default()
    };
    read_optional!(value, cursor, mask, ID, id, 4, read_i32);
    read_optional!(
      value,
      cursor,
      mask,
      HELP_CONTEXT_ID,
      help_context_id,
      4,
      read_i32
    );
    read_optional!(
      value,
      cursor,
      mask,
      BIT_FLAGS,
      bit_flags,
      4,
      read_site_flags
    );
    read_optional!(
      value,
      cursor,
      mask,
      OBJECT_STREAM_SIZE,
      object_stream_size,
      4,
      read_u32
    );
    read_optional!(value, cursor, mask, TAB_INDEX, tab_index, 2, read_i16);
    read_optional!(
      value,
      cursor,
      mask,
      CLSID_CACHE_INDEX,
      clsid_cache_index,
      2,
      read_site_class_index
    );
    read_optional!(value, cursor, mask, GROUP_ID, group_id, 2, read_u16);
    value.control_tip_text = read_count_descriptor(cursor, mask.contains(Mask::CONTROL_TIP_TEXT))?;
    value.runtime_license_key =
      read_count_descriptor(cursor, mask.contains(Mask::RUNTIME_LICENSE_KEY))?;
    value.control_source = read_count_descriptor(cursor, mask.contains(Mask::CONTROL_SOURCE))?;
    value.row_source = read_count_descriptor(cursor, mask.contains(Mask::ROW_SOURCE))?;
    value.trailing_padding = cursor.read_alignment(4)?;
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: SitePropertyMask) -> Result<()> {
    use SitePropertyMask as Mask;
    write_count_descriptor(
      bytes,
      mask.contains(Mask::NAME),
      self.name.as_ref(),
      "Site.Name",
    )?;
    write_count_descriptor(
      bytes,
      mask.contains(Mask::TAG),
      self.tag.as_ref(),
      "Site.Tag",
    )?;
    write_optional!(self, bytes, mask, ID, id, i32);
    write_optional!(self, bytes, mask, HELP_CONTEXT_ID, help_context_id, i32);
    write_optional!(self, bytes, mask, BIT_FLAGS, bit_flags, u32);
    write_optional!(
      self,
      bytes,
      mask,
      OBJECT_STREAM_SIZE,
      object_stream_size,
      u32
    );
    write_optional!(self, bytes, mask, TAB_INDEX, tab_index, i16);
    write_optional!(self, bytes, mask, CLSID_CACHE_INDEX, clsid_cache_index, u16);
    write_optional!(self, bytes, mask, GROUP_ID, group_id, u16);
    write_count_descriptor(
      bytes,
      mask.contains(Mask::CONTROL_TIP_TEXT),
      self.control_tip_text.as_ref(),
      "Site.ControlTipText",
    )?;
    write_count_descriptor(
      bytes,
      mask.contains(Mask::RUNTIME_LICENSE_KEY),
      self.runtime_license_key.as_ref(),
      "Site.RuntimeLicenseKey",
    )?;
    write_count_descriptor(
      bytes,
      mask.contains(Mask::CONTROL_SOURCE),
      self.control_source.as_ref(),
      "Site.ControlSource",
    )?;
    write_count_descriptor(
      bytes,
      mask.contains(Mask::ROW_SOURCE),
      self.row_source.as_ref(),
      "Site.RowSource",
    )?;
    append_padding(bytes, &self.trailing_padding, 4, "Site DataBlock")
  }
}

impl SiteExtraDataBlock {
  fn read(
    cursor: &mut SliceCursor<'_>,
    mask: SitePropertyMask,
    data: &SiteDataBlock,
  ) -> Result<Self> {
    Ok(Self {
      name: read_fm_string(cursor, data.name.as_ref())?,
      tag: read_fm_string(cursor, data.tag.as_ref())?,
      position: read_fm_position(cursor, mask.contains(SitePropertyMask::POSITION))?,
      control_tip_text: read_fm_string(cursor, data.control_tip_text.as_ref())?,
      runtime_license_key: read_fm_string(cursor, data.runtime_license_key.as_ref())?,
      control_source: read_fm_string(cursor, data.control_source.as_ref())?,
      row_source: read_fm_string(cursor, data.row_source.as_ref())?,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: SitePropertyMask, data: &SiteDataBlock) -> Result<()> {
    write_fm_string(bytes, data.name.as_ref(), self.name.as_ref(), "Site.Name")?;
    write_fm_string(bytes, data.tag.as_ref(), self.tag.as_ref(), "Site.Tag")?;
    write_fm_position(
      bytes,
      mask.contains(SitePropertyMask::POSITION),
      self.position,
      "Site.Position",
    )?;
    write_fm_string(
      bytes,
      data.control_tip_text.as_ref(),
      self.control_tip_text.as_ref(),
      "Site.ControlTipText",
    )?;
    write_fm_string(
      bytes,
      data.runtime_license_key.as_ref(),
      self.runtime_license_key.as_ref(),
      "Site.RuntimeLicenseKey",
    )?;
    write_fm_string(
      bytes,
      data.control_source.as_ref(),
      self.control_source.as_ref(),
      "Site.ControlSource",
    )?;
    write_fm_string(
      bytes,
      data.row_source.as_ref(),
      self.row_source.as_ref(),
      "Site.RowSource",
    )
  }
}

impl SiteClassInfo {
  fn from_prefix(bytes: &[u8]) -> Result<(Self, usize)> {
    if bytes.len() < 8 {
      return Err(Error::invalid(0, "truncated SiteClassInfo"));
    }
    let mut cursor = SliceCursor::new(bytes);
    let version = cursor.read_u16()?;
    if version != 0 {
      return Err(Error::invalid(0, "SiteClassInfo version must be 0"));
    }
    let size = usize::from(cursor.read_u16()?);
    if size < 4 {
      return Err(Error::invalid(2, "cbClassTable is smaller than its mask"));
    }
    let end = 4usize
      .checked_add(size)
      .ok_or_else(|| Error::Limit("SiteClassInfo boundary overflow".into()))?;
    if end > bytes.len() {
      return Err(Error::invalid(2, "cbClassTable exceeds FormSiteData"));
    }
    cursor.end = end;
    let property_mask = ClassInfoPropertyMask::from_bits_retain(cursor.read_u32()?);
    validate_class_info_mask(property_mask)?;
    let data_block = ClassInfoDataBlock::read(&mut cursor, property_mask)?;
    let extra_data_block = ClassInfoExtraDataBlock::read(&mut cursor, property_mask)?;
    if cursor.position != end {
      return Err(Error::invalid(
        cursor.position as u64,
        "SiteClassInfo fields do not consume cbClassTable",
      ));
    }
    Ok((
      Self {
        version,
        property_mask,
        data_block,
        extra_data_block,
      },
      end,
    ))
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.version != 0 {
      return Err(Error::invalid(0, "SiteClassInfo version must be 0"));
    }
    validate_class_info_mask(self.property_mask)?;
    let mut bytes = vec![0, 0, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    self.data_block.write(&mut bytes, self.property_mask)?;
    self
      .extra_data_block
      .write(&mut bytes, self.property_mask)?;
    let size = u16::try_from(bytes.len() - 4)
      .map_err(|_| Error::Limit("cbClassTable exceeds u16".into()))?;
    bytes[2..4].copy_from_slice(&size.to_le_bytes());
    Ok(bytes)
  }
}

impl ClassInfoDataBlock {
  fn read(cursor: &mut SliceCursor<'_>, mask: ClassInfoPropertyMask) -> Result<Self> {
    use ClassInfoPropertyMask as Mask;
    let mut value = Self::default();
    read_optional!(
      value,
      cursor,
      mask,
      CLASS_FLAGS,
      class_table_flags,
      2,
      read_class_table_flags
    );
    read_optional!(
      value,
      cursor,
      mask,
      CLASS_FLAGS,
      variable_flags,
      2,
      read_variable_flags
    );
    read_optional!(
      value,
      cursor,
      mask,
      COUNT_OF_METHODS,
      count_of_methods,
      4,
      read_u32
    );
    read_optional!(value, cursor, mask, DISPID_BIND, dispid_bind, 4, read_u32);
    read_optional!(
      value,
      cursor,
      mask,
      GET_BIND_INDEX,
      get_bind_index,
      2,
      read_u16
    );
    read_optional!(
      value,
      cursor,
      mask,
      PUT_BIND_INDEX,
      put_bind_index,
      2,
      read_u16
    );
    read_optional!(
      value,
      cursor,
      mask,
      BIND_TYPE,
      bind_type,
      2,
      read_variant_type
    );
    read_optional!(
      value,
      cursor,
      mask,
      GET_VALUE_INDEX,
      get_value_index,
      2,
      read_u16
    );
    read_optional!(
      value,
      cursor,
      mask,
      PUT_VALUE_INDEX,
      put_value_index,
      2,
      read_u16
    );
    read_optional!(
      value,
      cursor,
      mask,
      VALUE_TYPE,
      value_type,
      2,
      read_variant_type
    );
    read_optional!(
      value,
      cursor,
      mask,
      DISPID_ROWSET,
      dispid_rowset,
      4,
      read_u32
    );
    read_optional!(value, cursor, mask, SET_ROWSET, set_rowset, 2, read_u16);
    value.trailing_padding = cursor.read_alignment(4)?;
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: ClassInfoPropertyMask) -> Result<()> {
    use ClassInfoPropertyMask as Mask;
    write_optional!(self, bytes, mask, CLASS_FLAGS, class_table_flags, u16);
    write_optional!(self, bytes, mask, CLASS_FLAGS, variable_flags, u16);
    write_optional!(self, bytes, mask, COUNT_OF_METHODS, count_of_methods, u32);
    write_optional!(self, bytes, mask, DISPID_BIND, dispid_bind, u32);
    write_optional!(self, bytes, mask, GET_BIND_INDEX, get_bind_index, u16);
    write_optional!(self, bytes, mask, PUT_BIND_INDEX, put_bind_index, u16);
    write_optional!(self, bytes, mask, BIND_TYPE, bind_type, u16);
    write_optional!(self, bytes, mask, GET_VALUE_INDEX, get_value_index, u16);
    write_optional!(self, bytes, mask, PUT_VALUE_INDEX, put_value_index, u16);
    write_optional!(self, bytes, mask, VALUE_TYPE, value_type, u16);
    write_optional!(self, bytes, mask, DISPID_ROWSET, dispid_rowset, u32);
    write_optional!(self, bytes, mask, SET_ROWSET, set_rowset, u16);
    append_padding(bytes, &self.trailing_padding, 4, "ClassInfo DataBlock")
  }
}

impl ClassInfoExtraDataBlock {
  fn read(cursor: &mut SliceCursor<'_>, mask: ClassInfoPropertyMask) -> Result<Self> {
    Ok(Self {
      class_id: if mask.contains(ClassInfoPropertyMask::CLSID) {
        Some(cursor.read_guid()?)
      } else {
        None
      },
      dispatch_event: if mask.contains(ClassInfoPropertyMask::DISPATCH_EVENT) {
        Some(cursor.read_guid()?)
      } else {
        None
      },
      default_program: if mask.contains(ClassInfoPropertyMask::DEFAULT_PROGRAM) {
        Some(cursor.read_guid()?)
      } else {
        None
      },
    })
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: ClassInfoPropertyMask) -> Result<()> {
    write_masked_guid(
      bytes,
      mask.contains(ClassInfoPropertyMask::CLSID),
      self.class_id,
      "SiteClassInfo.Clsid",
    )?;
    write_masked_guid(
      bytes,
      mask.contains(ClassInfoPropertyMask::DISPATCH_EVENT),
      self.dispatch_event,
      "SiteClassInfo.DispatchEvent",
    )?;
    write_masked_guid(
      bytes,
      mask.contains(ClassInfoPropertyMask::DEFAULT_PROGRAM),
      self.default_program,
      "SiteClassInfo.DefaultProgram",
    )
  }
}

impl CommandButtonControl {
  const MASK_SIZE: usize = 4;

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 + Self::MASK_SIZE {
      return Err(Error::invalid(
        0,
        "truncated MS-OFORMS CommandButtonControl",
      ));
    }
    let mut cursor = SliceCursor::new(bytes);
    let minor_version = cursor.read_u8()?;
    let major_version = cursor.read_u8()?;
    validate_version(minor_version, major_version, "CommandButtonControl")?;
    let property_size = usize::from(cursor.read_u16()?);
    if property_size < Self::MASK_SIZE {
      return Err(Error::invalid(
        2,
        "cbCommandButton is smaller than CommandButtonPropMask",
      ));
    }
    let property_end = 4usize
      .checked_add(property_size)
      .ok_or_else(|| Error::Limit("CommandButton property boundary overflow".into()))?;
    if property_end > bytes.len() {
      return Err(Error::invalid(
        2,
        "cbCommandButton exceeds the control stream",
      ));
    }
    cursor.end = property_end;
    let property_mask = CommandButtonPropertyMask::from_bits_retain(cursor.read_u32()?);
    validate_command_button_mask(property_mask)?;
    let data_block = CommandButtonDataBlock::read(&mut cursor, property_mask)?;
    validate_control_various(
      data_block.various_property_bits.as_ref(),
      CachedControlClass::CommandButton,
    )?;
    let extra_data_block =
      CommandButtonExtraDataBlock::read(&mut cursor, property_mask, &data_block)?;
    if cursor.position != property_end {
      return Err(Error::invalid(
        cursor.position as u64,
        "CommandButton property fields do not consume cbCommandButton",
      ));
    }
    cursor.end = bytes.len();
    let stream_data = CommandButtonStreamData::read(&mut cursor, property_mask)?;
    let (text_props, text_props_size) = TextProps::from_prefix(&bytes[cursor.position..])?;
    cursor.position += text_props_size;
    if cursor.position != bytes.len() {
      return Err(Error::invalid(
        cursor.position as u64,
        "unexpected bytes after CommandButton TextProps",
      ));
    }
    Ok(Self {
      minor_version,
      major_version,
      property_mask,
      take_focus_on_click: !property_mask.contains(CommandButtonPropertyMask::TAKE_FOCUS_ON_CLICK),
      data_block,
      extra_data_block,
      stream_data,
      text_props,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_version(
      self.minor_version,
      self.major_version,
      "CommandButtonControl",
    )?;
    validate_command_button_mask(self.property_mask)?;
    validate_control_various(
      self.data_block.various_property_bits.as_ref(),
      CachedControlClass::CommandButton,
    )?;
    if self.take_focus_on_click
      == self
        .property_mask
        .contains(CommandButtonPropertyMask::TAKE_FOCUS_ON_CLICK)
    {
      return Err(Error::invalid(
        0,
        "CommandButton TakeFocusOnClick does not match its inverse mask bit",
      ));
    }
    let mut bytes = vec![self.minor_version, self.major_version, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    self.data_block.write(&mut bytes, self.property_mask)?;
    self
      .extra_data_block
      .write(&mut bytes, self.property_mask, &self.data_block)?;
    let property_size = u16::try_from(bytes.len() - 4)
      .map_err(|_| Error::Limit("cbCommandButton exceeds u16".into()))?;
    bytes[2..4].copy_from_slice(&property_size.to_le_bytes());
    self.stream_data.write(&mut bytes, self.property_mask)?;
    bytes.extend_from_slice(&self.text_props.to_bytes()?);
    Ok(bytes)
  }
}

impl CommandButtonDataBlock {
  fn read(cursor: &mut SliceCursor<'_>, mask: CommandButtonPropertyMask) -> Result<Self> {
    use CommandButtonPropertyMask as Mask;

    let mut value = Self::default();
    read_optional!(
      value,
      cursor,
      mask,
      FORE_COLOR,
      fore_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      BACK_COLOR,
      back_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      4,
      read_various_properties
    );
    if mask.contains(CommandButtonPropertyMask::CAPTION) {
      value.caption = Some(AlignedValue {
        padding_before: cursor.read_alignment(4)?,
        value: CountOfBytesWithCompressionFlag::from_raw(cursor.read_u32()?),
      });
    }
    read_optional!(
      value,
      cursor,
      mask,
      PICTURE_POSITION,
      picture_position,
      4,
      read_picture_position
    );
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_POINTER,
      mouse_pointer,
      1,
      read_mouse_pointer
    );
    read_optional!(
      value,
      cursor,
      mask,
      PICTURE,
      picture_marker,
      2,
      read_persistence_marker
    );
    read_optional!(value, cursor, mask, ACCELERATOR, accelerator, 2, read_u16);
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_ICON,
      mouse_icon_marker,
      2,
      read_persistence_marker
    );
    value.trailing_padding = cursor.read_alignment(4)?;
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: CommandButtonPropertyMask) -> Result<()> {
    use CommandButtonPropertyMask as Mask;

    write_optional!(self, bytes, mask, FORE_COLOR, fore_color, u32);
    write_optional!(self, bytes, mask, BACK_COLOR, back_color, u32);
    write_optional!(
      self,
      bytes,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      u32
    );
    match (
      mask.contains(CommandButtonPropertyMask::CAPTION),
      self.caption.as_ref(),
    ) {
      (true, Some(value)) => {
        append_padding(bytes, &value.padding_before, 4, "CommandButton.Caption")?;
        bytes.extend_from_slice(&value.value.to_raw()?.to_le_bytes());
      }
      (false, None) => {}
      _ => return Err(mask_field_mismatch("CommandButton", "Caption")),
    }
    write_optional!(self, bytes, mask, PICTURE_POSITION, picture_position, u32);
    write_optional!(self, bytes, mask, MOUSE_POINTER, mouse_pointer, u8);
    write_optional!(self, bytes, mask, PICTURE, picture_marker, u16);
    write_optional!(self, bytes, mask, ACCELERATOR, accelerator, u16);
    write_optional!(self, bytes, mask, MOUSE_ICON, mouse_icon_marker, u16);
    append_padding(bytes, &self.trailing_padding, 4, "CommandButton DataBlock")
  }
}

impl CommandButtonExtraDataBlock {
  fn read(
    cursor: &mut SliceCursor<'_>,
    mask: CommandButtonPropertyMask,
    data: &CommandButtonDataBlock,
  ) -> Result<Self> {
    let caption = read_fm_string(cursor, data.caption.as_ref())?;
    let size = if mask.contains(CommandButtonPropertyMask::SIZE) {
      Some(FmSize {
        width: cursor.read_i32()?,
        height: cursor.read_i32()?,
      })
    } else {
      None
    };
    Ok(Self { caption, size })
  }

  fn write(
    &self,
    bytes: &mut Vec<u8>,
    mask: CommandButtonPropertyMask,
    data: &CommandButtonDataBlock,
  ) -> Result<()> {
    write_fm_string(
      bytes,
      data.caption.as_ref(),
      self.caption.as_ref(),
      "CommandButton.Caption",
    )?;
    match (mask.contains(CommandButtonPropertyMask::SIZE), self.size) {
      (true, Some(size)) => {
        bytes.extend_from_slice(&size.width.to_le_bytes());
        bytes.extend_from_slice(&size.height.to_le_bytes());
        Ok(())
      }
      (false, None) => Ok(()),
      _ => Err(mask_field_mismatch("CommandButton", "Size")),
    }
  }
}

impl CommandButtonStreamData {
  fn read(cursor: &mut SliceCursor<'_>, mask: CommandButtonPropertyMask) -> Result<Self> {
    Ok(Self {
      picture: if mask.contains(CommandButtonPropertyMask::PICTURE) {
        Some(cursor.read_picture()?)
      } else {
        None
      },
      mouse_icon: if mask.contains(CommandButtonPropertyMask::MOUSE_ICON) {
        Some(cursor.read_picture()?)
      } else {
        None
      },
    })
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: CommandButtonPropertyMask) -> Result<()> {
    write_picture(
      bytes,
      mask.contains(CommandButtonPropertyMask::PICTURE),
      self.picture.as_ref(),
      "CommandButton.Picture",
    )?;
    write_picture(
      bytes,
      mask.contains(CommandButtonPropertyMask::MOUSE_ICON),
      self.mouse_icon.as_ref(),
      "CommandButton.MouseIcon",
    )
  }
}

fn read_cached_form_control(
  class: CachedControlClass,
  bytes: &[u8],
  position: usize,
) -> Result<FormControlPersistence> {
  match class {
    CachedControlClass::Image => Ok(FormControlPersistence::Image(Box::new(
      ImageControl::from_bytes(bytes)?,
    ))),
    CachedControlClass::SpinButton => Ok(FormControlPersistence::SpinButton(Box::new(
      SpinButtonControl::from_bytes(bytes)?,
    ))),
    CachedControlClass::CommandButton => Ok(FormControlPersistence::CommandButton(Box::new(
      CommandButtonControl::from_bytes(bytes)?,
    ))),
    CachedControlClass::TabStrip => Ok(FormControlPersistence::TabStrip(Box::new(
      TabStripControl::from_bytes(bytes)?,
    ))),
    CachedControlClass::Label => Ok(FormControlPersistence::Label(Box::new(
      LabelControl::from_bytes(bytes)?,
    ))),
    CachedControlClass::MorphDataLegacy
    | CachedControlClass::TextBox
    | CachedControlClass::ListBox
    | CachedControlClass::ComboBox
    | CachedControlClass::CheckBox
    | CachedControlClass::OptionButton
    | CachedControlClass::ToggleButton => {
      let value = MorphDataControl::from_bytes(bytes)?;
      validate_morph_cached_class(&value, class)?;
      Ok(FormControlPersistence::MorphData(Box::new(value)))
    }
    CachedControlClass::ScrollBar => Ok(FormControlPersistence::ScrollBar(Box::new(
      ScrollBarControl::from_bytes(bytes)?,
    ))),
    _ => Err(Error::invalid(
      position as u64,
      format!("cached MS-OFORMS class {class:?} is not implemented as an o-stream leaf"),
    )),
  }
}

fn write_cached_form_control(
  value: &FormControlPersistence,
  class: CachedControlClass,
) -> Result<Vec<u8>> {
  match (value, class) {
    (FormControlPersistence::Image(value), CachedControlClass::Image) => value.to_bytes(),
    (FormControlPersistence::SpinButton(value), CachedControlClass::SpinButton) => value.to_bytes(),
    (FormControlPersistence::CommandButton(value), CachedControlClass::CommandButton) => {
      value.to_bytes()
    }
    (FormControlPersistence::TabStrip(value), CachedControlClass::TabStrip) => value.to_bytes(),
    (FormControlPersistence::Label(value), CachedControlClass::Label) => value.to_bytes(),
    (
      FormControlPersistence::MorphData(value),
      CachedControlClass::MorphDataLegacy
      | CachedControlClass::TextBox
      | CachedControlClass::ListBox
      | CachedControlClass::ComboBox
      | CachedControlClass::CheckBox
      | CachedControlClass::OptionButton
      | CachedControlClass::ToggleButton,
    ) => {
      validate_morph_cached_class(value, class)?;
      value.to_bytes()
    }
    (FormControlPersistence::ScrollBar(value), CachedControlClass::ScrollBar) => value.to_bytes(),
    _ => Err(Error::invalid(
      0,
      "FormObjectControl persistence does not match its cached class index",
    )),
  }
}

impl MorphDataControl {
  const MASK_SIZE: usize = 8;

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 4 + Self::MASK_SIZE {
      return Err(Error::invalid(0, "truncated MS-OFORMS MorphDataControl"));
    }
    let mut cursor = SliceCursor::new(bytes);
    let minor_version = cursor.read_u8()?;
    let major_version = cursor.read_u8()?;
    validate_version(minor_version, major_version, "MorphDataControl")?;
    let property_size = usize::from(cursor.read_u16()?);
    if property_size < Self::MASK_SIZE {
      return Err(Error::invalid(
        2,
        "cbMorphData is smaller than MorphDataPropMask",
      ));
    }
    let property_end = 4usize
      .checked_add(property_size)
      .ok_or_else(|| Error::Limit("MorphDataControl property boundary overflow".into()))?;
    if property_end > bytes.len() {
      return Err(Error::invalid(2, "cbMorphData exceeds the control stream"));
    }
    cursor.end = property_end;
    let property_mask = MorphDataPropertyMask::from_bits_retain(cursor.read_u64()?);
    validate_morph_mask(property_mask)?;
    let data_block = MorphDataDataBlock::read(&mut cursor, property_mask)?;
    validate_morph_data(&data_block)?;
    let extra_data_block = MorphDataExtraDataBlock::read(&mut cursor, property_mask, &data_block)?;
    if cursor.position != property_end {
      return Err(Error::invalid(
        cursor.position as u64,
        "MorphData property fields do not consume cbMorphData",
      ));
    }

    cursor.end = bytes.len();
    let stream_data = MorphDataStreamData::read(&mut cursor, property_mask)?;
    let (text_props, text_props_size) = TextProps::from_prefix(&bytes[cursor.position..])?;
    cursor.position += text_props_size;
    let column_count = data_block
      .column_info_count
      .as_ref()
      .map_or(0, |value| usize::from(value.value));
    let mut column_info = Vec::with_capacity(column_count);
    for _ in 0..column_count {
      let (value, size) = MorphDataColumnInfo::from_prefix(&bytes[cursor.position..])?;
      cursor.position += size;
      column_info.push(value);
    }
    if cursor.position != bytes.len() {
      return Err(Error::invalid(
        cursor.position as u64,
        "unexpected bytes after MorphData column information",
      ));
    }
    Ok(Self {
      minor_version,
      major_version,
      property_mask,
      data_block,
      extra_data_block,
      stream_data,
      text_props,
      column_info,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_version(self.minor_version, self.major_version, "MorphDataControl")?;
    validate_morph_mask(self.property_mask)?;
    validate_morph_data(&self.data_block)?;
    let expected_columns = self
      .data_block
      .column_info_count
      .as_ref()
      .map_or(0, |value| usize::from(value.value));
    if expected_columns != self.column_info.len() {
      return Err(Error::invalid(
        0,
        "MorphData cColumnInfo does not match rgColumnInfo length",
      ));
    }

    let mut bytes = vec![self.minor_version, self.major_version, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    self.data_block.write(&mut bytes, self.property_mask)?;
    self
      .extra_data_block
      .write(&mut bytes, self.property_mask, &self.data_block)?;
    let property_size = bytes
      .len()
      .checked_sub(4)
      .ok_or_else(|| Error::Limit("MorphData property size underflow".into()))?;
    let property_size =
      u16::try_from(property_size).map_err(|_| Error::Limit("cbMorphData exceeds u16".into()))?;
    bytes[2..4].copy_from_slice(&property_size.to_le_bytes());
    self.stream_data.write(&mut bytes, self.property_mask)?;
    bytes.extend_from_slice(&self.text_props.to_bytes()?);
    for column in &self.column_info {
      bytes.extend_from_slice(&column.to_bytes()?);
    }
    Ok(bytes)
  }

  pub fn data_and_extra_size(&self) -> Result<usize> {
    let bytes = self.to_bytes()?;
    Ok(usize::from(u16::from_le_bytes([bytes[2], bytes[3]])) - Self::MASK_SIZE)
  }

  pub fn following_data_size(&self) -> Result<usize> {
    let bytes = self.to_bytes()?;
    let property_end = 4 + usize::from(u16::from_le_bytes([bytes[2], bytes[3]]));
    Ok(bytes.len() - property_end)
  }
}

impl MorphDataDataBlock {
  fn read(cursor: &mut SliceCursor<'_>, mask: MorphDataPropertyMask) -> Result<Self> {
    use MorphDataPropertyMask as Mask;

    let mut value = Self::default();
    read_optional!(
      value,
      cursor,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      4,
      read_various_properties
    );
    read_optional!(
      value,
      cursor,
      mask,
      BACK_COLOR,
      back_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      FORE_COLOR,
      fore_color,
      4,
      read_ole_color
    );
    read_optional!(value, cursor, mask, MAX_LENGTH, max_length, 4, read_u32);
    read_optional!(
      value,
      cursor,
      mask,
      BORDER_STYLE,
      border_style,
      1,
      read_border_style_u8
    );
    read_optional!(
      value,
      cursor,
      mask,
      SCROLL_BARS,
      scroll_bars,
      1,
      read_scroll_bars
    );
    read_optional!(
      value,
      cursor,
      mask,
      DISPLAY_STYLE,
      display_style,
      1,
      read_display_style
    );
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_POINTER,
      mouse_pointer,
      1,
      read_mouse_pointer
    );
    read_optional!(
      value,
      cursor,
      mask,
      PASSWORD_CHAR,
      password_char,
      2,
      read_u16
    );
    read_optional!(value, cursor, mask, LIST_WIDTH, list_width, 4, read_u32);
    read_optional!(value, cursor, mask, BOUND_COLUMN, bound_column, 2, read_u16);
    read_optional!(value, cursor, mask, TEXT_COLUMN, text_column, 2, read_i16);
    read_optional!(value, cursor, mask, COLUMN_COUNT, column_count, 2, read_i16);
    read_optional!(value, cursor, mask, LIST_ROWS, list_rows, 2, read_u16);
    read_optional!(
      value,
      cursor,
      mask,
      COLUMN_INFO_COUNT,
      column_info_count,
      2,
      read_u16
    );
    read_optional!(
      value,
      cursor,
      mask,
      MATCH_ENTRY,
      match_entry,
      1,
      read_match_entry
    );
    read_optional!(
      value,
      cursor,
      mask,
      LIST_STYLE,
      list_style,
      1,
      read_list_style
    );
    read_optional!(
      value,
      cursor,
      mask,
      SHOW_DROP_BUTTON_WHEN,
      show_drop_button_when,
      1,
      read_show_drop_button_when
    );
    read_optional!(
      value,
      cursor,
      mask,
      DROP_BUTTON_STYLE,
      drop_button_style,
      1,
      read_drop_button_style
    );
    read_optional!(
      value,
      cursor,
      mask,
      MULTI_SELECT,
      multi_select,
      1,
      read_multi_select
    );
    read_descriptor!(value, cursor, mask, VALUE, value);
    read_descriptor!(value, cursor, mask, CAPTION, caption);
    read_optional!(
      value,
      cursor,
      mask,
      PICTURE_POSITION,
      picture_position,
      4,
      read_picture_position
    );
    read_optional!(
      value,
      cursor,
      mask,
      BORDER_COLOR,
      border_color,
      4,
      read_ole_color
    );
    read_optional!(
      value,
      cursor,
      mask,
      SPECIAL_EFFECT,
      special_effect,
      4,
      read_special_effect_u32
    );
    read_optional!(
      value,
      cursor,
      mask,
      MOUSE_ICON,
      mouse_icon_marker,
      2,
      read_persistence_marker
    );
    read_optional!(
      value,
      cursor,
      mask,
      PICTURE,
      picture_marker,
      2,
      read_persistence_marker
    );
    read_optional!(value, cursor, mask, ACCELERATOR, accelerator, 2, read_u16);
    read_descriptor!(value, cursor, mask, GROUP_NAME, group_name);
    value.trailing_padding = cursor.read_alignment(4)?;
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: MorphDataPropertyMask) -> Result<()> {
    use MorphDataPropertyMask as Mask;

    write_optional!(
      self,
      bytes,
      mask,
      VARIOUS_PROPERTY_BITS,
      various_property_bits,
      u32
    );
    write_optional!(self, bytes, mask, BACK_COLOR, back_color, u32);
    write_optional!(self, bytes, mask, FORE_COLOR, fore_color, u32);
    write_optional!(self, bytes, mask, MAX_LENGTH, max_length, u32);
    write_optional!(self, bytes, mask, BORDER_STYLE, border_style, u8);
    write_optional!(self, bytes, mask, SCROLL_BARS, scroll_bars, u8);
    write_optional!(self, bytes, mask, DISPLAY_STYLE, display_style, u8);
    write_optional!(self, bytes, mask, MOUSE_POINTER, mouse_pointer, u8);
    write_optional!(self, bytes, mask, PASSWORD_CHAR, password_char, u16);
    write_optional!(self, bytes, mask, LIST_WIDTH, list_width, u32);
    write_optional!(self, bytes, mask, BOUND_COLUMN, bound_column, u16);
    write_optional!(self, bytes, mask, TEXT_COLUMN, text_column, i16);
    write_optional!(self, bytes, mask, COLUMN_COUNT, column_count, i16);
    write_optional!(self, bytes, mask, LIST_ROWS, list_rows, u16);
    write_optional!(self, bytes, mask, COLUMN_INFO_COUNT, column_info_count, u16);
    write_optional!(self, bytes, mask, MATCH_ENTRY, match_entry, u8);
    write_optional!(self, bytes, mask, LIST_STYLE, list_style, u8);
    write_optional!(
      self,
      bytes,
      mask,
      SHOW_DROP_BUTTON_WHEN,
      show_drop_button_when,
      u8
    );
    write_optional!(self, bytes, mask, DROP_BUTTON_STYLE, drop_button_style, u8);
    write_optional!(self, bytes, mask, MULTI_SELECT, multi_select, u8);
    write_descriptor!(self, bytes, mask, VALUE, value);
    write_descriptor!(self, bytes, mask, CAPTION, caption);
    write_optional!(self, bytes, mask, PICTURE_POSITION, picture_position, u32);
    write_optional!(self, bytes, mask, BORDER_COLOR, border_color, u32);
    write_optional!(self, bytes, mask, SPECIAL_EFFECT, special_effect, u32);
    write_optional!(self, bytes, mask, MOUSE_ICON, mouse_icon_marker, u16);
    write_optional!(self, bytes, mask, PICTURE, picture_marker, u16);
    write_optional!(self, bytes, mask, ACCELERATOR, accelerator, u16);
    write_descriptor!(self, bytes, mask, GROUP_NAME, group_name);
    append_padding(bytes, &self.trailing_padding, 4, "MorphData DataBlock")
  }
}

impl MorphDataExtraDataBlock {
  fn read(
    cursor: &mut SliceCursor<'_>,
    mask: MorphDataPropertyMask,
    data: &MorphDataDataBlock,
  ) -> Result<Self> {
    let size = if mask.contains(MorphDataPropertyMask::SIZE) {
      Some(FmSize {
        width: cursor.read_i32()?,
        height: cursor.read_i32()?,
      })
    } else {
      None
    };
    let value = read_fm_string(cursor, data.value.as_ref())?;
    let caption = read_fm_string(cursor, data.caption.as_ref())?;
    let group_name = read_fm_string(cursor, data.group_name.as_ref())?;
    Ok(Self {
      size,
      value,
      caption,
      group_name,
    })
  }

  fn write(
    &self,
    bytes: &mut Vec<u8>,
    mask: MorphDataPropertyMask,
    data: &MorphDataDataBlock,
  ) -> Result<()> {
    match (mask.contains(MorphDataPropertyMask::SIZE), self.size) {
      (true, Some(size)) => {
        bytes.extend_from_slice(&size.width.to_le_bytes());
        bytes.extend_from_slice(&size.height.to_le_bytes());
      }
      (false, None) => {}
      _ => return Err(mask_field_mismatch("MorphData", "Size")),
    }
    write_fm_string(bytes, data.value.as_ref(), self.value.as_ref(), "Value")?;
    write_fm_string(
      bytes,
      data.caption.as_ref(),
      self.caption.as_ref(),
      "Caption",
    )?;
    write_fm_string(
      bytes,
      data.group_name.as_ref(),
      self.group_name.as_ref(),
      "GroupName",
    )
  }
}

impl MorphDataStreamData {
  fn read(cursor: &mut SliceCursor<'_>, mask: MorphDataPropertyMask) -> Result<Self> {
    Ok(Self {
      mouse_icon: if mask.contains(MorphDataPropertyMask::MOUSE_ICON) {
        Some(cursor.read_picture()?)
      } else {
        None
      },
      picture: if mask.contains(MorphDataPropertyMask::PICTURE) {
        Some(cursor.read_picture()?)
      } else {
        None
      },
    })
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: MorphDataPropertyMask) -> Result<()> {
    write_picture(
      bytes,
      mask.contains(MorphDataPropertyMask::MOUSE_ICON),
      self.mouse_icon.as_ref(),
      "MouseIcon",
    )?;
    write_picture(
      bytes,
      mask.contains(MorphDataPropertyMask::PICTURE),
      self.picture.as_ref(),
      "Picture",
    )
  }
}

impl TextProps {
  fn from_prefix(bytes: &[u8]) -> Result<(Self, usize)> {
    if bytes.len() < 8 {
      return Err(Error::invalid(0, "truncated MS-OFORMS TextProps"));
    }
    let mut cursor = SliceCursor::new(bytes);
    let minor_version = cursor.read_u8()?;
    let major_version = cursor.read_u8()?;
    validate_version(minor_version, major_version, "TextProps")?;
    let property_size = usize::from(cursor.read_u16()?);
    if property_size < 4 {
      return Err(Error::invalid(
        2,
        "cbTextProps is smaller than its PropMask",
      ));
    }
    let end = 4usize
      .checked_add(property_size)
      .ok_or_else(|| Error::Limit("TextProps boundary overflow".into()))?;
    if end > bytes.len() {
      return Err(Error::invalid(2, "cbTextProps exceeds the control stream"));
    }
    cursor.end = end;
    let property_mask = TextPropsPropertyMask::from_bits_retain(cursor.read_u32()?);
    if property_mask.intersects(TextPropsPropertyMask::UNUSED1 | TextPropsPropertyMask::UNUSED2) {
      return Err(Error::invalid(
        4,
        "TextProps property mask has unused bits set",
      ));
    }
    let data_block = TextPropsDataBlock::read(&mut cursor, property_mask)?;
    let extra_data_block = TextPropsExtraDataBlock::read(&mut cursor, property_mask, &data_block)?;
    if cursor.position != end {
      return Err(Error::invalid(
        cursor.position as u64,
        "TextProps fields do not consume cbTextProps",
      ));
    }
    Ok((
      Self {
        minor_version,
        major_version,
        property_mask,
        data_block,
        extra_data_block,
      },
      end,
    ))
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_version(self.minor_version, self.major_version, "TextProps")?;
    if self
      .property_mask
      .intersects(TextPropsPropertyMask::UNUSED1 | TextPropsPropertyMask::UNUSED2)
    {
      return Err(Error::invalid(
        4,
        "TextProps property mask has unused bits set",
      ));
    }
    let mut bytes = vec![self.minor_version, self.major_version, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    self.data_block.write(&mut bytes, self.property_mask)?;
    self
      .extra_data_block
      .write(&mut bytes, self.property_mask, &self.data_block)?;
    let size =
      u16::try_from(bytes.len() - 4).map_err(|_| Error::Limit("cbTextProps exceeds u16".into()))?;
    bytes[2..4].copy_from_slice(&size.to_le_bytes());
    Ok(bytes)
  }
}

impl TextPropsDataBlock {
  fn validate(&self) -> Result<()> {
    if let Some(font_height) = &self.font_height
      && font_height.value > 4_294_967
    {
      return Err(Error::invalid(
        0,
        "TextProps FontHeight must not exceed 4,294,967 twips",
      ));
    }
    if let Some(font_weight) = &self.font_weight
      && font_weight.value > 1_000
    {
      return Err(Error::invalid(
        0,
        "TextProps FontWeight must be in the range 0 through 1000",
      ));
    }
    if let Some(value) = &self.font_pitch_and_family {
      value.value.validate()?;
    }
    Ok(())
  }

  fn read(cursor: &mut SliceCursor<'_>, mask: TextPropsPropertyMask) -> Result<Self> {
    use TextPropsPropertyMask as Mask;

    let mut value = Self::default();
    if mask.contains(TextPropsPropertyMask::FONT_NAME) {
      let padding_before = cursor.read_alignment(4)?;
      value.font_name = Some(AlignedValue {
        padding_before,
        value: CountOfBytesWithCompressionFlag::from_raw(cursor.read_u32()?),
      });
    }
    read_optional!(
      value,
      cursor,
      mask,
      FONT_EFFECTS,
      font_effects,
      4,
      read_font_effects
    );
    read_optional!(value, cursor, mask, FONT_HEIGHT, font_height, 4, read_u32);
    read_optional!(
      value,
      cursor,
      mask,
      FONT_CHAR_SET,
      font_char_set,
      1,
      read_u8
    );
    read_optional!(
      value,
      cursor,
      mask,
      FONT_PITCH_AND_FAMILY,
      font_pitch_and_family,
      1,
      read_font_pitch_and_family
    );
    read_optional!(
      value,
      cursor,
      mask,
      PARAGRAPH_ALIGN,
      paragraph_align,
      1,
      read_paragraph_alignment
    );
    read_optional!(value, cursor, mask, FONT_WEIGHT, font_weight, 2, read_u16);
    value.trailing_padding = cursor.read_alignment(4)?;
    value.validate()?;
    Ok(value)
  }

  fn write(&self, bytes: &mut Vec<u8>, mask: TextPropsPropertyMask) -> Result<()> {
    use TextPropsPropertyMask as Mask;

    self.validate()?;

    match (
      mask.contains(TextPropsPropertyMask::FONT_NAME),
      self.font_name.as_ref(),
    ) {
      (true, Some(value)) => {
        append_padding(bytes, &value.padding_before, 4, "TextProps.FontName")?;
        bytes.extend_from_slice(&value.value.to_raw()?.to_le_bytes());
      }
      (false, None) => {}
      _ => return Err(mask_field_mismatch("TextProps", "FontName")),
    }
    write_optional!(self, bytes, mask, FONT_EFFECTS, font_effects, u32);
    write_optional!(self, bytes, mask, FONT_HEIGHT, font_height, u32);
    write_optional!(self, bytes, mask, FONT_CHAR_SET, font_char_set, u8);
    write_optional!(
      self,
      bytes,
      mask,
      FONT_PITCH_AND_FAMILY,
      font_pitch_and_family,
      u8
    );
    write_optional!(self, bytes, mask, PARAGRAPH_ALIGN, paragraph_align, u8);
    write_optional!(self, bytes, mask, FONT_WEIGHT, font_weight, u16);
    append_padding(bytes, &self.trailing_padding, 4, "TextProps DataBlock")
  }
}

impl TextPropsExtraDataBlock {
  fn read(
    cursor: &mut SliceCursor<'_>,
    _mask: TextPropsPropertyMask,
    data: &TextPropsDataBlock,
  ) -> Result<Self> {
    Ok(Self {
      font_name: read_fm_string(cursor, data.font_name.as_ref())?,
    })
  }

  fn write(
    &self,
    bytes: &mut Vec<u8>,
    _mask: TextPropsPropertyMask,
    data: &TextPropsDataBlock,
  ) -> Result<()> {
    write_fm_string(
      bytes,
      data.font_name.as_ref(),
      self.font_name.as_ref(),
      "FontName",
    )
  }
}

impl MorphDataColumnInfo {
  fn from_prefix(bytes: &[u8]) -> Result<(Self, usize)> {
    if bytes.len() < 8 {
      return Err(Error::invalid(0, "truncated MorphDataColumnInfo"));
    }
    let mut cursor = SliceCursor::new(bytes);
    let minor_version = cursor.read_u8()?;
    let major_version = cursor.read_u8()?;
    validate_version(minor_version, major_version, "MorphDataColumnInfo")?;
    let size = usize::from(cursor.read_u16()?);
    if size < 4 {
      return Err(Error::invalid(
        2,
        "cbColumnInfo is smaller than its PropMask",
      ));
    }
    let end = 4usize
      .checked_add(size)
      .ok_or_else(|| Error::Limit("MorphDataColumnInfo boundary overflow".into()))?;
    if end > bytes.len() {
      return Err(Error::invalid(2, "cbColumnInfo exceeds the control stream"));
    }
    cursor.end = end;
    let property_mask = MorphDataColumnInfoPropertyMask::from_bits_retain(cursor.read_u32()?);
    if property_mask.intersects(MorphDataColumnInfoPropertyMask::UNUSED) {
      return Err(Error::invalid(
        4,
        "MorphDataColumnInfo has unused mask bits set",
      ));
    }
    let column_width = if property_mask.contains(MorphDataColumnInfoPropertyMask::COLUMN_WIDTH) {
      Some(AlignedValue {
        padding_before: cursor.read_alignment(4)?,
        value: cursor.read_i32()?,
      })
    } else {
      None
    };
    if cursor.position != end {
      return Err(Error::invalid(
        cursor.position as u64,
        "MorphDataColumnInfo fields do not consume cbColumnInfo",
      ));
    }
    Ok((
      Self {
        minor_version,
        major_version,
        property_mask,
        column_width,
      },
      end,
    ))
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_version(
      self.minor_version,
      self.major_version,
      "MorphDataColumnInfo",
    )?;
    if self
      .property_mask
      .intersects(MorphDataColumnInfoPropertyMask::UNUSED)
    {
      return Err(Error::invalid(
        4,
        "MorphDataColumnInfo has unused mask bits set",
      ));
    }
    if self
      .property_mask
      .contains(MorphDataColumnInfoPropertyMask::COLUMN_WIDTH)
      != self.column_width.is_some()
    {
      return Err(mask_field_mismatch("MorphDataColumnInfo", "ColumnWidth"));
    }
    let mut bytes = vec![self.minor_version, self.major_version, 0, 0];
    bytes.extend_from_slice(&self.property_mask.bits().to_le_bytes());
    if let Some(width) = &self.column_width {
      append_padding(
        &mut bytes,
        &width.padding_before,
        4,
        "MorphDataColumnInfo.ColumnWidth",
      )?;
      bytes.extend_from_slice(&width.value.to_le_bytes());
    }
    let size = u16::try_from(bytes.len() - 4)
      .map_err(|_| Error::Limit("cbColumnInfo exceeds u16".into()))?;
    bytes[2..4].copy_from_slice(&size.to_le_bytes());
    Ok(bytes)
  }
}

fn read_fm_string(
  cursor: &mut SliceCursor<'_>,
  descriptor: Option<&AlignedValue<CountOfBytesWithCompressionFlag>>,
) -> Result<Option<FmString>> {
  let Some(descriptor) = descriptor else {
    return Ok(None);
  };
  let declared_byte_count = usize::try_from(descriptor.value.byte_count)
    .map_err(|_| Error::Limit("MS-OFORMS string length does not fit usize".into()))?;
  let remaining = cursor.end - cursor.position;
  let (byte_count, length_mode) = if declared_byte_count <= remaining {
    (declared_byte_count, FmStringLengthMode::Declared)
  } else {
    let low_word = usize::from(descriptor.value.byte_count as u16);
    if low_word > remaining {
      return Err(Error::invalid(
        cursor.position as u64,
        "MS-OFORMS string length exceeds its bounded property block",
      ));
    }
    (low_word, FmStringLengthMode::LowWordCompatibility)
  };
  let bytes = cursor.read_vec(byte_count)?;
  let padding_after = cursor.read_alignment(4)?;
  let value = FmString {
    bytes,
    padding_after,
    length_mode,
  };
  value.validate(descriptor.value)?;
  Ok(Some(value))
}

fn read_sized_array_strings(
  cursor: &mut SliceCursor<'_>,
  descriptor: Option<&AlignedValue<u32>>,
  name: &str,
) -> Result<Option<Vec<ArrayString>>> {
  let Some(descriptor) = descriptor else {
    return Ok(None);
  };
  if descriptor.value == 0 {
    return Err(Error::invalid(
      cursor.position as u64,
      format!("TabStrip {name} size must be greater than zero"),
    ));
  }
  let size = usize::try_from(descriptor.value)
    .map_err(|_| Error::Limit(format!("TabStrip {name} size does not fit usize")))?;
  let bytes = cursor.read_vec(size)?;
  let mut inner = SliceCursor::new(&bytes);
  let mut values = Vec::new();
  while inner.position < inner.end {
    let raw = inner.read_u32()?;
    let compressed = raw & 0x8000_0000 != 0;
    let character_count = raw & 0x7fff_ffff;
    let byte_count = if compressed {
      usize::try_from(character_count)
        .map_err(|_| Error::Limit("ArrayString size does not fit usize".into()))?
    } else {
      usize::try_from(character_count)
        .map_err(|_| Error::Limit("ArrayString size does not fit usize".into()))?
        .checked_mul(2)
        .ok_or_else(|| Error::Limit("ArrayString byte size overflow".into()))?
    };
    let string_bytes = inner.read_vec(byte_count)?;
    let padding_after = inner.read_alignment(4)?;
    values.push(ArrayString {
      character_count,
      compressed,
      bytes: string_bytes,
      padding_after,
    });
  }
  Ok(Some(values))
}

fn write_sized_array_strings(
  bytes: &mut Vec<u8>,
  descriptor: Option<&AlignedValue<u32>>,
  values: Option<&[ArrayString]>,
  name: &str,
) -> Result<()> {
  match (descriptor, values) {
    (Some(descriptor), Some(values)) => {
      let mut encoded = Vec::new();
      for value in values {
        if value.character_count & 0x8000_0000 != 0 {
          return Err(Error::Limit(
            "ArrayString character count exceeds 31 bits".into(),
          ));
        }
        let expected_bytes = usize::try_from(value.character_count)
          .map_err(|_| Error::Limit("ArrayString size does not fit usize".into()))?
          .checked_mul(if value.compressed { 1 } else { 2 })
          .ok_or_else(|| Error::Limit("ArrayString byte size overflow".into()))?;
        if value.bytes.len() != expected_bytes {
          return Err(Error::invalid(
            0,
            format!("TabStrip {name} ArrayString length does not match its count"),
          ));
        }
        encoded.extend_from_slice(
          &(value.character_count | if value.compressed { 0x8000_0000 } else { 0 }).to_le_bytes(),
        );
        encoded.extend_from_slice(&value.bytes);
        append_padding(
          &mut encoded,
          &value.padding_after,
          4,
          "TabStrip ArrayString",
        )?;
      }
      let encoded_size = u32::try_from(encoded.len())
        .map_err(|_| Error::Limit(format!("TabStrip {name} exceeds u32")))?;
      if encoded_size == 0 || descriptor.value != encoded_size {
        return Err(Error::invalid(
          0,
          format!("TabStrip {name} size does not match its array"),
        ));
      }
      bytes.extend_from_slice(&encoded);
      Ok(())
    }
    (None, None) => Ok(()),
    _ => Err(mask_field_mismatch("TabStrip ExtraDataBlock", name)),
  }
}

fn read_count_descriptor(
  cursor: &mut SliceCursor<'_>,
  present: bool,
) -> Result<Option<AlignedValue<CountOfBytesWithCompressionFlag>>> {
  if !present {
    return Ok(None);
  }
  Ok(Some(AlignedValue {
    padding_before: cursor.read_alignment(4)?,
    value: CountOfBytesWithCompressionFlag::from_raw(cursor.read_u32()?),
  }))
}

fn write_count_descriptor(
  bytes: &mut Vec<u8>,
  present: bool,
  value: Option<&AlignedValue<CountOfBytesWithCompressionFlag>>,
  name: &str,
) -> Result<()> {
  match (present, value) {
    (true, Some(value)) => {
      append_padding(bytes, &value.padding_before, 4, name)?;
      bytes.extend_from_slice(&value.value.to_raw()?.to_le_bytes());
      Ok(())
    }
    (false, None) => Ok(()),
    _ => Err(mask_field_mismatch("MS-OFORMS property block", name)),
  }
}

fn read_fm_size(cursor: &mut SliceCursor<'_>, present: bool) -> Result<Option<FmSize>> {
  if !present {
    return Ok(None);
  }
  Ok(Some(FmSize {
    width: cursor.read_i32()?,
    height: cursor.read_i32()?,
  }))
}

fn write_fm_size(
  bytes: &mut Vec<u8>,
  present: bool,
  value: Option<FmSize>,
  name: &str,
) -> Result<()> {
  match (present, value) {
    (true, Some(value)) => {
      bytes.extend_from_slice(&value.width.to_le_bytes());
      bytes.extend_from_slice(&value.height.to_le_bytes());
      Ok(())
    }
    (false, None) => Ok(()),
    _ => Err(mask_field_mismatch("MS-OFORMS property block", name)),
  }
}

fn read_fm_position(cursor: &mut SliceCursor<'_>, present: bool) -> Result<Option<FmPosition>> {
  if !present {
    return Ok(None);
  }
  Ok(Some(FmPosition {
    left: cursor.read_i32()?,
    top: cursor.read_i32()?,
  }))
}

fn write_fm_position(
  bytes: &mut Vec<u8>,
  present: bool,
  value: Option<FmPosition>,
  name: &str,
) -> Result<()> {
  match (present, value) {
    (true, Some(value)) => {
      bytes.extend_from_slice(&value.left.to_le_bytes());
      bytes.extend_from_slice(&value.top.to_le_bytes());
      Ok(())
    }
    (false, None) => Ok(()),
    _ => Err(mask_field_mismatch("MS-OFORMS property block", name)),
  }
}

fn write_masked_guid(
  bytes: &mut Vec<u8>,
  present: bool,
  value: Option<Guid>,
  name: &str,
) -> Result<()> {
  match (present, value) {
    (true, Some(value)) => {
      write_guid(bytes, value);
      Ok(())
    }
    (false, None) => Ok(()),
    _ => Err(mask_field_mismatch("MS-OFORMS property block", name)),
  }
}

fn write_fm_string(
  bytes: &mut Vec<u8>,
  descriptor: Option<&AlignedValue<CountOfBytesWithCompressionFlag>>,
  value: Option<&FmString>,
  name: &str,
) -> Result<()> {
  match (descriptor, value) {
    (Some(descriptor), Some(value)) => {
      value.validate(descriptor.value)?;
      bytes.extend_from_slice(&value.bytes);
      append_padding(bytes, &value.padding_after, 4, name)
    }
    (None, None) => Ok(()),
    _ => Err(mask_field_mismatch("MS-OFORMS string", name)),
  }
}

fn property_control_cursor<'a>(
  bytes: &'a [u8],
  name: &str,
) -> Result<(u8, u8, usize, SliceCursor<'a>)> {
  if bytes.len() < 8 {
    return Err(Error::invalid(
      0,
      format!("{name} is smaller than its header"),
    ));
  }
  let mut cursor = SliceCursor::new(bytes);
  let minor_version = cursor.read_u8()?;
  let major_version = cursor.read_u8()?;
  validate_version(minor_version, major_version, name)?;
  let size = usize::from(cursor.read_u16()?);
  if size < 4 {
    return Err(Error::invalid(
      2,
      format!("{name} property size is smaller than its mask"),
    ));
  }
  let property_end = 4usize
    .checked_add(size)
    .ok_or_else(|| Error::Limit(format!("{name} property boundary overflow")))?;
  if property_end > bytes.len() {
    return Err(Error::invalid(
      2,
      format!("{name} property size exceeds the stream"),
    ));
  }
  cursor.end = property_end;
  Ok((minor_version, major_version, property_end, cursor))
}

fn finalize_property_control(bytes: &mut [u8], size_name: &str) -> Result<()> {
  let size = u16::try_from(bytes.len().saturating_sub(4))
    .map_err(|_| Error::Limit(format!("{size_name} exceeds u16")))?;
  bytes[2..4].copy_from_slice(&size.to_le_bytes());
  Ok(())
}

fn validate_morph_mask(mask: MorphDataPropertyMask) -> Result<()> {
  if !mask.contains(MorphDataPropertyMask::SIZE) {
    return Err(Error::invalid(4, "MorphData fSize must be set"));
  }
  if !mask.contains(MorphDataPropertyMask::RESERVED) {
    return Err(Error::invalid(4, "MorphData reserved bit must be set"));
  }
  if mask.intersects(
    MorphDataPropertyMask::UNUSED1
      | MorphDataPropertyMask::UNUSED2
      | MorphDataPropertyMask::UNUSED3,
  ) {
    return Err(Error::invalid(
      4,
      "MorphData property mask has unused bits set",
    ));
  }
  Ok(())
}

fn validate_morph_data(data: &MorphDataDataBlock) -> Result<()> {
  if let Some(value) = &data.text_column
    && value.value < -1
  {
    return Err(Error::invalid(
      0,
      "MorphData TextColumn must be at least -1",
    ));
  }
  if let Some(value) = &data.column_count
    && value.value < -1
  {
    return Err(Error::invalid(
      0,
      "MorphData ColumnCount must be at least -1",
    ));
  }
  Ok(())
}

fn validate_morph_cached_class(value: &MorphDataControl, class: CachedControlClass) -> Result<()> {
  let display_style = value
    .data_block
    .display_style
    .as_ref()
    .map_or(FmDisplayStyle::Text, |value| value.value);
  let effective_class = match (class, display_style) {
    (CachedControlClass::MorphDataLegacy, FmDisplayStyle::Text) => CachedControlClass::TextBox,
    (CachedControlClass::MorphDataLegacy, FmDisplayStyle::List) => CachedControlClass::ListBox,
    (CachedControlClass::MorphDataLegacy, FmDisplayStyle::Combo | FmDisplayStyle::DropList) => {
      CachedControlClass::ComboBox
    }
    (CachedControlClass::MorphDataLegacy, FmDisplayStyle::CheckBox) => CachedControlClass::CheckBox,
    (CachedControlClass::MorphDataLegacy, FmDisplayStyle::OptionButton) => {
      CachedControlClass::OptionButton
    }
    (CachedControlClass::MorphDataLegacy, FmDisplayStyle::Toggle) => {
      CachedControlClass::ToggleButton
    }
    (CachedControlClass::TextBox, FmDisplayStyle::Text)
    | (CachedControlClass::ListBox, FmDisplayStyle::List)
    | (CachedControlClass::ComboBox, FmDisplayStyle::Combo | FmDisplayStyle::DropList)
    | (CachedControlClass::CheckBox, FmDisplayStyle::CheckBox)
    | (CachedControlClass::OptionButton, FmDisplayStyle::OptionButton)
    | (CachedControlClass::ToggleButton, FmDisplayStyle::Toggle) => class,
    _ => {
      return Err(Error::invalid(
        0,
        "MorphData DisplayStyle does not match its cached control class",
      ));
    }
  };
  validate_control_various(
    value.data_block.various_property_bits.as_ref(),
    effective_class,
  )?;

  if effective_class == CachedControlClass::ToggleButton
    && value
      .data_block
      .special_effect
      .as_ref()
      .is_some_and(|effect| effect.value != FmSpecialEffect::Sunken)
  {
    return Err(Error::invalid(
      0,
      "ToggleButton SpecialEffect must be Sunken",
    ));
  }
  if effective_class == CachedControlClass::ListBox
    && value
      .data_block
      .scroll_bars
      .as_ref()
      .map_or(FmScrollBars::None, |scroll_bars| scroll_bars.value)
      != FmScrollBars::Both
  {
    return Err(Error::invalid(0, "ListBox ScrollBars must be Both"));
  }
  if !matches!(
    effective_class,
    CachedControlClass::ComboBox | CachedControlClass::ListBox
  ) && (!value.column_info.is_empty()
    || value
      .data_block
      .column_info_count
      .as_ref()
      .is_some_and(|count| count.value != 0))
  {
    return Err(Error::invalid(
      0,
      "MorphData column information is only valid for ComboBox and ListBox",
    ));
  }
  Ok(())
}

fn validate_control_various(
  stored: Option<&AlignedValue<VariousPropertiesBitfield>>,
  class: CachedControlClass,
) -> Result<()> {
  use VariousPropertiesBitfield as Bits;

  let default = match class {
    CachedControlClass::CommandButton
    | CachedControlClass::Image
    | CachedControlClass::TabStrip
    | CachedControlClass::ScrollBar
    | CachedControlClass::SpinButton => 0x0000_001b,
    CachedControlClass::Label => 0x0080_001b,
    CachedControlClass::TextBox
    | CachedControlClass::ListBox
    | CachedControlClass::ComboBox
    | CachedControlClass::CheckBox
    | CachedControlClass::OptionButton
    | CachedControlClass::ToggleButton => 0x2c80_081b,
    _ => {
      return Err(Error::invalid(
        0,
        "VariousPropertyBits has no cached-class definition",
      ));
    }
  };
  let value = stored.map_or_else(|| Bits::from_bits_retain(default), |stored| stored.value);
  value.validate()?;

  let common =
    Bits::RESERVED1 | Bits::ENABLED | Bits::BACK_STYLE | Bits::RESERVED2 | Bits::IME_MODE;
  let allowed = match class {
    CachedControlClass::Image
    | CachedControlClass::TabStrip
    | CachedControlClass::ScrollBar
    | CachedControlClass::SpinButton => common,
    CachedControlClass::CommandButton => common | Bits::LOCKED | Bits::WORD_WRAP | Bits::AUTO_SIZE,
    CachedControlClass::Label => common | Bits::WORD_WRAP | Bits::AUTO_SIZE,
    CachedControlClass::TextBox => {
      common
        | Bits::LOCKED
        | Bits::INTEGRAL_HEIGHT
        | Bits::EDITABLE
        | Bits::DRAG_BEHAVIOR
        | Bits::ENTER_KEY_BEHAVIOR
        | Bits::ENTER_FIELD_BEHAVIOR
        | Bits::TAB_KEY_BEHAVIOR
        | Bits::WORD_WRAP
        | Bits::BORDERS_SUPPRESS
        | Bits::SELECTION_MARGIN
        | Bits::AUTO_WORD_SELECT
        | Bits::AUTO_SIZE
        | Bits::HIDE_SELECTION
        | Bits::AUTO_TAB
        | Bits::MULTI_LINE
    }
    CachedControlClass::ComboBox => {
      common
        | Bits::LOCKED
        | Bits::COLUMN_HEADS
        | Bits::INTEGRAL_HEIGHT
        | Bits::MATCH_REQUIRED
        | Bits::EDITABLE
        | Bits::DRAG_BEHAVIOR
        | Bits::ENTER_FIELD_BEHAVIOR
        | Bits::WORD_WRAP
        | Bits::BORDERS_SUPPRESS
        | Bits::SELECTION_MARGIN
        | Bits::AUTO_WORD_SELECT
        | Bits::AUTO_SIZE
        | Bits::HIDE_SELECTION
        | Bits::AUTO_TAB
    }
    CachedControlClass::ListBox => {
      common
        | Bits::LOCKED
        | Bits::COLUMN_HEADS
        | Bits::INTEGRAL_HEIGHT
        | Bits::WORD_WRAP
        | Bits::BORDERS_SUPPRESS
        | Bits::SELECTION_MARGIN
        | Bits::AUTO_WORD_SELECT
        | Bits::HIDE_SELECTION
    }
    CachedControlClass::CheckBox
    | CachedControlClass::OptionButton
    | CachedControlClass::ToggleButton => {
      common
        | Bits::LOCKED
        | Bits::INTEGRAL_HEIGHT
        | Bits::WORD_WRAP
        | Bits::BORDERS_SUPPRESS
        | Bits::SELECTION_MARGIN
        | Bits::AUTO_WORD_SELECT
        | Bits::AUTO_SIZE
        | Bits::HIDE_SELECTION
        | if matches!(
          class,
          CachedControlClass::CheckBox | CachedControlClass::OptionButton
        ) {
          Bits::ALIGNMENT
        } else {
          Bits::empty()
        }
    }
    _ => unreachable!("class is filtered above"),
  };
  if value.bits() & !allowed.bits() != 0 {
    return Err(Error::invalid(
      0,
      format!(
        "VariousPropertyBits 0x{:08x} contains fields 0x{:08x} that do not apply to {class:?}",
        value.bits(),
        value.bits() & !allowed.bits()
      ),
    ));
  }

  let required = match class {
    CachedControlClass::TabStrip
    | CachedControlClass::ScrollBar
    | CachedControlClass::SpinButton => Bits::BACK_STYLE,
    CachedControlClass::TextBox => Bits::EDITABLE,
    CachedControlClass::ComboBox => Bits::WORD_WRAP,
    CachedControlClass::ListBox => {
      Bits::BACK_STYLE
        | Bits::WORD_WRAP
        | Bits::SELECTION_MARGIN
        | Bits::AUTO_WORD_SELECT
        | Bits::HIDE_SELECTION
    }
    CachedControlClass::CheckBox
    | CachedControlClass::OptionButton
    | CachedControlClass::ToggleButton => {
      Bits::INTEGRAL_HEIGHT | Bits::SELECTION_MARGIN | Bits::AUTO_WORD_SELECT | Bits::HIDE_SELECTION
    }
    _ => Bits::empty(),
  };
  if !value.contains(required) {
    return Err(Error::invalid(
      0,
      format!("VariousPropertyBits omits fields required for {class:?}"),
    ));
  }
  Ok(())
}

fn validate_page_data(data: &PageDataBlock) -> Result<()> {
  if let Some(value) = &data.transition_period
    && value.value > 10_000
  {
    return Err(Error::invalid(
      0,
      "Page TransitionPeriod must be in the range 0 through 10000",
    ));
  }
  Ok(())
}

fn validate_form_mask(mask: FormPropertyMask) -> Result<()> {
  if !mask.contains(FormPropertyMask::DRAW_BUFFER) {
    return Err(Error::invalid(4, "Form fDrawBuffer must be set"));
  }
  if mask.intersects(
    FormPropertyMask::UNUSED1
      | FormPropertyMask::UNUSED2
      | FormPropertyMask::RESERVED
      | FormPropertyMask::UNUSED3,
  ) {
    return Err(Error::invalid(4, "Form property mask has unused bits set"));
  }
  Ok(())
}

fn validate_site_mask(mask: SitePropertyMask) -> Result<()> {
  if mask.intersects(SitePropertyMask::UNUSED1 | SitePropertyMask::UNUSED2) {
    return Err(Error::invalid(4, "Site property mask has unused bits set"));
  }
  Ok(())
}

fn validate_site_class_flags(data: &SiteDataBlock) -> Result<()> {
  let flags = data.bit_flags.as_ref().map_or_else(
    || SiteFlags::from_bits_retain(0x0000_0033),
    |value| value.value,
  );
  flags.validate()?;
  let class = data
    .clsid_cache_index
    .as_ref()
    .map_or(SiteClassIndex::Invalid, |value| value.value);
  let promotes_children = matches!(
    class,
    SiteClassIndex::Cached(
      CachedControlClass::Form | CachedControlClass::Frame | CachedControlClass::MultiPage
    )
  );
  if flags.contains(SiteFlags::PROMOTE_CONTROLS) != promotes_children {
    return Err(Error::invalid(
      0,
      "SITE_FLAG fPromoteControls does not match the embedded control class",
    ));
  }
  if flags.contains(SiteFlags::PRESERVE_HEIGHT)
    && class != SiteClassIndex::Cached(CachedControlClass::ListBox)
  {
    return Err(Error::invalid(
      0,
      "SITE_FLAG fPreserveHeight only applies to ListBox",
    ));
  }
  Ok(())
}

fn validate_class_info_mask(mask: ClassInfoPropertyMask) -> Result<()> {
  if mask.intersects(ClassInfoPropertyMask::UNUSED1 | ClassInfoPropertyMask::UNUSED2) {
    return Err(Error::invalid(
      4,
      "SiteClassInfo property mask has unused bits set",
    ));
  }
  Ok(())
}

fn validate_tab_strip_mask(mask: TabStripPropertyMask) -> Result<()> {
  if !mask.contains(TabStripPropertyMask::SIZE) {
    return Err(Error::invalid(4, "TabStrip fSize must be set"));
  }
  if !mask.contains(TabStripPropertyMask::NEW_VERSION) {
    return Err(Error::invalid(4, "TabStrip fNewVersion must be set"));
  }
  if mask.intersects(
    TabStripPropertyMask::UNUSED1
      | TabStripPropertyMask::UNUSED2
      | TabStripPropertyMask::UNUSED3
      | TabStripPropertyMask::UNUSED4
      | TabStripPropertyMask::UNUSED,
  ) {
    return Err(Error::invalid(
      4,
      "TabStrip property mask has unused bits set",
    ));
  }
  Ok(())
}

fn validate_tab_strip_data(data: &TabStripDataBlock, extra: &TabStripExtraDataBlock) -> Result<()> {
  for (name, value) in [
    ("TabFixedWidth", data.tab_fixed_width.as_ref()),
    ("TabFixedHeight", data.tab_fixed_height.as_ref()),
  ] {
    if let Some(value) = value
      && value.value > 254_000
    {
      return Err(Error::invalid(
        0,
        format!("TabStrip {name} must not exceed 254000"),
      ));
    }
  }

  let item_count = extra.items.as_ref().map_or(0usize, Vec::len);
  let tab_data_count = data
    .tab_data_count
    .as_ref()
    .map_or(0u32, |value| value.value);
  if usize::try_from(tab_data_count).map_or(true, |count| count > item_count) {
    return Err(Error::invalid(
      0,
      "TabStrip TabData count exceeds its number of tabs",
    ));
  }
  let list_index = data.list_index.as_ref().map_or(-1, |value| value.value);
  if list_index < -1 || usize::try_from(list_index).is_ok_and(|index| index >= item_count) {
    return Err(Error::invalid(
      0,
      "TabStrip ListIndex is outside its tab item range",
    ));
  }
  Ok(())
}

fn validate_image_mask(mask: ImagePropertyMask) -> Result<()> {
  if !mask.contains(ImagePropertyMask::SIZE) {
    return Err(Error::invalid(4, "Image fSize must be set"));
  }
  if mask.intersects(ImagePropertyMask::UNUSED1 | ImagePropertyMask::UNUSED2) {
    return Err(Error::invalid(4, "Image property mask has unused bits set"));
  }
  Ok(())
}

fn validate_label_mask(mask: LabelPropertyMask) -> Result<()> {
  if !mask.contains(LabelPropertyMask::SIZE) {
    return Err(Error::invalid(4, "Label fSize must be set"));
  }
  if mask.intersects(LabelPropertyMask::UNUSED) {
    return Err(Error::invalid(4, "Label property mask has unused bits set"));
  }
  Ok(())
}

fn validate_spin_button_mask(mask: SpinButtonPropertyMask) -> Result<()> {
  if !mask.contains(SpinButtonPropertyMask::SIZE) {
    return Err(Error::invalid(4, "SpinButton fSize must be set"));
  }
  if mask.intersects(SpinButtonPropertyMask::UNUSED1 | SpinButtonPropertyMask::UNUSED2) {
    return Err(Error::invalid(
      4,
      "SpinButton property mask has unused bits set",
    ));
  }
  validate_prev_next_mask(
    mask.contains(SpinButtonPropertyMask::VARIOUS_PROPERTY_BITS),
    mask.contains(SpinButtonPropertyMask::PREV_ENABLED),
    mask.contains(SpinButtonPropertyMask::NEXT_ENABLED),
    "SpinButton",
  )
}

fn validate_spin_button_enabled_mask(
  mask: SpinButtonPropertyMask,
  data: &SpinButtonDataBlock,
) -> Result<()> {
  validate_enabled_mask_value(
    mask.contains(SpinButtonPropertyMask::VARIOUS_PROPERTY_BITS),
    mask.contains(SpinButtonPropertyMask::PREV_ENABLED),
    data.various_property_bits.as_ref(),
    data.prev_enabled.as_ref(),
    data.next_enabled.as_ref(),
    "SpinButton",
  )?;
  validate_position_range(
    data.min.as_ref(),
    data.max.as_ref(),
    data.position.as_ref(),
    100,
    "SpinButton",
  )
}

fn validate_scroll_bar_mask(mask: ScrollBarPropertyMask) -> Result<()> {
  if !mask.contains(ScrollBarPropertyMask::SIZE) {
    return Err(Error::invalid(4, "ScrollBar fSize must be set"));
  }
  if mask.intersects(ScrollBarPropertyMask::UNUSED1 | ScrollBarPropertyMask::UNUSED2) {
    return Err(Error::invalid(
      4,
      "ScrollBar property mask has unused bits set",
    ));
  }
  validate_prev_next_mask(
    mask.contains(ScrollBarPropertyMask::VARIOUS_PROPERTY_BITS),
    mask.contains(ScrollBarPropertyMask::PREV_ENABLED),
    mask.contains(ScrollBarPropertyMask::NEXT_ENABLED),
    "ScrollBar",
  )
}

fn validate_scroll_bar_enabled_mask(
  mask: ScrollBarPropertyMask,
  data: &ScrollBarDataBlock,
) -> Result<()> {
  validate_enabled_mask_value(
    mask.contains(ScrollBarPropertyMask::VARIOUS_PROPERTY_BITS),
    mask.contains(ScrollBarPropertyMask::PREV_ENABLED),
    data.various_property_bits.as_ref(),
    data.prev_enabled.as_ref(),
    data.next_enabled.as_ref(),
    "ScrollBar",
  )?;
  validate_position_range(
    data.min.as_ref(),
    data.max.as_ref(),
    data.position.as_ref(),
    32_767,
    "ScrollBar",
  )
}

fn validate_position_range(
  min: Option<&AlignedValue<i32>>,
  max: Option<&AlignedValue<i32>>,
  position: Option<&AlignedValue<i32>>,
  default_max: i32,
  name: &str,
) -> Result<()> {
  let min = min.map_or(0, |value| value.value);
  let max = max.map_or(default_max, |value| value.value);
  let position = position.map_or(0, |value| value.value);
  if position < min.min(max) || position > min.max(max) {
    return Err(Error::invalid(
      0,
      format!("{name} Position is outside the inclusive Min/Max range"),
    ));
  }
  Ok(())
}

fn validate_prev_next_mask(
  has_various_properties: bool,
  has_prev_enabled: bool,
  has_next_enabled: bool,
  name: &str,
) -> Result<()> {
  if has_prev_enabled != has_next_enabled {
    return Err(Error::invalid(
      4,
      format!("{name} fNextEnabled must equal fPrevEnabled"),
    ));
  }
  if !has_various_properties && has_prev_enabled {
    return Err(Error::invalid(
      4,
      format!("{name} fPrevEnabled requires fVariousPropertyBits"),
    ));
  }
  Ok(())
}

fn validate_enabled_mask_value(
  has_various_properties: bool,
  has_prev_enabled: bool,
  various_property_bits: Option<&AlignedValue<VariousPropertiesBitfield>>,
  prev_enabled: Option<&AlignedValue<EnabledState>>,
  next_enabled: Option<&AlignedValue<EnabledState>>,
  name: &str,
) -> Result<()> {
  if has_prev_enabled != prev_enabled.is_some() || has_prev_enabled != next_enabled.is_some() {
    return Err(mask_field_mismatch(name, "PrevEnabled/NextEnabled"));
  }
  if has_various_properties {
    let enabled = various_property_bits
      .ok_or_else(|| mask_field_mismatch(name, "VariousPropertyBits"))?
      .value
      .contains(VariousPropertiesBitfield::ENABLED);
    if has_prev_enabled == enabled {
      return Err(Error::invalid(
        4,
        format!("{name} fPrevEnabled must be the inverse of Enabled"),
      ));
    }
    if let (Some(prev), Some(next)) = (prev_enabled, next_enabled)
      && prev.value != next.value
    {
      return Err(Error::invalid(
        4,
        format!("{name} PrevEnabled/NextEnabled must be equal Boolean values"),
      ));
    }
    if !enabled && prev_enabled.is_some_and(|value| value.value.is_enabled()) {
      return Err(Error::invalid(
        4,
        format!("{name} PrevEnabled must be zero when Enabled is zero"),
      ));
    }
  }
  Ok(())
}

fn validate_command_button_mask(mask: CommandButtonPropertyMask) -> Result<()> {
  if !mask.contains(CommandButtonPropertyMask::SIZE) {
    return Err(Error::invalid(4, "CommandButton fSize must be set"));
  }
  if mask.intersects(CommandButtonPropertyMask::UNUSED) {
    return Err(Error::invalid(
      4,
      "CommandButton property mask has unused bits set",
    ));
  }
  Ok(())
}

fn validate_version(minor: u8, major: u8, name: &str) -> Result<()> {
  if minor != 0 || major != 2 {
    return Err(Error::invalid(0, format!("{name} version must be 0.2")));
  }
  Ok(())
}

fn write_picture(
  bytes: &mut Vec<u8>,
  present: bool,
  value: Option<&GuidAndPicture>,
  name: &str,
) -> Result<()> {
  match (present, value) {
    (true, Some(value)) => {
      if value.class_id != GuidAndPicture::STD_PICTURE_CLASS_ID {
        return Err(Error::invalid(
          0,
          format!("{name} has an invalid StdPicture CLSID"),
        ));
      }
      if value.preamble != GuidAndPicture::PREAMBLE {
        return Err(Error::invalid(
          0,
          format!("{name} has an invalid StdPicture preamble"),
        ));
      }
      write_guid(bytes, value.class_id);
      bytes.extend_from_slice(&value.preamble.to_le_bytes());
      let size = u32::try_from(value.picture.len())
        .map_err(|_| Error::Limit(format!("{name} picture exceeds u32")))?;
      bytes.extend_from_slice(&size.to_le_bytes());
      bytes.extend_from_slice(&value.picture);
      Ok(())
    }
    (false, None) => Ok(()),
    _ => Err(mask_field_mismatch("MorphData StreamData", name)),
  }
}

fn append_padding(bytes: &mut Vec<u8>, padding: &[u8], alignment: usize, name: &str) -> Result<()> {
  let expected = bytes.len().next_multiple_of(alignment) - bytes.len();
  if padding.len() != expected {
    return Err(Error::invalid(
      bytes.len() as u64,
      format!("{name} alignment padding has the wrong length"),
    ));
  }
  bytes.extend_from_slice(padding);
  Ok(())
}

fn mask_field_mismatch(block: &str, field: &str) -> Error {
  Error::invalid(
    0,
    format!("{block} mask presence does not match field {field}"),
  )
}

fn write_guid(bytes: &mut Vec<u8>, value: Guid) {
  bytes.extend_from_slice(&value.data1.to_le_bytes());
  bytes.extend_from_slice(&value.data2.to_le_bytes());
  bytes.extend_from_slice(&value.data3.to_le_bytes());
  bytes.extend_from_slice(&value.data4);
}

struct SliceCursor<'a> {
  bytes: &'a [u8],
  position: usize,
  end: usize,
}

impl<'a> SliceCursor<'a> {
  fn new(bytes: &'a [u8]) -> Self {
    Self {
      bytes,
      position: 0,
      end: bytes.len(),
    }
  }

  fn read_vec(&mut self, len: usize) -> Result<Vec<u8>> {
    let end = self
      .position
      .checked_add(len)
      .ok_or_else(|| Error::Limit("MS-OFORMS cursor overflow".into()))?;
    if end > self.end {
      return Err(Error::invalid(
        self.position as u64,
        "truncated MS-OFORMS structure",
      ));
    }
    let value = self.bytes[self.position..end].to_vec();
    self.position = end;
    Ok(value)
  }

  fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
    let bytes = self.read_vec(N)?;
    Ok(bytes.try_into().expect("fixed-size MS-OFORMS field"))
  }

  fn read_alignment(&mut self, alignment: usize) -> Result<Vec<u8>> {
    let len = self.position.next_multiple_of(alignment) - self.position;
    self.read_vec(len)
  }

  fn read_u8(&mut self) -> Result<u8> {
    Ok(self.read_array::<1>()?[0])
  }

  fn read_mouse_pointer(&mut self) -> Result<FmMousePointer> {
    Ok(FmMousePointer::from_raw(self.read_u8()?))
  }

  fn read_border_style_u8(&mut self) -> Result<FmBorderStyle> {
    Ok(FmBorderStyle::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_border_style_u16(&mut self) -> Result<FmBorderStyle> {
    Ok(FmBorderStyle::from_raw(u32::from(self.read_u16()?)))
  }

  fn read_picture_alignment(&mut self) -> Result<FmPictureAlignment> {
    Ok(FmPictureAlignment::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_picture_size_mode(&mut self) -> Result<FmPictureSizeMode> {
    Ok(FmPictureSizeMode::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_picture_position(&mut self) -> Result<FmPicturePosition> {
    Ok(FmPicturePosition::from_raw(self.read_u32()?))
  }

  fn read_special_effect_u8(&mut self) -> Result<FmSpecialEffect> {
    Ok(FmSpecialEffect::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_special_effect_u16(&mut self) -> Result<FmSpecialEffect> {
    Ok(FmSpecialEffect::from_raw(u32::from(self.read_u16()?)))
  }

  fn read_special_effect_u32(&mut self) -> Result<FmSpecialEffect> {
    Ok(FmSpecialEffect::from_raw(self.read_u32()?))
  }

  fn read_orientation(&mut self) -> Result<FmOrientation> {
    Ok(FmOrientation::from_raw(self.read_u32()?))
  }

  fn read_scroll_bars(&mut self) -> Result<FmScrollBars> {
    Ok(FmScrollBars::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_form_scroll_bars(&mut self) -> Result<FormScrollBarFlags> {
    let value = FormScrollBarFlags::from_bits_retain(self.read_u8()?);
    value.validate()?;
    Ok(value)
  }

  fn read_display_style(&mut self) -> Result<FmDisplayStyle> {
    Ok(FmDisplayStyle::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_list_style(&mut self) -> Result<FmListStyle> {
    Ok(FmListStyle::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_match_entry(&mut self) -> Result<FmMatchEntry> {
    Ok(FmMatchEntry::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_show_drop_button_when(&mut self) -> Result<FmShowDropButtonWhen> {
    Ok(FmShowDropButtonWhen::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_drop_button_style(&mut self) -> Result<FmDropButtonStyle> {
    Ok(FmDropButtonStyle::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_multi_select(&mut self) -> Result<FmMultiSelect> {
    Ok(FmMultiSelect::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_cycle(&mut self) -> Result<FmCycle> {
    Ok(FmCycle::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_tab_orientation(&mut self) -> Result<FmTabOrientation> {
    Ok(FmTabOrientation::from_raw(self.read_u32()?))
  }

  fn read_tab_style(&mut self) -> Result<FmTabStyle> {
    Ok(FmTabStyle::from_raw(self.read_u32()?))
  }

  fn read_click_control_mode(&mut self) -> Result<FmClickControlMode> {
    Ok(FmClickControlMode::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_double_click_control_mode(&mut self) -> Result<FmDoubleClickControlMode> {
    Ok(FmDoubleClickControlMode::from_raw(u32::from(
      self.read_u8()?,
    )))
  }

  fn read_paragraph_alignment(&mut self) -> Result<FmParagraphAlignment> {
    Ok(FmParagraphAlignment::from_raw(u32::from(self.read_u8()?)))
  }

  fn read_transition_effect(&mut self) -> Result<FmTransitionEffect> {
    Ok(FmTransitionEffect::from_raw(self.read_u32()?))
  }

  fn read_font_effects(&mut self) -> Result<FmFontEffects> {
    let value = FmFontEffects::from_bits_retain(self.read_u32()?);
    value.validate()?;
    Ok(value)
  }

  fn read_font_pitch_and_family(&mut self) -> Result<FmFontPitchAndFamily> {
    let value = FmFontPitchAndFamily::from_raw(self.read_u8()?);
    value.validate()?;
    Ok(value)
  }

  fn read_u16(&mut self) -> Result<u16> {
    Ok(u16::from_le_bytes(self.read_array()?))
  }

  fn read_i16(&mut self) -> Result<i16> {
    Ok(i16::from_le_bytes(self.read_array()?))
  }

  fn read_u32(&mut self) -> Result<u32> {
    Ok(u32::from_le_bytes(self.read_array()?))
  }

  fn read_ole_color(&mut self) -> Result<OleColor> {
    OleColor::from_raw(self.read_u32()?)
  }

  fn read_various_properties(&mut self) -> Result<VariousPropertiesBitfield> {
    let value = VariousPropertiesBitfield::from_bits_retain(self.read_u32()?);
    value.validate()?;
    Ok(value)
  }

  fn read_form_flags(&mut self) -> Result<FormFlags> {
    let value = FormFlags::from_bits_retain(self.read_u32()?);
    value.validate()?;
    Ok(value)
  }

  fn read_site_flags(&mut self) -> Result<SiteFlags> {
    let value = SiteFlags::from_bits_retain(self.read_u32()?);
    value.validate()?;
    Ok(value)
  }

  fn read_design_extender_flags(&mut self) -> Result<DesignExtenderFlags> {
    let value = DesignExtenderFlags::from_bits_retain(self.read_u32()?);
    value.validate()?;
    Ok(value)
  }

  fn read_class_table_flags(&mut self) -> Result<ClassTableFlags> {
    let value = ClassTableFlags::from_bits_retain(self.read_u16()?);
    value.validate()?;
    Ok(value)
  }

  fn read_variable_flags(&mut self) -> Result<VariableFlags> {
    let value = VariableFlags::from_bits_retain(self.read_u16()?);
    value.validate()?;
    Ok(value)
  }

  fn read_variant_type(&mut self) -> Result<VariantType> {
    let value = VariantType::from_raw(self.read_u16()?);
    value.validate()?;
    Ok(value)
  }

  fn read_proportional_thumb(&mut self) -> Result<ProportionalThumb> {
    ProportionalThumb::from_raw(self.read_i16()?)
  }

  fn read_enabled_state(&mut self) -> Result<EnabledState> {
    EnabledState::from_raw(self.read_i32()?)
  }

  fn read_site_class_index(&mut self) -> Result<SiteClassIndex> {
    Ok(SiteClassIndex::from_raw(self.read_u16()?))
  }

  fn read_persistence_marker(&mut self) -> Result<PersistenceMarker> {
    PersistenceMarker::from_raw(self.read_u16()?)
  }

  fn read_i32(&mut self) -> Result<i32> {
    Ok(i32::from_le_bytes(self.read_array()?))
  }

  fn read_u64(&mut self) -> Result<u64> {
    Ok(u64::from_le_bytes(self.read_array()?))
  }

  fn read_guid(&mut self) -> Result<Guid> {
    Ok(Guid::from_fields(
      self.read_u32()?,
      self.read_u16()?,
      self.read_u16()?,
      self.read_array()?,
    ))
  }

  fn read_picture(&mut self) -> Result<GuidAndPicture> {
    let class_id = self.read_guid()?;
    if class_id != GuidAndPicture::STD_PICTURE_CLASS_ID {
      return Err(Error::invalid(
        self.position.saturating_sub(16) as u64,
        "GuidAndPicture has an invalid StdPicture CLSID",
      ));
    }
    let preamble = self.read_u32()?;
    if preamble != GuidAndPicture::PREAMBLE {
      return Err(Error::invalid(
        self.position.saturating_sub(4) as u64,
        "StdPicture has an invalid preamble",
      ));
    }
    let size = usize::try_from(self.read_u32()?)
      .map_err(|_| Error::Limit("StdPicture size does not fit usize".into()))?;
    Ok(GuidAndPicture {
      class_id,
      preamble,
      picture: self.read_vec(size)?,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn text_props() -> TextProps {
    TextProps {
      minor_version: 0,
      major_version: 2,
      property_mask: TextPropsPropertyMask::FONT_NAME
        | TextPropsPropertyMask::FONT_EFFECTS
        | TextPropsPropertyMask::FONT_HEIGHT
        | TextPropsPropertyMask::FONT_CHAR_SET
        | TextPropsPropertyMask::PARAGRAPH_ALIGN,
      data_block: TextPropsDataBlock {
        font_name: Some(AlignedValue {
          padding_before: vec![],
          value: CountOfBytesWithCompressionFlag {
            byte_count: 5,
            compressed: true,
          },
        }),
        font_effects: Some(AlignedValue {
          padding_before: vec![],
          value: FmFontEffects::BOLD,
        }),
        font_height: Some(AlignedValue {
          padding_before: vec![],
          value: 160,
        }),
        font_char_set: Some(AlignedValue {
          padding_before: vec![],
          value: 1,
        }),
        paragraph_align: Some(AlignedValue {
          padding_before: vec![],
          value: FmParagraphAlignment::Compatibility(0),
        }),
        trailing_padding: vec![0, 0],
        ..TextPropsDataBlock::default()
      },
      extra_data_block: TextPropsExtraDataBlock {
        font_name: Some(FmString {
          bytes: b"Arial".to_vec(),
          padding_after: vec![0, 0, 0],
          length_mode: FmStringLengthMode::Declared,
        }),
      },
    }
  }

  fn aligned<T>(value: T, padding_before: &[u8]) -> AlignedValue<T> {
    AlignedValue {
      padding_before: padding_before.to_vec(),
      value,
    }
  }

  fn color(raw: u32) -> OleColor {
    OleColor::from_raw(raw).unwrap()
  }

  fn various(raw: u32) -> VariousPropertiesBitfield {
    let value = VariousPropertiesBitfield::from_bits_retain(raw);
    value.validate().unwrap();
    value
  }

  fn picture(payload: u8) -> GuidAndPicture {
    GuidAndPicture {
      class_id: GuidAndPicture::STD_PICTURE_CLASS_ID,
      preamble: GuidAndPicture::PREAMBLE,
      picture: vec![payload],
    }
  }

  #[test]
  fn common_property_types_are_static_and_byte_exact() {
    let system = OleColor::from_raw(0x8000_0006).unwrap();
    assert_eq!(system.color_type, OleColorType::SystemPalette);
    assert_eq!(system.entry.red_and_green_or_palette_index, 6);
    assert_eq!(system.entry.blue, 0);
    assert_eq!(system.palette_index(), Some(6));
    assert_eq!(system.rgb_components(), None);
    assert_eq!(system.raw(), 0x8000_0006);

    let rgb = OleColor::from_raw(0x0211_2233).unwrap();
    assert_eq!(rgb.color_type, OleColorType::RgbColor);
    assert_eq!(rgb.entry.red_and_green_or_palette_index, 0x2233);
    assert_eq!(rgb.entry.blue, 0x11);
    assert_eq!(rgb.rgb_components(), Some((0x33, 0x22, 0x11)));
    assert_eq!(rgb.palette_index(), None);
    assert_eq!(rgb.raw(), 0x0211_2233);
    assert!(OleColor::from_raw(0x0101_0006).is_err());

    let compressed = FmString {
      bytes: vec![b'A', 0xe9],
      padding_after: vec![0, 0],
      length_mode: FmStringLengthMode::Declared,
    };
    assert_eq!(
      compressed
        .decode(CountOfBytesWithCompressionFlag {
          byte_count: 2,
          compressed: true,
        })
        .unwrap(),
      "Aé"
    );

    let uncompressed = FmString {
      bytes: "A水"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>(),
      padding_after: vec![],
      length_mode: FmStringLengthMode::Declared,
    };
    assert_eq!(
      uncompressed
        .decode(CountOfBytesWithCompressionFlag {
          byte_count: 4,
          compressed: false,
        })
        .unwrap(),
      "A水"
    );

    let flags = various(0x2c80_081b);
    assert!(flags.contains(VariousPropertiesBitfield::ENABLED));
    assert!(flags.contains(VariousPropertiesBitfield::INTEGRAL_HEIGHT));
    assert_eq!(flags.ime_mode(), FmImeMode::NoControl);
    let flags = flags.with_ime_mode(FmImeMode::Hanzi);
    assert_eq!(flags.ime_mode(), FmImeMode::Hanzi);
    assert_eq!(
      flags.bits() & VariousPropertiesBitfield::IME_MODE.bits(),
      0x0006_0000
    );

    assert!(
      VariousPropertiesBitfield::from_bits_retain(0x2c80_081a)
        .validate()
        .is_err()
    );
    assert!(
      VariousPropertiesBitfield::from_bits_retain(0x2d80_081b)
        .validate()
        .is_err()
    );
    assert!(FormFlags::from_bits_retain(0x0000_0001).validate().is_err());

    assert_eq!(FmMousePointer::from_raw(0x63), FmMousePointer::Custom);
    assert_eq!(FmMousePointer::Compatibility(0x04).raw(), 0x04);
    assert_eq!(FmBorderStyle::from_raw(1), FmBorderStyle::Single);
    assert_eq!(FmOrientation::from_raw(u32::MAX), FmOrientation::Auto);
    assert_eq!(FmPicturePosition::AboveCenter.raw(), 0x0007_0001);

    let mut encoded = Vec::new();
    <FmBorderStyle as FormScalar<u16>>::write_le(&FmBorderStyle::Single, &mut encoded).unwrap();
    <FmSpecialEffect as FormScalar<u32>>::write_le(&FmSpecialEffect::Bump, &mut encoded).unwrap();
    assert_eq!(encoded, [1, 0, 6, 0, 0, 0]);
    assert!(
      <FmBorderStyle as FormScalar<u8>>::write_le(
        &FmBorderStyle::Compatibility(0x100),
        &mut Vec::new(),
      )
      .is_err()
    );

    let scroll_bars = FormScrollBarFlags::KEEP_HORIZONTAL
      | FormScrollBarFlags::KEEP_VERTICAL
      | FormScrollBarFlags::VERTICAL;
    assert_eq!(scroll_bars.bits(), 0x0e);
    assert!(scroll_bars.validate().is_ok());
    assert!(
      FormScrollBarFlags::from_bits_retain(0x80)
        .validate()
        .is_err()
    );

    let site_flags =
      SiteFlags::TAB_STOP | SiteFlags::VISIBLE | SiteFlags::STREAMED | SiteFlags::AUTO_SIZE;
    assert_eq!(site_flags.bits(), 0x33);
    assert!(site_flags.validate().is_ok());
    assert!(SiteFlags::from_bits_retain(1 << 10).validate().is_err());

    let design_flags = DesignExtenderFlags::from_bits_retain(0x0001_5f55);
    assert!(design_flags.validate().is_ok());
    assert!(design_flags.contains(DesignExtenderFlags::INHERIT_GRID_X));
    assert!(
      DesignExtenderFlags::from_bits_retain(1 << 18)
        .validate()
        .is_err()
    );

    let class_flags = ClassTableFlags::DUAL_INTERFACE | ClassTableFlags::NO_AGGREGATION;
    assert_eq!(class_flags.bits(), 0x0006);
    assert!(
      ClassTableFlags::from_bits_retain(0x0008)
        .validate()
        .is_err()
    );
    let variable_flags = VariableFlags::BINDABLE | VariableFlags::DISPLAY_BIND;
    assert_eq!(variable_flags.bits(), 0x0014);
    assert!(VariableFlags::from_bits_retain(0x8000).validate().is_err());

    let variant_type = VariantType::from_raw(0x6008);
    assert_eq!(variant_type.base, VariantBaseType::Bstr);
    assert!(variant_type.array);
    assert!(variant_type.by_reference);
    assert_eq!(variant_type.raw(), 0x6008);
    assert!(VariantType::from_raw(0x4000).validate().is_err());

    let mut encoded = Vec::new();
    <ProportionalThumb as FormScalar<i16>>::write_le(
      &ProportionalThumb::Proportional,
      &mut encoded,
    )
    .unwrap();
    assert_eq!(encoded, [0xff, 0xff]);
    assert!(ProportionalThumb::from_raw(1).is_err());

    let font_flags = StdFontFlags::ITALIC | StdFontFlags::UNDERLINE;
    assert_eq!(font_flags.bits(), 0x06);
    assert!(font_flags.validate().is_ok());
    assert!(StdFontFlags::BOLD_RESERVED.validate().is_err());
    let tab_flags = TabStripTabFlags::VISIBLE | TabStripTabFlags::ENABLED;
    assert_eq!(tab_flags.bits(), 3);
    assert!(TabStripTabFlags::from_bits_retain(4).validate().is_err());
    assert_eq!(EnabledState::from_raw(1).unwrap(), EnabledState::Enabled);
    assert!(EnabledState::from_raw(-1).is_err());
    assert!(
      MorphDataColumnInfoPropertyMask::from_bits_retain(2)
        .intersects(MorphDataColumnInfoPropertyMask::UNUSED)
    );

    assert_eq!(
      SiteClassIndex::from_raw(17),
      SiteClassIndex::Cached(CachedControlClass::CommandButton)
    );
    assert_eq!(SiteClassIndex::from_raw(0x7fff), SiteClassIndex::Invalid);
    assert_eq!(
      SiteClassIndex::from_raw(0x8005),
      SiteClassIndex::ClassTable(5)
    );
    assert_eq!(SiteClassIndex::ClassTable(5).to_raw().unwrap(), 0x8005);
    assert!(SiteClassIndex::ClassTable(0x8000).to_raw().is_err());
    assert_eq!(
      PersistenceMarker::from_raw(0xffff).unwrap(),
      PersistenceMarker
    );
    assert!(PersistenceMarker::from_raw(0).is_err());
  }

  fn morph_data() -> MorphDataControl {
    let mask = MorphDataPropertyMask::VARIOUS_PROPERTY_BITS
      | MorphDataPropertyMask::BACK_COLOR
      | MorphDataPropertyMask::FORE_COLOR
      | MorphDataPropertyMask::BORDER_STYLE
      | MorphDataPropertyMask::DISPLAY_STYLE
      | MorphDataPropertyMask::SIZE
      | MorphDataPropertyMask::BORDER_COLOR
      | MorphDataPropertyMask::SPECIAL_EFFECT
      | MorphDataPropertyMask::RESERVED;
    MorphDataControl {
      minor_version: 0,
      major_version: 2,
      property_mask: mask,
      data_block: MorphDataDataBlock {
        various_property_bits: Some(AlignedValue {
          padding_before: vec![],
          value: various(0x2c80_481b),
        }),
        back_color: Some(AlignedValue {
          padding_before: vec![],
          value: color(0x8000_0005),
        }),
        fore_color: Some(AlignedValue {
          padding_before: vec![],
          value: color(0x8000_0008),
        }),
        border_style: Some(AlignedValue {
          padding_before: vec![],
          value: FmBorderStyle::Single,
        }),
        display_style: Some(AlignedValue {
          padding_before: vec![],
          value: FmDisplayStyle::Text,
        }),
        border_color: Some(AlignedValue {
          padding_before: vec![0, 0],
          value: color(0x8000_0006),
        }),
        special_effect: Some(AlignedValue {
          padding_before: vec![],
          value: FmSpecialEffect::Sunken,
        }),
        ..MorphDataDataBlock::default()
      },
      extra_data_block: MorphDataExtraDataBlock {
        size: Some(FmSize {
          width: 1_000,
          height: 500,
        }),
        ..MorphDataExtraDataBlock::default()
      },
      stream_data: MorphDataStreamData::default(),
      text_props: text_props(),
      column_info: vec![],
    }
  }

  #[test]
  fn command_button_round_trips_static_property_blocks() {
    let value = CommandButtonControl {
      minor_version: 0,
      major_version: 2,
      property_mask: CommandButtonPropertyMask::CAPTION | CommandButtonPropertyMask::SIZE,
      take_focus_on_click: true,
      data_block: CommandButtonDataBlock {
        caption: Some(AlignedValue {
          padding_before: vec![],
          value: CountOfBytesWithCompressionFlag {
            byte_count: 4,
            compressed: true,
          },
        }),
        ..CommandButtonDataBlock::default()
      },
      extra_data_block: CommandButtonExtraDataBlock {
        caption: Some(FmString {
          bytes: b"Test".to_vec(),
          padding_after: vec![],
          length_mode: FmStringLengthMode::Declared,
        }),
        size: Some(FmSize {
          width: 1_000,
          height: 500,
        }),
      },
      stream_data: CommandButtonStreamData::default(),
      text_props: text_props(),
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(CommandButtonControl::from_bytes(&bytes).unwrap(), value);
    assert_eq!(
      CommandButtonControl::from_bytes(&bytes)
        .unwrap()
        .to_bytes()
        .unwrap(),
      bytes
    );
  }

  #[test]
  fn remaining_cached_controls_round_trip_minimal_static_shapes() {
    let image = [
      0x00, 0x02, 0x0c, 0x00, 0x00, 0x02, 0x00, 0x00, 0xe8, 0x03, 0x00, 0x00, 0xf4, 0x01, 0x00,
      0x00,
    ];
    assert_eq!(
      ImageControl::from_bytes(&image)
        .unwrap()
        .to_bytes()
        .unwrap(),
      image
    );

    let label = [
      0x00, 0x02, 0x0c, 0x00, 0x20, 0x00, 0x00, 0x00, 0xe8, 0x03, 0x00, 0x00, 0xf4, 0x01, 0x00,
      0x00, 0x00, 0x02, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    assert_eq!(
      LabelControl::from_bytes(&label)
        .unwrap()
        .to_bytes()
        .unwrap(),
      label
    );

    let spin_button = [
      0x00, 0x02, 0x0c, 0x00, 0x08, 0x00, 0x00, 0x00, 0xe8, 0x03, 0x00, 0x00, 0xf4, 0x01, 0x00,
      0x00,
    ];
    assert_eq!(
      SpinButtonControl::from_bytes(&spin_button)
        .unwrap()
        .to_bytes()
        .unwrap(),
      spin_button
    );

    let scroll_bar = [
      0x00, 0x02, 0x0c, 0x00, 0x08, 0x00, 0x00, 0x00, 0xe8, 0x03, 0x00, 0x00, 0xf4, 0x01, 0x00,
      0x00,
    ];
    assert_eq!(
      ScrollBarControl::from_bytes(&scroll_bar)
        .unwrap()
        .to_bytes()
        .unwrap(),
      scroll_bar
    );

    for (class_index, bytes) in [
      (12, image.as_slice()),
      (16, spin_button.as_slice()),
      (21, label.as_slice()),
      (47, scroll_bar.as_slice()),
    ] {
      let class = CachedControlClass::from_raw(class_index);
      let value = read_cached_form_control(class, bytes, 0).unwrap();
      assert_eq!(write_cached_form_control(&value, class).unwrap(), bytes);
    }
  }

  #[test]
  fn remaining_cached_controls_round_trip_every_optional_field() {
    let image_mask = ImagePropertyMask::AUTO_SIZE
      | ImagePropertyMask::BORDER_COLOR
      | ImagePropertyMask::BACK_COLOR
      | ImagePropertyMask::BORDER_STYLE
      | ImagePropertyMask::MOUSE_POINTER
      | ImagePropertyMask::PICTURE_SIZE_MODE
      | ImagePropertyMask::SPECIAL_EFFECT
      | ImagePropertyMask::SIZE
      | ImagePropertyMask::PICTURE
      | ImagePropertyMask::PICTURE_ALIGNMENT
      | ImagePropertyMask::PICTURE_TILING
      | ImagePropertyMask::VARIOUS_PROPERTY_BITS
      | ImagePropertyMask::MOUSE_ICON;
    let image = ImageControl {
      minor_version: 0,
      major_version: 2,
      property_mask: image_mask,
      auto_size: true,
      picture_tiling: true,
      data_block: ImageDataBlock {
        border_color: Some(aligned(color(0x8000_0006), &[])),
        back_color: Some(aligned(color(0x8000_000f), &[])),
        border_style: Some(aligned(FmBorderStyle::Single, &[])),
        mouse_pointer: Some(aligned(FmMousePointer::Cross, &[])),
        picture_size_mode: Some(aligned(FmPictureSizeMode::Zoom, &[])),
        special_effect: Some(aligned(FmSpecialEffect::Compatibility(4), &[])),
        picture_marker: Some(aligned(PersistenceMarker, &[])),
        picture_alignment: Some(aligned(FmPictureAlignment::Compatibility(5), &[])),
        various_property_bits: Some(aligned(various(0x1b), &[0xa1])),
        mouse_icon_marker: Some(aligned(PersistenceMarker, &[])),
        trailing_padding: vec![0, 0],
      },
      size: FmSize {
        width: 1_000,
        height: 500,
      },
      stream_data: PictureStreamData {
        picture: Some(picture(1)),
        mouse_icon: Some(picture(2)),
      },
    };
    let bytes = image.to_bytes().unwrap();
    assert_eq!(ImageControl::from_bytes(&bytes).unwrap(), image);

    let label_mask = LabelPropertyMask::FORE_COLOR
      | LabelPropertyMask::BACK_COLOR
      | LabelPropertyMask::VARIOUS_PROPERTY_BITS
      | LabelPropertyMask::CAPTION
      | LabelPropertyMask::PICTURE_POSITION
      | LabelPropertyMask::SIZE
      | LabelPropertyMask::MOUSE_POINTER
      | LabelPropertyMask::BORDER_COLOR
      | LabelPropertyMask::BORDER_STYLE
      | LabelPropertyMask::SPECIAL_EFFECT
      | LabelPropertyMask::PICTURE
      | LabelPropertyMask::ACCELERATOR
      | LabelPropertyMask::MOUSE_ICON;
    let label = LabelControl {
      minor_version: 0,
      major_version: 2,
      property_mask: label_mask,
      data_block: LabelDataBlock {
        fore_color: Some(aligned(color(0x8000_0008), &[])),
        back_color: Some(aligned(color(0x8000_000f), &[])),
        various_property_bits: Some(aligned(various(0x1b), &[])),
        caption: Some(aligned(
          CountOfBytesWithCompressionFlag {
            byte_count: 4,
            compressed: true,
          },
          &[],
        )),
        picture_position: Some(aligned(FmPicturePosition::Compatibility(7), &[])),
        mouse_pointer: Some(aligned(FmMousePointer::Cross, &[])),
        border_color: Some(aligned(color(0x8000_0006), &[0xa1, 0xa2, 0xa3])),
        border_style: Some(aligned(FmBorderStyle::Single, &[])),
        special_effect: Some(aligned(FmSpecialEffect::Sunken, &[])),
        picture_marker: Some(aligned(PersistenceMarker, &[])),
        accelerator: Some(aligned(u16::from(b'T'), &[])),
        mouse_icon_marker: Some(aligned(PersistenceMarker, &[])),
        trailing_padding: vec![0, 0],
      },
      extra_data_block: LabelExtraDataBlock {
        caption: Some(FmString {
          bytes: b"Test".to_vec(),
          padding_after: vec![],
          length_mode: FmStringLengthMode::Declared,
        }),
        size: Some(FmSize {
          width: 1_000,
          height: 500,
        }),
      },
      stream_data: PictureStreamData {
        picture: Some(picture(3)),
        mouse_icon: Some(picture(4)),
      },
      text_props: text_props(),
    };
    let bytes = label.to_bytes().unwrap();
    assert_eq!(LabelControl::from_bytes(&bytes).unwrap(), label);

    let spin_mask = SpinButtonPropertyMask::FORE_COLOR
      | SpinButtonPropertyMask::BACK_COLOR
      | SpinButtonPropertyMask::VARIOUS_PROPERTY_BITS
      | SpinButtonPropertyMask::SIZE
      | SpinButtonPropertyMask::MIN
      | SpinButtonPropertyMask::MAX
      | SpinButtonPropertyMask::POSITION
      | SpinButtonPropertyMask::PREV_ENABLED
      | SpinButtonPropertyMask::NEXT_ENABLED
      | SpinButtonPropertyMask::SMALL_CHANGE
      | SpinButtonPropertyMask::ORIENTATION
      | SpinButtonPropertyMask::DELAY
      | SpinButtonPropertyMask::MOUSE_ICON
      | SpinButtonPropertyMask::MOUSE_POINTER;
    let spin_button = SpinButtonControl {
      minor_version: 0,
      major_version: 2,
      property_mask: spin_mask,
      data_block: SpinButtonDataBlock {
        fore_color: Some(aligned(color(0x8000_0008), &[])),
        back_color: Some(aligned(color(0x8000_000f), &[])),
        various_property_bits: Some(aligned(various(0x19), &[])),
        min: Some(aligned(-10, &[])),
        max: Some(aligned(10, &[])),
        position: Some(aligned(1, &[])),
        prev_enabled: Some(aligned(EnabledState::Disabled, &[])),
        next_enabled: Some(aligned(EnabledState::Disabled, &[])),
        small_change: Some(aligned(2, &[])),
        orientation: Some(aligned(FmOrientation::Horizontal, &[])),
        delay: Some(aligned(50, &[])),
        mouse_icon_marker: Some(aligned(PersistenceMarker, &[])),
        mouse_pointer: Some(aligned(FmMousePointer::Cross, &[])),
        trailing_padding: vec![0],
      },
      size: FmSize {
        width: 1_000,
        height: 500,
      },
      mouse_icon: Some(picture(5)),
    };
    let bytes = spin_button.to_bytes().unwrap();
    assert_eq!(SpinButtonControl::from_bytes(&bytes).unwrap(), spin_button);
    let mut invalid_spin_button = spin_button.clone();
    invalid_spin_button
      .data_block
      .various_property_bits
      .as_mut()
      .unwrap()
      .value = various(0x1b);
    assert!(invalid_spin_button.to_bytes().is_err());

    let scroll_mask = ScrollBarPropertyMask::FORE_COLOR
      | ScrollBarPropertyMask::BACK_COLOR
      | ScrollBarPropertyMask::VARIOUS_PROPERTY_BITS
      | ScrollBarPropertyMask::SIZE
      | ScrollBarPropertyMask::MOUSE_POINTER
      | ScrollBarPropertyMask::MIN
      | ScrollBarPropertyMask::MAX
      | ScrollBarPropertyMask::POSITION
      | ScrollBarPropertyMask::PREV_ENABLED
      | ScrollBarPropertyMask::NEXT_ENABLED
      | ScrollBarPropertyMask::SMALL_CHANGE
      | ScrollBarPropertyMask::LARGE_CHANGE
      | ScrollBarPropertyMask::ORIENTATION
      | ScrollBarPropertyMask::PROPORTIONAL_THUMB
      | ScrollBarPropertyMask::DELAY
      | ScrollBarPropertyMask::MOUSE_ICON;
    let scroll_bar = ScrollBarControl {
      minor_version: 0,
      major_version: 2,
      property_mask: scroll_mask,
      data_block: ScrollBarDataBlock {
        fore_color: Some(aligned(color(0x8000_0008), &[])),
        back_color: Some(aligned(color(0x8000_000f), &[])),
        various_property_bits: Some(aligned(various(0x19), &[])),
        mouse_pointer: Some(aligned(FmMousePointer::Cross, &[])),
        min: Some(aligned(-10, &[0xa1, 0xa2, 0xa3])),
        max: Some(aligned(10, &[])),
        position: Some(aligned(1, &[])),
        prev_enabled: Some(aligned(EnabledState::Disabled, &[])),
        next_enabled: Some(aligned(EnabledState::Disabled, &[])),
        small_change: Some(aligned(2, &[])),
        large_change: Some(aligned(4, &[])),
        orientation: Some(aligned(FmOrientation::Horizontal, &[])),
        proportional_thumb: Some(aligned(ProportionalThumb::Proportional, &[])),
        delay: Some(aligned(50, &[0xa4, 0xa5])),
        mouse_icon_marker: Some(aligned(PersistenceMarker, &[])),
        trailing_padding: vec![0, 0],
      },
      size: FmSize {
        width: 1_000,
        height: 500,
      },
      mouse_icon: Some(picture(6)),
    };
    let bytes = scroll_bar.to_bytes().unwrap();
    assert_eq!(ScrollBarControl::from_bytes(&bytes).unwrap(), scroll_bar);
    let mut invalid_scroll_bar = scroll_bar.clone();
    invalid_scroll_bar
      .data_block
      .next_enabled
      .as_mut()
      .unwrap()
      .value = EnabledState::Enabled;
    assert!(invalid_scroll_bar.to_bytes().is_err());
  }

  #[test]
  fn restricted_oforms_scalars_reject_values_outside_spec_ranges() {
    let mut text = text_props();
    text.property_mask |= TextPropsPropertyMask::FONT_WEIGHT;
    text.data_block.font_weight = Some(aligned(1_001, &[]));
    assert!(text.to_bytes().is_err());
    text.data_block.font_weight = Some(aligned(1_000, &[]));
    text.data_block.font_height.as_mut().unwrap().value = 4_294_968;
    assert!(text.to_bytes().is_err());

    text.data_block.font_height.as_mut().unwrap().value = 4_294_967;
    text.property_mask |= TextPropsPropertyMask::FONT_PITCH_AND_FAMILY;
    text.data_block.font_pitch_and_family = Some(aligned(
      FmFontPitchAndFamily {
        pitch: FmFontPitch::Compatibility(3),
        family: FmFontFamily::Roman,
      },
      &[],
    ));
    assert!(text.to_bytes().is_err());

    let form_data = FormDataBlock {
      zoom: Some(aligned(9, &[])),
      ..FormDataBlock::default()
    };
    assert!(form_data.validate().is_err());
    let form_data = FormDataBlock {
      draw_buffer: Some(aligned(15_999, &[])),
      ..FormDataBlock::default()
    };
    assert!(form_data.validate().is_err());
    let page_data = PageDataBlock {
      transition_period: Some(aligned(10_001, &[])),
      ..PageDataBlock::default()
    };
    assert!(validate_page_data(&page_data).is_err());

    let mut tab_data = TabStripDataBlock {
      tab_fixed_width: Some(aligned(254_001, &[])),
      ..TabStripDataBlock::default()
    };
    let mut tab_extra = TabStripExtraDataBlock::default();
    assert!(validate_tab_strip_data(&tab_data, &tab_extra).is_err());
    tab_data.tab_fixed_width = Some(aligned(254_000, &[]));
    tab_data.tab_data_count = Some(aligned(1, &[]));
    assert!(validate_tab_strip_data(&tab_data, &tab_extra).is_err());
    tab_extra.items = Some(vec![ArrayString {
      character_count: 1,
      compressed: true,
      bytes: b"A".to_vec(),
      padding_after: vec![0, 0, 0],
    }]);
    tab_data.list_index = Some(aligned(1, &[]));
    assert!(validate_tab_strip_data(&tab_data, &tab_extra).is_err());
    tab_data.list_index = Some(aligned(0, &[]));
    assert!(validate_tab_strip_data(&tab_data, &tab_extra).is_ok());

    let invalid_text_column = MorphDataDataBlock {
      text_column: Some(aligned(-2, &[])),
      ..MorphDataDataBlock::default()
    };
    assert!(validate_morph_data(&invalid_text_column).is_err());
    let invalid_column_count = MorphDataDataBlock {
      column_count: Some(aligned(-2, &[])),
      ..MorphDataDataBlock::default()
    };
    assert!(validate_morph_data(&invalid_column_count).is_err());

    assert!(
      validate_position_range(
        Some(&aligned(10, &[])),
        Some(&aligned(-10, &[])),
        Some(&aligned(11, &[])),
        100,
        "SpinButton",
      )
      .is_err()
    );

    let mut toggle = morph_data();
    toggle.data_block.display_style.as_mut().unwrap().value = FmDisplayStyle::Toggle;
    toggle
      .data_block
      .various_property_bits
      .as_mut()
      .unwrap()
      .value = various(0x2c80_081b);
    toggle.data_block.special_effect.as_mut().unwrap().value = FmSpecialEffect::Raised;
    assert!(validate_morph_cached_class(&toggle, CachedControlClass::ToggleButton).is_err());
    assert!(validate_morph_cached_class(&toggle, CachedControlClass::CheckBox).is_err());
    toggle.data_block.special_effect.as_mut().unwrap().value = FmSpecialEffect::Sunken;
    assert!(validate_morph_cached_class(&toggle, CachedControlClass::ToggleButton).is_ok());

    assert!(
      validate_control_various(
        Some(&aligned(various(0x1000_001b), &[])),
        CachedControlClass::Image,
      )
      .is_err()
    );
    assert!(
      validate_control_various(
        Some(&aligned(various(0x0000_0013), &[])),
        CachedControlClass::TabStrip,
      )
      .is_err()
    );

    let mut list_box = morph_data();
    list_box.data_block.display_style.as_mut().unwrap().value = FmDisplayStyle::List;
    list_box
      .data_block
      .various_property_bits
      .as_mut()
      .unwrap()
      .value = various(0x2c80_081b);
    assert!(validate_morph_cached_class(&list_box, CachedControlClass::ListBox).is_err());
    list_box.data_block.scroll_bars = Some(aligned(FmScrollBars::Both, &[]));
    assert!(validate_morph_cached_class(&list_box, CachedControlClass::ListBox).is_ok());

    let mut invalid_site = SiteDataBlock {
      bit_flags: Some(aligned(SiteFlags::PROMOTE_CONTROLS, &[])),
      clsid_cache_index: Some(aligned(
        SiteClassIndex::Cached(CachedControlClass::CommandButton),
        &[],
      )),
      ..SiteDataBlock::default()
    };
    assert!(validate_site_class_flags(&invalid_site).is_err());
    invalid_site.clsid_cache_index.as_mut().unwrap().value =
      SiteClassIndex::Cached(CachedControlClass::Frame);
    assert!(validate_site_class_flags(&invalid_site).is_ok());
  }

  #[test]
  fn form_control_round_trips_empty_page_container() {
    let bytes = [
      0x00, 0x04, 0x1c, 0x00, 0x40, 0x0c, 0x00, 0x08, 0x04, 0x80, 0x00, 0x00, 0x00, 0x7d, 0x00,
      0x00, 0x07, 0x07, 0x00, 0x00, 0x2b, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
      0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let value = FormControl::from_bytes(&bytes).unwrap();
    assert!(value.site_data.sites.is_empty());
    assert_eq!(value.to_bytes().unwrap(), bytes);
    assert_eq!(
      FormObjectStream::from_form(&value, &[])
        .unwrap()
        .to_bytes(&value)
        .unwrap(),
      []
    );
  }

  #[test]
  fn parent_storage_recursively_round_trips_known_streams_and_preserves_other_entries() {
    const EMPTY_FORM: [u8; 40] = [
      0x00, 0x04, 0x1c, 0x00, 0x40, 0x0c, 0x00, 0x08, 0x04, 0x80, 0x00, 0x00, 0x00, 0x7d, 0x00,
      0x00, 0x07, 0x07, 0x00, 0x00, 0x2b, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
      0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let mut parent_form = FormControl::from_bytes(&EMPTY_FORM).unwrap();
    let site = OleSiteConcreteControl {
      version: 0,
      property_mask: SitePropertyMask::ID
        | SitePropertyMask::BIT_FLAGS
        | SitePropertyMask::CLSID_CACHE_INDEX,
      data_block: SiteDataBlock {
        id: Some(aligned(1, &[])),
        bit_flags: Some(aligned(
          SiteFlags::TAB_STOP
            | SiteFlags::VISIBLE
            | SiteFlags::AUTO_SIZE
            | SiteFlags::PROMOTE_CONTROLS,
          &[],
        )),
        clsid_cache_index: Some(aligned(
          SiteClassIndex::Cached(CachedControlClass::Form),
          &[],
        )),
        trailing_padding: vec![0, 0],
        ..SiteDataBlock::default()
      },
      extra_data_block: SiteExtraDataBlock::default(),
    };
    let site_size = site.to_bytes().unwrap().len();
    parent_form.site_data = FormSiteData {
      count_of_site_class_info: None,
      class_table: Vec::new(),
      count_of_sites: 1,
      count_of_bytes: u32::try_from(4 + site_size).unwrap(),
      depths_and_types: vec![FormObjectDepthTypeCount {
        depth: 1,
        count: 1,
        site_type: 1,
        compressed_count: false,
      }],
      array_padding: vec![0, 0],
      sites: vec![site],
    };

    let mut compound = CompoundFile::new(crate::cfb::Version::V3).unwrap();
    compound.create_storage("/Form").unwrap();
    compound
      .create_stream("/Form/f", parent_form.to_bytes().unwrap())
      .unwrap();
    compound.create_stream("/Form/o", Vec::new()).unwrap();
    compound
      .create_stream("/Form/custom", vec![9, 8, 7])
      .unwrap();
    compound.create_storage("/Form/i01").unwrap();
    compound
      .replace_storage_class_id("/Form/i01", ParentControlStorage::PAGE_CLASS_ID)
      .unwrap();
    compound
      .create_stream("/Form/i01/f", EMPTY_FORM.to_vec())
      .unwrap();
    compound.create_stream("/Form/i01/o", Vec::new()).unwrap();

    let aggregate = ParentControlStorage::from_compound(&compound, "/Form").unwrap();
    assert_eq!(aggregate.children.len(), 1);
    assert_eq!(aggregate.children[0].site_index, 0);
    assert_eq!(aggregate.children[0].storage_name, "i01");
    assert_eq!(aggregate.children[0].storage.path, Path::new("/Form/i01"));
    aggregate.write_to_compound(&mut compound).unwrap();
    assert_eq!(compound.stream("/Form/custom").unwrap(), [9, 8, 7]);
    assert_eq!(
      compound.stream("/Form/f").unwrap(),
      parent_form.to_bytes().unwrap()
    );
    assert_eq!(compound.stream("/Form/i01/f").unwrap(), EMPTY_FORM);

    let mut located = LocatedParentControlStorage::from_compound(&compound, "/Form").unwrap();
    let identity = located.identity.clone();
    let before_failed_edit = located.clone();
    let failed: Result<()> = located.edit(|model| {
      model.class_id = ParentControlStorage::FRAME_CLASS_ID;
      Ok(())
    });
    assert!(failed.is_err());
    assert_eq!(located, before_failed_edit);
    located
      .edit(|model| {
        model.form.picture_tiling = true;
        model
          .form
          .property_mask
          .insert(FormPropertyMask::PICTURE_TILING);
        Ok(())
      })
      .unwrap();
    located.write_if_modified(&mut compound).unwrap();
    let reopened = LocatedParentControlStorage::from_compound(&compound, "/Form").unwrap();
    assert_eq!(reopened.identity, identity);
    assert!(reopened.model().form.picture_tiling);
    assert!(!reopened.is_modified());
    assert_eq!(compound.stream("/Form/custom").unwrap(), [9, 8, 7]);

    compound.remove_entry("/Form/i01/o").unwrap();
    assert!(ParentControlStorage::from_compound(&compound, "/Form").is_err());
  }

  #[test]
  fn multi_page_x_round_trips_page_and_id_properties() {
    let bytes = [
      0x00, 0x02, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x04, 0x00, 0x00, 0x00, 0x00,
      0x00, 0x00, 0x02, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x0c, 0x00, 0x06, 0x00,
      0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x05,
      0x00, 0x00, 0x00,
    ];
    let value = MultiPageXStream::from_bytes(&bytes).unwrap();
    assert_eq!(value.pages.len(), 3);
    assert_eq!(value.multi_page.page_ids, [4, 5]);
    assert_eq!(value.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn single_stream_ole_control_types_clsid_and_external_payload_boundary() {
    let value = SingleStreamOleControl {
      class_id: SingleStreamOleControl::SCRIPTLET_COMPONENT_CLASS_ID,
      persistence: ExternalComPersistStream {
        bytes: vec![0, 0, b'>', 0, 9, 0],
      },
    };
    let bytes = value.to_bytes();
    let reparsed = SingleStreamOleControl::from_bytes(&bytes).unwrap();
    assert_eq!(reparsed, value);
    assert!(reparsed.is_scriptlet_component());
    assert!(SingleStreamOleControl::from_bytes(&bytes[..15]).is_err());
  }

  #[test]
  fn morph_data_round_trips_all_typed_blocks() {
    let value = morph_data();
    let bytes = value.to_bytes().unwrap();
    assert_eq!(MorphDataControl::from_bytes(&bytes).unwrap(), value);
    assert_eq!(
      MorphDataControl::from_bytes(&bytes)
        .unwrap()
        .to_bytes()
        .unwrap(),
      bytes
    );
    let persistence =
      read_cached_form_control(CachedControlClass::MorphDataLegacy, &bytes, 0).unwrap();
    assert_eq!(
      write_cached_form_control(&persistence, CachedControlClass::MorphDataLegacy).unwrap(),
      bytes
    );
  }

  #[test]
  fn morph_data_preserves_legacy_low_word_string_length() {
    let bytes = [
      0x00, 0x02, 0x28, 0x00, 0x17, 0x01, 0x00, 0x86, 0x00, 0x00, 0x00, 0x00, 0x1b, 0x48, 0x80,
      0x2c, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0xc0, 0xc0,
      0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0xb2, 0x0e, 0x00, 0x00, 0x4b, 0x02, 0x00, 0x00, 0x00,
      0x02, 0x1c, 0x00, 0x37, 0x00, 0x00, 0x00, 0x07, 0x00, 0x04, 0x80, 0x01, 0x00, 0x00, 0x00,
      0xb4, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x56, 0x65, 0x72, 0x64, 0x61, 0x6e, 0x61,
      0x00,
    ];
    let value = MorphDataControl::from_bytes(&bytes).unwrap();
    let font_name = value
      .text_props
      .extra_data_block
      .font_name
      .as_ref()
      .unwrap();
    assert_eq!(font_name.bytes, b"Verdana");
    assert_eq!(
      font_name.length_mode,
      FmStringLengthMode::LowWordCompatibility
    );
    assert_eq!(value.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn morph_data_rejects_invalid_version_and_boundary() {
    assert!(MorphDataControl::from_bytes(&[0, 1, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
    assert!(MorphDataControl::from_bytes(&[0, 2, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
    assert!(MorphDataControl::from_bytes(&[0, 2, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
  }
}
