use std::{
  cmp::Ordering,
  collections::BTreeMap,
  io::{Cursor, Write},
  path::Path,
};

use crate::{
  Error, Result,
  common::{FileTime, Guid},
  io::{SdkWrite, Writer},
};

use super::{
  CfbStreamOverride, CompoundFile, Entry, EntryKind, Version,
  allocation::{DIFAT_SECTOR, END_OF_CHAIN, FAT_SECTOR},
  directory::{
    DIRECTORY_ENTRY_LEN, DirectoryColor, DirectoryEntry, DirectoryObjectType, DirectoryPointer,
  },
  header::{BYTE_ORDER_LE, FREE_SECTOR, Header, MAGIC, MINI_SECTOR_SHIFT, MINI_STREAM_CUTOFF},
  name::{compare_names, validate_entry_name},
};

const MINI_SECTOR_LEN: usize = 64;
const HEADER_DIFAT_LEN: usize = 109;

pub(crate) fn write_compound(compound: &CompoundFile) -> Result<Vec<u8>> {
  let layout = build_layout(
    compound.version,
    &compound.entries,
    &compound.unallocated_sectors,
    &[],
  )?;
  let output_len = layout_output_len(
    &layout,
    &compound.unallocated_sectors,
    &compound.trailing_data,
    &[],
  )?;
  let mut output = Vec::with_capacity(output_len);
  emit_layout(
    &layout,
    &compound.unallocated_sectors,
    &compound.trailing_data,
    &[],
    &mut output,
  )?;
  debug_assert_eq!(output.len(), output_len);
  Ok(output)
}

pub(crate) fn write_compound_to(compound: &CompoundFile, writer: &mut impl Write) -> Result<()> {
  write_compound_to_with_overrides(compound, &[], writer)
}

