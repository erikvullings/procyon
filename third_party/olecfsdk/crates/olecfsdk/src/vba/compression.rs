//! Static representation and decompressor for MS-OVBA compressed containers.

use crate::{Error, Result, limits::Limits};

const SIGNATURE_BYTE: u8 = 0x01;
const CHUNK_SIGNATURE: u16 = 0b011;
const MAX_CHUNK_DATA: usize = 4096;
// Each group of eight literals needs one flag byte. 3,640 literals plus 455
// flag bytes fit in the 4,095-byte compressed chunk payload limit.
const MAX_LITERAL_CHUNK_OUTPUT: usize = 3640;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompressedContainer {
  pub signature: u8,
  pub chunks: Vec<CompressedChunk>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompressedChunk {
  pub header: CompressedChunkHeader,
  pub data: CompressedChunkData,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompressedChunkHeader {
  pub raw: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompressedChunkData {
  Raw(Vec<u8>),
  Compressed(Vec<TokenSequence>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenSequence {
  pub flag_byte: u8,
  pub tokens: Vec<Token>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
  Literal(u8),
  Copy(CopyToken),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopyToken {
  /// Exact packed value, retained for byte-identical serialization.
  pub raw: u16,
  pub offset: u16,
  pub length: u16,
}

impl CompressedChunkHeader {
  pub fn signature(self) -> u16 {
    (self.raw >> 12) & 0b111
  }

  pub fn is_compressed(self) -> bool {
    self.raw & 0x8000 != 0
  }

  pub fn chunk_size(self) -> usize {
    usize::from(self.raw & 0x0fff) + 3
  }

  pub fn data_size(self) -> usize {
    self.chunk_size() - 2
  }

  fn validate(self, offset: usize) -> Result<()> {
    if self.signature() != CHUNK_SIGNATURE {
      return Err(Error::invalid(
        offset as u64,
        "VBA compressed chunk signature must be 0b011",
      ));
    }
    if !self.is_compressed() && self.data_size() != MAX_CHUNK_DATA {
      return Err(Error::invalid(
        offset as u64,
        "uncompressed VBA chunk must contain 4096 bytes",
      ));
    }
    Ok(())
  }
}

impl CompressedContainer {
  pub fn from_uncompressed(bytes: &[u8]) -> Self {
    let chunks = bytes
      .chunks(MAX_LITERAL_CHUNK_OUTPUT)
      .map(|chunk| {
        let sequences = chunk
          .chunks(8)
          .map(|values| TokenSequence {
            flag_byte: 0,
            tokens: values.iter().copied().map(Token::Literal).collect(),
          })
          .collect::<Vec<_>>();
        let encoded_size = chunk.len() + sequences.len();
        debug_assert!((1..=4095).contains(&encoded_size));
        CompressedChunk {
          header: CompressedChunkHeader {
            raw: 0xb000 | (encoded_size as u16 - 1),
          },
          data: CompressedChunkData::Compressed(sequences),
        }
      })
      .collect();
    Self {
      signature: SIGNATURE_BYTE,
      chunks,
    }
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_bytes_with_limits(bytes, Limits::default())
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    if bytes.len() as u64 > limits.max_stream_size {
      return Err(Error::Limit(format!(
        "VBA compressed container length {} exceeds {}",
        bytes.len(),
        limits.max_stream_size
      )));
    }
    let (&signature, mut remaining) = bytes
      .split_first()
      .ok_or_else(|| Error::invalid(0, "missing VBA compression signature"))?;
    if signature != SIGNATURE_BYTE {
      return Err(Error::invalid(
        0,
        "VBA compressed container signature must be 0x01",
      ));
    }

    let mut chunks = Vec::new();
    let mut container_offset = 1usize;
    let mut decompressed_len = 0usize;
    while !remaining.is_empty() {
      if remaining.len() < 2 {
        return Err(Error::invalid(
          container_offset as u64,
          "truncated VBA compressed chunk header",
        ));
      }
      let header = CompressedChunkHeader {
        raw: u16::from_le_bytes([remaining[0], remaining[1]]),
      };
      header.validate(container_offset)?;
      let data_size = header.data_size();
      let chunk_size = header.chunk_size();
      if remaining.len() < chunk_size {
        return Err(Error::invalid(
          container_offset as u64,
          "truncated VBA compressed chunk",
        ));
      }
      let data_bytes = &remaining[2..chunk_size];
      let data = if header.is_compressed() {
        let (sequences, output_len) =
          parse_token_sequences(data_bytes, container_offset + 2, decompressed_len, limits)?;
        decompressed_len = output_len;
        CompressedChunkData::Compressed(sequences)
      } else {
        decompressed_len =
          checked_output_len(decompressed_len, data_size, limits, container_offset)?;
        CompressedChunkData::Raw(data_bytes.to_vec())
      };
      chunks.push(CompressedChunk { header, data });
      remaining = &remaining[chunk_size..];
      container_offset += chunk_size;
    }
    Ok(Self { signature, chunks })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    if self.signature != SIGNATURE_BYTE {
      return Err(Error::invalid(
        0,
        "VBA compressed container signature must be 0x01",
      ));
    }
    let mut bytes = vec![self.signature];
    for chunk in &self.chunks {
      let offset = bytes.len();
      chunk.header.validate(offset)?;
      let data = chunk.data.to_bytes(offset + 2)?;
      if data.len() != chunk.header.data_size() {
        return Err(Error::invalid(
          offset as u64,
          "VBA compressed chunk header/data length mismatch",
        ));
      }
      if chunk.header.is_compressed() != matches!(chunk.data, CompressedChunkData::Compressed(_)) {
        return Err(Error::invalid(
          offset as u64,
          "VBA compressed chunk flag/data kind mismatch",
        ));
      }
      bytes.extend_from_slice(&chunk.header.raw.to_le_bytes());
      bytes.extend_from_slice(&data);
    }
    Ok(bytes)
  }

  pub fn decompress(&self) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    for chunk in &self.chunks {
      let chunk_start = output.len();
      match &chunk.data {
        CompressedChunkData::Raw(bytes) => output.extend_from_slice(bytes),
        CompressedChunkData::Compressed(sequences) => {
          for sequence in sequences {
            for token in &sequence.tokens {
              match token {
                Token::Literal(value) => output.push(*value),
                Token::Copy(token) => copy_from_chunk(
                  &mut output,
                  chunk_start,
                  usize::from(token.offset),
                  usize::from(token.length),
                )?,
              }
              if output.len() - chunk_start > MAX_CHUNK_DATA {
                return Err(Error::invalid(
                  chunk_start as u64,
                  "VBA decompressed chunk exceeds 4096 bytes",
                ));
              }
            }
          }
        }
      }
    }
    Ok(output)
  }
}

impl CompressedChunkData {
  fn to_bytes(&self, offset: usize) -> Result<Vec<u8>> {
    match self {
      Self::Raw(bytes) => Ok(bytes.clone()),
      Self::Compressed(sequences) => {
        let mut bytes = Vec::new();
        for (sequence_index, sequence) in sequences.iter().enumerate() {
          // The normative prose requires at least one token, but the
          // decompression pseudocode permits a final flag-only sequence
          // and Office emits it in otherwise valid VBA projects.
          let is_final_flag_only =
            sequence.tokens.is_empty() && sequence_index + 1 == sequences.len();
          if (!is_final_flag_only && sequence.tokens.is_empty()) || sequence.tokens.len() > 8 {
            return Err(Error::invalid(
              offset as u64 + bytes.len() as u64,
              "VBA token sequence must contain 1 through 8 tokens unless it is a final flag-only sequence",
            ));
          }
          bytes.push(sequence.flag_byte);
          for (index, token) in sequence.tokens.iter().enumerate() {
            let flagged_copy = sequence.flag_byte & (1 << index) != 0;
            if flagged_copy != matches!(token, Token::Copy(_)) {
              return Err(Error::invalid(
                offset as u64 + bytes.len() as u64,
                "VBA flag byte/token kind mismatch",
              ));
            }
            match token {
              Token::Literal(value) => bytes.push(*value),
              Token::Copy(value) => bytes.extend_from_slice(&value.raw.to_le_bytes()),
            }
          }
        }
        Ok(bytes)
      }
    }
  }
}

fn parse_token_sequences(
  bytes: &[u8],
  absolute_offset: usize,
  prior_output_len: usize,
  limits: Limits,
) -> Result<(Vec<TokenSequence>, usize)> {
  let mut sequences = Vec::new();
  let mut cursor = 0usize;
  let mut chunk = Vec::new();
  while cursor < bytes.len() {
    let flag_byte = bytes[cursor];
    cursor += 1;
    let mut tokens = Vec::new();
    for index in 0..8 {
      if cursor >= bytes.len() {
        break;
      }
      let is_copy = flag_byte & (1 << index) != 0;
      if is_copy {
        if bytes.len() - cursor < 2 {
          return Err(Error::invalid(
            (absolute_offset + cursor) as u64,
            "truncated VBA copy token",
          ));
        }
        let raw = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]);
        cursor += 2;
        let (offset, length) = unpack_copy_token(raw, chunk.len());
        copy_from_chunk(&mut chunk, 0, offset, length)?;
        tokens.push(Token::Copy(CopyToken {
          raw,
          offset: offset as u16,
          length: length as u16,
        }));
      } else {
        chunk.push(bytes[cursor]);
        tokens.push(Token::Literal(bytes[cursor]));
        cursor += 1;
      }
      if chunk.len() > MAX_CHUNK_DATA {
        return Err(Error::invalid(
          absolute_offset as u64,
          "VBA decompressed chunk exceeds 4096 bytes",
        ));
      }
    }
    sequences.push(TokenSequence { flag_byte, tokens });
  }
  let output_len = checked_output_len(prior_output_len, chunk.len(), limits, absolute_offset)?;
  Ok((sequences, output_len))
}

fn unpack_copy_token(raw: u16, decompressed_position: usize) -> (usize, usize) {
  let bit_count = if decompressed_position <= 1 {
    4
  } else {
    (usize::BITS - (decompressed_position - 1).leading_zeros()).max(4) as usize
  };
  let length_mask = u16::MAX >> bit_count;
  let offset_mask = !length_mask;
  let length = usize::from(raw & length_mask) + 3;
  let offset = usize::from((raw & offset_mask) >> (16 - bit_count)) + 1;
  (offset, length)
}

fn copy_from_chunk(
  output: &mut Vec<u8>,
  chunk_start: usize,
  offset: usize,
  length: usize,
) -> Result<()> {
  let chunk_position = output
    .len()
    .checked_sub(chunk_start)
    .ok_or_else(|| Error::invalid(chunk_start as u64, "invalid VBA chunk position"))?;
  if offset == 0 || offset > chunk_position {
    return Err(Error::invalid(
      output.len() as u64,
      "VBA copy token offset exceeds decompressed chunk",
    ));
  }
  for _ in 0..length {
    let source = output.len() - offset;
    let value = output[source];
    output.push(value);
  }
  Ok(())
}

fn checked_output_len(
  current: usize,
  additional: usize,
  limits: Limits,
  offset: usize,
) -> Result<usize> {
  let value = current
    .checked_add(additional)
    .ok_or_else(|| Error::Limit("VBA decompressed length overflow".into()))?;
  if value as u64 > limits.max_stream_size || value > limits.max_allocation {
    return Err(Error::Limit(format!(
      "VBA decompressed data at {offset} exceeds configured limits"
    )));
  }
  Ok(value)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn compressed_copy_token_round_trips_and_decompresses() {
    // Three literals followed by an overlapping copy of "ABCABC".
    let bytes = [0x01, 0x05, 0xb0, 0x08, b'A', b'B', b'C', 0x03, 0x20];
    let container = CompressedContainer::from_bytes(&bytes).unwrap();
    assert_eq!(container.to_bytes().unwrap(), bytes);
    assert_eq!(container.decompress().unwrap(), b"ABCABCABC");
    let CompressedChunkData::Compressed(sequences) = &container.chunks[0].data else {
      panic!("expected a compressed chunk")
    };
    assert_eq!(sequences.len(), 1);
    assert_eq!(sequences[0].tokens.len(), 4);
    assert_eq!(
      sequences[0].tokens[3],
      Token::Copy(CopyToken {
        raw: 0x2003,
        offset: 3,
        length: 6,
      })
    );
  }

  #[test]
  fn literal_compressor_round_trips_across_chunk_boundaries() {
    for size in [0, 1, 8, 9, 3639, 3640, 3641, 8192] {
      let bytes: Vec<_> = (0..size).map(|index| index as u8).collect();
      let container = CompressedContainer::from_uncompressed(&bytes);
      let encoded = container.to_bytes().unwrap();
      let reparsed = CompressedContainer::from_bytes(&encoded).unwrap();
      assert_eq!(reparsed.decompress().unwrap(), bytes);
    }
  }

  #[test]
  fn raw_chunk_round_trips() {
    let mut bytes = vec![0x01, 0xff, 0x3f];
    bytes.extend((0..4096).map(|value| value as u8));
    let container = CompressedContainer::from_bytes(&bytes).unwrap();
    assert_eq!(container.to_bytes().unwrap(), bytes);
    assert_eq!(container.decompress().unwrap(), bytes[3..]);
  }

  #[test]
  fn malformed_containers_are_rejected() {
    assert!(CompressedContainer::from_bytes(&[]).is_err());
    assert!(CompressedContainer::from_bytes(&[0]).is_err());
    assert!(CompressedContainer::from_bytes(&[1, 5]).is_err());
    assert!(CompressedContainer::from_bytes(&[1, 5, 0xb0, 1, 0]).is_err());
    // A copy token cannot reference bytes before the current chunk.
    assert!(CompressedContainer::from_bytes(&[1, 2, 0xb0, 1, 0, 0]).is_err());
  }
}
