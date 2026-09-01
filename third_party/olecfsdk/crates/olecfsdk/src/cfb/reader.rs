use std::{
  io::{self, BufRead, Cursor, Read, Seek, SeekFrom, Write},
  path::{Path, PathBuf},
  sync::Arc,
};

use crate::{
  Error, Result,
  common::{FileTime, Guid},
  io::{SdkWrite, Writer},
  limits::Limits,
};

use super::{
  CompoundFile, Difat, Directory, DirectoryObjectType, EntryKind, Fat, FatEntry, Header, MiniFat,
  MiniFatEntry, MiniSectorId, SectorId, Version,
  allocation::END_OF_CHAIN,
  directory::{DIRECTORY_ENTRY_LEN, DirectoryEntry},
  header::{FREE_SECTOR, HEADER_LEN, MINI_STREAM_CUTOFF},
  name,
  sector::{CfbReadAt, ReadAtSectorSource, SectorRead, SectorWrite, SeekSectorSource},
};

const DEFAULT_STREAM_BUFFER_SIZE: usize = 1024 * 1024;
const MINI_SECTOR_LEN: usize = 64;

/// Metadata for an MS-CFB directory entry without eagerly materializing its stream payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryInfo {
  pub stream_id: u32,
  pub path: PathBuf,
  pub name: String,
  pub kind: EntryKind,
  pub clsid: Guid,
  pub state_bits: u32,
  pub created: FileTime,
  pub modified: FileTime,
  pub stream_len: u64,
}

impl EntryInfo {
  pub fn is_stream(&self) -> bool {
    self.kind == EntryKind::Stream
  }

  pub fn is_storage(&self) -> bool {
    self.kind != EntryKind::Stream
  }
}

/// A seekable MS-CFB reader that keeps allocation metadata in memory and reads
/// stream payloads through bounded buffers.
pub struct CompoundFileReader<R> {
  version: Version,
  header: Header,
  difat: Difat,
  fat: Fat,
  mini_fat: MiniFat,
  directory: Directory,
  entries: Vec<EntryInfo>,
  root_mini_chain: Arc<[SectorId]>,
  source: SeekSectorSource<R>,
  header_padding_is_zero: bool,
  limits: Limits,
  stream_buffer_size: usize,
}

impl<R: Read + Seek> CompoundFileReader<R> {
  pub fn from_reader(reader: R) -> Result<Self> {
    Self::from_reader_with_limits(reader, Limits::default())
  }

  pub fn from_reader_strict(reader: R) -> Result<Self> {
    let compound = Self::from_reader(reader)?;
    compound.validate_strict()?;
    Ok(compound)
  }

  pub fn from_reader_with_limits(reader: R, limits: Limits) -> Result<Self> {
    Self::from_reader_with_buffer_size(reader, limits, DEFAULT_STREAM_BUFFER_SIZE)
  }

  pub fn from_reader_with_buffer_size(
    mut reader: R,
    limits: Limits,
    stream_buffer_size: usize,
  ) -> Result<Self> {
    let original_len = reader.seek(SeekFrom::End(0))?;
    if original_len > limits.max_file_size {
      return Err(Error::Limit(format!(
        "file length {original_len} exceeds {}",
        limits.max_file_size
      )));
    }
    reader.seek(SeekFrom::Start(0))?;
    let mut header_bytes = [0; HEADER_LEN];
    reader.read_exact(&mut header_bytes)?;
    let header = Header::from_bytes(&header_bytes)?;
    let sector_len = header.sector_len();
    if limits.max_allocation < sector_len {
      return Err(Error::Limit(format!(
        "stream buffer requires one {sector_len}-byte sector but max allocation is {}",
        limits.max_allocation
      )));
    }
    if original_len < sector_len as u64 {
      return Err(Error::invalid(
        0,
        "CFB file is shorter than its header sector",
      ));
    }
    let header_padding_is_zero = if sector_len > HEADER_LEN {
      let mut padding = vec![0; sector_len - HEADER_LEN];
      reader.read_exact(&mut padding)?;
      padding.iter().all(|byte| *byte == 0)
    } else {
      true
    };
    let mut source = SeekSectorSource::new(reader, original_len, &header, limits.max_file_size)?;
    if source.sector_count() > u32::MAX as usize {
      return Err(Error::Limit("CFB sector count exceeds u32".into()));
    }
    source.full_sector(SectorId::new(header.first_directory_sector)?)?;
    let difat = Difat::read(&header, &mut source, limits)?;
    let fat = Fat::read(&difat, &mut source)?;
    let directory_sectors = fat.chain(header.first_directory_sector, source.sector_count())?;
    let mini_fat = MiniFat::read(&header, &fat, &mut source, limits)?;
    let directory = Directory::read(
      directory_sectors,
      header.number_of_directory_sectors,
      &mut source,
      limits,
    )?;
    let version = header.version();
    let entries = read_entry_info(&directory, version, limits)?;
    let root = directory.root();
    let root_len = root.effective_stream_size(version);
    let root_mini_chain = regular_chain(
      &fat,
      root.start_sector,
      root_len,
      source.sector_count(),
      source.sector_len(),
    )?;
    ensure_physical_capacity(&source, &root_mini_chain, root_len, "root mini")?;
    let root_mini_chain = Arc::from(root_mini_chain);
    let stream_buffer_size = stream_buffer_size
      .max(sector_len)
      .min(limits.max_allocation);
    Ok(Self {
      version,
      header,
      difat,
      fat,
      mini_fat,
      directory,
      entries,
      root_mini_chain,
      source,
      header_padding_is_zero,
      limits,
      stream_buffer_size,
    })
  }

