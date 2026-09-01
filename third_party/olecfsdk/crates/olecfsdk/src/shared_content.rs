//! Office-wide content owned by a binary file root.
//!
//! These structures sit above MS-CFB and below the host-specific DOC, XLS,
//! and PPT trees. Each located node retains its CFB identity, while its
//! payload is the sole write authority for that managed stream.

use std::{
  path::{Path, PathBuf},
  sync::Arc,
};

use crate::{
  Error, ParseDiagnostic, ParseDiagnosticCode, ParseOptions, ParseOutcome, Result, SaveOptions,
  SpecificationReference,
  cfb::{CompoundFile, EntryKind},
  forms::ParentControlStorageModel,
  io::BinaryFormat,
  property_set::PropertySetStream,
  vba::{
    DOC_VBA_PROJECT_STORAGE_NAME, LocatedVbaProject, VBA_STORAGE_NAME, VbaModuleSourceMutation,
    XLS_VBA_PROJECT_STORAGE_NAME,
  },
};

pub const SUMMARY_INFORMATION_STREAM: &str = "/\u{5}SummaryInformation";
pub const DOCUMENT_SUMMARY_INFORMATION_STREAM: &str = "/\u{5}DocumentSummaryInformation";
const DOCUMENT_SUMMARY_INFORMATION_FMTID: [u8; 16] = [
  0x02, 0xd5, 0xcd, 0xd5, 0x9c, 0x2e, 0x1b, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2b, 0x2c, 0xf9, 0xae,
];
const VBA_DIGITAL_SIGNATURE_PROPERTY_ID: u32 = 0x18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OfficeHostKind {
  Doc,
  Xls,
  Ppt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficePropertySetKind {
  SummaryInformation,
  DocumentSummaryInformation,
}

impl OfficePropertySetKind {
  pub const fn canonical_path(self) -> &'static str {
    match self {
      Self::SummaryInformation => SUMMARY_INFORMATION_STREAM,
      Self::DocumentSummaryInformation => DOCUMENT_SUMMARY_INFORMATION_STREAM,
    }
  }

  /// CFB directory-entry name without its parent path.
  pub const fn stream_name(self) -> &'static str {
    match self {
      Self::SummaryInformation => "\u{5}SummaryInformation",
      Self::DocumentSummaryInformation => "\u{5}DocumentSummaryInformation",
    }
  }

  fn from_stream_name(name: &str) -> Option<Self> {
    [Self::SummaryInformation, Self::DocumentSummaryInformation]
      .into_iter()
      .find(|kind| name.eq_ignore_ascii_case(kind.stream_name()))
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficePropertySetData {
  Parsed(PropertySetStream),
  Compatibility { bytes: Vec<u8>, reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedPropertySetStream {
  path: PathBuf,
  kind: OfficePropertySetKind,
  data: OfficePropertySetData,
}

impl LocatedPropertySetStream {
  pub fn path(&self) -> &Path {
    &self.path
  }

  pub const fn kind(&self) -> OfficePropertySetKind {
    self.kind
  }

  pub const fn data(&self) -> &OfficePropertySetData {
    &self.data
  }

  pub fn property_set(&self) -> Option<&PropertySetStream> {
    match &self.data {
      OfficePropertySetData::Parsed(value) => Some(value),
      OfficePropertySetData::Compatibility { .. } => None,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfficeVbaProject {
  Parsed(Box<LocatedVbaProject>),
  Compatibility {
    project_root_path: PathBuf,
    vba_storage_path: Option<PathBuf>,
    reason: String,
  },
}

impl OfficeVbaProject {
  pub fn project(&self) -> Option<&LocatedVbaProject> {
    match self {
      Self::Parsed(project) => Some(project),
      Self::Compatibility { .. } => None,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeVbaModuleMutation {
  pub vba: VbaModuleSourceMutation,
  pub invalidated_oleps_signatures: usize,
  pub invalidated_host_signatures: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeFormsMutation {
  pub invalidated_oleps_signatures: usize,
  pub invalidated_host_signatures: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OfficeSharedContent {
  property_set_streams: Arc<Vec<LocatedPropertySetStream>>,
  vba_project: Option<Arc<OfficeVbaProject>>,
}

impl OfficeSharedContent {
  pub fn from_compound_file_with_options(
    compound: &CompoundFile,
    options: ParseOptions,
  ) -> Result<ParseOutcome<Self>> {
    Self::from_compound_file_with_host(compound, options, None)
  }

  pub(crate) fn from_compound_file_with_host(
    compound: &CompoundFile,
    options: ParseOptions,
    host: Option<OfficeHostKind>,
  ) -> Result<ParseOutcome<Self>> {
    let mut property_set_streams = Vec::new();
    let mut diagnostics = Vec::new();
    for entry in compound.entries().iter().filter(|entry| {
      entry.kind == EntryKind::Stream && entry.path.parent() == Some(Path::new("/"))
    }) {
      let Some(kind) = OfficePropertySetKind::from_stream_name(&entry.name) else {
        continue;
      };
      if property_set_streams
        .iter()
        .any(|node: &LocatedPropertySetStream| node.kind == kind)
      {
        return Err(Error::invalid(
          0,
          format!("duplicate root Office property-set stream for {kind:?}"),
        ));
      }
      let data = match PropertySetStream::from_bytes_with_limits(&entry.data, options.limits) {
        Ok(value) => OfficePropertySetData::Parsed(value),
        Err(error) if options.is_strict() => {
          return Err(Error::invalid(
            error.offset().unwrap_or(0),
            format!(
              "{} is not a conforming MS-OLEPS PropertySetStream: {error}",
              entry.path.display()
            ),
          ));
        }
        Err(error) => {
          diagnostics.push(ParseDiagnostic::warning(
            ParseDiagnosticCode::InvalidStreamPreserved,
            BinaryFormat::PropertySet,
            entry.path.to_str(),
            error.offset(),
            "PropertySetStream",
            SpecificationReference {
              document: "MS-OLEPS",
              section: "2.21",
            },
            format!("preserved an invalid root property-set stream: {error}"),
          ));
          OfficePropertySetData::Compatibility {
            bytes: entry.data.to_vec(),
            reason: error.to_string(),
          }
        }
      };
      property_set_streams.push(LocatedPropertySetStream {
        path: entry.path.clone(),
        kind,
        data,
      });
    }
    property_set_streams.sort_by_key(|node| match node.kind {
      OfficePropertySetKind::SummaryInformation => 0,
      OfficePropertySetKind::DocumentSummaryInformation => 1,
    });
    let vba_project = parse_host_vba_project(compound, options, host, &mut diagnostics)?;
    Ok(ParseOutcome::new(
      Self {
        property_set_streams: Arc::new(property_set_streams),
        vba_project: vba_project.map(Arc::new),
      },
      diagnostics,
    ))
  }

  pub fn property_set_streams(&self) -> &[LocatedPropertySetStream] {
    &self.property_set_streams
  }

  pub fn vba_project(&self) -> Option<&OfficeVbaProject> {
    self.vba_project.as_deref()
  }

  /// Replaces a host VBA module source and invalidates every derived cache
  /// and OLE-property-set signature in one shared-tree transaction.
  pub fn replace_vba_module_source(
    &mut self,
    stream_name: &str,
    source: &[u8],
  ) -> Result<OfficeVbaModuleMutation> {
    let mut candidate = self.clone();
    let project = candidate
      .vba_project
      .as_mut()
      .map(Arc::make_mut)
      .ok_or_else(|| Error::invalid(0, "Office file has no host-owned VBA project"))?;
    let OfficeVbaProject::Parsed(project) = project else {
      return Err(Error::invalid(
        0,
        "cannot edit an invalid VBA compatibility project",
      ));
    };
    let vba = project.replace_module_source(stream_name, source)?;
    let invalidated_oleps_signatures = candidate.remove_oleps_vba_signatures()?;
    *self = candidate;
    Ok(OfficeVbaModuleMutation {
      vba,
      invalidated_oleps_signatures,
      invalidated_host_signatures: 0,
    })
  }

  /// Edits one VBA Designer storage through the shared Office tree and
  /// invalidates signatures covering the project in the same transaction.
  pub fn edit_vba_designer_storage(
    &mut self,
    index: usize,
    edit: impl FnOnce(&mut ParentControlStorageModel) -> Result<()>,
  ) -> Result<OfficeFormsMutation> {
    let mut candidate = self.clone();
    let project = candidate
      .vba_project
      .as_mut()
      .map(Arc::make_mut)
      .ok_or_else(|| Error::invalid(0, "Office file has no host-owned VBA project"))?;
    let OfficeVbaProject::Parsed(project) = project else {
      return Err(Error::invalid(
        0,
        "cannot edit Forms in an invalid VBA compatibility project",
      ));
    };
    project.edit_designer_storage(index, edit)?;
    let invalidated_oleps_signatures = candidate.remove_oleps_vba_signatures()?;
    *self = candidate;
    Ok(OfficeFormsMutation {
      invalidated_oleps_signatures,
      invalidated_host_signatures: 0,
    })
  }

  pub fn property_set(&self, kind: OfficePropertySetKind) -> Option<&PropertySetStream> {
    self
      .property_set_streams
      .iter()
      .find(|node| node.kind == kind)
      .and_then(LocatedPropertySetStream::property_set)
  }

  /// Applies and validates one typed mutation before committing it.
  pub fn edit_property_set<T>(
    &mut self,
    kind: OfficePropertySetKind,
    edit: impl FnOnce(&mut PropertySetStream) -> Result<T>,
  ) -> Result<T> {
    let node = Arc::make_mut(&mut self.property_set_streams)
      .iter_mut()
      .find(|node| node.kind == kind)
      .ok_or_else(|| Error::invalid(0, format!("{kind:?} property-set stream is missing")))?;
    let OfficePropertySetData::Parsed(current) = &node.data else {
      return Err(Error::invalid(
        0,
        format!("cannot edit invalid {kind:?} compatibility bytes"),
      ));
    };
    let mut candidate = current.clone();
    let result = edit(&mut candidate)?;
    let bytes = candidate.to_bytes()?;
    let validated = PropertySetStream::from_bytes(&bytes)?;
    node.data = OfficePropertySetData::Parsed(validated);
    Ok(result)
  }

  pub fn replace_property_set(
    &mut self,
    kind: OfficePropertySetKind,
    value: PropertySetStream,
  ) -> Result<Option<PropertySetStream>> {
    let validated = PropertySetStream::from_bytes(&value.to_bytes()?)?;
    let property_set_streams = Arc::make_mut(&mut self.property_set_streams);
    if let Some(node) = property_set_streams
      .iter_mut()
      .find(|node| node.kind == kind)
    {
      let previous = std::mem::replace(&mut node.data, OfficePropertySetData::Parsed(validated));
      return match previous {
        OfficePropertySetData::Parsed(previous) => Ok(Some(previous)),
        OfficePropertySetData::Compatibility { .. } => Ok(None),
      };
    }
    property_set_streams.push(LocatedPropertySetStream {
      path: PathBuf::from(kind.canonical_path()),
      kind,
      data: OfficePropertySetData::Parsed(validated),
    });
    Ok(None)
  }

  pub fn remove_property_set(
    &mut self,
    kind: OfficePropertySetKind,
  ) -> Option<LocatedPropertySetStream> {
    let index = self
      .property_set_streams
      .iter()
      .position(|node| node.kind == kind)?;
    Some(Arc::make_mut(&mut self.property_set_streams).remove(index))
  }

  /// Replaces all root OLEPS streams from this tree as one transaction.
  pub fn write_to_compound_file(
    &self,
    compound: &mut CompoundFile,
    options: SaveOptions,
  ) -> Result<()> {
    self.validate()?;
    let mut candidate = compound.clone();
    let managed_paths = candidate
      .entries()
      .iter()
      .filter(|entry| {
        entry.kind == EntryKind::Stream
          && entry.path.parent() == Some(Path::new("/"))
          && OfficePropertySetKind::from_stream_name(&entry.name).is_some()
      })
      .filter_map(|entry| {
        OfficePropertySetKind::from_stream_name(&entry.name).map(|kind| (entry.path.clone(), kind))
      })
      .collect::<Vec<_>>();
    for (path, source_kind) in managed_paths {
      if !self
        .property_set_streams
        .iter()
        .any(|node| node.kind == source_kind)
      {
        candidate.remove_stream(path)?;
      }
    }
    for node in self.property_set_streams.iter() {
      let bytes = match &node.data {
        OfficePropertySetData::Parsed(value) => value.to_bytes()?,
        OfficePropertySetData::Compatibility { .. } if !options.preserves_compatibility() => {
          return Err(Error::invalid(
            0,
            format!(
              "strict save rejects invalid property-set stream {}",
              node.path.display()
            ),
          ));
        }
        OfficePropertySetData::Compatibility { bytes, .. } => bytes.clone(),
      };
      candidate.upsert_stream(&node.path, bytes)?;
    }
    match self.vba_project.as_deref() {
      Some(OfficeVbaProject::Parsed(project)) => {
        project.write_if_modified(&mut candidate)?;
      }
      Some(OfficeVbaProject::Compatibility { .. }) if !options.preserves_compatibility() => {
        return Err(Error::invalid(
          0,
          "strict save rejects an invalid host VBA project",
        ));
      }
      Some(OfficeVbaProject::Compatibility { .. }) | None => {}
    }
    *compound = candidate;
    Ok(())
  }

  fn validate(&self) -> Result<()> {
    for (index, node) in self.property_set_streams.iter().enumerate() {
      if node.path.parent() != Some(Path::new("/"))
        || OfficePropertySetKind::from_stream_name(
          node
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(""),
        ) != Some(node.kind)
      {
        return Err(Error::invalid(
          0,
          format!(
            "invalid CFB identity for {:?}: {}",
            node.kind,
            node.path.display()
          ),
        ));
      }
      if self.property_set_streams[..index]
        .iter()
        .any(|previous| previous.kind == node.kind)
      {
        return Err(Error::invalid(0, "duplicate Office property-set kind"));
      }
    }
    Ok(())
  }

  fn remove_oleps_vba_signatures(&mut self) -> Result<usize> {
    let Some(node) = Arc::make_mut(&mut self.property_set_streams)
      .iter_mut()
      .find(|node| node.kind == OfficePropertySetKind::DocumentSummaryInformation)
    else {
      return Ok(0);
    };
    let OfficePropertySetData::Parsed(stream) = &node.data else {
      return Err(Error::invalid(
        0,
        "cannot invalidate a VBA signature in invalid DocumentSummaryInformation bytes",
      ));
    };
    let mut candidate = stream.clone();
    let mut removed = 0usize;
    for property_set in &mut candidate.property_sets {
      if property_set.format_identifier == DOCUMENT_SUMMARY_INFORMATION_FMTID {
        let before = property_set.properties.len();
        property_set
          .properties
          .retain(|property| property.identifier != VBA_DIGITAL_SIGNATURE_PROPERTY_ID);
        removed += before - property_set.properties.len();
      }
    }
    let validated = PropertySetStream::from_bytes(&candidate.to_bytes()?)?;
    node.data = OfficePropertySetData::Parsed(validated);
    Ok(removed)
  }

  pub(crate) fn invalidate_oleps_vba_signatures(&mut self) -> Result<usize> {
    self.remove_oleps_vba_signatures()
  }
}

fn parse_host_vba_project(
  compound: &CompoundFile,
  options: ParseOptions,
  host: Option<OfficeHostKind>,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<Option<OfficeVbaProject>> {
  let expected_root_name = match host {
    Some(OfficeHostKind::Doc) => DOC_VBA_PROJECT_STORAGE_NAME,
    Some(OfficeHostKind::Xls) => XLS_VBA_PROJECT_STORAGE_NAME,
    Some(OfficeHostKind::Ppt) | None => return Ok(None),
  };
  let Some(project_root) = compound.entries().iter().find(|entry| {
    entry.kind == EntryKind::Storage
      && entry.path.parent() == Some(Path::new("/"))
      && entry.name.eq_ignore_ascii_case(expected_root_name)
  }) else {
    return Ok(None);
  };
  let vba_storage = compound.entries().iter().find(|entry| {
    entry.kind == EntryKind::Storage
      && entry.path.parent() == Some(project_root.path.as_path())
      && entry.name.eq_ignore_ascii_case(VBA_STORAGE_NAME)
  });
  let parsed = vba_storage
    .ok_or_else(|| Error::invalid(0, "VBA project root has no VBA storage"))
    .and_then(|storage| {
      LocatedVbaProject::from_compound_file_at_with_limits(compound, &storage.path, options.limits)
    });
  match parsed {
    Ok(project) => Ok(Some(OfficeVbaProject::Parsed(Box::new(project)))),
    Err(error) if options.is_strict() => Err(Error::invalid(
      error.offset().unwrap_or(0),
      format!(
        "{} is not a conforming MS-OVBA Project Root Storage: {error}",
        project_root.path.display()
      ),
    )),
    Err(error) => {
      diagnostics.push(ParseDiagnostic::warning(
        ParseDiagnosticCode::InvalidStreamPreserved,
        BinaryFormat::Vba,
        project_root.path.to_str(),
        error.offset(),
        "Project Root Storage",
        SpecificationReference {
          document: "MS-OVBA",
          section: "2.2.1",
        },
        format!("preserved an invalid host VBA project: {error}"),
      ));
      Ok(Some(OfficeVbaProject::Compatibility {
        project_root_path: project_root.path.clone(),
        vba_storage_path: vba_storage.map(|storage| storage.path.clone()),
        reason: error.to_string(),
      }))
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    cfb::Version,
    property_set::{Property, PropertySet},
  };

  fn valid_property_set() -> PropertySetStream {
    PropertySetStream {
      version: 0,
      system_identifier: 0x0002_0005,
      clsid: [0; 16],
      property_sets: vec![PropertySet {
        format_identifier: [0x2a; 16],
        properties: vec![Property {
          identifier: 1,
          offset: 16,
          raw: vec![0x02, 0, 0, 0, 0xe4, 0x04, 0, 0],
        }],
        prefix_padding: Vec::new(),
      }],
      trailing_padding: vec![0; 4],
    }
  }

  #[test]
  fn root_property_set_is_located_edited_and_reopened() {
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound
      .create_stream(
        SUMMARY_INFORMATION_STREAM,
        valid_property_set().to_bytes().unwrap(),
      )
      .unwrap();
    let mut shared =
      OfficeSharedContent::from_compound_file_with_options(&compound, ParseOptions::default())
        .unwrap()
        .value;
    assert_eq!(
      shared.property_set_streams()[0].path(),
      Path::new(SUMMARY_INFORMATION_STREAM)
    );
    let original = shared.clone();
    assert!(Arc::ptr_eq(
      &shared.property_set_streams,
      &original.property_set_streams,
    ));
    shared
      .edit_property_set(OfficePropertySetKind::SummaryInformation, |stream| {
        stream.system_identifier = 7;
        Ok(())
      })
      .unwrap();
    assert!(!Arc::ptr_eq(
      &shared.property_set_streams,
      &original.property_set_streams,
    ));
    assert_ne!(
      original
        .property_set(OfficePropertySetKind::SummaryInformation)
        .unwrap()
        .system_identifier,
      7,
    );
    shared
      .write_to_compound_file(&mut compound, SaveOptions::default())
      .unwrap();
    let reopened =
      OfficeSharedContent::from_compound_file_with_options(&compound, ParseOptions::default())
        .unwrap();
    assert_eq!(
      reopened
        .value
        .property_set(OfficePropertySetKind::SummaryInformation)
        .unwrap()
        .system_identifier,
      7
    );
  }

  #[test]
  fn compatible_invalid_stream_requires_preserving_save() {
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound
      .create_stream(SUMMARY_INFORMATION_STREAM, vec![3, 4])
      .unwrap();
    assert!(
      OfficeSharedContent::from_compound_file_with_options(&compound, ParseOptions::default())
        .is_err()
    );
    let outcome = OfficeSharedContent::from_compound_file_with_options(
      &compound,
      ParseOptions::compatible(crate::limits::Limits::default()),
    )
    .unwrap();
    assert_eq!(outcome.diagnostics.len(), 1);
    let mut output = CompoundFile::new(Version::V3).unwrap();
    assert!(
      outcome
        .value
        .write_to_compound_file(&mut output, SaveOptions::default())
        .is_err()
    );
    outcome
      .value
      .write_to_compound_file(&mut output, SaveOptions::preserving_compatibility())
      .unwrap();
    assert_eq!(
      output.stream(SUMMARY_INFORMATION_STREAM),
      Some([3, 4].as_slice())
    );
  }

  #[test]
  fn nested_property_set_is_not_owned_by_host_root() {
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound.create_storage("/Object").unwrap();
    compound
      .create_stream(
        "/Object/\u{5}SummaryInformation",
        valid_property_set().to_bytes().unwrap(),
      )
      .unwrap();
    let shared =
      OfficeSharedContent::from_compound_file_with_options(&compound, ParseOptions::default())
        .unwrap();
    assert!(shared.value.property_set_streams().is_empty());
  }
}
