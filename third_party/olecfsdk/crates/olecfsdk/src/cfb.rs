use std::{
  fmt,
  io::{Cursor, Read, Write},
  ops::Deref,
  ops::Range,
  path::{Path, PathBuf},
  sync::{Arc, OnceLock},
};

use crate::{
  Error, Result,
  common::{FileTime, Guid},
  limits::Limits,
};

mod allocation;
mod directory;
mod header;
mod name;
mod owned_stream;
mod reader;
mod sector;
mod stream;
mod writer;

pub use allocation::{Difat, Fat, FatEntry, FatMarkerMismatch, MiniFat, MiniFatEntry};
pub use directory::{
  Directory, DirectoryColor, DirectoryEntry, DirectoryObjectType, DirectoryPointer,
};
pub use header::Header;
pub use name::compare_names;
pub use owned_stream::OwnedCfbStream;
pub use reader::{CfbReadStream, CfbStreamMut, CompoundFileReader, EntryInfo};
pub use sector::{CfbReadAt, MiniSectorId, SectorId};

pub(crate) trait CfbStreamWriter {
  fn write_to(&self, writer: &mut dyn Write) -> Result<()>;
}

pub(crate) struct CfbStreamOverride<'a> {
  path: &'a Path,
  len: usize,
  writer: &'a dyn CfbStreamWriter,
}

impl<'a> CfbStreamOverride<'a> {
  pub(crate) fn new(path: &'a Path, len: usize, writer: &'a dyn CfbStreamWriter) -> Self {
    Self { path, len, writer }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
  V3,
  V4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
  Root,
  Storage,
  Stream,
}

/// Clone-on-write bytes for one CFB stream.
///
/// Parsed streams may borrow one or more ranges from a shared CFB image. A
/// contiguous stream remains zero-copy; a fragmented stream is assembled once
/// on first access. Mutable [`CompoundFile`] APIs detach only the stream being
/// changed.
#[derive(Clone)]
pub struct CfbStreamData(Arc<CfbStreamBacking>);

enum CfbStreamBacking {
  Owned(Vec<u8>),
  Archived {
    source: Arc<Vec<u8>>,
    ranges: Box<[Range<usize>]>,
    len: usize,
    materialized: OnceLock<Vec<u8>>,
  },
}

impl CfbStreamData {
  pub(crate) fn archived(
    source: Arc<Vec<u8>>,
    ranges: Vec<Range<usize>>,
    len: usize,
  ) -> Result<Self> {
    let archived_len = ranges.iter().try_fold(0usize, |total, range| {
      if range.start > range.end || range.end > source.len() {
        return Err(Error::invalid(
          0,
          "archived CFB stream range is outside its source",
        ));
      }
      total
        .checked_add(range.len())
        .ok_or_else(|| Error::Limit("archived CFB stream length overflow".into()))
    })?;
    if archived_len != len {
      return Err(Error::invalid(
        0,
        "archived CFB stream ranges do not match its logical length",
      ));
    }
    Ok(Self(Arc::new(CfbStreamBacking::Archived {
      source,
      ranges: ranges.into_boxed_slice(),
      len,
      materialized: OnceLock::new(),
    })))
  }

  /// Borrows the stream as bytes, materializing a fragmented archived stream
  /// at most once.
  pub fn as_slice(&self) -> &[u8] {
    match self.0.as_ref() {
      CfbStreamBacking::Owned(bytes) => bytes,
      CfbStreamBacking::Archived {
        source,
        ranges,
        len,
        materialized,
      } => {
        if ranges.is_empty() {
          return &[];
        }
        if ranges.len() == 1 {
          return &source[ranges[0].clone()];
        }
        materialized.get_or_init(|| {
          let mut bytes = Vec::with_capacity(*len);
          for range in ranges.iter() {
            bytes.extend_from_slice(&source[range.clone()]);
          }
          bytes
        })
      }
    }
  }

  pub fn len(&self) -> usize {
    match self.0.as_ref() {
      CfbStreamBacking::Owned(bytes) => bytes.len(),
      CfbStreamBacking::Archived { len, .. } => *len,
    }
  }

  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }

  pub(crate) fn write_to(&self, writer: &mut impl Write) -> Result<()> {
    match self.0.as_ref() {
      CfbStreamBacking::Owned(bytes) => writer.write_all(bytes)?,
      CfbStreamBacking::Archived { source, ranges, .. } => {
        for range in ranges.iter() {
          writer.write_all(&source[range.clone()])?;
        }
      }
    }
    Ok(())
  }

  /// Returns mutable bytes, copying only when another clone shares them.
  pub fn to_mut(&mut self) -> &mut Vec<u8> {
    let can_mutate_owned = Arc::get_mut(&mut self.0)
      .is_some_and(|backing| matches!(backing, CfbStreamBacking::Owned(_)));
    if !can_mutate_owned {
      let bytes = self.as_slice().to_vec();
      self.0 = Arc::new(CfbStreamBacking::Owned(bytes));
    }
    let CfbStreamBacking::Owned(bytes) =
      Arc::get_mut(&mut self.0).expect("detached CFB stream backing is uniquely owned")
    else {
      unreachable!("detached CFB stream backing is owned")
    };
    bytes
  }

  /// Unwraps uniquely owned bytes or copies them when they remain shared.
  pub fn into_vec(self) -> Vec<u8> {
    match Arc::try_unwrap(self.0) {
      Ok(CfbStreamBacking::Owned(bytes)) => bytes,
      Ok(CfbStreamBacking::Archived {
        source,
        ranges,
        len,
        materialized,
      }) => materialized.into_inner().unwrap_or_else(|| {
        let mut bytes = Vec::with_capacity(len);
        for range in ranges.iter() {
          bytes.extend_from_slice(&source[range.clone()]);
        }
        bytes
      }),
      Err(shared) => CfbStreamData(shared).as_slice().to_vec(),
    }
  }
}

impl Default for CfbStreamData {
  fn default() -> Self {
    Vec::new().into()
  }
}

impl fmt::Debug for CfbStreamData {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_tuple("CfbStreamData")
      .field(&self.as_slice())
      .finish()
  }
}

impl PartialEq for CfbStreamData {
  fn eq(&self, other: &Self) -> bool {
    self.as_slice() == other.as_slice()
  }
}

impl Eq for CfbStreamData {}

impl Deref for CfbStreamData {
  type Target = [u8];

  fn deref(&self) -> &Self::Target {
    self.as_slice()
  }
}

impl AsRef<[u8]> for CfbStreamData {
  fn as_ref(&self) -> &[u8] {
    self.as_slice()
  }
}

impl From<Vec<u8>> for CfbStreamData {
  fn from(value: Vec<u8>) -> Self {
    Self(Arc::new(CfbStreamBacking::Owned(value)))
  }
}

impl From<CfbStreamData> for Vec<u8> {
  fn from(value: CfbStreamData) -> Self {
    value.into_vec()
  }
}

impl PartialEq<Vec<u8>> for CfbStreamData {
  fn eq(&self, other: &Vec<u8>) -> bool {
    self.as_slice() == other.as_slice()
  }
}

impl PartialEq<CfbStreamData> for Vec<u8> {
  fn eq(&self, other: &CfbStreamData) -> bool {
    self == other.as_slice()
  }
}

