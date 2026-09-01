use std::{
  cell::Cell,
  io::{Read, Seek, SeekFrom, Write},
  sync::Arc,
};

use crate::{Error, Result};

use super::header::Header;

pub const MAX_REGULAR_SECTOR: u32 = 0xffff_fffa;

/// Positional read capability used by independent CFB stream cursors.
///
/// Unlike [`Read`] + [`Seek`], this operation does not mutate a shared file
/// position, so different stream cursors can read through the same backing
/// object without a global lock.
pub trait CfbReadAt {
  fn read_exact_at(&self, offset: u64, output: &mut [u8]) -> std::io::Result<()>;
}

fn read_slice_at(bytes: &[u8], offset: u64, output: &mut [u8]) -> std::io::Result<()> {
  if output.is_empty() {
    return Ok(());
  }
  let start = usize::try_from(offset)
    .map_err(|_| std::io::Error::other("CFB read offset does not fit usize"))?;
  let end = start
    .checked_add(output.len())
    .ok_or_else(|| std::io::Error::other("CFB read range overflow"))?;
  let input = bytes.get(start..end).ok_or_else(|| {
    std::io::Error::new(
      std::io::ErrorKind::UnexpectedEof,
      "positional CFB read extends beyond the backing object",
    )
  })?;
  output.copy_from_slice(input);
  Ok(())
}

impl CfbReadAt for [u8] {
  fn read_exact_at(&self, offset: u64, output: &mut [u8]) -> std::io::Result<()> {
    read_slice_at(self, offset, output)
  }
}

impl CfbReadAt for Vec<u8> {
  fn read_exact_at(&self, offset: u64, output: &mut [u8]) -> std::io::Result<()> {
    read_slice_at(self, offset, output)
  }
}

impl<T: AsRef<[u8]>> CfbReadAt for std::io::Cursor<T> {
  fn read_exact_at(&self, offset: u64, output: &mut [u8]) -> std::io::Result<()> {
    read_slice_at(self.get_ref().as_ref(), offset, output)
  }
}

impl<T: CfbReadAt + ?Sized> CfbReadAt for Arc<T> {
  fn read_exact_at(&self, offset: u64, output: &mut [u8]) -> std::io::Result<()> {
    (**self).read_exact_at(offset, output)
  }
}

#[cfg(unix)]
impl CfbReadAt for std::fs::File {
  fn read_exact_at(&self, offset: u64, mut output: &mut [u8]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;

    let mut position = offset;
    while !output.is_empty() {
      let count = self.read_at(output, position)?;
      if count == 0 {
        return Err(std::io::Error::new(
          std::io::ErrorKind::UnexpectedEof,
          "positional CFB file read reached EOF",
        ));
      }
      position = position
        .checked_add(count as u64)
        .ok_or_else(|| std::io::Error::other("CFB file read offset overflow"))?;
      output = &mut output[count..];
    }
    Ok(())
  }
}

#[cfg(windows)]
impl CfbReadAt for std::fs::File {
  fn read_exact_at(&self, offset: u64, mut output: &mut [u8]) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;

    let mut position = offset;
    while !output.is_empty() {
      let count = self.seek_read(output, position)?;
      if count == 0 {
        return Err(std::io::Error::new(
          std::io::ErrorKind::UnexpectedEof,
          "positional CFB file read reached EOF",
        ));
      }
      position = position
        .checked_add(count as u64)
        .ok_or_else(|| std::io::Error::other("CFB file read offset overflow"))?;
      output = &mut output[count..];
    }
    Ok(())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SectorId(u32);

impl SectorId {
  pub fn new(value: u32) -> Result<Self> {
    if value > MAX_REGULAR_SECTOR {
      return Err(Error::invalid(
        0,
        format!("invalid regular sector ID 0x{value:08x}"),
      ));
    }
    Ok(Self(value))
  }