pub(crate) fn write_compound_with_overrides(
  compound: &CompoundFile,
  overrides: &[CfbStreamOverride<'_>],
) -> Result<Vec<u8>> {
  validate_overrides(&compound.entries, overrides)?;
  let layout = build_layout(
    compound.version,
    &compound.entries,
    &compound.unallocated_sectors,
    overrides,
  )?;
  let output_len = layout_output_len(
    &layout,
    &compound.unallocated_sectors,
    &compound.trailing_data,
    overrides,
  )?;
  let mut output = Vec::with_capacity(output_len);
  emit_layout(
    &layout,
    &compound.unallocated_sectors,
    &compound.trailing_data,
    overrides,
    &mut output,
  )?;
  debug_assert_eq!(output.len(), output_len);
  Ok(output)
}

pub(crate) fn write_compound_to_with_overrides(
  compound: &CompoundFile,
  overrides: &[CfbStreamOverride<'_>],
  writer: &mut impl Write,
) -> Result<()> {
  validate_overrides(&compound.entries, overrides)?;
  write_logical_compound_to(
    compound.version,
    &compound.entries,
    &compound.unallocated_sectors,
    &compound.trailing_data,
    overrides,
    writer,
  )
}

pub(crate) fn write_empty_compound(version: Version) -> Result<Vec<u8>> {
  let entries = [Entry {
    path: "/".into(),
    name: "Root Entry".into(),
    kind: EntryKind::Root,
    clsid: Guid::ZERO,
    state_bits: 0,
    created: FileTime::ZERO,
    modified: FileTime::ZERO,
    data: Vec::new().into(),
  }];
  let layout = build_layout(version, &entries, &[], &[])?;
  let output_len = layout_output_len(&layout, &[], &[], &[])?;
  let mut output = Vec::with_capacity(output_len);
  emit_layout(&layout, &[], &[], &[], &mut output)?;
  debug_assert_eq!(output.len(), output_len);
  Ok(output)
}

struct CompoundLayout<'a> {
  sector_len: usize,
  ordered: Vec<&'a Entry>,
  mini_stream_len: usize,
  mini_fat: Vec<u32>,
  directory_entries: Vec<DirectoryEntry>,
  padded_directory_entry_count: usize,
  fat: Vec<u32>,
  fat_sector_ids: Vec<u32>,
  difat_sector_ids: Vec<u32>,
  header: Header,
}

fn write_logical_compound_to(
  version: Version,
  entries: &[Entry],
  unallocated_sectors: &[Vec<u8>],
  trailing_data: &[u8],
  overrides: &[CfbStreamOverride<'_>],
  writer: &mut impl Write,
) -> Result<()> {
  let layout = build_layout(version, entries, unallocated_sectors, overrides)?;
  emit_layout(
    &layout,
    unallocated_sectors,
    trailing_data,
    overrides,
    writer,
  )
}

fn build_layout<'a>(
  version: Version,
  entries: &'a [Entry],
  unallocated_sectors: &[Vec<u8>],
  overrides: &[CfbStreamOverride<'_>],
) -> Result<CompoundLayout<'a>> {
  let sector_len = match version {
    Version::V3 => 512,
    Version::V4 => 4096,
  };
  let ordered = ordered_entries(entries)?;
  if version == Version::V3
    && ordered
      .iter()
      .any(|entry| entry.kind == EntryKind::Stream && stream_len(entry, overrides) > 0x8000_0000)
  {
    return Err(Error::Limit(
      "CFB v3 streams cannot exceed the specified 2 GiB limit".into(),
    ));
  }
  let mut starts = vec![END_OF_CHAIN; ordered.len()];
  let mut mini_starts = vec![END_OF_CHAIN; ordered.len()];
  let mut mini_fat = Vec::new();

  for (index, entry) in ordered.iter().enumerate() {
    if entry.kind != EntryKind::Stream
      || stream_len(entry, overrides) == 0
      || stream_len(entry, overrides) >= MINI_STREAM_CUTOFF as usize
    {
      continue;
    }
    let start = u32_len(mini_fat.len(), "mini-sector count")?;
    mini_starts[index] = start;
    let count = div_ceil(stream_len(entry, overrides), MINI_SECTOR_LEN);
    for offset in 0..count {
      let offset = u32_len(offset, "mini-sector chain length")?;
      let current = start
        .checked_add(offset)
        .ok_or_else(|| Error::Limit("mini-sector ID overflow".into()))?;
      mini_fat.push(if offset as usize + 1 == count {
        END_OF_CHAIN
      } else {
        current + 1
      });
    }
  }
  let mini_stream_len = mini_fat
    .len()
    .checked_mul(MINI_SECTOR_LEN)
    .ok_or_else(|| Error::Limit("root mini stream size overflow".into()))?;
  if version == Version::V3 && mini_stream_len > 0x8000_0000 {
    return Err(Error::Limit(
      "CFB v3 mini stream cannot exceed the specified 2 GiB limit".into(),
    ));
  }

  for sector in unallocated_sectors {
    if sector.len() != sector_len {
      return Err(Error::invalid(0, "unallocated sector has the wrong size"));
    }
  }

  let mut next_sector = 0usize;
  let mut chains = Vec::<(u32, usize)>::new();
  let root_mini_start =
    reserve_payload(&mut next_sector, &mut chains, mini_stream_len, sector_len)?;

  let mini_fat_entries_per_sector = sector_len / 4;
  if !mini_fat.is_empty() {
    let padded = checked_next_multiple(
      mini_fat.len(),
      mini_fat_entries_per_sector,
      "MiniFAT entry count",
    )?;
    mini_fat.resize(padded, FREE_SECTOR);
  }
  let mini_fat_bytes_len = mini_fat
    .len()
    .checked_mul(4)
    .ok_or_else(|| Error::Limit("MiniFAT byte size overflow".into()))?;
  let mini_fat_start = reserve_payload(
    &mut next_sector,
    &mut chains,
    mini_fat_bytes_len,
    sector_len,
  )?;
  let mini_fat_sector_count = mini_fat.len() / mini_fat_entries_per_sector;

  for (index, entry) in ordered.iter().enumerate() {
    if entry.kind == EntryKind::Stream
      && stream_len(entry, overrides) >= MINI_STREAM_CUTOFF as usize
    {
      starts[index] = reserve_payload(
        &mut next_sector,
        &mut chains,
        stream_len(entry, overrides),
        sector_len,
      )?;
    } else if entry.kind == EntryKind::Stream {
      starts[index] = mini_starts[index];
    }
  }

  let directory_entries = build_directory_entries(
    &ordered,
    &starts,
    root_mini_start,
    mini_stream_len as u64,
    overrides,
  )?;
  let entries_per_sector = sector_len / DIRECTORY_ENTRY_LEN;
  let padded_directory_entry_count = checked_next_multiple(
    directory_entries.len(),
    entries_per_sector,
    "directory entry count",
  )?;
  let directory_bytes_len = padded_directory_entry_count
    .checked_mul(DIRECTORY_ENTRY_LEN)
    .ok_or_else(|| Error::Limit("directory byte size overflow".into()))?;
  let directory_start = reserve_payload(
    &mut next_sector,
    &mut chains,
    directory_bytes_len,
    sector_len,
  )?;
  let directory_sector_count = directory_bytes_len / sector_len;

  let data_sector_count = next_sector;
  let fat_entries_per_sector = sector_len / 4;
  let difat_entries_per_sector = fat_entries_per_sector - 1;
  let (fat_sector_count, difat_sector_count) = allocation_table_counts(
    data_sector_count
      .checked_add(unallocated_sectors.len())
      .ok_or_else(|| Error::Limit("CFB sector count overflow".into()))?,
    fat_entries_per_sector,
    difat_entries_per_sector,
  )?;
  let difat_start_index = data_sector_count;
  let fat_start_index = difat_start_index
    .checked_add(difat_sector_count)
    .ok_or_else(|| Error::Limit("CFB sector count overflow".into()))?;
  let total_sector_count = fat_start_index
    .checked_add(fat_sector_count)
    .ok_or_else(|| Error::Limit("CFB sector count overflow".into()))?;
  let fat_entry_count = fat_sector_count
    .checked_mul(fat_entries_per_sector)
    .ok_or_else(|| Error::Limit("FAT entry count overflow".into()))?;
  let mut fat = vec![FREE_SECTOR; fat_entry_count];
  for &(start, count) in &chains {
    mark_chain(&mut fat, start, count)?;
  }
  for entry in fat.iter_mut().take(fat_start_index).skip(difat_start_index) {
    *entry = DIFAT_SECTOR;
  }
  for entry in fat
    .iter_mut()
    .take(total_sector_count)
    .skip(fat_start_index)
  {
    *entry = FAT_SECTOR;
  }

  let fat_sector_ids: Vec<u32> = (fat_start_index..total_sector_count)
    .map(|value| u32_len(value, "FAT sector ID"))
    .collect::<Result<_>>()?;
  let difat_sector_ids: Vec<u32> = (difat_start_index..fat_start_index)
    .map(|value| u32_len(value, "DIFAT sector ID"))
    .collect::<Result<_>>()?;
  let mut header_difat = [FREE_SECTOR; HEADER_DIFAT_LEN];
  let initial_count = fat_sector_ids.len().min(HEADER_DIFAT_LEN);
  header_difat[..initial_count].copy_from_slice(&fat_sector_ids[..initial_count]);
  let header = Header {
    signature: MAGIC,
    clsid: [0; 16],
    minor_version: 0x003e,
    major_version: match version {
      Version::V3 => 3,
      Version::V4 => 4,
    },
    byte_order: BYTE_ORDER_LE,
    sector_shift: match version {
      Version::V3 => 9,
      Version::V4 => 12,
    },
    mini_sector_shift: MINI_SECTOR_SHIFT,
    reserved: [0; 6],
    number_of_directory_sectors: if version == Version::V4 {
      u32_len(directory_sector_count, "directory sector count")?
    } else {
      0
    },
    number_of_fat_sectors: u32_len(fat_sector_count, "FAT sector count")?,
    first_directory_sector: directory_start,
    transaction_signature: 0,
    mini_stream_cutoff: MINI_STREAM_CUTOFF,
    first_mini_fat_sector: mini_fat_start,
    number_of_mini_fat_sectors: u32_len(mini_fat_sector_count, "MiniFAT sector count")?,
    first_difat_sector: difat_sector_ids.first().copied().unwrap_or(END_OF_CHAIN),
    number_of_difat_sectors: u32_len(difat_sector_count, "DIFAT sector count")?,
    difat: header_difat,
  };

  Ok(CompoundLayout {
    sector_len,
    ordered,
    mini_stream_len,
    mini_fat,
    directory_entries,
    padded_directory_entry_count,
    fat,
    fat_sector_ids,
    difat_sector_ids,
    header,
  })
}

fn validate_overrides(entries: &[Entry], overrides: &[CfbStreamOverride<'_>]) -> Result<()> {
  for (index, stream_override) in overrides.iter().enumerate() {
    if overrides[..index]
      .iter()
      .any(|previous| previous.path == stream_override.path)
    {
      return Err(Error::invalid(
        0,
        format!(
          "duplicate CFB stream override {}",
          stream_override.path.display()
        ),
      ));
    }
    let entry = entries
      .iter()
      .find(|entry| entry.path == stream_override.path)
      .ok_or_else(|| {
        Error::invalid(
          0,
          format!(
            "CFB stream override {} does not exist",
            stream_override.path.display()
          ),
        )
      })?;
    if entry.kind != EntryKind::Stream {
      return Err(Error::invalid(
        0,
        format!(
          "CFB stream override {} is not a stream",
          stream_override.path.display()
        ),
      ));
    }
  }
  Ok(())
}

fn stream_override<'a>(
  entry: &Entry,
  overrides: &'a [CfbStreamOverride<'_>],
) -> Option<&'a CfbStreamOverride<'a>> {
  overrides
    .iter()
    .find(|stream_override| stream_override.path == entry.path)
}

