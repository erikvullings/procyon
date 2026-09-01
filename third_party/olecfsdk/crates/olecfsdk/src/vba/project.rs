//! MS-OVBA PROJECT and PROJECTwm stream models.

use crate::{Error, Result, common::CodePage, limits::Limits};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectStream {
  pub records: Vec<ProjectRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectRecord {
  pub kind: ProjectRecordKind,
  pub ending: LineEnding,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectRecordKind {
  Property {
    kind: ProjectPropertyKind,
    value: Vec<u8>,
  },
  HostExtenderHeader,
  HostExtender {
    value: Vec<u8>,
  },
  WorkspaceHeader,
  Workspace {
    value: Vec<u8>,
  },
  Blank,
  Unknown {
    bytes: Vec<u8>,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectPropertyKind {
  Id,
  DocumentModule,
  StandardModule,
  ClassModule,
  DesignerModule,
  Package,
  HelpFile,
  ExeName32,
  Name,
  HelpContextId,
  Description,
  VersionCompatible32,
  ProtectionState,
  Password,
  VisibilityState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
  CrLf,
  Lf,
  Cr,
  None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectWmStream {
  pub names: Vec<NameMap>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameMap {
  /// Includes neither the one-byte null terminator nor decoded text.
  pub module_name_mbcs: Vec<u8>,
  /// Includes neither the UTF-16 null terminator nor decoded text.
  pub module_name_unicode: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectLkStream {
  pub version: u16,
  pub licenses: Vec<LicenseInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LicenseInfo {
  pub class_id: [u8; 16],
  pub license_key: Vec<u8>,
  pub license_required: u32,
}

#[derive(Clone, Copy)]
enum Section {
  Properties,
  HostExtenders,
  Workspace,
}

impl ProjectStream {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_bytes_with_limits(bytes, Limits::default())
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    if bytes.len() as u64 > limits.max_stream_size {
      return Err(Error::Limit(format!(
        "PROJECT stream length {} exceeds {}",
        bytes.len(),
        limits.max_stream_size
      )));
    }
    let mut records = Vec::new();
    let mut cursor = 0usize;
    let mut section = Section::Properties;
    while cursor < bytes.len() {
      let (line, ending, next) = take_line(bytes, cursor);
      cursor = next;
      let kind = if line.is_empty() {
        ProjectRecordKind::Blank
      } else if line == b"[Host Extender Info]" {
        section = Section::HostExtenders;
        ProjectRecordKind::HostExtenderHeader
      } else if line == b"[Workspace]" {
        section = Section::Workspace;
        ProjectRecordKind::WorkspaceHeader
      } else {
        match section {
          Section::Properties => parse_property_line(line),
          Section::HostExtenders if line.contains(&b'=') => ProjectRecordKind::HostExtender {
            value: line.to_vec(),
          },
          Section::Workspace if line.contains(&b'=') => ProjectRecordKind::Workspace {
            value: line.to_vec(),
          },
          _ => ProjectRecordKind::Unknown {
            bytes: line.to_vec(),
          },
        }
      };
      records.push(ProjectRecord { kind, ending });
      if records.len() > limits.max_entries {
        return Err(Error::Limit(format!(
          "PROJECT record count exceeds {}",
          limits.max_entries
        )));
      }
    }
    Ok(Self { records })
  }

  pub fn to_bytes(&self) -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in &self.records {
      match &record.kind {
        ProjectRecordKind::Property { kind, value } => {
          bytes.extend_from_slice(kind.prefix());
          bytes.extend_from_slice(value);
        }
        ProjectRecordKind::HostExtenderHeader => bytes.extend_from_slice(b"[Host Extender Info]"),
        ProjectRecordKind::HostExtender { value } | ProjectRecordKind::Workspace { value } => {
          bytes.extend_from_slice(value)
        }
        ProjectRecordKind::WorkspaceHeader => bytes.extend_from_slice(b"[Workspace]"),
        ProjectRecordKind::Blank => {}
        ProjectRecordKind::Unknown { bytes: value } => bytes.extend_from_slice(value),
      }
      bytes.extend_from_slice(record.ending.bytes());
    }
    bytes
  }

  pub fn text(&self, code_page: CodePage) -> Result<String> {
    code_page.decode(&self.to_bytes())
  }

  pub fn has_unknown_records(&self) -> bool {
    self
      .records
      .iter()
      .any(|record| matches!(record.kind, ProjectRecordKind::Unknown { .. }))
  }
}

impl ProjectWmStream {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_bytes_with_limits(bytes, Limits::default())
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    if bytes.len() as u64 > limits.max_stream_size {
      return Err(Error::Limit(format!(
        "PROJECTwm stream length {} exceeds {}",
        bytes.len(),
        limits.max_stream_size
      )));
    }
    let mut cursor = 0usize;
    let mut names = Vec::new();
    loop {
      if bytes.get(cursor..) == Some(&[0, 0][..]) {
        break;
      }
      let mbcs_end = bytes[cursor..]
        .iter()
        .position(|value| *value == 0)
        .map(|relative| cursor + relative)
        .ok_or_else(|| Error::invalid(cursor as u64, "unterminated PROJECTwm MBCS name"))?;
      if mbcs_end == cursor {
        return Err(Error::invalid(
          cursor as u64,
          "PROJECTwm MBCS name must not be empty",
        ));
      }
      let module_name_mbcs = bytes[cursor..mbcs_end].to_vec();
      cursor = mbcs_end + 1;
      let mut module_name_unicode = Vec::new();
      loop {
        let pair = bytes
          .get(cursor..cursor.saturating_add(2))
          .ok_or_else(|| Error::invalid(cursor as u64, "unterminated PROJECTwm Unicode name"))?;
        cursor += 2;
        let value = u16::from_le_bytes([pair[0], pair[1]]);
        if value == 0 {
          break;
        }
        module_name_unicode.push(value);
      }
      if module_name_unicode.is_empty() {
        return Err(Error::invalid(
          cursor as u64,
          "PROJECTwm Unicode name must not be empty",
        ));
      }
      names.push(NameMap {
        module_name_mbcs,
        module_name_unicode,
      });
      if names.len() > limits.max_entries {
        return Err(Error::Limit(format!(
          "PROJECTwm name count exceeds {}",
          limits.max_entries
        )));
      }
    }
    Ok(Self { names })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for name in &self.names {
      if name.module_name_mbcs.is_empty()
        || name.module_name_mbcs.contains(&0)
        || name.module_name_unicode.is_empty()
        || name.module_name_unicode.contains(&0)
      {
        return Err(Error::invalid(0, "invalid PROJECTwm module name"));
      }
      bytes.extend_from_slice(&name.module_name_mbcs);
      bytes.push(0);
      for value in &name.module_name_unicode {
        bytes.extend_from_slice(&value.to_le_bytes());
      }
      bytes.extend_from_slice(&0u16.to_le_bytes());
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
    Ok(bytes)
  }

  pub fn validate_names(&self, code_page: CodePage) -> Result<()> {
    for name in &self.names {
      let mbcs = code_page.decode(&name.module_name_mbcs)?;
      let unicode = String::from_utf16(&name.module_name_unicode)
        .map_err(|_| Error::invalid(0, "invalid PROJECTwm UTF-16 module name"))?;
      if mbcs != unicode {
        return Err(Error::invalid(
          0,
          "PROJECTwm MBCS and Unicode module names differ",
        ));
      }
    }
    Ok(())
  }
}

impl ProjectLkStream {
  pub const VERSION: u16 = 1;

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_bytes_with_limits(bytes, Limits::default())
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    if bytes.len() as u64 > limits.max_stream_size {
      return Err(Error::Limit(format!(
        "PROJECTlk stream length {} exceeds {}",
        bytes.len(),
        limits.max_stream_size
      )));
    }
    let mut cursor = 0usize;
    let version = take_u16(bytes, &mut cursor, "truncated PROJECTlk version")?;
    if version != Self::VERSION {
      return Err(Error::invalid(0, "PROJECTlk version must be 1"));
    }
    let count = usize::try_from(take_u32(
      bytes,
      &mut cursor,
      "truncated PROJECTlk license count",
    )?)
    .map_err(|_| Error::Limit("PROJECTlk license count does not fit usize".into()))?;
    if count > limits.max_entries {
      return Err(Error::Limit(format!(
        "PROJECTlk license count exceeds {}",
        limits.max_entries
      )));
    }
    let mut licenses = Vec::with_capacity(count);
    for _ in 0..count {
      let class_id: [u8; 16] = take_bytes(bytes, &mut cursor, 16, "truncated PROJECTlk ClassID")?
        .try_into()
        .expect("take_bytes returned exactly 16 bytes");
      let key_len = usize::try_from(take_u32(
        bytes,
        &mut cursor,
        "truncated PROJECTlk license-key size",
      )?)
      .map_err(|_| Error::Limit("PROJECTlk key length does not fit usize".into()))?;
      if key_len > limits.max_allocation {
        return Err(Error::Limit(
          "PROJECTlk license key exceeds allocation limit".into(),
        ));
      }
      let license_key = take_bytes(
        bytes,
        &mut cursor,
        key_len,
        "truncated PROJECTlk license key",
      )?
      .to_vec();
      let license_required = take_u32(bytes, &mut cursor, "truncated PROJECTlk LicenseRequired")?;
      licenses.push(LicenseInfo {
        class_id,
        license_key,
        license_required,
      });
    }
    if cursor != bytes.len() {
      return Err(Error::invalid(
        cursor as u64,
        "unexpected trailing bytes in PROJECTlk stream",
      ));
    }
    Ok(Self { version, licenses })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.version != Self::VERSION {
      return Err(Error::invalid(0, "PROJECTlk version must be 1"));
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&self.version.to_le_bytes());
    bytes.extend_from_slice(
      &u32::try_from(self.licenses.len())
        .map_err(|_| Error::Limit("PROJECTlk license count exceeds u32".into()))?
        .to_le_bytes(),
    );
    for license in &self.licenses {
      bytes.extend_from_slice(&license.class_id);
      bytes.extend_from_slice(
        &u32::try_from(license.license_key.len())
          .map_err(|_| Error::Limit("PROJECTlk license key exceeds u32".into()))?
          .to_le_bytes(),
      );
      bytes.extend_from_slice(&license.license_key);
      bytes.extend_from_slice(&license.license_required.to_le_bytes());
    }
    Ok(bytes)
  }
}

fn parse_property_line(line: &[u8]) -> ProjectRecordKind {
  for kind in ProjectPropertyKind::ALL {
    if let Some(value) = line.strip_prefix(kind.prefix()) {
      return ProjectRecordKind::Property {
        kind,
        value: value.to_vec(),
      };
    }
  }
  ProjectRecordKind::Unknown {
    bytes: line.to_vec(),
  }
}

impl ProjectPropertyKind {
  const ALL: [Self; 15] = [
    Self::Id,
    Self::DocumentModule,
    Self::StandardModule,
    Self::ClassModule,
    Self::DesignerModule,
    Self::Package,
    Self::HelpFile,
    Self::ExeName32,
    Self::Name,
    Self::HelpContextId,
    Self::Description,
    Self::VersionCompatible32,
    Self::ProtectionState,
    Self::Password,
    Self::VisibilityState,
  ];

  fn prefix(self) -> &'static [u8] {
    match self {
      Self::Id => b"ID=",
      Self::DocumentModule => b"Document=",
      Self::StandardModule => b"Module=",
      Self::ClassModule => b"Class=",
      Self::DesignerModule => b"BaseClass=",
      Self::Package => b"Package=",
      Self::HelpFile => b"HelpFile=",
      Self::ExeName32 => b"ExeName32=",
      Self::Name => b"Name=",
      Self::HelpContextId => b"HelpContextID=",
      Self::Description => b"Description=",
      Self::VersionCompatible32 => b"VersionCompatible32=",
      Self::ProtectionState => b"CMG=",
      Self::Password => b"DPB=",
      Self::VisibilityState => b"GC=",
    }
  }
}

impl LineEnding {
  fn bytes(self) -> &'static [u8] {
    match self {
      Self::CrLf => b"\r\n",
      Self::Lf => b"\n",
      Self::Cr => b"\r",
      Self::None => b"",
    }
  }
}

fn take_line(bytes: &[u8], start: usize) -> (&[u8], LineEnding, usize) {
  let relative = bytes[start..]
    .iter()
    .position(|value| matches!(value, b'\r' | b'\n'));
  let Some(relative) = relative else {
    return (&bytes[start..], LineEnding::None, bytes.len());
  };
  let end = start + relative;
  if bytes[end] == b'\r' && bytes.get(end + 1) == Some(&b'\n') {
    (&bytes[start..end], LineEnding::CrLf, end + 2)
  } else if bytes[end] == b'\r' {
    (&bytes[start..end], LineEnding::Cr, end + 1)
  } else {
    (&bytes[start..end], LineEnding::Lf, end + 1)
  }
}

fn take_u16(bytes: &[u8], cursor: &mut usize, message: &str) -> Result<u16> {
  let value = take_bytes(bytes, cursor, 2, message)?;
  Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn take_u32(bytes: &[u8], cursor: &mut usize, message: &str) -> Result<u32> {
  let value = take_bytes(bytes, cursor, 4, message)?;
  Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn take_bytes<'a>(
  bytes: &'a [u8],
  cursor: &mut usize,
  len: usize,
  message: &str,
) -> Result<&'a [u8]> {
  let end = cursor
    .checked_add(len)
    .ok_or_else(|| Error::Limit("VBA project stream cursor overflow".into()))?;
  let value = bytes
    .get(*cursor..end)
    .ok_or_else(|| Error::invalid(*cursor as u64, message))?;
  *cursor = end;
  Ok(value)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn project_text_records_round_trip_mixed_endings() {
    let bytes = b"ID=\"{00000000-0000-0000-0000-000000000000}\"\r\nModule=M\n\r\n[Host Extender Info]\r&H1={G};VBE;&H0\r\n[Workspace]\r\nM=0, 0, 1, 1, C";
    let project = ProjectStream::from_bytes(bytes).unwrap();
    assert!(!project.has_unknown_records());
    assert_eq!(project.to_bytes(), bytes);
  }

  #[test]
  fn project_wm_name_map_round_trips() {
    let value = ProjectWmStream {
      names: vec![NameMap {
        module_name_mbcs: b"Module1".to_vec(),
        module_name_unicode: "Module1".encode_utf16().collect(),
      }],
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(ProjectWmStream::from_bytes(&bytes).unwrap(), value);
  }

  #[test]
  fn malformed_project_wm_is_rejected() {
    assert!(ProjectWmStream::from_bytes(&[]).is_err());
    assert!(ProjectWmStream::from_bytes(&[0, 0, 0]).is_err());
    assert!(ProjectWmStream::from_bytes(b"name\0x").is_err());
  }

  #[test]
  fn project_lk_license_records_round_trip() {
    let value = ProjectLkStream {
      version: 1,
      licenses: vec![LicenseInfo {
        class_id: [0x5a; 16],
        license_key: b"key".to_vec(),
        license_required: 1,
      }],
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(ProjectLkStream::from_bytes(&bytes).unwrap(), value);
  }
}
