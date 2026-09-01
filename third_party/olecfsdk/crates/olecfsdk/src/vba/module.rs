//! MS-OVBA module stream, split at the `MODULEOFFSET.TextOffset` boundary.

use crate::{Error, Result, limits::Limits};

use super::compression::CompressedContainer;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleStream {
  /// Implementation-specific, version-dependent bytes. MS-OVBA requires
  /// readers to ignore them; they remain explicitly bounded for round-trip.
  pub performance_cache: Vec<u8>,
  pub compressed_source_code: CompressedContainer,
}

impl ModuleStream {
  pub fn from_bytes(bytes: &[u8], text_offset: u32) -> Result<Self> {
    Self::from_bytes_with_limits(bytes, text_offset, Limits::default())
  }

  pub fn from_bytes_with_limits(bytes: &[u8], text_offset: u32, limits: Limits) -> Result<Self> {
    if bytes.len() as u64 > limits.max_stream_size {
      return Err(Error::Limit(format!(
        "VBA module stream length {} exceeds {}",
        bytes.len(),
        limits.max_stream_size
      )));
    }
    let offset = usize::try_from(text_offset)
      .map_err(|_| Error::Limit("VBA module text offset does not fit usize".into()))?;
    let source = bytes.get(offset..).ok_or_else(|| {
      Error::invalid(
        text_offset as u64,
        "VBA module text offset exceeds stream length",
      )
    })?;
    Ok(Self {
      performance_cache: bytes[..offset].to_vec(),
      compressed_source_code: CompressedContainer::from_bytes_with_limits(source, limits)?,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = self.performance_cache.clone();
    bytes.extend_from_slice(&self.compressed_source_code.to_bytes()?);
    Ok(bytes)
  }

  pub fn to_interoperable_bytes(&self) -> Result<Vec<u8>> {
    self.compressed_source_code.to_bytes()
  }

  pub fn source_bytes(&self) -> Result<Vec<u8>> {
    self.compressed_source_code.decompress()
  }

  pub fn replace_source_bytes(&mut self, source: &[u8]) -> Result<Vec<u8>> {
    let previous = self.source_bytes()?;
    self.compressed_source_code = CompressedContainer::from_uncompressed(source);
    self.performance_cache.clear();
    Ok(previous)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn module_cache_and_source_round_trip() {
    let mut bytes = vec![0xaa, 0xbb];
    bytes.extend_from_slice(&[1, 4, 0xb0, 0, b'A', b'B', b'C', b'D']);
    let module = ModuleStream::from_bytes(&bytes, 2).unwrap();
    assert_eq!(module.performance_cache, [0xaa, 0xbb]);
    assert_eq!(module.source_bytes().unwrap(), b"ABCD");
    assert_eq!(module.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn invalid_text_offset_is_rejected() {
    assert!(ModuleStream::from_bytes(&[1], 2).is_err());
  }

  #[test]
  fn replacing_source_recompresses_and_invalidates_cache() {
    let mut bytes = vec![0xaa, 0xbb];
    bytes.extend_from_slice(
      &CompressedContainer::from_uncompressed(b"old")
        .to_bytes()
        .unwrap(),
    );
    let mut module = ModuleStream::from_bytes(&bytes, 2).unwrap();
    assert_eq!(module.replace_source_bytes(b"new source").unwrap(), b"old");
    assert!(module.performance_cache.is_empty());
    assert_eq!(module.source_bytes().unwrap(), b"new source");
    assert_eq!(
      ModuleStream::from_bytes(&module.to_bytes().unwrap(), 0)
        .unwrap()
        .source_bytes()
        .unwrap(),
      b"new source"
    );
  }
}