impl<const N: usize> PartialEq<&[u8; N]> for CfbStreamData {
  fn eq(&self, other: &&[u8; N]) -> bool {
    self.as_slice() == other.as_slice()
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
  pub path: PathBuf,
  pub name: String,
  pub kind: EntryKind,
  pub clsid: Guid,
  pub state_bits: u32,
  pub created: FileTime,
  pub modified: FileTime,
  /// Immutable stream bytes shared by cheap [`CompoundFile`] clones.
  ///
  /// Mutable stream APIs use clone-on-write, so the allocation is copied
  /// only when a shared stream is actually edited.
  pub data: CfbStreamData,
}

impl Entry {
  pub fn is_stream(&self) -> bool {
    self.kind == EntryKind::Stream
  }
  pub fn is_storage(&self) -> bool {
    self.kind != EntryKind::Stream
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompoundFile {
  version: Version,
  header: Header,
  difat: Arc<Difat>,
  fat: Arc<Fat>,
  mini_fat: Arc<MiniFat>,
  directory: Arc<Directory>,
  entries: Arc<Vec<Entry>>,
  header_padding_is_zero: bool,
  unallocated_sectors: Arc<Vec<Vec<u8>>>,
  trailing_data: Arc<Vec<u8>>,
}

impl CompoundFile {
  pub fn new(version: Version) -> Result<Self> {
    Self::from_vec(writer::write_empty_compound(version)?)
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Self::from_bytes_with_limits(bytes, Limits::default())
  }

  /// Parses a CFB image and additionally enforces canonical MS-CFB fields.
  ///
  /// Use [`Self::from_bytes`] for compatibility reading of legacy producer
  /// quirks that are normalized by the deterministic writer.
  pub fn from_bytes_strict(bytes: &[u8]) -> Result<Self> {
    let compound = Self::from_bytes(bytes)?;
    compound.validate_strict()?;
    Ok(compound)
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    Self::from_shared_archive_with_limits(Arc::new(bytes.to_vec()), limits)
  }

  /// Parses an owned CFB image without first copying its complete byte
  /// buffer. Stream payloads retain archived ranges from this shared image.
  pub fn from_vec(bytes: Vec<u8>) -> Result<Self> {
    Self::from_vec_with_limits(bytes, Limits::default())
  }

  pub fn from_vec_with_limits(bytes: Vec<u8>, limits: Limits) -> Result<Self> {
    Self::from_shared_archive_with_limits(Arc::new(bytes), limits)
  }

  pub(crate) fn from_shared_archive_with_limits(
    archive: Arc<Vec<u8>>,
    limits: Limits,
  ) -> Result<Self> {
    let bytes = archive.as_slice();
    if bytes.len() as u64 > limits.max_file_size {
      return Err(Error::Limit(format!(
        "file length {} exceeds {}",
        bytes.len(),
        limits.max_file_size
      )));
    }
    let header = Header::from_bytes(bytes)?;
    let sector_len = header.sector_len();
    let header_padding_is_zero = if sector_len > header::HEADER_LEN {
      bytes
        .get(header::HEADER_LEN..sector_len)
        .is_some_and(|padding| padding.iter().all(|byte| *byte == 0))
    } else {
      true
    };
    let mut sectors = sector::SectorSource::new(bytes, bytes.len(), &header)?;
    if sectors.sector_count() > u32::MAX as usize {
      return Err(Error::Limit("CFB sector count exceeds u32".into()));
    }
    sectors.full_sector(SectorId::new(header.first_directory_sector)?)?;
    let difat = Difat::read(&header, &mut sectors, limits)?;
    let fat = Fat::read(&difat, &mut sectors)?;
    let directory_sectors = fat.chain(header.first_directory_sector, sectors.sector_count())?;
    let mini_fat = MiniFat::read(&header, &fat, &mut sectors, limits)?;
    let directory = Directory::read(
      directory_sectors,
      header.number_of_directory_sectors,
      &mut sectors,
      limits,
    )?;
    let version = header.version();
    let entries = stream::read_entries_archived(
      &header,
      &fat,
      &mini_fat,
      &directory,
      &mut sectors,
      &archive,
      limits,
    )?;
    let mut unallocated_sectors = Vec::new();
    let mut unallocated_bytes = 0usize;
    for index in 0..sectors.sector_count() {
      let id = SectorId::new(index as u32)?;
      if sectors.is_partial(id) {
        continue;
      }
      let is_allocation_sector =
        difat.fat_sectors().contains(&id) || difat.difat_sectors().contains(&id);
      if !is_allocation_sector && fat.is_free_or_unaddressed(id) {
        unallocated_bytes = unallocated_bytes
          .checked_add(sectors.sector_len())
          .ok_or_else(|| Error::Limit("unallocated sector size overflow".into()))?;
        if unallocated_bytes > limits.max_allocation {
          return Err(Error::Limit(format!(
            "unallocated sector data {unallocated_bytes} exceeds {}",
            limits.max_allocation
          )));
        }
        unallocated_sectors.push(sectors.sector(id)?.to_vec());
      }
    }
    let trailing_data = sectors.unaccessed_partial_data().to_vec();
    Ok(Self {
      version,
      header,
      difat: Arc::new(difat),
      fat: Arc::new(fat),
      mini_fat: Arc::new(mini_fat),
      directory: Arc::new(directory),
      entries: Arc::new(entries),
      header_padding_is_zero,
      unallocated_sectors: Arc::new(unallocated_sectors),
      trailing_data: Arc::new(trailing_data),
    })
  }

  pub fn from_reader(reader: impl Read) -> Result<Self> {
    Self::from_reader_with_limits(reader, Limits::default())
  }

  pub fn from_reader_strict(reader: impl Read) -> Result<Self> {
    let compound = Self::from_reader(reader)?;
    compound.validate_strict()?;
    Ok(compound)
  }

  pub fn from_reader_with_limits(mut reader: impl Read, limits: Limits) -> Result<Self> {
    let maximum = limits.max_file_size.saturating_add(1);
    let mut bytes = Vec::new();
    reader.by_ref().take(maximum).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limits.max_file_size {
      return Err(Error::Limit(format!(
        "file length exceeds {}",
        limits.max_file_size
      )));
    }
    Self::from_vec_with_limits(bytes, limits)
  }

  pub fn open(path: impl AsRef<Path>) -> Result<Self> {
    Self::from_reader(std::fs::File::open(path)?)
  }

  pub fn open_strict(path: impl AsRef<Path>) -> Result<Self> {
    Self::from_reader_strict(std::fs::File::open(path)?)
  }

  pub fn version(&self) -> Version {
    self.version
  }
  pub fn entries(&self) -> &[Entry] {
    &self.entries
  }
  pub fn root_entry(&self) -> &Entry {
    &self.entries[0]
  }
  pub fn header(&self) -> &Header {
    &self.header
  }
  pub fn difat(&self) -> &Difat {
    &self.difat
  }
  pub fn fat(&self) -> &Fat {
    &self.fat
  }
  pub fn mini_fat(&self) -> &MiniFat {
    &self.mini_fat
  }
  pub fn directory_sectors(&self) -> &[SectorId] {
    self.directory.sectors()
  }
  pub fn directory(&self) -> &Directory {
    &self.directory
  }
  pub fn entry(&self, path: impl AsRef<Path>) -> Option<&Entry> {
    self
      .entry_index(path.as_ref())
      .map(|index| &self.entries[index])
  }
  pub fn contains_entry(&self, path: impl AsRef<Path>) -> bool {
    self.entry(path).is_some()
  }
  pub fn is_stream(&self, path: impl AsRef<Path>) -> bool {
    self.entry(path).is_some_and(Entry::is_stream)
  }
  pub fn is_storage(&self, path: impl AsRef<Path>) -> bool {
    self.entry(path).is_some_and(Entry::is_storage)
  }
  pub fn children(&self, path: impl AsRef<Path>) -> Result<Vec<&Entry>> {
    let parent = self.required_entry_index(path.as_ref())?;
    if self.entries[parent].is_stream() {
      return Err(Error::invalid(
        0,
        format!(
          "CFB entry {} is not a storage",
          self.entries[parent].path.display()
        ),
      ));
    }
    let parent_path = self.entries[parent].path.as_path();
    let mut children: Vec<_> = self
      .entries
      .iter()
      .filter(|entry| entry.path.parent() == Some(parent_path))
      .collect();
    children.sort_by(|left, right| name::compare_names(&left.name, &right.name));
    Ok(children)
  }
  pub fn walk_storage(&self, path: impl AsRef<Path>) -> Result<Vec<&Entry>> {
    let root = self.required_entry_index(path.as_ref())?;
    if self.entries[root].is_stream() {
      return Err(Error::invalid(
        0,
        format!(
          "CFB entry {} is not a storage",
          self.entries[root].path.display()
        ),
      ));
    }
    let mut output = Vec::new();
    let mut stack = vec![root];
    while let Some(index) = stack.pop() {
      let entry = &self.entries[index];
      output.push(entry);
      if entry.is_stream() {
        continue;
      }
      let mut children: Vec<_> = self
        .entries
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.path.parent() == Some(entry.path.as_path()))
        .map(|(index, _)| index)
        .collect();
      children.sort_by(|left, right| {
        name::compare_names(&self.entries[*left].name, &self.entries[*right].name)
      });
      stack.extend(children.into_iter().rev());
    }
    Ok(output)
  }
  pub fn stream(&self, path: impl AsRef<Path>) -> Option<&[u8]> {
    self
      .entry(path)
      .filter(|entry| entry.is_stream())
      .map(|entry| entry.data.as_slice())
  }
  /// Opens an owned-model stream through the standard `Read + Seek` cursor API.
  pub fn open_stream(&self, path: impl AsRef<Path>) -> Result<Cursor<&[u8]>> {
    let index = self.required_entry_index(path.as_ref())?;
    if !self.entries[index].is_stream() {
      return Err(Error::invalid(0, "CFB entry is not a stream"));
    }
    Ok(Cursor::new(self.entries[index].data.as_slice()))
  }
  pub fn stream_mut(&mut self, path: impl AsRef<Path>) -> Option<&mut Vec<u8>> {
    let index = self.entry_index(path.as_ref())?;
    if !self.entries[index].is_stream() {
      return None;
    }
    Some(Arc::make_mut(&mut self.entries)[index].data.to_mut())
  }
  /// Opens a fully materialized stream for `Read + Write + Seek` and `set_len` edits.
  pub fn open_stream_mut(&mut self, path: impl AsRef<Path>) -> Result<OwnedCfbStream<'_>> {
    let index = self.required_entry_index(path.as_ref())?;
    if !self.entries[index].is_stream() {
      return Err(Error::invalid(0, "CFB entry is not a stream"));
    }
    let version = self.version;
    Ok(OwnedCfbStream::new(
      Arc::make_mut(&mut self.entries)[index].data.to_mut(),
      version,
    ))
  }
  pub fn replace_stream(&mut self, path: impl AsRef<Path>, data: Vec<u8>) -> Result<Vec<u8>> {
    let path = path.as_ref();
    let index = self.required_entry_index(path)?;
    let entry = &mut Arc::make_mut(&mut self.entries)[index];
    if !entry.is_stream() {
      return Err(Error::invalid(
        0,
        format!("CFB entry {} is not a stream", path.display()),
      ));
    }
    Ok(replace_stream_data(&mut entry.data, data))
  }

