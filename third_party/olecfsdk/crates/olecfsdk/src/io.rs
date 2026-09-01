use std::io::{Read, Seek, SeekFrom, Write};

use crate::{Error, Result, limits::Limits};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BinaryFormat {
  #[default]
  Unknown,
  Cfb,
  PropertySet,
  Vba,
  Ograph,
  Xls,
  Ppt,
  Doc,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParseMode {
  #[default]
  Strict,
  Compatible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IoContext {
  pub format: BinaryFormat,
  pub version: u32,
  pub code_page: Option<u16>,
  pub mode: ParseMode,
  pub limits: Limits,
}

impl Default for IoContext {
  fn default() -> Self {
    Self {
      format: BinaryFormat::Unknown,
      version: 0,
      code_page: None,
      mode: ParseMode::Strict,
      limits: Limits::default(),
    }
  }
}

pub trait SdkRead: Sized {
  fn read_from<R: Read + Seek>(reader: &mut Reader<R>) -> Result<Self>;
}

pub trait SdkWrite {
  fn write_to<W: Write>(&self, writer: &mut Writer<W>) -> Result<()>;
}

pub trait SdkSize {
  fn sdk_size(&self) -> u64;
}

pub trait SdkEnumValue: Copy {
  type Repr: Copy + std::fmt::Display;
  fn from_raw(value: Self::Repr) -> Option<Self>;
  fn raw(self) -> Self::Repr;
}

pub struct Reader<R> {
  inner: R,
  start: u64,
  end: u64,
  context: IoContext,
}

impl<R: Read + Seek> Reader<R> {
  pub fn new(mut inner: R) -> Result<Self> {
    let start = inner.stream_position()?;
    let end = inner.seek(SeekFrom::End(0))?;
    inner.seek(SeekFrom::Start(start))?;
    Ok(Self {
      inner,
      start,
      end,
      context: IoContext::default(),
    })
  }

  pub fn with_context(mut inner: R, context: IoContext) -> Result<Self> {
    let start = inner.stream_position()?;
    let end = inner.seek(SeekFrom::End(0))?;
    inner.seek(SeekFrom::Start(start))?;
    Ok(Self {
      inner,
      start,
      end,
      context,
    })
  }

  pub fn with_bounds(mut inner: R, start: u64, len: u64) -> Result<Self> {
    let end = start
      .checked_add(len)
      .ok_or_else(|| Error::invalid(start, "reader bounds overflow"))?;
    let actual_end = inner.seek(SeekFrom::End(0))?;
    if end > actual_end {
      return Err(Error::invalid(start, "reader bounds exceed input"));
    }
    inner.seek(SeekFrom::Start(start))?;
    Ok(Self {
      inner,
      start,
      end,
      context: IoContext::default(),
    })
  }

  pub fn position(&mut self) -> Result<u64> {
    Ok(self.inner.stream_position()?)
  }

  pub fn remaining(&mut self) -> Result<u64> {
    let position = self.position()?;
    self
      .end
      .checked_sub(position)
      .ok_or_else(|| Error::invalid(position, "reader moved beyond its bounds"))
  }

  pub fn seek_to(&mut self, position: u64) -> Result<()> {
    if position < self.start || position > self.end {
      return Err(Error::invalid(
        position,
        "seek position is outside bounded input",
      ));
    }
    self.inner.seek(SeekFrom::Start(position))?;
    Ok(())
  }

  pub fn start(&self) -> u64 {
    self.start
  }

  pub fn end(&self) -> u64 {
    self.end
  }

  pub fn context(&self) -> &IoContext {
    &self.context
  }

  pub fn context_mut(&mut self) -> &mut IoContext {
    &mut self.context
  }

  pub fn sub_reader(&mut self, len: u64) -> Result<Reader<&mut R>> {
    let start = self.position()?;
    if len > self.remaining()? {
      return Err(Error::invalid(start, "sub-reader exceeds bounded input"));
    }
    let end = start
      .checked_add(len)
      .ok_or_else(|| Error::invalid(start, "sub-reader end overflow"))?;
    Ok(Reader {
      inner: &mut self.inner,
      start,
      end,
      context: self.context,
    })
  }

  pub fn read_vec(&mut self, len: usize) -> Result<Vec<u8>> {
    self.ensure_allocation(len, 1)?;
    let mut value = vec![0; len];
    self.read_exact(&mut value)?;
    Ok(value)
  }

  pub fn ensure_allocation(&self, count: usize, element_size: usize) -> Result<()> {
    let bytes = count
      .checked_mul(element_size)
      .ok_or_else(|| Error::Limit("binary allocation size overflow".into()))?;
    if bytes > self.context.limits.max_allocation {
      return Err(Error::Limit(format!(
        "binary allocation {bytes} exceeds {}",
        self.context.limits.max_allocation
      )));
    }
    Ok(())
  }

  pub fn read_alignment(&mut self, alignment: usize) -> Result<Vec<u8>> {
    if alignment == 0 || !alignment.is_power_of_two() {
      return Err(Error::invalid(
        self.position()?,
        "alignment must be a power of two",
      ));
    }
    let position = usize::try_from(self.position()?)
      .map_err(|_| Error::Limit("reader position does not fit usize".into()))?;
    let padding = position.next_multiple_of(alignment) - position;
    self.read_vec(padding)
  }

  pub fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
    let mut value = [0; N];
    self.read_exact(&mut value)?;
    Ok(value)
  }

  fn ensure(&mut self, len: usize) -> Result<()> {
    if self.remaining()? < len as u64 {
      return Err(Error::invalid(self.position()?, "truncated bounded input"));
    }
    Ok(())
  }

  pub fn read_u8(&mut self) -> Result<u8> {
    Ok(self.read_array::<1>()?[0])
  }
  pub fn read_i8(&mut self) -> Result<i8> {
    Ok(self.read_u8()? as i8)
  }
  pub fn read_u16(&mut self) -> Result<u16> {
    Ok(u16::from_le_bytes(self.read_array()?))
  }
  pub fn read_i16(&mut self) -> Result<i16> {
    Ok(i16::from_le_bytes(self.read_array()?))
  }
  pub fn read_u32(&mut self) -> Result<u32> {
    Ok(u32::from_le_bytes(self.read_array()?))
  }
  pub fn read_i32(&mut self) -> Result<i32> {
    Ok(i32::from_le_bytes(self.read_array()?))
  }
  pub fn read_u64(&mut self) -> Result<u64> {
    Ok(u64::from_le_bytes(self.read_array()?))
  }
  pub fn read_i64(&mut self) -> Result<i64> {
    Ok(i64::from_le_bytes(self.read_array()?))
  }
  pub fn read_f32(&mut self) -> Result<f32> {
    Ok(f32::from_le_bytes(self.read_array()?))
  }
  pub fn read_f64(&mut self) -> Result<f64> {
    Ok(f64::from_le_bytes(self.read_array()?))
  }
}

impl<R: Read + Seek> Read for Reader<R> {
  fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
    let position = self.inner.stream_position()?;
    let remaining = self.end.saturating_sub(position);
    let allowed = usize::try_from(remaining.min(buf.len() as u64)).unwrap_or(buf.len());
    self.inner.read(&mut buf[..allowed])
  }

  fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
    self.ensure(buf.len()).map_err(std::io::Error::other)?;
    self.inner.read_exact(buf)
  }
}