fn stream_len(entry: &Entry, overrides: &[CfbStreamOverride<'_>]) -> usize {
  stream_override(entry, overrides).map_or_else(|| entry.data.len(), |value| value.len)
}

fn write_stream(
  entry: &Entry,
  overrides: &[CfbStreamOverride<'_>],
  writer: &mut impl Write,
) -> Result<()> {
  let Some(stream_override) = stream_override(entry, overrides) else {
    return entry.data.write_to(writer);
  };
  let mut exact = ExactSizeWriter {
    writer,
    remaining: stream_override.len,
  };
  stream_override.writer.write_to(&mut exact)?;
  if exact.remaining != 0 {
    return Err(Error::invalid(
      0,
      format!(
        "CFB stream override {} emitted {} fewer bytes than declared",
        stream_override.path.display(),
        exact.remaining
      ),
    ));
  }
  Ok(())
}

struct ExactSizeWriter<'a, W: Write + ?Sized> {
  writer: &'a mut W,
  remaining: usize,
}

impl<W: Write + ?Sized> Write for ExactSizeWriter<'_, W> {
  fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
    if bytes.len() > self.remaining {
      return Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "CFB stream override emitted more bytes than declared",
      ));
    }
    let written = self.writer.write(bytes)?;
    self.remaining = self.remaining.saturating_sub(written);
    Ok(written)
  }

  fn flush(&mut self) -> std::io::Result<()> {
    self.writer.flush()
  }
}