  /// Replaces a stream without materializing its previous shared bytes.
  ///
  /// File-root serializers use this path when the old value is intentionally
  /// discarded. The public [`Self::replace_stream`] API still returns the
  /// previous owned value for callers that need it.
  pub(crate) fn overwrite_stream(&mut self, path: impl AsRef<Path>, data: Vec<u8>) -> Result<()> {
    let path = path.as_ref();
    let index = self.required_entry_index(path)?;
    let entry = &mut Arc::make_mut(&mut self.entries)[index];
    if !entry.is_stream() {
      return Err(Error::invalid(
        0,
        format!("CFB entry {} is not a stream", path.display()),
      ));
    }
    entry.data = data.into();
    Ok(())
  }

  pub fn create_or_replace_stream(
    &mut self,
    path: impl AsRef<Path>,
    data: Vec<u8>,
  ) -> Result<Option<Vec<u8>>> {
    let path = path.as_ref();
    match self.entry_index(path) {
      Some(index) if self.entries[index].is_stream() => Ok(Some(replace_stream_data(
        &mut Arc::make_mut(&mut self.entries)[index].data,
        data,
      ))),
      Some(index) => Err(Error::invalid(
        0,
        format!(
          "CFB entry {} is not a stream",
          self.entries[index].path.display()
        ),
      )),
      None => {
        self.create_stream(path, data)?;
        Ok(None)
      }
    }
  }

  pub(crate) fn upsert_stream(&mut self, path: impl AsRef<Path>, data: Vec<u8>) -> Result<()> {
    let path = path.as_ref();
    match self.entry_index(path) {
      Some(index) if self.entries[index].is_stream() => {
        Arc::make_mut(&mut self.entries)[index].data = data.into();
        Ok(())
      }
      Some(index) => Err(Error::invalid(
        0,
        format!(
          "CFB entry {} is not a stream",
          self.entries[index].path.display()
        ),
      )),
      None => self.create_stream(path, data),
    }
  }

  pub fn replace_storage_class_id(
    &mut self,
    path: impl AsRef<Path>,
    class_id: Guid,
  ) -> Result<Guid> {
    let path = path.as_ref();
    let index = self.required_entry_index(path)?;
    let entry = &mut Arc::make_mut(&mut self.entries)[index];
    if !entry.is_storage() {
      return Err(Error::invalid(
        0,
        format!("CFB entry {} is not a storage", path.display()),
      ));
    }
    Ok(std::mem::replace(&mut entry.clsid, class_id))
  }

  pub fn replace_state_bits(&mut self, path: impl AsRef<Path>, bits: u32) -> Result<u32> {
    let index = self.required_entry_index(path.as_ref())?;
    Ok(std::mem::replace(
      &mut Arc::make_mut(&mut self.entries)[index].state_bits,
      bits,
    ))
  }

