use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};

use super::Version;

/// A mutable stream in the fully materialized [`super::CompoundFile`] model.
///
/// This is the complete-functionality fallback for edits that do not need a
/// file-backed allocator. Growing the stream zero-fills the new range; the
/// compound writer chooses MiniFAT or FAT storage from the final length.
pub struct OwnedCfbStream<'a> {
  data: &'a mut Vec<u8>,
  position: u64,
  maximum_len: u64,
}

impl<'a> OwnedCfbStream<'a> {
  pub(crate) fn new(data: &'a mut Vec<u8>, version: Version) -> Self {
    Self {
      data,
      position: 0,
      maximum_len: match version {
        Version::V3 => 0x8000_0000,
        Version::V4 => usize::MAX as u64,
      },
    }
  }

  pub fn len(&self) -> u64 {
    self.data.len() as u64
  }

  pub fn is_empty(&self) -> bool {
    self.data.is_empty()
  }

  /// Truncates or extends the stream, zero-filling any newly exposed bytes.
  pub fn set_len(&mut self, len: u64) -> io::Result<()> {
    let len = self.checked_len(len)?;
    self.data.resize(len, 0);
    self.position = self.position.min(len as u64);
    Ok(())
  }

  fn checked_len(&self, len: u64) -> io::Result<usize> {
    if len > self.maximum_len {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("CFB stream length {len} exceeds the version limit"),
      ));
    }
    usize::try_from(len).map_err(|_| {
      io::Error::new(
        io::ErrorKind::InvalidInput,
        "CFB stream length does not fit usize",
      )
    })
  }
}

impl BufRead for OwnedCfbStream<'_> {
  fn fill_buf(&mut self) -> io::Result<&[u8]> {
    let position = usize::try_from(self.position)
      .map_err(|_| io::Error::other("CFB stream position does not fit usize"))?;
    Ok(&self.data[position..])
  }

  fn consume(&mut self, amount: usize) {
    debug_assert!(amount <= self.data.len() - self.position as usize);
    self.position += amount as u64;
  }
}

impl Read for OwnedCfbStream<'_> {
  fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
    let available = self.fill_buf()?;
    let count = available.len().min(output.len());
    output[..count].copy_from_slice(&available[..count]);
    self.consume(count);
    Ok(count)
  }
}

impl Seek for OwnedCfbStream<'_> {
  fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
    let candidate = match position {
      SeekFrom::Start(value) => i128::from(value),
      SeekFrom::End(delta) => i128::from(self.len()) + i128::from(delta),
      SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
    };
    if candidate < 0 || candidate > i128::from(self.len()) {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
          "cannot seek to {candidate}; CFB stream length is {}",
          self.len()
        ),
      ));
    }
    self.position = candidate as u64;
    Ok(self.position)
  }
}

impl Write for OwnedCfbStream<'_> {
  fn write(&mut self, input: &[u8]) -> io::Result<usize> {
    let end = self
      .position
      .checked_add(input.len() as u64)
      .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "stream end overflow"))?;
    let end = self.checked_len(end)?;
    let position = self.position as usize;
    if end > self.data.len() {
      self.data.resize(end, 0);
    }
    self.data[position..end].copy_from_slice(input);
    self.position = end as u64;
    Ok(input.len())
  }

  fn flush(&mut self) -> io::Result<()> {
    Ok(())
  }
}
