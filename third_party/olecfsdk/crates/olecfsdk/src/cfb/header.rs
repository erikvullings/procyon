use std::io::Cursor;

use crate::{
  Error, Result, SdkObject,
  io::{Reader, SdkRead, SdkSize},
};

use super::Version;

pub const HEADER_LEN: usize = 512;
pub const MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
pub const BYTE_ORDER_LE: u16 = 0xfffe;
pub const MINI_SECTOR_SHIFT: u16 = 6;
pub const MINI_STREAM_CUTOFF: u32 = 4096;
pub const FREE_SECTOR: u32 = 0xffff_ffff;

/// The fixed 512-byte CFB header defined by MS-CFB section 2.2.
///
/// Compatibility validation intentionally leaves minor version, reserved
/// bytes, and the header CLSID intact. Strict-mode diagnostics will validate
/// those fields without losing their original values.
#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_header")]
pub struct Header {
  pub signature: [u8; 8],
  pub clsid: [u8; 16],
  pub minor_version: u16,
  pub major_version: u16,
  pub byte_order: u16,
  pub sector_shift: u16,
  pub mini_sector_shift: u16,
  pub reserved: [u8; 6],
  pub number_of_directory_sectors: u32,
  pub number_of_fat_sectors: u32,
  pub first_directory_sector: u32,
  pub transaction_signature: u32,
  pub mini_stream_cutoff: u32,
  pub first_mini_fat_sector: u32,
  pub number_of_mini_fat_sectors: u32,
  pub first_difat_sector: u32,
  pub number_of_difat_sectors: u32,
  pub difat: [u32; 109],
}

impl Header {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < HEADER_LEN {
      return Err(Error::invalid(0, "truncated CFB header"));
    }
    let mut reader = Reader::with_bounds(Cursor::new(bytes), 0, HEADER_LEN as u64)?;
    let header = Self::read_from(&mut reader)?;
    debug_assert_eq!(header.sdk_size(), HEADER_LEN as u64);
    Ok(header)
  }

  pub fn version(&self) -> Version {
    match self.major_version {
      3 => Version::V3,
      4 => Version::V4,
      _ => unreachable!("validated CFB major version"),
    }
  }

  pub fn sector_len(&self) -> usize {
    1usize << self.sector_shift
  }
}

fn validate_header(header: &Header) -> Result<()> {
  if header.signature != MAGIC {
    return Err(Error::invalid(0, "invalid CFB signature"));
  }
  if header.byte_order != BYTE_ORDER_LE {
    return Err(Error::invalid(28, "CFB byte order must be little-endian"));
  }
  let expected_shift = match header.major_version {
    3 => 9,
    4 => 12,
    version => {
      return Err(Error::invalid(
        26,
        format!("unsupported CFB major version {version}"),
      ));
    }
  };
  if header.sector_shift != expected_shift {
    return Err(Error::invalid(
      30,
      format!(
        "CFB version {} requires sector shift {expected_shift}, found {}",
        header.major_version, header.sector_shift
      ),
    ));
  }
  if header.mini_sector_shift != MINI_SECTOR_SHIFT {
    return Err(Error::invalid(32, "CFB mini-sector shift must be 6"));
  }
  if header.mini_stream_cutoff != MINI_STREAM_CUTOFF {
    return Err(Error::invalid(56, "CFB mini-stream cutoff must be 4096"));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use std::io::Cursor;

  use crate::io::{SdkWrite, Writer};

  use super::*;

  fn valid_header() -> Header {
    Header {
      signature: MAGIC,
      clsid: [0; 16],
      minor_version: 0x003e,
      major_version: 3,
      byte_order: BYTE_ORDER_LE,
      sector_shift: 9,
      mini_sector_shift: MINI_SECTOR_SHIFT,
      reserved: [0; 6],
      number_of_directory_sectors: 0,
      number_of_fat_sectors: 1,
      first_directory_sector: 1,
      transaction_signature: 0,
      mini_stream_cutoff: MINI_STREAM_CUTOFF,
      first_mini_fat_sector: FREE_SECTOR,
      number_of_mini_fat_sectors: 0,
      first_difat_sector: FREE_SECTOR,
      number_of_difat_sectors: 0,
      difat: [FREE_SECTOR; 109],
    }
  }

  #[test]
  fn static_header_is_exactly_512_bytes() {
    let header = valid_header();
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    header.write_to(&mut writer).unwrap();
    let bytes = writer.into_inner().into_inner();
    assert_eq!(bytes.len(), HEADER_LEN);
    assert_eq!(Header::from_bytes(&bytes).unwrap(), header);
  }

  #[test]
  fn rejects_version_sector_shift_mismatch() {
    let mut header = valid_header();
    header.major_version = 4;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    assert!(header.write_to(&mut writer).is_err());
  }
}