  pub fn replace_creation_time(
    &mut self,
    path: impl AsRef<Path>,
    time: FileTime,
  ) -> Result<FileTime> {
    let index = self.required_entry_index(path.as_ref())?;
    if self.entries[index].kind != EntryKind::Storage {
      return Err(Error::invalid(
        0,
        "CFB creation time is writable only for non-root storage entries",
      ));
    }
    Ok(std::mem::replace(
      &mut Arc::make_mut(&mut self.entries)[index].created,
      time,
    ))
  }

  pub fn replace_modified_time(
    &mut self,
    path: impl AsRef<Path>,
    time: FileTime,
  ) -> Result<FileTime> {
    let index = self.required_entry_index(path.as_ref())?;
    if self.entries[index].is_stream() {
      return Err(Error::invalid(
        0,
        "CFB stream modified time must remain zero",
      ));
    }
    Ok(std::mem::replace(
      &mut Arc::make_mut(&mut self.entries)[index].modified,
      time,
    ))
  }

  pub fn create_storage(&mut self, path: impl AsRef<Path>) -> Result<()> {
    self.create_entry(path.as_ref(), EntryKind::Storage, Vec::new())
  }
  pub fn create_storage_all(&mut self, path: impl AsRef<Path>) -> Result<()> {
    let names = name::path_components(path.as_ref())
      .ok_or_else(|| Error::invalid(0, format!("invalid CFB path {}", path.as_ref().display())))?;
    let mut parent = 0usize;
    for name in names {
      let parent_path = self.entries[parent].path.clone();
      let candidate = parent_path.join(&name);
      match self.entry_index(&candidate) {
        Some(index) if self.entries[index].is_storage() => parent = index,
        Some(index) => {
          return Err(Error::invalid(
            0,
            format!(
              "CFB entry {} is not a storage",
              self.entries[index].path.display()
            ),
          ));
        }
        None => {
          self.create_storage(&candidate)?;
          parent = self.required_entry_index(&candidate)?;
        }
      }
    }
    Ok(())
  }
  pub fn create_stream(&mut self, path: impl AsRef<Path>, data: Vec<u8>) -> Result<()> {
    self.create_entry(path.as_ref(), EntryKind::Stream, data)
  }

  pub fn rename_entry(&mut self, path: impl AsRef<Path>, new_name: &str) -> Result<()> {
    let path = path.as_ref();
    let index = self.required_entry_index(path)?;
    if self.entries[index].kind == EntryKind::Root {
      return Err(Error::invalid(0, "CFB root entry cannot be renamed"));
    }
    name::validate_entry_name(new_name)?;
    let old_path = self.entries[index].path.clone();
    let parent = old_path
      .parent()
      .ok_or_else(|| Error::invalid(0, "CFB entry path has no parent"))?;
    if self.entries.iter().enumerate().any(|(sibling, entry)| {
      sibling != index
        && entry.path.parent() == Some(parent)
        && name::names_equal(&entry.name, new_name)
    }) {
      return Err(Error::invalid(
        0,
        format!("CFB sibling name {new_name} already exists case-insensitively"),
      ));
    }
    let new_path = parent.join(new_name);
    let entries = Arc::make_mut(&mut self.entries);
    for entry in entries.iter_mut() {
      let Ok(suffix) = entry.path.strip_prefix(&old_path) else {
        continue;
      };
      entry.path = if suffix.as_os_str().is_empty() {
        new_path.clone()
      } else {
        new_path.join(suffix)
      };
    }
    entries[index].name = new_name.to_owned();
    Ok(())
  }

  pub fn remove_entry(&mut self, path: impl AsRef<Path>) -> Result<Entry> {
    let path = path.as_ref();
    let index = self.required_entry_index(path)?;
    if self.entries[index].kind == EntryKind::Root {
      return Err(Error::invalid(0, "CFB root entry cannot be removed"));
    }
    let actual_path = self.entries[index].path.clone();
    if self
      .entries
      .iter()
      .any(|entry| entry.path != actual_path && entry.path.starts_with(&actual_path))
    {
      return Err(Error::invalid(
        0,
        format!("CFB storage {} is not empty", path.display()),
      ));
    }
    Ok(Arc::make_mut(&mut self.entries).remove(index))
  }

  pub fn remove_stream(&mut self, path: impl AsRef<Path>) -> Result<Entry> {
    let index = self.required_entry_index(path.as_ref())?;
    if !self.entries[index].is_stream() {
      return Err(Error::invalid(0, "CFB entry is not a stream"));
    }
    self.remove_entry(path)
  }

  pub fn remove_storage(&mut self, path: impl AsRef<Path>) -> Result<Entry> {
    let index = self.required_entry_index(path.as_ref())?;
    if self.entries[index].kind != EntryKind::Storage {
      return Err(Error::invalid(0, "CFB entry is not a non-root storage"));
    }
    self.remove_entry(path)
  }

  pub fn remove_storage_all(&mut self, path: impl AsRef<Path>) -> Result<Vec<Entry>> {
    let index = self.required_entry_index(path.as_ref())?;
    if self.entries[index].is_stream() {
      return Err(Error::invalid(0, "CFB entry is not a storage"));
    }
    let root = self.entries[index].path.clone();
    let remove_root = self.entries[index].kind != EntryKind::Root;
    let mut removed = Vec::new();
    let mut kept = Vec::with_capacity(self.entries.len());
    let entries = Arc::make_mut(&mut self.entries);
    for entry in entries.drain(..) {
      let matches = entry.path.starts_with(&root) && (remove_root || entry.path != root);
      if matches {
        removed.push(entry);
      } else {
        kept.push(entry);
      }
    }
    *entries = kept;
    removed.sort_by(|left, right| {
      right
        .path
        .components()
        .count()
        .cmp(&left.path.components().count())
    });
    Ok(removed)
  }

  fn create_entry(&mut self, path: &Path, kind: EntryKind, data: Vec<u8>) -> Result<()> {
    let mut names = name::path_components(path)
      .ok_or_else(|| Error::invalid(0, format!("invalid CFB path {}", path.display())))?;
    let name = names
      .pop()
      .ok_or_else(|| Error::invalid(0, "CFB root entry already exists"))?;
    name::validate_entry_name(&name)?;
    if self.entry_index(path).is_some() {
      return Err(Error::invalid(
        0,
        format!("CFB entry {} already exists", path.display()),
      ));
    }
    let parent_index = self.entry_index_from_components(&names).ok_or_else(|| {
      Error::invalid(
        0,
        format!("CFB parent for {} does not exist", path.display()),
      )
    })?;
    let parent_entry = &self.entries[parent_index];
    if parent_entry.is_stream() {
      return Err(Error::invalid(
        0,
        format!("CFB parent {} is a stream", parent_entry.path.display()),
      ));
    }
    if self.entries.iter().any(|entry| {
      entry.path.parent() == Some(parent_entry.path.as_path())
        && name::names_equal(&entry.name, &name)
    }) {
      return Err(Error::invalid(
        0,
        format!("CFB sibling name {name} already exists case-insensitively"),
      ));
    }
    let entry_path = parent_entry.path.join(&name);
    Arc::make_mut(&mut self.entries).push(Entry {
      path: entry_path,
      name,
      kind,
      clsid: Guid::ZERO,
      state_bits: 0,
      created: FileTime::ZERO,
      modified: FileTime::ZERO,
      data: data.into(),
    });
    Ok(())
  }

