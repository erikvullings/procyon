//! Version-dependent VBA project cache streams.

use std::{io::Cursor, path::PathBuf};

use crate::{
  Error, Result, SdkObject,
  io::{BinaryFormat, IoContext, Reader, SdkRead, SdkWrite, Writer},
  limits::Limits,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SrpStreamName {
  pub hex_digits: String,
}

impl SrpStreamName {
  pub fn parse(name: &str) -> Result<Option<Self>> {
    let Some(prefix) = name.get(..6) else {
      return Ok(None);
    };
    if !prefix.eq_ignore_ascii_case("__SRP_") {
      return Ok(None);
    }
    let hex_digits = &name[6..];
    if hex_digits.is_empty()
      || hex_digits.len() > 25
      || !hex_digits.bytes().all(|value| value.is_ascii_hexdigit())
    {
      return Err(Error::invalid(0, "invalid MS-OVBA SRP stream name"));
    }
    Ok(Some(Self {
      hex_digits: hex_digits.to_owned(),
    }))
  }

  pub fn cfb_name(&self) -> String {
    format!("__SRP_{}", self.hex_digits)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SrpStream {
  pub path: PathBuf,
  pub name: SrpStreamName,
  /// MS-OVBA defines these bytes as implementation-specific and requires
  /// them to be ignored on read and omitted on interoperable write.
  pub implementation_specific_cache: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_vba_project_stream")]
pub struct VbaProjectStream {
  pub reserved1: u16,
  pub version: u16,
  pub reserved2: u8,
  pub reserved3: u16,
  /// Implementation-specific data that MS-OVBA requires readers to ignore.
  #[sdk(remaining)]
  pub performance_cache: Vec<u8>,
}

impl VbaProjectStream {
  pub const RESERVED1: u16 = 0x61cc;
  pub const INTEROPERABLE_VERSION: u16 = 0xffff;

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_bytes_with_limits(bytes, Limits::default())
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    if bytes.len() as u64 > limits.max_stream_size {
      return Err(Error::Limit(format!(
        "_VBA_PROJECT stream length {} exceeds {}",
        bytes.len(),
        limits.max_stream_size
      )));
    }
    let context = IoContext {
      format: BinaryFormat::Vba,
      limits,
      ..IoContext::default()
    };
    Self::read_from(&mut Reader::with_context(Cursor::new(bytes), context)?)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    self.write_to(&mut writer)?;
    Ok(writer.into_inner().into_inner())
  }

  /// Produces the interoperable form required by MS-OVBA when caches are
  /// intentionally discarded during a semantic rewrite.
  pub fn to_interoperable_bytes(&self) -> Result<Vec<u8>> {
    let mut value = self.clone();
    value.version = Self::INTEROPERABLE_VERSION;
    value.performance_cache.clear();
    value.to_bytes()
  }
}

fn validate_vba_project_stream(value: &VbaProjectStream) -> Result<()> {
  if value.reserved1 != VbaProjectStream::RESERVED1 {
    return Err(Error::invalid(0, "_VBA_PROJECT Reserved1 must be 0x61cc"));
  }
  if value.reserved2 != 0 {
    return Err(Error::invalid(4, "_VBA_PROJECT Reserved2 must be zero"));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn vba_project_cache_round_trips_and_can_be_discarded() {
    let value = VbaProjectStream {
      reserved1: VbaProjectStream::RESERVED1,
      version: 0x1234,
      reserved2: 0,
      reserved3: 0x5678,
      performance_cache: vec![1, 2, 3],
    };
    let bytes = value.to_bytes().unwrap();
    assert_eq!(VbaProjectStream::from_bytes(&bytes).unwrap(), value);
    assert_eq!(
      value.to_interoperable_bytes().unwrap(),
      [0xcc, 0x61, 0xff, 0xff, 0, 0x78, 0x56]
    );
  }

  #[test]
  fn invalid_fixed_header_is_rejected() {
    assert!(VbaProjectStream::from_bytes(&[0; 7]).is_err());
  }

  #[test]
  fn srp_stream_name_enforces_ms_ovba_grammar() {
    let name = SrpStreamName::parse("__SRP_0aF19").unwrap().unwrap();
    assert_eq!(name.hex_digits, "0aF19");
    assert_eq!(name.cfb_name(), "__SRP_0aF19");
    assert!(SrpStreamName::parse("module").unwrap().is_none());
    assert!(SrpStreamName::parse("__SRP_").is_err());
    assert!(SrpStreamName::parse("__SRP_not-hex").is_err());
    assert!(SrpStreamName::parse(&format!("__SRP_{}", "A".repeat(26))).is_err());
  }
}