  pub fn version(&self) -> Version {
    self.version
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

  pub fn directory(&self) -> &Directory {
    &self.directory
  }

  pub fn entries(&self) -> &[EntryInfo] {
    &self.entries
  }

  pub fn root_entry(&self) -> &EntryInfo {
    self
      .entries
      .iter()
      .find(|entry| entry.kind == EntryKind::Root)
      .expect("validated CFB directory has a root entry")
  }

  pub fn entry(&self, path: impl AsRef<Path>) -> Option<&EntryInfo> {
    self
      .entry_index(path.as_ref())
      .map(|index| &self.entries[index])
  }

  pub fn contains_entry(&self, path: impl AsRef<Path>) -> bool {
    self.entry(path).is_some()
  }

  pub fn is_stream(&self, path: impl AsRef<Path>) -> bool {
    self.entry(path).is_some_and(EntryInfo::is_stream)
  }

  pub fn is_storage(&self, path: impl AsRef<Path>) -> bool {
    self.entry(path).is_some_and(EntryInfo::is_storage)
  }

  pub fn children(&self, path: impl AsRef<Path>) -> Result<Vec<&EntryInfo>> {
    let parent = self.required_entry_index(path.as_ref())?;
    if self.entries[parent].is_stream() {
      return Err(Error::invalid(0, "CFB entry is not a storage"));
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

  pub fn walk_storage(&self, path: impl AsRef<Path>) -> Result<Vec<&EntryInfo>> {
    let root = self.required_entry_index(path.as_ref())?;
    if self.entries[root].is_stream() {
      return Err(Error::invalid(0, "CFB entry is not a storage"));
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

  /// Opens a stream through the generic seekable fallback. The returned
  /// cursor exclusively borrows this reader until it is dropped.
  pub fn open_stream_borrowed(&mut self, path: impl AsRef<Path>) -> Result<CfbStreamMut<'_, R>> {
    let index = self.required_entry_index(path.as_ref())?;
    let entry = &self.entries[index];
    let stream_id = entry.stream_id;
    let len = entry.stream_len;
    let chain = self.stream_chain(index)?;
    let buffer_capacity = usize::try_from(len)
      .unwrap_or(self.stream_buffer_size)
      .min(self.stream_buffer_size)
      .max(1);
    Ok(CfbStreamMut {
      source: &mut self.source,
      header: &mut self.header,
      fat: &mut self.fat,
      mini_fat: &mut self.mini_fat,
      difat: &mut self.difat,
      directory: &mut self.directory,
      entries: &mut self.entries,
      entry_index: index,
      root_mini_chain: &mut self.root_mini_chain,
      version: self.version,
      stream_id,
      chain,
      len,
      position: 0,
      buffer_start: 0,
      buffer_len: 0,
      buffer: vec![0; buffer_capacity],
      max_buffer_capacity: self.stream_buffer_size,
      max_stream_size: self.limits.max_stream_size,
    })
  }

  fn stream_chain(&self, index: usize) -> Result<StreamChain> {
    let entry = &self.entries[index];
    if !entry.is_stream() {
      return Err(Error::invalid(0, "CFB entry is not a stream"));
    }
    let raw = self
      .directory
      .entries()
      .get(entry.stream_id as usize)
      .ok_or_else(|| Error::invalid(0, "CFB stream directory entry is missing"))?;
    if entry.stream_len < MINI_STREAM_CUTOFF as u64 {
      if entry.stream_len != 0 && matches!(raw.start_sector, END_OF_CHAIN | FREE_SECTOR) {
        return Err(Error::invalid(
          0,
          "non-empty stream has no mini-sector chain",
        ));
      }
      let root_len = self.directory.root().effective_stream_size(self.version);
      let mini_sector_count = usize::try_from(root_len / MINI_SECTOR_LEN as u64)
        .map_err(|_| Error::Limit("root mini stream size does not fit usize".into()))?;
      let mini_chain = if entry.stream_len == 0 {
        Vec::new()
      } else {
        self.mini_fat.chain(raw.start_sector, mini_sector_count)?
      };
      ensure_chain_capacity(mini_chain.len(), MINI_SECTOR_LEN, entry.stream_len, "mini")?;
      Ok(StreamChain::Mini {
        mini_chain,
        root_chain: self.root_mini_chain.clone(),
        root_len,
      })
    } else {
      Ok(StreamChain::Regular(regular_chain(
        &self.fat,
        raw.start_sector,
        entry.stream_len,
        self.source.sector_count(),
        self.source.sector_len(),
      )?))
    }
  }

  /// Consumes the streaming reader and materializes the existing full owned
  /// model. This is the explicit fallback for callers that need unrestricted
  /// logical editing before the file-backed editor is used.
  pub fn into_owned(self) -> Result<CompoundFile> {
    let limits = self.limits;
    let mut reader = self.source.into_inner();
    reader.seek(SeekFrom::Start(0))?;
    CompoundFile::from_reader_with_limits(reader, limits)
  }

  pub fn into_inner(self) -> R {
    self.source.into_inner()
  }

  pub fn validate_strict(&self) -> Result<()> {
    if self.header.clsid != [0; 16] {
      return Err(Error::invalid(8, "CFB header CLSID must be zero"));
    }
    if self.header.reserved != [0; 6] {
      return Err(Error::invalid(34, "CFB header reserved bytes must be zero"));
    }
    if !self.header_padding_is_zero {
      return Err(Error::invalid(
        HEADER_LEN as u64,
        "CFB v4 header padding must be zero",
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
      .checked_div(MINI_SECTOR_LEN as u64)
      .and_then(|count| usize::try_from(count).ok())
      .ok_or_else(|| Error::Limit("root mini stream size does not fit usize".into()))?;
    self.mini_fat.validate_strict(root_mini_sector_count)?;
    self.directory.validate_strict(self.version)?;
    self.validate_stream_chains(root_mini_sector_count)
  }

  fn entry_index(&self, path: &Path) -> Option<usize> {
    let names = name::path_components(path)?;
    let mut current = self
      .entries
      .iter()
      .position(|entry| entry.kind == EntryKind::Root)?;
    for requested in names {
      let parent = self.entries[current].path.as_path();
      current = self.entries.iter().position(|entry| {
        entry.path.parent() == Some(parent) && name::names_equal(&entry.name, &requested)
      })?;
    }
    Some(current)
  }

  fn required_entry_index(&self, path: &Path) -> Result<usize> {
    self
      .entry_index(path)
      .ok_or_else(|| Error::invalid(0, format!("CFB entry {} does not exist", path.display())))
  }

  fn validate_stream_chains(&self, root_mini_sector_count: usize) -> Result<()> {
    for entry in self.entries.iter().filter(|entry| entry.is_stream()) {
      if entry.stream_len == 0 {
        continue;
      }
      let raw = self
        .directory
        .entries()
        .get(entry.stream_id as usize)
        .ok_or_else(|| Error::invalid(0, "CFB stream directory entry is missing"))?;
      if entry.stream_len < MINI_STREAM_CUTOFF as u64 {
        if matches!(raw.start_sector, END_OF_CHAIN | FREE_SECTOR) {
          return Err(Error::invalid(
            0,
            "non-empty stream has no mini-sector chain",
          ));
        }
        let chain = self
          .mini_fat
          .chain(raw.start_sector, root_mini_sector_count)?;
        ensure_chain_capacity(chain.len(), MINI_SECTOR_LEN, entry.stream_len, "mini")?;
      } else {
        let chain = regular_chain(
          &self.fat,
          raw.start_sector,
          entry.stream_len,
          self.source.sector_count(),
          self.source.sector_len(),
        )?;
        ensure_physical_capacity(&self.source, &chain, entry.stream_len, "regular")?;
      }
    }
    Ok(())
  }
}

impl<R: Read + Seek + CfbReadAt> CompoundFileReader<R> {
  /// Opens an independent read-only stream cursor.
  ///
  /// Multiple cursors may coexist because payload reads use positional I/O
  /// and do not mutate the backing object's shared seek position.
  pub fn open_stream(&self, path: impl AsRef<Path>) -> Result<CfbReadStream<'_, R>> {
    let index = self.required_entry_index(path.as_ref())?;
    let entry = &self.entries[index];
    let chain = self.stream_chain(index)?;
    let buffer_capacity = usize::try_from(entry.stream_len)
      .unwrap_or(self.stream_buffer_size)
      .min(self.stream_buffer_size)
      .max(1);
    Ok(CfbReadStream {
      source: self.source.read_at_source(),
      chain,
      len: entry.stream_len,
      position: 0,
      buffer_start: 0,
      buffer_len: 0,
      buffer: vec![0; buffer_capacity],
    })
  }
}

impl<R: Read + Write + Seek> CompoundFileReader<R> {
  /// Opens an exclusively borrowed writable stream cursor.
  pub fn open_stream_mut(&mut self, path: impl AsRef<Path>) -> Result<CfbStreamMut<'_, R>> {
    self.open_stream_borrowed(path)
  }

  /// Creates an empty stream, replacing an existing stream at the same path.
  pub fn create_stream(&mut self, path: impl AsRef<Path>) -> Result<CfbStreamMut<'_, R>> {
    self.create_stream_with_mode(path.as_ref(), true)
  }

  /// Creates an empty stream and fails if the path already exists.
  pub fn create_new_stream(&mut self, path: impl AsRef<Path>) -> Result<CfbStreamMut<'_, R>> {
    self.create_stream_with_mode(path.as_ref(), false)
  }

  fn create_stream_with_mode(
    &mut self,
    path: &Path,
    overwrite: bool,
  ) -> Result<CfbStreamMut<'_, R>> {
    let names = name::path_components(path)
      .ok_or_else(|| Error::invalid(0, "invalid root-relative CFB path"))?;
    let name = names
      .last()
      .ok_or_else(|| Error::invalid(0, "cannot create the CFB root stream"))?;
    name::validate_entry_name(name)?;
    let canonical = path_from_names(&names);
    if let Some(index) = self.entry_index(&canonical) {
      if !self.entries[index].is_stream() {
        return Err(Error::invalid(
          0,
          "a storage already exists at the stream path",
        ));
      }
      if !overwrite {
        return Err(Error::invalid(0, "CFB stream already exists"));
      }
      let mut stream = self.open_stream_mut(&canonical)?;
      stream.set_len(0).map_err(Error::Io)?;
      return Ok(stream);
    }
    let parent_path = path_from_names(&names[..names.len() - 1]);
    let parent_index = self.required_entry_index(&parent_path)?;
    if self.entries[parent_index].is_stream() {
      return Err(Error::invalid(0, "CFB stream parent is not a storage"));
    }
    let stream_id = self.allocate_directory_entry()?;
    *self
      .directory
      .entry_mut(stream_id)
      .expect("selected unallocated directory entry remains present") =
      DirectoryEntry::empty_named(name, DirectoryObjectType::Stream)?;
    self.entries.push(EntryInfo {
      stream_id,
      path: canonical.clone(),
      name: name.clone(),
      kind: EntryKind::Stream,
      clsid: Guid::ZERO,
      state_bits: 0,
      created: FileTime::ZERO,
      modified: FileTime::ZERO,
      stream_len: 0,
    });
    self
      .entries
      .sort_by(|left, right| left.path.cmp(&right.path));
    self.rebuild_storage_children(&parent_path)?;
    self.persist_directory()?;
    self.open_stream_mut(canonical)
  }

  /// Removes a stream and releases its FAT or MiniFAT allocation chain.
  pub fn remove_stream(&mut self, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    let index = self.required_entry_index(path)?;
    if !self.entries[index].is_stream() {
      return Err(Error::invalid(0, "CFB entry is not a stream"));
    }
    let canonical = self.entries[index].path.clone();
    let parent = canonical
      .parent()
      .ok_or_else(|| Error::invalid(0, "CFB stream has no parent storage"))?
      .to_path_buf();
    let stream_id = self.entries[index].stream_id;
    {
      let mut stream = self.open_stream_mut(&canonical)?;
      stream.set_len(0).map_err(Error::Io)?;
    }
    *self
      .directory
      .entry_mut(stream_id)
      .expect("validated directory entry remains present") = DirectoryEntry::unallocated();
    self.entries.retain(|entry| entry.stream_id != stream_id);
    self.rebuild_storage_children(&parent)?;
    self.persist_directory()
  }

  /// Creates an empty storage object. Its parent storage must already exist.
  pub fn create_storage(&mut self, path: impl AsRef<Path>) -> Result<()> {
    self.create_storage_path(path.as_ref())
  }

  /// Creates a storage object and all missing parent storages.
  pub fn create_storage_all(&mut self, path: impl AsRef<Path>) -> Result<()> {
    let names = name::path_components(path.as_ref())
      .ok_or_else(|| Error::invalid(0, "invalid root-relative CFB path"))?;
    let mut current = PathBuf::from("/");
    for name in names {
      current.push(name);
      if let Some(entry) = self.entry(&current) {
        if entry.is_stream() {
          return Err(Error::invalid(0, "a stream blocks the storage path"));
        }
      } else {
        self.create_storage_path(&current)?;
      }
    }
    Ok(())
  }

  fn create_storage_path(&mut self, path: &Path) -> Result<()> {
    let names = name::path_components(path)
      .ok_or_else(|| Error::invalid(0, "invalid root-relative CFB path"))?;
    let name = names
      .last()
      .ok_or_else(|| Error::invalid(0, "the CFB root storage already exists"))?;
    name::validate_entry_name(name)?;
    let canonical = path_from_names(&names);
    if self.contains_entry(&canonical) {
      return Err(Error::invalid(0, "CFB entry already exists"));
    }
    let parent_path = path_from_names(&names[..names.len() - 1]);
    let parent = self.required_entry_index(&parent_path)?;
    if self.entries[parent].is_stream() {
      return Err(Error::invalid(0, "CFB storage parent is a stream"));
    }
    let stream_id = self.allocate_directory_entry()?;
    *self
      .directory
      .entry_mut(stream_id)
      .expect("selected unallocated directory entry remains present") =
      DirectoryEntry::empty_named(name, DirectoryObjectType::Storage)?;
    self.entries.push(EntryInfo {
      stream_id,
      path: canonical,
      name: name.clone(),
      kind: EntryKind::Storage,
      clsid: Guid::ZERO,
      state_bits: 0,
      created: FileTime::ZERO,
      modified: FileTime::ZERO,
      stream_len: 0,
    });
    self
      .entries
      .sort_by(|left, right| left.path.cmp(&right.path));
    self.rebuild_storage_children(&parent_path)?;
    self.persist_directory()
  }

  /// Removes an empty non-root storage object.
  pub fn remove_storage(&mut self, path: impl AsRef<Path>) -> Result<()> {
    let index = self.required_entry_index(path.as_ref())?;
    let entry = &self.entries[index];
    if entry.kind == EntryKind::Root {
      return Err(Error::invalid(0, "cannot remove the CFB root storage"));
    }
    if entry.is_stream() {
      return Err(Error::invalid(0, "CFB entry is not a storage"));
    }
    let canonical = entry.path.clone();
    if self
      .entries
      .iter()
      .any(|candidate| candidate.path.parent() == Some(canonical.as_path()))
    {
      return Err(Error::invalid(0, "CFB storage is not empty"));
    }
    let parent = canonical.parent().unwrap_or(Path::new("/")).to_path_buf();
    let stream_id = entry.stream_id;
    *self
      .directory
      .entry_mut(stream_id)
      .expect("validated directory entry remains present") = DirectoryEntry::unallocated();
    self
      .entries
      .retain(|candidate| candidate.stream_id != stream_id);
    self.rebuild_storage_children(&parent)?;
    self.persist_directory()
  }

  /// Recursively removes a storage and all descendants. For the root, only
  /// descendants are removed.
  pub fn remove_storage_all(&mut self, path: impl AsRef<Path>) -> Result<()> {
    let root = self
      .entry(path.as_ref())
      .ok_or_else(|| Error::invalid(0, "CFB storage does not exist"))?;
    if root.is_stream() {
      return Err(Error::invalid(0, "CFB entry is not a storage"));
    }
    let root_path = root.path.clone();
    let root_is_file_root = root.kind == EntryKind::Root;
    let mut descendants: Vec<_> = self
      .entries
      .iter()
      .filter(|entry| entry.path != root_path && entry.path.starts_with(&root_path))
      .map(|entry| (entry.path.clone(), entry.kind))
      .collect();
    descendants.sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
    for (path, kind) in descendants {
      if kind == EntryKind::Stream {
        self.remove_stream(&path)?;
      } else {
        self.remove_storage(&path)?;
      }
    }
    if !root_is_file_root {
      self.remove_storage(&root_path)?;
    }
    Ok(())
  }

  /// Sets the CLSID of a storage object, including the root storage.
  pub fn set_storage_clsid(&mut self, path: impl AsRef<Path>, clsid: Guid) -> Result<()> {
    let index = self.required_entry_index(path.as_ref())?;
    if self.entries[index].is_stream() {
      return Err(Error::invalid(0, "CFB entry is not a storage"));
    }
    let stream_id = self.entries[index].stream_id;
    self
      .directory
      .entry_mut(stream_id)
      .expect("validated directory entry remains present")
      .clsid = clsid;
    self.entries[index].clsid = clsid;
    self.persist_directory()
  }

  /// Sets the user-defined state bits of any CFB entry.
  pub fn set_state_bits(&mut self, path: impl AsRef<Path>, state_bits: u32) -> Result<()> {
    let index = self.required_entry_index(path.as_ref())?;
    let stream_id = self.entries[index].stream_id;
    self
      .directory
      .entry_mut(stream_id)
      .expect("validated directory entry remains present")
      .state_bits = state_bits;
    self.entries[index].state_bits = state_bits;
    self.persist_directory()
  }

  /// Sets a storage creation time. MS-CFB requires stream timestamps to be
  /// zero, so stream entries are rejected instead of producing invalid data.
  pub fn set_created_time(&mut self, path: impl AsRef<Path>, time: FileTime) -> Result<()> {
    self.set_storage_time(path.as_ref(), time, true)
  }

  /// Sets a storage modification time. MS-CFB requires stream timestamps to
  /// be zero, so stream entries are rejected instead of producing invalid data.
  pub fn set_modified_time(&mut self, path: impl AsRef<Path>, time: FileTime) -> Result<()> {
    self.set_storage_time(path.as_ref(), time, false)
  }

  fn set_storage_time(&mut self, path: &Path, time: FileTime, created: bool) -> Result<()> {
    let index = self.required_entry_index(path)?;
    if self.entries[index].is_stream() {
      return Err(Error::invalid(
        0,
        "CFB stream creation and modification times must be zero",
      ));
    }
    let stream_id = self.entries[index].stream_id;
    let entry = self
      .directory
      .entry_mut(stream_id)
      .expect("validated directory entry remains present");
    if created {
      entry.creation_time = time;
      self.entries[index].created = time;
    } else {
      entry.modified_time = time;
      self.entries[index].modified = time;
    }
    self.persist_directory()
  }

  /// Moves an entry tree to a new storage and/or gives it a new name.
  pub fn move_entry(&mut self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<()> {
    let source_index = self.required_entry_index(from.as_ref())?;
    if self.entries[source_index].kind == EntryKind::Root {
      return Err(Error::invalid(0, "cannot move the CFB root storage"));
    }
    let source = self.entries[source_index].path.clone();
    let destination_names = name::path_components(to.as_ref())
      .ok_or_else(|| Error::invalid(0, "invalid destination CFB path"))?;
    let destination_name = destination_names
      .last()
      .ok_or_else(|| Error::invalid(0, "cannot replace the CFB root storage"))?;
    name::validate_entry_name(destination_name)?;
    let destination = path_from_names(&destination_names);
    if self.contains_entry(&destination) {
      return Err(Error::invalid(0, "destination CFB entry already exists"));
    }
    if destination.starts_with(&source) {
      return Err(Error::invalid(0, "cannot move a storage inside itself"));
    }
    let old_parent = source.parent().unwrap_or(Path::new("/")).to_path_buf();
    let new_parent = path_from_names(&destination_names[..destination_names.len() - 1]);
    let parent_index = self.required_entry_index(&new_parent)?;
    if self.entries[parent_index].is_stream() {
      return Err(Error::invalid(0, "destination parent is not a storage"));
    }
    let stream_id = self.entries[source_index].stream_id;
    self
      .directory
      .entry_mut(stream_id)
      .expect("validated directory entry remains present")
      .set_name(destination_name)?;
    for entry in self
      .entries
      .iter_mut()
      .filter(|entry| entry.path == source || entry.path.starts_with(&source))
    {
      let suffix = entry.path.strip_prefix(&source).unwrap().to_path_buf();
      entry.path = destination.join(suffix);
      if entry.stream_id == stream_id {
        entry.name = destination_name.clone();
      }
    }
    self
      .entries
      .sort_by(|left, right| left.path.cmp(&right.path));
    self.rebuild_storage_children(&old_parent)?;
    if new_parent != old_parent {
      self.rebuild_storage_children(&new_parent)?;
    }
    self.persist_directory()
  }

  /// Renames an entry without changing its parent storage.
  pub fn rename_entry(&mut self, path: impl AsRef<Path>, new_name: &str) -> Result<()> {
    name::validate_entry_name(new_name)?;
    let index = self.required_entry_index(path.as_ref())?;
    let parent = self.entries[index].path.parent().unwrap_or(Path::new("/"));
    self.move_entry(path, parent.join(new_name))
  }

  /// Copies a stream or an entire storage tree using a bounded transfer
  /// buffer. The destination parent storage must already exist.
  pub fn copy_entry(&mut self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<()> {
    let source = self
      .entry(from.as_ref())
      .ok_or_else(|| Error::invalid(0, "source CFB entry does not exist"))?
      .clone();
    let destination_names = name::path_components(to.as_ref())
      .ok_or_else(|| Error::invalid(0, "invalid destination CFB path"))?;
    let destination = path_from_names(&destination_names);
    if self.contains_entry(&destination) {
      return Err(Error::invalid(0, "destination CFB entry already exists"));
    }
    if source.kind != EntryKind::Stream && destination.starts_with(&source.path) {
      return Err(Error::invalid(0, "cannot copy a storage inside itself"));
    }
    if source.kind == EntryKind::Stream {
      return self.copy_stream_path(&source, &destination);
    }

    self.create_storage(&destination)?;
    self.copy_entry_metadata(&source, &destination)?;
    let mut descendants: Vec<_> = self
      .entries
      .iter()
      .filter(|entry| entry.path != source.path && entry.path.starts_with(&source.path))
      .cloned()
      .collect();
    descendants.sort_by_key(|entry| entry.path.components().count());
    for entry in descendants {
      let suffix = entry.path.strip_prefix(&source.path).unwrap();
      let target = destination.join(suffix);
      if entry.kind == EntryKind::Stream {
        self.copy_stream_path(&entry, &target)?;
      } else {
        self.create_storage(&target)?;
        self.copy_entry_metadata(&entry, &target)?;
      }
    }
    Ok(())
  }

  fn copy_stream_path(&mut self, source: &EntryInfo, destination: &Path) -> Result<()> {
    {
      let mut stream = self.create_new_stream(destination)?;
      stream.set_len(source.stream_len).map_err(Error::Io)?;
    }
    let source_chain = {
      let stream = self.open_stream_mut(&source.path)?;
      stream.chain.clone()
    };
    let destination_chain = {
      let stream = self.open_stream_mut(destination)?;
      stream.chain.clone()
    };
    let buffer_len = self
      .stream_buffer_size
      .min(self.limits.max_allocation)
      .max(self.source.sector_len());
    let mut buffer = vec![0; buffer_len];
    let mut offset = 0u64;
    while offset < source.stream_len {
      let count = usize::try_from((source.stream_len - offset).min(buffer.len() as u64))
        .map_err(|_| Error::Limit("stream copy chunk does not fit usize".into()))?;
      let read = read_chain_at(
        &mut self.source,
        &source_chain,
        source.stream_len,
        offset,
        &mut buffer[..count],
      )?;
      if read != count {
        return Err(Error::invalid(
          offset,
          "source stream chain ended during bounded copy",
        ));
      }
      write_chain_at(
        &mut self.source,
        &destination_chain,
        source.stream_len,
        offset,
        &buffer[..count],
      )?;
      offset += count as u64;
    }
    self.copy_entry_metadata(source, destination)
  }

  fn copy_entry_metadata(&mut self, source: &EntryInfo, destination: &Path) -> Result<()> {
    let index = self.required_entry_index(destination)?;
    let stream_id = self.entries[index].stream_id;
    let raw = self
      .directory
      .entry_mut(stream_id)
      .expect("validated destination directory entry remains present");
    raw.state_bits = source.state_bits;
    if source.kind != EntryKind::Stream {
      raw.clsid = source.clsid;
      raw.creation_time = source.created;
      raw.modified_time = source.modified;
    }
    self.entries[index].state_bits = source.state_bits;
    self.entries[index].clsid = source.clsid;
    self.entries[index].created = source.created;
    self.entries[index].modified = source.modified;
    self.persist_directory()
  }

  /// Flushes all direct stream and directory edits to the backing object.
  pub fn flush(&mut self) -> Result<()> {
    self.source.flush()
  }

  fn rebuild_storage_children(&mut self, parent_path: &Path) -> Result<()> {
    let parent = self.required_entry_index(parent_path)?;
    let parent_id = self.entries[parent].stream_id;
    let mut children: Vec<_> = self
      .entries
      .iter()
      .filter(|entry| entry.path.parent() == Some(parent_path))
      .map(|entry| entry.stream_id)
      .collect();
    self.directory.rebuild_children(parent_id, &mut children)
  }

  fn allocate_directory_entry(&mut self) -> Result<u32> {
    if let Some(index) = self
      .directory
      .entries()
      .iter()
      .position(|entry| entry.object_type == DirectoryObjectType::Unallocated)
    {
      return u32::try_from(index)
        .map_err(|_| Error::Limit("directory stream ID does not fit u32".into()));
    }
    self.grow_directory()?;
    let index = self
      .directory
      .entries()
      .iter()
      .position(|entry| entry.object_type == DirectoryObjectType::Unallocated)
      .expect("new directory sector contains unallocated entries");
    u32::try_from(index).map_err(|_| Error::Limit("directory stream ID does not fit u32".into()))
  }

  fn grow_directory(&mut self) -> Result<()> {
    let entries_per_sector = self.source.sector_len() / DIRECTORY_ENTRY_LEN;
    let new_entry_count = self
      .directory
      .entries()
      .len()
      .checked_add(entries_per_sector)
      .ok_or_else(|| Error::Limit("directory entry count overflow".into()))?;
    if new_entry_count > self.limits.max_entries {
      return Err(Error::Limit(format!(
        "directory entry count {new_entry_count} exceeds {}",
        self.limits.max_entries
      )));
    }
    let sector = RegularAllocator {
      source: &mut self.source,
      header: &mut self.header,
      difat: &mut self.difat,
      fat: &mut self.fat,
    }
    .allocate()?;
    let previous = *self
      .directory
      .sectors()
      .last()
      .ok_or_else(|| Error::invalid(0, "CFB directory has no sector chain"))?;
    self.set_directory_fat_entry(previous, FatEntry::Sector(sector))?;
    self.set_directory_fat_entry(sector, FatEntry::EndOfChain)?;
    self.directory.push_sector(sector, entries_per_sector);
    if self.version == Version::V4 {
      self.header.number_of_directory_sectors = self
        .header
        .number_of_directory_sectors
        .checked_add(1)
        .ok_or_else(|| Error::Limit("directory sector count overflow".into()))?;
      self
        .source
        .write_header_at(40, &self.header.number_of_directory_sectors.to_le_bytes())?;
      self
        .directory
        .set_declared_sector_count(self.header.number_of_directory_sectors);
    }
    self.persist_directory()
  }

  fn set_directory_fat_entry(&mut self, id: SectorId, entry: FatEntry) -> Result<()> {
    RegularAllocator {
      source: &mut self.source,
      header: &mut self.header,
      difat: &mut self.difat,
      fat: &mut self.fat,
    }
    .set_entry(id, entry)
  }

  fn persist_directory(&mut self) -> Result<()> {
    let entries_per_sector = self.source.sector_len() / DIRECTORY_ENTRY_LEN;
    for (index, entry) in self.directory.entries().iter().enumerate() {
      let sector = *self
        .directory
        .sectors()
        .get(index / entries_per_sector)
        .ok_or_else(|| Error::invalid(0, "directory entry has no physical sector"))?;
      let mut bytes = [0; DIRECTORY_ENTRY_LEN];
      let cursor = Cursor::new(bytes.as_mut_slice());
      let mut writer = Writer::new(cursor);
      entry.write_to(&mut writer)?;
      self.source.write_sector_at(
        sector,
        index % entries_per_sector * DIRECTORY_ENTRY_LEN,
        &bytes,
      )?;
    }
    Ok(())
  }
}

fn path_from_names(names: &[String]) -> PathBuf {
  let mut path = PathBuf::from("/");
  for name in names {
    path.push(name);
  }
  path
}

impl CompoundFileReader<std::fs::File> {
  /// Creates a new version-3 compound file and opens it for direct editing.
  pub fn create(path: impl AsRef<Path>) -> Result<Self> {
    Self::create_with_version(path, Version::V3)
  }

  /// Creates a new compound file and opens it for direct editing.
  pub fn create_with_version(path: impl AsRef<Path>, version: Version) -> Result<Self> {
    let path = path.as_ref();
    let mut file = std::fs::OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .truncate(true)
      .open(path)?;
    CompoundFile::new(version)?.write_to(&mut file)?;
    file.seek(SeekFrom::Start(0))?;
    Self::from_reader(file)
  }

  pub fn open(path: impl AsRef<Path>) -> Result<Self> {
    Self::from_reader(std::fs::File::open(path)?)
  }

  pub fn open_strict(path: impl AsRef<Path>) -> Result<Self> {
    Self::from_reader_strict(std::fs::File::open(path)?)
  }

  /// Opens a file-backed compound file for independent positional reads and
  /// exclusive in-place stream or directory editing.
  pub fn open_rw(path: impl AsRef<Path>) -> Result<Self> {
    let file = std::fs::OpenOptions::new()
      .read(true)
      .write(true)
      .open(path)?;
    Self::from_reader(file)
  }

  /// Strict variant of [`Self::open_rw`].
  pub fn open_rw_strict(path: impl AsRef<Path>) -> Result<Self> {
    let file = std::fs::OpenOptions::new()
      .read(true)
      .write(true)
      .open(path)?;
    Self::from_reader_strict(file)
  }
}

#[derive(Clone)]
enum StreamChain {
  Regular(Vec<SectorId>),
  Mini {
    mini_chain: Vec<MiniSectorId>,
    root_chain: Arc<[SectorId]>,
    root_len: u64,
  },
}

/// An independent, bounded-buffer read cursor for one CFB stream object.
///
/// Cursors borrow the compound file immutably and use [`CfbReadAt`], so
/// cursors for different streams can coexist and be read in any order.
pub struct CfbReadStream<'a, R> {
  source: ReadAtSectorSource<'a, R>,
  chain: StreamChain,
  len: u64,
  position: u64,
  buffer_start: u64,
  buffer_len: usize,
  buffer: Vec<u8>,
}

impl<R> CfbReadStream<'_, R> {
  pub fn len(&self) -> u64 {
    self.len
  }

  pub fn is_empty(&self) -> bool {
    self.len == 0
  }

  pub fn buffer_capacity(&self) -> usize {
    self.buffer.len()
  }
}

impl<R: CfbReadAt> BufRead for CfbReadStream<'_, R> {
  fn fill_buf(&mut self) -> io::Result<&[u8]> {
    if self.position == self.len {
      return Ok(&[]);
    }
    let buffer_end = self.buffer_start + self.buffer_len as u64;
    if self.buffer_len == 0 || self.position < self.buffer_start || self.position >= buffer_end {
      self.buffer_start = self.position;
      let remaining = self.len - self.position;
      let requested = usize::try_from(remaining)
        .unwrap_or(usize::MAX)
        .min(self.buffer.len());
      self.buffer_len = match &self.chain {
        StreamChain::Regular(chain) => read_regular_at(
          &mut self.source,
          chain,
          self.len,
          self.position,
          &mut self.buffer[..requested],
        ),
        StreamChain::Mini {
          mini_chain,
          root_chain,
          root_len,
        } => read_mini_at(
          &mut self.source,
          mini_chain,
          root_chain,
          *root_len,
          self.len,
          self.position,
          &mut self.buffer[..requested],
        ),
      }
      .map_err(as_io_error)?;
    }
    let offset = usize::try_from(self.position - self.buffer_start)
      .map_err(|_| io::Error::other("CFB stream buffer offset does not fit usize"))?;
    Ok(&self.buffer[offset..self.buffer_len])
  }

  fn consume(&mut self, amount: usize) {
    debug_assert!(self.position + amount as u64 <= self.buffer_start + self.buffer_len as u64);
    self.position += amount as u64;
  }
}

impl<R: CfbReadAt> Read for CfbReadStream<'_, R> {
  fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
    let available = self.fill_buf()?;
    let count = available.len().min(output.len());
    output[..count].copy_from_slice(&available[..count]);
    self.consume(count);
    Ok(count)
  }
}