  pub fn get(self) -> u32 {
    self.0
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MiniSectorId(u32);

impl MiniSectorId {
  pub fn new(value: u32) -> Result<Self> {
    if value > MAX_REGULAR_SECTOR {
      return Err(Error::invalid(
        0,
        format!("invalid mini-sector ID 0x{value:08x}"),
      ));
    }
    Ok(Self(value))
  }

  pub fn get(self) -> u32 {
    self.0
  }
}

pub(crate) trait SectorRead {
  type Sector<'a>: AsRef<[u8]>
  where
    Self: 'a;

  fn sector_count(&self) -> usize;
  fn sector_len(&self) -> usize;
  fn valid_len(&self, id: SectorId) -> usize;
  fn has_partial_sector(&self) -> bool;
  fn sector(&mut self, id: SectorId) -> Result<Self::Sector<'_>>;

  fn full_sector(&mut self, id: SectorId) -> Result<Self::Sector<'_>> {
    if self.valid_len(id) != self.sector_len() {
      return Err(Error::invalid(
        0,
        "truncated CFB allocation or directory sector",
      ));
    }
    self.sector(id)
  }
}

pub(crate) trait SectorWrite: SectorRead {
  fn write_header_at(&mut self, offset: usize, bytes: &[u8]) -> Result<()>;
  fn write_sector_at(&mut self, id: SectorId, offset: usize, bytes: &[u8]) -> Result<()>;
  fn zero_sector(&mut self, id: SectorId) -> Result<()>;
  fn append_zero_sector(&mut self) -> Result<SectorId>;
  fn flush(&mut self) -> Result<()>;
}

pub(crate) struct SectorSource<'a> {
  bytes: &'a [u8],
  sector_len: usize,
  sector_count: usize,
  partial_sector: Option<SectorId>,
  partial_len: usize,
  partial_accessed: Cell<bool>,
  partial_buffer: Vec<u8>,
}

impl<'a> SectorSource<'a> {
  pub(crate) fn new(bytes: &'a [u8], original_len: usize, header: &Header) -> Result<Self> {
    let sector_len = header.sector_len();
    if bytes.len() < sector_len {
      return Err(Error::invalid(
        0,
        "CFB file is shorter than its header sector",
      ));
    }
    if original_len > bytes.len() {
      return Err(Error::invalid(
        0,
        "internal CFB image length exceeds its backing bytes",
      ));
    }
    let padded_len = original_len
      .checked_add(sector_len - 1)
      .map(|len| len / sector_len * sector_len)
      .ok_or_else(|| Error::Limit("padded CFB length overflow".into()))?;
    let complete_sector_count = padded_len / sector_len;
    let partial_len = original_len % sector_len;
    let partial_sector = if partial_len == 0 {
      None
    } else {
      let id = complete_sector_count
        .checked_sub(2)
        .ok_or_else(|| Error::invalid(0, "partial CFB header sector"))?;
      Some(SectorId::new(id as u32)?)
    };
    Ok(Self {
      bytes,
      sector_len,
      sector_count: complete_sector_count - 1,
      partial_sector,
      partial_len,
      partial_accessed: Cell::new(false),
      partial_buffer: Vec::new(),
    })
  }

  pub(crate) fn sector_count(&self) -> usize {
    self.sector_count
  }

  pub(crate) fn sector_len(&self) -> usize {
    self.sector_len
  }

  pub(crate) fn sector(&mut self, id: SectorId) -> Result<&[u8]> {
    let index =
      usize::try_from(id.get()).map_err(|_| Error::invalid(0, "sector ID does not fit usize"))?;
    if index >= self.sector_count {
      return Err(Error::invalid(
        0,
        format!("sector {index} is beyond EOF ({})", self.sector_count),
      ));
    }
    let start = index
      .checked_add(1)
      .and_then(|value| value.checked_mul(self.sector_len))
      .ok_or_else(|| Error::invalid(0, "sector offset overflow"))?;
    let end = start
      .checked_add(self.sector_len)
      .ok_or_else(|| Error::invalid(0, "sector end overflow"))?;
    if self.partial_sector == Some(id) {
      self.partial_accessed.set(true);
      self.partial_buffer.resize(self.sector_len, 0);
      self.partial_buffer.fill(0);
      self.partial_buffer[..self.partial_len]
        .copy_from_slice(&self.bytes[start..start + self.partial_len]);
      return Ok(&self.partial_buffer);
    }
    Ok(&self.bytes[start..end])
  }

  pub(crate) fn full_sector(&mut self, id: SectorId) -> Result<&[u8]> {
    if self.valid_len(id) != self.sector_len {
      return Err(Error::invalid(
        0,
        "truncated CFB allocation or directory sector",
      ));
    }
    SectorSource::sector(self, id)
  }