  fn entry_index(&self, path: &Path) -> Option<usize> {
    let root = self
      .entries
      .iter()
      .position(|entry| entry.kind == EntryKind::Root)?;
    let mut current = root;
    for component in path.components() {
      match component {
        std::path::Component::Prefix(_) => return None,
        std::path::Component::RootDir => current = root,
        std::path::Component::CurDir => {}
        std::path::Component::ParentDir => {
          if current == root {
            return None;
          }
          let parent = self.entries[current].path.parent()?;
          current = self.entries.iter().position(|entry| entry.path == parent)?;
        }
        std::path::Component::Normal(requested) => {
          let requested = requested.to_str()?;
          let parent = self.entries[current].path.as_path();
          current = self.entries.iter().position(|entry| {
            entry.path.parent() == Some(parent) && name::names_equal(&entry.name, requested)
          })?;
        }
      }
    }
    Some(current)
  }

  fn entry_index_from_components(&self, names: &[String]) -> Option<usize> {
    let mut current = self
      .entries
      .iter()
      .position(|entry| entry.kind == EntryKind::Root)?;
    for requested in names {
      let parent = self.entries[current].path.as_path();
      current = self.entries.iter().position(|entry| {
        entry.path.parent() == Some(parent) && name::names_equal(&entry.name, requested)
      })?;
    }
    Some(current)
  }

  fn required_entry_index(&self, path: &Path) -> Result<usize> {
    self
      .entry_index(path)
      .ok_or_else(|| Error::invalid(0, format!("CFB entry {} does not exist", path.display())))
  }
  pub fn trailing_data(&self) -> &[u8] {
    &self.trailing_data
  }
  pub fn unallocated_sectors(&self) -> &[Vec<u8>] {
    &self.unallocated_sectors
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    writer::write_compound(self)
  }

  pub fn write_to(&self, mut writer: impl Write) -> Result<()> {
    writer::write_compound_to(self, &mut writer)
  }

  pub(crate) fn write_to_with_stream_overrides(
    &self,
    overrides: &[CfbStreamOverride<'_>],
    mut writer: impl Write,
  ) -> Result<()> {
    let overrides = self.resolve_stream_overrides(overrides)?;
    writer::write_compound_to_with_overrides(self, &overrides, &mut writer)
  }

  pub(crate) fn to_bytes_with_stream_overrides(
    &self,
    overrides: &[CfbStreamOverride<'_>],
  ) -> Result<Vec<u8>> {
    let overrides = self.resolve_stream_overrides(overrides)?;
    writer::write_compound_with_overrides(self, &overrides)
  }

  fn resolve_stream_overrides<'a>(
    &'a self,
    overrides: &'a [CfbStreamOverride<'a>],
  ) -> Result<Vec<CfbStreamOverride<'a>>> {
    overrides
      .iter()
      .map(|stream_override| {
        let entry = self.entry(stream_override.path).ok_or_else(|| {
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
        Ok(CfbStreamOverride::new(
          &entry.path,
          stream_override.len,
          stream_override.writer,
        ))
      })
      .collect()
  }

  pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
    self.write_to(std::fs::File::create(path)?)
  }

  /// Validates the retained physical source representation in strict mode.
  ///
  /// Logical edits are materialized and validated when serialized; the raw
  /// header, allocation tables, and directory accessors intentionally keep
  /// describing the source image until that output is reopened.
  pub fn validate_strict(&self) -> Result<()> {
    if self.header.clsid != [0; 16] {
      return Err(Error::invalid(8, "CFB header CLSID must be zero"));
    }
    if self.header.reserved != [0; 6] {
      return Err(Error::invalid(34, "CFB header reserved bytes must be zero"));
    }
    if !self.header_padding_is_zero {
      return Err(Error::invalid(
        header::HEADER_LEN as u64,
        "CFB v4 header padding must be zero",
      ));
    }
    if self.version == Version::V3 && self.header.number_of_directory_sectors != 0 {
      return Err(Error::invalid(
        40,
        "CFB v3 directory sector count must be zero",
      ));
    }
    if !self.fat.marker_mismatches().is_empty() {
      return Err(Error::invalid(
        0,
        "CFB allocation sectors have non-canonical FAT markers",
      ));
    }
    self.difat.validate_strict()?;
    self.fat.validate_strict()?;
    if !self.mini_fat.sector_count_matches_header() {
      return Err(Error::invalid(
        64,
        "CFB MiniFAT sector count does not match the header",
      ));
    }
    let root_mini_sector_count = self
      .directory
      .root()
      .effective_stream_size(self.version)
      .checked_div(64)
      .and_then(|count| usize::try_from(count).ok())
      .ok_or_else(|| Error::Limit("root mini stream size does not fit usize".into()))?;
    self.mini_fat.validate_strict(root_mini_sector_count)?;
    self.directory.validate_strict(self.version)
  }

  pub fn logical_eq(&self, other: &Self) -> bool {
    self.version == other.version
      && self.entries == other.entries
      && self.unallocated_sectors == other.unallocated_sectors
      && self.trailing_data == other.trailing_data
  }
}

fn replace_stream_data(slot: &mut CfbStreamData, data: Vec<u8>) -> Vec<u8> {
  std::mem::replace(slot, data.into()).into_vec()
}

pub fn round_trip_bytes(bytes: &[u8]) -> Result<Vec<u8>> {
  let original = CompoundFile::from_bytes(bytes)?;
  let output = original.to_bytes()?;
  let reopened = CompoundFile::from_bytes_strict(&output)?;
  if !original.logical_eq(&reopened) {
    return Err(Error::invalid(
      0,
      "CFB logical structure changed after round-trip",
    ));
  }
  Ok(output)
}

#[cfg(test)]
mod tests {
  use std::io::{Seek, SeekFrom};

  use super::*;

  struct TestStreamWriter(Vec<u8>);

  impl CfbStreamWriter for TestStreamWriter {
    fn write_to(&self, writer: &mut dyn Write) -> Result<()> {
      writer.write_all(&self.0)?;
      Ok(())
    }
  }

  #[test]
  fn streaming_overrides_drive_layout_and_enforce_exact_lengths() {
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound.create_stream("/mInI", vec![1]).unwrap();
    compound.create_stream("/Regular", vec![2; 4096]).unwrap();
    let mini = TestStreamWriter(vec![3; 127]);
    let regular = TestStreamWriter(vec![4; 8193]);
    let overrides = [
      CfbStreamOverride::new(Path::new("/Mini"), mini.0.len(), &mini),
      CfbStreamOverride::new(Path::new("/Regular"), regular.0.len(), &regular),
    ];

    let bytes = compound.to_bytes_with_stream_overrides(&overrides).unwrap();
    let reopened = CompoundFile::from_bytes(&bytes).unwrap();
    assert_eq!(reopened.stream("/Mini"), Some(mini.0.as_slice()));
    assert_eq!(reopened.stream("/Regular"), Some(regular.0.as_slice()));

    let under = [CfbStreamOverride::new(
      Path::new("/Mini"),
      mini.0.len() + 1,
      &mini,
    )];
    assert!(compound.to_bytes_with_stream_overrides(&under).is_err());
    let over = [CfbStreamOverride::new(
      Path::new("/Mini"),
      mini.0.len() - 1,
      &mini,
    )];
    assert!(compound.to_bytes_with_stream_overrides(&over).is_err());
  }