fn emit_layout(
  layout: &CompoundLayout<'_>,
  unallocated_sectors: &[Vec<u8>],
  trailing_data: &[u8],
  overrides: &[CfbStreamOverride<'_>],
  writer: &mut impl Write,
) -> Result<()> {
  let header = encode_header(&layout.header)?;
  writer.write_all(&header)?;
  write_zeros(writer, layout.sector_len - header.len())?;

  for entry in &layout.ordered {
    let stream_len = stream_len(entry, overrides);
    if entry.kind != EntryKind::Stream
      || stream_len == 0
      || stream_len >= MINI_STREAM_CUTOFF as usize
    {
      continue;
    }
    write_stream(entry, overrides, writer)?;
    write_zeros(
      writer,
      checked_next_multiple(stream_len, MINI_SECTOR_LEN, "mini stream payload length")?
        - stream_len,
    )?;
  }
  write_zeros(
    writer,
    checked_next_multiple(
      layout.mini_stream_len,
      layout.sector_len,
      "root mini stream length",
    )? - layout.mini_stream_len,
  )?;

  write_u32_sectors(writer, &layout.mini_fat, layout.sector_len)?;

  for entry in &layout.ordered {
    let stream_len = stream_len(entry, overrides);
    if entry.kind != EntryKind::Stream || stream_len < MINI_STREAM_CUTOFF as usize {
      continue;
    }
    write_stream(entry, overrides, writer)?;
    write_zeros(
      writer,
      checked_next_multiple(
        stream_len,
        layout.sector_len,
        "regular stream payload length",
      )? - stream_len,
    )?;
  }

  for entry in &layout.directory_entries {
    writer.write_all(&encode_directory_entry(entry)?)?;
  }
  let unallocated = encode_directory_entry(&unallocated_directory_entry())?;
  for _ in layout.directory_entries.len()..layout.padded_directory_entry_count {
    writer.write_all(&unallocated)?;
  }

  let entries_per_sector = layout.sector_len / 4;
  let difat_entries_per_sector = entries_per_sector - 1;
  let remaining_fat = &layout.fat_sector_ids[HEADER_DIFAT_LEN.min(layout.fat_sector_ids.len())..];
  let mut values = vec![FREE_SECTOR; entries_per_sector];
  for (index, &sector_id) in layout.difat_sector_ids.iter().enumerate() {
    values.fill(FREE_SECTOR);
    let begin = index * difat_entries_per_sector;
    let end = (begin + difat_entries_per_sector).min(remaining_fat.len());
    values[..end - begin].copy_from_slice(&remaining_fat[begin..end]);
    values[difat_entries_per_sector] = layout
      .difat_sector_ids
      .get(index + 1)
      .copied()
      .unwrap_or(END_OF_CHAIN);
    debug_assert_eq!(
      sector_id as usize,
      layout.header.first_difat_sector as usize + index
    );
    write_u32_sectors(writer, &values, layout.sector_len)?;
  }
  write_u32_sectors(writer, &layout.fat, layout.sector_len)?;

  for sector in unallocated_sectors {
    writer.write_all(sector)?;
  }
  writer.write_all(trailing_data)?;
  Ok(())
}

fn layout_output_len(
  layout: &CompoundLayout<'_>,
  unallocated_sectors: &[Vec<u8>],
  trailing_data: &[u8],
  overrides: &[CfbStreamOverride<'_>],
) -> Result<usize> {
  let mut len = layout.sector_len;
  let mut add = |amount: usize| -> Result<()> {
    len = len
      .checked_add(amount)
      .ok_or_else(|| Error::Limit("CFB output length overflow".into()))?;
    Ok(())
  };

  add(checked_next_multiple(
    layout.mini_stream_len,
    layout.sector_len,
    "root mini stream length",
  )?)?;
  add(
    layout
      .mini_fat
      .len()
      .checked_mul(4)
      .ok_or_else(|| Error::Limit("CFB MiniFAT byte length overflow".into()))?,
  )?;
  for entry in &layout.ordered {
    let stream_len = stream_len(entry, overrides);
    if entry.kind == EntryKind::Stream && stream_len >= MINI_STREAM_CUTOFF as usize {
      add(checked_next_multiple(
        stream_len,
        layout.sector_len,
        "regular stream payload length",
      )?)?;
    }
  }
  add(
    layout
      .padded_directory_entry_count
      .checked_mul(DIRECTORY_ENTRY_LEN)
      .ok_or_else(|| Error::Limit("CFB directory byte length overflow".into()))?,
  )?;
  add(
    layout
      .difat_sector_ids
      .len()
      .checked_mul(layout.sector_len)
      .ok_or_else(|| Error::Limit("CFB DIFAT byte length overflow".into()))?,
  )?;
  add(
    layout
      .fat
      .len()
      .checked_mul(4)
      .ok_or_else(|| Error::Limit("CFB FAT byte length overflow".into()))?,
  )?;
  for sector in unallocated_sectors {
    add(sector.len())?;
  }
  add(trailing_data.len())?;
  Ok(len)
}

fn ordered_entries(entries: &[Entry]) -> Result<Vec<&Entry>> {
  let root = entries
    .iter()
    .find(|entry| entry.kind == EntryKind::Root && entry.path == Path::new("/"))
    .ok_or_else(|| Error::invalid(0, "logical CFB model has no root entry"))?;
  if entries
    .iter()
    .filter(|entry| entry.kind == EntryKind::Root)
    .count()
    != 1
  {
    return Err(Error::invalid(0, "logical CFB model must contain one root"));
  }
  let mut rest: Vec<_> = entries
    .iter()
    .filter(|entry| entry.kind != EntryKind::Root)
    .collect();
  rest.sort_by(|left, right| left.path.cmp(&right.path));
  let mut ordered = Vec::with_capacity(entries.len());
  ordered.push(root);
  ordered.extend(rest);
  Ok(ordered)
}

fn build_directory_entries(
  entries: &[&Entry],
  starts: &[u32],
  root_mini_start: u32,
  root_mini_len: u64,
  overrides: &[CfbStreamOverride<'_>],
) -> Result<Vec<DirectoryEntry>> {
  let mut ids = BTreeMap::new();
  for (index, entry) in entries.iter().enumerate() {
    if ids.insert(entry.path.clone(), index as u32).is_some() {
      return Err(Error::invalid(0, "duplicate logical CFB path"));
    }
  }
  let mut records = Vec::with_capacity(entries.len());
  for (index, entry) in entries.iter().enumerate() {
    let (object_type, start_sector, stream_size) = match entry.kind {
      EntryKind::Root => (DirectoryObjectType::Root, root_mini_start, root_mini_len),
      EntryKind::Storage => (DirectoryObjectType::Storage, 0, 0),
      EntryKind::Stream => (
        DirectoryObjectType::Stream,
        starts[index],
        stream_len(entry, overrides) as u64,
      ),
    };
    records.push(DirectoryEntry {
      name_buffer: encode_name(if entry.kind == EntryKind::Root {
        "Root Entry"
      } else {
        &entry.name
      })?,
      name_length: name_length(if entry.kind == EntryKind::Root {
        "Root Entry"
      } else {
        &entry.name
      })?,
      object_type,
      color: DirectoryColor::Black,
      left_sibling: DirectoryPointer::None,
      right_sibling: DirectoryPointer::None,
      child: DirectoryPointer::None,
      clsid: if entry.kind == EntryKind::Stream {
        Guid::ZERO
      } else {
        entry.clsid
      },
      state_bits: entry.state_bits,
      creation_time: if matches!(entry.kind, EntryKind::Root | EntryKind::Stream) {
        FileTime::ZERO
      } else {
        entry.created
      },
      modified_time: if entry.kind == EntryKind::Stream {
        FileTime::ZERO
      } else {
        entry.modified
      },
      start_sector,
      stream_size,
    });
  }

  let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
  for (index, entry) in entries.iter().enumerate().skip(1) {
    let parent = entry
      .path
      .parent()
      .and_then(|path| ids.get(path))
      .copied()
      .ok_or_else(|| Error::invalid(0, format!("missing parent for {}", entry.path.display())))?;
    if entries[parent as usize].kind == EntryKind::Stream {
      return Err(Error::invalid(
        0,
        "stream cannot contain directory children",
      ));
    }
    children.entry(parent).or_default().push(index as u32);
  }
  for (parent, child_ids) in &mut children {
    child_ids.sort_by(|left, right| {
      compare_names(
        &entries[*left as usize].name,
        &entries[*right as usize].name,
      )
    });
    for pair in child_ids.windows(2) {
      if compare_names(
        &entries[pair[0] as usize].name,
        &entries[pair[1] as usize].name,
      ) == Ordering::Equal
      {
        return Err(Error::invalid(0, "duplicate case-insensitive CFB name"));
      }
    }
    records[*parent as usize].child = build_sibling_tree(child_ids, &mut records);
  }
  Ok(records)
}

fn build_sibling_tree(ids: &[u32], records: &mut [DirectoryEntry]) -> DirectoryPointer {
  fn build(
    ids: &[u32],
    records: &mut [DirectoryEntry],
    depth: usize,
    depths: &mut Vec<(u32, usize)>,
  ) -> DirectoryPointer {
    if ids.is_empty() {
      return DirectoryPointer::None;
    }
    let middle = ids.len() / 2;
    let id = ids[middle];
    let left = build(&ids[..middle], records, depth + 1, depths);
    let right = build(&ids[middle + 1..], records, depth + 1, depths);
    records[id as usize].left_sibling = left;
    records[id as usize].right_sibling = right;
    depths.push((id, depth));
    DirectoryPointer::Entry(id)
  }

  let mut depths = Vec::new();
  let root = build(ids, records, 0, &mut depths);
  let max_depth = depths.iter().map(|(_, depth)| *depth).max().unwrap_or(0);
  if max_depth > 0 {
    for (id, depth) in depths {
      records[id as usize].color = if depth == max_depth {
        DirectoryColor::Red
      } else {
        DirectoryColor::Black
      };
    }
  }
  root
}

fn reserve_payload(
  next_sector: &mut usize,
  chains: &mut Vec<(u32, usize)>,
  byte_len: usize,
  sector_len: usize,
) -> Result<u32> {
  if byte_len == 0 {
    return Ok(END_OF_CHAIN);
  }
  let start = u32_len(*next_sector, "sector ID")?;
  let count = div_ceil(byte_len, sector_len);
  *next_sector = next_sector
    .checked_add(count)
    .ok_or_else(|| Error::Limit("CFB sector count overflow".into()))?;
  chains.push((start, count));
  Ok(start)
}

fn checked_next_multiple(value: usize, divisor: usize, what: &str) -> Result<usize> {
  let remainder = value % divisor;
  if remainder == 0 {
    return Ok(value);
  }
  value
    .checked_add(divisor - remainder)
    .ok_or_else(|| Error::Limit(format!("{what} overflow")))
}

fn allocation_table_counts(
  data_count: usize,
  fat_capacity: usize,
  difat_capacity: usize,
) -> Result<(usize, usize)> {
  let mut fat_count = 1usize;
  loop {
    let difat_count = if fat_count <= HEADER_DIFAT_LEN {
      0
    } else {
      div_ceil(fat_count - HEADER_DIFAT_LEN, difat_capacity)
    };
    let total = data_count
      .checked_add(fat_count)
      .and_then(|value| value.checked_add(difat_count))
      .ok_or_else(|| Error::Limit("CFB sector count overflow".into()))?;
    let needed = div_ceil(total, fat_capacity).max(1);
    if needed == fat_count {
      return Ok((fat_count, difat_count));
    }
    fat_count = needed;
  }
}

fn mark_chain(fat: &mut [u32], start: u32, count: usize) -> Result<()> {
  let start = start as usize;
  for offset in 0..count {
    let index = start
      .checked_add(offset)
      .ok_or_else(|| Error::Limit("FAT chain index overflow".into()))?;
    fat[index] = if offset + 1 == count {
      END_OF_CHAIN
    } else {
      u32_len(index + 1, "FAT next sector")?
    };
  }
  Ok(())
}

fn encode_header(header: &Header) -> Result<[u8; 512]> {
  let mut bytes = [0; 512];
  let cursor = Cursor::new(bytes.as_mut_slice());
  let mut writer = Writer::new(cursor);
  header.write_to(&mut writer)?;
  Ok(bytes)
}

fn encode_directory_entry(entry: &DirectoryEntry) -> Result<[u8; DIRECTORY_ENTRY_LEN]> {
  let mut bytes = [0; DIRECTORY_ENTRY_LEN];
  let cursor = Cursor::new(bytes.as_mut_slice());
  let mut writer = Writer::new(cursor);
  entry.write_to(&mut writer)?;
  Ok(bytes)
}

fn write_u32_sectors(writer: &mut impl Write, values: &[u32], sector_len: usize) -> Result<()> {
  if values.is_empty() {
    return Ok(());
  }
  let entries_per_sector = sector_len / 4;
  if !values.len().is_multiple_of(entries_per_sector) {
    return Err(Error::invalid(
      0,
      "allocation table is not padded to a complete sector",
    ));
  }
  let mut bytes = vec![0; sector_len];
  for sector in values.chunks_exact(entries_per_sector) {
    for (slot, value) in bytes.chunks_exact_mut(4).zip(sector) {
      slot.copy_from_slice(&value.to_le_bytes());
    }
    writer.write_all(&bytes)?;
  }
  Ok(())
}

fn write_zeros(writer: &mut impl Write, mut len: usize) -> Result<()> {
  const ZEROS: [u8; 4096] = [0; 4096];
  while len != 0 {
    let count = len.min(ZEROS.len());
    writer.write_all(&ZEROS[..count])?;
    len -= count;
  }
  Ok(())
}

fn unallocated_directory_entry() -> DirectoryEntry {
  DirectoryEntry {
    name_buffer: [0; 32],
    name_length: 0,
    object_type: DirectoryObjectType::Unallocated,
    color: DirectoryColor::Red,
    left_sibling: DirectoryPointer::None,
    right_sibling: DirectoryPointer::None,
    child: DirectoryPointer::None,
    clsid: Guid::ZERO,
    state_bits: 0,
    creation_time: FileTime::ZERO,
    modified_time: FileTime::ZERO,
    start_sector: 0,
    stream_size: 0,
  }
}

fn encode_name(name: &str) -> Result<[u16; 32]> {
  validate_entry_name(name)?;
  let chars: Vec<_> = name.encode_utf16().collect();
  let mut buffer = [0; 32];
  buffer[..chars.len()].copy_from_slice(&chars);
  Ok(buffer)
}

fn name_length(name: &str) -> Result<u16> {
  let chars = name.encode_utf16().count();
  u16::try_from((chars + 1) * 2).map_err(|_| Error::invalid(0, "CFB name length overflow"))
}

fn div_ceil(value: usize, divisor: usize) -> usize {
  value.div_ceil(divisor)
}

fn u32_len(value: usize, what: &str) -> Result<u32> {
  u32::try_from(value).map_err(|_| Error::Limit(format!("{what} exceeds u32")))
}

#[cfg(test)]
mod tests {
  use std::io;

  use super::*;

  struct ShortWriter {
    bytes: Vec<u8>,
    maximum_write: usize,
  }

  impl Write for ShortWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
      let count = bytes.len().min(self.maximum_write);
      self.bytes.extend_from_slice(&bytes[..count]);
      Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
      Ok(())
    }
  }

  #[test]
  fn allocation_counts_include_fat_and_difat_sectors() {
    assert_eq!(allocation_table_counts(2, 128, 127).unwrap(), (1, 0));
    let (fat, difat) = allocation_table_counts(20_000, 128, 127).unwrap();
    assert!(fat > HEADER_DIFAT_LEN);
    assert!(difat > 0);
    assert!(20_000 + fat + difat <= fat * 128);
  }

  #[test]
  fn rebuild_writer_is_sequential_and_accepts_partial_writes() {
    for version in [Version::V3, Version::V4] {
      let mut compound = CompoundFile::new(version).unwrap();
      compound.create_storage_all("/Data/Nested").unwrap();
      compound
        .create_stream("/Data/Nested/Mini", vec![0x31; 977])
        .unwrap();
      compound
        .create_stream("/Data/Regular", vec![0x52; 65_537])
        .unwrap();

      let expected = compound.to_bytes().unwrap();
      let mut output = ShortWriter {
        bytes: Vec::new(),
        maximum_write: 73,
      };
      compound.write_to(&mut output).unwrap();

      assert_eq!(output.bytes, expected);
      let reopened = CompoundFile::from_bytes_strict(&output.bytes).unwrap();
      assert!(compound.logical_eq(&reopened));
    }
  }
}