  pub(crate) fn valid_len(&self, id: SectorId) -> usize {
    if self.partial_sector == Some(id) {
      self.partial_len
    } else {
      self.sector_len
    }
  }

  pub(crate) fn is_partial(&self, id: SectorId) -> bool {
    self.partial_sector == Some(id)
  }

  pub(crate) fn has_partial_sector(&self) -> bool {
    self.partial_sector.is_some()
  }

  pub(crate) fn unaccessed_partial_data(&self) -> &'a [u8] {
    if self.partial_accessed.get() || self.partial_len == 0 {
      return &[];
    }
    let start = self.sector_count * self.sector_len;
    &self.bytes[start..start + self.partial_len]
  }
}

impl SectorRead for SectorSource<'_> {
  type Sector<'a>
    = &'a [u8]
  where
    Self: 'a;

  fn sector_count(&self) -> usize {
    self.sector_count()
  }

  fn sector_len(&self) -> usize {
    self.sector_len()
  }

  fn valid_len(&self, id: SectorId) -> usize {
    self.valid_len(id)
  }

  fn has_partial_sector(&self) -> bool {
    self.has_partial_sector()
  }

  fn sector(&mut self, id: SectorId) -> Result<Self::Sector<'_>> {
    self.sector(id)
  }

  fn full_sector(&mut self, id: SectorId) -> Result<Self::Sector<'_>> {
    SectorSource::full_sector(self, id)
  }
}

pub(crate) struct SeekSectorSource<R> {
  reader: R,
  sector_len: usize,
  sector_count: usize,
  max_file_size: u64,
  partial_sector: Option<SectorId>,
  partial_len: usize,
  buffer: Vec<u8>,
}

pub(crate) struct ReadAtSectorSource<'a, R> {
  reader: &'a R,
  sector_len: usize,
  sector_count: usize,
  partial_sector: Option<SectorId>,
  partial_len: usize,
  buffer: Vec<u8>,
}

impl<R: CfbReadAt> ReadAtSectorSource<'_, R> {
  fn sector_offset(&self, id: SectorId) -> Result<u64> {
    let index = id.get() as usize;
    if index >= self.sector_count {
      return Err(Error::invalid(
        0,
        format!("sector {index} is beyond EOF ({})", self.sector_count),
      ));
    }
    u64::from(id.get())
      .checked_add(1)
      .and_then(|value| value.checked_mul(self.sector_len as u64))
      .ok_or_else(|| Error::invalid(0, "sector offset overflow"))
  }
}

impl<R: CfbReadAt> SectorRead for ReadAtSectorSource<'_, R> {
  type Sector<'a>
    = &'a [u8]
  where
    Self: 'a;

  fn sector_count(&self) -> usize {
    self.sector_count
  }

  fn sector_len(&self) -> usize {
    self.sector_len
  }

  fn valid_len(&self, id: SectorId) -> usize {
    if self.partial_sector == Some(id) {
      self.partial_len
    } else {
      self.sector_len
    }
  }

  fn has_partial_sector(&self) -> bool {
    self.partial_sector.is_some()
  }

  fn sector(&mut self, id: SectorId) -> Result<Self::Sector<'_>> {
    let offset = self.sector_offset(id)?;
    let valid_len = self.valid_len(id);
    self.buffer.fill(0);
    self
      .reader
      .read_exact_at(offset, &mut self.buffer[..valid_len])?;
    Ok(&self.buffer)
  }
}

