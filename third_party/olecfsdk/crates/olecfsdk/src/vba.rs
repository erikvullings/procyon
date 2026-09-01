//! MS-OVBA structures shared by VBA projects embedded in Office files.

use std::path::PathBuf;

use crate::{
  Error, Result,
  cfb::{CompoundFile, Entry},
  common::CodePage,
  forms::{LocatedParentControlStorage, ParentControlStorageModel},
  limits::Limits,
};

pub mod cache;
pub mod compression;
pub mod directory;
pub mod module;
pub mod project;

use cache::{SrpStream, SrpStreamName, VbaProjectStream};
use compression::CompressedContainer;
use directory::{DirStream, ModuleDescriptor};
use module::ModuleStream;
use project::{ProjectLkStream, ProjectStream, ProjectWmStream};

/// Fixed name of the MS-OVBA storage inside a host project storage.
pub const VBA_STORAGE_NAME: &str = "VBA";
/// Fixed name of the compressed VBA directory stream.
pub const VBA_DIRECTORY_STREAM_NAME: &str = "dir";
/// Fixed name of the version-dependent VBA project cache stream.
pub const VBA_PROJECT_CACHE_STREAM_NAME: &str = "_VBA_PROJECT";
/// Fixed name of the textual VBA project stream.
pub const VBA_PROJECT_STREAM_NAME: &str = "PROJECT";
/// Fixed name of the VBA module-name map stream.
pub const VBA_PROJECT_WM_STREAM_NAME: &str = "PROJECTwm";
/// Fixed name of the optional VBA project licensing stream.
pub const VBA_PROJECT_LK_STREAM_NAME: &str = "PROJECTlk";
/// Fixed root storage name used by MS-DOC VBA projects.
pub const DOC_VBA_PROJECT_STORAGE_NAME: &str = "Macros";
/// Fixed root storage name used by MS-XLS VBA projects.
pub const XLS_VBA_PROJECT_STORAGE_NAME: &str = "_VBA_PROJECT_CUR";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VbaProject {
  pub vba_storage_path: PathBuf,
  pub project: Option<ProjectStream>,
  pub project_wm: Option<ProjectWmStream>,
  pub project_lk: Option<ProjectLkStream>,
  pub directory_container: CompressedContainer,
  pub directory: DirStream,
  pub cache: VbaProjectStream,
  pub srp_streams: Vec<SrpStream>,
  pub modules: Vec<VbaModule>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VbaModule {
  pub descriptor: ModuleDescriptor,
  pub stream_path: PathBuf,
  pub stream: ModuleStream,
}

/// Storage-neutral MS-OVBA project payload.
///
/// Unlike [`VbaProject`], this model contains no CFB paths. It can therefore
/// become part of a host file's owned Rust tree without mixing logical VBA
/// state with the adapter used to locate its streams.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VbaProjectModel {
  pub project: Option<ProjectStream>,
  pub project_wm: Option<ProjectWmStream>,
  pub project_lk: Option<ProjectLkStream>,
  pub directory_container: CompressedContainer,
  pub directory: DirStream,
  pub cache: VbaProjectStream,
  pub srp_caches: Vec<VbaSrpCache>,
  pub modules: Vec<VbaModuleModel>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VbaModuleModel {
  pub descriptor: ModuleDescriptor,
  pub stream: ModuleStream,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VbaSrpCache {
  pub name: SrpStreamName,
  pub implementation_specific_cache: Vec<u8>,
}

/// CFB identities paired with one storage-neutral VBA project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VbaProjectCfbIdentity {
  pub storage_path: PathBuf,
  pub directory_stream_path: PathBuf,
  pub project_cache_stream_path: PathBuf,
  pub srp_stream_paths: Vec<PathBuf>,
  pub module_stream_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedVbaProject {
  identity: VbaProjectCfbIdentity,
  model: VbaProjectModel,
  source_model: VbaProjectModel,
  designer_storages: Vec<LocatedParentControlStorage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VbaModuleSourceMutation {
  pub previous_source: Vec<u8>,
  pub discarded_project_cache_bytes: usize,
  pub discarded_module_cache_bytes: usize,
  pub discarded_srp_streams: usize,
  pub discarded_srp_bytes: usize,
}

impl LocatedVbaProject {
  pub fn from_compound_file(compound_file: &CompoundFile) -> Result<Self> {
    Self::from_compound_file_with_limits(compound_file, Limits::default())
  }

  pub fn from_compound_file_with_limits(
    compound_file: &CompoundFile,
    limits: Limits,
  ) -> Result<Self> {
    let project = VbaProject::from_compound_file_with_limits(compound_file, limits)?;
    Self::from_parsed_in_compound(compound_file, project)
  }

  pub fn from_compound_file_at(
    compound_file: &CompoundFile,
    vba_storage_path: impl AsRef<std::path::Path>,
  ) -> Result<Self> {
    Self::from_compound_file_at_with_limits(compound_file, vba_storage_path, Limits::default())
  }

  pub fn from_compound_file_at_with_limits(
    compound_file: &CompoundFile,
    vba_storage_path: impl AsRef<std::path::Path>,
    limits: Limits,
  ) -> Result<Self> {
    let project =
      VbaProject::from_compound_file_at_with_limits(compound_file, vba_storage_path, limits)?;
    Self::from_parsed_in_compound(compound_file, project)
  }

  fn from_parsed_in_compound(compound_file: &CompoundFile, parsed: VbaProject) -> Result<Self> {
    let mut project: Self = parsed.into();
    let project_root = project
      .identity
      .storage_path
      .parent()
      .unwrap_or(std::path::Path::new("/"));
    project.designer_storages =
      LocatedParentControlStorage::discover_below(compound_file, project_root)?;
    Ok(project)
  }

  pub const fn model(&self) -> &VbaProjectModel {
    &self.model
  }

  pub const fn identity(&self) -> &VbaProjectCfbIdentity {
    &self.identity
  }

  pub fn is_modified(&self) -> bool {
    self.model != self.source_model
      || self
        .designer_storages
        .iter()
        .any(LocatedParentControlStorage::is_modified)
  }

  pub fn designer_storages(&self) -> &[LocatedParentControlStorage] {
    &self.designer_storages
  }

  pub fn edit_designer_storage<T>(
    &mut self,
    index: usize,
    edit: impl FnOnce(&mut ParentControlStorageModel) -> Result<T>,
  ) -> Result<T> {
    let storage = self.designer_storages.get_mut(index).ok_or_else(|| {
      Error::invalid(
        0,
        format!("VBA project has no Forms designer storage at index {index}"),
      )
    })?;
    storage.edit(edit)
  }

  /// Replaces one module source and normalizes every version-dependent
  /// cache in a single model transaction.
  pub fn replace_module_source(
    &mut self,
    stream_name: &str,
    source: &[u8],
  ) -> Result<VbaModuleSourceMutation> {
    let mut candidate = self.model.clone();
    let previous_source = candidate.replace_module_source(stream_name, source)?;
    let discarded_project_cache_bytes = candidate.cache.performance_cache.len();
    let discarded_module_cache_bytes = candidate
      .modules
      .iter()
      .map(|module| module.stream.performance_cache.len())
      .sum();
    let discarded_srp_streams = candidate.srp_caches.len();
    let discarded_srp_bytes = candidate
      .srp_caches
      .iter()
      .map(|stream| stream.implementation_specific_cache.len())
      .sum();
    candidate.prepare_interoperable()?;
    self.validate_model_identities(&candidate)?;
    self.model = candidate;
    Ok(VbaModuleSourceMutation {
      previous_source,
      discarded_project_cache_bytes,
      discarded_module_cache_bytes,
      discarded_srp_streams,
      discarded_srp_bytes,
    })
  }

  /// Emits the interoperable MS-OVBA representation as one CFB transaction.
  pub fn write_interoperable_to_compound_file(
    &self,
    compound_file: &mut CompoundFile,
  ) -> Result<()> {
    let mut normalized = self.clone();
    normalized.model.prepare_interoperable()?;
    normalized.write_current_to_compound_file(compound_file)
  }

  /// Writes a previously validated current model only when it differs from
  /// the parse-time snapshot.
  pub(crate) fn write_if_modified(&self, compound_file: &mut CompoundFile) -> Result<()> {
    if !self.is_modified() {
      return Ok(());
    }
    self.write_current_to_compound_file(compound_file)
  }

  fn write_current_to_compound_file(&self, compound_file: &mut CompoundFile) -> Result<()> {
    self.validate_model_identities(&self.model)?;
    let mut updated = compound_file.clone();
    if self.model != self.source_model {
      let encoded_directory = self.model.directory_container.to_bytes()?;
      let encoded_cache = self.model.cache.to_bytes()?;
      let encoded_modules = self
        .model
        .modules
        .iter()
        .zip(&self.identity.module_stream_paths)
        .map(|(module, path)| Ok((path, module.stream.to_bytes()?)))
        .collect::<Result<Vec<_>>>()?;
      updated.overwrite_stream(&self.identity.directory_stream_path, encoded_directory)?;
      updated.overwrite_stream(&self.identity.project_cache_stream_path, encoded_cache)?;
      for (path, bytes) in encoded_modules {
        updated.overwrite_stream(path, bytes)?;
      }
      for path in &self.identity.srp_stream_paths {
        updated.remove_stream(path)?;
      }
    }
    for storage in &self.designer_storages {
      storage.write_if_modified(&mut updated)?;
    }
    *compound_file = updated;
    Ok(())
  }

  fn validate_model_identities(&self, model: &VbaProjectModel) -> Result<()> {
    if self.identity.module_stream_paths.len() != model.modules.len() {
      return Err(Error::invalid(
        0,
        "VBA module model and CFB identity counts differ",
      ));
    }
    if model.srp_caches.len() > self.identity.srp_stream_paths.len() {
      return Err(Error::invalid(
        0,
        "VBA model has an SRP cache without a stable CFB identity",
      ));
    }
    Ok(())
  }
}

impl VbaProjectModel {
  pub fn replace_module_source(&mut self, stream_name: &str, source: &[u8]) -> Result<Vec<u8>> {
    let code_page = CodePage(self.directory.code_page().unwrap_or(1252));
    let module = self
      .modules
      .iter_mut()
      .find(|module| {
        module
          .descriptor
          .stream_name_with_code_page(code_page)
          .is_ok_and(|value| value.eq_ignore_ascii_case(stream_name))
      })
      .ok_or_else(|| {
        Error::invalid(0, format!("VBA project has no module stream {stream_name}"))
      })?;
    module.stream.replace_source_bytes(source)
  }

  fn prepare_interoperable(&mut self) -> Result<()> {
    let offset_count = self.directory.set_module_offsets(0);
    if offset_count != self.modules.len() {
      return Err(Error::invalid(
        0,
        format!(
          "VBA dir has {offset_count} module offsets but {} parsed modules",
          self.modules.len()
        ),
      ));
    }
    self.directory_container = CompressedContainer::from_uncompressed(&self.directory.to_bytes()?);
    self.cache.version = VbaProjectStream::INTEROPERABLE_VERSION;
    self.cache.performance_cache.clear();
    for module in &mut self.modules {
      module.stream.performance_cache.clear();
    }
    self.srp_caches.clear();
    Ok(())
  }
}

impl From<VbaProject> for LocatedVbaProject {
  fn from(project: VbaProject) -> Self {
    let VbaProject {
      vba_storage_path,
      project,
      project_wm,
      project_lk,
      directory_container,
      directory,
      cache,
      srp_streams,
      modules,
    } = project;
    let srp_stream_paths = srp_streams
      .iter()
      .map(|stream| stream.path.clone())
      .collect();
    let srp_caches = srp_streams
      .into_iter()
      .map(|stream| VbaSrpCache {
        name: stream.name,
        implementation_specific_cache: stream.implementation_specific_cache,
      })
      .collect();
    let module_stream_paths = modules
      .iter()
      .map(|module| module.stream_path.clone())
      .collect();
    let modules = modules
      .into_iter()
      .map(|module| VbaModuleModel {
        descriptor: module.descriptor,
        stream: module.stream,
      })
      .collect();
    let model = VbaProjectModel {
      project,
      project_wm,
      project_lk,
      directory_container,
      directory,
      cache,
      srp_caches,
      modules,
    };
    Self {
      identity: VbaProjectCfbIdentity {
        directory_stream_path: vba_storage_path.join(VBA_DIRECTORY_STREAM_NAME),
        project_cache_stream_path: vba_storage_path.join(VBA_PROJECT_CACHE_STREAM_NAME),
        storage_path: vba_storage_path,
        srp_stream_paths,
        module_stream_paths,
      },
      source_model: model.clone(),
      model,
      designer_storages: Vec::new(),
    }
  }
}

impl VbaProject {
  pub fn is_present(compound_file: &CompoundFile) -> bool {
    compound_file
      .entries()
      .iter()
      .any(|entry| entry.is_storage() && entry.name.eq_ignore_ascii_case(VBA_STORAGE_NAME))
  }

  pub fn from_compound_file(compound_file: &CompoundFile) -> Result<Self> {
    Self::from_compound_file_with_limits(compound_file, Limits::default())
  }

  pub fn from_compound_file_with_limits(
    compound_file: &CompoundFile,
    limits: Limits,
  ) -> Result<Self> {
    let vba_storage = compound_file
      .entries()
      .iter()
      .find(|entry| {
        entry.is_storage()
          && entry.name.eq_ignore_ascii_case(VBA_STORAGE_NAME)
          && child_stream(compound_file, &entry.path, VBA_DIRECTORY_STREAM_NAME).is_some()
          && child_stream(compound_file, &entry.path, VBA_PROJECT_CACHE_STREAM_NAME).is_some()
      })
      .ok_or_else(|| Error::invalid(0, "compound file has no VBA storage"))?;
    Self::from_compound_file_at_with_limits(compound_file, &vba_storage.path, limits)
  }

  pub fn from_compound_file_at(
    compound_file: &CompoundFile,
    vba_storage_path: impl AsRef<std::path::Path>,
  ) -> Result<Self> {
    Self::from_compound_file_at_with_limits(compound_file, vba_storage_path, Limits::default())
  }

  pub fn from_compound_file_at_with_limits(
    compound_file: &CompoundFile,
    vba_storage_path: impl AsRef<std::path::Path>,
    limits: Limits,
  ) -> Result<Self> {
    let vba_storage_path = vba_storage_path.as_ref();
    let vba_storage = compound_file
      .entries()
      .iter()
      .find(|entry| entry.is_storage() && entry.path == vba_storage_path)
      .ok_or_else(|| Error::invalid(0, "compound file has no VBA storage at path"))?;
    let vba_storage_path = vba_storage.path.clone();
    let directory_entry = child_stream(compound_file, &vba_storage_path, VBA_DIRECTORY_STREAM_NAME)
      .ok_or_else(|| Error::invalid(0, "VBA storage has no dir stream"))?;
    let directory_container =
      CompressedContainer::from_bytes_with_limits(&directory_entry.data, limits)?;
    let directory_bytes = directory_container.decompress()?;
    let directory = DirStream::from_bytes_with_limits(&directory_bytes, limits)?;
    let cache_entry = child_stream(
      compound_file,
      &vba_storage_path,
      VBA_PROJECT_CACHE_STREAM_NAME,
    )
    .ok_or_else(|| Error::invalid(0, "VBA storage has no _VBA_PROJECT stream"))?;
    let cache = VbaProjectStream::from_bytes_with_limits(&cache_entry.data, limits)?;
    let mut srp_streams = Vec::new();
    for entry in compound_file
      .entries()
      .iter()
      .filter(|entry| entry.is_stream() && entry.path.parent() == Some(vba_storage_path.as_path()))
    {
      if let Some(name) = SrpStreamName::parse(&entry.name)? {
        if entry.data.len() as u64 > limits.max_stream_size {
          return Err(Error::Limit(format!(
            "VBA SRP stream length {} exceeds {}",
            entry.data.len(),
            limits.max_stream_size
          )));
        }
        srp_streams.push(SrpStream {
          path: entry.path.clone(),
          name,
          implementation_specific_cache: entry.data.to_vec(),
        });
      }
    }

    let project_storage_path = vba_storage_path
      .parent()
      .unwrap_or(std::path::Path::new("/"));
    let project = child_stream(compound_file, project_storage_path, VBA_PROJECT_STREAM_NAME)
      .map(|entry| ProjectStream::from_bytes_with_limits(&entry.data, limits))
      .transpose()?;
    let project_wm = child_stream(
      compound_file,
      project_storage_path,
      VBA_PROJECT_WM_STREAM_NAME,
    )
    .map(|entry| ProjectWmStream::from_bytes(&entry.data))
    .transpose()?;
    let project_lk = child_stream(
      compound_file,
      project_storage_path,
      VBA_PROJECT_LK_STREAM_NAME,
    )
    .map(|entry| ProjectLkStream::from_bytes_with_limits(&entry.data, limits))
    .transpose()?;
    let code_page = CodePage(directory.code_page().unwrap_or(1252));
    let mut modules = Vec::new();
    for descriptor in directory.modules() {
      let stream_name = descriptor.stream_name_with_code_page(code_page)?;
      let entry = child_stream(compound_file, &vba_storage_path, &stream_name)
        .ok_or_else(|| Error::invalid(0, format!("missing VBA module stream {stream_name}")))?;
      let text_offset = descriptor
        .text_offset
        .ok_or_else(|| Error::invalid(0, format!("VBA module {stream_name} has no text offset")))?;
      modules.push(VbaModule {
        descriptor,
        stream_path: entry.path.clone(),
        stream: ModuleStream::from_bytes_with_limits(&entry.data, text_offset, limits)?,
      });
    }
    Ok(Self {
      vba_storage_path,
      project,
      project_wm,
      project_lk,
      directory_container,
      directory,
      cache,
      srp_streams,
      modules,
    })
  }

  /// Writes the MS-OVBA interoperable representation atomically.
  ///
  /// Version-dependent caches are discarded, every module source starts at
  /// offset zero, the corresponding dir records are updated and recompressed,
  /// and all SRP streams are removed.
  pub fn write_interoperable_to_compound_file(
    &self,
    compound_file: &mut CompoundFile,
  ) -> Result<()> {
    LocatedVbaProject::from(self.clone()).write_interoperable_to_compound_file(compound_file)
  }

  pub fn replace_module_source(&mut self, stream_name: &str, source: &[u8]) -> Result<Vec<u8>> {
    let module = self
      .modules
      .iter_mut()
      .find(|module| {
        module
          .stream_path
          .file_name()
          .and_then(|value| value.to_str())
          .is_some_and(|value| value.eq_ignore_ascii_case(stream_name))
      })
      .ok_or_else(|| {
        Error::invalid(0, format!("VBA project has no module stream {stream_name}"))
      })?;
    module.stream.replace_source_bytes(source)
  }
}

fn child_stream<'a>(
  compound_file: &'a CompoundFile,
  parent: &std::path::Path,
  name: &str,
) -> Option<&'a Entry> {
  compound_file.entries().iter().find(|entry| {
    entry.is_stream()
      && entry.path.parent() == Some(parent)
      && entry.name.eq_ignore_ascii_case(name)
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    cfb::Version,
    vba::directory::{
      DirRecord, MarkerRecordKind, MbcsStringRecordKind, U16RecordKind, U32RecordKind,
    },
    vba::project::LicenseInfo,
  };

  #[test]
  fn interoperable_write_rebuilds_offsets_and_discards_all_caches() {
    let directory = DirStream {
      records: vec![
        DirRecord::U16 {
          kind: U16RecordKind::ProjectCodePage,
          value: 1252,
        },
        DirRecord::MbcsString {
          kind: MbcsStringRecordKind::ModuleName,
          bytes: b"Module1".to_vec(),
        },
        DirRecord::MbcsString {
          kind: MbcsStringRecordKind::ModuleStreamName,
          bytes: b"Module1".to_vec(),
        },
        DirRecord::U32 {
          kind: U32RecordKind::ModuleOffset,
          value: 2,
        },
        DirRecord::Marker {
          kind: MarkerRecordKind::ModuleTerminator,
          reserved: 0,
        },
        DirRecord::Terminator,
      ],
      reserved: 0,
    };
    let encoded_directory = CompressedContainer::from_uncompressed(&directory.to_bytes().unwrap())
      .to_bytes()
      .unwrap();
    let cache = VbaProjectStream {
      reserved1: VbaProjectStream::RESERVED1,
      version: 0x1234,
      reserved2: 0,
      reserved3: 7,
      performance_cache: vec![9, 8, 7],
    };
    let mut module = vec![0xaa, 0xbb];
    module.extend_from_slice(
      &CompressedContainer::from_uncompressed(b"Sub Main()\r\nEnd Sub")
        .to_bytes()
        .unwrap(),
    );

    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound.create_storage("/VBA").unwrap();
    compound
      .create_stream("/VBA/dir", encoded_directory)
      .unwrap();
    compound
      .create_stream("/VBA/_VBA_PROJECT", cache.to_bytes().unwrap())
      .unwrap();
    compound.create_stream("/VBA/Module1", module).unwrap();
    compound
      .create_stream("/VBA/__SRP_A1", vec![1, 2, 3])
      .unwrap();
    let project_lk = ProjectLkStream {
      version: ProjectLkStream::VERSION,
      licenses: vec![LicenseInfo {
        class_id: [0x44; 16],
        license_key: b"opaque-license".to_vec(),
        license_required: 1,
      }],
    };
    compound
      .create_stream("/PROJECTlk", project_lk.to_bytes().unwrap())
      .unwrap();

    let project = VbaProject::from_compound_file(&compound).unwrap();
    let mut located = LocatedVbaProject::from(project);
    assert_eq!(located.identity.storage_path, PathBuf::from("/VBA"));
    assert_eq!(located.model.srp_caches.len(), 1);
    assert_eq!(located.model.project_lk, Some(project_lk.clone()));
    assert_eq!(
      located
        .model
        .replace_module_source("module1", b"Sub Changed()\r\nEnd Sub")
        .unwrap(),
      b"Sub Main()\r\nEnd Sub"
    );
    assert!(located.model.replace_module_source("missing", b"").is_err());
    located
      .write_interoperable_to_compound_file(&mut compound)
      .unwrap();
    assert!(compound.stream("/VBA/__SRP_A1").is_none());

    let reopened = VbaProject::from_compound_file(&compound).unwrap();
    assert_eq!(
      reopened.cache.version,
      VbaProjectStream::INTEROPERABLE_VERSION
    );
    assert!(reopened.cache.performance_cache.is_empty());
    assert!(reopened.srp_streams.is_empty());
    assert_eq!(reopened.project_lk, Some(project_lk));
    assert_eq!(reopened.directory.module_offsets().collect::<Vec<_>>(), [0]);
    assert!(reopened.modules[0].stream.performance_cache.is_empty());
    assert_eq!(
      reopened.modules[0].stream.source_bytes().unwrap(),
      b"Sub Changed()\r\nEnd Sub"
    );
  }
}
