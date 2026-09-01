//! Shared binary strings and code-page handling used across legacy Office formats.

use std::io::{Read, Seek, Write};

use crate::{
  Error, Result, SdkObject,
  io::{Reader, SdkRead, SdkSize, SdkWrite, Writer},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CodePage(pub u16);

/// MS-DTYP GUID/CLSID fields in their structured form.
///
/// The first three fields are persisted little-endian; `data4` retains its
/// eight bytes in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, SdkObject)]
pub struct Guid {
  pub data1: u32,
  pub data2: u16,
  pub data3: u16,
  pub data4: [u8; 8],
}

/// Raw MS-DTYP FILETIME value: 100-nanosecond ticks since 1601-01-01 UTC.
///
/// Zero is retained as the format-defined "no time recorded" sentinel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct FileTime(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedString {
  pub code_page: CodePage,
  /// Exact persisted bytes, including a terminator when the owning format
  /// defines it as part of the string packet.
  pub bytes: Vec<u8>,
}

impl CodePage {
  pub const WINDOWS_1252: Self = Self(1252);
  pub const UTF_16LE: Self = Self(1200);

  pub fn is_supported(self) -> bool {
    codepage::to_encoding(self.0).is_some()
  }

  pub fn decode(self, bytes: &[u8]) -> Result<String> {
    let encoding = codepage::to_encoding(self.0)
      .ok_or_else(|| Error::invalid(0, format!("unsupported Office code page {}", self.0)))?;
    let (text, had_errors) = encoding.decode_without_bom_handling(bytes);
    if had_errors {
      return Err(Error::invalid(
        0,
        format!("invalid byte sequence for Office code page {}", self.0),
      ));
    }
    Ok(text.into_owned())
  }

  pub fn encode(self, text: &str) -> Result<Vec<u8>> {
    let encoding = codepage::to_encoding(self.0)
      .ok_or_else(|| Error::invalid(0, format!("unsupported Office code page {}", self.0)))?;
    let (bytes, _, had_errors) = encoding.encode(text);
    if had_errors {
      return Err(Error::invalid(
        0,
        format!("text is not representable in Office code page {}", self.0),
      ));
    }
    Ok(bytes.into_owned())
  }
}

impl Guid {
  pub const ZERO: Self = Self {
    data1: 0,
    data2: 0,
    data3: 0,
    data4: [0; 8],
  };

  pub const fn from_fields(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
    Self {
      data1,
      data2,
      data3,
      data4,
    }
  }

  pub fn is_zero(self) -> bool {
    self.data1 == 0 && self.data2 == 0 && self.data3 == 0 && self.data4 == [0; 8]
  }
}

impl std::fmt::Display for Guid {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      formatter,
      "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
      self.data1,
      self.data2,
      self.data3,
      self.data4[0],
      self.data4[1],
      self.data4[2],
      self.data4[3],
      self.data4[4],
      self.data4[5],
      self.data4[6],
      self.data4[7],
    )
  }
}

impl FileTime {
  pub const ZERO: Self = Self(0);

  pub const fn from_ticks(ticks: u64) -> Self {
    Self(ticks)
  }

  pub const fn from_parts(low: u32, high: u32) -> Self {
    Self((low as u64) | ((high as u64) << 32))
  }

  pub const fn low(self) -> u32 {
    self.0 as u32
  }

  pub const fn high(self) -> u32 {
    (self.0 >> 32) as u32
  }

  pub const fn ticks(self) -> u64 {
    self.0
  }

  pub const fn is_recorded(self) -> bool {
    self.0 != 0
  }
}

impl SdkRead for FileTime {
  fn read_from<R: Read + Seek>(reader: &mut Reader<R>) -> Result<Self> {
    Ok(Self::from_ticks(reader.read_u64()?))
  }
}

impl SdkWrite for FileTime {
  fn write_to<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    writer.write_u64(self.ticks())
  }
}

impl SdkSize for FileTime {
  fn sdk_size(&self) -> u64 {
    8
  }
}

impl EncodedString {
  pub fn new(code_page: CodePage, bytes: Vec<u8>) -> Self {
    Self { code_page, bytes }
  }

  pub fn text(&self) -> Result<String> {
    self.code_page.decode(&self.bytes)
  }

  pub fn from_text(code_page: CodePage, text: &str) -> Result<Self> {
    Ok(Self {
      code_page,
      bytes: code_page.encode(text)?,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn encoded_string_preserves_bytes_and_decodes_by_code_page() {
    let value = EncodedString::new(CodePage::WINDOWS_1252, b"caf\xe9".to_vec());
    assert_eq!(value.text().unwrap(), "caf\u{e9}");
    assert_eq!(
      EncodedString::from_text(CodePage::WINDOWS_1252, "caf\u{e9}").unwrap(),
      value
    );
  }

  #[test]
  fn unsupported_code_page_is_explicit() {
    assert!(!CodePage(0xffff).is_supported());
    assert!(CodePage(0xffff).decode(b"text").is_err());
  }

  #[test]
  fn guid_uses_ms_dtyp_field_endianness_and_canonical_display() {
    let value = Guid::from_fields(
      0x0011_2233,
      0x4455,
      0x6677,
      [0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
    );
    let expected = [
      0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
      0xff,
    ];
    let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
    value.write_to(&mut writer).unwrap();
    assert_eq!(writer.into_inner().into_inner(), expected);

    let mut reader = Reader::new(std::io::Cursor::new(expected)).unwrap();
    assert_eq!(Guid::read_from(&mut reader).unwrap(), value);
    assert_eq!(value.to_string(), "00112233-4455-6677-8899-aabbccddeeff");
    assert_eq!(value.sdk_size(), 16);
    assert!(Guid::ZERO.is_zero());
  }

  #[test]
  fn filetime_parts_round_trip_without_endian_ambiguity() {
    let value = FileTime::from_parts(0x89ab_cdef, 0x0123_4567);
    assert_eq!(value.ticks(), 0x0123_4567_89ab_cdef);
    assert_eq!(value.low(), 0x89ab_cdef);
    assert_eq!(value.high(), 0x0123_4567);
  }

  #[test]
  fn filetime_binary_round_trip_is_little_endian() {
    let value = FileTime::from_ticks(0x0123_4567_89ab_cdef);
    let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
    value.write_to(&mut writer).unwrap();
    let bytes = writer.into_inner().into_inner();
    assert_eq!(bytes, 0x0123_4567_89ab_cdef_u64.to_le_bytes());

    let mut reader = Reader::new(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(FileTime::read_from(&mut reader).unwrap(), value);
  }
}