impl<R> Seek for CfbReadStream<'_, R> {
  fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
    self.position = checked_stream_seek(self.position, self.len, position)?;
    Ok(self.position)
  }
}

struct RegularAllocator<'a, R> {
  source: &'a mut SeekSectorSource<R>,
  header: &'a mut Header,
  difat: &'a mut Difat,
  fat: &'a mut Fat,
}

impl<R: Read + Write + Seek> RegularAllocator<'_, R> {
  fn allocate(&mut self) -> Result<SectorId> {
    let reusable = (0..self.source.sector_count()).find_map(|raw| {
      let id = SectorId::new(raw as u32).ok()?;
      (self.fat.entry(id) == Some(FatEntry::Free)
        && self.source.valid_len(id) == self.source.sector_len())
      .then_some(id)
    });
    let id = if let Some(id) = reusable {
      self.source.zero_sector(id)?;
      id
    } else {
      let needs_fat_sector = self.source.sector_count() >= self.fat.allocation_capacity();
      let metadata_sectors = if needs_fat_sector {
        self.fat_growth_sector_count()
      } else {
        0
      };
      self
        .source
        .ensure_append_sector_count(metadata_sectors + 1)?;
      if needs_fat_sector {
        self.add_fat_sector()?;
      }
      self.source.append_zero_sector()?
    };
    self.set_entry(id, FatEntry::EndOfChain)?;
    Ok(id)
  }

  fn fat_growth_sector_count(&self) -> usize {
    let difat_index = self.difat.fat_sectors().len();
    if difat_index < self.header.difat.len() {
      return 1;
    }
    let difat_payload_len = self.source.sector_len() / 4 - 1;
    let external_index = difat_index - self.header.difat.len();
    let difat_slot = external_index / difat_payload_len;
    if difat_slot < self.difat.difat_sectors().len() {
      1
    } else {
      2
    }
  }

  fn set_entry(&mut self, id: SectorId, entry: FatEntry) -> Result<()> {
    let entries_per_sector = self.source.sector_len() / 4;
    let index = id.get() as usize;
    let fat_sector = *self
      .difat
      .fat_sectors()
      .get(index / entries_per_sector)
      .ok_or_else(|| Error::invalid(0, "FAT entry has no physical FAT sector"))?;
    self.source.write_sector_at(
      fat_sector,
      index % entries_per_sector * 4,
      &entry.raw().to_le_bytes(),
    )?;
    self.fat.set_entry(id, entry)
  }

  fn add_fat_sector(&mut self) -> Result<()> {
    let difat_index = self.difat.fat_sectors().len();
    if difat_index < self.header.difat.len() {
      return self.add_header_difat_fat_sector(difat_index);
    }
    let entries_per_sector = self.source.sector_len() / 4;
    let difat_payload_len = entries_per_sector - 1;
    let external_index = difat_index - self.header.difat.len();
    let difat_slot = external_index / difat_payload_len;
    if difat_slot < self.difat.difat_sectors().len() {
      let sector = self.source.append_zero_sector()?;
      if sector.get() as usize != self.fat.allocation_capacity() {
        return Err(Error::invalid(
          0,
          "new FAT sector is not aligned with FAT coverage",
        ));
      }
      let mut entries = vec![0xff; self.source.sector_len()];
      entries[..4].copy_from_slice(&super::allocation::FAT_SECTOR.to_le_bytes());
      self.source.write_sector_at(sector, 0, &entries)?;
      let difat_sector = self.difat.difat_sectors()[difat_slot];
      self.source.write_sector_at(
        difat_sector,
        external_index % difat_payload_len * 4,
        &sector.get().to_le_bytes(),
      )?;
      self.fat.push_fat_sector(sector, entries_per_sector);
      self.difat.push_external_fat_sector(sector);
      return self.increment_fat_sector_count();
    }

    let difat_sector = self.source.append_zero_sector()?;
    let fat_sector = self.source.append_zero_sector()?;
    if difat_sector.get() as usize != self.fat.allocation_capacity() {
      return Err(Error::invalid(
        0,
        "new DIFAT sector is not aligned with FAT coverage",
      ));
    }
    let mut fat_entries = vec![0xff; self.source.sector_len()];
    fat_entries[..4].copy_from_slice(&super::allocation::DIFAT_SECTOR.to_le_bytes());
    fat_entries[4..8].copy_from_slice(&super::allocation::FAT_SECTOR.to_le_bytes());
    self.source.write_sector_at(fat_sector, 0, &fat_entries)?;

    let mut difat_entries = vec![0xff; self.source.sector_len()];
    difat_entries[..4].copy_from_slice(&fat_sector.get().to_le_bytes());
    let next_offset = self.source.sector_len() - 4;
    difat_entries[next_offset..].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
    self
      .source
      .write_sector_at(difat_sector, 0, &difat_entries)?;
    if let Some(&previous) = self.difat.difat_sectors().last() {
      self
        .source
        .write_sector_at(previous, next_offset, &difat_sector.get().to_le_bytes())?;
    } else {
      self.header.first_difat_sector = difat_sector.get();
      self
        .source
        .write_header_at(68, &difat_sector.get().to_le_bytes())?;
    }
    self.header.number_of_difat_sectors = self
      .header
      .number_of_difat_sectors
      .checked_add(1)
      .ok_or_else(|| Error::Limit("DIFAT sector count overflow".into()))?;
    self
      .source
      .write_header_at(72, &self.header.number_of_difat_sectors.to_le_bytes())?;
    self
      .fat
      .push_difat_and_fat_sectors(difat_sector, fat_sector, entries_per_sector);
    self.difat.push_difat_sector(difat_sector);
    self.difat.push_external_fat_sector(fat_sector);
    self.increment_fat_sector_count()
  }

  fn add_header_difat_fat_sector(&mut self, difat_index: usize) -> Result<()> {
    let sector = self.source.append_zero_sector()?;
    if sector.get() as usize != self.fat.allocation_capacity() {
      return Err(Error::invalid(
        0,
        "new FAT sector is not aligned with FAT coverage",
      ));
    }
    let mut entries = vec![0xff; self.source.sector_len()];
    entries[..4].copy_from_slice(&super::allocation::FAT_SECTOR.to_le_bytes());
    self.source.write_sector_at(sector, 0, &entries)?;
    self
      .fat
      .push_fat_sector(sector, self.source.sector_len() / 4);
    self.difat.push_header_fat_sector(sector)?;
    self.header.difat[difat_index] = sector.get();
    self
      .source
      .write_header_at(76 + difat_index * 4, &sector.get().to_le_bytes())?;
    self.increment_fat_sector_count()
  }

  fn increment_fat_sector_count(&mut self) -> Result<()> {
    self.header.number_of_fat_sectors = self
      .header
      .number_of_fat_sectors
      .checked_add(1)
      .ok_or_else(|| Error::Limit("FAT sector count overflow".into()))?;
    self
      .source
      .write_header_at(44, &self.header.number_of_fat_sectors.to_le_bytes())
  }
}

