use std::collections::BTreeSet;

use crate::{Error, Result, limits::Limits};

use super::{
  header::{FREE_SECTOR, Header},
  sector::{MAX_REGULAR_SECTOR, MiniSectorId, SectorId, SectorRead},
};

pub const DIFAT_SECTOR: u32 = 0xffff_fffc;
pub const FAT_SECTOR: u32 = 0xffff_fffd;
pub const END_OF_CHAIN: u32 = 0xffff_fffe;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FatEntry {
  Sector(SectorId),
  DifatSector,
  FatSector,
  EndOfChain,
  Free,
  Invalid(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MiniFatEntry {
  MiniSector(MiniSectorId),
  EndOfChain,
  Free,
  Invalid(u32),
}

impl MiniFatEntry {
  pub fn from_raw(value: u32) -> Self {
    match value {
      0..=MAX_REGULAR_SECTOR => Self::MiniSector(MiniSectorId::new(value).unwrap()),
      END_OF_CHAIN => Self::EndOfChain,
      FREE_SECTOR => Self::Free,
      value => Self::Invalid(value),
    }
  }

  pub fn raw(self) -> u32 {
    match self {
      Self::MiniSector(id) => id.get(),
      Self::EndOfChain => END_OF_CHAIN,
      Self::Free => FREE_SECTOR,
      Self::Invalid(value) => value,
    }
  }
}

impl FatEntry {
  pub fn from_raw(value: u32) -> Self {
    match value {
      0..=MAX_REGULAR_SECTOR => Self::Sector(SectorId::new(value).unwrap()),
      DIFAT_SECTOR => Self::DifatSector,
      FAT_SECTOR => Self::FatSector,
      END_OF_CHAIN => Self::EndOfChain,
      FREE_SECTOR => Self::Free,
      value => Self::Invalid(value),
    }
  }

  pub fn raw(self) -> u32 {
    match self {
      Self::Sector(id) => id.get(),
      Self::DifatSector => DIFAT_SECTOR,
      Self::FatSector => FAT_SECTOR,
      Self::EndOfChain => END_OF_CHAIN,
      Self::Free => FREE_SECTOR,
      Self::Invalid(value) => value,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difat {
  fat_sectors: Vec<SectorId>,
  difat_sectors: Vec<SectorId>,
  canonical_entries: bool,
  canonical_terminator: bool,
}

impl Difat {
  pub(crate) fn push_header_fat_sector(&mut self, sector: SectorId) -> Result<()> {
    if self.fat_sectors.len() >= 109 {
      return Err(Error::invalid(0, "header DIFAT is full"));
    }
    self.fat_sectors.push(sector);
    Ok(())
  }

  pub(crate) fn push_external_fat_sector(&mut self, sector: SectorId) {
    self.fat_sectors.push(sector);
  }

  pub(crate) fn push_difat_sector(&mut self, sector: SectorId) {
    self.difat_sectors.push(sector);
  }

  pub(crate) fn read<S: SectorRead + ?Sized>(
    header: &Header,
    source: &mut S,
    limits: Limits,
  ) -> Result<Self> {
    let expected_fat = usize::try_from(header.number_of_fat_sectors)
      .map_err(|_| Error::Limit("FAT sector count does not fit usize".into()))?;
    if expected_fat > source.sector_count() {
      return Err(Error::invalid(44, "FAT sector count exceeds file sectors"));
    }
    let fat_bytes = expected_fat
      .checked_mul(source.sector_len())
      .ok_or_else(|| Error::Limit("FAT allocation size overflow".into()))?;
    if fat_bytes > limits.max_allocation {
      return Err(Error::Limit(format!(
        "FAT allocation {fat_bytes} exceeds {}",
        limits.max_allocation
      )));
    }

    let mut fat_sectors = Vec::with_capacity(expected_fat);
    let mut seen_fat = BTreeSet::new();
    for &raw in &header.difat {
      if fat_sectors.len() == expected_fat || raw == FREE_SECTOR {
        break;
      }
      push_unique_sector(raw, "header DIFAT", &mut fat_sectors, &mut seen_fat)?;
    }
    let header_used = expected_fat.min(header.difat.len());
    let mut canonical_entries = header.difat[..header_used]
      .iter()
      .all(|raw| *raw != FREE_SECTOR)
      && header.difat[header_used..]
        .iter()
        .all(|raw| *raw == FREE_SECTOR);

    let expected_difat = usize::try_from(header.number_of_difat_sectors)
      .map_err(|_| Error::Limit("DIFAT sector count does not fit usize".into()))?;
    if expected_difat > source.sector_count() {
      return Err(Error::invalid(
        72,
        "DIFAT sector count exceeds file sectors",
      ));
    }
    let mut difat_sectors = Vec::with_capacity(expected_difat);
    let mut seen_difat = BTreeSet::new();
    let mut next = header.first_difat_sector;
    for _ in 0..expected_difat {
      if matches!(next, END_OF_CHAIN | FREE_SECTOR) {
        return Err(Error::invalid(
          68,
          "DIFAT chain ended before declared count",
        ));
      }
      let id = SectorId::new(next)?;
      if !seen_difat.insert(id) {
        return Err(Error::invalid(68, "DIFAT sector chain contains a cycle"));
      }
      difat_sectors.push(id);
      let sector = source.full_sector(id)?;
      let values: Vec<_> = u32_entries(sector.as_ref()).collect();
      let (chain, entries) = values
        .split_last()
        .ok_or_else(|| Error::invalid(0, "empty DIFAT sector"))?;
      let needed = expected_fat.saturating_sub(fat_sectors.len());
      let used = needed.min(entries.len());
      canonical_entries &= entries[..used].iter().all(|raw| *raw != FREE_SECTOR);
      if fat_sectors.len() < expected_fat {
        for &raw in &entries[..used] {
          if fat_sectors.len() == expected_fat || raw == FREE_SECTOR {
            break;
          }
          push_unique_sector(raw, "DIFAT", &mut fat_sectors, &mut seen_fat)?;
        }
      }
      canonical_entries &= entries[used..].iter().all(|raw| *raw == FREE_SECTOR);
      next = *chain;
    }
    if fat_sectors.len() != expected_fat {
      return Err(Error::invalid(
        44,
        format!(
          "declared {expected_fat} FAT sectors but DIFAT contains {}",
          fat_sectors.len()
        ),
      ));
    }
    Ok(Self {
      fat_sectors,
      difat_sectors,
      canonical_entries,
      canonical_terminator: next == END_OF_CHAIN,
    })
  }

  pub fn fat_sectors(&self) -> &[SectorId] {
    &self.fat_sectors
  }

  pub fn difat_sectors(&self) -> &[SectorId] {
    &self.difat_sectors
  }

  pub(crate) fn validate_strict(&self) -> Result<()> {
    if !self.canonical_entries {
      return Err(Error::invalid(
        76,
        "CFB DIFAT FAT locations must be contiguous and remaining entries FREESECT",
      ));
    }
    if !self.canonical_terminator {
      return Err(Error::invalid(
        68,
        "CFB DIFAT chain must terminate with ENDOFCHAIN",
      ));
    }
    Ok(())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fat {
  entries: Vec<FatEntry>,
  marker_mismatches: Vec<FatMarkerMismatch>,
  file_sector_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FatMarkerMismatch {
  pub sector: SectorId,
  pub expected: FatEntry,
  pub actual: FatEntry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MiniFat {
  sectors: Vec<SectorId>,
  entries: Vec<MiniFatEntry>,
  declared_sector_count: u32,
}

impl MiniFat {
  pub(crate) fn entry(&self, id: MiniSectorId) -> Option<MiniFatEntry> {
    self.entries.get(id.get() as usize).copied()
  }

  pub(crate) fn set_entry(&mut self, id: MiniSectorId, entry: MiniFatEntry) -> Result<()> {
    let slot = self
      .entries
      .get_mut(id.get() as usize)
      .ok_or_else(|| Error::invalid(0, "MiniFAT entry is outside the allocation table"))?;
    *slot = entry;
    Ok(())
  }

  pub(crate) fn allocation_capacity(&self) -> usize {
    self.entries.len()
  }

  pub(crate) fn push_sector(&mut self, sector: SectorId, entries_per_sector: usize) {
    self.sectors.push(sector);
    self
      .entries
      .extend(std::iter::repeat_n(MiniFatEntry::Free, entries_per_sector));
    self.declared_sector_count = self.sectors.len() as u32;
  }

  pub(crate) fn read<S: SectorRead + ?Sized>(
    header: &Header,
    fat: &Fat,
    source: &mut S,
    limits: Limits,
  ) -> Result<Self> {
    let sectors = if matches!(header.first_mini_fat_sector, END_OF_CHAIN | FREE_SECTOR) {
      Vec::new()
    } else {
      fat.chain(header.first_mini_fat_sector, source.sector_count())?
    };
    let allocation = sectors
      .len()
      .checked_mul(source.sector_len())
      .ok_or_else(|| Error::Limit("MiniFAT allocation size overflow".into()))?;
    if allocation > limits.max_allocation {
      return Err(Error::Limit(format!(
        "MiniFAT allocation {allocation} exceeds {}",
        limits.max_allocation
      )));
    }
    let mut entries = Vec::with_capacity(allocation / 4);
    for &sector in &sectors {
      let bytes = source.full_sector(sector)?;
      entries.extend(u32_entries(bytes.as_ref()).map(MiniFatEntry::from_raw));
    }
    Ok(Self {
      sectors,
      entries,
      declared_sector_count: header.number_of_mini_fat_sectors,
    })
  }

  pub fn sectors(&self) -> &[SectorId] {
    &self.sectors
  }

  pub fn entries(&self) -> &[MiniFatEntry] {
    &self.entries
  }

  pub fn declared_sector_count(&self) -> u32 {
    self.declared_sector_count
  }

  pub fn sector_count_matches_header(&self) -> bool {
    self.sectors.len() == self.declared_sector_count as usize
  }

  pub(crate) fn validate_strict(&self, mini_sector_count: usize) -> Result<()> {
    if self.entries.len() < mini_sector_count {
      return Err(Error::invalid(
        0,
        "CFB MiniFAT does not cover the root mini stream",
      ));
    }
    let mut pointees = BTreeSet::new();
    for entry in &self.entries[..mini_sector_count] {
      match *entry {
        MiniFatEntry::MiniSector(target) => {
          if target.get() as usize >= mini_sector_count {
            return Err(Error::invalid(
              0,
              "CFB MiniFAT points beyond the mini stream",
            ));
          }
          if !pointees.insert(target) {
            return Err(Error::invalid(0, "CFB mini-sector is pointed to twice"));
          }
        }
        MiniFatEntry::Invalid(value) => {
          return Err(Error::invalid(
            0,
            format!("invalid MiniFAT marker 0x{value:08x}"),
          ));
        }
        MiniFatEntry::EndOfChain | MiniFatEntry::Free => {}
      }
    }
    if self.entries[mini_sector_count..]
      .iter()
      .any(|entry| *entry != MiniFatEntry::Free)
    {
      return Err(Error::invalid(
        0,
        "CFB MiniFAT entries beyond the mini stream must be FREESECT",
      ));
    }
    Ok(())
  }

  pub fn chain(&self, start: u32, mini_sector_count: usize) -> Result<Vec<MiniSectorId>> {
    if start == END_OF_CHAIN {
      return Ok(Vec::new());
    }
    let mut current = MiniSectorId::new(start)?;
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    loop {
      let index = current.get() as usize;
      if index >= mini_sector_count {
        return Err(Error::invalid(
          0,
          format!("mini-sector {index} is beyond the root mini stream"),
        ));
      }
      if !seen.insert(current) {
        return Err(Error::invalid(0, "MiniFAT chain contains a cycle"));
      }
      chain.push(current);
      let entry = self
        .entries
        .get(index)
        .copied()
        .ok_or_else(|| Error::invalid(0, "mini-sector chain is outside the MiniFAT"))?;
      match entry {
        MiniFatEntry::MiniSector(next) => current = next,
        MiniFatEntry::EndOfChain => return Ok(chain),
        other => {
          return Err(Error::invalid(
            0,
            format!("invalid MiniFAT chain marker 0x{:08x}", other.raw()),
          ));
        }
      }
    }
  }
}

impl Fat {
  pub(crate) fn entry(&self, id: SectorId) -> Option<FatEntry> {
    self.entries.get(id.get() as usize).copied()
  }

  pub(crate) fn set_entry(&mut self, id: SectorId, entry: FatEntry) -> Result<()> {
    let slot = self
      .entries
      .get_mut(id.get() as usize)
      .ok_or_else(|| Error::invalid(0, "FAT entry is outside the allocation table"))?;
    *slot = entry;
    self.file_sector_count = self.file_sector_count.max(id.get() as usize + 1);
    Ok(())
  }

  pub(crate) fn allocation_capacity(&self) -> usize {
    self.entries.len()
  }

  pub(crate) fn push_fat_sector(&mut self, sector: SectorId, entries_per_sector: usize) {
    debug_assert_eq!(sector.get() as usize, self.entries.len());
    self.entries.push(FatEntry::FatSector);
    self
      .entries
      .extend(std::iter::repeat_n(FatEntry::Free, entries_per_sector - 1));
    self.file_sector_count = self.file_sector_count.max(sector.get() as usize + 1);
  }

  pub(crate) fn push_difat_and_fat_sectors(
    &mut self,
    difat_sector: SectorId,
    fat_sector: SectorId,
    entries_per_sector: usize,
  ) {
    debug_assert_eq!(difat_sector.get() as usize, self.entries.len());
    debug_assert_eq!(fat_sector.get(), difat_sector.get() + 1);
    self.entries.push(FatEntry::DifatSector);
    self.entries.push(FatEntry::FatSector);
    self
      .entries
      .extend(std::iter::repeat_n(FatEntry::Free, entries_per_sector - 2));
    self.file_sector_count = self.file_sector_count.max(fat_sector.get() as usize + 1);
  }

  pub(crate) fn read<S: SectorRead + ?Sized>(difat: &Difat, source: &mut S) -> Result<Self> {
    let entries_per_sector = source.sector_len() / 4;
    let capacity = difat
      .fat_sectors
      .len()
      .checked_mul(entries_per_sector)
      .ok_or_else(|| Error::Limit("FAT entry count overflow".into()))?;
    let mut entries = Vec::with_capacity(capacity);
    for &sector in &difat.fat_sectors {
      let bytes = source.full_sector(sector)?;
      entries.extend(u32_entries(bytes.as_ref()).map(FatEntry::from_raw));
    }
    if source.has_partial_sector() {
      let mut effective_len = entries.len();
      while effective_len > source.sector_count() {
        let last = entries[effective_len - 1];
        let is_compatible_padding = matches!(
          last,
          FatEntry::Free | FatEntry::FatSector | FatEntry::DifatSector
        ) || matches!(last, FatEntry::Sector(id) if id.get() == 0);
        if !is_compatible_padding {
          break;
        }
        effective_len -= 1;
      }
      if effective_len > source.sector_count() {
        return Err(Error::invalid(
          44,
          format!(
            "FAT has {effective_len} effective entries but partial file has only {} sectors",
            source.sector_count()
          ),
        ));
      }
    }
    let mut marker_mismatches = Vec::new();
    collect_marker_mismatches(
      &entries,
      &difat.fat_sectors,
      FatEntry::FatSector,
      &mut marker_mismatches,
    )?;
    collect_marker_mismatches(
      &entries,
      &difat.difat_sectors,
      FatEntry::DifatSector,
      &mut marker_mismatches,
    )?;
    Ok(Self {
      entries,
      marker_mismatches,
      file_sector_count: source.sector_count() - usize::from(source.has_partial_sector()),
    })
  }

  pub fn entries(&self) -> &[FatEntry] {
    &self.entries
  }

  /// Non-canonical allocation-sector markers tolerated in compatibility
  /// mode. The original FAT entries remain available in `entries()`.
  pub fn marker_mismatches(&self) -> &[FatMarkerMismatch] {
    &self.marker_mismatches
  }

  pub(crate) fn validate_strict(&self) -> Result<()> {
    if self.entries.len() < self.file_sector_count {
      return Err(Error::invalid(
        44,
        "CFB FAT does not cover every file sector",
      ));
    }
    let mut pointees = BTreeSet::new();
    for entry in &self.entries[..self.file_sector_count] {
      match *entry {
        FatEntry::Sector(target) => {
          if target.get() as usize >= self.file_sector_count {
            return Err(Error::invalid(0, "CFB FAT points beyond the file"));
          }
          if !pointees.insert(target) {
            return Err(Error::invalid(0, "CFB sector is pointed to twice"));
          }
        }
        FatEntry::Invalid(value) => {
          return Err(Error::invalid(
            0,
            format!("invalid FAT marker 0x{value:08x}"),
          ));
        }
        FatEntry::DifatSector | FatEntry::FatSector | FatEntry::EndOfChain | FatEntry::Free => {}
      }
    }
    if self.entries[self.file_sector_count..]
      .iter()
      .any(|entry| *entry != FatEntry::Free)
    {
      return Err(Error::invalid(
        0,
        "CFB FAT entries beyond end-of-file must be FREESECT",
      ));
    }
    Ok(())
  }

  pub(crate) fn is_free_or_unaddressed(&self, sector: SectorId) -> bool {
    self
      .entries
      .get(sector.get() as usize)
      .is_none_or(|entry| *entry == FatEntry::Free)
  }

  pub fn chain(&self, start: u32, sector_count: usize) -> Result<Vec<SectorId>> {
    if start == END_OF_CHAIN {
      return Ok(Vec::new());
    }
    let mut current = SectorId::new(start)?;
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    loop {
      let index = current.get() as usize;
      if index >= sector_count {
        return Err(Error::invalid(0, format!("sector {index} is beyond EOF")));
      }
      if !seen.insert(current) {
        return Err(Error::invalid(0, "FAT chain contains a cycle"));
      }
      chain.push(current);
      let entry = self
        .entries
        .get(index)
        .copied()
        .ok_or_else(|| Error::invalid(0, "FAT chain is outside the FAT"))?;
      match entry {
        FatEntry::Sector(next) => current = next,
        FatEntry::EndOfChain => return Ok(chain),
        other => {
          return Err(Error::invalid(
            0,
            format!("invalid FAT chain marker 0x{:08x}", other.raw()),
          ));
        }
      }
    }
  }
}

fn push_unique_sector(
  raw: u32,
  table: &str,
  output: &mut Vec<SectorId>,
  seen: &mut BTreeSet<SectorId>,
) -> Result<()> {
  let id = SectorId::new(raw)?;
  if !seen.insert(id) {
    return Err(Error::invalid(0, format!("duplicate sector in {table}")));
  }
  output.push(id);
  Ok(())
}

fn collect_marker_mismatches(
  entries: &[FatEntry],
  sectors: &[SectorId],
  expected: FatEntry,
  output: &mut Vec<FatMarkerMismatch>,
) -> Result<()> {
  for &sector in sectors {
    let actual = entries
      .get(sector.get() as usize)
      .copied()
      .ok_or_else(|| Error::invalid(0, "allocation sector is outside the FAT"))?;
    if actual != expected {
      output.push(FatMarkerMismatch {
        sector,
        expected,
        actual,
      });
    }
  }
  Ok(())
}

fn u32_entries(bytes: &[u8]) -> impl Iterator<Item = u32> + '_ {
  bytes
    .chunks_exact(4)
    .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn fat_entry_preserves_every_raw_value() {
    for raw in [
      0,
      MAX_REGULAR_SECTOR,
      0xffff_fffb,
      DIFAT_SECTOR,
      FAT_SECTOR,
      END_OF_CHAIN,
      FREE_SECTOR,
    ] {
      assert_eq!(FatEntry::from_raw(raw).raw(), raw);
    }
  }

  #[test]
  fn chain_detects_cycles_and_special_markers() {
    let fat = Fat {
      entries: vec![
        FatEntry::Sector(SectorId::new(1).unwrap()),
        FatEntry::EndOfChain,
      ],
      marker_mismatches: Vec::new(),
      file_sector_count: 2,
    };
    assert_eq!(
      fat.chain(0, 2).unwrap(),
      [SectorId::new(0).unwrap(), SectorId::new(1).unwrap()]
    );

    let cyclic = Fat {
      entries: vec![FatEntry::Sector(SectorId::new(0).unwrap())],
      marker_mismatches: Vec::new(),
      file_sector_count: 1,
    };
    assert!(cyclic.chain(0, 1).is_err());
    assert!(fat.chain(FREE_SECTOR, 2).is_err());
  }
}
