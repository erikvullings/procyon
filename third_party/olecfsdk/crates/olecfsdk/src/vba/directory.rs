//! Static MS-OVBA `dir` stream record framing and common record payloads.

use crate::{Error, Result, common::CodePage, limits::Limits};

const PROJECT_VERSION: u16 = 0x0009;
const DIR_TERMINATOR: u16 = 0x0010;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirStream {
  pub records: Vec<DirRecord>,
  /// MS-OVBA 2.3.4.2 Reserved field following the 0x0010 terminator.
  pub reserved: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModuleDescriptor {
  pub name_mbcs: Option<Vec<u8>>,
  pub name_unicode: Option<Vec<u16>>,
  pub stream_name_mbcs: Option<Vec<u8>>,
  pub stream_name_unicode: Option<Vec<u16>>,
  pub text_offset: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceRegistered {
  pub libid: Vec<u8>,
  pub reserved1: u32,
  pub reserved2: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceProject {
  pub libid_absolute: Vec<u8>,
  pub libid_relative: Vec<u8>,
  pub major_version: u32,
  pub minor_version: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceControlTwiddled {
  pub libid: Vec<u8>,
  pub reserved1: u32,
  pub reserved2: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceControlExtended {
  pub libid: Vec<u8>,
  pub reserved4: u32,
  pub reserved5: u16,
  pub original_type_lib: [u8; 16],
  pub cookie: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirRecord {
  U32 {
    kind: U32RecordKind,
    value: u32,
  },
  U16 {
    kind: U16RecordKind,
    value: u16,
  },
  MbcsString {
    kind: MbcsStringRecordKind,
    bytes: Vec<u8>,
  },
  UnicodeString {
    kind: UnicodeStringRecordKind,
    code_units: Vec<u16>,
  },
  ProjectVersion {
    reserved: u32,
    major: u32,
    minor: u16,
  },
  Marker {
    kind: MarkerRecordKind,
    reserved: u32,
  },
  ReferenceRegistered(ReferenceRegistered),
  ReferenceProject(ReferenceProject),
  ReferenceControlTwiddled(ReferenceControlTwiddled),
  ReferenceControlExtended(ReferenceControlExtended),
  ReferenceOriginal {
    libid: Vec<u8>,
  },
  Unknown {
    id: u16,
    payload: Vec<u8>,
  },
  Terminator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum U32RecordKind {
  ProjectSysKind,
  ProjectLcid,
  ProjectLcidInvoke,
  ProjectHelpContext,
  ProjectLibFlags,
  ModuleOffset,
  ModuleHelpContext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum U16RecordKind {
  ProjectCodePage,
  ProjectModules,
  ProjectCookie,
  ModuleCookie,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MbcsStringRecordKind {
  ProjectName,
  ProjectDocString,
  ProjectHelpFilePath,
  ProjectConstants,
  ReferenceName,
  ModuleName,
  ModuleStreamName,
  ModuleDocString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnicodeStringRecordKind {
  ProjectDocString,
  ProjectHelpFilePath,
  ProjectConstants,
  ReferenceName,
  ModuleName,
  ModuleStreamName,
  ModuleDocString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerRecordKind {
  ModuleProcedural,
  ModuleClassOrDocument,
  ModuleReadOnly,
  ModulePrivate,
  ModuleTerminator,
}

impl DirStream {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_bytes_with_limits(bytes, Limits::default())
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    if bytes.len() as u64 > limits.max_stream_size {
      return Err(Error::Limit(format!(
        "VBA dir stream length {} exceeds {}",
        bytes.len(),
        limits.max_stream_size
      )));
    }
    let mut cursor = 0usize;
    let mut records = Vec::new();
    while cursor < bytes.len() {
      let record_offset = cursor;
      let id = take_u16(bytes, &mut cursor, "truncated VBA dir record id")?;
      if id == DIR_TERMINATOR {
        records.push(DirRecord::Terminator);
        let reserved = take_u32(bytes, &mut cursor, "truncated VBA dir Reserved field")?;
        if reserved != 0 {
          return Err(Error::invalid(
            cursor.saturating_sub(4) as u64,
            "VBA dir Reserved field must be zero",
          ));
        }
        if cursor != bytes.len() {
          return Err(Error::invalid(
            cursor as u64,
            "VBA dir stream has bytes after Reserved field",
          ));
        }
        return Ok(Self { records, reserved });
      }
      if id == PROJECT_VERSION {
        records.push(DirRecord::ProjectVersion {
          reserved: take_u32(bytes, &mut cursor, "truncated PROJECTVERSION reserved")?,
          major: take_u32(bytes, &mut cursor, "truncated PROJECTVERSION major")?,
          minor: take_u16(bytes, &mut cursor, "truncated PROJECTVERSION minor")?,
        });
        continue;
      }
      if let Some(kind) = MarkerRecordKind::from_id(id) {
        records.push(DirRecord::Marker {
          kind,
          reserved: take_u32(bytes, &mut cursor, "truncated VBA module marker")?,
        });
        continue;
      }

      let size = usize::try_from(take_u32(
        bytes,
        &mut cursor,
        "truncated VBA dir record size",
      )?)
      .map_err(|_| Error::Limit("VBA dir record size does not fit usize".into()))?;
      if size > limits.max_allocation {
        return Err(Error::Limit(format!(
          "VBA dir record at {record_offset} exceeds allocation limit"
        )));
      }
      let end = cursor
        .checked_add(size)
        .ok_or_else(|| Error::Limit("VBA dir record end overflow".into()))?;
      let payload = bytes
        .get(cursor..end)
        .ok_or_else(|| Error::invalid(record_offset as u64, "truncated VBA dir record payload"))?;
      cursor = end;
      records.push(decode_sized_record(id, payload, record_offset)?);
      if records.len() > limits.max_entries {
        return Err(Error::Limit(format!(
          "VBA dir record count exceeds {}",
          limits.max_entries
        )));
      }
    }
    Err(Error::invalid(
      bytes.len() as u64,
      "VBA dir stream has no 0x0010 terminator",
    ))
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut saw_terminator = false;
    for record in &self.records {
      if saw_terminator {
        return Err(Error::invalid(
          bytes.len() as u64,
          "VBA dir record follows terminator",
        ));
      }
      record.write_to(&mut bytes)?;
      saw_terminator = matches!(record, DirRecord::Terminator);
    }
    if !saw_terminator {
      return Err(Error::invalid(
        bytes.len() as u64,
        "VBA dir stream has no 0x0010 terminator",
      ));
    }
    if self.reserved != 0 {
      return Err(Error::invalid(
        bytes.len() as u64,
        "VBA dir Reserved field must be zero",
      ));
    }
    bytes.extend_from_slice(&self.reserved.to_le_bytes());
    Ok(bytes)
  }

  pub fn code_page(&self) -> Option<u16> {
    self.records.iter().find_map(|record| match record {
      DirRecord::U16 {
        kind: U16RecordKind::ProjectCodePage,
        value,
      } => Some(*value),
      _ => None,
    })
  }

  pub fn module_offsets(&self) -> impl Iterator<Item = u32> + '_ {
    self.records.iter().filter_map(|record| match record {
      DirRecord::U32 {
        kind: U32RecordKind::ModuleOffset,
        value,
      } => Some(*value),
      _ => None,
    })
  }

  pub fn set_module_offsets(&mut self, offset: u32) -> usize {
    let mut changed = 0;
    for record in &mut self.records {
      if let DirRecord::U32 {
        kind: U32RecordKind::ModuleOffset,
        value,
      } = record
      {
        *value = offset;
        changed += 1;
      }
    }
    changed
  }

  pub fn modules(&self) -> Vec<ModuleDescriptor> {
    let mut modules = Vec::new();
    let mut current: Option<ModuleDescriptor> = None;
    for record in &self.records {
      match record {
        DirRecord::MbcsString {
          kind: MbcsStringRecordKind::ModuleName,
          bytes,
        } => {
          if let Some(module) = current.take() {
            modules.push(module);
          }
          current = Some(ModuleDescriptor {
            name_mbcs: Some(bytes.clone()),
            ..ModuleDescriptor::default()
          });
        }
        DirRecord::UnicodeString {
          kind: UnicodeStringRecordKind::ModuleName,
          code_units,
        } => {
          if let Some(module) = current.as_mut() {
            module.name_unicode = Some(code_units.clone());
          }
        }
        DirRecord::MbcsString {
          kind: MbcsStringRecordKind::ModuleStreamName,
          bytes,
        } => {
          if let Some(module) = current.as_mut() {
            module.stream_name_mbcs = Some(bytes.clone());
          }
        }
        DirRecord::UnicodeString {
          kind: UnicodeStringRecordKind::ModuleStreamName,
          code_units,
        } => {
          if let Some(module) = current.as_mut() {
            module.stream_name_unicode = Some(code_units.clone());
          }
        }
        DirRecord::U32 {
          kind: U32RecordKind::ModuleOffset,
          value,
        } => {
          if let Some(module) = current.as_mut() {
            module.text_offset = Some(*value);
          }
        }
        DirRecord::Marker {
          kind: MarkerRecordKind::ModuleTerminator,
          ..
        } => {
          if let Some(module) = current.take() {
            modules.push(module);
          }
        }
        _ => {}
      }
    }
    if let Some(module) = current {
      modules.push(module);
    }
    modules
  }
}

impl ModuleDescriptor {
  pub fn stream_name(&self) -> Option<String> {
    if let Some(code_units) = &self.stream_name_unicode {
      return String::from_utf16(code_units).ok();
    }
    self
      .stream_name_mbcs
      .as_deref()
      .and_then(|bytes| std::str::from_utf8(bytes).ok())
      .map(str::to_owned)
  }

  pub fn stream_name_with_code_page(&self, code_page: CodePage) -> Result<String> {
    if let Some(code_units) = &self.stream_name_unicode {
      return String::from_utf16(code_units)
        .map_err(|_| Error::invalid(0, "invalid UTF-16 VBA module stream name"));
    }
    let bytes = self
      .stream_name_mbcs
      .as_deref()
      .ok_or_else(|| Error::invalid(0, "VBA module descriptor has no stream name"))?;
    code_page.decode(bytes)
  }
}

impl DirRecord {
  fn write_to(&self, bytes: &mut Vec<u8>) -> Result<()> {
    match self {
      Self::U32 { kind, value } => write_sized(bytes, kind.id(), &value.to_le_bytes()),
      Self::U16 { kind, value } => write_sized(bytes, kind.id(), &value.to_le_bytes()),
      Self::MbcsString { kind, bytes: value } => write_sized(bytes, kind.id(), value),
      Self::UnicodeString { kind, code_units } => {
        let mut payload = Vec::with_capacity(code_units.len().saturating_mul(2));
        for value in code_units {
          payload.extend_from_slice(&value.to_le_bytes());
        }
        write_sized(bytes, kind.id(), &payload)
      }
      Self::ProjectVersion {
        reserved,
        major,
        minor,
      } => {
        bytes.extend_from_slice(&PROJECT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&reserved.to_le_bytes());
        bytes.extend_from_slice(&major.to_le_bytes());
        bytes.extend_from_slice(&minor.to_le_bytes());
        Ok(())
      }
      Self::Marker { kind, reserved } => {
        bytes.extend_from_slice(&kind.id().to_le_bytes());
        bytes.extend_from_slice(&reserved.to_le_bytes());
        Ok(())
      }
      Self::ReferenceRegistered(value) => {
        let mut payload = Vec::new();
        write_length_prefixed(&mut payload, &value.libid)?;
        payload.extend_from_slice(&value.reserved1.to_le_bytes());
        payload.extend_from_slice(&value.reserved2.to_le_bytes());
        write_sized(bytes, 0x000d, &payload)
      }
      Self::ReferenceProject(value) => {
        let mut payload = Vec::new();
        write_length_prefixed(&mut payload, &value.libid_absolute)?;
        write_length_prefixed(&mut payload, &value.libid_relative)?;
        payload.extend_from_slice(&value.major_version.to_le_bytes());
        payload.extend_from_slice(&value.minor_version.to_le_bytes());
        write_sized(bytes, 0x000e, &payload)
      }
      Self::ReferenceControlTwiddled(value) => {
        let mut payload = Vec::new();
        write_length_prefixed(&mut payload, &value.libid)?;
        payload.extend_from_slice(&value.reserved1.to_le_bytes());
        payload.extend_from_slice(&value.reserved2.to_le_bytes());
        write_sized(bytes, 0x002f, &payload)
      }
      Self::ReferenceControlExtended(value) => {
        let mut payload = Vec::new();
        write_length_prefixed(&mut payload, &value.libid)?;
        payload.extend_from_slice(&value.reserved4.to_le_bytes());
        payload.extend_from_slice(&value.reserved5.to_le_bytes());
        payload.extend_from_slice(&value.original_type_lib);
        payload.extend_from_slice(&value.cookie.to_le_bytes());
        write_sized(bytes, 0x0030, &payload)
      }
      Self::ReferenceOriginal { libid } => write_sized(bytes, 0x0033, libid),
      Self::Unknown { id, payload } => write_sized(bytes, *id, payload),
      Self::Terminator => {
        bytes.extend_from_slice(&DIR_TERMINATOR.to_le_bytes());
        Ok(())
      }
    }
  }
}

fn decode_sized_record(id: u16, payload: &[u8], offset: usize) -> Result<DirRecord> {
  if let Some(kind) = U32RecordKind::from_id(id) {
    let value = exact_u32(payload, offset, "VBA u32 dir record must contain 4 bytes")?;
    return Ok(DirRecord::U32 { kind, value });
  }
  if let Some(kind) = U16RecordKind::from_id(id) {
    let value = exact_u16(payload, offset, "VBA u16 dir record must contain 2 bytes")?;
    return Ok(DirRecord::U16 { kind, value });
  }
  if let Some(kind) = MbcsStringRecordKind::from_id(id) {
    return Ok(DirRecord::MbcsString {
      kind,
      bytes: payload.to_vec(),
    });
  }
  if let Some(kind) = UnicodeStringRecordKind::from_id(id) {
    if !payload.len().is_multiple_of(2) {
      return Err(Error::invalid(
        offset as u64,
        "VBA Unicode dir string has an odd byte length",
      ));
    }
    return Ok(DirRecord::UnicodeString {
      kind,
      code_units: payload
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect(),
    });
  }
  match id {
    0x000d => return parse_reference_registered(payload, offset),
    0x000e => return parse_reference_project(payload, offset),
    0x002f => return parse_reference_control_twiddled(payload, offset),
    0x0030 => return parse_reference_control_extended(payload, offset),
    0x0033 => {
      return Ok(DirRecord::ReferenceOriginal {
        libid: payload.to_vec(),
      });
    }
    _ => {}
  }
  Ok(DirRecord::Unknown {
    id,
    payload: payload.to_vec(),
  })
}

fn parse_reference_registered(payload: &[u8], offset: usize) -> Result<DirRecord> {
  let mut cursor = 0;
  let libid = take_length_prefixed(payload, &mut cursor, offset, "REFERENCEREGISTERED Libid")?;
  let reserved1 = take_u32(
    payload,
    &mut cursor,
    "truncated REFERENCEREGISTERED Reserved1",
  )?;
  let reserved2 = take_u16(
    payload,
    &mut cursor,
    "truncated REFERENCEREGISTERED Reserved2",
  )?;
  ensure_payload_end(payload, cursor, offset, "REFERENCEREGISTERED")?;
  Ok(DirRecord::ReferenceRegistered(ReferenceRegistered {
    libid,
    reserved1,
    reserved2,
  }))
}

fn parse_reference_project(payload: &[u8], offset: usize) -> Result<DirRecord> {
  let mut cursor = 0;
  let libid_absolute = take_length_prefixed(
    payload,
    &mut cursor,
    offset,
    "REFERENCEPROJECT LibidAbsolute",
  )?;
  let libid_relative = take_length_prefixed(
    payload,
    &mut cursor,
    offset,
    "REFERENCEPROJECT LibidRelative",
  )?;
  let major_version = take_u32(payload, &mut cursor, "truncated REFERENCEPROJECT major")?;
  let minor_version = take_u16(payload, &mut cursor, "truncated REFERENCEPROJECT minor")?;
  ensure_payload_end(payload, cursor, offset, "REFERENCEPROJECT")?;
  Ok(DirRecord::ReferenceProject(ReferenceProject {
    libid_absolute,
    libid_relative,
    major_version,
    minor_version,
  }))
}

fn parse_reference_control_twiddled(payload: &[u8], offset: usize) -> Result<DirRecord> {
  let mut cursor = 0;
  let libid = take_length_prefixed(
    payload,
    &mut cursor,
    offset,
    "REFERENCECONTROL LibidTwiddled",
  )?;
  let reserved1 = take_u32(payload, &mut cursor, "truncated REFERENCECONTROL Reserved1")?;
  let reserved2 = take_u16(payload, &mut cursor, "truncated REFERENCECONTROL Reserved2")?;
  ensure_payload_end(payload, cursor, offset, "REFERENCECONTROL twiddled")?;
  Ok(DirRecord::ReferenceControlTwiddled(
    ReferenceControlTwiddled {
      libid,
      reserved1,
      reserved2,
    },
  ))
}

fn parse_reference_control_extended(payload: &[u8], offset: usize) -> Result<DirRecord> {
  let mut cursor = 0;
  let libid = take_length_prefixed(
    payload,
    &mut cursor,
    offset,
    "REFERENCECONTROL LibidExtended",
  )?;
  let reserved4 = take_u32(payload, &mut cursor, "truncated REFERENCECONTROL Reserved4")?;
  let reserved5 = take_u16(payload, &mut cursor, "truncated REFERENCECONTROL Reserved5")?;
  let original = take_bytes(
    payload,
    &mut cursor,
    16,
    offset,
    "REFERENCECONTROL OriginalTypeLib",
  )?;
  let original_type_lib: [u8; 16] = original
    .try_into()
    .expect("take_bytes returned exactly 16 bytes");
  let cookie = take_u32(payload, &mut cursor, "truncated REFERENCECONTROL Cookie")?;
  ensure_payload_end(payload, cursor, offset, "REFERENCECONTROL extended")?;
  Ok(DirRecord::ReferenceControlExtended(
    ReferenceControlExtended {
      libid,
      reserved4,
      reserved5,
      original_type_lib,
      cookie,
    },
  ))
}

fn take_length_prefixed(
  payload: &[u8],
  cursor: &mut usize,
  offset: usize,
  name: &str,
) -> Result<Vec<u8>> {
  let len = usize::try_from(take_u32(payload, cursor, "truncated VBA string length")?)
    .map_err(|_| Error::Limit(format!("{name} length does not fit usize")))?;
  Ok(take_bytes(payload, cursor, len, offset, name)?.to_vec())
}

fn take_bytes<'a>(
  payload: &'a [u8],
  cursor: &mut usize,
  len: usize,
  offset: usize,
  name: &str,
) -> Result<&'a [u8]> {
  let end = cursor
    .checked_add(len)
    .ok_or_else(|| Error::Limit(format!("{name} end overflow")))?;
  let value = payload
    .get(*cursor..end)
    .ok_or_else(|| Error::invalid(offset as u64 + *cursor as u64, format!("truncated {name}")))?;
  *cursor = end;
  Ok(value)
}

fn ensure_payload_end(payload: &[u8], cursor: usize, offset: usize, name: &str) -> Result<()> {
  if cursor != payload.len() {
    return Err(Error::invalid(
      offset as u64 + cursor as u64,
      format!("unexpected trailing bytes in {name}"),
    ));
  }
  Ok(())
}

fn write_length_prefixed(bytes: &mut Vec<u8>, value: &[u8]) -> Result<()> {
  let len =
    u32::try_from(value.len()).map_err(|_| Error::Limit("VBA string length exceeds u32".into()))?;
  bytes.extend_from_slice(&len.to_le_bytes());
  bytes.extend_from_slice(value);
  Ok(())
}

fn write_sized(bytes: &mut Vec<u8>, id: u16, payload: &[u8]) -> Result<()> {
  let size = u32::try_from(payload.len())
    .map_err(|_| Error::Limit("VBA dir record size exceeds u32".into()))?;
  bytes.extend_from_slice(&id.to_le_bytes());
  bytes.extend_from_slice(&size.to_le_bytes());
  bytes.extend_from_slice(payload);
  Ok(())
}

fn take_u16(bytes: &[u8], cursor: &mut usize, message: &str) -> Result<u16> {
  let end = cursor
    .checked_add(2)
    .ok_or_else(|| Error::Limit("VBA dir cursor overflow".into()))?;
  let value = bytes
    .get(*cursor..end)
    .ok_or_else(|| Error::invalid(*cursor as u64, message))?;
  *cursor = end;
  Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn take_u32(bytes: &[u8], cursor: &mut usize, message: &str) -> Result<u32> {
  let end = cursor
    .checked_add(4)
    .ok_or_else(|| Error::Limit("VBA dir cursor overflow".into()))?;
  let value = bytes
    .get(*cursor..end)
    .ok_or_else(|| Error::invalid(*cursor as u64, message))?;
  *cursor = end;
  Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn exact_u16(payload: &[u8], offset: usize, message: &str) -> Result<u16> {
  let value: [u8; 2] = payload
    .try_into()
    .map_err(|_| Error::invalid(offset as u64, message))?;
  Ok(u16::from_le_bytes(value))
}

fn exact_u32(payload: &[u8], offset: usize, message: &str) -> Result<u32> {
  let value: [u8; 4] = payload
    .try_into()
    .map_err(|_| Error::invalid(offset as u64, message))?;
  Ok(u32::from_le_bytes(value))
}

impl U32RecordKind {
  fn from_id(id: u16) -> Option<Self> {
    Some(match id {
      0x0001 => Self::ProjectSysKind,
      0x0002 => Self::ProjectLcid,
      0x0014 => Self::ProjectLcidInvoke,
      0x0007 => Self::ProjectHelpContext,
      0x0008 => Self::ProjectLibFlags,
      0x0031 => Self::ModuleOffset,
      0x001e => Self::ModuleHelpContext,
      _ => return None,
    })
  }

  fn id(self) -> u16 {
    match self {
      Self::ProjectSysKind => 0x0001,
      Self::ProjectLcid => 0x0002,
      Self::ProjectLcidInvoke => 0x0014,
      Self::ProjectHelpContext => 0x0007,
      Self::ProjectLibFlags => 0x0008,
      Self::ModuleOffset => 0x0031,
      Self::ModuleHelpContext => 0x001e,
    }
  }
}

impl U16RecordKind {
  fn from_id(id: u16) -> Option<Self> {
    Some(match id {
      0x0003 => Self::ProjectCodePage,
      0x000f => Self::ProjectModules,
      0x0013 => Self::ProjectCookie,
      0x002c => Self::ModuleCookie,
      _ => return None,
    })
  }

  fn id(self) -> u16 {
    match self {
      Self::ProjectCodePage => 0x0003,
      Self::ProjectModules => 0x000f,
      Self::ProjectCookie => 0x0013,
      Self::ModuleCookie => 0x002c,
    }
  }
}

impl MbcsStringRecordKind {
  fn from_id(id: u16) -> Option<Self> {
    Some(match id {
      0x0004 => Self::ProjectName,
      0x0005 => Self::ProjectDocString,
      0x0006 => Self::ProjectHelpFilePath,
      0x000c => Self::ProjectConstants,
      0x0016 => Self::ReferenceName,
      0x0019 => Self::ModuleName,
      0x001a => Self::ModuleStreamName,
      0x001c => Self::ModuleDocString,
      _ => return None,
    })
  }

  fn id(self) -> u16 {
    match self {
      Self::ProjectName => 0x0004,
      Self::ProjectDocString => 0x0005,
      Self::ProjectHelpFilePath => 0x0006,
      Self::ProjectConstants => 0x000c,
      Self::ReferenceName => 0x0016,
      Self::ModuleName => 0x0019,
      Self::ModuleStreamName => 0x001a,
      Self::ModuleDocString => 0x001c,
    }
  }
}

impl UnicodeStringRecordKind {
  fn from_id(id: u16) -> Option<Self> {
    Some(match id {
      0x0040 => Self::ProjectDocString,
      0x003d => Self::ProjectHelpFilePath,
      0x003c => Self::ProjectConstants,
      0x003e => Self::ReferenceName,
      0x0047 => Self::ModuleName,
      0x0032 => Self::ModuleStreamName,
      0x0048 => Self::ModuleDocString,
      _ => return None,
    })
  }

  fn id(self) -> u16 {
    match self {
      Self::ProjectDocString => 0x0040,
      Self::ProjectHelpFilePath => 0x003d,
      Self::ProjectConstants => 0x003c,
      Self::ReferenceName => 0x003e,
      Self::ModuleName => 0x0047,
      Self::ModuleStreamName => 0x0032,
      Self::ModuleDocString => 0x0048,
    }
  }
}

impl MarkerRecordKind {
  fn from_id(id: u16) -> Option<Self> {
    Some(match id {
      0x0021 => Self::ModuleProcedural,
      0x0022 => Self::ModuleClassOrDocument,
      0x0025 => Self::ModuleReadOnly,
      0x0028 => Self::ModulePrivate,
      0x002b => Self::ModuleTerminator,
      _ => return None,
    })
  }

  fn id(self) -> u16 {
    match self {
      Self::ModuleProcedural => 0x0021,
      Self::ModuleClassOrDocument => 0x0022,
      Self::ModuleReadOnly => 0x0025,
      Self::ModulePrivate => 0x0028,
      Self::ModuleTerminator => 0x002b,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn common_dir_records_round_trip() {
    let mut bytes = Vec::new();
    write_sized(&mut bytes, 0x0001, &1u32.to_le_bytes()).unwrap();
    write_sized(&mut bytes, 0x0003, &1252u16.to_le_bytes()).unwrap();
    write_sized(&mut bytes, 0x0004, b"Project").unwrap();
    bytes.extend_from_slice(&PROJECT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&0x0001_0002u32.to_le_bytes());
    bytes.extend_from_slice(&3u16.to_le_bytes());
    bytes.extend_from_slice(&DIR_TERMINATOR.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());

    let stream = DirStream::from_bytes(&bytes).unwrap();
    assert_eq!(stream.code_page(), Some(1252));
    assert_eq!(stream.to_bytes().unwrap(), bytes);
    assert!(matches!(stream.records.last(), Some(DirRecord::Terminator)));
  }

  #[test]
  fn module_records_are_grouped_into_descriptors() {
    let stream = DirStream {
      records: vec![
        DirRecord::MbcsString {
          kind: MbcsStringRecordKind::ModuleName,
          bytes: b"Module1".to_vec(),
        },
        DirRecord::UnicodeString {
          kind: UnicodeStringRecordKind::ModuleStreamName,
          code_units: "CodeModule".encode_utf16().collect(),
        },
        DirRecord::U32 {
          kind: U32RecordKind::ModuleOffset,
          value: 17,
        },
        DirRecord::Marker {
          kind: MarkerRecordKind::ModuleTerminator,
          reserved: 0,
        },
        DirRecord::Terminator,
      ],
      reserved: 0,
    };
    assert_eq!(
      stream.modules(),
      [ModuleDescriptor {
        name_mbcs: Some(b"Module1".to_vec()),
        stream_name_unicode: Some("CodeModule".encode_utf16().collect()),
        text_offset: Some(17),
        ..ModuleDescriptor::default()
      }]
    );
    assert_eq!(
      stream.modules()[0].stream_name().as_deref(),
      Some("CodeModule")
    );
  }

  #[test]
  fn odd_unicode_and_truncated_records_are_rejected() {
    let mut odd = Vec::new();
    write_sized(&mut odd, 0x0047, &[1]).unwrap();
    assert!(DirStream::from_bytes(&odd).is_err());
    assert!(DirStream::from_bytes(&[1, 0, 4, 0, 0]).is_err());
    assert!(DirStream::from_bytes(&DIR_TERMINATOR.to_le_bytes()).is_err());
    assert!(
      DirStream::from_bytes(&[
        DIR_TERMINATOR as u8,
        (DIR_TERMINATOR >> 8) as u8,
        1,
        0,
        0,
        0,
      ])
      .is_err()
    );
  }

  #[test]
  fn reference_records_have_static_payloads() {
    let records = [
      DirRecord::ReferenceRegistered(ReferenceRegistered {
        libid: b"registered".to_vec(),
        reserved1: 0,
        reserved2: 0,
      }),
      DirRecord::ReferenceProject(ReferenceProject {
        libid_absolute: b"absolute".to_vec(),
        libid_relative: b"relative".to_vec(),
        major_version: 2,
        minor_version: 3,
      }),
      DirRecord::ReferenceControlExtended(ReferenceControlExtended {
        libid: b"extended".to_vec(),
        reserved4: 0,
        reserved5: 0,
        original_type_lib: [0x44; 16],
        cookie: 7,
      }),
    ];
    for record in records {
      let stream = DirStream {
        records: vec![record, DirRecord::Terminator],
        reserved: 0,
      };
      let bytes = stream.to_bytes().unwrap();
      assert_eq!(DirStream::from_bytes(&bytes).unwrap(), stream);
    }
  }
}