/// An exclusively borrowed, bounded-buffer writable stream object.
///
/// It implements [`Write`] when the backing object is writable. Writes within
/// the current stream length go directly to the backing object. [`Self::set_len`]
/// grows or shrinks the physical sector chain and transparently migrates streams
/// across the regular/mini-stream cutoff.
pub struct CfbStreamMut<'a, R> {
  source: &'a mut SeekSectorSource<R>,
  header: &'a mut Header,
  fat: &'a mut Fat,
  mini_fat: &'a mut MiniFat,
  difat: &'a mut Difat,
  directory: &'a mut Directory,
  entries: &'a mut [EntryInfo],
  entry_index: usize,
  root_mini_chain: &'a mut Arc<[SectorId]>,
  version: Version,
  stream_id: u32,
  chain: StreamChain,
  len: u64,
  position: u64,
  buffer_start: u64,
  buffer_len: usize,
  buffer: Vec<u8>,
  max_buffer_capacity: usize,
  max_stream_size: u64,
}

impl<R> CfbStreamMut<'_, R> {
  pub fn len(&self) -> u64 {
    self.len
  }

  pub fn is_empty(&self) -> bool {
    self.len == 0
  }

  pub fn buffer_capacity(&self) -> usize {
    self.buffer.len()
  }
}

impl<R: Read + Seek> BufRead for CfbStreamMut<'_, R> {
  fn fill_buf(&mut self) -> io::Result<&[u8]> {
    if self.position == self.len {
      return Ok(&[]);
    }
    let buffer_end = self.buffer_start + self.buffer_len as u64;
    if self.buffer_len == 0 || self.position < self.buffer_start || self.position >= buffer_end {
      self.buffer_start = self.position;
      let remaining = self.len - self.position;
      let requested = usize::try_from(remaining)
        .unwrap_or(usize::MAX)
        .min(self.buffer.len());
      self.buffer_len = match &self.chain {
        StreamChain::Regular(chain) => read_regular_at(
          self.source,
          chain,
          self.len,
          self.position,
          &mut self.buffer[..requested],
        ),
        StreamChain::Mini {
          mini_chain,
          root_chain,
          root_len,
        } => read_mini_at(
          self.source,
          mini_chain,
          root_chain,
          *root_len,
          self.len,
          self.position,
          &mut self.buffer[..requested],
        ),
      }
      .map_err(as_io_error)?;
    }
    let offset = usize::try_from(self.position - self.buffer_start)
      .map_err(|_| io::Error::other("CFB stream buffer offset does not fit usize"))?;
    Ok(&self.buffer[offset..self.buffer_len])
  }

  fn consume(&mut self, amount: usize) {
    debug_assert!(self.position + amount as u64 <= self.buffer_start + self.buffer_len as u64);
    self.position += amount as u64;
  }
}

impl<R: Read + Seek> Read for CfbStreamMut<'_, R> {
  fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
    let available = self.fill_buf()?;
    let count = available.len().min(output.len());
    output[..count].copy_from_slice(&available[..count]);
    self.consume(count);
    Ok(count)
  }
}

impl<R: Read + Seek> Seek for CfbStreamMut<'_, R> {
  fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
    self.position = checked_stream_seek(self.position, self.len, position)?;
    Ok(self.position)
  }
}

impl<R: Read + Write + Seek> Write for CfbStreamMut<'_, R> {
  fn write(&mut self, input: &[u8]) -> io::Result<usize> {
    if input.is_empty() {
      return Ok(0);
    }
    let end = self
      .position
      .checked_add(input.len() as u64)
      .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "stream end overflow"))?;
    if end > self.len {
      self.set_len(end)?;
    }
    let count = input.len();
    match &self.chain {
      StreamChain::Regular(chain) => {
        write_regular_at(self.source, chain, self.len, self.position, &input[..count])
      }
      StreamChain::Mini {
        mini_chain,
        root_chain,
        root_len,
      } => write_mini_at(
        self.source,
        mini_chain,
        root_chain,
        *root_len,
        self.len,
        self.position,
        &input[..count],
      ),
    }
    .map_err(as_io_error)?;
    self.position += count as u64;
    self.buffer_len = 0;
    Ok(count)
  }

  fn flush(&mut self) -> io::Result<()> {
    self.source.flush().map_err(as_io_error)
  }
}