  #[test]
  fn empty_compound_file_round_trips() {
    for version in [Version::V3, Version::V4] {
      let source = CompoundFile::new(version).unwrap().to_bytes().unwrap();
      let output = round_trip_bytes(&source).unwrap();
      let parsed = CompoundFile::from_bytes(&output).unwrap();
      assert_eq!(parsed.version(), version);
    }
  }

  #[test]
  fn nested_streams_round_trip() {
    let mut source = CompoundFile::new(Version::V3).unwrap();
    source.create_storage("/Macros").unwrap();
    source.create_storage("/Macros/VBA").unwrap();
    source
      .create_stream("/WordDocument", b"word".to_vec())
      .unwrap();
    source
      .create_stream("/Macros/VBA/dir", b"vba".to_vec())
      .unwrap();
    let bytes = source.to_bytes().unwrap();
    let output = round_trip_bytes(&bytes).unwrap();
    let parsed = CompoundFile::from_bytes(&output).unwrap();
    assert_eq!(
      parsed.entry("/WordDocument").unwrap().data.as_slice(),
      b"word"
    );
    assert_eq!(
      parsed.entry("/Macros/VBA/dir").unwrap().data.as_slice(),
      b"vba"
    );
  }

  #[test]
  fn native_stream_editing_crosses_mini_stream_cutoff_in_v3_and_v4() {
    for version in [Version::V3, Version::V4] {
      let mut source = CompoundFile::new(version).unwrap();
      source.create_storage("/Data").unwrap();
      source
        .create_stream("/Data/Small", b"small".to_vec())
        .unwrap();
      source
        .create_stream("/Data/Large", vec![0x11; 5_000])
        .unwrap();

      let mut compound = CompoundFile::from_bytes(&source.to_bytes().unwrap()).unwrap();
      assert_eq!(compound.stream("/Data/Small"), Some(b"small".as_slice()));
      assert_eq!(compound.stream("/Data"), None);
      assert_eq!(
        compound
          .replace_stream("/Data/Small", vec![0x22; 5_001])
          .unwrap(),
        b"small"
      );
      *compound.stream_mut("/Data/Large").unwrap() = vec![0x33; 63];
      assert!(compound.replace_stream("/Data", Vec::new()).is_err());
      assert!(compound.replace_stream("/Missing", Vec::new()).is_err());

      let encoded = compound.to_bytes().unwrap();
      let reopened = CompoundFile::from_bytes(&encoded).unwrap();
      assert_eq!(
        reopened.stream("/Data/Small"),
        Some(vec![0x22; 5_001].as_slice())
      );
      assert_eq!(
        reopened.stream("/Data/Large"),
        Some(vec![0x33; 63].as_slice())
      );
    }
  }

  #[test]
  fn compound_clone_shares_stream_bytes_until_mutation() {
    let mut source = CompoundFile::new(Version::V3).unwrap();
    source.create_stream("/Data", vec![0x31; 65_537]).unwrap();
    let mut cloned = source.clone();

    assert!(Arc::ptr_eq(&source.entries, &cloned.entries));
    assert!(Arc::ptr_eq(
      &source.entry("/Data").unwrap().data.0,
      &cloned.entry("/Data").unwrap().data.0,
    ));
    cloned.stream_mut("/Data").unwrap()[0] = 0x52;
    assert!(!Arc::ptr_eq(&source.entries, &cloned.entries));
    assert_eq!(source.stream("/Data").unwrap()[0], 0x31);
    assert_eq!(cloned.stream("/Data").unwrap()[0], 0x52);
    assert!(!Arc::ptr_eq(
      &source.entry("/Data").unwrap().data.0,
      &cloned.entry("/Data").unwrap().data.0,
    ));
  }

  #[test]
  fn archived_streams_borrow_contiguous_ranges_and_materialize_fragments_once() {
    let source = Arc::new((0u8..16).collect::<Vec<_>>());
    let contiguous =
      CfbStreamData::archived(Arc::clone(&source), std::iter::once(3..11).collect(), 8).unwrap();
    let CfbStreamBacking::Archived {
      ranges,
      materialized,
      ..
    } = contiguous.0.as_ref()
    else {
      panic!("stream should retain archived backing")
    };
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0], 3..11);
    assert!(materialized.get().is_none());
    assert_eq!(contiguous.as_slice(), &source[3..11]);
    assert!(materialized.get().is_none());

    let mut fragmented =
      CfbStreamData::archived(Arc::clone(&source), vec![1..3, 7..10], 5).unwrap();
    let shared = fragmented.clone();
    assert_eq!(fragmented.as_slice(), [1, 2, 7, 8, 9]);
    let CfbStreamBacking::Archived { materialized, .. } = fragmented.0.as_ref() else {
      panic!("stream should retain archived backing")
    };
    assert_eq!(
      materialized.get().map(Vec::as_slice),
      Some([1, 2, 7, 8, 9].as_slice())
    );
    assert!(Arc::ptr_eq(&fragmented.0, &shared.0));

