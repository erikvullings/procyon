//! Structures shared by the legacy Office binary formats and specified by MS-OSHARED.

use std::io::{Cursor, Read, Seek, Write};

use bitflags::bitflags;

use crate::{
  Error, Result, SdkObject,
  common::FileTime,
  io::{Reader, SdkRead, SdkSize, SdkWrite, Writer},
};

pub const MSO_ENVELOPE_CLSID: [u8; 16] = [
  0x1a, 0xf0, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// MS-OSHARED 2.3.4.5 PBString character storage, including its terminating NUL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PbStringCharacters {
  Ansi(Vec<u8>),
  Unicode(Vec<u16>),
}

/// MS-OSHARED 2.3.4.5 PBString.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PbString {
  /// Includes the terminating NUL counted by `cch`.
  pub characters: PbStringCharacters,
}

/// MS-OSHARED 2.3.4.2 FactoidType.
#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(size_prefix = "u32", validate_at = "validate_factoid_type")]
pub struct FactoidType {
  pub id: u32,
  pub uri: PbString,
  pub tag: PbString,
  pub download_url: PbString,
}

/// MS-OSHARED 2.3.4.1 PropertyBagStore.
#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate_at = "validate_property_bag_store")]
pub struct PropertyBagStore {
  #[sdk(count_prefix = "u32", min_element_size = 17)]
  pub factoid_types: Vec<FactoidType>,
  /// `cbHdr`; MS-OSHARED requires 0x000C.
  pub cb_hdr: u16,
  /// `sVer`; MS-OSHARED requires 0x0100.
  pub version: u16,
  /// `cfactoid`; reserved for future use and ignored by readers.
  pub reserved_factoid_count: u32,
  #[sdk(count_prefix = "u32", min_element_size = 3)]
  pub string_table: Vec<PbString>,
}

impl SdkRead for PbString {
  fn read_from<R: Read + Seek>(reader: &mut Reader<R>) -> Result<Self> {
    let offset = reader.position()?;
    let header = reader.read_u16()?;
    let count = usize::from(header & 0x7fff);
    let characters = if header & 0x8000 != 0 {
      PbStringCharacters::Ansi(reader.read_vec(count)?)
    } else {
      let byte_count = count
        .checked_mul(2)
        .ok_or_else(|| Error::Limit("PBString byte count overflow".into()))?;
      let bytes = reader.read_vec(byte_count)?;
      PbStringCharacters::Unicode(
        bytes
          .chunks_exact(2)
          .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
          .collect(),
      )
    };
    let value = Self { characters };
    validate_pb_string(&value, offset)?;
    Ok(value)
  }
}

impl SdkWrite for PbString {
  fn write_to<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_pb_string(self, writer.position()?)?;
    match &self.characters {
      PbStringCharacters::Ansi(values) => {
        let count = u16::try_from(values.len())
          .map_err(|_| Error::Limit("ANSI PBString exceeds u16".into()))?;
        if count > 0x7fff {
          return Err(Error::Limit("ANSI PBString exceeds 15-bit count".into()));
        }
        writer.write_u16(count | 0x8000)?;
        writer.write_all(values)?;
      }
      PbStringCharacters::Unicode(values) => {
        let count = u16::try_from(values.len())
          .map_err(|_| Error::Limit("Unicode PBString exceeds u16".into()))?;
        if count > 0x7fff {
          return Err(Error::Limit("Unicode PBString exceeds 15-bit count".into()));
        }
        writer.write_u16(count)?;
        for value in values {
          writer.write_u16(*value)?;
        }
      }
    }
    Ok(())
  }
}

impl SdkSize for PbString {
  fn sdk_size(&self) -> u64 {
    2 + match &self.characters {
      PbStringCharacters::Ansi(values) => values.len() as u64,
      PbStringCharacters::Unicode(values) => (values.len() as u64) * 2,
    }
  }
}

fn validate_pb_string(value: &PbString, offset: u64) -> Result<()> {
  let valid = match &value.characters {
    PbStringCharacters::Ansi(values) => values
      .split_last()
      .is_some_and(|(last, body)| *last == 0 && body.iter().all(|value| *value != 0)),
    PbStringCharacters::Unicode(values) => values
      .split_last()
      .is_some_and(|(last, body)| *last == 0 && body.iter().all(|value| *value != 0)),
  };
  if !valid {
    return Err(Error::invalid(
      offset,
      "PBString must contain exactly one terminating NUL character",
    ));
  }
  Ok(())
}

fn validate_factoid_type(value: &FactoidType, offset: u64) -> Result<()> {
  if value.id > u32::from(u16::MAX) {
    return Err(Error::invalid(
      offset,
      "FactoidType id exceeds the MS-OSHARED 16-bit range",
    ));
  }
  Ok(())
}