impl<R: Read + Write + Seek> CfbStreamMut<'_, R> {
  /// Truncates or extends a stream and updates its allocation chain and
  /// directory entry in the backing compound file.
  pub fn set_len(&mut self, new_len: u64) -> io::Result<()> {
    if new_len == self.len {
      return Ok(());
    }
    if new_len > self.max_stream_size {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
          "stream length {new_len} exceeds configured limit {}",
          self.max_stream_size
        ),
      ));
    }
    if self.version == Version::V3 && new_len > 0x8000_0000 {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "CFB v3 stream length exceeds 2 GiB",
      ));
    }
    let old_is_mini = self.len < MINI_STREAM_CUTOFF as u64;
    let new_is_mini = new_len < MINI_STREAM_CUTOFF as u64;
    match (old_is_mini, new_is_mini) {
      (true, true) => self.resize_mini(new_len)?,
      (false, false) => self.resize_regular(new_len)?,
      (true, false) => self.migrate_mini_to_regular(new_len)?,
      (false, true) => self.migrate_regular_to_mini(new_len)?,
    }
    self
      .directory
      .entry_mut(self.stream_id)
      .expect("validated directory entry remains present")
      .stream_size = new_len;
    self.entries[self.entry_index].stream_len = new_len;
    self
      .write_directory_stream_location(self.stream_id)
      .map_err(as_io_error)?;
    self.len = new_len;
    self.position = self.position.min(new_len);
    self.buffer_len = 0;
    let desired_buffer_capacity = usize::try_from(new_len)
      .unwrap_or(self.max_buffer_capacity)
      .min(self.max_buffer_capacity)
      .max(1);
    if desired_buffer_capacity > self.buffer.len() {
      self.buffer.resize(desired_buffer_capacity, 0);
    }
    Ok(())
  }

  fn migrate_mini_to_regular(&mut self, new_len: u64) -> io::Result<()> {
    let StreamChain::Mini {
      mini_chain,
      root_chain,
      root_len,
    } = &self.chain
    else {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "expected mini chain",
      ));
    };
    let old_mini_chain = mini_chain.clone();
    let old_root_chain = root_chain.clone();
    let old_root_len = *root_len;
    let mut preserved = vec![0; self.len as usize];
    read_mini_at(
      self.source,
      &old_mini_chain,
      &old_root_chain,
      old_root_len,
      self.len,
      0,
      &mut preserved,
    )
    .map_err(as_io_error)?;

    let desired = usize::try_from(new_len.div_ceil(self.source.sector_len() as u64))
      .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "stream chain too large"))?;
    if desired > self.available_regular_allocations() {
      return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "stream migration requires adding another FAT sector",
      ));
    }
    let mut regular_chain = Vec::with_capacity(desired);
    for _ in 0..desired {
      let id = self.allocate_regular_sector().map_err(as_io_error)?;
      if let Some(&previous) = regular_chain.last() {
        self
          .set_fat_entry(previous, FatEntry::Sector(id))
          .map_err(as_io_error)?;
      }
      regular_chain.push(id);
    }
    let zeros = vec![0; self.source.sector_len().min(1024 * 1024)];
    let mut offset = 0u64;
    while offset < new_len {
      let count = usize::try_from((new_len - offset).min(zeros.len() as u64)).unwrap();
      write_regular_at(
        self.source,
        &regular_chain,
        new_len,
        offset,
        &zeros[..count],
      )
      .map_err(as_io_error)?;
      offset += count as u64;
    }
    write_regular_at(self.source, &regular_chain, new_len, 0, &preserved).map_err(as_io_error)?;
    self
      .directory
      .entry_mut(self.stream_id)
      .expect("validated directory entry remains present")
      .start_sector = regular_chain[0].get();
    for id in old_mini_chain {
      self
        .set_mini_fat_entry(id, MiniFatEntry::Free)
        .map_err(as_io_error)?;
    }
    self.chain = StreamChain::Regular(regular_chain);
    Ok(())
  }

  fn migrate_regular_to_mini(&mut self, new_len: u64) -> io::Result<()> {
    let StreamChain::Regular(chain) = &self.chain else {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "expected regular chain",
      ));
    };
    let old_chain = chain.clone();
    let mut preserved = vec![0; new_len as usize];
    read_regular_at(self.source, &old_chain, self.len, 0, &mut preserved).map_err(as_io_error)?;

    let mut root_chain = self.root_mini_chain.to_vec();
    let mut root_len = self.directory.root().effective_stream_size(self.version);
    let desired = usize::try_from(new_len.div_ceil(MINI_SECTOR_LEN as u64))
      .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "mini chain too large"))?;
    let mut mini_chain = Vec::with_capacity(desired);
    for _ in 0..desired {
      let id = self
        .allocate_mini_sector(&mut root_chain, &mut root_len)
        .map_err(as_io_error)?;
      if let Some(&previous) = mini_chain.last() {
        self
          .set_mini_fat_entry(previous, MiniFatEntry::MiniSector(id))
          .map_err(as_io_error)?;
      }
      mini_chain.push(id);
    }
    if !preserved.is_empty() {
      write_mini_at(
        self.source,
        &mini_chain,
        &root_chain,
        root_len,
        new_len,
        0,
        &preserved,
      )
      .map_err(as_io_error)?;
    }
    let start = mini_chain
      .first()
      .map_or(END_OF_CHAIN, |sector| sector.get());
    self
      .directory
      .entry_mut(self.stream_id)
      .expect("validated directory entry remains present")
      .start_sector = start;
    for id in old_chain {
      self
        .set_fat_entry(id, FatEntry::Free)
        .map_err(as_io_error)?;
    }
    *self.root_mini_chain = Arc::from(root_chain.clone());
    self.chain = StreamChain::Mini {
      mini_chain,
      root_chain: Arc::from(root_chain),
      root_len,
    };
    Ok(())
  }

  fn resize_regular(&mut self, new_len: u64) -> io::Result<()> {
    let StreamChain::Regular(current_chain) = &self.chain else {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "regular stream has a mini-sector chain",
      ));
    };
    let mut chain = current_chain.clone();
    let sector_len = self.source.sector_len();
    let desired = usize::try_from(new_len.div_ceil(sector_len as u64))
      .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "stream chain too large"))?;
    if desired > chain.len() {
      let needed = desired - chain.len();
      if needed > self.available_regular_allocations() {
        return Err(io::Error::new(
          io::ErrorKind::Unsupported,
          "stream growth requires adding another FAT sector",
        ));
      }
      for _ in 0..needed {
        let id = self.allocate_regular_sector().map_err(as_io_error)?;
        if let Some(&previous) = chain.last() {
          self
            .set_fat_entry(previous, FatEntry::Sector(id))
            .map_err(as_io_error)?;
        }
        chain.push(id);
      }
    } else if desired < chain.len() {
      let released = chain.split_off(desired);
      let last = *chain
        .last()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty regular chain"))?;
      self
        .set_fat_entry(last, FatEntry::EndOfChain)
        .map_err(as_io_error)?;
      for id in released {
        self
          .set_fat_entry(id, FatEntry::Free)
          .map_err(as_io_error)?;
      }
    }
    if new_len < self.len && !new_len.is_multiple_of(sector_len as u64) {
      let tail_start = usize::try_from(new_len % sector_len as u64)
        .map_err(|_| io::Error::other("regular stream tail offset does not fit usize"))?;
      let final_sector = *chain
        .last()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty regular chain"))?;
      let tail_len = self
        .source
        .valid_len(final_sector)
        .saturating_sub(tail_start);
      self
        .source
        .write_sector_at(final_sector, tail_start, &vec![0; tail_len])
        .map_err(as_io_error)?;
    }
    if new_len > self.len {
      let mut offset = self.len;
      let mut remaining = new_len - self.len;
      let zeros = vec![0; self.buffer.len().max(sector_len).min(1024 * 1024)];
      while remaining > 0 {
        let count = usize::try_from(remaining.min(zeros.len() as u64)).unwrap();
        write_regular_at(self.source, &chain, new_len, offset, &zeros[..count])
          .map_err(as_io_error)?;
        offset += count as u64;
        remaining -= count as u64;
      }
    }
    self.chain = StreamChain::Regular(chain);
    Ok(())
  }

  fn resize_mini(&mut self, new_len: u64) -> io::Result<()> {
    let StreamChain::Mini {
      mini_chain: current_chain,
      root_chain: current_root_chain,
      root_len: current_root_len,
    } = &self.chain
    else {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "mini stream has a regular-sector chain",
      ));
    };
    let mut mini_chain = current_chain.clone();
    let mut root_chain = current_root_chain.to_vec();
    let mut root_len = *current_root_len;
    let desired = usize::try_from(new_len.div_ceil(MINI_SECTOR_LEN as u64))
      .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "mini chain too large"))?;
    if desired > mini_chain.len() {
      for _ in mini_chain.len()..desired {
        let id = self
          .allocate_mini_sector(&mut root_chain, &mut root_len)
          .map_err(as_io_error)?;
        if let Some(&previous) = mini_chain.last() {
          self
            .set_mini_fat_entry(previous, MiniFatEntry::MiniSector(id))
            .map_err(as_io_error)?;
        }
        mini_chain.push(id);
      }
    } else if desired < mini_chain.len() {
      let released = mini_chain.split_off(desired);
      if let Some(&last) = mini_chain.last() {
        self
          .set_mini_fat_entry(last, MiniFatEntry::EndOfChain)
          .map_err(as_io_error)?;
      }
      for id in released {
        self
          .set_mini_fat_entry(id, MiniFatEntry::Free)
          .map_err(as_io_error)?;
      }
    }
    if new_len != 0 && new_len < self.len && !new_len.is_multiple_of(MINI_SECTOR_LEN as u64) {
      let final_mini_sector = *mini_chain
        .last()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty mini chain"))?;
      let within = usize::try_from(new_len % MINI_SECTOR_LEN as u64)
        .map_err(|_| io::Error::other("mini stream tail offset does not fit usize"))?;
      let root_offset = u64::from(final_mini_sector.get())
        .checked_mul(MINI_SECTOR_LEN as u64)
        .and_then(|offset| offset.checked_add(within as u64))
        .ok_or_else(|| io::Error::other("root mini stream tail offset overflow"))?;
      write_regular_at(
        self.source,
        &root_chain,
        root_len,
        root_offset,
        &[0; MINI_SECTOR_LEN][..MINI_SECTOR_LEN - within],
      )
      .map_err(as_io_error)?;
    }
    if new_len > self.len {
      let mut offset = self.len;
      let mut remaining = new_len - self.len;
      let zeros = [0; MINI_SECTOR_LEN];
      while remaining > 0 {
        let count = usize::try_from(remaining.min(MINI_SECTOR_LEN as u64)).unwrap();
        write_mini_at(
          self.source,
          &mini_chain,
          &root_chain,
          root_len,
          new_len,
          offset,
          &zeros[..count],
        )
        .map_err(as_io_error)?;
        offset += count as u64;
        remaining -= count as u64;
      }
    }
    let start = mini_chain
      .first()
      .map_or(END_OF_CHAIN, |sector| sector.get());
    self
      .directory
      .entry_mut(self.stream_id)
      .expect("validated directory entry remains present")
      .start_sector = start;
    *self.root_mini_chain = Arc::from(root_chain.clone());
    self.chain = StreamChain::Mini {
      mini_chain,
      root_chain: Arc::from(root_chain),
      root_len,
    };
    Ok(())
  }

  fn allocate_mini_sector(
    &mut self,
    root_chain: &mut Vec<SectorId>,
    root_len: &mut u64,
  ) -> Result<MiniSectorId> {
    let root_count = usize::try_from(*root_len / MINI_SECTOR_LEN as u64)
      .map_err(|_| Error::Limit("root mini-sector count does not fit usize".into()))?;
    let reusable = (0..root_count).find_map(|raw| {
      let id = MiniSectorId::new(raw as u32).ok()?;
      (self.mini_fat.entry(id) == Some(MiniFatEntry::Free)).then_some(id)
    });
    let id = if let Some(id) = reusable {
      id
    } else {
      if root_count >= self.mini_fat.allocation_capacity() {
        self.add_mini_fat_sector()?;
      }
      let id = MiniSectorId::new(
        u32::try_from(root_count)
          .map_err(|_| Error::Limit("allocated mini-sector ID does not fit u32".into()))?,
      )?;
      let new_root_len = root_len
        .checked_add(MINI_SECTOR_LEN as u64)
        .ok_or_else(|| Error::Limit("root mini stream length overflow".into()))?;
      let needed_regular = usize::try_from(new_root_len.div_ceil(self.source.sector_len() as u64))
        .map_err(|_| Error::Limit("root mini stream chain is too large".into()))?;
      if needed_regular > root_chain.len() {
        let sector = self.allocate_regular_sector()?;
        if let Some(&previous) = root_chain.last() {
          self.set_fat_entry(previous, FatEntry::Sector(sector))?;
        }
        root_chain.push(sector);
      }
      *root_len = new_root_len;
      let root = self
        .directory
        .entry_mut(0)
        .ok_or_else(|| Error::invalid(0, "root directory entry is missing"))?;
      root.start_sector = root_chain
        .first()
        .map_or(END_OF_CHAIN, |sector| sector.get());
      root.stream_size = *root_len;
      if let Some(root_info) = self
        .entries
        .iter_mut()
        .find(|entry| entry.kind == EntryKind::Root)
      {
        root_info.stream_len = *root_len;
      }
      self.write_directory_stream_location(0)?;
      id
    };
    let root_offset = u64::from(id.get()) * MINI_SECTOR_LEN as u64;
    write_regular_at(
      self.source,
      root_chain,
      *root_len,
      root_offset,
      &[0; MINI_SECTOR_LEN],
    )?;
    self.set_mini_fat_entry(id, MiniFatEntry::EndOfChain)?;
    Ok(id)
  }

  fn allocate_regular_sector(&mut self) -> Result<SectorId> {
    RegularAllocator {
      source: self.source,
      header: self.header,
      difat: self.difat,
      fat: self.fat,
    }
    .allocate()
  }

  fn available_regular_allocations(&self) -> usize {
    let reusable = (0..self.source.sector_count())
      .filter(|&raw| {
        SectorId::new(raw as u32).ok().is_some_and(|id| {
          self.fat.entry(id) == Some(FatEntry::Free)
            && self.source.valid_len(id) == self.source.sector_len()
        })
      })
      .count();
    let appendable = if self.source.has_partial_sector() {
      0
    } else {
      (super::sector::MAX_REGULAR_SECTOR as usize).saturating_sub(self.source.sector_count())
    };
    reusable + appendable
  }

  fn set_fat_entry(&mut self, id: SectorId, entry: FatEntry) -> Result<()> {
    RegularAllocator {
      source: self.source,
      header: self.header,
      difat: self.difat,
      fat: self.fat,
    }
    .set_entry(id, entry)
  }

  fn set_mini_fat_entry(&mut self, id: MiniSectorId, entry: MiniFatEntry) -> Result<()> {
    let entries_per_sector = self.source.sector_len() / 4;
    let index = id.get() as usize;
    let mini_fat_sector = *self
      .mini_fat
      .sectors()
      .get(index / entries_per_sector)
      .ok_or_else(|| Error::invalid(0, "MiniFAT entry has no physical sector"))?;
    self.source.write_sector_at(
      mini_fat_sector,
      index % entries_per_sector * 4,
      &entry.raw().to_le_bytes(),
    )?;
    self.mini_fat.set_entry(id, entry)
  }

  fn add_mini_fat_sector(&mut self) -> Result<()> {
    let sector = self.allocate_regular_sector()?;
    let free_entries = vec![0xff; self.source.sector_len()];
    self.source.write_sector_at(sector, 0, &free_entries)?;
    if let Some(&previous) = self.mini_fat.sectors().last() {
      self.set_fat_entry(previous, FatEntry::Sector(sector))?;
    } else {
      self.header.first_mini_fat_sector = sector.get();
      self
        .source
        .write_header_at(60, &sector.get().to_le_bytes())?;
    }
    let new_count = self
      .mini_fat
      .sectors()
      .len()
      .checked_add(1)
      .and_then(|count| u32::try_from(count).ok())
      .ok_or_else(|| Error::Limit("MiniFAT sector count does not fit u32".into()))?;
    self.header.number_of_mini_fat_sectors = new_count;
    self.source.write_header_at(64, &new_count.to_le_bytes())?;
    self
      .mini_fat
      .push_sector(sector, self.source.sector_len() / 4);
    Ok(())
  }

  fn write_directory_stream_location(&mut self, stream_id: u32) -> Result<()> {
    let entries_per_sector = self.source.sector_len() / DIRECTORY_ENTRY_LEN;
    let index = stream_id as usize;
    let sector = *self
      .directory
      .sectors()
      .get(index / entries_per_sector)
      .ok_or_else(|| Error::invalid(0, "directory entry has no physical sector"))?;
    let offset = index % entries_per_sector * DIRECTORY_ENTRY_LEN + 116;
    let entry = self
      .directory
      .entries()
      .get(index)
      .ok_or_else(|| Error::invalid(0, "directory entry is missing"))?;
    let mut bytes = [0; 12];
    bytes[..4].copy_from_slice(&entry.start_sector.to_le_bytes());
    bytes[4..].copy_from_slice(&entry.stream_size.to_le_bytes());
    self.source.write_sector_at(sector, offset, &bytes)
  }
}