impl<R: Read + Seek> SeekSectorSource<R> {
  pub(crate) fn new(
    reader: R,
    original_len: u64,
    header: &Header,
    max_file_size: u64,
  ) -> Result<Self> {
    let sector_len = header.sector_len();
    if original_len < sector_len as u64 {
      return Err(Error::invalid(
        0,
        "CFB file is shorter than its header sector",
      ));
    }
    let sector_len_u64 = sector_len as u64;
    let padded_len = original_len
      .checked_add(sector_len_u64 - 1)
      .map(|len| len / sector_len_u64 * sector_len_u64)
      .ok_or_else(|| Error::Limit("padded CFB length overflow".into()))?;
    let complete_sector_count = usize::try_from(padded_len / sector_len_u64)
      .map_err(|_| Error::Limit("CFB sector count does not fit usize".into()))?;
    let partial_len = usize::try_from(original_len % sector_len_u64)
      .map_err(|_| Error::Limit("partial sector length does not fit usize".into()))?;
    let partial_sector = if partial_len == 0 {
      None
    } else {
      let id = complete_sector_count
        .checked_sub(2)
        .ok_or_else(|| Error::invalid(0, "partial CFB header sector"))?;
      Some(SectorId::new(u32::try_from(id).map_err(|_| {
        Error::Limit("partial sector ID does not fit u32".into())
      })?)?)
    };
    Ok(Self {
      reader,
      sector_len,
      sector_count: complete_sector_count - 1,
      max_file_size,
      partial_sector,
      partial_len,
      buffer: vec![0; sector_len],
    })
  }

  pub(crate) fn into_inner(self) -> R {
    self.reader
  }

  pub(crate) fn read_at_source(&self) -> ReadAtSectorSource<'_, R>
  where
    R: CfbReadAt,
  {
    ReadAtSectorSource {
      reader: &self.reader,
      sector_len: self.sector_len,
      sector_count: self.sector_count,
      partial_sector: self.partial_sector,
      partial_len: self.partial_len,
      buffer: vec![0; self.sector_len],
    }
  }

  pub(crate) fn ensure_append_sector_count(&self, count: usize) -> Result<()> {
    if self.partial_sector.is_some() {
      return Err(Error::invalid(
        0,
        "cannot extend a CFB with trailing partial-sector data in place",
      ));
    }
    let new_sector_count = self
      .sector_count
      .checked_add(count)
      .ok_or_else(|| Error::Limit("CFB sector count overflow".into()))?;
    let new_file_size = u64::try_from(new_sector_count)
      .ok()
      .and_then(|count| count.checked_add(1))
      .and_then(|count| count.checked_mul(self.sector_len as u64))
      .ok_or_else(|| Error::Limit("CFB file length overflow".into()))?;
    if new_file_size > self.max_file_size {
      return Err(Error::Limit(format!(
        "file length {new_file_size} exceeds {}",
        self.max_file_size
      )));
    }
    Ok(())
  }

  fn sector_offset(&self, id: SectorId) -> Result<u64> {
    let index =
      usize::try_from(id.get()).map_err(|_| Error::invalid(0, "sector ID does not fit usize"))?;
    if index >= self.sector_count {
      return Err(Error::invalid(
        0,
        format!("sector {index} is beyond EOF ({})", self.sector_count),
      ));
    }
    let physical_index = index
      .checked_add(1)
      .ok_or_else(|| Error::invalid(0, "sector index overflow"))?;
    u64::try_from(physical_index)
      .ok()
      .and_then(|value| value.checked_mul(self.sector_len as u64))
      .ok_or_else(|| Error::invalid(0, "sector offset overflow"))
  }
}

impl<R: Read + Seek> SectorRead for SeekSectorSource<R> {
  type Sector<'a>
    = &'a [u8]
  where
    Self: 'a;

  fn sector_count(&self) -> usize {
    self.sector_count
  }

  fn sector_len(&self) -> usize {
    self.sector_len
  }

  fn valid_len(&self, id: SectorId) -> usize {
    if self.partial_sector == Some(id) {
      self.partial_len
    } else {
      self.sector_len
    }
  }

  fn has_partial_sector(&self) -> bool {
    self.partial_sector.is_some()
  }

  fn sector(&mut self, id: SectorId) -> Result<Self::Sector<'_>> {
    let offset = self.sector_offset(id)?;
    let valid_len = self.valid_len(id);
    self.buffer.fill(0);
    self.reader.seek(SeekFrom::Start(offset))?;
    self.reader.read_exact(&mut self.buffer[..valid_len])?;
    Ok(&self.buffer)
  }
}

impl<R: Read + Write + Seek> SectorWrite for SeekSectorSource<R> {
  fn write_header_at(&mut self, offset: usize, bytes: &[u8]) -> Result<()> {
    let end = offset
      .checked_add(bytes.len())
      .ok_or_else(|| Error::Limit("header write range overflow".into()))?;
    if end > super::header::HEADER_LEN {
      return Err(Error::invalid(0, "write extends beyond the CFB header"));
    }
    self.reader.seek(SeekFrom::Start(offset as u64))?;
    self.reader.write_all(bytes)?;
    Ok(())
  }