pub struct Writer<W> {
  inner: W,
  position: u64,
  context: IoContext,
}

impl<W: Write> Writer<W> {
  pub fn new(inner: W) -> Self {
    Self {
      inner,
      position: 0,
      context: IoContext::default(),
    }
  }
  pub fn with_position(inner: W, position: u64) -> Self {
    Self {
      inner,
      position,
      context: IoContext::default(),
    }
  }
  pub fn with_context(inner: W, context: IoContext) -> Self {
    Self {
      inner,
      position: 0,
      context,
    }
  }
  pub fn position(&self) -> Result<u64> {
    Ok(self.position)
  }
  pub fn into_inner(self) -> W {
    self.inner
  }
  pub fn context(&self) -> &IoContext {
    &self.context
  }
  pub fn alignment_padding(&mut self, alignment: usize) -> Result<usize> {
    if alignment == 0 || !alignment.is_power_of_two() {
      return Err(Error::invalid(
        self.position()?,
        "alignment must be a power of two",
      ));
    }
    let position = usize::try_from(self.position()?)
      .map_err(|_| Error::Limit("writer position does not fit usize".into()))?;
    Ok(position.next_multiple_of(alignment) - position)
  }
  pub fn write_alignment(&mut self, alignment: usize, value: u8) -> Result<usize> {
    let padding = self.alignment_padding(alignment)?;
    self.write_all(&vec![value; padding])?;
    Ok(padding)
  }
  pub fn write_u8(&mut self, value: u8) -> Result<()> {
    Ok(self.write_all(&[value])?)
  }
  pub fn write_i8(&mut self, value: i8) -> Result<()> {
    self.write_u8(value as u8)
  }
  pub fn write_u16(&mut self, value: u16) -> Result<()> {
    Ok(self.write_all(&value.to_le_bytes())?)
  }
  pub fn write_i16(&mut self, value: i16) -> Result<()> {
    Ok(self.write_all(&value.to_le_bytes())?)
  }
  pub fn write_u32(&mut self, value: u32) -> Result<()> {
    Ok(self.write_all(&value.to_le_bytes())?)
  }
  pub fn write_i32(&mut self, value: i32) -> Result<()> {
    Ok(self.write_all(&value.to_le_bytes())?)
  }
  pub fn write_u64(&mut self, value: u64) -> Result<()> {
    Ok(self.write_all(&value.to_le_bytes())?)
  }
  pub fn write_i64(&mut self, value: i64) -> Result<()> {
    Ok(self.write_all(&value.to_le_bytes())?)
  }
  pub fn write_f32(&mut self, value: f32) -> Result<()> {
    Ok(self.write_all(&value.to_le_bytes())?)
  }
  pub fn write_f64(&mut self, value: f64) -> Result<()> {
    Ok(self.write_all(&value.to_le_bytes())?)
  }
}