fn read_entry_info(
  directory: &Directory,
  version: Version,
  limits: Limits,
) -> Result<Vec<EntryInfo>> {
  let mut entries = Vec::new();
  for (stream_id, path) in directory.paths()? {
    let raw = directory
      .entries()
      .get(stream_id as usize)
      .ok_or_else(|| Error::invalid(0, "directory path references a missing entry"))?;
    let kind = match raw.object_type {
      DirectoryObjectType::Root => EntryKind::Root,
      DirectoryObjectType::Storage => EntryKind::Storage,
      DirectoryObjectType::Stream => EntryKind::Stream,
      DirectoryObjectType::Unallocated => {
        return Err(Error::invalid(
          0,
          "unallocated entry is reachable from root",
        ));
      }
    };
    let stream_len = raw.effective_stream_size(version);
    if stream_len > limits.max_stream_size {
      return Err(Error::Limit(format!(
        "stream length {stream_len} exceeds {}",
        limits.max_stream_size
      )));
    }
    entries.push(EntryInfo {
      stream_id,
      path,
      name: raw.name()?,
      kind,
      clsid: if kind == EntryKind::Stream {
        Guid::ZERO
      } else {
        raw.clsid
      },
      state_bits: raw.state_bits,
      created: if matches!(kind, EntryKind::Root | EntryKind::Stream) {
        FileTime::ZERO
      } else {
        raw.creation_time
      },
      modified: if kind == EntryKind::Stream {
        FileTime::ZERO
      } else {
        raw.modified_time
      },
      stream_len,
    });
  }
  entries.sort_by(|left, right| left.path.cmp(&right.path));
  Ok(entries)
}

fn regular_chain(
  fat: &Fat,
  start: u32,
  len: u64,
  sector_count: usize,
  sector_len: usize,
) -> Result<Vec<SectorId>> {
  if len == 0 {
    return Ok(Vec::new());
  }
  if matches!(start, END_OF_CHAIN | FREE_SECTOR) {
    return Err(Error::invalid(
      0,
      "non-empty stream has no regular sector chain",
    ));
  }
  let chain = fat.chain(start, sector_count)?;
  ensure_chain_capacity(chain.len(), sector_len, len, "regular")?;
  Ok(chain)
}

fn ensure_chain_capacity(count: usize, unit_len: usize, len: u64, kind: &str) -> Result<()> {
  let capacity = u64::try_from(count)
    .ok()
    .and_then(|count| count.checked_mul(unit_len as u64))
    .ok_or_else(|| Error::Limit(format!("{kind} stream chain size overflow")))?;
  if capacity < len {
    return Err(Error::invalid(
      0,
      format!("{kind} stream chain is shorter than stream size"),
    ));
  }
  Ok(())
}

fn ensure_physical_capacity<S: SectorRead + ?Sized>(
  source: &S,
  chain: &[SectorId],
  len: u64,
  kind: &str,
) -> Result<()> {
  if len == 0 {
    return Ok(());
  }
  let sector_len = source.sector_len() as u64;
  let final_index = usize::try_from((len - 1) / sector_len)
    .map_err(|_| Error::Limit(format!("{kind} stream chain index does not fit usize")))?;
  let final_sector = *chain
    .get(final_index)
    .ok_or_else(|| Error::invalid(0, format!("{kind} stream chain is too short")))?;
  let required = usize::try_from((len - 1) % sector_len + 1)
    .map_err(|_| Error::Limit(format!("{kind} stream tail length does not fit usize")))?;
  if source.valid_len(final_sector) < required {
    return Err(Error::invalid(
      0,
      format!("{kind} stream data is truncated at physical EOF"),
    ));
  }
  Ok(())
}

fn read_regular_at<S: SectorRead + ?Sized>(
  source: &mut S,
  chain: &[SectorId],
  stream_len: u64,
  offset: u64,
  output: &mut [u8],
) -> Result<usize> {
  if offset >= stream_len || output.is_empty() {
    return Ok(0);
  }
  let requested = usize::try_from((stream_len - offset).min(output.len() as u64))
    .map_err(|_| Error::Limit("stream read length does not fit usize".into()))?;
  let sector_len = source.sector_len() as u64;
  let mut logical = offset;
  let mut written = 0usize;
  while written < requested {
    let chain_index = usize::try_from(logical / sector_len)
      .map_err(|_| Error::Limit("stream chain index does not fit usize".into()))?;
    let within = usize::try_from(logical % sector_len)
      .map_err(|_| Error::Limit("sector offset does not fit usize".into()))?;
    let sector_id = *chain
      .get(chain_index)
      .ok_or_else(|| Error::invalid(0, "stream chain ended before stream size"))?;
    let valid_len = source.valid_len(sector_id);
    let count = (requested - written).min(source.sector_len() - within);
    if within + count > valid_len {
      return Err(Error::invalid(
        0,
        "stream data is truncated at physical EOF",
      ));
    }
    let bytes = source.sector(sector_id)?;
    output[written..written + count].copy_from_slice(&bytes.as_ref()[within..within + count]);
    logical += count as u64;
    written += count;
  }
  Ok(written)
}

fn read_mini_at<S: SectorRead + ?Sized>(
  source: &mut S,
  mini_chain: &[MiniSectorId],
  root_chain: &[SectorId],
  root_len: u64,
  stream_len: u64,
  offset: u64,
  output: &mut [u8],
) -> Result<usize> {
  if offset >= stream_len || output.is_empty() {
    return Ok(0);
  }
  let requested = usize::try_from((stream_len - offset).min(output.len() as u64))
    .map_err(|_| Error::Limit("mini stream read length does not fit usize".into()))?;
  let mut logical = offset;
  let mut written = 0usize;
  while written < requested {
    let chain_index = usize::try_from(logical / MINI_SECTOR_LEN as u64)
      .map_err(|_| Error::Limit("mini-chain index does not fit usize".into()))?;
    let within = usize::try_from(logical % MINI_SECTOR_LEN as u64)
      .map_err(|_| Error::Limit("mini-sector offset does not fit usize".into()))?;
    let mini_sector = *mini_chain
      .get(chain_index)
      .ok_or_else(|| Error::invalid(0, "mini stream chain ended before stream size"))?;
    let count = (requested - written).min(MINI_SECTOR_LEN - within);
    let root_offset = u64::from(mini_sector.get())
      .checked_mul(MINI_SECTOR_LEN as u64)
      .and_then(|value| value.checked_add(within as u64))
      .ok_or_else(|| Error::Limit("root mini stream offset overflow".into()))?;
    let read = read_regular_at(
      source,
      root_chain,
      root_len,
      root_offset,
      &mut output[written..written + count],
    )?;
    if read != count {
      return Err(Error::invalid(0, "mini-sector is outside the root stream"));
    }
    logical += count as u64;
    written += count;
  }
  Ok(written)
}

fn write_regular_at<S: SectorWrite + ?Sized>(
  source: &mut S,
  chain: &[SectorId],
  stream_len: u64,
  offset: u64,
  input: &[u8],
) -> Result<()> {
  if offset > stream_len || input.len() as u64 > stream_len - offset {
    return Err(Error::invalid(0, "write extends beyond the CFB stream"));
  }
  let sector_len = source.sector_len() as u64;
  let mut logical = offset;
  let mut consumed = 0usize;
  while consumed < input.len() {
    let chain_index = usize::try_from(logical / sector_len)
      .map_err(|_| Error::Limit("stream chain index does not fit usize".into()))?;
    let within = usize::try_from(logical % sector_len)
      .map_err(|_| Error::Limit("sector offset does not fit usize".into()))?;
    let sector_id = *chain
      .get(chain_index)
      .ok_or_else(|| Error::invalid(0, "stream chain ended before stream size"))?;
    let count = (input.len() - consumed).min(source.sector_len() - within);
    source.write_sector_at(sector_id, within, &input[consumed..consumed + count])?;
    logical += count as u64;
    consumed += count;
  }
  Ok(())
}

fn write_mini_at<S: SectorWrite + ?Sized>(
  source: &mut S,
  mini_chain: &[MiniSectorId],
  root_chain: &[SectorId],
  root_len: u64,
  stream_len: u64,
  offset: u64,
  input: &[u8],
) -> Result<()> {
  if offset > stream_len || input.len() as u64 > stream_len - offset {
    return Err(Error::invalid(
      0,
      "write extends beyond the CFB mini stream",
    ));
  }
  let mut logical = offset;
  let mut consumed = 0usize;
  while consumed < input.len() {
    let chain_index = usize::try_from(logical / MINI_SECTOR_LEN as u64)
      .map_err(|_| Error::Limit("mini-chain index does not fit usize".into()))?;
    let within = usize::try_from(logical % MINI_SECTOR_LEN as u64)
      .map_err(|_| Error::Limit("mini-sector offset does not fit usize".into()))?;
    let mini_sector = *mini_chain
      .get(chain_index)
      .ok_or_else(|| Error::invalid(0, "mini stream chain ended before stream size"))?;
    let count = (input.len() - consumed).min(MINI_SECTOR_LEN - within);
    let root_offset = u64::from(mini_sector.get())
      .checked_mul(MINI_SECTOR_LEN as u64)
      .and_then(|value| value.checked_add(within as u64))
      .ok_or_else(|| Error::Limit("root mini stream offset overflow".into()))?;
    write_regular_at(
      source,
      root_chain,
      root_len,
      root_offset,
      &input[consumed..consumed + count],
    )?;
    logical += count as u64;
    consumed += count;
  }
  Ok(())
}

fn read_chain_at<S: SectorRead + ?Sized>(
  source: &mut S,
  chain: &StreamChain,
  len: u64,
  offset: u64,
  output: &mut [u8],
) -> Result<usize> {
  match chain {
    StreamChain::Regular(chain) => read_regular_at(source, chain, len, offset, output),
    StreamChain::Mini {
      mini_chain,
      root_chain,
      root_len,
    } => read_mini_at(
      source, mini_chain, root_chain, *root_len, len, offset, output,
    ),
  }
}

fn write_chain_at<S: SectorWrite + ?Sized>(
  source: &mut S,
  chain: &StreamChain,
  len: u64,
  offset: u64,
  input: &[u8],
) -> Result<()> {
  match chain {
    StreamChain::Regular(chain) => write_regular_at(source, chain, len, offset, input),
    StreamChain::Mini {
      mini_chain,
      root_chain,
      root_len,
    } => write_mini_at(
      source, mini_chain, root_chain, *root_len, len, offset, input,
    ),
  }
}

fn checked_stream_seek(current: u64, len: u64, position: SeekFrom) -> io::Result<u64> {
  let candidate = match position {
    SeekFrom::Start(value) => i128::from(value),
    SeekFrom::End(delta) => i128::from(len) + i128::from(delta),
    SeekFrom::Current(delta) => i128::from(current) + i128::from(delta),
  };
  if candidate < 0 || candidate > i128::from(len) {
    return Err(io::Error::new(
      io::ErrorKind::InvalidInput,
      format!("cannot seek to {candidate}; CFB stream length is {len}"),
    ));
  }
  Ok(candidate as u64)
}

fn as_io_error(error: Error) -> io::Error {
  match error {
    Error::Io(error) => error,
    other => io::Error::new(io::ErrorKind::InvalidData, other),
  }
}