  fn write_sector_at(&mut self, id: SectorId, offset: usize, bytes: &[u8]) -> Result<()> {
    let end = offset
      .checked_add(bytes.len())
      .ok_or_else(|| Error::Limit("sector write range overflow".into()))?;
    if end > self.valid_len(id) {
      return Err(Error::invalid(
        0,
        "sector write extends beyond the physical sector data",
      ));
    }
    let physical = self
      .sector_offset(id)?
      .checked_add(offset as u64)
      .ok_or_else(|| Error::Limit("sector write offset overflow".into()))?;
    self.reader.seek(SeekFrom::Start(physical))?;
    self.reader.write_all(bytes)?;
    Ok(())
  }

  fn zero_sector(&mut self, id: SectorId) -> Result<()> {
    if self.valid_len(id) != self.sector_len {
      return Err(Error::invalid(
        0,
        "cannot allocate a partial physical sector",
      ));
    }
    let offset = self.sector_offset(id)?;
    self.reader.seek(SeekFrom::Start(offset))?;
    let zeros = vec![0; self.sector_len];
    self.reader.write_all(&zeros)?;
    Ok(())
  }

  fn append_zero_sector(&mut self) -> Result<SectorId> {
    self.ensure_append_sector_count(1)?;
    let raw = u32::try_from(self.sector_count)
      .map_err(|_| Error::Limit("appended sector ID does not fit u32".into()))?;
    let id = SectorId::new(raw)?;
    let offset = (self.sector_count as u64 + 1)
      .checked_mul(self.sector_len as u64)
      .ok_or_else(|| Error::Limit("appended sector offset overflow".into()))?;
    self.reader.seek(SeekFrom::Start(offset))?;
    let zeros = vec![0; self.sector_len];
    self.reader.write_all(&zeros)?;
    self.sector_count += 1;
    Ok(id)
  }

  fn flush(&mut self) -> Result<()> {
    self.reader.flush()?;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::cfb::header::{
    BYTE_ORDER_LE, FREE_SECTOR, MAGIC, MINI_SECTOR_SHIFT, MINI_STREAM_CUTOFF,
  };

  fn header() -> Header {
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
      number_of_fat_sectors: 0,
      first_directory_sector: FREE_SECTOR,
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
  fn checked_sector_addressing_excludes_header_sector() {
    let mut bytes = vec![0; 3 * 512];
    bytes[512] = 7;
    bytes[1024] = 11;
    let mut source = SectorSource::new(&bytes, bytes.len(), &header()).unwrap();
    assert_eq!(source.sector_count(), 2);
    assert_eq!(source.sector(SectorId::new(0).unwrap()).unwrap()[0], 7);
    assert_eq!(source.sector(SectorId::new(1).unwrap()).unwrap()[0], 11);
    assert!(source.sector(SectorId::new(2).unwrap()).is_err());
  }

  #[test]
  fn trailing_bytes_are_outside_the_sector_address_space() {
    let mut bytes = vec![0; 3 * 512];
    let original_len = 2 * 512 + 3;
    bytes[2 * 512..original_len].copy_from_slice(&[3, 5, 7]);
    let source = SectorSource::new(&bytes, original_len, &header()).unwrap();
    assert_eq!(source.sector_count(), 2);
    assert_eq!(source.unaccessed_partial_data(), [3, 5, 7]);
  }

  #[test]
  fn partial_sector_is_zero_padded_without_copying_the_complete_image() {
    let mut bytes = vec![0; 2 * 512 + 3];
    bytes[2 * 512..].copy_from_slice(&[3, 5, 7]);
    let mut source = SectorSource::new(&bytes, bytes.len(), &header()).unwrap();

    assert_eq!(source.sector_count(), 2);
    let partial = source.sector(SectorId::new(1).unwrap()).unwrap();
    assert_eq!(&partial[..3], &[3, 5, 7]);
    assert!(partial[3..].iter().all(|byte| *byte == 0));
    assert!(source.unaccessed_partial_data().is_empty());
  }
}