    fragmented.to_mut()[0] = 0xff;
    assert_eq!(fragmented.as_slice(), [0xff, 2, 7, 8, 9]);
    assert_eq!(shared.as_slice(), [1, 2, 7, 8, 9]);
    assert!(!Arc::ptr_eq(&fragmented.0, &shared.0));
  }

  #[test]
  fn owned_parser_retains_regular_stream_ranges_from_the_shared_image() {
    let mut source = CompoundFile::new(Version::V3).unwrap();
    source.create_stream("/Data", vec![0x41; 65_537]).unwrap();
    let parsed = CompoundFile::from_vec(source.to_bytes().unwrap()).unwrap();
    let data = &parsed.entry("/Data").unwrap().data;
    let CfbStreamBacking::Archived {
      ranges,
      materialized,
      ..
    } = data.0.as_ref()
    else {
      panic!("parsed stream should retain archived backing")
    };
    assert_eq!(ranges.len(), 1);
    assert!(materialized.get().is_none());
    assert_eq!(data.as_slice(), [0x41; 65_537]);
    assert!(materialized.get().is_none());
  }

  #[test]
  fn streaming_writer_preserves_fragmented_archives_without_materializing_them() {
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound.create_stream("/Data", Vec::new()).unwrap();
    let source = Arc::new(vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    let data = CfbStreamData::archived(Arc::clone(&source), vec![0..2, 4..6], 4).unwrap();
    Arc::make_mut(&mut compound.entries)
      .iter_mut()
      .find(|entry| entry.path == Path::new("/Data"))
      .unwrap()
      .data = data.clone();

    let mut output = Vec::new();
    compound.write_to(&mut output).unwrap();
    let CfbStreamBacking::Archived { materialized, .. } = data.0.as_ref() else {
      panic!("stream should retain archived backing")
    };
    assert!(materialized.get().is_none());
    assert_eq!(
      CompoundFile::from_bytes_strict(&output)
        .unwrap()
        .stream("/Data"),
      Some([0x11, 0x22, 0x55, 0x66].as_slice())
    );
  }

  #[test]
  fn owned_stream_set_len_zero_fills_and_crosses_the_mini_stream_cutoff() {
    for version in [Version::V3, Version::V4] {
      let mut compound = CompoundFile::new(version).unwrap();
      compound.create_stream("/Data", vec![0x11; 1_000]).unwrap();
      {
        let mut stream = compound.open_stream_mut("/Data").unwrap();
        stream.set_len(5_000).unwrap();
        assert_eq!(stream.len(), 5_000);
        stream.seek(SeekFrom::Start(4_999)).unwrap();
        stream.write_all(&[0x22]).unwrap();
        stream.seek(SeekFrom::Start(999)).unwrap();
        let mut boundary = [0; 2];
        stream.read_exact(&mut boundary).unwrap();
        assert_eq!(boundary, [0x11, 0]);
        stream.set_len(500).unwrap();
        assert_eq!(stream.len(), 500);
        assert_eq!(stream.stream_position().unwrap(), 500);
      }

      let bytes = compound.to_bytes().unwrap();
      let reopened = CompoundFile::from_bytes_strict(&bytes).unwrap();
      assert_eq!(reopened.stream("/Data"), Some([0x11; 500].as_slice()));
    }
  }

  #[test]
  fn native_directory_creation_builds_nested_v3_and_v4_trees() {
    for version in [Version::V3, Version::V4] {
      let mut compound = CompoundFile::new(version).unwrap();
      assert_eq!(compound.version(), version);
      assert_eq!(compound.entries().len(), 1);

      compound.create_storage("/Data").unwrap();
      compound.create_storage("/Data/Nested").unwrap();
      compound
        .create_stream("/Data/Nested/Mini", vec![0x44; 63])
        .unwrap();
      compound
        .create_stream("/Regular", vec![0x55; 4_096])
        .unwrap();
      compound.create_stream("/Empty", Vec::new()).unwrap();
      assert!(compound.create_storage("/").is_err());
      compound.create_storage("relative").unwrap();
      assert!(compound.entry("/RELATIVE").unwrap().is_storage());
      assert!(compound.create_storage("/Missing/Child").is_err());
      assert!(compound.create_storage("/Regular/Child").is_err());
      assert!(compound.create_stream("/Data", Vec::new()).is_err());
      assert!(compound.create_stream("/data", Vec::new()).is_err());
      assert!(compound.create_stream("/Bad:Name", Vec::new()).is_err());
      compound.create_stream("/Bad\0Name", Vec::new()).unwrap();
      compound.create_stream("/Data/../Oops", Vec::new()).unwrap();
      assert!(compound.entry("/Oops").unwrap().is_stream());
      assert!(
        compound
          .create_stream(format!("/{}", "x".repeat(32)), Vec::new())
          .is_err()
      );
      compound.rename_entry("/Data", "Archive").unwrap();
      assert!(compound.entry("/Data").is_none());
      assert!(compound.entry("/Archive/Nested/Mini").is_some());
      compound
        .rename_entry("/Archive/Nested/Mini", "mini")
        .unwrap();
      compound
        .create_stream("/Archive/Nested/Other", b"other".to_vec())
        .unwrap();
      assert!(
        compound
          .rename_entry("/Archive/Nested/mini", "OTHER")
          .is_err()
      );
      assert!(compound.rename_entry("/Regular", "archive").is_err());
      assert!(compound.rename_entry("/", "Root2").is_err());
      assert!(compound.rename_entry("/Missing", "Missing2").is_err());
      assert!(compound.rename_entry("/Regular", "Bad:Name").is_err());
      assert!(compound.remove_entry("/Archive").is_err());
      let removed = compound.remove_entry("/Empty").unwrap();
      assert_eq!(removed.kind, EntryKind::Stream);
      assert!(compound.remove_entry("/Empty").is_err());
      compound.remove_storage("RELATIVE").unwrap();
      compound.create_storage("/Vacant").unwrap();
      assert_eq!(
        compound.remove_entry("/Vacant").unwrap().kind,
        EntryKind::Storage
      );
      assert!(compound.remove_entry("/").is_err());

      let encoded = compound.to_bytes().unwrap();
      let reopened = CompoundFile::from_bytes(&encoded).unwrap();
      assert_eq!(
        reopened.stream("/Archive/Nested/mini"),
        Some([0x44; 63].as_slice())
      );
      assert_eq!(
        reopened.stream("/Archive/Nested/Other"),
        Some(b"other".as_slice())
      );
      assert_eq!(
        reopened.stream("/Regular"),
        Some(vec![0x55; 4_096].as_slice())
      );
      assert!(reopened.stream("/Empty").is_none());
      assert!(reopened.entry("/Archive/Nested").unwrap().is_storage());
    }
  }

  #[test]
  fn trailing_data_is_preserved_outside_sector_space() {
    let mut bytes = CompoundFile::new(Version::V3).unwrap().to_bytes().unwrap();
    bytes.extend_from_slice(b"trailing");
    let parsed = CompoundFile::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.trailing_data(), b"trailing");
    let output = parsed.to_bytes().unwrap();
    assert!(output.ends_with(b"trailing"));
    CompoundFile::from_bytes_strict(&output).unwrap();
    assert!(parsed.logical_eq(&CompoundFile::from_bytes(&output).unwrap()));
  }

  #[test]
  fn unallocated_physical_sectors_are_preserved() {
    let mut bytes = CompoundFile::new(Version::V3).unwrap().to_bytes().unwrap();
    bytes.extend_from_slice(&[0x5a; 512]);
    let parsed = CompoundFile::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.unallocated_sectors(), [vec![0x5a; 512]]);
    let output = parsed.to_bytes().unwrap();
    let reopened = CompoundFile::from_bytes(&output).unwrap();
    assert!(parsed.logical_eq(&reopened));

    let mut many = CompoundFile::new(Version::V3).unwrap();
    many.unallocated_sectors = Arc::new(vec![vec![0x6b; 512]; 130]);
    let output = many.to_bytes().unwrap();
    let reopened = CompoundFile::from_bytes_strict(&output).unwrap();
    assert_eq!(reopened.unallocated_sectors().len(), 130);
    assert!(many.logical_eq(&reopened));
  }

  #[test]
  fn strict_open_rejects_compatibility_only_header_and_directory_shapes() {
    let bytes = CompoundFile::new(Version::V3).unwrap().to_bytes().unwrap();
    CompoundFile::from_bytes_strict(&bytes).unwrap();

    let mut reserved = bytes.clone();
    reserved[34] = 1;
    assert!(CompoundFile::from_bytes(&reserved).is_ok());
    assert!(CompoundFile::from_bytes_strict(&reserved).is_err());

    let mut noncanonical_difat_end = bytes.clone();
    noncanonical_difat_end[68..72].copy_from_slice(&header::FREE_SECTOR.to_le_bytes());
    assert!(CompoundFile::from_bytes(&noncanonical_difat_end).is_ok());
    assert!(CompoundFile::from_bytes_strict(&noncanonical_difat_end).is_err());

    let header = Header::from_bytes(&bytes).unwrap();
    let directory_offset =
      header.sector_len() + header.first_directory_sector as usize * header.sector_len();
    let mut unterminated = bytes.clone();
    unterminated[directory_offset + "Root Entry".encode_utf16().count() * 2] = 1;
    assert!(CompoundFile::from_bytes(&unterminated).is_ok());
    assert!(CompoundFile::from_bytes_strict(&unterminated).is_err());

    let mut noncanonical_free_entry = bytes.clone();
    noncanonical_free_entry[directory_offset + 2 * directory::DIRECTORY_ENTRY_LEN + 67] = 1;
    assert!(CompoundFile::from_bytes(&noncanonical_free_entry).is_ok());
    assert!(CompoundFile::from_bytes_strict(&noncanonical_free_entry).is_err());

    let fat_sector = header.difat[0] as usize;
    let fat_offset = header.sector_len() + fat_sector * header.sector_len();
    let file_sector_count = bytes.len() / header.sector_len() - 1;
    let mut noncanonical_fat_padding = bytes.clone();
    let padding = fat_offset + file_sector_count * 4;
    noncanonical_fat_padding[padding..padding + 4]
      .copy_from_slice(&allocation::END_OF_CHAIN.to_le_bytes());
    assert!(CompoundFile::from_bytes(&noncanonical_fat_padding).is_ok());
    assert!(CompoundFile::from_bytes_strict(&noncanonical_fat_padding).is_err());

    let mut nonzero_root_creation = reserved;
    nonzero_root_creation[directory_offset + 100] = 1;
    let compatible = CompoundFile::from_bytes(&nonzero_root_creation).unwrap();
    assert_eq!(compatible.root_entry().created, FileTime::ZERO);
    assert_eq!(compatible.directory().root().creation_time.ticks(), 1);
    assert!(CompoundFile::from_bytes_strict(&nonzero_root_creation).is_err());
    CompoundFile::from_bytes_strict(&compatible.to_bytes().unwrap()).unwrap();

    let mut tree = CompoundFile::new(Version::V3).unwrap();
    for name in ["A", "B", "C", "D", "E", "F"] {
      tree.create_stream(name, Vec::new()).unwrap();
    }
    let mut transitive_order_violation = tree.to_bytes().unwrap();
    let canonical_tree = CompoundFile::from_bytes(&transitive_order_violation).unwrap();
    let header = Header::from_bytes(&transitive_order_violation).unwrap();
    let directory_offset =
      header.sector_len() + header.first_directory_sector as usize * header.sector_len();
    assert_eq!(
      canonical_tree.directory().entries()[3].raw_name().unwrap(),
      "C"
    );
    transitive_order_violation[directory_offset + 3 * directory::DIRECTORY_ENTRY_LEN] = b'E';
    assert!(CompoundFile::from_bytes(&transitive_order_violation).is_ok());
    assert!(CompoundFile::from_bytes_strict(&transitive_order_violation).is_err());

    let mut v4_padding = CompoundFile::new(Version::V4).unwrap().to_bytes().unwrap();
    v4_padding[header::HEADER_LEN] = 1;
    assert!(CompoundFile::from_bytes(&v4_padding).is_ok());
    assert!(CompoundFile::from_bytes_strict(&v4_padding).is_err());
  }

  #[test]
  fn storage_walk_uses_cfb_preorder() {
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound.create_storage("/Data").unwrap();
    compound.create_stream("/Data/B", Vec::new()).unwrap();
    compound.create_storage("/Data/C").unwrap();
    compound.create_stream("/Data/C/X", Vec::new()).unwrap();
    compound.create_stream("/Data/AA", Vec::new()).unwrap();

    let paths: Vec<_> = compound
      .walk_storage("/data")
      .unwrap()
      .into_iter()
      .map(|entry| entry.path.as_path())
      .collect();
    assert_eq!(
      paths,
      ["/Data", "/Data/B", "/Data/C", "/Data/C/X", "/Data/AA"].map(Path::new)
    );
  }

  #[test]
  fn logical_api_matches_cfb_path_and_metadata_semantics() {
    let mut compound = CompoundFile::new(Version::V4).unwrap();
    compound.create_storage_all("Data/Nested/Leaf").unwrap();
    assert!(compound.is_storage("/data/NESTED/leaf"));
    assert_eq!(compound.children("DATA/NESTED").unwrap().len(), 1);

    assert_eq!(
      compound
        .create_or_replace_stream("data/nested/leaf/Value", b"first".to_vec())
        .unwrap(),
      None
    );
    assert_eq!(
      compound.stream("/DATA/NESTED/LEAF/value"),
      Some(b"first".as_slice())
    );
    let mut reader = compound.open_stream("data/nested/leaf/value").unwrap();
    let mut streamed = Vec::new();
    reader.read_to_end(&mut streamed).unwrap();
    assert_eq!(streamed, b"first");
    {
      let mut stream = compound.open_stream_mut("data/nested/leaf/value").unwrap();
      stream.seek(SeekFrom::End(0)).unwrap();
      stream.write_all(b"!").unwrap();
    }
    assert_eq!(
      compound.stream("data/nested/leaf/value"),
      Some(b"first!".as_slice())
    );
    assert_eq!(
      compound
        .create_or_replace_stream("/Data/Nested/Leaf/VALUE", b"second".to_vec())
        .unwrap(),
      Some(b"first!".to_vec())
    );

    let clsid = Guid::from_fields(0x1234_5678, 0x9abc, 0xdef0, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(
      compound
        .replace_storage_class_id("/data/nested", clsid)
        .unwrap(),
      Guid::ZERO
    );
    assert_eq!(
      compound.replace_state_bits("DATA/NESTED", 0x55aa).unwrap(),
      0
    );
    assert_eq!(
      compound
        .replace_creation_time("data/nested", FileTime::from_ticks(11))
        .unwrap(),
      FileTime::ZERO
    );
    assert_eq!(
      compound
        .replace_modified_time("/", FileTime::from_ticks(22))
        .unwrap(),
      FileTime::ZERO
    );
    assert!(
      compound
        .replace_creation_time("/", FileTime::from_ticks(1))
        .is_err()
    );
    assert!(
      compound
        .replace_modified_time("data/nested/leaf/value", FileTime::from_ticks(1))
        .is_err()
    );

    let walked = compound.walk_storage("/DATA/NESTED").unwrap();
    assert_eq!(walked.len(), 3);

    let mut bytes = Vec::new();
    compound.write_to(&mut bytes).unwrap();
    let mut reopened = CompoundFile::from_reader(bytes.as_slice()).unwrap();
    let nested = reopened.entry("/data/NESTED").unwrap();
    assert_eq!(nested.clsid, clsid);
    assert_eq!(nested.state_bits, 0x55aa);
    assert_eq!(nested.created, FileTime::from_ticks(11));
    assert_eq!(reopened.root_entry().modified, FileTime::from_ticks(22));
    assert_eq!(
      reopened.stream("data/nested/leaf/value"),
      Some(b"second".as_slice())
    );

    assert_eq!(
      reopened
        .remove_stream("DATA/NESTED/LEAF/VALUE")
        .unwrap()
        .data
        .as_slice(),
      b"second"
    );
    let removed = reopened.remove_storage_all("data/nested").unwrap();
    assert_eq!(removed.len(), 2);
    assert!(reopened.entry("/Data/Nested").is_none());
    assert!(reopened.entry("/Data").is_some());
  }
}