#[cfg(test)]
mod tests {
  use std::{
    cell::Cell,
    io::{Cursor, Read, Seek, SeekFrom, Write},
    rc::Rc,
  };

  use super::*;

  struct CountingReader {
    inner: Cursor<Vec<u8>>,
    bytes_read: Rc<Cell<u64>>,
    largest_read: Rc<Cell<usize>>,
  }

  #[derive(Default)]
  struct FailureControl {
    writes_before_failure: Cell<Option<usize>>,
    fail_flush: Cell<bool>,
  }

  struct FailingCursor {
    inner: Cursor<Vec<u8>>,
    control: Rc<FailureControl>,
  }

  impl FailingCursor {
    fn new(bytes: Vec<u8>) -> (Self, Rc<FailureControl>) {
      let control = Rc::new(FailureControl::default());
      (
        Self {
          inner: Cursor::new(bytes),
          control: control.clone(),
        },
        control,
      )
    }
  }

  impl Read for FailingCursor {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
      self.inner.read(output)
    }
  }

  impl Write for FailingCursor {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
      if let Some(remaining) = self.control.writes_before_failure.get() {
        if remaining == 0 {
          return Err(io::Error::other("injected CFB backing write failure"));
        }
        self.control.writes_before_failure.set(Some(remaining - 1));
      }
      self.inner.write(input)
    }

    fn flush(&mut self) -> io::Result<()> {
      if self.control.fail_flush.get() {
        Err(io::Error::other("injected CFB backing flush failure"))
      } else {
        self.inner.flush()
      }
    }
  }

  impl Seek for FailingCursor {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
      self.inner.seek(position)
    }
  }

  impl CountingReader {
    fn new(bytes: Vec<u8>) -> (Self, Rc<Cell<u64>>, Rc<Cell<usize>>) {
      let bytes_read = Rc::new(Cell::new(0));
      let largest_read = Rc::new(Cell::new(0));
      (
        Self {
          inner: Cursor::new(bytes),
          bytes_read: bytes_read.clone(),
          largest_read: largest_read.clone(),
        },
        bytes_read,
        largest_read,
      )
    }
  }

  impl Read for CountingReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
      let count = self.inner.read(output)?;
      self.bytes_read.set(self.bytes_read.get() + count as u64);
      self.largest_read.set(self.largest_read.get().max(count));
      Ok(count)
    }
  }

  impl Seek for CountingReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
      self.inner.seek(position)
    }
  }

  fn compound_bytes() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let large: Vec<_> = (0..4 * 1024 * 1024)
      .map(|index| (index % 251) as u8)
      .collect();
    let small: Vec<_> = (0..977).map(|index| (index % 239) as u8).collect();
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound.create_storage_all("/Data/Nested").unwrap();
    compound
      .create_stream("/Data/Large", large.clone())
      .unwrap();
    compound
      .create_stream("/Data/Nested/Small", small.clone())
      .unwrap();
    (compound.to_bytes().unwrap(), large, small)
  }

  #[test]
  fn opening_seekable_cfb_does_not_materialize_stream_payloads() {
    let (bytes, large, _) = compound_bytes();
    let file_len = bytes.len() as u64;
    let (source, bytes_read, largest_read) = CountingReader::new(bytes);
    let mut compound =
      CompoundFileReader::from_reader_with_buffer_size(source, Limits::default(), 4096).unwrap();

    assert!(bytes_read.get() < file_len / 8);
    assert!(largest_read.get() <= 4096);
    assert_eq!(
      compound.entry("/data/large").unwrap().stream_len,
      large.len() as u64
    );

    let mut stream = compound.open_stream_borrowed("/DATA/LARGE").unwrap();
    assert_eq!(stream.len(), large.len() as u64);
    assert_eq!(stream.buffer_capacity(), 4096);
    let mut first = [0; 37];
    stream.read_exact(&mut first).unwrap();
    assert_eq!(first.as_slice(), &large[..first.len()]);
    stream.seek(SeekFrom::End(-53)).unwrap();
    let mut tail = Vec::new();
    stream.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, large[large.len() - 53..]);
    assert!(bytes_read.get() < file_len / 4);
  }

  #[test]
  fn mini_stream_reads_and_seeks_across_mini_sector_boundaries() {
    let (bytes, _, small) = compound_bytes();
    let compound = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    let mut stream = compound.open_stream("/Data/Nested/Small").unwrap();
    assert_eq!(stream.len(), small.len() as u64);
    assert_eq!(stream.buffer_capacity(), small.len());
    stream.seek(SeekFrom::Start(61)).unwrap();
    let mut actual = vec![0; 197];
    stream.read_exact(&mut actual).unwrap();
    assert_eq!(actual, small[61..61 + 197]);
    assert!(stream.seek(SeekFrom::End(1)).is_err());
    assert!(stream.seek(SeekFrom::Current(-10_000)).is_err());
  }

  #[test]
  fn positional_stream_cursors_coexist_and_read_interleaved() {
    let (bytes, large, small) = compound_bytes();
    let compound = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    let mut large_stream = compound.open_stream("/Data/Large").unwrap();
    let mut small_stream = compound.open_stream("/Data/Nested/Small").unwrap();

    large_stream.seek(SeekFrom::Start(487)).unwrap();
    small_stream.seek(SeekFrom::Start(61)).unwrap();
    let mut large_chunk = [0; 79];
    let mut small_chunk = [0; 131];
    large_stream.read_exact(&mut large_chunk).unwrap();
    small_stream.read_exact(&mut small_chunk).unwrap();
    assert_eq!(large_chunk, large[487..566]);
    assert_eq!(small_chunk, small[61..192]);

    large_stream.seek(SeekFrom::End(-53)).unwrap();
    small_stream.seek(SeekFrom::Start(0)).unwrap();
    let mut large_tail = Vec::new();
    let mut small_head = [0; 64];
    large_stream.read_to_end(&mut large_tail).unwrap();
    small_stream.read_exact(&mut small_head).unwrap();
    assert_eq!(large_tail, large[large.len() - 53..]);
    assert_eq!(small_head, small[..64]);
  }

  #[test]
  fn positional_stream_cursors_support_parallel_reads_without_a_global_lock() {
    let (bytes, large, small) = compound_bytes();
    let compound = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    std::thread::scope(|scope| {
      let large_read = scope.spawn(|| {
        let mut stream = compound.open_stream("/Data/Large").unwrap();
        stream.seek(SeekFrom::Start(1021)).unwrap();
        let mut output = [0; 257];
        stream.read_exact(&mut output).unwrap();
        output
      });
      let small_read = scope.spawn(|| {
        let mut stream = compound.open_stream("/Data/Nested/Small").unwrap();
        stream.seek(SeekFrom::Start(113)).unwrap();
        let mut output = [0; 197];
        stream.read_exact(&mut output).unwrap();
        output
      });
      assert_eq!(large_read.join().unwrap(), large[1021..1278]);
      assert_eq!(small_read.join().unwrap(), small[113..310]);
    });
  }

  #[test]
  fn into_owned_is_an_explicit_full_feature_fallback() {
    let (bytes, large, small) = compound_bytes();
    let reader = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();
    let owned = reader.into_owned().unwrap();
    assert_eq!(owned.stream("/Data/Large"), Some(large.as_slice()));
    assert_eq!(owned.stream("/Data/Nested/Small"), Some(small.as_slice()));
  }

  #[test]
  fn writable_backing_overwrites_regular_and_mini_streams_in_place() {
    let (bytes, mut large, mut small) = compound_bytes();
    let mut compound = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();

    let large_patch = vec![0xa5; 79];
    {
      let mut stream = compound.open_stream_mut("/Data/Large").unwrap();
      stream.seek(SeekFrom::Start(487)).unwrap();
      stream.write_all(&large_patch).unwrap();
      stream.flush().unwrap();
      stream.seek(SeekFrom::Start(487)).unwrap();
      let mut actual = vec![0; large_patch.len()];
      stream.read_exact(&mut actual).unwrap();
      assert_eq!(actual, large_patch);
    }
    large[487..487 + large_patch.len()].copy_from_slice(&large_patch);

    let small_patch = vec![0x5a; 131];
    {
      let mut stream = compound.open_stream_mut("/Data/Nested/Small").unwrap();
      stream.seek(SeekFrom::Start(61)).unwrap();
      stream.write_all(&small_patch).unwrap();
      stream.flush().unwrap();
    }
    small[61..61 + small_patch.len()].copy_from_slice(&small_patch);

    let bytes = compound.into_inner().into_inner();
    let reopened = CompoundFile::from_bytes_strict(&bytes).unwrap();
    assert_eq!(reopened.stream("/Data/Large"), Some(large.as_slice()));
    assert_eq!(
      reopened.stream("/Data/Nested/Small"),
      Some(small.as_slice())
    );
  }

  #[test]
  fn writable_stream_surfaces_backing_write_and_flush_failures() {
    let (bytes, _, _) = compound_bytes();
    let (backing, control) = FailingCursor::new(bytes);
    let mut compound = CompoundFileReader::from_reader(backing).unwrap();
    let mut stream = compound.open_stream_mut("/Data/Large").unwrap();

    control.writes_before_failure.set(Some(0));
    let error = stream.write_all(&[0xa5]).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);

    control.writes_before_failure.set(None);
    control.fail_flush.set(true);
    let error = stream.flush().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
  }

  #[test]
  fn writable_stream_surfaces_allocation_table_update_failures() {
    let bytes = CompoundFile::new(Version::V3).unwrap().to_bytes().unwrap();
    let (backing, control) = FailingCursor::new(bytes);
    let mut compound = CompoundFileReader::from_reader(backing).unwrap();
    {
      let _stream = compound.create_new_stream("/Growing").unwrap();
    }
    control.writes_before_failure.set(Some(2));
    let mut stream = compound.open_stream_mut("/Growing").unwrap();
    let error = stream.set_len(5000).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
  }

  #[test]
  fn writable_growth_honors_configured_stream_and_directory_limits() {
    let bytes = CompoundFile::new(Version::V3).unwrap().to_bytes().unwrap();
    let limits = Limits {
      max_stream_size: 100,
      max_entries: 4,
      ..Limits::default()
    };
    let mut compound =
      CompoundFileReader::from_reader_with_limits(Cursor::new(bytes), limits).unwrap();

    {
      let mut stream = compound.create_new_stream("/Limited").unwrap();
      let error = stream.set_len(101).unwrap_err();
      assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
      assert_eq!(stream.len(), 0);
    }
    compound.create_storage("/Second").unwrap();
    compound.create_storage("/Third").unwrap();
    assert!(matches!(
      compound.create_storage("/NeedsAnotherDirectorySector"),
      Err(Error::Limit(_))
    ));
  }

  #[test]
  fn writable_growth_preflights_the_configured_file_size_limit() {
    let bytes = CompoundFile::new(Version::V3).unwrap().to_bytes().unwrap();
    let limits = Limits {
      max_file_size: bytes.len() as u64,
      ..Limits::default()
    };
    let mut compound =
      CompoundFileReader::from_reader_with_limits(Cursor::new(bytes), limits).unwrap();
    {
      let mut stream = compound.create_new_stream("/NoRoom").unwrap();
      assert!(stream.set_len(1).is_err());
      assert_eq!(stream.len(), 0);
    }

    let bytes = compound.into_inner().into_inner();
    let reopened = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    assert_eq!(reopened.entry("/NoRoom").unwrap().stream_len, 0);
  }

  #[test]
  fn shrinking_streams_zeroes_unused_regular_and_mini_sector_tails() {
    let (bytes, _, _) = compound_bytes();
    let mut compound = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();
    {
      let mut stream = compound.open_stream_mut("/Data/Large").unwrap();
      stream.set_len(4501).unwrap();
    }
    {
      let mut stream = compound.open_stream_mut("/Data/Nested/Small").unwrap();
      stream.set_len(101).unwrap();
    }

    let regular_index = compound
      .required_entry_index(Path::new("/Data/Large"))
      .unwrap();
    let StreamChain::Regular(regular_chain) = compound.stream_chain(regular_index).unwrap() else {
      panic!("4501-byte stream must use regular sectors");
    };
    let sector_len = compound.source.sector_len();
    let tail_start = 4501 % sector_len;
    let regular_tail = compound
      .source
      .sector(*regular_chain.last().unwrap())
      .unwrap();
    assert!(regular_tail[tail_start..].iter().all(|byte| *byte == 0));

    let mini_index = compound
      .required_entry_index(Path::new("/Data/Nested/Small"))
      .unwrap();
    let StreamChain::Mini {
      mini_chain,
      root_chain,
      root_len,
    } = compound.stream_chain(mini_index).unwrap()
    else {
      panic!("101-byte stream must use mini sectors");
    };
    let within = 101 % MINI_SECTOR_LEN;
    let root_offset =
      u64::from(mini_chain.last().unwrap().get()) * MINI_SECTOR_LEN as u64 + within as u64;
    let mut mini_tail = vec![0; MINI_SECTOR_LEN - within];
    read_regular_at(
      &mut compound.source,
      &root_chain,
      root_len,
      root_offset,
      &mut mini_tail,
    )
    .unwrap();
    assert!(mini_tail.iter().all(|byte| *byte == 0));
  }

  #[test]
  fn writable_regular_stream_grows_and_shrinks_its_fat_chain() {
    let (bytes, mut large, _) = compound_bytes();
    let patch = vec![0x3c; 777];
    let mut compound = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();
    {
      let mut stream = compound.open_stream_mut("/Data/Large").unwrap();
      stream.seek(SeekFrom::End(0)).unwrap();
      stream.write_all(&patch).unwrap();
      stream.flush().unwrap();
    }
    large.extend_from_slice(&patch);

    let bytes = compound.into_inner().into_inner();
    let mut compound = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    let mut actual = Vec::new();
    compound
      .open_stream("/Data/Large")
      .unwrap()
      .read_to_end(&mut actual)
      .unwrap();
    assert_eq!(actual, large);

    {
      let mut stream = compound.open_stream_mut("/Data/Large").unwrap();
      stream.set_len(5000).unwrap();
      stream.flush().unwrap();
    }
    let bytes = compound.into_inner().into_inner();
    let reopened = CompoundFile::from_bytes_strict(&bytes).unwrap();
    assert_eq!(reopened.stream("/Data/Large"), Some(&large[..5000]));
  }

  #[test]
  fn writable_mini_stream_grows_root_stream_and_releases_mini_chain() {
    let (bytes, _, mut small) = compound_bytes();
    let patch = vec![0xc3; 333];
    let mut compound = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();
    {
      let mut stream = compound.open_stream_mut("/Data/Nested/Small").unwrap();
      stream.seek(SeekFrom::End(0)).unwrap();
      stream.write_all(&patch).unwrap();
      stream.flush().unwrap();
    }
    small.extend_from_slice(&patch);

    let bytes = compound.into_inner().into_inner();
    let mut compound = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    let mut actual = Vec::new();
    compound
      .open_stream("/Data/Nested/Small")
      .unwrap()
      .read_to_end(&mut actual)
      .unwrap();
    assert_eq!(actual, small);

    {
      let mut stream = compound.open_stream_mut("/Data/Nested/Small").unwrap();
      stream.set_len(100).unwrap();
      stream.flush().unwrap();
    }
    let bytes = compound.into_inner().into_inner();
    let reopened = CompoundFile::from_bytes_strict(&bytes).unwrap();
    assert_eq!(reopened.stream("/Data/Nested/Small"), Some(&small[..100]));
  }

  #[test]
  fn writable_stream_migrates_across_the_mini_stream_cutoff_both_ways() {
    let (bytes, _, small) = compound_bytes();
    let mut compound = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();
    {
      let mut stream = compound.open_stream_mut("/Data/Nested/Small").unwrap();
      stream.set_len(5000).unwrap();
      stream.flush().unwrap();
    }
    let bytes = compound.into_inner().into_inner();
    let mut compound = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    let mut grown = Vec::new();
    compound
      .open_stream("/Data/Nested/Small")
      .unwrap()
      .read_to_end(&mut grown)
      .unwrap();
    assert_eq!(&grown[..small.len()], small);
    assert!(grown[small.len()..].iter().all(|byte| *byte == 0));

    {
      let mut stream = compound.open_stream_mut("/Data/Nested/Small").unwrap();
      stream.set_len(1000).unwrap();
      stream.flush().unwrap();
    }
    let bytes = compound.into_inner().into_inner();
    let reopened = CompoundFile::from_bytes_strict(&bytes).unwrap();
    assert_eq!(reopened.stream("/Data/Nested/Small"), Some(&grown[..1000]));
  }

  #[test]
  fn empty_stream_growth_creates_the_first_minifat_and_root_stream() {
    for version in [Version::V3, Version::V4] {
      let mut owned = CompoundFile::new(version).unwrap();
      owned.create_stream("/Empty", Vec::new()).unwrap();
      let bytes = owned.to_bytes().unwrap();
      let payload = vec![0x71; 100];

      let mut compound = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();
      {
        let mut stream = compound.open_stream_mut("/Empty").unwrap();
        assert_eq!(stream.buffer_capacity(), 1);
        stream.write_all(&payload).unwrap();
        assert_eq!(stream.buffer_capacity(), payload.len());
        stream.flush().unwrap();
      }
      let bytes = compound.into_inner().into_inner();
      let reopened = CompoundFile::from_bytes_strict(&bytes).unwrap();
      assert_eq!(reopened.stream("/Empty"), Some(payload.as_slice()));
    }
  }

  #[test]
  fn regular_stream_growth_adds_a_fat_sector_when_coverage_is_exhausted() {
    let mut owned = CompoundFile::new(Version::V3).unwrap();
    owned.create_stream("/Large", Vec::new()).unwrap();
    let bytes = owned.to_bytes().unwrap();
    let mut compound = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();
    {
      let mut stream = compound.open_stream_mut("/Large").unwrap();
      stream.set_len(70_000).unwrap();
      stream.flush().unwrap();
    }
    let bytes = compound.into_inner().into_inner();
    let reopened = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    assert!(reopened.header().number_of_fat_sectors >= 2);
    let mut data = Vec::new();
    reopened
      .open_stream("/Large")
      .unwrap()
      .read_to_end(&mut data)
      .unwrap();
    assert_eq!(data.len(), 70_000);
    assert!(data.iter().all(|byte| *byte == 0));
  }

  #[test]
  fn large_regular_stream_growth_bootstraps_an_external_difat_chain() {
    let mut owned = CompoundFile::new(Version::V3).unwrap();
    owned.create_stream("/Large", Vec::new()).unwrap();
    let bytes = owned.to_bytes().unwrap();
    let mut compound = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();
    {
      let mut stream = compound.open_stream_mut("/Large").unwrap();
      stream.set_len(7_200_000).unwrap();
      stream.flush().unwrap();
    }
    let bytes = compound.into_inner().into_inner();
    let reopened = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    assert!(reopened.header().number_of_fat_sectors > 109);
    assert!(reopened.header().number_of_difat_sectors >= 1);
    let mut stream = reopened.open_stream("/Large").unwrap();
    assert_eq!(stream.len(), 7_200_000);
    stream.seek(SeekFrom::End(-32)).unwrap();
    let mut tail = Vec::new();
    stream.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, [0; 32]);
  }

  #[test]
  fn file_backed_directory_creates_and_removes_streams() {
    let (bytes, _, _) = compound_bytes();
    let payload = vec![0x42; 5000];
    let mut compound = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();
    {
      let mut stream = compound.create_new_stream("/Data/Created").unwrap();
      stream.write_all(&payload).unwrap();
      stream.flush().unwrap();
    }
    assert!(compound.create_new_stream("/data/created").is_err());
    compound.remove_stream("/Data/Nested/Small").unwrap();
    compound.flush().unwrap();

    let bytes = compound.into_inner().into_inner();
    let mut reopened = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    assert!(!reopened.contains_entry("/Data/Nested/Small"));
    let mut actual = Vec::new();
    reopened
      .open_stream("/Data/Created")
      .unwrap()
      .read_to_end(&mut actual)
      .unwrap();
    assert_eq!(actual, payload);

    reopened.remove_stream("/Data/Created").unwrap();
    let bytes = reopened.into_inner().into_inner();
    let reopened = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    assert!(!reopened.contains_entry("/Data/Created"));
  }

  #[test]
  fn file_path_create_opens_a_strictly_valid_writable_compound_file() {
    for version in [Version::V3, Version::V4] {
      let path = std::env::temp_dir().join(format!(
        "olecfsdk-create-{}-{}.cfb",
        std::process::id(),
        match version {
          Version::V3 => 3,
          Version::V4 => 4,
        }
      ));
      let mut compound = CompoundFileReader::create_with_version(&path, version).unwrap();
      compound.create_storage("/Data").unwrap();
      {
        let mut stream = compound.create_new_stream("/Data/Value").unwrap();
        stream.write_all(b"created through file backing").unwrap();
        stream.flush().unwrap();
      }
      drop(compound);

      let reopened = CompoundFileReader::open_strict(&path).unwrap();
      let mut actual = Vec::new();
      reopened
        .open_stream("/Data/Value")
        .unwrap()
        .read_to_end(&mut actual)
        .unwrap();
      assert_eq!(actual, b"created through file backing");
      drop(reopened);
      std::fs::remove_file(path).unwrap();
    }
  }

  #[test]
  fn file_backed_directory_moves_and_removes_storage_trees() {
    let (bytes, _, small) = compound_bytes();
    let mut compound = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();
    compound.create_storage_all("/Other/Nested").unwrap();
    assert!(compound.remove_storage("/Data/Nested").is_err());
    compound.move_entry("/Data/Nested", "/Other/Moved").unwrap();
    compound.rename_entry("/Other/Moved", "Renamed").unwrap();
    assert!(!compound.contains_entry("/Data/Nested/Small"));
    let mut actual = Vec::new();
    compound
      .open_stream("/Other/Renamed/Small")
      .unwrap()
      .read_to_end(&mut actual)
      .unwrap();
    assert_eq!(actual, small);
    compound.flush().unwrap();

    let bytes = compound.into_inner().into_inner();
    let mut reopened = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    assert!(reopened.contains_entry("/Other/Renamed/Small"));
    reopened.remove_storage_all("/Other").unwrap();
    let bytes = reopened.into_inner().into_inner();
    let reopened = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    assert!(!reopened.contains_entry("/Other"));
    assert!(reopened.contains_entry("/Data/Large"));
  }

  #[test]
  fn file_backed_directory_updates_spec_metadata() {
    let (bytes, _, _) = compound_bytes();
    let mut compound = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();
    let clsid = Guid {
      data1: 0x1234_5678,
      data2: 0x9abc,
      data3: 0xdef0,
      data4: [1, 2, 3, 4, 5, 6, 7, 8],
    };
    compound.set_storage_clsid("/Data", clsid).unwrap();
    compound.set_state_bits("/Data/Large", 0xa5a5_5a5a).unwrap();
    compound.set_created_time("/Data", FileTime(10)).unwrap();
    compound.set_modified_time("/Data", FileTime(20)).unwrap();
    assert!(
      compound
        .set_created_time("/Data/Large", FileTime(30))
        .is_err()
    );

    let bytes = compound.into_inner().into_inner();
    let reopened = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    let storage = reopened.entry("/Data").unwrap();
    assert_eq!(storage.clsid, clsid);
    assert_eq!(storage.created, FileTime(10));
    assert_eq!(storage.modified, FileTime(20));
    assert_eq!(
      reopened.entry("/Data/Large").unwrap().state_bits,
      0xa5a5_5a5a
    );
  }

  #[test]
  fn file_backed_directory_chain_grows_in_v3_and_v4() {
    for version in [Version::V3, Version::V4] {
      let bytes = CompoundFile::new(version).unwrap().to_bytes().unwrap();
      let mut compound = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();
      for index in 0..40 {
        compound
          .create_storage(format!("/Storage{index:02}"))
          .unwrap();
      }
      compound.validate_strict().unwrap();
      if version == Version::V3 {
        assert_eq!(compound.directory().declared_sector_count(), 0);
      }
      compound.flush().unwrap();
      let bytes = compound.into_inner().into_inner();
      let reopened = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
      assert_eq!(reopened.entries().len(), 41);
      assert!(reopened.directory().sectors().len() > 1);
      if version == Version::V4 {
        assert_eq!(
          reopened.header().number_of_directory_sectors as usize,
          reopened.directory().sectors().len()
        );
      }
    }
  }

  #[test]
  fn file_backed_directory_growth_expands_the_fat() {
    let bytes = CompoundFile::new(Version::V3).unwrap().to_bytes().unwrap();
    let mut compound = CompoundFileReader::from_reader(Cursor::new(bytes)).unwrap();
    for index in 0..520 {
      compound
        .create_storage(format!("/Storage{index:04}"))
        .unwrap();
    }
    compound.validate_strict().unwrap();
    assert!(compound.header().number_of_fat_sectors > 1);

    let bytes = compound.into_inner().into_inner();
    let reopened = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    assert_eq!(reopened.entries().len(), 521);
    assert!(reopened.contains_entry("/Storage0519"));
  }

  #[test]
  fn file_backed_storage_copy_uses_bounded_stream_transfers() {
    let (bytes, large, small) = compound_bytes();
    let mut compound =
      CompoundFileReader::from_reader_with_buffer_size(Cursor::new(bytes), Limits::default(), 4096)
        .unwrap();
    compound.copy_entry("/Data", "/Clone").unwrap();
    compound.flush().unwrap();
    let bytes = compound.into_inner().into_inner();
    let reopened = CompoundFileReader::from_reader_strict(Cursor::new(bytes)).unwrap();
    let mut actual_large = Vec::new();
    reopened
      .open_stream("/Clone/Large")
      .unwrap()
      .read_to_end(&mut actual_large)
      .unwrap();
    let mut actual_small = Vec::new();
    reopened
      .open_stream("/Clone/Nested/Small")
      .unwrap()
      .read_to_end(&mut actual_small)
      .unwrap();
    assert_eq!(actual_large, large);
    assert_eq!(actual_small, small);
  }
}