fn validate_property_bag_store(value: &PropertyBagStore, offset: u64) -> Result<()> {
  if value.cb_hdr != 0x000c || value.version != 0x0100 {
    return Err(Error::invalid(
      offset,
      "PropertyBagStore cbHdr or sVer violates MS-OSHARED",
    ));
  }
  Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NumberingFormat(u8);

impl NumberingFormat {
  pub const ARABIC: Self = Self(0x00);
  pub const UPPER_ROMAN: Self = Self(0x01);
  pub const LOWER_ROMAN: Self = Self(0x02);
  pub const UPPER_LETTER: Self = Self(0x03);
  pub const LOWER_LETTER: Self = Self(0x04);
  pub const BULLET: Self = Self(0x17);
  pub const NONE: Self = Self(0xff);

  pub fn from_code(code: u8) -> Result<Self> {
    if code <= 0x3b || code == 0xff {
      Ok(Self(code))
    } else {
      Err(Error::invalid(0, "MSONFC value is undefined"))
    }
  }

  pub fn from_u16(code: u16) -> Result<Self> {
    Self::from_code(u8::try_from(code).map_err(|_| Error::invalid(0, "MSONFC exceeds 8 bits"))?)
  }

  pub const fn code(self) -> u8 {
    self.0
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MsoEnvelopeClsid {
  pub clsid: [u8; 16],
  pub data: MsoEnvelopeData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MsoEnvelopeData {
  Envelope(Box<MsoEnvelope>),
  OutOfScope(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MsoEnvelope {
  pub version: MsoEnvelopeVersion,
  pub last_sent_time: EnvelopeMinuteTime,
  pub flag_status: EnvelopeFlagStatus,
  pub reply_time: EnvelopeMinuteTime,
  pub request: EnvelopeString,
  pub sent_representing_entry_id: Vec<u8>,
  pub sent_representing_name: EnvelopeString,
  pub internet_account_stamp: EnvelopeString,
  pub internet_account_name: EnvelopeString,
  pub expiry_time: EnvelopeMinuteTime,
  pub deferred_delivery_time: EnvelopeMinuteTime,
  pub delete_after_submit: bool,
  pub security: EnvelopeSecurityFlags,
  pub originator_delivery_report_requested: bool,
  pub read_receipt_requested: bool,
  pub categories: EnvelopeString,
  pub sensitivity: EnvelopeSensitivity,
  pub importance: EnvelopeImportance,
  pub subject: EnvelopeString,
  pub voting_options: Vec<u8>,
  pub reply_recipients: EnvelopeRecipientCollection,
  pub contact_link_recipients: Option<EnvelopeRecipientCollection>,
  pub recipients: EnvelopeRecipientCollection,
  pub attachments: Vec<EnvelopeAttachment>,
  pub intro_text: Option<Vec<u16>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MsoEnvelopeVersion {
  Ansi6,
  Unicode8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvelopeMinuteTime(pub i32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EnvelopeFlagStatus {
  NotFlagged,
  Flagged,
  Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EnvelopeSensitivity {
  Normal,
  Personal,
  Private,
  Confidential,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EnvelopeImportance {
  Low,
  Normal,
  High,
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct EnvelopeSecurityFlags: u32 {
        const SIGNED = 0x0000_0001;
        const ENCRYPTED = 0x0000_0002;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvelopeString {
  Ansi(Vec<u8>),
  Unicode(Vec<u16>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeRecipientCollection {
  pub recipients: Vec<EnvelopeRecipient>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeRecipient {
  pub ignored: u32,
  pub properties: Vec<EnvelopeRecipientProperty>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeRecipientProperty {
  pub property_id: u16,
  pub value: EnvelopeRecipientPropertyValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvelopeRecipientPropertyValue {
  Long(u32),
  Null(u32),
  Boolean(u16),
  SystemTime(FileTime),
  Error(u32),
  String8(Vec<u8>),
  Unicode(Vec<u16>),
  Binary(Vec<u8>),
  MultiString8(Vec<Vec<u8>>),
  MultiBinary(Vec<Vec<u8>>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeAttachment {
  pub method: u32,
  pub name: Vec<u16>,
  pub data: Vec<u8>,
}

impl MsoEnvelopeClsid {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 16 {
      return Err(Error::invalid(
        0,
        "MsoEnvelopeCLSID is shorter than its CLSID",
      ));
    }
    let clsid: [u8; 16] = bytes[..16]
      .try_into()
      .expect("the 16-byte CLSID prefix was checked");
    let data = if clsid == MSO_ENVELOPE_CLSID {
      MsoEnvelopeData::Envelope(Box::new(MsoEnvelope::from_bytes(&bytes[16..])?))
    } else {
      MsoEnvelopeData::OutOfScope(bytes[16..].to_vec())
    };
    Ok(Self { clsid, data })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    match (&self.clsid, &self.data) {
      (clsid, MsoEnvelopeData::Envelope(_)) if clsid != &MSO_ENVELOPE_CLSID => {
        return Err(Error::invalid(
          0,
          "typed MsoEnvelope has a non-envelope CLSID",
        ));
      }
      (clsid, MsoEnvelopeData::OutOfScope(_)) if clsid == &MSO_ENVELOPE_CLSID => {
        return Err(Error::invalid(
          0,
          "standard MsoEnvelope CLSID has out-of-scope data",
        ));
      }
      _ => {}
    }
    let mut bytes = self.clsid.to_vec();
    match &self.data {
      MsoEnvelopeData::Envelope(value) => bytes.extend_from_slice(&value.to_bytes()?),
      MsoEnvelopeData::OutOfScope(value) => bytes.extend_from_slice(value),
    }
    Ok(bytes)
  }
}

impl MsoEnvelope {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(bytes))?;
    let version = MsoEnvelopeVersion::from_u32(reader.read_u32()?)?;
    let value = Self {
      version,
      last_sent_time: EnvelopeMinuteTime::read(&mut reader)?,
      flag_status: EnvelopeFlagStatus::from_u32(reader.read_u32()?)?,
      reply_time: EnvelopeMinuteTime::read(&mut reader)?,
      request: EnvelopeString::read(&mut reader, version)?,
      sent_representing_entry_id: read_u32_bytes(&mut reader, "entry id")?,
      sent_representing_name: EnvelopeString::read(&mut reader, version)?,
      internet_account_stamp: EnvelopeString::read(&mut reader, version)?,
      internet_account_name: EnvelopeString::read(&mut reader, version)?,
      expiry_time: EnvelopeMinuteTime::read(&mut reader)?,
      deferred_delivery_time: EnvelopeMinuteTime::read(&mut reader)?,
      delete_after_submit: read_bool32(&mut reader, "DeleteAfterSubmit")?,
      security: {
        let bits = reader.read_u32()?;
        let offset = reader.position()?.saturating_sub(4);
        EnvelopeSecurityFlags::from_bits(bits)
          .ok_or_else(|| Error::invalid(offset, "invalid SecurityFlags"))?
      },
      originator_delivery_report_requested: read_bool32(
        &mut reader,
        "OriginatorDeliveryReportRequested",
      )?,
      read_receipt_requested: read_bool32(&mut reader, "ReadReceiptRequested")?,
      categories: EnvelopeString::read(&mut reader, version)?,
      sensitivity: EnvelopeSensitivity::from_u32(reader.read_u32()?)?,
      importance: EnvelopeImportance::from_u32(reader.read_u32()?)?,
      subject: EnvelopeString::read(&mut reader, version)?,
      voting_options: read_u16_bytes(&mut reader, "VotingOptions")?,
      reply_recipients: EnvelopeRecipientCollection::read(&mut reader)?,
      contact_link_recipients: if version == MsoEnvelopeVersion::Unicode8 {
        Some(EnvelopeRecipientCollection::read(&mut reader)?)
      } else {
        None
      },
      recipients: EnvelopeRecipientCollection::read(&mut reader)?,
      attachments: EnvelopeAttachment::read_collection(&mut reader)?,
      intro_text: if version == MsoEnvelopeVersion::Unicode8 {
        Some(read_u32_utf16_bytes(&mut reader, "IntroText")?)
      } else {
        None
      },
    };
    if reader.remaining()? != 0 {
      return Err(Error::invalid(
        reader.position()?,
        "MsoEnvelope has trailing bytes",
      ));
    }
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if (self.version == MsoEnvelopeVersion::Unicode8) != self.contact_link_recipients.is_some()
      || (self.version == MsoEnvelopeVersion::Unicode8) != self.intro_text.is_some()
    {
      return Err(Error::invalid(
        0,
        "version-dependent MsoEnvelope fields changed",
      ));
    }
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer.write_u32(self.version.to_u32())?;
    self.last_sent_time.write(&mut writer)?;
    writer.write_u32(self.flag_status.to_u32())?;
    self.reply_time.write(&mut writer)?;
    self.request.write(&mut writer, self.version)?;
    write_u32_bytes(&mut writer, &self.sent_representing_entry_id, "entry id")?;
    self
      .sent_representing_name
      .write(&mut writer, self.version)?;
    self
      .internet_account_stamp
      .write(&mut writer, self.version)?;
    self
      .internet_account_name
      .write(&mut writer, self.version)?;
    self.expiry_time.write(&mut writer)?;
    self.deferred_delivery_time.write(&mut writer)?;
    writer.write_u32(u32::from(self.delete_after_submit))?;
    writer.write_u32(self.security.bits())?;
    writer.write_u32(u32::from(self.originator_delivery_report_requested))?;
    writer.write_u32(u32::from(self.read_receipt_requested))?;
    self.categories.write(&mut writer, self.version)?;
    writer.write_u32(self.sensitivity.to_u32())?;
    writer.write_u32(self.importance.to_u32())?;
    self.subject.write(&mut writer, self.version)?;
    write_u16_bytes(&mut writer, &self.voting_options, "VotingOptions")?;
    self.reply_recipients.write(&mut writer)?;
    if let Some(value) = &self.contact_link_recipients {
      value.write(&mut writer)?;
    }
    self.recipients.write(&mut writer)?;
    EnvelopeAttachment::write_collection(&mut writer, &self.attachments)?;
    if let Some(value) = &self.intro_text {
      write_u32_utf16_bytes(&mut writer, value, "IntroText")?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

impl MsoEnvelopeVersion {
  fn from_u32(value: u32) -> Result<Self> {
    match value {
      6 => Ok(Self::Ansi6),
      8 => Ok(Self::Unicode8),
      _ => Err(Error::invalid(
        0,
        format!("unknown MsoEnvelope version {value}"),
      )),
    }
  }

  fn to_u32(self) -> u32 {
    match self {
      Self::Ansi6 => 6,
      Self::Unicode8 => 8,
    }
  }
}

impl EnvelopeMinuteTime {
  pub const UNSPECIFIED: Self = Self(0x5ae9_80e0);

  fn read<R: Read + Seek>(reader: &mut Reader<R>) -> Result<Self> {
    let value = Self(reader.read_i32()?);
    value.validate()?;
    Ok(value)
  }

  fn write<W: Write>(self, writer: &mut Writer<W>) -> Result<()> {
    self.validate()?;
    writer.write_i32(self.0)
  }

  fn validate(self) -> Result<()> {
    if !(0..=Self::UNSPECIFIED.0).contains(&self.0) {
      return Err(Error::invalid(0, "MsoEnvelope minute time is out of range"));
    }
    Ok(())
  }
}

macro_rules! u32_enum {
    ($name:ident { $($variant:ident = $value:literal),+ $(,)? }) => {
        impl $name {
            fn from_u32(value: u32) -> Result<Self> {
                match value {
                    $($value => Ok(Self::$variant),)+
                    _ => Err(Error::invalid(0, format!("invalid {} value {value}", stringify!($name)))),
                }
            }

            fn to_u32(self) -> u32 {
                match self {
                    $(Self::$variant => $value,)+
                }
            }
        }
    };
}

u32_enum!(EnvelopeFlagStatus {
    NotFlagged = 0,
    Flagged = 1,
    Complete = 2,
});
u32_enum!(EnvelopeSensitivity {
    Normal = 0,
    Personal = 1,
    Private = 2,
    Confidential = 3,
});
u32_enum!(EnvelopeImportance {
    Low = 0,
    Normal = 1,
    High = 2,
});

impl EnvelopeString {
  fn read<R: Read + Seek>(reader: &mut Reader<R>, version: MsoEnvelopeVersion) -> Result<Self> {
    let count = usize::from(reader.read_u16()?);
    match version {
      MsoEnvelopeVersion::Ansi6 => Ok(Self::Ansi(reader.read_vec(count)?)),
      MsoEnvelopeVersion::Unicode8 => {
        reader.ensure_allocation(count, 2)?;
        let mut value = Vec::with_capacity(count);
        for _ in 0..count {
          value.push(reader.read_u16()?);
        }
        Ok(Self::Unicode(value))
      }
    }
  }

  fn write<W: Write>(&self, writer: &mut Writer<W>, version: MsoEnvelopeVersion) -> Result<()> {
    match (version, self) {
      (MsoEnvelopeVersion::Ansi6, Self::Ansi(value)) => {
        writer.write_u16(checked_u16(value.len(), "envelope ANSI string")?)?;
        Ok(writer.write_all(value)?)
      }
      (MsoEnvelopeVersion::Unicode8, Self::Unicode(value)) => {
        writer.write_u16(checked_u16(value.len(), "envelope Unicode string")?)?;
        for character in value {
          writer.write_u16(*character)?;
        }
        Ok(())
      }
      _ => Err(Error::invalid(0, "MsoEnvelope string encoding changed")),
    }
  }

  pub fn len(&self) -> usize {
    match self {
      Self::Ansi(value) => value.len(),
      Self::Unicode(value) => value.len(),
    }
  }

  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }
}

impl EnvelopeRecipientCollection {
  const TAG: u32 = 0xdcca_0123;
  const VERSION: u32 = 1;

  fn read<R: Read + Seek>(reader: &mut Reader<R>) -> Result<Self> {
    if reader.read_u32()? != Self::TAG || reader.read_u32()? != Self::VERSION {
      return Err(Error::invalid(
        reader.position()?.saturating_sub(8),
        "invalid EnvRecipientCollection header",
      ));
    }
    let raw_count = reader.read_u32()?;
    let count = checked_count(reader, raw_count, 8, "recipient")?;
    let mut recipients = Vec::with_capacity(count);
    for _ in 0..count {
      recipients.push(EnvelopeRecipient::read(reader)?);
    }
    Ok(Self { recipients })
  }

  fn write<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    writer.write_u32(Self::TAG)?;
    writer.write_u32(Self::VERSION)?;
    writer.write_u32(checked_u32(self.recipients.len(), "recipient count")?)?;
    for recipient in &self.recipients {
      recipient.write(writer)?;
    }
    Ok(())
  }
}

impl EnvelopeRecipient {
  fn read<R: Read + Seek>(reader: &mut Reader<R>) -> Result<Self> {
    let raw_count = reader.read_u32()?;
    let count = checked_count(reader, raw_count, 4, "recipient property")?;
    let ignored = reader.read_u32()?;
    let mut properties = Vec::with_capacity(count);
    for _ in 0..count {
      properties.push(EnvelopeRecipientProperty::read(reader)?);
    }
    Ok(Self {
      ignored,
      properties,
    })
  }

  fn write<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    writer.write_u32(checked_u32(
      self.properties.len(),
      "recipient property count",
    )?)?;
    writer.write_u32(self.ignored)?;
    for property in &self.properties {
      property.write(writer)?;
    }
    Ok(())
  }
}

impl EnvelopeRecipientProperty {
  fn read<R: Read + Seek>(reader: &mut Reader<R>) -> Result<Self> {
    let tag = reader.read_u32()?;
    let property_id = (tag >> 16) as u16;
    let value = EnvelopeRecipientPropertyValue::read(reader, tag as u16)?;
    Ok(Self { property_id, value })
  }

  fn write<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    writer.write_u32((u32::from(self.property_id) << 16) | u32::from(self.value.type_id()))?;
    self.value.write(writer)
  }
}

impl EnvelopeRecipientPropertyValue {
  fn read<R: Read + Seek>(reader: &mut Reader<R>, type_id: u16) -> Result<Self> {
    match type_id {
      0x0003 => Ok(Self::Long(reader.read_u32()?)),
      0x0001 => Ok(Self::Null(reader.read_u32()?)),
      0x000b => Ok(Self::Boolean(reader.read_u16()?)),
      0x0040 => {
        let high = reader.read_u32()?;
        let low = reader.read_u32()?;
        Ok(Self::SystemTime(FileTime::from_parts(low, high)))
      }
      0x000a => Ok(Self::Error(reader.read_u32()?)),
      0x001e => Ok(Self::String8(read_u16_bytes(reader, "PT_STRING8")?)),
      0x001f => Ok(Self::Unicode(read_u16_utf16_bytes(reader, "PT_UNICODE")?)),
      0x0102 => Ok(Self::Binary(read_u16_bytes(reader, "PT_BINARY")?)),
      0x101e => {
        let raw_count = reader.read_u32()?;
        let count = checked_count(reader, raw_count, 2, "PT_MV_STRING8")?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
          values.push(read_u16_bytes(reader, "PT_MV_STRING8 value")?);
        }
        Ok(Self::MultiString8(values))
      }
      0x1102 => {
        let raw_count = reader.read_u32()?;
        let count = checked_count(reader, raw_count, 2, "PT_MV_BINARY")?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
          values.push(read_u16_bytes(reader, "PT_MV_BINARY value")?);
        }
        Ok(Self::MultiBinary(values))
      }
      _ => Err(Error::invalid(
        reader.position()?.saturating_sub(4),
        format!("unknown envelope recipient property type {type_id:#06x}"),
      )),
    }
  }

  fn type_id(&self) -> u16 {
    match self {
      Self::Long(_) => 0x0003,
      Self::Null(_) => 0x0001,
      Self::Boolean(_) => 0x000b,
      Self::SystemTime(_) => 0x0040,
      Self::Error(_) => 0x000a,
      Self::String8(_) => 0x001e,
      Self::Unicode(_) => 0x001f,
      Self::Binary(_) => 0x0102,
      Self::MultiString8(_) => 0x101e,
      Self::MultiBinary(_) => 0x1102,
    }
  }

  fn write<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::Long(value) | Self::Null(value) | Self::Error(value) => writer.write_u32(*value),
      Self::Boolean(value) => writer.write_u16(*value),
      Self::SystemTime(value) => {
        writer.write_u32(value.high())?;
        writer.write_u32(value.low())
      }
      Self::String8(value) | Self::Binary(value) => {
        write_u16_bytes(writer, value, "recipient property")
      }
      Self::Unicode(value) => write_u16_utf16_bytes(writer, value, "recipient Unicode property"),
      Self::MultiString8(values) | Self::MultiBinary(values) => {
        writer.write_u32(checked_u32(values.len(), "multi-value property count")?)?;
        for value in values {
          write_u16_bytes(writer, value, "multi-value property")?;
        }
        Ok(())
      }
    }
  }
}

impl EnvelopeAttachment {
  fn read_collection<R: Read + Seek>(reader: &mut Reader<R>) -> Result<Vec<Self>> {
    let raw_count = reader.read_u32()?;
    let count = checked_count(reader, raw_count, 13, "attachment")?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
      let method = reader.read_u32()?;
      let name_count = usize::from(reader.read_u8()?);
      reader.ensure_allocation(name_count, 2)?;
      let mut name = Vec::with_capacity(name_count);
      for _ in 0..name_count {
        name.push(reader.read_u16()?);
      }
      let size_low = reader.read_u32()?;
      let size_high = reader.read_u32()?;
      let size = usize::try_from(FileTime::from_parts(size_low, size_high).ticks())
        .map_err(|_| Error::Limit("attachment size exceeds usize".into()))?;
      let data = reader.read_vec(size)?;
      values.push(Self { method, name, data });
    }
    Ok(values)
  }

  fn write_collection<W: Write>(writer: &mut Writer<W>, values: &[Self]) -> Result<()> {
    writer.write_u32(checked_u32(values.len(), "attachment count")?)?;
    for value in values {
      writer.write_u32(value.method)?;
      writer.write_u8(
        u8::try_from(value.name.len())
          .map_err(|_| Error::Limit("attachment name exceeds u8".into()))?,
      )?;
      for character in &value.name {
        writer.write_u16(*character)?;
      }
      let size = u64::try_from(value.data.len())
        .map_err(|_| Error::Limit("attachment size exceeds u64".into()))?;
      writer.write_u32(size as u32)?;
      writer.write_u32((size >> 32) as u32)?;
      writer.write_all(&value.data)?;
    }
    Ok(())
  }
}

fn read_bool32<R: Read + Seek>(reader: &mut Reader<R>, name: &str) -> Result<bool> {
  match reader.read_u32()? {
    0 => Ok(false),
    1 => Ok(true),
    value => Err(Error::invalid(
      reader.position()?.saturating_sub(4),
      format!("{name} is not a boolean: {value}"),
    )),
  }
}

fn checked_count<R: Read + Seek>(
  reader: &mut Reader<R>,
  count: u32,
  minimum_size: u64,
  name: &str,
) -> Result<usize> {
  let count =
    usize::try_from(count).map_err(|_| Error::Limit(format!("{name} count exceeds usize")))?;
  reader.ensure_allocation(count, usize::try_from(minimum_size).unwrap_or(usize::MAX))?;
  if u64::try_from(count).unwrap_or(u64::MAX) > reader.remaining()? / minimum_size {
    return Err(Error::invalid(
      reader.position()?,
      format!("{name} count exceeds bounded input"),
    ));
  }
  Ok(count)
}

fn checked_u16(value: usize, name: &str) -> Result<u16> {
  u16::try_from(value).map_err(|_| Error::Limit(format!("{name} exceeds u16")))
}

fn checked_u32(value: usize, name: &str) -> Result<u32> {
  u32::try_from(value).map_err(|_| Error::Limit(format!("{name} exceeds u32")))
}

fn read_u16_bytes<R: Read + Seek>(reader: &mut Reader<R>, name: &str) -> Result<Vec<u8>> {
  let size = usize::from(reader.read_u16()?);
  reader
    .read_vec(size)
    .map_err(|error| add_context(error, name))
}

fn write_u16_bytes<W: Write>(writer: &mut Writer<W>, value: &[u8], name: &str) -> Result<()> {
  writer.write_u16(checked_u16(value.len(), name)?)?;
  Ok(writer.write_all(value)?)
}

fn read_u32_bytes<R: Read + Seek>(reader: &mut Reader<R>, name: &str) -> Result<Vec<u8>> {
  let size = usize::try_from(reader.read_u32()?)
    .map_err(|_| Error::Limit(format!("{name} size exceeds usize")))?;
  reader
    .read_vec(size)
    .map_err(|error| add_context(error, name))
}

fn write_u32_bytes<W: Write>(writer: &mut Writer<W>, value: &[u8], name: &str) -> Result<()> {
  writer.write_u32(checked_u32(value.len(), name)?)?;
  Ok(writer.write_all(value)?)
}

fn read_u16_utf16_bytes<R: Read + Seek>(reader: &mut Reader<R>, name: &str) -> Result<Vec<u16>> {
  let byte_count = usize::from(reader.read_u16()?);
  read_utf16_payload(reader, byte_count, name)
}

fn write_u16_utf16_bytes<W: Write>(
  writer: &mut Writer<W>,
  value: &[u16],
  name: &str,
) -> Result<()> {
  let byte_count = value
    .len()
    .checked_mul(2)
    .ok_or_else(|| Error::Limit(format!("{name} size overflows usize")))?;
  writer.write_u16(checked_u16(byte_count, name)?)?;
  write_utf16_payload(writer, value)
}

fn read_u32_utf16_bytes<R: Read + Seek>(reader: &mut Reader<R>, name: &str) -> Result<Vec<u16>> {
  let byte_count = usize::try_from(reader.read_u32()?)
    .map_err(|_| Error::Limit(format!("{name} size exceeds usize")))?;
  read_utf16_payload(reader, byte_count, name)
}

fn write_u32_utf16_bytes<W: Write>(
  writer: &mut Writer<W>,
  value: &[u16],
  name: &str,
) -> Result<()> {
  let byte_count = value
    .len()
    .checked_mul(2)
    .ok_or_else(|| Error::Limit(format!("{name} size overflows usize")))?;
  writer.write_u32(checked_u32(byte_count, name)?)?;
  write_utf16_payload(writer, value)
}

fn read_utf16_payload<R: Read + Seek>(
  reader: &mut Reader<R>,
  byte_count: usize,
  name: &str,
) -> Result<Vec<u16>> {
  if !byte_count.is_multiple_of(2) {
    return Err(Error::invalid(
      reader.position()?,
      format!("{name} byte count is odd"),
    ));
  }
  let count = byte_count / 2;
  reader.ensure_allocation(count, 2)?;
  let mut value = Vec::with_capacity(count);
  for _ in 0..count {
    value.push(reader.read_u16()?);
  }
  Ok(value)
}

fn write_utf16_payload<W: Write>(writer: &mut Writer<W>, value: &[u16]) -> Result<()> {
  for character in value {
    writer.write_u16(*character)?;
  }
  Ok(())
}

fn add_context(error: Error, name: &str) -> Error {
  match error {
    Error::InvalidData { offset, message } => Error::invalid(offset, format!("{name}: {message}")),
    other => other,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn empty_collection() -> EnvelopeRecipientCollection {
    EnvelopeRecipientCollection {
      recipients: Vec::new(),
    }
  }

  #[test]
  fn mso_envelope_round_trips_all_public_recipient_property_variants() {
    let properties = vec![
      EnvelopeRecipientProperty {
        property_id: 1,
        value: EnvelopeRecipientPropertyValue::Long(7),
      },
      EnvelopeRecipientProperty {
        property_id: 2,
        value: EnvelopeRecipientPropertyValue::Null(0),
      },
      EnvelopeRecipientProperty {
        property_id: 3,
        value: EnvelopeRecipientPropertyValue::Boolean(1),
      },
      EnvelopeRecipientProperty {
        property_id: 4,
        value: EnvelopeRecipientPropertyValue::SystemTime(FileTime::from_ticks(9)),
      },
      EnvelopeRecipientProperty {
        property_id: 5,
        value: EnvelopeRecipientPropertyValue::Error(0x8000_4005),
      },
      EnvelopeRecipientProperty {
        property_id: 6,
        value: EnvelopeRecipientPropertyValue::String8(b"mail".to_vec()),
      },
      EnvelopeRecipientProperty {
        property_id: 7,
        value: EnvelopeRecipientPropertyValue::Unicode("name".encode_utf16().collect()),
      },
      EnvelopeRecipientProperty {
        property_id: 8,
        value: EnvelopeRecipientPropertyValue::Binary(vec![1, 2]),
      },
      EnvelopeRecipientProperty {
        property_id: 9,
        value: EnvelopeRecipientPropertyValue::MultiString8(vec![b"a".to_vec()]),
      },
      EnvelopeRecipientProperty {
        property_id: 10,
        value: EnvelopeRecipientPropertyValue::MultiBinary(vec![vec![3, 4]]),
      },
    ];
    let envelope = MsoEnvelopeClsid {
      clsid: MSO_ENVELOPE_CLSID,
      data: MsoEnvelopeData::Envelope(Box::new(MsoEnvelope {
        version: MsoEnvelopeVersion::Unicode8,
        last_sent_time: EnvelopeMinuteTime::UNSPECIFIED,
        flag_status: EnvelopeFlagStatus::Flagged,
        reply_time: EnvelopeMinuteTime::UNSPECIFIED,
        request: EnvelopeString::Unicode(Vec::new()),
        sent_representing_entry_id: Vec::new(),
        sent_representing_name: EnvelopeString::Unicode(Vec::new()),
        internet_account_stamp: EnvelopeString::Unicode(Vec::new()),
        internet_account_name: EnvelopeString::Unicode(Vec::new()),
        expiry_time: EnvelopeMinuteTime::UNSPECIFIED,
        deferred_delivery_time: EnvelopeMinuteTime::UNSPECIFIED,
        delete_after_submit: false,
        security: EnvelopeSecurityFlags::SIGNED,
        originator_delivery_report_requested: false,
        read_receipt_requested: true,
        categories: EnvelopeString::Unicode(Vec::new()),
        sensitivity: EnvelopeSensitivity::Personal,
        importance: EnvelopeImportance::High,
        subject: EnvelopeString::Unicode("subject".encode_utf16().collect()),
        voting_options: b"Yes;No".to_vec(),
        reply_recipients: EnvelopeRecipientCollection {
          recipients: vec![EnvelopeRecipient {
            ignored: 0x1234,
            properties,
          }],
        },
        contact_link_recipients: Some(empty_collection()),
        recipients: empty_collection(),
        attachments: vec![EnvelopeAttachment {
          method: 1,
          name: "a.txt".encode_utf16().collect(),
          data: vec![5, 6, 7],
        }],
        intro_text: Some("intro".encode_utf16().collect()),
      })),
    };
    let bytes = envelope.to_bytes().unwrap();
    assert_eq!(MsoEnvelopeClsid::from_bytes(&bytes).unwrap(), envelope);
  }

  #[test]
  fn mso_envelope_version_6_round_trips_ansi_strings() {
    let envelope = MsoEnvelopeClsid {
      clsid: MSO_ENVELOPE_CLSID,
      data: MsoEnvelopeData::Envelope(Box::new(MsoEnvelope {
        version: MsoEnvelopeVersion::Ansi6,
        last_sent_time: EnvelopeMinuteTime::UNSPECIFIED,
        flag_status: EnvelopeFlagStatus::NotFlagged,
        reply_time: EnvelopeMinuteTime::UNSPECIFIED,
        request: EnvelopeString::Ansi(b"follow up".to_vec()),
        sent_representing_entry_id: Vec::new(),
        sent_representing_name: EnvelopeString::Ansi(b"sender".to_vec()),
        internet_account_stamp: EnvelopeString::Ansi(Vec::new()),
        internet_account_name: EnvelopeString::Ansi(Vec::new()),
        expiry_time: EnvelopeMinuteTime::UNSPECIFIED,
        deferred_delivery_time: EnvelopeMinuteTime::UNSPECIFIED,
        delete_after_submit: true,
        security: EnvelopeSecurityFlags::empty(),
        originator_delivery_report_requested: false,
        read_receipt_requested: false,
        categories: EnvelopeString::Ansi(Vec::new()),
        sensitivity: EnvelopeSensitivity::Normal,
        importance: EnvelopeImportance::Normal,
        subject: EnvelopeString::Ansi(b"subject".to_vec()),
        voting_options: Vec::new(),
        reply_recipients: empty_collection(),
        contact_link_recipients: None,
        recipients: empty_collection(),
        attachments: Vec::new(),
        intro_text: None,
      })),
    };
    let bytes = envelope.to_bytes().unwrap();
    assert_eq!(MsoEnvelopeClsid::from_bytes(&bytes).unwrap(), envelope);
  }

  #[test]
  fn nonstandard_mso_envelope_clsid_is_explicitly_out_of_scope() {
    let value = MsoEnvelopeClsid {
      clsid: [0x55; 16],
      data: MsoEnvelopeData::OutOfScope(vec![1, 2, 3]),
    };
    assert_eq!(
      MsoEnvelopeClsid::from_bytes(&value.to_bytes().unwrap()).unwrap(),
      value
    );
  }
}
