//! MS-OLEPS property set stream structures.

use std::io::{Cursor, Write};

use crate::{
  Error, Result, SdkObject,
  common::CodePage,
  io::{BinaryFormat, IoContext, Reader, SdkRead, SdkWrite, Writer},
  limits::Limits,
};

pub const PROPERTY_SET_BYTE_ORDER: u16 = 0xfffe;

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct PropertyIdentifierAndOffset {
  pub property_identifier: u32,
  pub offset: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
struct PropertySetHeader {
  size: u32,
  property_count: u32,
  #[sdk(count = "property_count")]
  properties: Vec<PropertyIdentifierAndOffset>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Property {
  pub identifier: u32,
  pub offset: u32,
  /// Bounded property packet, including its alignment padding.
  pub raw: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dictionary {
  pub entries: Vec<DictionaryEntry>,
  pub padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DictionaryEntry {
  pub property_identifier: u32,
  pub name: DictionaryName,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DictionaryName {
  Mbcs(Vec<u8>),
  Unicode {
    code_units: Vec<u16>,
    padding: Vec<u8>,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropertyType(pub u16);

impl PropertyType {
  pub const EMPTY: Self = Self(0x0000);
  pub const NULL: Self = Self(0x0001);
  pub const I2: Self = Self(0x0002);
  pub const I4: Self = Self(0x0003);
  pub const R4: Self = Self(0x0004);
  pub const R8: Self = Self(0x0005);
  pub const CY: Self = Self(0x0006);
  pub const DATE: Self = Self(0x0007);
  pub const BSTR: Self = Self(0x0008);
  pub const ERROR: Self = Self(0x000a);
  pub const BOOL: Self = Self(0x000b);
  pub const DECIMAL: Self = Self(0x000e);
  pub const I1: Self = Self(0x0010);
  pub const UI1: Self = Self(0x0011);
  pub const UI2: Self = Self(0x0012);
  pub const UI4: Self = Self(0x0013);
  pub const I8: Self = Self(0x0014);
  pub const UI8: Self = Self(0x0015);
  pub const INT: Self = Self(0x0016);
  pub const UINT: Self = Self(0x0017);
  pub const LPSTR: Self = Self(0x001e);
  pub const LPWSTR: Self = Self(0x001f);
  pub const FILETIME: Self = Self(0x0040);
  pub const BLOB: Self = Self(0x0041);
  pub const STREAM: Self = Self(0x0042);
  pub const STORAGE: Self = Self(0x0043);
  pub const STREAMED_OBJECT: Self = Self(0x0044);
  pub const STORED_OBJECT: Self = Self(0x0045);
  pub const BLOB_OBJECT: Self = Self(0x0046);
  pub const CF: Self = Self(0x0047);
  pub const CLSID: Self = Self(0x0048);
  pub const VERSIONED_STREAM: Self = Self(0x0049);
  pub const VECTOR_I2: Self = Self(0x1002);
  pub const VECTOR_I4: Self = Self(0x1003);
  pub const VECTOR_R4: Self = Self(0x1004);
  pub const VECTOR_R8: Self = Self(0x1005);
  pub const VECTOR_CY: Self = Self(0x1006);
  pub const VECTOR_DATE: Self = Self(0x1007);
  pub const VECTOR_BSTR: Self = Self(0x1008);
  pub const VECTOR_ERROR: Self = Self(0x100a);
  pub const VECTOR_BOOL: Self = Self(0x100b);
  pub const VECTOR_VARIANT: Self = Self(0x100c);
  pub const VECTOR_I1: Self = Self(0x1010);
  pub const VECTOR_UI1: Self = Self(0x1011);
  pub const VECTOR_UI2: Self = Self(0x1012);
  pub const VECTOR_UI4: Self = Self(0x1013);
  pub const VECTOR_I8: Self = Self(0x1014);
  pub const VECTOR_UI8: Self = Self(0x1015);
  pub const VECTOR_LPSTR: Self = Self(0x101e);
  pub const VECTOR_LPWSTR: Self = Self(0x101f);
  pub const VECTOR_FILETIME: Self = Self(0x1040);
  pub const VECTOR_CF: Self = Self(0x1047);
  pub const VECTOR_CLSID: Self = Self(0x1048);
  pub const ARRAY_I2: Self = Self(0x2002);
  pub const ARRAY_I4: Self = Self(0x2003);
  pub const ARRAY_R4: Self = Self(0x2004);
  pub const ARRAY_R8: Self = Self(0x2005);
  pub const ARRAY_CY: Self = Self(0x2006);
  pub const ARRAY_DATE: Self = Self(0x2007);
  pub const ARRAY_BSTR: Self = Self(0x2008);
  pub const ARRAY_ERROR: Self = Self(0x200a);
  pub const ARRAY_BOOL: Self = Self(0x200b);
  pub const ARRAY_VARIANT: Self = Self(0x200c);
  pub const ARRAY_DECIMAL: Self = Self(0x200e);
  pub const ARRAY_I1: Self = Self(0x2010);
  pub const ARRAY_UI1: Self = Self(0x2011);
  pub const ARRAY_UI2: Self = Self(0x2012);
  pub const ARRAY_UI4: Self = Self(0x2013);
  pub const ARRAY_INT: Self = Self(0x2016);
  pub const ARRAY_UINT: Self = Self(0x2017);
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct ArrayDimension {
  pub size: u32,
  pub index_offset: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct Decimal {
  pub reserved: u16,
  pub scale: u8,
  pub sign: u8,
  pub high: u32,
  pub low: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArrayValue {
  I8(Vec<i8>),
  U8(Vec<u8>),
  I16(Vec<i16>),
  U16(Vec<u16>),
  I32(Vec<i32>),
  U32(Vec<u32>),
  I64(Vec<i64>),
  F32Bits(Vec<u32>),
  F64Bits(Vec<u64>),
  Bool(Vec<i16>),
  Decimal(Vec<Decimal>),
  CodePageStrings(Vec<CodePageStringPacket>),
  Variants(Vec<TypedPropertyValue>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodePageStringPacket {
  pub bytes: Vec<u8>,
  pub padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnicodeStringPacket {
  pub code_units: Vec<u16>,
  pub padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardDataPacket {
  pub format: u32,
  pub data: Vec<u8>,
  pub padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VectorValue {
  I8(Vec<i8>),
  U8(Vec<u8>),
  I16(Vec<i16>),
  U16(Vec<u16>),
  I32(Vec<i32>),
  U32(Vec<u32>),
  I64(Vec<i64>),
  U64(Vec<u64>),
  F32Bits(Vec<u32>),
  F64Bits(Vec<u64>),
  Bool(Vec<i16>),
  Filetime(Vec<u64>),
  ClipboardData(Vec<ClipboardDataPacket>),
  Clsid(Vec<[u8; 16]>),
  CodePageStrings(Vec<CodePageStringPacket>),
  UnicodeStrings(Vec<UnicodeStringPacket>),
  Variants(Vec<TypedPropertyValue>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedPropertyValue {
  Empty {
    reserved: u16,
    trailing: Vec<u8>,
  },
  Null {
    reserved: u16,
    trailing: Vec<u8>,
  },
  I8Bit {
    property_type: PropertyType,
    reserved: u16,
    value: i8,
    padding: Vec<u8>,
  },
  U8Bit {
    property_type: PropertyType,
    reserved: u16,
    value: u8,
    padding: Vec<u8>,
  },
  I16 {
    reserved: u16,
    value: i16,
    padding: Vec<u8>,
  },
  U16 {
    reserved: u16,
    value: u16,
    padding: Vec<u8>,
  },
  I32 {
    property_type: PropertyType,
    reserved: u16,
    value: i32,
    trailing: Vec<u8>,
  },
  U32 {
    property_type: PropertyType,
    reserved: u16,
    value: u32,
    trailing: Vec<u8>,
  },
  I64 {
    property_type: PropertyType,
    reserved: u16,
    value: i64,
    trailing: Vec<u8>,
  },
  U64 {
    reserved: u16,
    value: u64,
    trailing: Vec<u8>,
  },
  F32Bits {
    reserved: u16,
    bits: u32,
    trailing: Vec<u8>,
  },
  F64Bits {
    property_type: PropertyType,
    reserved: u16,
    bits: u64,
    trailing: Vec<u8>,
  },
  Bool {
    reserved: u16,
    value: i16,
    padding: Vec<u8>,
  },
  Decimal {
    reserved: u16,
    value: Decimal,
    trailing: Vec<u8>,
  },
  Filetime {
    reserved: u16,
    value: u64,
    trailing: Vec<u8>,
  },
  CodePageString {
    property_type: PropertyType,
    reserved: u16,
    bytes: Vec<u8>,
    padding: Vec<u8>,
  },
  UnicodeString {
    reserved: u16,
    code_units: Vec<u16>,
    padding: Vec<u8>,
  },
  Blob {
    property_type: PropertyType,
    reserved: u16,
    bytes: Vec<u8>,
    padding: Vec<u8>,
  },
  IndirectPropertyName {
    property_type: PropertyType,
    reserved: u16,
    bytes: Vec<u8>,
    padding: Vec<u8>,
  },
  VersionedStream {
    reserved: u16,
    version_guid: [u8; 16],
    stream_name: Vec<u8>,
    padding: Vec<u8>,
  },
  ClipboardData {
    reserved: u16,
    format: u32,
    data: Vec<u8>,
    padding: Vec<u8>,
  },
  Clsid {
    reserved: u16,
    value: [u8; 16],
    trailing: Vec<u8>,
  },
  Vector {
    property_type: PropertyType,
    reserved: u16,
    values: VectorValue,
    padding: Vec<u8>,
  },
  Array {
    property_type: PropertyType,
    reserved: u16,
    dimensions: Vec<ArrayDimension>,
    values: ArrayValue,
    padding: Vec<u8>,
  },
  Unknown {
    property_type: PropertyType,
    reserved: u16,
    raw: Vec<u8>,
  },
}

impl Property {
  pub fn typed_value(&self) -> Result<TypedPropertyValue> {
    TypedPropertyValue::from_bytes(&self.raw)
  }

  pub fn dictionary(&self, code_page: u16) -> Result<Dictionary> {
    if self.identifier != 0 {
      return Err(Error::invalid(
        self.offset as u64,
        "OLEPS dictionary must have property identifier 0",
      ));
    }
    Dictionary::from_bytes(&self.raw, code_page)
  }

  /// Decodes a scalar OLEPS string directly from this property's persisted
  /// packet without first allocating a second `TypedPropertyValue` payload.
  ///
  /// `VT_LPSTR` and `VT_BSTR` require the containing property's code page;
  /// `VT_LPWSTR` is decoded as UTF-16LE. The format-defined terminal NUL is
  /// excluded from the returned Rust `String`.
  pub fn string_value(&self, code_page: Option<u16>) -> Result<Option<String>> {
    let header = self.raw.get(..8).ok_or_else(|| {
      Error::invalid(
        self.offset as u64,
        "OLEPS string property packet is shorter than its header",
      )
    })?;
    let property_type = PropertyType(u16::from_le_bytes([header[0], header[1]]));
    if !matches!(
      property_type,
      PropertyType::LPSTR | PropertyType::BSTR | PropertyType::LPWSTR
    ) {
      return Ok(None);
    }
    let count = usize::try_from(u32::from_le_bytes([
      header[4], header[5], header[6], header[7],
    ]))
    .map_err(|_| Error::Limit("OLEPS string length does not fit usize".into()))?;
    let payload = &self.raw[8..];
    match property_type {
      PropertyType::LPSTR | PropertyType::BSTR => {
        let bytes = payload.get(..count).ok_or_else(|| {
          Error::invalid(
            self.offset as u64,
            "OLEPS code-page string exceeds its property packet",
          )
        })?;
        let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
        let code_page = code_page.ok_or_else(|| {
          Error::invalid(
            self.offset as u64,
            "OLEPS code-page string has no CodePage property",
          )
        })?;
        CodePage(code_page).decode(bytes).map(Some)
      }
      PropertyType::LPWSTR => {
        let byte_count = count
          .checked_mul(2)
          .ok_or_else(|| Error::Limit("OLEPS Unicode string byte length overflow".into()))?;
        let bytes = payload.get(..byte_count).ok_or_else(|| {
          Error::invalid(
            self.offset as u64,
            "OLEPS Unicode string exceeds its property packet",
          )
        })?;
        let has_terminal_nul = bytes
          .get(byte_count.saturating_sub(2)..byte_count)
          .is_some_and(|unit| unit == [0, 0]);
        let value_bytes = if has_terminal_nul {
          &bytes[..byte_count - 2]
        } else {
          bytes
        };
        char::decode_utf16(
          value_bytes
            .chunks_exact(2)
            .map(|unit| u16::from_le_bytes([unit[0], unit[1]])),
        )
        .collect::<std::result::Result<String, _>>()
        .map(Some)
        .map_err(|_| {
          Error::invalid(
            self.offset as u64,
            "OLEPS Unicode string contains an unpaired surrogate",
          )
        })
      }
      _ => unreachable!("non-string property type returned above"),
    }
  }
}

impl Dictionary {
  pub fn from_bytes(bytes: &[u8], code_page: u16) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(bytes))?;
    let count = usize::try_from(reader.read_u32()?)
      .map_err(|_| Error::Limit("OLEPS dictionary count does not fit usize".into()))?;
    reader.ensure_allocation(count, 8)?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
      let property_identifier = reader.read_u32()?;
      let len = usize::try_from(reader.read_u32()?)
        .map_err(|_| Error::Limit("OLEPS dictionary name length does not fit usize".into()))?;
      let name = if code_page == 1200 {
        reader.ensure_allocation(len, 2)?;
        let mut code_units = Vec::with_capacity(len);
        for _ in 0..len {
          code_units.push(reader.read_u16()?);
        }
        DictionaryName::Unicode {
          code_units,
          padding: reader.read_alignment(4)?,
        }
      } else {
        DictionaryName::Mbcs(reader.read_vec(len)?)
      };
      entries.push(DictionaryEntry {
        property_identifier,
        name,
      });
    }
    Ok(Self {
      entries,
      padding: read_remaining(&mut reader)?,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer.write_u32(
      u32::try_from(self.entries.len())
        .map_err(|_| Error::Limit("OLEPS dictionary count exceeds u32".into()))?,
    )?;
    for entry in &self.entries {
      writer.write_u32(entry.property_identifier)?;
      match &entry.name {
        DictionaryName::Mbcs(bytes) => {
          writer.write_u32(
            u32::try_from(bytes.len())
              .map_err(|_| Error::Limit("OLEPS dictionary name exceeds u32".into()))?,
          )?;
          writer.write_all(bytes)?;
        }
        DictionaryName::Unicode {
          code_units,
          padding,
        } => {
          writer.write_u32(
            u32::try_from(code_units.len())
              .map_err(|_| Error::Limit("OLEPS dictionary Unicode name exceeds u32".into()))?,
          )?;
          for code_unit in code_units {
            writer.write_u16(*code_unit)?;
          }
          let expected = writer.alignment_padding(4)?;
          if padding.len() != expected {
            return Err(Error::invalid(
              writer.position()?,
              "OLEPS dictionary Unicode padding mismatch",
            ));
          }
          writer.write_all(padding)?;
        }
      }
    }
    writer.write_all(&self.padding)?;
    Ok(writer.into_inner().into_inner())
  }
}

impl TypedPropertyValue {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(bytes))?;
    let property_type = PropertyType(reader.read_u16()?);
    let reserved = reader.read_u16()?;
    let value = match property_type {
      PropertyType::EMPTY => Self::Empty {
        reserved,
        trailing: read_remaining(&mut reader)?,
      },
      PropertyType::NULL => Self::Null {
        reserved,
        trailing: read_remaining(&mut reader)?,
      },
      PropertyType::I1 => Self::I8Bit {
        property_type,
        reserved,
        value: reader.read_i8()?,
        padding: read_remaining(&mut reader)?,
      },
      PropertyType::UI1 => Self::U8Bit {
        property_type,
        reserved,
        value: reader.read_u8()?,
        padding: read_remaining(&mut reader)?,
      },
      PropertyType::I2 => Self::I16 {
        reserved,
        value: reader.read_i16()?,
        padding: read_remaining(&mut reader)?,
      },
      PropertyType::UI2 => Self::U16 {
        reserved,
        value: reader.read_u16()?,
        padding: read_remaining(&mut reader)?,
      },
      PropertyType::I4 | PropertyType::INT => Self::I32 {
        property_type,
        reserved,
        value: reader.read_i32()?,
        trailing: read_remaining(&mut reader)?,
      },
      PropertyType::UI4 | PropertyType::UINT | PropertyType::ERROR => Self::U32 {
        property_type,
        reserved,
        value: reader.read_u32()?,
        trailing: read_remaining(&mut reader)?,
      },
      PropertyType::I8 | PropertyType::CY => Self::I64 {
        property_type,
        reserved,
        value: reader.read_i64()?,
        trailing: read_remaining(&mut reader)?,
      },
      PropertyType::UI8 => Self::U64 {
        reserved,
        value: reader.read_u64()?,
        trailing: read_remaining(&mut reader)?,
      },
      PropertyType::R4 => Self::F32Bits {
        reserved,
        bits: reader.read_u32()?,
        trailing: read_remaining(&mut reader)?,
      },
      PropertyType::R8 | PropertyType::DATE => Self::F64Bits {
        property_type,
        reserved,
        bits: reader.read_u64()?,
        trailing: read_remaining(&mut reader)?,
      },
      PropertyType::BOOL => Self::Bool {
        reserved,
        value: reader.read_i16()?,
        padding: read_remaining(&mut reader)?,
      },
      PropertyType::DECIMAL => Self::Decimal {
        reserved,
        value: Decimal::read_from(&mut reader)?,
        trailing: read_remaining(&mut reader)?,
      },
      PropertyType::FILETIME => Self::Filetime {
        reserved,
        value: reader.read_u64()?,
        trailing: read_remaining(&mut reader)?,
      },
      PropertyType::LPSTR | PropertyType::BSTR => {
        let len = usize::try_from(reader.read_u32()?)
          .map_err(|_| Error::Limit("OLEPS string length does not fit usize".into()))?;
        let value = reader.read_vec(len)?;
        Self::CodePageString {
          property_type,
          reserved,
          bytes: value,
          padding: read_remaining(&mut reader)?,
        }
      }
      PropertyType::LPWSTR => {
        let count = usize::try_from(reader.read_u32()?)
          .map_err(|_| Error::Limit("OLEPS Unicode length does not fit usize".into()))?;
        reader.ensure_allocation(count, 2)?;
        let mut code_units = Vec::with_capacity(count);
        for _ in 0..count {
          code_units.push(reader.read_u16()?);
        }
        Self::UnicodeString {
          reserved,
          code_units,
          padding: read_remaining(&mut reader)?,
        }
      }
      PropertyType::BLOB | PropertyType::BLOB_OBJECT => {
        let len = usize::try_from(reader.read_u32()?)
          .map_err(|_| Error::Limit("OLEPS BLOB length does not fit usize".into()))?;
        Self::Blob {
          property_type,
          reserved,
          bytes: reader.read_vec(len)?,
          padding: read_remaining(&mut reader)?,
        }
      }
      PropertyType::STREAM
      | PropertyType::STORAGE
      | PropertyType::STREAMED_OBJECT
      | PropertyType::STORED_OBJECT => {
        let len = usize::try_from(reader.read_u32()?).map_err(|_| {
          Error::Limit("OLEPS indirect property name length does not fit usize".into())
        })?;
        Self::IndirectPropertyName {
          property_type,
          reserved,
          bytes: reader.read_vec(len)?,
          padding: read_remaining(&mut reader)?,
        }
      }
      PropertyType::CF => {
        let size = usize::try_from(reader.read_u32()?)
          .map_err(|_| Error::Limit("OLEPS ClipboardData size does not fit usize".into()))?;
        if size < 4 {
          return Err(Error::invalid(
            reader.position()?,
            "OLEPS ClipboardData size is smaller than Format",
          ));
        }
        let format = reader.read_u32()?;
        Self::ClipboardData {
          reserved,
          format,
          data: reader.read_vec(size - 4)?,
          padding: read_remaining(&mut reader)?,
        }
      }
      PropertyType::CLSID => Self::Clsid {
        reserved,
        value: reader.read_array::<16>()?,
        trailing: read_remaining(&mut reader)?,
      },
      PropertyType::VERSIONED_STREAM => {
        let version_guid = reader.read_array::<16>()?;
        let len = usize::try_from(reader.read_u32()?).map_err(|_| {
          Error::Limit("OLEPS versioned stream name length does not fit usize".into())
        })?;
        Self::VersionedStream {
          reserved,
          version_guid,
          stream_name: reader.read_vec(len)?,
          padding: read_remaining(&mut reader)?,
        }
      }
      PropertyType::VECTOR_I2
      | PropertyType::VECTOR_I4
      | PropertyType::VECTOR_R4
      | PropertyType::VECTOR_R8
      | PropertyType::VECTOR_CY
      | PropertyType::VECTOR_DATE
      | PropertyType::VECTOR_ERROR
      | PropertyType::VECTOR_BOOL
      | PropertyType::VECTOR_I1
      | PropertyType::VECTOR_UI1
      | PropertyType::VECTOR_UI2
      | PropertyType::VECTOR_UI4
      | PropertyType::VECTOR_I8
      | PropertyType::VECTOR_UI8
      | PropertyType::VECTOR_FILETIME
      | PropertyType::VECTOR_CLSID => {
        let values = read_fixed_vector(&mut reader, property_type)?;
        Self::Vector {
          property_type,
          reserved,
          values,
          padding: read_remaining(&mut reader)?,
        }
      }
      PropertyType::VECTOR_BSTR | PropertyType::VECTOR_LPSTR | PropertyType::VECTOR_LPWSTR => {
        let raw = read_remaining(&mut reader)?;
        let (values, padding) = read_string_vector_bytes(&raw, property_type)?;
        Self::Vector {
          property_type,
          reserved,
          values,
          padding,
        }
      }
      PropertyType::VECTOR_CF => {
        let values = read_clipboard_vector(&mut reader)?;
        Self::Vector {
          property_type,
          reserved,
          values,
          padding: read_remaining(&mut reader)?,
        }
      }
      PropertyType::VECTOR_VARIANT => {
        let raw = read_remaining(&mut reader)?;
        let (values, padding) = read_variant_vector_bytes(&raw)?;
        Self::Vector {
          property_type,
          reserved,
          values,
          padding,
        }
      }
      PropertyType::ARRAY_I2
      | PropertyType::ARRAY_I4
      | PropertyType::ARRAY_R4
      | PropertyType::ARRAY_R8
      | PropertyType::ARRAY_CY
      | PropertyType::ARRAY_DATE
      | PropertyType::ARRAY_BSTR
      | PropertyType::ARRAY_ERROR
      | PropertyType::ARRAY_BOOL
      | PropertyType::ARRAY_VARIANT
      | PropertyType::ARRAY_DECIMAL
      | PropertyType::ARRAY_I1
      | PropertyType::ARRAY_UI1
      | PropertyType::ARRAY_UI2
      | PropertyType::ARRAY_UI4
      | PropertyType::ARRAY_INT
      | PropertyType::ARRAY_UINT => {
        let scalar_type = reader.read_u32()?;
        let expected_scalar_type = u32::from(property_type.0 & !0x2000);
        if scalar_type != expected_scalar_type {
          return Err(Error::invalid(
            reader.position()?,
            "OLEPS array header type does not match property type",
          ));
        }
        let dimension_count = usize::try_from(reader.read_u32()?)
          .map_err(|_| Error::Limit("OLEPS array dimension count does not fit usize".into()))?;
        if !(1..=31).contains(&dimension_count) {
          return Err(Error::invalid(
            reader.position()?,
            "OLEPS array dimension count must be from 1 through 31",
          ));
        }
        reader.ensure_allocation(dimension_count, 8)?;
        let mut dimensions = Vec::with_capacity(dimension_count);
        let mut value_count = 1usize;
        for _ in 0..dimension_count {
          let dimension = ArrayDimension::read_from(&mut reader)?;
          let size = usize::try_from(dimension.size)
            .map_err(|_| Error::Limit("OLEPS array dimension size does not fit usize".into()))?;
          value_count = value_count
            .checked_mul(size)
            .ok_or_else(|| Error::Limit("OLEPS array element count overflow".into()))?;
          dimensions.push(dimension);
        }
        let values = read_array_values(&mut reader, property_type, value_count)?;
        Self::Array {
          property_type,
          reserved,
          dimensions,
          values,
          padding: read_remaining(&mut reader)?,
        }
      }
      _ => Self::Unknown {
        property_type,
        reserved,
        raw: read_remaining(&mut reader)?,
      },
    };
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    match self {
      Self::Empty { reserved, trailing } => {
        write_type(&mut writer, PropertyType::EMPTY, *reserved)?;
        writer.write_all(trailing)?;
      }
      Self::Null { reserved, trailing } => {
        write_type(&mut writer, PropertyType::NULL, *reserved)?;
        writer.write_all(trailing)?;
      }
      Self::I8Bit {
        property_type,
        reserved,
        value,
        padding,
      } => {
        write_type(&mut writer, *property_type, *reserved)?;
        writer.write_i8(*value)?;
        writer.write_all(padding)?;
      }
      Self::U8Bit {
        property_type,
        reserved,
        value,
        padding,
      } => {
        write_type(&mut writer, *property_type, *reserved)?;
        writer.write_u8(*value)?;
        writer.write_all(padding)?;
      }
      Self::I16 {
        reserved,
        value,
        padding,
      } => {
        write_type(&mut writer, PropertyType::I2, *reserved)?;
        writer.write_i16(*value)?;
        writer.write_all(padding)?;
      }
      Self::U16 {
        reserved,
        value,
        padding,
      } => {
        write_type(&mut writer, PropertyType::UI2, *reserved)?;
        writer.write_u16(*value)?;
        writer.write_all(padding)?;
      }
      Self::I32 {
        property_type,
        reserved,
        value,
        trailing,
      } => {
        write_type(&mut writer, *property_type, *reserved)?;
        writer.write_i32(*value)?;
        writer.write_all(trailing)?;
      }
      Self::U32 {
        property_type,
        reserved,
        value,
        trailing,
      } => {
        write_type(&mut writer, *property_type, *reserved)?;
        writer.write_u32(*value)?;
        writer.write_all(trailing)?;
      }
      Self::I64 {
        property_type,
        reserved,
        value,
        trailing,
      } => {
        write_type(&mut writer, *property_type, *reserved)?;
        writer.write_i64(*value)?;
        writer.write_all(trailing)?;
      }
      Self::U64 {
        reserved,
        value,
        trailing,
      } => {
        write_type(&mut writer, PropertyType::UI8, *reserved)?;
        writer.write_u64(*value)?;
        writer.write_all(trailing)?;
      }
      Self::F32Bits {
        reserved,
        bits,
        trailing,
      } => {
        write_type(&mut writer, PropertyType::R4, *reserved)?;
        writer.write_u32(*bits)?;
        writer.write_all(trailing)?;
      }
      Self::F64Bits {
        property_type,
        reserved,
        bits,
        trailing,
      } => {
        write_type(&mut writer, *property_type, *reserved)?;
        writer.write_u64(*bits)?;
        writer.write_all(trailing)?;
      }
      Self::Bool {
        reserved,
        value,
        padding,
      } => {
        write_type(&mut writer, PropertyType::BOOL, *reserved)?;
        writer.write_i16(*value)?;
        writer.write_all(padding)?;
      }
      Self::Decimal {
        reserved,
        value,
        trailing,
      } => {
        write_type(&mut writer, PropertyType::DECIMAL, *reserved)?;
        value.write_to(&mut writer)?;
        writer.write_all(trailing)?;
      }
      Self::Filetime {
        reserved,
        value,
        trailing,
      } => {
        write_type(&mut writer, PropertyType::FILETIME, *reserved)?;
        writer.write_u64(*value)?;
        writer.write_all(trailing)?;
      }
      Self::CodePageString {
        property_type,
        reserved,
        bytes,
        padding,
      } => {
        write_type(&mut writer, *property_type, *reserved)?;
        writer.write_u32(
          u32::try_from(bytes.len())
            .map_err(|_| Error::Limit("OLEPS string exceeds u32".into()))?,
        )?;
        writer.write_all(bytes)?;
        writer.write_all(padding)?;
      }
      Self::UnicodeString {
        reserved,
        code_units,
        padding,
      } => {
        write_type(&mut writer, PropertyType::LPWSTR, *reserved)?;
        writer.write_u32(
          u32::try_from(code_units.len())
            .map_err(|_| Error::Limit("OLEPS Unicode string exceeds u32".into()))?,
        )?;
        for value in code_units {
          writer.write_u16(*value)?;
        }
        writer.write_all(padding)?;
      }
      Self::Blob {
        property_type,
        reserved,
        bytes,
        padding,
      } => {
        if !matches!(
          *property_type,
          PropertyType::BLOB | PropertyType::BLOB_OBJECT
        ) {
          return Err(Error::invalid(0, "OLEPS BLOB type/value mismatch"));
        }
        write_type(&mut writer, *property_type, *reserved)?;
        writer.write_u32(
          u32::try_from(bytes.len()).map_err(|_| Error::Limit("OLEPS BLOB exceeds u32".into()))?,
        )?;
        writer.write_all(bytes)?;
        writer.write_all(padding)?;
      }
      Self::IndirectPropertyName {
        property_type,
        reserved,
        bytes,
        padding,
      } => {
        if !matches!(
          *property_type,
          PropertyType::STREAM
            | PropertyType::STORAGE
            | PropertyType::STREAMED_OBJECT
            | PropertyType::STORED_OBJECT
        ) {
          return Err(Error::invalid(
            0,
            "OLEPS indirect property name type/value mismatch",
          ));
        }
        write_type(&mut writer, *property_type, *reserved)?;
        writer.write_u32(
          u32::try_from(bytes.len())
            .map_err(|_| Error::Limit("OLEPS indirect property name exceeds u32".into()))?,
        )?;
        writer.write_all(bytes)?;
        writer.write_all(padding)?;
      }
      Self::VersionedStream {
        reserved,
        version_guid,
        stream_name,
        padding,
      } => {
        write_type(&mut writer, PropertyType::VERSIONED_STREAM, *reserved)?;
        writer.write_all(version_guid)?;
        writer.write_u32(
          u32::try_from(stream_name.len())
            .map_err(|_| Error::Limit("OLEPS versioned stream name exceeds u32".into()))?,
        )?;
        writer.write_all(stream_name)?;
        writer.write_all(padding)?;
      }
      Self::ClipboardData {
        reserved,
        format,
        data,
        padding,
      } => {
        write_type(&mut writer, PropertyType::CF, *reserved)?;
        let size = data
          .len()
          .checked_add(4)
          .ok_or_else(|| Error::Limit("OLEPS ClipboardData size overflow".into()))?;
        writer.write_u32(
          u32::try_from(size)
            .map_err(|_| Error::Limit("OLEPS ClipboardData size exceeds u32".into()))?,
        )?;
        writer.write_u32(*format)?;
        writer.write_all(data)?;
        writer.write_all(padding)?;
      }
      Self::Clsid {
        reserved,
        value,
        trailing,
      } => {
        write_type(&mut writer, PropertyType::CLSID, *reserved)?;
        writer.write_all(value)?;
        writer.write_all(trailing)?;
      }
      Self::Vector {
        property_type,
        reserved,
        values,
        padding,
      } => {
        if !values.matches_property_type(*property_type) {
          return Err(Error::invalid(0, "OLEPS vector type/value mismatch"));
        }
        write_type(&mut writer, *property_type, *reserved)?;
        values.write_to(&mut writer)?;
        writer.write_all(padding)?;
      }
      Self::Array {
        property_type,
        reserved,
        dimensions,
        values,
        padding,
      } => {
        validate_array(*property_type, dimensions, values)?;
        write_type(&mut writer, *property_type, *reserved)?;
        writer.write_u32(u32::from(property_type.0 & !0x2000))?;
        writer.write_u32(
          u32::try_from(dimensions.len())
            .map_err(|_| Error::Limit("OLEPS array dimension count exceeds u32".into()))?,
        )?;
        for dimension in dimensions {
          dimension.write_to(&mut writer)?;
        }
        values.write_to(&mut writer)?;
        writer.write_all(padding)?;
      }
      Self::Unknown {
        property_type,
        reserved,
        raw,
      } => {
        write_type(&mut writer, *property_type, *reserved)?;
        writer.write_all(raw)?;
      }
    }
    Ok(writer.into_inner().into_inner())
  }
}

impl VectorValue {
  fn matches_property_type(&self, property_type: PropertyType) -> bool {
    matches!(
      (self, property_type),
      (Self::I8(_), PropertyType::VECTOR_I1)
        | (Self::U8(_), PropertyType::VECTOR_UI1)
        | (Self::I16(_), PropertyType::VECTOR_I2)
        | (Self::U16(_), PropertyType::VECTOR_UI2)
        | (Self::I32(_), PropertyType::VECTOR_I4)
        | (
          Self::U32(_),
          PropertyType::VECTOR_UI4 | PropertyType::VECTOR_ERROR
        )
        | (
          Self::I64(_),
          PropertyType::VECTOR_I8 | PropertyType::VECTOR_CY
        )
        | (Self::U64(_), PropertyType::VECTOR_UI8)
        | (Self::F32Bits(_), PropertyType::VECTOR_R4)
        | (
          Self::F64Bits(_),
          PropertyType::VECTOR_R8 | PropertyType::VECTOR_DATE
        )
        | (Self::Bool(_), PropertyType::VECTOR_BOOL)
        | (Self::Filetime(_), PropertyType::VECTOR_FILETIME)
        | (Self::ClipboardData(_), PropertyType::VECTOR_CF)
        | (Self::Clsid(_), PropertyType::VECTOR_CLSID)
        | (
          Self::CodePageStrings(_),
          PropertyType::VECTOR_BSTR | PropertyType::VECTOR_LPSTR
        )
        | (Self::UnicodeStrings(_), PropertyType::VECTOR_LPWSTR)
        | (Self::Variants(_), PropertyType::VECTOR_VARIANT)
    )
  }

  fn write_to<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    let count = match self {
      Self::I8(values) => values.len(),
      Self::U8(values) => values.len(),
      Self::I16(values) => values.len(),
      Self::U16(values) => values.len(),
      Self::I32(values) => values.len(),
      Self::U32(values) => values.len(),
      Self::I64(values) => values.len(),
      Self::U64(values) => values.len(),
      Self::F32Bits(values) => values.len(),
      Self::F64Bits(values) => values.len(),
      Self::Bool(values) => values.len(),
      Self::Filetime(values) => values.len(),
      Self::ClipboardData(values) => values.len(),
      Self::Clsid(values) => values.len(),
      Self::CodePageStrings(values) => values.len(),
      Self::UnicodeStrings(values) => values.len(),
      Self::Variants(values) => values.len(),
    };
    writer.write_u32(
      u32::try_from(count).map_err(|_| Error::Limit("OLEPS vector exceeds u32".into()))?,
    )?;
    match self {
      Self::I8(values) => values.iter().try_for_each(|value| writer.write_i8(*value)),
      Self::U8(values) => writer.write_all(values).map_err(Into::into),
      Self::I16(values) | Self::Bool(values) => {
        values.iter().try_for_each(|value| writer.write_i16(*value))
      }
      Self::U16(values) => values.iter().try_for_each(|value| writer.write_u16(*value)),
      Self::I32(values) => values.iter().try_for_each(|value| writer.write_i32(*value)),
      Self::U32(values) | Self::F32Bits(values) => {
        values.iter().try_for_each(|value| writer.write_u32(*value))
      }
      Self::I64(values) => values.iter().try_for_each(|value| writer.write_i64(*value)),
      Self::U64(values) | Self::F64Bits(values) | Self::Filetime(values) => {
        values.iter().try_for_each(|value| writer.write_u64(*value))
      }
      Self::Clsid(values) => values
        .iter()
        .try_for_each(|value| writer.write_all(value).map_err(Into::into)),
      Self::ClipboardData(values) => {
        for value in values {
          let size = value
            .data
            .len()
            .checked_add(4)
            .ok_or_else(|| Error::Limit("OLEPS clipboard vector element size overflow".into()))?;
          writer.write_u32(
            u32::try_from(size)
              .map_err(|_| Error::Limit("OLEPS clipboard vector element exceeds u32".into()))?,
          )?;
          writer.write_u32(value.format)?;
          writer.write_all(&value.data)?;
          validate_and_write_padding(writer, &value.padding)?;
        }
        Ok(())
      }
      Self::CodePageStrings(values) => {
        for value in values {
          writer.write_u32(
            u32::try_from(value.bytes.len())
              .map_err(|_| Error::Limit("OLEPS string vector element exceeds u32".into()))?,
          )?;
          writer.write_all(&value.bytes)?;
          validate_and_write_padding(writer, &value.padding)?;
        }
        Ok(())
      }
      Self::UnicodeStrings(values) => {
        for value in values {
          writer.write_u32(
            u32::try_from(value.code_units.len())
              .map_err(|_| Error::Limit("OLEPS Unicode vector element exceeds u32".into()))?,
          )?;
          for code_unit in &value.code_units {
            writer.write_u16(*code_unit)?;
          }
          validate_and_write_padding(writer, &value.padding)?;
        }
        Ok(())
      }
      Self::Variants(values) => {
        for value in values {
          let Some(property_type) = typed_property_type(value) else {
            return Err(Error::invalid(
              writer.position()?,
              "invalid nested OLEPS variant vector element",
            ));
          };
          if !variant_type_allowed(property_type, VariantContainer::Vector) {
            return Err(Error::invalid(
              writer.position()?,
              "invalid OLEPS variant vector element type",
            ));
          }
          writer.write_all(&value.to_bytes()?)?;
        }
        Ok(())
      }
    }
  }
}

impl ArrayValue {
  fn len(&self) -> usize {
    match self {
      Self::I8(values) => values.len(),
      Self::U8(values) => values.len(),
      Self::I16(values) => values.len(),
      Self::U16(values) => values.len(),
      Self::I32(values) => values.len(),
      Self::U32(values) => values.len(),
      Self::I64(values) => values.len(),
      Self::F32Bits(values) => values.len(),
      Self::F64Bits(values) => values.len(),
      Self::Bool(values) => values.len(),
      Self::Decimal(values) => values.len(),
      Self::CodePageStrings(values) => values.len(),
      Self::Variants(values) => values.len(),
    }
  }

  fn matches_property_type(&self, property_type: PropertyType) -> bool {
    matches!(
      (self, property_type),
      (Self::I8(_), PropertyType::ARRAY_I1)
        | (Self::U8(_), PropertyType::ARRAY_UI1)
        | (Self::I16(_), PropertyType::ARRAY_I2)
        | (Self::U16(_), PropertyType::ARRAY_UI2)
        | (
          Self::I32(_),
          PropertyType::ARRAY_I4 | PropertyType::ARRAY_INT
        )
        | (
          Self::U32(_),
          PropertyType::ARRAY_UI4 | PropertyType::ARRAY_UINT | PropertyType::ARRAY_ERROR
        )
        | (Self::I64(_), PropertyType::ARRAY_CY)
        | (Self::F32Bits(_), PropertyType::ARRAY_R4)
        | (
          Self::F64Bits(_),
          PropertyType::ARRAY_R8 | PropertyType::ARRAY_DATE
        )
        | (Self::Bool(_), PropertyType::ARRAY_BOOL)
        | (Self::Decimal(_), PropertyType::ARRAY_DECIMAL)
        | (Self::CodePageStrings(_), PropertyType::ARRAY_BSTR)
        | (Self::Variants(_), PropertyType::ARRAY_VARIANT)
    )
  }

  fn write_to<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::I8(values) => values.iter().try_for_each(|value| writer.write_i8(*value)),
      Self::U8(values) => writer.write_all(values).map_err(Into::into),
      Self::I16(values) | Self::Bool(values) => {
        values.iter().try_for_each(|value| writer.write_i16(*value))
      }
      Self::U16(values) => values.iter().try_for_each(|value| writer.write_u16(*value)),
      Self::I32(values) => values.iter().try_for_each(|value| writer.write_i32(*value)),
      Self::U32(values) | Self::F32Bits(values) => {
        values.iter().try_for_each(|value| writer.write_u32(*value))
      }
      Self::I64(values) => values.iter().try_for_each(|value| writer.write_i64(*value)),
      Self::F64Bits(values) => values.iter().try_for_each(|value| writer.write_u64(*value)),
      Self::Decimal(values) => values.iter().try_for_each(|value| value.write_to(writer)),
      Self::CodePageStrings(values) => {
        for value in values {
          writer.write_u32(
            u32::try_from(value.bytes.len())
              .map_err(|_| Error::Limit("OLEPS string array element exceeds u32".into()))?,
          )?;
          writer.write_all(&value.bytes)?;
          validate_and_write_padding(writer, &value.padding)?;
        }
        Ok(())
      }
      Self::Variants(values) => {
        for value in values {
          let property_type = typed_property_type(value)
            .ok_or_else(|| Error::invalid(0, "invalid OLEPS variant array element"))?;
          if !variant_type_allowed(property_type, VariantContainer::Array) {
            return Err(Error::invalid(
              writer.position()?,
              "invalid OLEPS variant array element type",
            ));
          }
          writer.write_all(&value.to_bytes()?)?;
        }
        Ok(())
      }
    }
  }
}

fn validate_array(
  property_type: PropertyType,
  dimensions: &[ArrayDimension],
  values: &ArrayValue,
) -> Result<()> {
  if !values.matches_property_type(property_type) {
    return Err(Error::invalid(0, "OLEPS array type/value mismatch"));
  }
  if !(1..=31).contains(&dimensions.len()) {
    return Err(Error::invalid(
      0,
      "OLEPS array dimension count must be from 1 through 31",
    ));
  }
  let expected_count = dimensions.iter().try_fold(1usize, |count, dimension| {
    let size = usize::try_from(dimension.size)
      .map_err(|_| Error::Limit("OLEPS array dimension size does not fit usize".into()))?;
    count
      .checked_mul(size)
      .ok_or_else(|| Error::Limit("OLEPS array element count overflow".into()))
  })?;
  if values.len() != expected_count {
    return Err(Error::invalid(
      0,
      "OLEPS array dimensions do not match element count",
    ));
  }
  Ok(())
}

#[derive(Clone, Copy)]
enum VariantContainer {
  Vector,
  Array,
}

fn variant_type_allowed(property_type: PropertyType, container: VariantContainer) -> bool {
  match container {
    VariantContainer::Vector => matches!(
      property_type,
      PropertyType::I2
        | PropertyType::I4
        | PropertyType::R4
        | PropertyType::R8
        | PropertyType::CY
        | PropertyType::DATE
        | PropertyType::BSTR
        | PropertyType::ERROR
        | PropertyType::BOOL
        | PropertyType::I1
        | PropertyType::UI1
        | PropertyType::UI2
        | PropertyType::UI4
        | PropertyType::I8
        | PropertyType::UI8
        | PropertyType::LPSTR
        | PropertyType::LPWSTR
        | PropertyType::FILETIME
        | PropertyType::CF
        | PropertyType::CLSID
    ),
    VariantContainer::Array => matches!(
      property_type,
      PropertyType::I2
        | PropertyType::I4
        | PropertyType::R4
        | PropertyType::R8
        | PropertyType::CY
        | PropertyType::DATE
        | PropertyType::BSTR
        | PropertyType::ERROR
        | PropertyType::BOOL
        | PropertyType::DECIMAL
        | PropertyType::I1
        | PropertyType::UI1
        | PropertyType::UI2
        | PropertyType::UI4
        | PropertyType::INT
        | PropertyType::UINT
    ),
  }
}

fn typed_property_type(value: &TypedPropertyValue) -> Option<PropertyType> {
  Some(match value {
    TypedPropertyValue::I8Bit { property_type, .. }
    | TypedPropertyValue::U8Bit { property_type, .. }
    | TypedPropertyValue::I32 { property_type, .. }
    | TypedPropertyValue::U32 { property_type, .. }
    | TypedPropertyValue::I64 { property_type, .. }
    | TypedPropertyValue::F64Bits { property_type, .. }
    | TypedPropertyValue::CodePageString { property_type, .. } => *property_type,
    TypedPropertyValue::I16 { .. } => PropertyType::I2,
    TypedPropertyValue::U16 { .. } => PropertyType::UI2,
    TypedPropertyValue::U64 { .. } => PropertyType::UI8,
    TypedPropertyValue::F32Bits { .. } => PropertyType::R4,
    TypedPropertyValue::Bool { .. } => PropertyType::BOOL,
    TypedPropertyValue::Decimal { .. } => PropertyType::DECIMAL,
    TypedPropertyValue::Filetime { .. } => PropertyType::FILETIME,
    TypedPropertyValue::UnicodeString { .. } => PropertyType::LPWSTR,
    TypedPropertyValue::ClipboardData { .. } => PropertyType::CF,
    TypedPropertyValue::Clsid { .. } => PropertyType::CLSID,
    _ => return None,
  })
}

fn read_variant_vector_bytes(bytes: &[u8]) -> Result<(VectorValue, Vec<u8>)> {
  parse_variant_vector(bytes, true).or_else(|_| parse_variant_vector(bytes, false))
}

fn parse_variant_vector(bytes: &[u8], aligned_strings: bool) -> Result<(VectorValue, Vec<u8>)> {
  let mut reader = Reader::new(Cursor::new(bytes))?;
  let count = usize::try_from(reader.read_u32()?)
    .map_err(|_| Error::Limit("OLEPS variant vector count does not fit usize".into()))?;
  reader.ensure_allocation(count, 4)?;
  let mut values = Vec::with_capacity(count);
  for _ in 0..count {
    values.push(read_variant_scalar(
      &mut reader,
      aligned_strings,
      VariantContainer::Vector,
    )?);
  }
  let padding = read_vector_outer_padding(&mut reader)?;
  Ok((VectorValue::Variants(values), padding))
}

fn read_variant_scalar<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  aligned_strings: bool,
  container: VariantContainer,
) -> Result<TypedPropertyValue> {
  let offset = reader.position()?;
  let property_type = PropertyType(reader.read_u16()?);
  let reserved = reader.read_u16()?;
  if !variant_type_allowed(property_type, container) {
    return Err(Error::invalid(
      offset,
      format!(
        "invalid OLEPS variant element type 0x{:04x}",
        property_type.0
      ),
    ));
  }
  let value = match property_type {
    PropertyType::I1 => TypedPropertyValue::I8Bit {
      property_type,
      reserved,
      value: reader.read_i8()?,
      padding: reader.read_alignment(4)?,
    },
    PropertyType::UI1 => TypedPropertyValue::U8Bit {
      property_type,
      reserved,
      value: reader.read_u8()?,
      padding: reader.read_alignment(4)?,
    },
    PropertyType::I2 => TypedPropertyValue::I16 {
      reserved,
      value: reader.read_i16()?,
      padding: reader.read_alignment(4)?,
    },
    PropertyType::UI2 => TypedPropertyValue::U16 {
      reserved,
      value: reader.read_u16()?,
      padding: reader.read_alignment(4)?,
    },
    PropertyType::I4 | PropertyType::INT => TypedPropertyValue::I32 {
      property_type,
      reserved,
      value: reader.read_i32()?,
      trailing: Vec::new(),
    },
    PropertyType::UI4 | PropertyType::UINT | PropertyType::ERROR => TypedPropertyValue::U32 {
      property_type,
      reserved,
      value: reader.read_u32()?,
      trailing: Vec::new(),
    },
    PropertyType::I8 | PropertyType::CY => TypedPropertyValue::I64 {
      property_type,
      reserved,
      value: reader.read_i64()?,
      trailing: Vec::new(),
    },
    PropertyType::UI8 => TypedPropertyValue::U64 {
      reserved,
      value: reader.read_u64()?,
      trailing: Vec::new(),
    },
    PropertyType::R4 => TypedPropertyValue::F32Bits {
      reserved,
      bits: reader.read_u32()?,
      trailing: Vec::new(),
    },
    PropertyType::R8 | PropertyType::DATE => TypedPropertyValue::F64Bits {
      property_type,
      reserved,
      bits: reader.read_u64()?,
      trailing: Vec::new(),
    },
    PropertyType::BOOL => TypedPropertyValue::Bool {
      reserved,
      value: reader.read_i16()?,
      padding: reader.read_alignment(4)?,
    },
    PropertyType::DECIMAL => TypedPropertyValue::Decimal {
      reserved,
      value: Decimal::read_from(reader)?,
      trailing: Vec::new(),
    },
    PropertyType::LPSTR | PropertyType::BSTR => {
      let len = usize::try_from(reader.read_u32()?)
        .map_err(|_| Error::Limit("OLEPS variant string length does not fit usize".into()))?;
      TypedPropertyValue::CodePageString {
        property_type,
        reserved,
        bytes: reader.read_vec(len)?,
        padding: if aligned_strings {
          reader.read_alignment(4)?
        } else {
          Vec::new()
        },
      }
    }
    PropertyType::LPWSTR => {
      let count = usize::try_from(reader.read_u32()?)
        .map_err(|_| Error::Limit("OLEPS variant Unicode length does not fit usize".into()))?;
      reader.ensure_allocation(count, 2)?;
      let mut code_units = Vec::with_capacity(count);
      for _ in 0..count {
        code_units.push(reader.read_u16()?);
      }
      TypedPropertyValue::UnicodeString {
        reserved,
        code_units,
        padding: if aligned_strings {
          reader.read_alignment(4)?
        } else {
          Vec::new()
        },
      }
    }
    PropertyType::FILETIME => TypedPropertyValue::Filetime {
      reserved,
      value: reader.read_u64()?,
      trailing: Vec::new(),
    },
    PropertyType::CF => {
      let size = usize::try_from(reader.read_u32()?)
        .map_err(|_| Error::Limit("OLEPS variant clipboard size does not fit usize".into()))?;
      if size < 4 {
        return Err(Error::invalid(
          offset,
          "OLEPS variant clipboard size is too small",
        ));
      }
      TypedPropertyValue::ClipboardData {
        reserved,
        format: reader.read_u32()?,
        data: reader.read_vec(size - 4)?,
        padding: reader.read_alignment(4)?,
      }
    }
    PropertyType::CLSID => TypedPropertyValue::Clsid {
      reserved,
      value: reader.read_array::<16>()?,
      trailing: Vec::new(),
    },
    _ => {
      return Err(Error::invalid(
        offset,
        format!(
          "unsupported OLEPS variant element type 0x{:04x}",
          property_type.0
        ),
      ));
    }
  };
  Ok(value)
}

fn validate_and_write_padding<W: Write>(writer: &mut Writer<W>, padding: &[u8]) -> Result<()> {
  let expected = writer.alignment_padding(4)?;
  if !padding.is_empty() && padding.len() != expected {
    return Err(Error::invalid(
      writer.position()?,
      "OLEPS string packet padding mismatch",
    ));
  }
  writer.write_all(padding)?;
  Ok(())
}

fn read_string_vector_bytes(
  bytes: &[u8],
  property_type: PropertyType,
) -> Result<(VectorValue, Vec<u8>)> {
  let aligned = parse_string_vector(bytes, property_type, true);
  aligned.or_else(|_| parse_string_vector(bytes, property_type, false))
}

fn parse_string_vector(
  bytes: &[u8],
  property_type: PropertyType,
  aligned: bool,
) -> Result<(VectorValue, Vec<u8>)> {
  let mut reader = Reader::new(Cursor::new(bytes))?;
  let count = usize::try_from(reader.read_u32()?)
    .map_err(|_| Error::Limit("OLEPS vector count does not fit usize".into()))?;
  reader.ensure_allocation(count, 4)?;
  if property_type == PropertyType::VECTOR_LPWSTR {
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
      let code_unit_count = usize::try_from(reader.read_u32()?)
        .map_err(|_| Error::Limit("OLEPS Unicode vector length does not fit usize".into()))?;
      reader.ensure_allocation(code_unit_count, 2)?;
      let mut code_units = Vec::with_capacity(code_unit_count);
      for _ in 0..code_unit_count {
        code_units.push(reader.read_u16()?);
      }
      values.push(UnicodeStringPacket {
        code_units,
        padding: if aligned {
          reader.read_alignment(4)?
        } else {
          Vec::new()
        },
      });
    }
    let padding = read_vector_outer_padding(&mut reader)?;
    Ok((VectorValue::UnicodeStrings(values), padding))
  } else {
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
      let len = usize::try_from(reader.read_u32()?)
        .map_err(|_| Error::Limit("OLEPS string vector length does not fit usize".into()))?;
      values.push(CodePageStringPacket {
        bytes: reader.read_vec(len)?,
        padding: if aligned {
          reader.read_alignment(4)?
        } else {
          Vec::new()
        },
      });
    }
    let padding = read_vector_outer_padding(&mut reader)?;
    Ok((VectorValue::CodePageStrings(values), padding))
  }
}

fn read_vector_outer_padding<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
) -> Result<Vec<u8>> {
  let remaining = reader.remaining()?;
  if remaining > 3 {
    return Err(Error::invalid(
      reader.position()?,
      "OLEPS string vector has unexpected trailing bytes",
    ));
  }
  read_remaining(reader)
}

fn read_fixed_vector<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  property_type: PropertyType,
) -> Result<VectorValue> {
  let count = usize::try_from(reader.read_u32()?)
    .map_err(|_| Error::Limit("OLEPS vector count does not fit usize".into()))?;
  macro_rules! read_values {
    ($size:expr, $method:ident, $variant:ident) => {{
      reader.ensure_allocation(count, $size)?;
      let mut values = Vec::with_capacity(count);
      for _ in 0..count {
        values.push(reader.$method()?);
      }
      VectorValue::$variant(values)
    }};
  }
  Ok(match property_type {
    PropertyType::VECTOR_I1 => read_values!(1, read_i8, I8),
    PropertyType::VECTOR_UI1 => read_values!(1, read_u8, U8),
    PropertyType::VECTOR_I2 => read_values!(2, read_i16, I16),
    PropertyType::VECTOR_UI2 => read_values!(2, read_u16, U16),
    PropertyType::VECTOR_I4 => read_values!(4, read_i32, I32),
    PropertyType::VECTOR_UI4 | PropertyType::VECTOR_ERROR => {
      read_values!(4, read_u32, U32)
    }
    PropertyType::VECTOR_R4 => read_values!(4, read_u32, F32Bits),
    PropertyType::VECTOR_I8 | PropertyType::VECTOR_CY => read_values!(8, read_i64, I64),
    PropertyType::VECTOR_UI8 => read_values!(8, read_u64, U64),
    PropertyType::VECTOR_R8 | PropertyType::VECTOR_DATE => {
      read_values!(8, read_u64, F64Bits)
    }
    PropertyType::VECTOR_BOOL => read_values!(2, read_i16, Bool),
    PropertyType::VECTOR_FILETIME => read_values!(8, read_u64, Filetime),
    PropertyType::VECTOR_CLSID => {
      reader.ensure_allocation(count, 16)?;
      let mut values = Vec::with_capacity(count);
      for _ in 0..count {
        values.push(reader.read_array::<16>()?);
      }
      VectorValue::Clsid(values)
    }
    _ => return Err(Error::invalid(0, "unsupported fixed OLEPS vector type")),
  })
}

fn read_clipboard_vector<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
) -> Result<VectorValue> {
  let count = usize::try_from(reader.read_u32()?)
    .map_err(|_| Error::Limit("OLEPS clipboard vector count does not fit usize".into()))?;
  reader.ensure_allocation(count, 8)?;
  let mut values = Vec::with_capacity(count);
  for _ in 0..count {
    let offset = reader.position()?;
    let size = usize::try_from(reader.read_u32()?)
      .map_err(|_| Error::Limit("OLEPS clipboard vector element size does not fit usize".into()))?;
    if size < 4 {
      return Err(Error::invalid(
        offset,
        "OLEPS clipboard vector element size is smaller than Format",
      ));
    }
    values.push(ClipboardDataPacket {
      format: reader.read_u32()?,
      data: reader.read_vec(size - 4)?,
      padding: reader.read_alignment(4)?,
    });
  }
  Ok(VectorValue::ClipboardData(values))
}

fn read_array_values<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  property_type: PropertyType,
  count: usize,
) -> Result<ArrayValue> {
  macro_rules! read_values {
    ($size:expr, $method:ident, $variant:ident) => {{
      reader.ensure_allocation(count, $size)?;
      let mut values = Vec::with_capacity(count);
      for _ in 0..count {
        values.push(reader.$method()?);
      }
      ArrayValue::$variant(values)
    }};
  }
  Ok(match property_type {
    PropertyType::ARRAY_I1 => read_values!(1, read_i8, I8),
    PropertyType::ARRAY_UI1 => read_values!(1, read_u8, U8),
    PropertyType::ARRAY_I2 => read_values!(2, read_i16, I16),
    PropertyType::ARRAY_UI2 => read_values!(2, read_u16, U16),
    PropertyType::ARRAY_I4 | PropertyType::ARRAY_INT => read_values!(4, read_i32, I32),
    PropertyType::ARRAY_UI4 | PropertyType::ARRAY_UINT | PropertyType::ARRAY_ERROR => {
      read_values!(4, read_u32, U32)
    }
    PropertyType::ARRAY_R4 => read_values!(4, read_u32, F32Bits),
    PropertyType::ARRAY_CY => read_values!(8, read_i64, I64),
    PropertyType::ARRAY_R8 | PropertyType::ARRAY_DATE => {
      read_values!(8, read_u64, F64Bits)
    }
    PropertyType::ARRAY_BOOL => read_values!(2, read_i16, Bool),
    PropertyType::ARRAY_DECIMAL => {
      reader.ensure_allocation(count, 16)?;
      let mut values = Vec::with_capacity(count);
      for _ in 0..count {
        values.push(Decimal::read_from(reader)?);
      }
      ArrayValue::Decimal(values)
    }
    PropertyType::ARRAY_BSTR => {
      reader.ensure_allocation(count, 4)?;
      let mut values = Vec::with_capacity(count);
      for _ in 0..count {
        let len = usize::try_from(reader.read_u32()?).map_err(|_| {
          Error::Limit("OLEPS string array element length does not fit usize".into())
        })?;
        values.push(CodePageStringPacket {
          bytes: reader.read_vec(len)?,
          padding: reader.read_alignment(4)?,
        });
      }
      ArrayValue::CodePageStrings(values)
    }
    PropertyType::ARRAY_VARIANT => {
      reader.ensure_allocation(count, 4)?;
      let mut values = Vec::with_capacity(count);
      for _ in 0..count {
        values.push(read_variant_scalar(reader, true, VariantContainer::Array)?);
      }
      ArrayValue::Variants(values)
    }
    _ => return Err(Error::invalid(0, "unsupported OLEPS array type")),
  })
}

fn write_type<W: Write>(
  writer: &mut Writer<W>,
  property_type: PropertyType,
  reserved: u16,
) -> Result<()> {
  writer.write_u16(property_type.0)?;
  writer.write_u16(reserved)
}

fn read_remaining<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Vec<u8>> {
  let remaining = usize::try_from(reader.remaining()?)
    .map_err(|_| Error::Limit("OLEPS remaining length does not fit usize".into()))?;
  reader.read_vec(remaining)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertySet {
  pub format_identifier: [u8; 16],
  pub properties: Vec<Property>,
  pub prefix_padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertySetStream {
  pub version: u16,
  pub system_identifier: u32,
  pub clsid: [u8; 16],
  pub property_sets: Vec<PropertySet>,
  pub trailing_padding: Vec<u8>,
}

impl PropertySetStream {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_bytes_with_limits(bytes, Limits::default())
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    if bytes.len() as u64 > limits.max_stream_size {
      return Err(Error::Limit(format!(
        "property set stream length {} exceeds {}",
        bytes.len(),
        limits.max_stream_size
      )));
    }
    let context = IoContext {
      format: BinaryFormat::PropertySet,
      limits,
      ..IoContext::default()
    };
    let mut reader = Reader::with_context(Cursor::new(bytes), context)?;
    let byte_order = reader.read_u16()?;
    if byte_order != PROPERTY_SET_BYTE_ORDER {
      return Err(Error::invalid(0, "OLEPS byte order must be 0xfffe"));
    }
    let version = reader.read_u16()?;
    if version > 1 {
      return Err(Error::invalid(2, "OLEPS version must be 0 or 1"));
    }
    let system_identifier = reader.read_u32()?;
    let clsid = reader.read_array::<16>()?;
    let count = reader.read_u32()?;
    if !matches!(count, 1 | 2) {
      return Err(Error::invalid(
        24,
        "OLEPS property set count must be 1 or 2",
      ));
    }
    reader.ensure_allocation(count as usize, 20)?;
    let mut descriptors = Vec::with_capacity(count as usize);
    for _ in 0..count {
      descriptors.push((reader.read_array::<16>()?, reader.read_u32()?));
    }
    let header_end = reader.position()? as usize;
    let mut property_sets = Vec::with_capacity(count as usize);
    let mut final_end = header_end;
    for (index, (format_identifier, offset)) in descriptors.iter().enumerate() {
      let start = *offset as usize;
      let bound = descriptors
        .get(index + 1)
        .map_or(bytes.len(), |(_, next)| *next as usize);
      if start < header_end || start > bound || bound > bytes.len() {
        return Err(Error::invalid(
          *offset as u64,
          "invalid OLEPS property set offset",
        ));
      }
      let property_set = parse_property_set(
        bytes
          .get(start..bound)
          .ok_or_else(|| Error::invalid(*offset as u64, "property set is out of bounds"))?,
        *format_identifier,
        limits,
      )?;
      let encoded_len = property_set.encoded_len()?;
      final_end = start
        .checked_add(encoded_len)
        .ok_or_else(|| Error::invalid(*offset as u64, "property set end overflow"))?;
      if final_end > bound {
        return Err(Error::invalid(
          *offset as u64,
          "property set exceeds its bound",
        ));
      }
      property_sets.push(property_set);
    }
    let trailing_padding = bytes.get(final_end..).unwrap_or_default().to_vec();
    Ok(Self {
      version,
      system_identifier,
      clsid,
      property_sets,
      trailing_padding,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if !matches!(self.property_sets.len(), 1 | 2) {
      return Err(Error::invalid(
        24,
        "OLEPS property set count must be 1 or 2",
      ));
    }
    let header_len = 28usize
      .checked_add(self.property_sets.len() * 20)
      .ok_or_else(|| Error::Limit("OLEPS header length overflow".into()))?;
    let sections: Vec<_> = self
      .property_sets
      .iter()
      .map(PropertySet::to_bytes)
      .collect::<Result<_>>()?;
    let mut offsets = Vec::with_capacity(sections.len());
    let mut next = header_len;
    for section in &sections {
      offsets
        .push(u32::try_from(next).map_err(|_| Error::Limit("OLEPS offset exceeds u32".into()))?);
      next = next
        .checked_add(section.len())
        .ok_or_else(|| Error::Limit("OLEPS stream length overflow".into()))?;
    }
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer.write_u16(PROPERTY_SET_BYTE_ORDER)?;
    writer.write_u16(self.version)?;
    writer.write_u32(self.system_identifier)?;
    writer.write_all(&self.clsid)?;
    writer.write_u32(self.property_sets.len() as u32)?;
    for ((property_set, offset), _) in self.property_sets.iter().zip(offsets).zip(&sections) {
      writer.write_all(&property_set.format_identifier)?;
      writer.write_u32(offset)?;
    }
    let mut bytes = writer.into_inner().into_inner();
    for section in sections {
      bytes.extend_from_slice(&section);
    }
    bytes.extend_from_slice(&self.trailing_padding);
    Ok(bytes)
  }
}

impl PropertySet {
  pub fn code_page(&self) -> Result<Option<u16>> {
    let Some(property) = self
      .properties
      .iter()
      .find(|property| property.identifier == 1)
    else {
      return Ok(None);
    };
    match property.typed_value()? {
      TypedPropertyValue::I16 { value, .. } => Ok(Some(value as u16)),
      _ => Err(Error::invalid(
        property.offset as u64,
        "OLEPS CodePage property must have type VT_I2",
      )),
    }
  }

  fn encoded_len(&self) -> Result<usize> {
    Ok(self.to_bytes()?.len())
  }

  fn to_bytes(&self) -> Result<Vec<u8>> {
    let table_end = 8usize
      .checked_add(self.properties.len() * 8)
      .ok_or_else(|| Error::Limit("OLEPS property table length overflow".into()))?;
    let first_property = table_end
      .checked_add(self.prefix_padding.len())
      .ok_or_else(|| Error::Limit("OLEPS property offset overflow".into()))?;
    let mut offsets = Vec::with_capacity(self.properties.len());
    let mut next = first_property;
    for property in &self.properties {
      offsets
        .push(u32::try_from(next).map_err(|_| Error::Limit("property offset exceeds u32".into()))?);
      next = next
        .checked_add(property.raw.len())
        .ok_or_else(|| Error::Limit("property set size overflow".into()))?;
    }
    let size =
      u32::try_from(next).map_err(|_| Error::Limit("property set size exceeds u32".into()))?;
    let header = PropertySetHeader {
      size,
      property_count: self.properties.len() as u32,
      properties: self
        .properties
        .iter()
        .zip(offsets)
        .map(|(property, offset)| PropertyIdentifierAndOffset {
          property_identifier: property.identifier,
          offset,
        })
        .collect(),
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    header.write_to(&mut writer)?;
    let mut bytes = writer.into_inner().into_inner();
    bytes.extend_from_slice(&self.prefix_padding);
    for property in &self.properties {
      bytes.extend_from_slice(&property.raw);
    }
    Ok(bytes)
  }
}

fn parse_property_set(
  bytes: &[u8],
  format_identifier: [u8; 16],
  limits: Limits,
) -> Result<PropertySet> {
  let context = IoContext {
    format: BinaryFormat::PropertySet,
    limits,
    ..IoContext::default()
  };
  let mut reader = Reader::with_context(Cursor::new(bytes), context)?;
  let header = PropertySetHeader::read_from(&mut reader)?;
  let size = header.size as usize;
  if size > bytes.len() || size < 8 + header.properties.len() * 8 {
    return Err(Error::invalid(0, "invalid OLEPS property set size"));
  }
  let table_end = reader.position()? as usize;
  let table = header.properties;
  // [MS-OLEPS] 2.21 PropertySet requires the
  // PropertyIdentifierAndOffset sequence to be ordered by increasing Offset.
  if table
    .windows(2)
    .any(|pair| pair[0].offset >= pair[1].offset)
  {
    return Err(Error::invalid(
      8,
      "OLEPS property table offsets are not strictly increasing",
    ));
  }
  let mut properties = Vec::with_capacity(table.len());
  for (index, item) in table.iter().enumerate() {
    let start = item.offset as usize;
    let end = table
      .get(index + 1)
      .map_or(size, |next| next.offset as usize);
    if start < table_end || start >= end || end > size {
      return Err(Error::invalid(
        item.offset as u64,
        format!(
          "invalid OLEPS property offset: id=0x{:08x}, start={start}, end={end}, size={size}, table_end={table_end}",
          item.property_identifier
        ),
      ));
    }
    properties.push(Property {
      identifier: item.property_identifier,
      offset: item.offset,
      raw: bytes[start..end].to_vec(),
    });
  }
  let first = table.first().map_or(size, |item| item.offset as usize);
  let prefix_padding = bytes[table_end..first].to_vec();
  Ok(PropertySet {
    format_identifier,
    properties,
    prefix_padding,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn one_section_property_stream_round_trips() {
    let fmtid = [0x2a; 16];
    let value = PropertySetStream {
      version: 0,
      system_identifier: 0x0002_0005,
      clsid: [0; 16],
      property_sets: vec![PropertySet {
        format_identifier: fmtid,
        properties: vec![Property {
          identifier: 1,
          offset: 16,
          raw: vec![0x02, 0, 0, 0, 0xe4, 0x04, 0, 0],
        }],
        prefix_padding: Vec::new(),
      }],
      trailing_padding: vec![0; 4],
    };
    let bytes = value.to_bytes().unwrap();
    let parsed = PropertySetStream::from_bytes(&bytes).unwrap();
    assert_eq!(parsed, value);
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
    let typed = parsed.property_sets[0].properties[0].typed_value().unwrap();
    assert_eq!(
      typed,
      TypedPropertyValue::I16 {
        reserved: 0,
        value: 1252,
        padding: vec![0, 0],
      }
    );
    assert_eq!(
      typed.to_bytes().unwrap(),
      parsed.property_sets[0].properties[0].raw
    );
  }

  #[test]
  fn out_of_order_property_offsets_are_rejected() {
    let value = PropertySetStream {
      version: 0,
      system_identifier: 0x0002_0005,
      clsid: [0; 16],
      property_sets: vec![PropertySet {
        format_identifier: [0x2a; 16],
        properties: vec![
          Property {
            identifier: 1,
            offset: 24,
            raw: vec![0x02, 0, 0, 0, 0xe4, 0x04, 0, 0],
          },
          Property {
            identifier: 2,
            offset: 32,
            raw: vec![0x03, 0, 0, 0, 7, 0, 0, 0],
          },
        ],
        prefix_padding: Vec::new(),
      }],
      trailing_padding: Vec::new(),
    };
    let mut bytes = value.to_bytes().unwrap();
    // The one-set stream header is 48 bytes and the PropertySet header is 8;
    // exchange its two 8-byte PropertyIdentifierAndOffset packets.
    bytes[56..72].rotate_left(8);
    let error = PropertySetStream::from_bytes(&bytes).unwrap_err();
    assert!(
      error
        .to_string()
        .contains("property table offsets are not strictly increasing")
    );
  }

  #[test]
  fn fixed_vector_property_round_trips() {
    let value = TypedPropertyValue::Vector {
      property_type: PropertyType::VECTOR_I2,
      reserved: 0,
      values: VectorValue::I16(vec![-2, 7, 300]),
      padding: vec![0, 0],
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(TypedPropertyValue::from_bytes(&bytes).unwrap(), value);
    assert_eq!(bytes.len(), 16);
  }

  #[test]
  fn clipboard_vector_property_round_trips() {
    let value = TypedPropertyValue::Vector {
      property_type: PropertyType::VECTOR_CF,
      reserved: 0,
      values: VectorValue::ClipboardData(vec![
        ClipboardDataPacket {
          format: 3,
          data: vec![1, 2, 3],
          padding: vec![0],
        },
        ClipboardDataPacket {
          format: 7,
          data: Vec::new(),
          padding: Vec::new(),
        },
      ]),
      padding: Vec::new(),
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(TypedPropertyValue::from_bytes(&bytes).unwrap(), value);
  }

  #[test]
  fn string_and_variant_array_properties_round_trip() {
    let strings = TypedPropertyValue::Array {
      property_type: PropertyType::ARRAY_BSTR,
      reserved: 0,
      dimensions: vec![
        ArrayDimension {
          size: 1,
          index_offset: 0,
        },
        ArrayDimension {
          size: 2,
          index_offset: 1,
        },
      ],
      values: ArrayValue::CodePageStrings(vec![
        CodePageStringPacket {
          bytes: b"A\0".to_vec(),
          padding: vec![0, 0],
        },
        CodePageStringPacket {
          bytes: b"BCD\0".to_vec(),
          padding: Vec::new(),
        },
      ]),
      padding: Vec::new(),
    };
    let bytes = strings.to_bytes().unwrap();
    assert_eq!(TypedPropertyValue::from_bytes(&bytes).unwrap(), strings);

    let variants = TypedPropertyValue::Array {
      property_type: PropertyType::ARRAY_VARIANT,
      reserved: 0,
      dimensions: vec![ArrayDimension {
        size: 3,
        index_offset: -2,
      }],
      values: ArrayValue::Variants(vec![
        TypedPropertyValue::I16 {
          reserved: 0,
          value: -7,
          padding: vec![0, 0],
        },
        TypedPropertyValue::Decimal {
          reserved: 0,
          value: Decimal {
            reserved: 0,
            scale: 2,
            sign: 0x80,
            high: 1,
            low: 2,
          },
          trailing: Vec::new(),
        },
        TypedPropertyValue::CodePageString {
          property_type: PropertyType::BSTR,
          reserved: 0,
          bytes: b"Z\0".to_vec(),
          padding: vec![0, 0],
        },
      ]),
      padding: Vec::new(),
    };
    let bytes = variants.to_bytes().unwrap();
    assert_eq!(TypedPropertyValue::from_bytes(&bytes).unwrap(), variants);

    let invalid = TypedPropertyValue::Array {
      property_type: PropertyType::ARRAY_VARIANT,
      reserved: 0,
      dimensions: vec![ArrayDimension {
        size: 1,
        index_offset: 0,
      }],
      values: ArrayValue::Variants(vec![TypedPropertyValue::U64 {
        reserved: 0,
        value: 1,
        trailing: Vec::new(),
      }]),
      padding: Vec::new(),
    };
    assert!(invalid.to_bytes().is_err());
  }

  #[test]
  fn fixed_multidimensional_array_property_round_trips() {
    let value = TypedPropertyValue::Array {
      property_type: PropertyType::ARRAY_I2,
      reserved: 0,
      dimensions: vec![
        ArrayDimension {
          size: 2,
          index_offset: -1,
        },
        ArrayDimension {
          size: 3,
          index_offset: 1,
        },
      ],
      values: ArrayValue::I16(vec![-2, 7, 300, 9, 10, 11]),
      padding: Vec::new(),
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(bytes.len(), 40);
    assert_eq!(&bytes[4..8], &u32::from(PropertyType::I2.0).to_le_bytes());
    assert_eq!(TypedPropertyValue::from_bytes(&bytes).unwrap(), value);

    let wrong_count = TypedPropertyValue::Array {
      property_type: PropertyType::ARRAY_I2,
      reserved: 0,
      dimensions: vec![ArrayDimension {
        size: 2,
        index_offset: 0,
      }],
      values: ArrayValue::I16(vec![1]),
      padding: Vec::new(),
    };
    assert!(wrong_count.to_bytes().is_err());

    let mut wrong_header = bytes;
    wrong_header[4] = PropertyType::UI2.0 as u8;
    assert!(TypedPropertyValue::from_bytes(&wrong_header).is_err());
  }

  #[test]
  fn every_fixed_array_scalar_type_round_trips() {
    let cases = [
      (PropertyType::ARRAY_I1, ArrayValue::I8(vec![-1, 2])),
      (PropertyType::ARRAY_UI1, ArrayValue::U8(vec![1, 2])),
      (PropertyType::ARRAY_I2, ArrayValue::I16(vec![-1, 2])),
      (PropertyType::ARRAY_UI2, ArrayValue::U16(vec![1, 2])),
      (PropertyType::ARRAY_I4, ArrayValue::I32(vec![-1, 2])),
      (PropertyType::ARRAY_INT, ArrayValue::I32(vec![-1, 2])),
      (PropertyType::ARRAY_UI4, ArrayValue::U32(vec![1, 2])),
      (PropertyType::ARRAY_UINT, ArrayValue::U32(vec![1, 2])),
      (PropertyType::ARRAY_ERROR, ArrayValue::U32(vec![1, 2])),
      (
        PropertyType::ARRAY_R4,
        ArrayValue::F32Bits(vec![1.0f32.to_bits(), f32::NAN.to_bits()]),
      ),
      (PropertyType::ARRAY_CY, ArrayValue::I64(vec![-1, 2])),
      (
        PropertyType::ARRAY_R8,
        ArrayValue::F64Bits(vec![1.0f64.to_bits(), f64::NAN.to_bits()]),
      ),
      (
        PropertyType::ARRAY_DATE,
        ArrayValue::F64Bits(vec![1.0f64.to_bits(), 2.0f64.to_bits()]),
      ),
      (PropertyType::ARRAY_BOOL, ArrayValue::Bool(vec![-1, 0])),
      (
        PropertyType::ARRAY_DECIMAL,
        ArrayValue::Decimal(vec![
          Decimal {
            reserved: 0,
            scale: 2,
            sign: 0,
            high: 1,
            low: 2,
          },
          Decimal {
            reserved: 0,
            scale: 4,
            sign: 0x80,
            high: 3,
            low: 4,
          },
        ]),
      ),
    ];
    for (property_type, values) in cases {
      let padding = if matches!(
        property_type,
        PropertyType::ARRAY_I1 | PropertyType::ARRAY_UI1
      ) {
        vec![0, 0]
      } else {
        Vec::new()
      };
      let value = TypedPropertyValue::Array {
        property_type,
        reserved: 0,
        dimensions: vec![ArrayDimension {
          size: 2,
          index_offset: 0,
        }],
        values,
        padding,
      };
      let bytes = value.to_bytes().unwrap();
      assert_eq!(TypedPropertyValue::from_bytes(&bytes).unwrap(), value);
    }
  }

  #[test]
  fn indirect_and_object_property_types_round_trip() {
    let values = [
      TypedPropertyValue::Null {
        reserved: 0,
        trailing: Vec::new(),
      },
      TypedPropertyValue::Decimal {
        reserved: 0,
        value: Decimal {
          reserved: 0,
          scale: 2,
          sign: 0x80,
          high: 0x1234,
          low: 0x5678,
        },
        trailing: Vec::new(),
      },
      TypedPropertyValue::Blob {
        property_type: PropertyType::BLOB_OBJECT,
        reserved: 0,
        bytes: vec![1, 2, 3],
        padding: vec![0],
      },
      TypedPropertyValue::IndirectPropertyName {
        property_type: PropertyType::STREAMED_OBJECT,
        reserved: 0,
        bytes: b"prop42\0".to_vec(),
        padding: vec![0],
      },
      TypedPropertyValue::VersionedStream {
        reserved: 0,
        version_guid: [0x5a; 16],
        stream_name: b"prop99\0".to_vec(),
        padding: vec![0],
      },
    ];
    for value in values {
      let bytes = value.to_bytes().unwrap();
      assert_eq!(TypedPropertyValue::from_bytes(&bytes).unwrap(), value);
    }

    let wrong_type = TypedPropertyValue::IndirectPropertyName {
      property_type: PropertyType::LPSTR,
      reserved: 0,
      bytes: b"prop1\0".to_vec(),
      padding: vec![0],
    };
    assert!(wrong_type.to_bytes().is_err());
  }

  #[test]
  fn dictionary_round_trips_in_mbcs_and_unicode_forms() {
    for (code_page, name) in [
      (1252, DictionaryName::Mbcs(b"Name\0".to_vec())),
      (
        1200,
        DictionaryName::Unicode {
          code_units: "Name\0".encode_utf16().collect(),
          padding: vec![0, 0],
        },
      ),
    ] {
      let value = Dictionary {
        entries: vec![DictionaryEntry {
          property_identifier: 2,
          name,
        }],
        padding: if code_page == 1252 {
          vec![0, 0, 0]
        } else {
          Vec::new()
        },
      };
      let bytes = value.to_bytes().unwrap();
      assert_eq!(Dictionary::from_bytes(&bytes, code_page).unwrap(), value);
    }
  }

  #[test]
  fn scalar_string_value_decodes_without_retaining_wire_terminators() {
    let code_page = Property {
      identifier: 2,
      offset: 32,
      raw: TypedPropertyValue::CodePageString {
        property_type: PropertyType::LPSTR,
        reserved: 0,
        bytes: b"caf\xe9\0".to_vec(),
        padding: vec![0, 0],
      }
      .to_bytes()
      .unwrap(),
    };
    assert_eq!(
      code_page.string_value(Some(1252)).unwrap().as_deref(),
      Some("caf\u{e9}")
    );

    let unicode = Property {
      identifier: 3,
      offset: 64,
      raw: TypedPropertyValue::UnicodeString {
        reserved: 0,
        code_units: "\u{6587}\u{6863}\0".encode_utf16().collect(),
        padding: vec![0, 0],
      }
      .to_bytes()
      .unwrap(),
    };
    assert_eq!(
      unicode.string_value(None).unwrap().as_deref(),
      Some("\u{6587}\u{6863}")
    );

    let scalar = Property {
      identifier: 4,
      offset: 96,
      raw: TypedPropertyValue::I32 {
        property_type: PropertyType::I4,
        reserved: 0,
        value: 7,
        trailing: Vec::new(),
      }
      .to_bytes()
      .unwrap(),
    };
    assert_eq!(scalar.string_value(None).unwrap(), None);
  }
}