impl<W: Write> Write for Writer<W> {
  fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
    let requested = u64::try_from(buf.len())
      .map_err(|_| std::io::Error::other("writer request length does not fit u64"))?;
    self
      .position
      .checked_add(requested)
      .ok_or_else(|| std::io::Error::other("writer position overflow"))?;
    let written = self.inner.write(buf)?;
    self.position += written as u64;
    Ok(written)
  }
  fn flush(&mut self) -> std::io::Result<()> {
    self.inner.flush()
  }
}

#[cfg(test)]
mod tests {
  use std::io::Cursor;

  use super::*;
  use crate::{SdkBitfield, SdkEnum, SdkObject};

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  struct Header {
    a: u16,
    b: u32,
    raw: [u8; 3],
    sectors: [u32; 3],
  }

  #[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
  #[sdk(repr = "u16")]
  enum Kind {
    One = 1,
    Two = 2,
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  struct CountedValues {
    count: u16,
    #[sdk(count = "count")]
    values: Vec<u32>,
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  struct LengthPrefixedValues {
    #[sdk(count_prefix = "u16")]
    values: Vec<u32>,
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  #[sdk(size_prefix = "u16")]
  struct SizePrefixedObject {
    tag: u16,
    value: u32,
  }

  #[derive(Debug, PartialEq, Eq)]
  struct MisreportedSize;

  impl SdkRead for MisreportedSize {
    fn read_from<R: Read + Seek>(reader: &mut Reader<R>) -> Result<Self> {
      reader.read_u16()?;
      Ok(Self)
    }
  }

  impl SdkWrite for MisreportedSize {
    fn write_to<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
      writer.write_u16(0)
    }
  }

  impl SdkSize for MisreportedSize {
    fn sdk_size(&self) -> u64 {
      1
    }
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  #[sdk(size_prefix = "u16")]
  struct SizePrefixedCustomObject {
    value: MisreportedSize,
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  #[sdk(size_prefix = "u8")]
  struct TinySizePrefixedObject {
    #[sdk(remaining)]
    bytes: Vec<u8>,
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  struct ConditionalAndPadding {
    flags: u16,
    #[sdk(condition = "flags", mask = 0x0001)]
    extra: Option<u32>,
    payload_len: u16,
    #[sdk(count = "payload_len")]
    payload: Vec<u8>,
    #[sdk(align = 4)]
    padding: Vec<u8>,
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  struct RemainingBytes {
    tag: u16,
    #[sdk(remaining)]
    tail: Vec<u8>,
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  struct RemainingWords {
    tag: u8,
    #[sdk(remaining)]
    values: Vec<u16>,
  }

  #[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
  struct FixedPair {
    first: u16,
    second: u16,
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  struct MinimumSizedPairs {
    #[sdk(count_prefix = "u16", min_element_size = 4)]
    values: Vec<FixedPair>,
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  struct RemainingFixedPairs {
    tag: u8,
    #[sdk(remaining(element_size = 4))]
    values: Vec<FixedPair>,
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  struct OptionalRemainingWord {
    tag: u16,
    #[sdk(optional_remaining)]
    value: Option<u32>,
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  struct OptionalRemainingWords {
    tag: u16,
    #[sdk(optional_remaining)]
    values: Option<[u16; 3]>,
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  struct OptionalTailWords {
    tag: u16,
    #[sdk(optional)]
    first: Option<u16>,
    #[sdk(optional)]
    second: Option<u16>,
    #[sdk(optional)]
    third: Option<u16>,
  }

  bitflags::bitflags! {
      #[derive(Clone, Copy, Debug, PartialEq, Eq)]
      struct TestFlags: u16 {
          const KNOWN = 0x0001;
      }
  }

  #[derive(Debug, PartialEq, Eq, SdkObject)]
  struct Flagged {
    #[sdk(bitflags = "u16")]
    flags: TestFlags,
  }

  #[derive(Clone, Copy, Debug, PartialEq, Eq, SdkBitfield)]
  #[sdk(repr = "u16", validate = validate_packed_options)]
  struct PackedOptions {
    #[sdk(bits = 0..=2)]
    kind: u8,
    #[sdk(bit = 3)]
    enabled: bool,
    #[sdk(bits = 8..=15)]
    producer_data: u8,
  }

  fn validate_packed_options(value: &PackedOptions, offset: u64) -> Result<()> {
    if value.kind > 5 {
      return Err(Error::invalid(
        offset,
        "packed kind is outside its specification",
      ));
    }
    Ok(())
  }

  #[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
  #[sdk(validate_at = "validate_positioned_object")]
  struct PositionedObject {
    value: u8,
  }

  fn validate_positioned_object(value: &PositionedObject, offset: u64) -> Result<()> {
    if value.value != 0 {
      return Err(Error::invalid(offset, "positioned object must be zero"));
    }
    Ok(())
  }

  #[test]
  fn derived_binary_round_trip() {
    let value = Header {
      a: 7,
      b: 11,
      raw: [1, 2, 3],
      sectors: [13, 17, u32::MAX],
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    value.write_to(&mut writer).unwrap();
    Kind::Two.write_to(&mut writer).unwrap();
    assert_eq!(value.sdk_size(), 21);
    let bytes = writer.into_inner().into_inner();
    let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
    assert_eq!(Header::read_from(&mut reader).unwrap(), value);
    assert_eq!(Kind::read_from(&mut reader).unwrap(), Kind::Two);
  }

  #[test]
  fn derive_optional_remaining_requires_zero_or_one_complete_value() {
    let absent = OptionalRemainingWord {
      tag: 7,
      value: None,
    };
    let present = OptionalRemainingWord {
      tag: 7,
      value: Some(0x1122_3344),
    };

    for value in [&absent, &present] {
      let mut writer = Writer::new(Cursor::new(Vec::new()));
      value.write_to(&mut writer).unwrap();
      let bytes = writer.into_inner().into_inner();
      assert_eq!(value.sdk_size(), bytes.len() as u64);
      let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
      assert_eq!(
        OptionalRemainingWord::read_from(&mut reader).unwrap(),
        *value
      );
    }

    for bytes in [[7, 0, 1].as_slice(), [7, 0, 1, 2, 3, 4, 5].as_slice()] {
      let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
      assert!(OptionalRemainingWord::read_from(&mut reader).is_err());
    }

    let words = OptionalRemainingWords {
      tag: 9,
      values: Some([0x1122, 0x3344, 0x5566]),
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    words.write_to(&mut writer).unwrap();
    let bytes = writer.into_inner().into_inner();
    assert_eq!(bytes, [9, 0, 0x22, 0x11, 0x44, 0x33, 0x66, 0x55]);
    assert_eq!(words.sdk_size(), bytes.len() as u64);
    let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
    assert_eq!(
      OptionalRemainingWords::read_from(&mut reader).unwrap(),
      words
    );

    for bytes in [
      [9, 0, 1, 2].as_slice(),
      [9, 0, 1, 2, 3, 4, 5, 6, 7, 8].as_slice(),
    ] {
      let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
      assert!(OptionalRemainingWords::read_from(&mut reader).is_err());
    }
  }

  #[test]
  fn derive_remaining_fixed_layout_objects_is_bounded_per_element() {
    let value = RemainingFixedPairs {
      tag: 7,
      values: vec![
        FixedPair {
          first: 0x1122,
          second: 0x3344,
        },
        FixedPair {
          first: 0x5566,
          second: 0x7788,
        },
      ],
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    value.write_to(&mut writer).unwrap();
    let bytes = writer.into_inner().into_inner();
    assert_eq!(value.sdk_size(), 9);
    assert_eq!(bytes, [7, 0x22, 0x11, 0x44, 0x33, 0x66, 0x55, 0x88, 0x77]);
    let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
    assert_eq!(RemainingFixedPairs::read_from(&mut reader).unwrap(), value);

    let mut partial = Reader::new(Cursor::new([7, 1, 2, 3])).unwrap();
    assert!(RemainingFixedPairs::read_from(&mut partial).is_err());
  }

  #[test]
  fn derive_optional_suffix_preserves_each_present_prefix_field() {
    for value in [
      OptionalTailWords {
        tag: 9,
        first: None,
        second: None,
        third: None,
      },
      OptionalTailWords {
        tag: 9,
        first: Some(0x1122),
        second: None,
        third: None,
      },
      OptionalTailWords {
        tag: 9,
        first: Some(0x1122),
        second: Some(0x3344),
        third: None,
      },
      OptionalTailWords {
        tag: 9,
        first: Some(0x1122),
        second: Some(0x3344),
        third: Some(0x5566),
      },
    ] {
      let mut writer = Writer::new(Cursor::new(Vec::new()));
      value.write_to(&mut writer).unwrap();
      let bytes = writer.into_inner().into_inner();
      assert_eq!(value.sdk_size(), bytes.len() as u64);
      let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
      assert_eq!(OptionalTailWords::read_from(&mut reader).unwrap(), value);
    }

    let gap = OptionalTailWords {
      tag: 9,
      first: None,
      second: Some(0x3344),
      third: None,
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    assert!(gap.write_to(&mut writer).is_err());

    for bytes in [[9, 0, 1].as_slice(), [9, 0, 1, 2, 3, 4, 5, 6, 7].as_slice()] {
      let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
      assert!(OptionalTailWords::read_from(&mut reader).is_err());
    }
  }

  #[test]
  fn derive_bitflags_retains_unknown_bits() {
    let bytes = [0x01, 0x80];
    let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
    let value = Flagged::read_from(&mut reader).unwrap();
    assert_eq!(value.flags.bits(), 0x8001);
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    value.write_to(&mut writer).unwrap();
    assert_eq!(writer.into_inner().into_inner(), bytes);
  }

  #[test]
  fn derive_generates_spec_bounded_bitfields() {
    let bytes = [0x0d, 0xa5];
    let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
    let value = PackedOptions::read_from(&mut reader).unwrap();
    assert_eq!(
      value,
      PackedOptions {
        kind: 5,
        enabled: true,
        producer_data: 0xa5,
      }
    );
    assert_eq!(value.sdk_size(), 2);
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    value.write_to(&mut writer).unwrap();
    assert_eq!(writer.into_inner().into_inner(), bytes);

    let mut reserved = Reader::new(Cursor::new([0x10, 0])).unwrap();
    assert!(PackedOptions::read_from(&mut reserved).is_err());
    let too_wide = PackedOptions {
      kind: 8,
      enabled: false,
      producer_data: 0,
    };
    assert!(
      too_wide
        .write_to(&mut Writer::new(Cursor::new(Vec::new())))
        .is_err()
    );
    let invalid_by_spec = PackedOptions {
      kind: 6,
      enabled: false,
      producer_data: 0,
    };
    assert!(
      invalid_by_spec
        .write_to(&mut Writer::new(Cursor::new(Vec::new())))
        .is_err()
    );
  }

  #[test]
  fn object_validator_can_receive_the_physical_start_offset() {
    let mut reader = Reader::with_bounds(Cursor::new([0, 0, 7]), 2, 1).unwrap();
    let read_error = PositionedObject::read_from(&mut reader).unwrap_err();
    assert_eq!(read_error.offset(), Some(2));

    let mut output = Cursor::new(vec![0, 0]);
    output.set_position(2);
    let mut writer = Writer::with_position(output, 2);
    let write_error = PositionedObject { value: 7 }
      .write_to(&mut writer)
      .unwrap_err();
    assert_eq!(write_error.offset(), Some(2));
  }

  #[test]
  fn writer_tracks_position_without_a_seekable_sink() {
    let value = Header {
      a: 1,
      b: 2,
      raw: [3, 4, 5],
      sectors: [6, 7, 8],
    };
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    value.write_to(&mut writer).unwrap();
    assert_eq!(writer.position().unwrap(), value.sdk_size());
    assert_eq!(writer.into_inner().len() as u64, value.sdk_size());
  }

  #[test]
  fn derive_reads_and_validates_counted_vectors() {
    let value = CountedValues {
      count: 3,
      values: vec![7, 11, 13],
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    value.write_to(&mut writer).unwrap();
    assert_eq!(value.sdk_size(), 14);
    let mut reader = Reader::new(Cursor::new(writer.into_inner().into_inner())).unwrap();
    assert_eq!(CountedValues::read_from(&mut reader).unwrap(), value);

    let invalid = CountedValues {
      count: 2,
      values: vec![1],
    };
    assert!(
      invalid
        .write_to(&mut Writer::new(Cursor::new(Vec::new())))
        .is_err()
    );
  }

  #[test]
  fn derive_generates_vector_count_prefixes() {
    let value = LengthPrefixedValues {
      values: vec![7, 11, 13],
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    value.write_to(&mut writer).unwrap();
    let bytes = writer.into_inner().into_inner();
    assert_eq!(bytes, [3, 0, 7, 0, 0, 0, 11, 0, 0, 0, 13, 0, 0, 0]);
    assert_eq!(value.sdk_size(), 14);

    let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
    assert_eq!(LengthPrefixedValues::read_from(&mut reader).unwrap(), value);
    assert_eq!(reader.remaining().unwrap(), 0);

    let too_many = LengthPrefixedValues {
      values: vec![0; usize::from(u16::MAX) + 1],
    };
    assert!(
      too_many
        .write_to(&mut Writer::new(Cursor::new(Vec::new())))
        .is_err()
    );

    let pairs = MinimumSizedPairs {
      values: vec![FixedPair {
        first: 1,
        second: 2,
      }],
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    pairs.write_to(&mut writer).unwrap();
    let bytes = writer.into_inner().into_inner();
    assert_eq!(bytes, [1, 0, 1, 0, 2, 0]);
    let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
    assert_eq!(MinimumSizedPairs::read_from(&mut reader).unwrap(), pairs);

    let mut impossible_count = Reader::new(Cursor::new([2, 0, 1, 0, 2, 0])).unwrap();
    assert!(MinimumSizedPairs::read_from(&mut impossible_count).is_err());
  }

  #[test]
  fn derive_size_prefix_bounds_the_complete_object_payload() {
    let value = SizePrefixedObject {
      tag: 0x1122,
      value: 0x3344_5566,
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    value.write_to(&mut writer).unwrap();
    let bytes = writer.into_inner().into_inner();
    assert_eq!(bytes, [6, 0, 0x22, 0x11, 0x66, 0x55, 0x44, 0x33]);
    assert_eq!(value.sdk_size(), 8);

    let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
    assert_eq!(SizePrefixedObject::read_from(&mut reader).unwrap(), value);
    assert_eq!(reader.remaining().unwrap(), 0);

    for bytes in [
      [5, 0, 0x22, 0x11, 0x66, 0x55, 0x44, 0x33].as_slice(),
      [7, 0, 0x22, 0x11, 0x66, 0x55, 0x44, 0x33].as_slice(),
      [7, 0, 0x22, 0x11, 0x66, 0x55, 0x44, 0x33, 0].as_slice(),
    ] {
      let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
      assert!(SizePrefixedObject::read_from(&mut reader).is_err());
    }

    assert!(
      SizePrefixedCustomObject {
        value: MisreportedSize,
      }
      .write_to(&mut Writer::new(Cursor::new(Vec::new())))
      .is_err()
    );
    assert!(
      TinySizePrefixedObject {
        bytes: vec![0; usize::from(u8::MAX) + 1],
      }
      .write_to(&mut Writer::new(Cursor::new(Vec::new())))
      .is_err()
    );
  }

  #[test]
  fn context_and_sub_reader_keep_hard_bounds() {
    let context = IoContext {
      format: BinaryFormat::Xls,
      version: 8,
      limits: Limits {
        max_allocation: 4,
        ..Limits::default()
      },
      ..IoContext::default()
    };
    let mut reader = Reader::with_context(Cursor::new(vec![1, 2, 3, 4, 5]), context).unwrap();
    {
      let mut child = reader.sub_reader(3).unwrap();
      assert_eq!(child.context().format, BinaryFormat::Xls);
      assert_eq!(child.read_vec(3).unwrap(), [1, 2, 3]);
      assert!(child.read_u8().is_err());
    }
    assert_eq!(reader.read_u8().unwrap(), 4);
    assert!(reader.read_vec(5).is_err());
  }

  #[test]
  fn bounded_seek_cannot_escape_reader_limits() {
    let mut reader = Reader::with_bounds(Cursor::new(vec![1, 2, 3, 4, 5]), 1, 3).unwrap();
    assert_eq!(reader.read_u8().unwrap(), 2);
    reader.seek_to(3).unwrap();
    assert_eq!(reader.read_u8().unwrap(), 4);
    assert!(reader.seek_to(0).is_err());
    assert!(reader.seek_to(5).is_err());
    reader.seek_to(4).unwrap();
    assert_eq!(reader.remaining().unwrap(), 0);
  }

  #[test]
  fn derive_supports_conditions_and_preserved_alignment() {
    let value = ConditionalAndPadding {
      flags: 1,
      extra: Some(0x1122_3344),
      payload_len: 3,
      payload: vec![5, 6, 7],
      padding: vec![0],
    };
    assert_eq!(value.sdk_size(), 12);
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    value.write_to(&mut writer).unwrap();
    let mut reader = Reader::new(Cursor::new(writer.into_inner().into_inner())).unwrap();
    assert_eq!(
      ConditionalAndPadding::read_from(&mut reader).unwrap(),
      value
    );

    let invalid = ConditionalAndPadding {
      flags: 0,
      extra: Some(1),
      payload_len: 0,
      payload: Vec::new(),
      padding: Vec::new(),
    };
    assert!(
      invalid
        .write_to(&mut Writer::new(Cursor::new(Vec::new())))
        .is_err()
    );
  }

  #[test]
  fn derive_supports_bounded_remaining_bytes() {
    let value = RemainingBytes {
      tag: 0x1234,
      tail: vec![1, 2, 3, 4],
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    value.write_to(&mut writer).unwrap();
    assert_eq!(value.sdk_size(), 6);
    let mut reader = Reader::new(Cursor::new(writer.into_inner().into_inner())).unwrap();
    assert_eq!(RemainingBytes::read_from(&mut reader).unwrap(), value);
  }

  #[test]
  fn derive_supports_typed_remaining_arrays() {
    let bytes = [5, 7, 0, 11, 0];
    let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
    let value = RemainingWords::read_from(&mut reader).unwrap();
    assert_eq!(
      value,
      RemainingWords {
        tag: 5,
        values: vec![7, 11]
      }
    );
    assert_eq!(value.sdk_size(), bytes.len() as u64);

    let mut writer = Writer::new(Cursor::new(Vec::new()));
    value.write_to(&mut writer).unwrap();
    assert_eq!(writer.into_inner().into_inner(), bytes);

    let mut odd = Reader::new(Cursor::new([5, 7, 0, 11])).unwrap();
    assert!(RemainingWords::read_from(&mut odd).is_err());
  }
}
