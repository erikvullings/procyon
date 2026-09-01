//! Typed file root for the PowerPoint binary format.
//!
//! [`PptFile`] owns the recursive MS-PPT record tree and keeps its source CFB
//! private as an immutable preservation snapshot. Serialization rebuilds the
//! managed PowerPoint Document, Current User, and Pictures streams from the
//! current tree. Strict entry points and saves are the default; compatible
//! parsing returns structured diagnostics and requires an explicit preserving
//! save policy for compatibility nodes. The independent [`PptHistoryStrategy`]
//! controls physical incremental history and is not a compatibility switch.

use std::{borrow::Cow, io::Write, path::Path, sync::Arc};

use crate::{
  Error, Result,
  cfb::{CfbStreamOverride, CfbStreamWriter, CompoundFile},
  forms::ParentControlStorageModel,
  io::BinaryFormat,
  limits::Limits,
  office_art::{
    OfficeArtBStoreDelayFileBlockLayout, OfficeArtBStoreDelayLayout, OfficeArtDrawingGraph,
  },
  parse::{
    ParseDiagnostic, ParseDiagnosticCode, ParseOptions, ParseOutcome, SpecificationReference,
    compound_from_bytes, compound_from_path, compound_from_vec, compound_outcome,
  },
  save::SaveOptions,
  shared_content::{
    OfficeFormsMutation, OfficeHostKind, OfficeSharedContent, OfficeVbaModuleMutation,
  },
};

use super::{
  BinaryTagData, CURRENT_USER_STREAM_PATH, CurrentUserData, CurrentUserStream, ExternalStorageAtom,
  PICTURES_STREAM_PATH, POWERPOINT_DOCUMENT_STREAM_PATH, PersistObjectDirectory, PicturesStream,
  PowerPointDocument, PptLiveImageStore, PptLivePresentation, PptLiveTextBodyMut, PptRecord,
  PptRecordData, PptRecordSequence, PptSlideId,
};

/// Complete typed root for a PowerPoint binary file.
///
/// The document stream remains a recursive [`super::PptRecordSequence`]; no
/// content is flattened into text, slide summaries, or image shortcuts. The
/// source CFB image is private so it cannot compete with these typed streams
/// as a write authority.
///
/// See the runnable `edit_ppt` example for open, live-presentation traversal,
/// slide-text edit, save, and strict reopen.
#[derive(Clone, Debug)]
pub struct PptFile {
  compound_file: CompoundFile,
  pub shared: OfficeSharedContent,
  /// Clone-shared recursive record tree. Call [`Arc::make_mut`] before
  /// direct field edits; transactional SDK methods detach it automatically.
  pub document: Arc<PowerPointDocument>,
  /// Clone-shared Current User stream, detached with the document tree when
  /// relayout changes the active edit offset.
  pub current_user: Arc<CurrentUserStream>,
  /// Clone-shared Pictures stream, detached automatically by SDK mutations.
  pub pictures: Option<Arc<PicturesStream>>,
  layout_baseline: PptManagedLayoutBaseline,
}

#[derive(Clone, Debug)]
struct PptManagedLayoutBaseline {
  document: Arc<PowerPointDocument>,
  current_user: Arc<CurrentUserStream>,
  pictures: Option<Arc<PicturesStream>>,
  strict: bool,
}

impl PptManagedLayoutBaseline {
  fn new(
    document: &Arc<PowerPointDocument>,
    current_user: &Arc<CurrentUserStream>,
    pictures: &Option<Arc<PicturesStream>>,
    strict: bool,
  ) -> Self {
    Self {
      document: Arc::clone(document),
      current_user: Arc::clone(current_user),
      pictures: pictures.as_ref().map(Arc::clone),
      strict,
    }
  }

  fn matches(
    &self,
    document: &Arc<PowerPointDocument>,
    current_user: &Arc<CurrentUserStream>,
    pictures: &Option<Arc<PicturesStream>>,
  ) -> bool {
    Arc::ptr_eq(&self.document, document)
      && Arc::ptr_eq(&self.current_user, current_user)
      && match (&self.pictures, pictures) {
        (Some(baseline), Some(current)) => Arc::ptr_eq(baseline, current),
        (None, None) => true,
        _ => false,
      }
  }
}

impl PartialEq for PptFile {
  fn eq(&self, other: &Self) -> bool {
    self.compound_file == other.compound_file
      && self.shared == other.shared
      && self.document == other.document
      && self.current_user == other.current_user
      && self.pictures == other.pictures
  }
}

struct PptManagedStreams {
  document: Vec<u8>,
  current_user: Vec<u8>,
  pictures: Option<Vec<u8>>,
}

struct PptDocumentStreamWriter<'a>(&'a PowerPointDocument);

struct PptPicturesStreamWriter<'a>(&'a PicturesStream);

impl CfbStreamWriter for PptDocumentStreamWriter<'_> {
  fn write_to(&self, writer: &mut dyn Write) -> Result<()> {
    self.0.write_to(writer)
  }
}

impl CfbStreamWriter for PptPicturesStreamWriter<'_> {
  fn write_to(&self, writer: &mut dyn Write) -> Result<()> {
    self.0.write_to(writer)
  }
}

impl PptManagedStreams {
  fn matches_source(&self, source: &CompoundFile) -> bool {
    source.stream(POWERPOINT_DOCUMENT_STREAM_PATH) == Some(self.document.as_slice())
      && source.stream(CURRENT_USER_STREAM_PATH) == Some(self.current_user.as_slice())
      && match (
        source.stream(PICTURES_STREAM_PATH),
        self.pictures.as_deref(),
      ) {
        (Some(source), Some(current)) => source == current,
        (None, None) => true,
        _ => false,
      }
  }
}

/// Result of appending one MS-PPT incremental-save checkpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PptAppendUserEditReport {
  pub previous_user_edit_offset: u32,
  pub user_edit_offset: u32,
  pub persist_directory_offset: u32,
  pub appended_persist_records: usize,
  pub persist_ids: Vec<u32>,
}

/// Explicit physical-history policy for a strict PPT save.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PptHistoryStrategy {
  /// Rewrite positions while retaining the existing physical record history.
  #[default]
  PreservePhysicalHistory,
  /// Append a new full current-directory checkpoint and retain preceding checkpoints.
  AppendUserEdit,
  /// Discard dead/history records and emit one normalized current edit.
  RebuildCurrentLiveState,
}

impl PptFile {
  fn sync_layout_baseline(&mut self, strict: bool) {
    self.layout_baseline =
      PptManagedLayoutBaseline::new(&self.document, &self.current_user, &self.pictures, strict);
  }

  fn has_current_managed_layout(&self) -> bool {
    self
      .layout_baseline
      .matches(&self.document, &self.current_user, &self.pictures)
  }

  fn write_ready_layout(&self, options: SaveOptions) -> Result<Cow<'_, Self>> {
    if (options.preserves_compatibility() || self.layout_baseline.strict)
      && self.has_current_managed_layout()
    {
      return Ok(Cow::Borrowed(self));
    }
    let mut rebuilt = self.clone();
    if matches!(rebuilt.current_user.data, CurrentUserData::Parsed(_)) {
      rebuilt.relayout_in_place_with_policy(options.preserves_compatibility())?;
    }
    Ok(Cow::Owned(rebuilt))
  }

  /// Returns the immutable parse-time CFB backing used to preserve unknown
  /// and externally-owned entries.
  ///
  /// Managed stream bytes in this snapshot do not reflect subsequent typed
  /// edits. Use [`Self::to_compound_file`] to inspect the current serialized
  /// file.
  pub fn source_compound_file(&self) -> &CompoundFile {
    &self.compound_file
  }

  /// Opens a path in strict mode and returns its owned MS-PPT tree.
  pub fn open(path: impl AsRef<Path>) -> Result<Self> {
    Ok(Self::open_with_options(path, ParseOptions::default())?.into_value())
  }

  /// Opens a path in compatible mode, returning every structured diagnostic
  /// alongside the owned tree.
  pub fn open_compatible(path: impl AsRef<Path>) -> Result<ParseOutcome<Self>> {
    Self::open_with_options(path, ParseOptions::compatible(Limits::default()))
  }

  pub fn open_with_options(
    path: impl AsRef<Path>,
    options: ParseOptions,
  ) -> Result<ParseOutcome<Self>> {
    let compound = compound_from_path(path.as_ref(), options, BinaryFormat::Ppt)?;
    Self::from_compound_outcome(compound, options)
  }

  /// Parses a complete CFB byte slice in strict mode.
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Ok(Self::from_bytes_with_options(bytes, ParseOptions::default())?.into_value())
  }

  /// Parses a complete CFB byte slice in compatible mode.
  pub fn from_bytes_compatible(bytes: &[u8]) -> Result<ParseOutcome<Self>> {
    Self::from_bytes_with_options(bytes, ParseOptions::compatible(Limits::default()))
  }

  pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self> {
    Ok(Self::from_bytes_with_options(bytes, ParseOptions::strict(limits))?.into_value())
  }

  pub fn from_bytes_with_options(
    bytes: &[u8],
    options: ParseOptions,
  ) -> Result<ParseOutcome<Self>> {
    let compound = compound_from_bytes(bytes, options, BinaryFormat::Ppt)?;
    Self::from_compound_outcome(compound, options)
  }

  /// Consumes a complete CFB image without copying its full archive buffer.
  pub fn from_vec(bytes: Vec<u8>) -> Result<Self> {
    Ok(Self::from_vec_with_options(bytes, ParseOptions::default())?.into_value())
  }

  pub fn from_vec_compatible(bytes: Vec<u8>) -> Result<ParseOutcome<Self>> {
    Self::from_vec_with_options(bytes, ParseOptions::compatible(Limits::default()))
  }

  pub fn from_vec_with_limits(bytes: Vec<u8>, limits: Limits) -> Result<Self> {
    Ok(Self::from_vec_with_options(bytes, ParseOptions::strict(limits))?.into_value())
  }

  pub fn from_vec_with_options(
    bytes: Vec<u8>,
    options: ParseOptions,
  ) -> Result<ParseOutcome<Self>> {
    let compound = compound_from_vec(bytes, options, BinaryFormat::Ppt)?;
    Self::from_compound_outcome(compound, options)
  }

  /// Consumes an owned CFB and parses its managed streams in strict mode.
  pub fn from_compound_file(compound_file: CompoundFile) -> Result<Self> {
    Ok(Self::from_compound_file_with_options(compound_file, ParseOptions::default())?.into_value())
  }

  pub fn from_compound_file_compatible(compound_file: CompoundFile) -> Result<ParseOutcome<Self>> {
    Self::from_compound_file_with_options(
      compound_file,
      ParseOptions::compatible(Limits::default()),
    )
  }

  pub fn from_compound_file_with_limits(
    compound_file: CompoundFile,
    limits: Limits,
  ) -> Result<Self> {
    Ok(
      Self::from_compound_file_with_options(compound_file, ParseOptions::strict(limits))?
        .into_value(),
    )
  }

  pub fn from_compound_file_with_options(
    compound_file: CompoundFile,
    options: ParseOptions,
  ) -> Result<ParseOutcome<Self>> {
    let compound = compound_outcome(compound_file, options, BinaryFormat::Ppt)?;
    Self::from_compound_outcome(compound, options)
  }

  /// Rebuilds PPT record lengths/positions and the incremental-save
  /// references owned by the Current User and PowerPoint Document streams.
  /// The update is transactional.
  pub fn relayout(&mut self) -> Result<()> {
    self.relayout_with_policy(false)
  }

  fn relayout_with_policy(&mut self, preserve_compatibility: bool) -> Result<()> {
    let mut rebuilt = self.clone();
    rebuilt.relayout_in_place_with_policy(preserve_compatibility)?;
    *self = rebuilt;
    Ok(())
  }

  fn relayout_in_place_with_policy(&mut self, preserve_compatibility: bool) -> Result<()> {
    let CurrentUserData::Parsed(current_user) = &self.current_user.data else {
      return Err(Error::invalid(
        0,
        "PPT relayout requires a conforming CurrentUserAtom",
      ));
    };
    let presentation = if preserve_compatibility {
      self
        .document
        .live_presentation_compatible(current_user)
        .map(ParseOutcome::into_value)
    } else {
      self.document.live_presentation(current_user)
    };
    let pictures_layout = match self.pictures.as_mut().map(Arc::make_mut) {
      Some(PicturesStream::Complete(pictures)) => Some(pictures.relayout()?),
      Some(PicturesStream::Compatibility { .. } | PicturesStream::Partial(_))
        if preserve_compatibility =>
      {
        None
      }
      Some(PicturesStream::Compatibility { .. } | PicturesStream::Partial(_)) => {
        return Err(Error::invalid(
          0,
          "PPT relayout requires a complete OfficeArtBStoreDelay",
        ));
      }
      None => None,
    };
    match presentation {
      Ok(_) => {
        Arc::make_mut(&mut self.document)
          .relocate_picture_references(pictures_layout.as_ref(), preserve_compatibility)?;
      }
      Err(_)
        if preserve_compatibility
          && pictures_layout
            .as_ref()
            .is_none_or(|layout| !layout.changed()) => {}
      Err(error) => return Err(error),
    }
    let CurrentUserData::Parsed(current_user) = &mut Arc::make_mut(&mut self.current_user).data
    else {
      unreachable!("CurrentUserAtom was checked above")
    };
    Arc::make_mut(&mut self.document).relayout_in_place(current_user, preserve_compatibility)?;
    self.sync_layout_baseline(!preserve_compatibility);
    Ok(())
  }

  /// Constructs the MS-PPT 2.1.2 Part 1 persist object directory from this
  /// file's Current User and PowerPoint Document streams.
  pub fn persist_object_directory(&self) -> Result<PersistObjectDirectory> {
    let CurrentUserData::Parsed(current_user) = &self.current_user.data else {
      return Err(Error::invalid(
        0,
        "PPT persist object directory requires a conforming CurrentUserAtom",
      ));
    };
    self.document.persist_object_directory(current_user)
  }

  /// Resolves the MS-PPT live presentation from the current user edit.
  pub fn live_presentation(&self) -> Result<PptLivePresentation<'_>> {
    let CurrentUserData::Parsed(current_user) = &self.current_user.data else {
      return Err(Error::invalid(
        0,
        "PPT live presentation requires a conforming CurrentUserAtom",
      ));
    };
    self.document.live_presentation(current_user)
  }

  /// Resolves the OfficeArt drawing graph from the current live PPT
  /// presentation rather than from superseded physical-history records.
  pub fn live_drawing_graph(&self) -> Result<OfficeArtDrawingGraph> {
    let CurrentUserData::Parsed(current_user) = &self.current_user.data else {
      return Err(Error::invalid(
        0,
        "PPT drawing graph requires a conforming CurrentUserAtom",
      ));
    };
    self.document.live_drawing_graph(current_user)
  }

  /// Resolves the live OfficeArt BLIP store to borrowed image payloads in
  /// the document tree or Pictures stream.
  pub fn live_image_store(&self) -> Result<PptLiveImageStore<'_>> {
    let CurrentUserData::Parsed(current_user) = &self.current_user.data else {
      return Err(Error::invalid(
        0,
        "PPT image store requires a conforming CurrentUserAtom",
      ));
    };
    self
      .document
      .live_image_store(current_user, self.pictures.as_deref())
  }

  pub fn live_presentation_compatible(&self) -> Result<ParseOutcome<PptLivePresentation<'_>>> {
    let CurrentUserData::Parsed(current_user) = &self.current_user.data else {
      return Err(Error::invalid(
        0,
        "PPT live presentation requires a conforming CurrentUserAtom",
      ));
    };
    self.document.live_presentation_compatible(current_user)
  }

  /// Transactionally edits the complete static record group for one list
  /// text body selected by the normative `SlidePersistAtom.slideId`.
  /// Failed edits, layout, or relationship validation leave the file root
  /// unchanged.
  pub fn edit_slide_text_body<T>(
    &mut self,
    slide_id: PptSlideId,
    text_body_index: usize,
    edit: impl FnOnce(PptLiveTextBodyMut<'_>) -> Result<T>,
  ) -> Result<T> {
    self.edit_slide_text_body_with_policy(slide_id, text_body_index, false, edit)
  }

  pub fn edit_slide_text_body_preserving_compatibility<T>(
    &mut self,
    slide_id: PptSlideId,
    text_body_index: usize,
    edit: impl FnOnce(PptLiveTextBodyMut<'_>) -> Result<T>,
  ) -> Result<T> {
    self.edit_slide_text_body_with_policy(slide_id, text_body_index, true, edit)
  }

  fn edit_slide_text_body_with_policy<T>(
    &mut self,
    slide_id: PptSlideId,
    text_body_index: usize,
    preserve_compatibility: bool,
    edit: impl FnOnce(PptLiveTextBodyMut<'_>) -> Result<T>,
  ) -> Result<T> {
    let mut rebuilt = self.clone();
    let source_offset = {
      let presentation = if preserve_compatibility {
        rebuilt.live_presentation_compatible()?.into_value()
      } else {
        rebuilt.live_presentation()?
      };
      let slides = if preserve_compatibility {
        presentation.slides_compatible()?
      } else {
        presentation.slides()?
      };
      let mut matches = slides.iter().filter(|slide| slide.id() == slide_id);
      let source = matches.next().ok_or_else(|| {
        Error::invalid(
          0,
          format!("presentation has no slide ID {}", slide_id.value()),
        )
      })?;
      if matches.next().is_some() {
        return Err(Error::invalid(
          source.object.source_record.offset,
          format!("presentation slide ID {} is ambiguous", slide_id.value()),
        ));
      }
      source.object.source_record.offset
    };
    let result = Arc::make_mut(&mut rebuilt.document).edit_list_text_body(
      source_offset,
      text_body_index,
      edit,
    )?;
    rebuilt.relayout_in_place_with_policy(preserve_compatibility)?;
    let presentation = if preserve_compatibility {
      rebuilt.live_presentation_compatible()?.into_value()
    } else {
      rebuilt.live_presentation()?
    };
    if preserve_compatibility {
      presentation.slides_compatible()?;
    } else {
      for slide in presentation.slides()? {
        slide.object.outline_text_references()?;
      }
    }
    *self = rebuilt;
    Ok(result)
  }

  /// Replaces the PowerPoint Document physical history with one current
  /// user edit containing only the MS-PPT live persist objects.
  pub fn rebuild_current_live_state(&mut self) -> Result<()> {
    let mut rebuilt = self.clone();
    let CurrentUserData::Parsed(current_user) = &mut Arc::make_mut(&mut rebuilt.current_user).data
    else {
      return Err(Error::invalid(
        0,
        "PPT current-live-state rebuild requires a conforming CurrentUserAtom",
      ));
    };
    Arc::make_mut(&mut rebuilt.document).rebuild_current_live_state(current_user)?;
    rebuilt.sync_layout_baseline(true);
    *self = rebuilt;
    Ok(())
  }

  /// Appends a full MS-PPT current persist-object checkpoint.
  ///
  /// The source checkpoint is restored from this root's compound-file
  /// snapshot, all current persist objects are appended as new physical
  /// records, and a new PersistDirectoryAtom/UserEditAtom pair becomes
  /// current. Calling this method commits a new source snapshot, so a later
  /// edit can append another checkpoint from the resulting root.
  pub fn append_user_edit(&mut self) -> Result<PptAppendUserEditReport> {
    let mut rebuilt = self.clone();
    let baseline = Self::from_compound_file(rebuilt.compound_file.clone())?;
    let CurrentUserData::Parsed(baseline_current_user) = &baseline.current_user.data else {
      return Err(Error::invalid(
        0,
        "append-user-edit requires a conforming source CurrentUserAtom",
      ));
    };
    let CurrentUserData::Parsed(current_user) = &mut Arc::make_mut(&mut rebuilt.current_user).data
    else {
      return Err(Error::invalid(
        0,
        "append-user-edit requires a conforming CurrentUserAtom",
      ));
    };
    let previous_edit_count = rebuilt
      .document
      .incremental_save_chain(current_user)?
      .edits
      .len();
    let previous_record_count = rebuilt.document.records.records.len();
    let source_pictures_layout =
      source_to_current_pictures_layout(baseline.pictures.as_deref(), rebuilt.pictures.as_deref())?;
    let persist_ids = Arc::make_mut(&mut rebuilt.document).append_user_edit_from_baseline(
      current_user,
      &baseline.document,
      baseline_current_user,
      source_pictures_layout.as_ref(),
    )?;
    rebuilt.relayout_in_place_with_policy(false)?;

    let CurrentUserData::Parsed(current_user) = &rebuilt.current_user.data else {
      unreachable!("append-user-edit retains the parsed CurrentUserAtom")
    };
    let chain = rebuilt.document.incremental_save_chain(current_user)?;
    if chain.edits.len() != previous_edit_count + 1 {
      return Err(Error::invalid(
        u64::from(current_user.offset_to_current_edit),
        "append-user-edit did not add exactly one incremental-save edit",
      ));
    }
    let current_edit = &chain.edits[0];
    let previous_edit = chain.edits.get(1).ok_or_else(|| {
      Error::invalid(
        u64::from(current_edit.user_edit_offset),
        "appended UserEditAtom does not reference the previous edit",
      )
    })?;
    if current_edit.user_edit.offset_last_edit != previous_edit.user_edit_offset {
      return Err(Error::invalid(
        u64::from(current_edit.user_edit_offset),
        "appended UserEditAtom offsetLastEdit does not reference the previous edit",
      ));
    }
    let metadata_record_count = previous_record_count
      .checked_add(2)
      .ok_or_else(|| Error::Limit("append-user-edit record count overflow".into()))?;
    let appended_persist_records = rebuilt
      .document
      .records
      .records
      .len()
      .checked_sub(metadata_record_count)
      .ok_or_else(|| Error::invalid(0, "append-user-edit record count decreased"))?;
    let report = PptAppendUserEditReport {
      previous_user_edit_offset: previous_edit.user_edit_offset,
      user_edit_offset: current_edit.user_edit_offset,
      persist_directory_offset: current_edit.persist_directory_offset,
      appended_persist_records,
      persist_ids,
    };

    rebuilt.compound_file = rebuilt.to_compound_file_with_current_layout(SaveOptions::default())?;
    *self = rebuilt;
    Ok(report)
  }

  fn from_compound_outcome(
    compound: ParseOutcome<CompoundFile>,
    options: ParseOptions,
  ) -> Result<ParseOutcome<Self>> {
    let ParseOutcome {
      value: compound_file,
      mut diagnostics,
    } = compound;
    let document = compound_file
      .stream(POWERPOINT_DOCUMENT_STREAM_PATH)
      .ok_or_else(|| Error::invalid(0, "PowerPoint Document stream is missing"))
      .and_then(|bytes| PowerPointDocument::from_bytes_with_limits(bytes, options.limits))?;
    audit_record_sequence(&document.records, 0, options.is_strict(), &mut diagnostics)?;
    let current_user = compound_file
      .stream(CURRENT_USER_STREAM_PATH)
      .ok_or_else(|| Error::invalid(0, "required Current User Stream is missing (MS-PPT 2.1.1)"))
      .and_then(CurrentUserStream::from_bytes)?;
    match &current_user.data {
      CurrentUserData::Parsed(atom) => {
        audit_current_user(&current_user, options.is_strict(), &mut diagnostics)?;
        if options.is_strict() {
          if let Err(error) = document.live_presentation(atom) {
            let offset = error
              .offset()
              .unwrap_or(u64::from(atom.offset_to_current_edit));
            return Err(Error::invalid(
              offset,
              format!(
                "PowerPoint Document Stream violates the live-record process in MS-PPT 2.1.2: {error}"
              ),
            ));
          }
        } else {
          match document.live_presentation_compatible(atom) {
            Ok(outcome) => diagnostics.extend(outcome.diagnostics),
            Err(error) => {
              let offset = error
                .offset()
                .unwrap_or(u64::from(atom.offset_to_current_edit));
              diagnostics.push(ParseDiagnostic::warning(
                ParseDiagnosticCode::InvalidReference,
                BinaryFormat::Ppt,
                Some(POWERPOINT_DOCUMENT_STREAM_PATH),
                Some(offset),
                "live-record process",
                SpecificationReference {
                  document: "MS-PPT",
                  section: "2.1.2",
                },
                format!("preserved a broken live-record process: {error}"),
              ));
            }
          }
        }
      }
      CurrentUserData::Compatibility(_) if options.is_strict() => {
        return Err(Error::invalid(
          0,
          "Current User Stream does not contain a conforming CurrentUserAtom",
        ));
      }
      CurrentUserData::Compatibility(_) => diagnostics.push(ParseDiagnostic::warning(
        ParseDiagnosticCode::NonconformingRecord,
        BinaryFormat::Ppt,
        Some(CURRENT_USER_STREAM_PATH),
        Some(0),
        "CurrentUserAtom",
        SpecificationReference {
          document: "MS-PPT",
          section: "2.3.2",
        },
        "preserved a nonconforming CurrentUserAtom body",
      )),
      CurrentUserData::Truncated(_) if options.is_strict() => {
        return Err(Error::invalid(
          0,
          "CurrentUserAtom body is shorter than RecordHeader.recLen",
        ));
      }
      CurrentUserData::Truncated(_) => diagnostics.push(ParseDiagnostic::warning(
        ParseDiagnosticCode::TruncatedRecord,
        BinaryFormat::Ppt,
        Some(CURRENT_USER_STREAM_PATH),
        Some(0),
        "CurrentUserAtom",
        SpecificationReference {
          document: "MS-PPT",
          section: "2.3.2",
        },
        "preserved the available prefix of a truncated CurrentUserAtom",
      )),
    }
    let pictures = compound_file
      .stream(PICTURES_STREAM_PATH)
      .map(|bytes| PicturesStream::from_bytes_with_limits(bytes, options.limits))
      .transpose()?;
    if let Some(pictures) = &pictures {
      let reason = match pictures {
        PicturesStream::Complete(_) => None,
        PicturesStream::Compatibility { reason, .. } => Some(reason.as_str()),
        PicturesStream::Partial(partial) => Some(partial.reason.as_str()),
      };
      if let Some(reason) = reason {
        if options.is_strict() {
          return Err(Error::invalid(
            0,
            format!("Pictures Stream violates MS-PPT 2.1.3 and MS-ODRAW 2.2.21: {reason}"),
          ));
        }
        diagnostics.push(ParseDiagnostic::warning(
          ParseDiagnosticCode::InvalidStreamPreserved,
          BinaryFormat::Ppt,
          Some(PICTURES_STREAM_PATH),
          Some(0),
          "OfficeArtBStoreDelay",
          SpecificationReference {
            document: "MS-PPT",
            section: "2.1.3",
          },
          format!("preserved a nonconforming Pictures Stream: {reason}"),
        ));
      }
    }
    let shared = OfficeSharedContent::from_compound_file_with_host(
      &compound_file,
      options,
      Some(OfficeHostKind::Ppt),
    )?;
    diagnostics.extend(shared.diagnostics);
    let document = Arc::new(document);
    let current_user = Arc::new(current_user);
    let pictures = pictures.map(Arc::new);
    let layout_baseline =
      PptManagedLayoutBaseline::new(&document, &current_user, &pictures, options.is_strict());
    Ok(ParseOutcome::new(
      Self {
        compound_file,
        shared: shared.value,
        document,
        current_user,
        pictures,
        layout_baseline,
      },
      diagnostics,
    ))
  }

  /// Transactionally edits the VBA project selected by the current PPT
  /// persist directory, recompressing its external storage when necessary.
  pub fn replace_vba_module_source(
    &mut self,
    stream_name: &str,
    source: &[u8],
  ) -> Result<OfficeVbaModuleMutation> {
    let mut candidate = self.clone();
    let CurrentUserData::Parsed(current_user) = &candidate.current_user.data else {
      return Err(Error::invalid(
        0,
        "PPT VBA mutation requires a conforming CurrentUserAtom",
      ));
    };
    let record_index = candidate
      .document
      .live_presentation(current_user)?
      .vba_project
      .ok_or_else(|| Error::invalid(0, "PPT live presentation has no VBA project"))?
      .reference
      .record_index;
    let record = Arc::make_mut(&mut candidate.document)
      .records
      .records
      .get_mut(record_index)
      .ok_or_else(|| Error::invalid(0, "PPT VBA persist record index is out of bounds"))?;
    let PptRecordData::ExternalStorage(storage) = &mut record.data else {
      return Err(Error::invalid(
        record.offset,
        "PPT VBA persist object is not an ExternalStorage record",
      ));
    };
    let vba = storage.replace_vba_module_source(stream_name, source)?;
    let invalidated_oleps_signatures = candidate.shared.invalidate_oleps_vba_signatures()?;
    candidate.relayout()?;
    *self = candidate;
    Ok(OfficeVbaModuleMutation {
      vba,
      invalidated_oleps_signatures,
      invalidated_host_signatures: 0,
    })
  }

  /// Transactionally edits a Designer storage in the live PPT VBA project.
  pub fn edit_vba_designer_storage(
    &mut self,
    index: usize,
    edit: impl FnOnce(&mut ParentControlStorageModel) -> Result<()>,
  ) -> Result<OfficeFormsMutation> {
    let mut candidate = self.clone();
    let CurrentUserData::Parsed(current_user) = &candidate.current_user.data else {
      return Err(Error::invalid(
        0,
        "PPT Forms mutation requires a conforming CurrentUserAtom",
      ));
    };
    let record_index = candidate
      .document
      .live_presentation(current_user)?
      .vba_project
      .ok_or_else(|| Error::invalid(0, "PPT live presentation has no VBA project"))?
      .reference
      .record_index;
    let record = Arc::make_mut(&mut candidate.document)
      .records
      .records
      .get_mut(record_index)
      .ok_or_else(|| Error::invalid(0, "PPT VBA persist record index is out of bounds"))?;
    let PptRecordData::ExternalStorage(storage) = &mut record.data else {
      return Err(Error::invalid(
        record.offset,
        "PPT VBA persist object is not an ExternalStorage record",
      ));
    };
    storage.edit_vba_designer_storage(index, edit)?;
    let invalidated_oleps_signatures = candidate.shared.invalidate_oleps_vba_signatures()?;
    candidate.relayout()?;
    *self = candidate;
    Ok(OfficeFormsMutation {
      invalidated_oleps_signatures,
      invalidated_host_signatures: 0,
    })
  }

  /// Rebuilds all managed streams from their typed trees and returns CFB.
  pub fn to_compound_file(&self) -> Result<CompoundFile> {
    self.to_compound_file_with_options(SaveOptions::default())
  }

  /// Rebuilds managed streams while retaining explicit compatibility nodes.
  pub fn to_compound_file_preserving_compatibility(&self) -> Result<CompoundFile> {
    self.to_compound_file_with_options(SaveOptions::preserving_compatibility())
  }

  /// Rebuilds managed streams under the requested compatibility policy.
  /// Physical history is preserved unless a history-strategy API is used.
  pub fn to_compound_file_with_options(&self, options: SaveOptions) -> Result<CompoundFile> {
    if let Ok(streams) = self.managed_streams_with_current_layout(options)
      && (options.preserves_compatibility() || self.layout_baseline.strict)
      && (self.has_current_managed_layout() || streams.matches_source(&self.compound_file))
    {
      return self.to_compound_file_with_managed_streams(options, streams);
    }
    let mut rebuilt = self.clone();
    if matches!(rebuilt.current_user.data, CurrentUserData::Parsed(_)) {
      rebuilt.relayout_in_place_with_policy(options.preserves_compatibility())?;
    }
    rebuilt.to_compound_file_with_current_layout(options)
  }

  /// Applies an explicit MS-PPT physical-history policy before strict save.
  pub fn to_compound_file_with_history_strategy(
    &self,
    strategy: PptHistoryStrategy,
  ) -> Result<CompoundFile> {
    let mut rebuilt = self.clone();
    match strategy {
      PptHistoryStrategy::PreservePhysicalHistory => {}
      PptHistoryStrategy::AppendUserEdit => {
        rebuilt.append_user_edit()?;
      }
      PptHistoryStrategy::RebuildCurrentLiveState => {
        rebuilt.rebuild_current_live_state()?;
      }
    }
    rebuilt.to_compound_file()
  }

  fn to_compound_file_with_current_layout(&self, options: SaveOptions) -> Result<CompoundFile> {
    let streams = self.managed_streams_with_current_layout(options)?;
    self.to_compound_file_with_managed_streams(options, streams)
  }

  fn managed_streams_with_current_layout(&self, options: SaveOptions) -> Result<PptManagedStreams> {
    self.validate_current_layout(options)?;
    Ok(PptManagedStreams {
      document: self.document.to_bytes()?,
      current_user: self.current_user.to_bytes()?,
      pictures: self
        .pictures
        .as_deref()
        .map(PicturesStream::to_bytes)
        .transpose()?,
    })
  }

  fn validate_current_layout(&self, options: SaveOptions) -> Result<()> {
    if !options.preserves_compatibility() {
      if !matches!(&self.current_user.data, CurrentUserData::Parsed(_)) {
        return Err(Error::invalid(
          0,
          "strict save rejects a nonconforming CurrentUserAtom",
        ));
      }
      audit_current_user(&self.current_user, true, &mut Vec::new())?;
      audit_record_sequence(&self.document.records, 0, true, &mut Vec::new())?;
      if let CurrentUserData::Parsed(atom) = &self.current_user.data {
        self.document.incremental_save_chain(atom)?;
      }
      if matches!(
        self.pictures.as_deref(),
        Some(PicturesStream::Compatibility { .. } | PicturesStream::Partial(_))
      ) {
        return Err(Error::invalid(
          0,
          "strict save rejects a nonconforming Pictures Stream",
        ));
      }
    }
    Ok(())
  }

  fn compound_for_managed_streaming(&self, options: SaveOptions) -> Result<CompoundFile> {
    self.validate_current_layout(options)?;
    let mut compound = self.compound_file.clone();
    compound.overwrite_stream(CURRENT_USER_STREAM_PATH, self.current_user.to_bytes()?)?;
    match self.pictures.as_deref() {
      Some(_) if !compound.is_stream(PICTURES_STREAM_PATH) => {
        compound.upsert_stream(PICTURES_STREAM_PATH, Vec::new())?;
      }
      None if compound.is_stream(PICTURES_STREAM_PATH) => {
        compound.remove_stream(PICTURES_STREAM_PATH)?;
      }
      Some(_) | None => {}
    }
    self.shared.write_to_compound_file(&mut compound, options)?;
    Ok(compound)
  }

  fn to_bytes_streaming_document(&self, options: SaveOptions) -> Result<Vec<u8>> {
    let ready = self.write_ready_layout(options)?;
    let compound = ready.compound_for_managed_streaming(options)?;
    let document_writer = PptDocumentStreamWriter(&ready.document);
    let document_len = ready.document.serialized_len()?;
    let pictures_writer = ready.pictures.as_deref().map(PptPicturesStreamWriter);
    let mut stream_overrides = Vec::with_capacity(2);
    stream_overrides.push(CfbStreamOverride::new(
      Path::new(POWERPOINT_DOCUMENT_STREAM_PATH),
      document_len,
      &document_writer,
    ));
    if let Some(pictures_writer) = pictures_writer.as_ref() {
      stream_overrides.push(CfbStreamOverride::new(
        Path::new(PICTURES_STREAM_PATH),
        pictures_writer.0.serialized_len()?,
        pictures_writer,
      ));
    }
    compound.to_bytes_with_stream_overrides(&stream_overrides)
  }

  fn write_streaming_document(&self, writer: impl Write, options: SaveOptions) -> Result<()> {
    let ready = self.write_ready_layout(options)?;
    let compound = ready.compound_for_managed_streaming(options)?;
    let document_writer = PptDocumentStreamWriter(&ready.document);
    let document_len = ready.document.serialized_len()?;
    let pictures_writer = ready.pictures.as_deref().map(PptPicturesStreamWriter);
    let mut stream_overrides = Vec::with_capacity(2);
    stream_overrides.push(CfbStreamOverride::new(
      Path::new(POWERPOINT_DOCUMENT_STREAM_PATH),
      document_len,
      &document_writer,
    ));
    if let Some(pictures_writer) = pictures_writer.as_ref() {
      stream_overrides.push(CfbStreamOverride::new(
        Path::new(PICTURES_STREAM_PATH),
        pictures_writer.0.serialized_len()?,
        pictures_writer,
      ));
    }
    compound.write_to_with_stream_overrides(&stream_overrides, writer)
  }

  fn to_compound_file_with_managed_streams(
    &self,
    options: SaveOptions,
    streams: PptManagedStreams,
  ) -> Result<CompoundFile> {
    let mut compound = self.compound_file.clone();
    compound.overwrite_stream(POWERPOINT_DOCUMENT_STREAM_PATH, streams.document)?;
    compound.overwrite_stream(CURRENT_USER_STREAM_PATH, streams.current_user)?;
    sync_optional_stream(
      &mut compound,
      PICTURES_STREAM_PATH,
      streams.pictures.map(Ok),
    )?;
    self.shared.write_to_compound_file(&mut compound, options)?;
    Ok(compound)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    self.to_bytes_with_options(SaveOptions::default())
  }

  pub fn to_bytes_preserving_compatibility(&self) -> Result<Vec<u8>> {
    self.to_bytes_with_options(SaveOptions::preserving_compatibility())
  }

  pub fn to_bytes_with_options(&self, options: SaveOptions) -> Result<Vec<u8>> {
    self.to_bytes_streaming_document(options)
  }

  pub fn to_bytes_with_history_strategy(&self, strategy: PptHistoryStrategy) -> Result<Vec<u8>> {
    self
      .to_compound_file_with_history_strategy(strategy)?
      .to_bytes()
  }

  pub fn write_to(&self, writer: impl Write) -> Result<()> {
    self.write_to_with_options(writer, SaveOptions::default())
  }

  pub fn write_to_preserving_compatibility(&self, writer: impl Write) -> Result<()> {
    self.write_to_with_options(writer, SaveOptions::preserving_compatibility())
  }

  pub fn write_to_with_options(&self, writer: impl Write, options: SaveOptions) -> Result<()> {
    self.write_streaming_document(writer, options)
  }

  pub fn write_to_with_history_strategy(
    &self,
    writer: impl Write,
    strategy: PptHistoryStrategy,
  ) -> Result<()> {
    self
      .to_compound_file_with_history_strategy(strategy)?
      .write_to(writer)
  }

  pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
    self.save_with_options(path, SaveOptions::default())
  }

  pub fn save_preserving_compatibility(&self, path: impl AsRef<Path>) -> Result<()> {
    self.save_with_options(path, SaveOptions::preserving_compatibility())
  }

  pub fn save_with_options(&self, path: impl AsRef<Path>, options: SaveOptions) -> Result<()> {
    self.write_streaming_document(std::io::sink(), options)?;
    self.write_streaming_document(std::fs::File::create(path)?, options)
  }

  pub fn save_with_history_strategy(
    &self,
    path: impl AsRef<Path>,
    strategy: PptHistoryStrategy,
  ) -> Result<()> {
    let compound = self.to_compound_file_with_history_strategy(strategy)?;
    compound.write_to(std::fs::File::create(path)?)
  }
}

fn source_to_current_pictures_layout(
  source: Option<&PicturesStream>,
  current: Option<&PicturesStream>,
) -> Result<Option<OfficeArtBStoreDelayLayout>> {
  let (source, current) = match (source, current) {
    (None, None) => return Ok(None),
    (None, Some(PicturesStream::Complete(_))) => {
      return Ok(Some(OfficeArtBStoreDelayLayout {
        file_blocks: Vec::new(),
      }));
    }
    (Some(PicturesStream::Complete(_)), None) => {
      return Err(Error::invalid(
        0,
        "append-user-edit cannot remove the source Pictures Stream",
      ));
    }
    (Some(PicturesStream::Complete(source)), Some(PicturesStream::Complete(current))) => {
      (source, current)
    }
    _ => {
      return Err(Error::invalid(
        0,
        "append-user-edit requires conforming source and current Pictures streams",
      ));
    }
  };
  if current.records.len() < source.records.len() {
    return Err(Error::invalid(
      0,
      "append-user-edit cannot remove Pictures Stream file blocks",
    ));
  }
  for (index, source_record) in source.records.iter().enumerate() {
    if current.records[index].header.record_type != source_record.header.record_type {
      return Err(Error::invalid(
        0,
        "append-user-edit only supports Pictures Stream additions after source file blocks",
      ));
    }
  }

  let mut normalized = current.clone();
  normalized.relayout()?;
  let mut old_offset = 0u32;
  let mut new_offset = 0u32;
  let mut file_blocks = Vec::with_capacity(source.records.len());
  for (record_index, source_record) in source.records.iter().enumerate() {
    let current_record = &normalized.records[record_index];
    let old_size = source_record
      .header
      .declared_length
      .checked_add(8)
      .ok_or_else(|| Error::Limit("source Pictures file-block size overflow".into()))?;
    let new_size = current_record
      .header
      .declared_length
      .checked_add(8)
      .ok_or_else(|| Error::Limit("current Pictures file-block size overflow".into()))?;
    file_blocks.push(OfficeArtBStoreDelayFileBlockLayout {
      record_index,
      record_type: source_record.header.record_type,
      old_offset,
      new_offset,
      old_size,
      new_size,
    });
    old_offset = old_offset
      .checked_add(old_size)
      .ok_or_else(|| Error::Limit("source Pictures Stream offset overflow".into()))?;
    new_offset = new_offset
      .checked_add(new_size)
      .ok_or_else(|| Error::Limit("current Pictures Stream offset overflow".into()))?;
  }
  Ok(Some(OfficeArtBStoreDelayLayout { file_blocks }))
}

fn audit_current_user(
  stream: &CurrentUserStream,
  strict: bool,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<()> {
  let CurrentUserData::Parsed(atom) = &stream.data else {
    return Ok(());
  };
  let mut violations = Vec::new();
  if stream.header.version != 0 || stream.header.instance != 0 {
    violations.push(format!(
      "RecordHeader has recVer {:#x} and recInstance {:#x}, expected 0 and 0",
      stream.header.version, stream.header.instance
    ));
  }
  if atom.fixed_size != 20 {
    violations.push(format!("size is {}, expected 20", atom.fixed_size));
  }
  if !matches!(atom.header_token, 0xe391_c05f | 0xf3d1_c4df) {
    violations.push(format!(
      "headerToken is {:#010x}, outside the two specified values",
      atom.header_token
    ));
  }
  if atom.declared_user_name_byte_length > 255 {
    violations.push(format!(
      "lenUserName is {}, greater than 255",
      atom.declared_user_name_byte_length
    ));
  }
  if atom.document_file_version != 0x03f4 || atom.major_version != 3 || atom.minor_version != 0 {
    violations.push(format!(
      "file/storage version is {:#06x}/{}.{}, expected 0x03f4/3.0",
      atom.document_file_version, atom.major_version, atom.minor_version
    ));
  }
  if !matches!(atom.release_version, 8 | 9) {
    violations.push(format!(
      "relVersion is {}, expected 8 or 9",
      atom.release_version
    ));
  }
  if !atom.trailing.is_empty() {
    violations.push(format!(
      "CurrentUserAtom contains {} trailing bytes after its specified fields",
      atom.trailing.len()
    ));
  }
  if !violations.is_empty() {
    report_current_user_issue(
      strict,
      diagnostics,
      ParseDiagnosticCode::NonconformingRecord,
      violations.join("; "),
    )?;
  }
  if let Some(unicode) = &atom.unicode_user_name
    && !unicode.is_complete
  {
    report_current_user_issue(
      strict,
      diagnostics,
      ParseDiagnosticCode::TruncatedRecord,
      format!(
        "unicodeUserName contains {} of the required {} UTF-16 code units",
        unicode.code_units.len(),
        atom.declared_user_name_byte_length
      ),
    )?;
  }
  Ok(())
}

fn report_current_user_issue(
  strict: bool,
  diagnostics: &mut Vec<ParseDiagnostic>,
  code: ParseDiagnosticCode,
  message: String,
) -> Result<()> {
  if strict {
    return Err(Error::invalid(
      0,
      format!("Current User Stream violates MS-PPT 2.3.2: {message}"),
    ));
  }
  diagnostics.push(ParseDiagnostic::warning(
    code,
    BinaryFormat::Ppt,
    Some(CURRENT_USER_STREAM_PATH),
    Some(0),
    "CurrentUserAtom",
    SpecificationReference {
      document: "MS-PPT",
      section: "2.3.2",
    },
    message,
  ));
  Ok(())
}

fn audit_record_sequence(
  sequence: &PptRecordSequence,
  base_offset: u64,
  strict: bool,
  diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<()> {
  for record in &sequence.records {
    match &record.data {
      PptRecordData::CompatibilityTextChars(code_units) => report_record_issue(
        strict,
        diagnostics,
        ParseDiagnosticCode::NonconformingRecord,
        record.offset,
        "TextCharsAtom",
        "2.9.40",
        format!(
          "preserved {} UTF-16 code units containing an unpaired surrogate",
          code_units.len()
        ),
      )?,
      PptRecordData::CompatibilityCString(code_units) => report_record_issue(
        strict,
        diagnostics,
        ParseDiagnosticCode::NonconformingRecord,
        record.offset,
        "CString",
        "2.9.7",
        format!(
          "preserved {} UTF-16 code units containing an unpaired surrogate",
          code_units.len()
        ),
      )?,
      PptRecordData::MalformedSpecRecord(value) => report_record_issue(
        strict,
        diagnostics,
        ParseDiagnosticCode::NonconformingRecord,
        record.offset,
        "Record",
        "2.3",
        format!(
          "record type 0x{:04X} has a body that violates its MS-PPT structure",
          value.record_type
        ),
      )?,
      PptRecordData::Truncated(bytes) => report_record_issue(
        strict,
        diagnostics,
        ParseDiagnosticCode::TruncatedRecord,
        record.offset,
        "RecordHeader",
        "2.3.1",
        format!(
          "record type 0x{:04X} declares {} body bytes but only {} remain",
          record.header.record_type,
          record.header.declared_length,
          bytes.len()
        ),
      )?,
      PptRecordData::MalformedTextSpecialInfo(bytes) => report_record_issue(
        strict,
        diagnostics,
        ParseDiagnosticCode::NonconformingRecord,
        record.offset,
        "TextSpecialInfoAtom",
        "2.9.54",
        format!(
          "TextSpecialInfoAtom contains {} bytes that do not form its rgSIRun array",
          bytes.len()
        ),
      )?,
      PptRecordData::MalformedStyleTextProp(_) | PptRecordData::UnresolvedStyleTextProp(_) => {
        report_record_issue(
          strict,
          diagnostics,
          ParseDiagnosticCode::NonconformingRecord,
          record.offset,
          "StyleTextPropAtom",
          "2.9.44",
          "StyleTextPropAtom does not form the runs required for its corresponding text".into(),
        )?
      }
      PptRecordData::MalformedTextMasterStyle(_) => report_record_issue(
        strict,
        diagnostics,
        ParseDiagnosticCode::NonconformingRecord,
        record.offset,
        "TextMasterStyleAtom",
        "2.9.35",
        "TextMasterStyleAtom body does not satisfy its level structures".into(),
      )?,
      PptRecordData::MalformedTextRuler(_) => report_record_issue(
        strict,
        diagnostics,
        ParseDiagnosticCode::NonconformingRecord,
        record.offset,
        "TextRulerAtom",
        "2.9.29",
        "TextRulerAtom body does not satisfy its masked field layout".into(),
      )?,
      PptRecordData::MalformedStyleTextProp9(_) => report_record_issue(
        strict,
        diagnostics,
        ParseDiagnosticCode::NonconformingRecord,
        record.offset,
        "StyleTextProp9Atom",
        "2.9.67",
        "StyleTextProp9Atom body does not satisfy its run layout".into(),
      )?,
      PptRecordData::MalformedTimeVariant(_) => report_record_issue(
        strict,
        diagnostics,
        ParseDiagnosticCode::NonconformingRecord,
        record.offset,
        "TimeVariant",
        "2.8.78",
        "TimeVariant body does not match its discriminant".into(),
      )?,
      PptRecordData::MalformedBlipEntity9 { reason, .. } => report_record_issue(
        strict,
        diagnostics,
        ParseDiagnosticCode::NonconformingRecord,
        record.offset,
        "BlipEntityAtom",
        "2.9.73",
        format!("BlipEntityAtom body is invalid: {reason}"),
      )?,
      PptRecordData::ExternalStorage(storage) => {
        let issue = match storage {
          ExternalStorageAtom::Parsed(_) => None,
          ExternalStorageAtom::MalformedCompressed { reason, .. }
          | ExternalStorageAtom::InvalidCompressed { reason, .. } => {
            Some(("ExOleObjStgCompressedAtom", "2.10.36", reason.as_str()))
          }
          ExternalStorageAtom::InvalidUncompressed { reason, .. } => {
            Some(("ExOleObjStgUncompressedAtom", "2.10.35", reason.as_str()))
          }
          ExternalStorageAtom::UnsupportedInstance { .. } => Some((
            "ExOleObjStg",
            "2.10.34",
            "recInstance is neither compressed nor uncompressed",
          )),
        };
        if let Some((structure, section, reason)) = issue {
          report_record_issue(
            strict,
            diagnostics,
            ParseDiagnosticCode::NonconformingRecord,
            record.offset,
            structure,
            section,
            format!("preserved an invalid external storage record: {reason}"),
          )?;
        }
      }
      PptRecordData::Container(children) | PptRecordData::ProgTags(children) => {
        audit_record_sequence(
          children,
          record.offset.saturating_add(8),
          strict,
          diagnostics,
        )?;
      }
      PptRecordData::ProgBinaryTag(value) => audit_record_sequence(
        &value.records,
        record.offset.saturating_add(8),
        strict,
        diagnostics,
      )?,
      PptRecordData::BinaryTagData(BinaryTagData::Records(children)) => {
        audit_record_sequence(
          children,
          record.offset.saturating_add(8),
          strict,
          diagnostics,
        )?;
      }
      _ => {}
    }
  }
  if !sequence.trailing_header_bytes.is_empty() {
    let offset = sequence
      .records
      .last()
      .map(record_physical_end)
      .unwrap_or(base_offset);
    report_record_issue(
      strict,
      diagnostics,
      ParseDiagnosticCode::TruncatedRecord,
      offset,
      "RecordHeader",
      "2.3.1",
      format!(
        "{} trailing bytes cannot form the required 8-byte RecordHeader",
        sequence.trailing_header_bytes.len()
      ),
    )?;
  }
  Ok(())
}

fn record_physical_end(record: &PptRecord) -> u64 {
  let body_len = match &record.data {
    PptRecordData::Truncated(bytes) => bytes.len() as u64,
    _ => u64::from(record.header.declared_length),
  };
  record.offset.saturating_add(8).saturating_add(body_len)
}

fn report_record_issue(
  strict: bool,
  diagnostics: &mut Vec<ParseDiagnostic>,
  code: ParseDiagnosticCode,
  offset: u64,
  structure: &'static str,
  section: &'static str,
  message: String,
) -> Result<()> {
  if strict {
    return Err(Error::invalid(
      offset,
      format!("PowerPoint Document Stream violates MS-PPT {section}: {message}"),
    ));
  }
  diagnostics.push(ParseDiagnostic::warning(
    code,
    BinaryFormat::Ppt,
    Some(POWERPOINT_DOCUMENT_STREAM_PATH),
    Some(offset),
    structure,
    SpecificationReference {
      document: "MS-PPT",
      section,
    },
    message,
  ));
  Ok(())
}

fn sync_optional_stream(
  compound: &mut CompoundFile,
  path: &str,
  bytes: Option<Result<Vec<u8>>>,
) -> Result<()> {
  match bytes {
    Some(bytes) => {
      compound.upsert_stream(path, bytes?)?;
    }
    None if compound.is_stream(path) => {
      compound.remove_stream(path)?;
    }
    None => {}
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    cfb::Version,
    office_art::{
      OfficeArtBStoreDelay, OfficeArtBitmapBlip, OfficeArtBitmapData, OfficeArtFbse,
      OfficeArtRecord, OfficeArtRecordData, OfficeArtRecordHeader,
    },
    ppt::{
      CURRENT_USER_ATOM, CurrentUserAtom, DOCUMENT_ATOM, DOCUMENT_CONTAINER, DocumentAtom,
      EXTERNAL_OLE_OBJECT_STORAGE, PERSIST_DIRECTORY_ATOM, PersistDirectoryAtom,
      PersistObjectReferenceStatus, PptLivePersistObjectRole, PptPoint, PptRecordHeader,
      SLIDE_LIST_WITH_TEXT_CONTAINER, USER_EDIT_ATOM, UnknownPptRecord, UserEditAtom,
    },
  };

  fn current_user_stream() -> CurrentUserStream {
    let atom = CurrentUserAtom {
      fixed_size: 20,
      header_token: 0xe391_c05f,
      offset_to_current_edit: 80,
      declared_user_name_byte_length: 3,
      document_file_version: 0x03f4,
      major_version: 3,
      minor_version: 0,
      unused: 0,
      ansi_user_name: b"Ada".to_vec(),
      release_version: 8,
      unicode_user_name: None,
      trailing: Vec::new(),
    };
    let (body, following) = atom.to_parts().unwrap();
    assert!(following.is_empty());
    CurrentUserStream {
      header: PptRecordHeader {
        version: 0,
        instance: 0,
        record_type: CURRENT_USER_ATOM,
        declared_length: body.len() as u32,
      },
      data: CurrentUserData::Parsed(atom),
      padding: Vec::new(),
    }
  }

  fn document_with_minimal_chain(mut suffix: Vec<u8>) -> Vec<u8> {
    let document_atom = DocumentAtom {
      slide_size: PptPoint { x: 720, y: 540 },
      notes_size: PptPoint { x: 540, y: 720 },
      server_zoom: PptPoint { x: 1, y: 1 },
      notes_master_persist_id_ref: 0,
      handout_master_persist_id_ref: 0,
      first_slide_number: 1,
      slide_size_type: 0,
      save_with_fonts: 0,
      omit_title_placeholders: 0,
      right_to_left: 0,
      show_comments: 0,
    };
    let document_atom_body = super::super::write_fixed(&document_atom).unwrap();
    let mut document_container_body = Vec::new();
    PptRecordHeader {
      version: 1,
      instance: 0,
      record_type: DOCUMENT_ATOM,
      declared_length: document_atom_body.len() as u32,
    }
    .write(&mut document_container_body)
    .unwrap();
    document_container_body.extend_from_slice(&document_atom_body);
    PptRecordHeader {
      version: 0x0f,
      instance: 1,
      record_type: SLIDE_LIST_WITH_TEXT_CONTAINER,
      declared_length: 0,
    }
    .write(&mut document_container_body)
    .unwrap();

    let persist_body = PersistDirectoryAtom {
      entries: vec![super::super::PersistDirectoryEntry {
        first_persist_id: 1,
        stream_offsets: vec![0],
      }],
    }
    .to_bytes()
    .unwrap();
    assert_eq!(document_container_body.len(), 56);
    assert_eq!(persist_body.len(), 8);
    let user_body = UserEditAtom {
      last_slide_id_ref: 0,
      version: 0,
      minor_version: 0,
      major_version: 3,
      offset_last_edit: 0,
      offset_persist_directory: 64,
      doc_persist_id_ref: 1,
      persist_id_seed: 1,
      last_view: 1,
      unused: 0,
      encrypt_session_persist_id_ref: None,
    };
    let user_body = super::super::write_fixed(&user_body).unwrap();
    let mut document = Vec::new();
    PptRecordHeader {
      version: 0x0f,
      instance: 0,
      record_type: DOCUMENT_CONTAINER,
      declared_length: document_container_body.len() as u32,
    }
    .write(&mut document)
    .unwrap();
    document.extend_from_slice(&document_container_body);
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: PERSIST_DIRECTORY_ATOM,
      declared_length: persist_body.len() as u32,
    }
    .write(&mut document)
    .unwrap();
    document.extend_from_slice(&persist_body);
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: USER_EDIT_ATOM,
      declared_length: user_body.len() as u32,
    }
    .write(&mut document)
    .unwrap();
    document.extend_from_slice(&user_body);
    document.append(&mut suffix);
    document
  }

  fn compound_with_document(document: Vec<u8>) -> CompoundFile {
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound
      .create_or_replace_stream(POWERPOINT_DOCUMENT_STREAM_PATH, document)
      .unwrap();
    compound
      .create_or_replace_stream(
        CURRENT_USER_STREAM_PATH,
        current_user_stream().to_bytes().unwrap(),
      )
      .unwrap();
    compound
  }

  #[test]
  fn file_root_round_trips_the_typed_document_stream() {
    let compound = compound_with_document(document_with_minimal_chain(Vec::new()));
    let file = PptFile::from_compound_file(compound).unwrap();
    assert_eq!(file.document.records.records.len(), 3);
    let direct = file.to_bytes().unwrap();
    assert_eq!(direct, file.to_compound_file().unwrap().to_bytes().unwrap());
    let mut streamed = Vec::new();
    file.write_to(&mut streamed).unwrap();
    assert_eq!(streamed, direct);
    let reopened = PptFile::from_bytes(&direct).unwrap();
    assert_eq!(reopened.document, file.document);
  }

  #[test]
  fn file_root_clone_shares_document_until_explicit_mutation() {
    let compound = compound_with_document(document_with_minimal_chain(Vec::new()));
    let file = PptFile::from_compound_file(compound).unwrap();
    let mut cloned = file.clone();
    assert!(Arc::ptr_eq(&file.document, &cloned.document));
    assert!(Arc::ptr_eq(&file.current_user, &cloned.current_user));
    assert!(file.has_current_managed_layout());
    assert!(cloned.has_current_managed_layout());

    Arc::make_mut(&mut cloned.document).records.records[0]
      .header
      .instance = 7;
    assert_eq!(file.document.records.records[0].header.instance, 0);
    assert_eq!(cloned.document.records.records[0].header.instance, 7);
    assert!(!Arc::ptr_eq(&file.document, &cloned.document));
    assert!(!cloned.has_current_managed_layout());

    let CurrentUserData::Parsed(current_user) = &mut Arc::make_mut(&mut cloned.current_user).data
    else {
      unreachable!()
    };
    current_user.unused = 9;
    let CurrentUserData::Parsed(original_current_user) = &file.current_user.data else {
      unreachable!()
    };
    assert_eq!(original_current_user.unused, 0);
    assert!(!Arc::ptr_eq(&file.current_user, &cloned.current_user));
    assert!(!cloned.has_current_managed_layout());
  }

  #[test]
  fn file_root_relayouts_variable_records_and_incremental_save_references() {
    let compound = compound_with_document(document_with_minimal_chain(Vec::new()));
    let mut file = PptFile::from_compound_file(compound).unwrap();
    assert!(file.has_current_managed_layout());
    let document = Arc::make_mut(&mut file.document);
    let PptRecordData::Container(value) = &mut document.records.records[0].data else {
      unreachable!()
    };
    value.records.push(PptRecord {
      offset: 8,
      header: PptRecordHeader {
        version: 0,
        instance: 0,
        record_type: 0x779f,
        declared_length: 5,
      },
      data: PptRecordData::Unknown(UnknownPptRecord {
        record_type: 0x779f,
        body: vec![1, 2, 3, 4, 5],
      }),
    });
    let PptRecordData::PersistDirectory(value) = &mut document.records.records[1].data else {
      unreachable!()
    };
    value.entries.push(super::super::PersistDirectoryEntry {
      first_persist_id: 2,
      stream_offsets: vec![0],
    });

    let mut invalid = file.clone();
    let PptRecordData::PersistDirectory(value) =
      &mut Arc::make_mut(&mut invalid.document).records.records[1].data
    else {
      unreachable!()
    };
    value.entries[0].stream_offsets[0] = 999;
    let unchanged_after_failure = invalid.clone();
    assert!(invalid.relayout().is_err());
    assert_eq!(invalid, unchanged_after_failure);

    assert!(!file.has_current_managed_layout());
    file.relayout().unwrap();
    assert!(file.has_current_managed_layout());
    assert_eq!(file.document.records.records[0].header.declared_length, 69);
    assert_eq!(file.document.records.records[1].offset, 77);
    assert_eq!(file.document.records.records[1].header.declared_length, 16);
    assert_eq!(file.document.records.records[2].offset, 101);
    let PptRecordData::UserEdit(user_edit) = &file.document.records.records[2].data else {
      unreachable!()
    };
    assert_eq!(user_edit.offset_persist_directory, 77);
    let CurrentUserData::Parsed(current_user) = &file.current_user.data else {
      unreachable!()
    };
    assert_eq!(current_user.offset_to_current_edit, 101);

    let reopened = PptFile::from_bytes(&file.to_bytes().unwrap()).unwrap();
    assert_eq!(reopened.document, file.document);
    assert_eq!(reopened.current_user, file.current_user);
  }

  #[test]
  fn file_root_relayouts_pictures_and_physical_fbse_delay_references() {
    fn bitmap_blip(data: Vec<u8>) -> OfficeArtRecord {
      OfficeArtRecord {
        header: OfficeArtRecordHeader {
          version: 0,
          instance: 0x06e0,
          record_type: 0xf01e,
          declared_length: u32::try_from(17 + data.len()).unwrap(),
        },
        data: OfficeArtRecordData::BitmapBlip(OfficeArtBitmapBlip {
          uid1: [0x11; 16],
          uid2: None,
          tag: 0xff,
          file_data: OfficeArtBitmapData::Encoded(data),
        }),
      }
    }

    let compound = compound_with_document(document_with_minimal_chain(Vec::new()));
    let mut file = PptFile::from_compound_file(compound).unwrap();
    let first_blip = bitmap_blip(vec![1, 2]);
    let second_blip = bitmap_blip(vec![3]);
    let old_second_offset = first_blip.header.declared_length + 8;
    let second_size = second_blip.header.declared_length + 8;
    file.pictures = Some(Arc::new(PicturesStream::Complete(OfficeArtBStoreDelay {
      records: vec![first_blip, second_blip],
    })));

    let fbse = OfficeArtFbse {
      win32_blip_type: 6,
      macos_blip_type: 6,
      uid: [0x22; 16],
      tag: 0xff,
      declared_blip_size: second_size,
      reference_count: 1,
      delay_offset: old_second_offset,
      unused1: 0,
      declared_name_length: 0,
      unused2: 0,
      unused3: 0,
      name_data: Vec::new(),
      embedded_blip: None,
      trailing: Vec::new(),
    };
    let fbse_record = PptRecord {
      offset: 0,
      header: PptRecordHeader {
        version: 2,
        instance: 6,
        record_type: 0xf007,
        declared_length: 36,
      },
      data: PptRecordData::OfficeArt(Box::new(OfficeArtRecord {
        header: OfficeArtRecordHeader {
          version: 2,
          instance: 6,
          record_type: 0xf007,
          declared_length: 36,
        },
        data: OfficeArtRecordData::Fbse(fbse),
      })),
    };
    let mut dead_fbse_record = fbse_record.clone();
    dead_fbse_record.offset = 120;
    let PptRecordData::Container(document) =
      &mut Arc::make_mut(&mut file.document).records.records[0].data
    else {
      unreachable!()
    };
    document.records.push(PptRecord {
      offset: 0,
      header: PptRecordHeader {
        version: 0x0f,
        instance: 1,
        record_type: 0xf001,
        declared_length: 44,
      },
      data: PptRecordData::Container(PptRecordSequence {
        records: vec![fbse_record],
        trailing_header_bytes: Vec::new(),
      }),
    });
    Arc::make_mut(&mut file.document)
      .records
      .records
      .push(dead_fbse_record);

    let Some(pictures) = &mut file.pictures else {
      unreachable!()
    };
    let PicturesStream::Complete(pictures) = Arc::make_mut(pictures) else {
      unreachable!()
    };
    let OfficeArtRecordData::BitmapBlip(first) = &mut pictures.records[0].data else {
      unreachable!()
    };
    let OfficeArtBitmapData::Encoded(data) = &mut first.file_data else {
      unreachable!()
    };
    data.extend_from_slice(&[4, 5, 6]);
    let OfficeArtRecordData::BitmapBlip(second) = &mut pictures.records[1].data else {
      unreachable!()
    };
    let OfficeArtBitmapData::Encoded(data) = &mut second.file_data else {
      unreachable!()
    };
    data.extend_from_slice(&[7, 8]);

    let mut invalid = file.clone();
    let PptRecordData::Container(document) =
      &mut Arc::make_mut(&mut invalid.document).records.records[0].data
    else {
      unreachable!()
    };
    let PptRecordData::Container(bstore) = &mut document.records.last_mut().unwrap().data else {
      unreachable!()
    };
    let PptRecordData::OfficeArt(fbse) = &mut bstore.records[0].data else {
      unreachable!()
    };
    let OfficeArtRecordData::Fbse(fbse) = &mut fbse.data else {
      unreachable!()
    };
    fbse.delay_offset = 999;
    let unchanged = invalid.clone();
    assert!(invalid.relayout().is_err());
    assert_eq!(invalid, unchanged);

    file.relayout().unwrap();
    let Some(PicturesStream::Complete(pictures)) = file.pictures.as_deref() else {
      unreachable!()
    };
    assert_eq!(pictures.records[0].header.declared_length, 22);
    assert_eq!(pictures.records[1].header.declared_length, 20);
    let new_second_offset = pictures.records[0].header.declared_length + 8;
    assert_eq!(new_second_offset, old_second_offset + 3);
    let PptRecordData::Container(document) = &file.document.records.records[0].data else {
      unreachable!()
    };
    let PptRecordData::Container(bstore) = &document.records.last().unwrap().data else {
      unreachable!()
    };
    let PptRecordData::OfficeArt(fbse) = &bstore.records[0].data else {
      unreachable!()
    };
    let OfficeArtRecordData::Fbse(fbse) = &fbse.data else {
      unreachable!()
    };
    assert_eq!(fbse.delay_offset, new_second_offset);
    assert_eq!(fbse.declared_blip_size, second_size + 2);
    let PptRecordData::OfficeArt(dead_fbse) = &file.document.records.records.last().unwrap().data
    else {
      unreachable!()
    };
    let OfficeArtRecordData::Fbse(dead_fbse) = &dead_fbse.data else {
      unreachable!()
    };
    assert_eq!(dead_fbse.delay_offset, new_second_offset);
    assert_eq!(dead_fbse.declared_blip_size, second_size + 2);

    let direct = file.to_bytes().unwrap();
    let materialized = file.to_compound_file().unwrap().to_bytes().unwrap();
    assert_eq!(direct, materialized);
    let mut streamed = Vec::new();
    file.write_to(&mut streamed).unwrap();
    assert_eq!(streamed, direct);
    let mut reopened = PptFile::from_bytes(&direct).unwrap();
    assert_eq!(reopened.document, file.document);
    assert_eq!(reopened.pictures, file.pictures);

    let Some(pictures) = &mut reopened.pictures else {
      unreachable!()
    };
    let PicturesStream::Complete(pictures) = Arc::make_mut(pictures) else {
      unreachable!()
    };
    let OfficeArtRecordData::BitmapBlip(first) = &mut pictures.records[0].data else {
      unreachable!()
    };
    let OfficeArtBitmapData::Encoded(data) = &mut first.file_data else {
      unreachable!()
    };
    data.push(9);
    reopened.relayout().unwrap();
    let report = reopened.append_user_edit().unwrap();
    assert_eq!(report.persist_ids, vec![1]);
    assert_eq!(
      reopened
        .persist_object_directory()
        .unwrap()
        .incremental_save_chain
        .edits
        .len(),
      2
    );
    PptFile::from_bytes(&reopened.to_bytes().unwrap()).unwrap();
  }

  #[test]
  fn file_root_appends_repeatable_user_edit_checkpoints() {
    fn set_first_slide_number(file: &mut PptFile, value: u16) {
      let record_index = file
        .live_presentation()
        .unwrap()
        .document
        .reference
        .record_index;
      let record = &mut Arc::make_mut(&mut file.document).records.records[record_index];
      let PptRecordData::Container(children) = &mut record.data else {
        unreachable!()
      };
      let document = children
        .records
        .iter_mut()
        .find_map(|record| match &mut record.data {
          PptRecordData::Document(document) => Some(document),
          _ => None,
        })
        .unwrap();
      document.first_slide_number = value;
    }

    fn first_slide_number(record: &PptRecord) -> u16 {
      let PptRecordData::Container(children) = &record.data else {
        unreachable!()
      };
      children
        .records
        .iter()
        .find_map(|record| match &record.data {
          PptRecordData::Document(document) => Some(document.first_slide_number),
          _ => None,
        })
        .unwrap()
    }

    let compound = compound_with_document(document_with_minimal_chain(Vec::new()));
    let mut file = PptFile::from_compound_file(compound).unwrap();
    set_first_slide_number(&mut file, 2);
    let first_report = file.append_user_edit().unwrap();
    assert_eq!(first_report.appended_persist_records, 1);
    assert_eq!(first_report.persist_ids, vec![1]);
    assert!(first_report.previous_user_edit_offset < first_report.persist_directory_offset);
    assert!(first_report.persist_directory_offset < first_report.user_edit_offset);

    let directory = file.persist_object_directory().unwrap();
    assert_eq!(directory.incremental_save_chain.edits.len(), 2);
    let current = directory.current_reference(1).unwrap();
    assert_eq!(
      first_slide_number(&file.document.records.records[current.record_index]),
      2
    );
    let previous = directory
      .references
      .iter()
      .find(|reference| {
        reference.persist_id == 1 && reference.status == PersistObjectReferenceStatus::Superseded
      })
      .unwrap();
    assert_eq!(
      first_slide_number(&file.document.records.records[previous.record_index]),
      1
    );

    set_first_slide_number(&mut file, 3);
    let second_report = file.append_user_edit().unwrap();
    assert_eq!(second_report.appended_persist_records, 1);
    assert_eq!(second_report.persist_ids, vec![1]);
    let directory = file.persist_object_directory().unwrap();
    assert_eq!(directory.incremental_save_chain.edits.len(), 3);
    let versions = directory
      .references
      .iter()
      .filter(|reference| reference.persist_id == 1)
      .map(|reference| first_slide_number(&file.document.records.records[reference.record_index]))
      .collect::<Vec<_>>();
    assert_eq!(versions, vec![3, 2, 1]);

    let reopened = PptFile::from_bytes(&file.to_bytes().unwrap()).unwrap();
    assert_eq!(reopened.document, file.document);
    assert_eq!(reopened.current_user, file.current_user);
    assert_eq!(
      reopened.live_presentation().unwrap().document.role,
      PptLivePersistObjectRole::Document
    );

    let compound = compound_with_document(document_with_minimal_chain(Vec::new()));
    let mut strategic = PptFile::from_compound_file(compound).unwrap();
    set_first_slide_number(&mut strategic, 4);
    let bytes = strategic
      .to_bytes_with_history_strategy(PptHistoryStrategy::AppendUserEdit)
      .unwrap();
    assert_eq!(
      strategic
        .persist_object_directory()
        .unwrap()
        .incremental_save_chain
        .edits
        .len(),
      1,
      "immutable strategy save must not mutate the source root"
    );
    let strategic_reopened = PptFile::from_bytes(&bytes).unwrap();
    assert_eq!(
      strategic_reopened
        .persist_object_directory()
        .unwrap()
        .incremental_save_chain
        .edits
        .len(),
      2
    );
    let current = strategic_reopened
      .persist_object_directory()
      .unwrap()
      .current_reference(1)
      .copied()
      .unwrap();
    assert_eq!(
      first_slide_number(&strategic_reopened.document.records.records[current.record_index]),
      4
    );
  }

  #[test]
  fn unknown_record_is_spec_allowed_in_strict_mode() {
    let mut document = Vec::new();
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: 0x779f,
      declared_length: 3,
    }
    .write(&mut document)
    .unwrap();
    document.extend_from_slice(&[1, 2, 3]);

    let document = document_with_minimal_chain(document);
    let file = PptFile::from_compound_file(compound_with_document(document.clone())).unwrap();
    assert!(matches!(
      file.document.records.records.last().unwrap().data,
      PptRecordData::Unknown(_)
    ));
    assert_eq!(
      file
        .to_compound_file()
        .unwrap()
        .stream(POWERPOINT_DOCUMENT_STREAM_PATH),
      Some(document.as_slice())
    );
  }

  #[test]
  fn malformed_and_truncated_document_records_require_compatible_mode() {
    let mut document = Vec::new();
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: DOCUMENT_ATOM,
      declared_length: 1,
    }
    .write(&mut document)
    .unwrap();
    document.push(0xff);
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: 0x1234,
      declared_length: 5,
    }
    .write(&mut document)
    .unwrap();
    document.extend_from_slice(&[1, 2, 3]);

    let document = document_with_minimal_chain(document);
    let compound = compound_with_document(document.clone());
    assert!(PptFile::from_compound_file(compound.clone()).is_err());
    let outcome = PptFile::from_compound_file_compatible(compound).unwrap();
    assert_eq!(outcome.diagnostics.len(), 2);
    assert_eq!(
      outcome.diagnostics[0].code,
      ParseDiagnosticCode::NonconformingRecord
    );
    assert_eq!(
      outcome.diagnostics[1].code,
      ParseDiagnosticCode::TruncatedRecord
    );
    assert_eq!(
      outcome.diagnostics[1].location.offset,
      Some((document.len() - 11) as u64)
    );
    assert!(outcome.value.to_compound_file().is_err());
    assert!(
      outcome
        .value
        .to_bytes_with_options(SaveOptions::default())
        .is_err()
    );
    let preserved_bytes = outcome
      .value
      .to_bytes_with_options(SaveOptions::preserving_compatibility())
      .unwrap();
    let preserved_bytes = CompoundFile::from_bytes(&preserved_bytes).unwrap();
    assert_eq!(
      preserved_bytes.stream(POWERPOINT_DOCUMENT_STREAM_PATH),
      Some(document.as_slice())
    );
    assert_eq!(
      outcome
        .value
        .to_compound_file_preserving_compatibility()
        .unwrap()
        .stream(POWERPOINT_DOCUMENT_STREAM_PATH),
      Some(document.as_slice())
    );
  }

  #[test]
  fn trailing_record_header_prefix_is_diagnostic() {
    let document = document_with_minimal_chain(vec![1, 2, 3, 4]);
    let compound = compound_with_document(document.clone());
    assert!(PptFile::from_compound_file(compound.clone()).is_err());
    let outcome = PptFile::from_compound_file_compatible(compound).unwrap();
    assert_eq!(outcome.diagnostics.len(), 1);
    assert_eq!(
      outcome.diagnostics[0].code,
      ParseDiagnosticCode::TruncatedRecord
    );
    assert_eq!(
      outcome.diagnostics[0].location.offset,
      Some((document.len() - 4) as u64)
    );
    assert_eq!(
      outcome
        .value
        .to_compound_file_preserving_compatibility()
        .unwrap()
        .stream(POWERPOINT_DOCUMENT_STREAM_PATH),
      Some(document.as_slice())
    );
  }

  #[test]
  fn partial_pictures_stream_requires_compatible_mode() {
    let mut pictures = Vec::new();
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: 0xf01e,
      declared_length: 5,
    }
    .write(&mut pictures)
    .unwrap();
    pictures.extend_from_slice(&[1, 2, 3]);
    let mut compound = compound_with_document(document_with_minimal_chain(Vec::new()));
    compound
      .create_or_replace_stream(PICTURES_STREAM_PATH, pictures.clone())
      .unwrap();

    assert!(PptFile::from_compound_file(compound.clone()).is_err());
    let outcome = PptFile::from_compound_file_compatible(compound).unwrap();
    assert!(matches!(
      outcome.value.pictures.as_deref(),
      Some(PicturesStream::Partial(_))
    ));
    assert_eq!(outcome.diagnostics.len(), 1);
    assert_eq!(
      outcome.diagnostics[0].code,
      ParseDiagnosticCode::InvalidStreamPreserved
    );
    assert_eq!(
      outcome.diagnostics[0].location.path.as_deref(),
      Some(PICTURES_STREAM_PATH)
    );
    assert!(outcome.value.to_compound_file().is_err());
    assert_eq!(
      outcome
        .value
        .to_compound_file_preserving_compatibility()
        .unwrap()
        .stream(PICTURES_STREAM_PATH),
      Some(pictures.as_slice())
    );
  }

  #[test]
  fn broken_current_edit_reference_requires_compatible_mode() {
    let compound = compound_with_document(Vec::new());
    assert!(PptFile::from_compound_file(compound.clone()).is_err());
    let outcome = PptFile::from_compound_file_compatible(compound).unwrap();
    assert_eq!(outcome.diagnostics.len(), 1);
    assert_eq!(
      outcome.diagnostics[0].code,
      ParseDiagnosticCode::InvalidReference
    );
    assert_eq!(outcome.diagnostics[0].location.offset, Some(80));
    assert!(outcome.value.to_compound_file().is_err());
    assert!(
      outcome
        .value
        .to_compound_file_preserving_compatibility()
        .is_ok()
    );
  }

  #[test]
  fn invalid_external_storage_requires_compatible_mode() {
    let mut suffix = Vec::new();
    PptRecordHeader {
      version: 0,
      instance: 1,
      record_type: EXTERNAL_OLE_OBJECT_STORAGE,
      declared_length: 5,
    }
    .write(&mut suffix)
    .unwrap();
    suffix.extend_from_slice(&[1, 0, 0, 0, 0xff]);
    let document = document_with_minimal_chain(suffix);
    let compound = compound_with_document(document.clone());

    assert!(PptFile::from_compound_file(compound.clone()).is_err());
    let outcome = PptFile::from_compound_file_compatible(compound).unwrap();
    assert_eq!(outcome.diagnostics.len(), 1);
    assert_eq!(
      outcome.diagnostics[0].code,
      ParseDiagnosticCode::NonconformingRecord
    );
    assert_eq!(
      outcome.diagnostics[0].structure,
      "ExOleObjStgCompressedAtom"
    );
    assert!(outcome.value.to_compound_file().is_err());
    assert_eq!(
      outcome
        .value
        .to_compound_file_preserving_compatibility()
        .unwrap()
        .stream(POWERPOINT_DOCUMENT_STREAM_PATH),
      Some(document.as_slice())
    );
  }

  #[test]
  fn named_malformed_variants_share_the_root_strictness_gate() {
    let sequence = PptRecordSequence {
      records: vec![PptRecord {
        offset: 123,
        header: PptRecordHeader {
          version: 0,
          instance: 0,
          record_type: 0xf142,
          declared_length: 1,
        },
        data: PptRecordData::MalformedTimeVariant(vec![0xff]),
      }],
      trailing_header_bytes: Vec::new(),
    };
    assert!(audit_record_sequence(&sequence, 0, true, &mut Vec::new()).is_err());
    let mut diagnostics = Vec::new();
    audit_record_sequence(&sequence, 0, false, &mut diagnostics).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
      diagnostics[0].code,
      ParseDiagnosticCode::NonconformingRecord
    );
    assert_eq!(diagnostics[0].structure, "TimeVariant");
    assert_eq!(diagnostics[0].specification.section, "2.8.78");
  }

  #[test]
  fn required_current_user_stream_is_not_invented_by_compatibility_mode() {
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound
      .create_or_replace_stream(POWERPOINT_DOCUMENT_STREAM_PATH, Vec::new())
      .unwrap();
    assert!(PptFile::from_compound_file(compound.clone()).is_err());
    assert!(PptFile::from_compound_file_compatible(compound).is_err());
  }

  #[test]
  fn truncated_current_user_is_compatible_only_and_diagnostic() {
    let mut compound = CompoundFile::new(Version::V3).unwrap();
    compound
      .create_or_replace_stream(POWERPOINT_DOCUMENT_STREAM_PATH, Vec::new())
      .unwrap();
    let mut bytes = Vec::new();
    PptRecordHeader {
      version: 0,
      instance: 0,
      record_type: CURRENT_USER_ATOM,
      declared_length: 5,
    }
    .write(&mut bytes)
    .unwrap();
    bytes.extend_from_slice(&[1, 2, 3]);
    compound
      .create_or_replace_stream(CURRENT_USER_STREAM_PATH, bytes)
      .unwrap();

    assert!(PptFile::from_compound_file(compound.clone()).is_err());
    let outcome = PptFile::from_compound_file_compatible(compound).unwrap();
    assert!(matches!(
        outcome.value.current_user.data,
        CurrentUserData::Truncated(ref bytes) if bytes == &[1, 2, 3]
    ));
    assert_eq!(outcome.diagnostics.len(), 1);
    assert_eq!(
      outcome.diagnostics[0].code,
      ParseDiagnosticCode::TruncatedRecord
    );
    assert_eq!(
      outcome.diagnostics[0].location.path.as_deref(),
      Some(CURRENT_USER_STREAM_PATH)
    );
    assert!(outcome.value.to_compound_file().is_err());
    let preserved = outcome
      .value
      .to_compound_file_preserving_compatibility()
      .unwrap();
    assert_eq!(
      preserved.stream(CURRENT_USER_STREAM_PATH),
      outcome.value.compound_file.stream(CURRENT_USER_STREAM_PATH)
    );
  }

  #[test]
  fn current_user_must_fields_require_compatible_mode() {
    let mut current = current_user_stream();
    let CurrentUserData::Parsed(atom) = &mut current.data else {
      panic!("test Current User stream is not parsed");
    };
    atom.document_file_version = 0;
    atom.release_version = 7;
    let mut compound = compound_with_document(document_with_minimal_chain(Vec::new()));
    compound
      .replace_stream(CURRENT_USER_STREAM_PATH, current.to_bytes().unwrap())
      .unwrap();

    assert!(PptFile::from_compound_file(compound.clone()).is_err());
    let outcome = PptFile::from_compound_file_compatible(compound).unwrap();
    assert_eq!(outcome.diagnostics.len(), 1);
    assert_eq!(
      outcome.diagnostics[0].code,
      ParseDiagnosticCode::NonconformingRecord
    );
    assert_eq!(outcome.diagnostics[0].structure, "CurrentUserAtom");
    assert!(outcome.value.to_compound_file().is_err());
    assert!(
      outcome
        .value
        .to_compound_file_preserving_compatibility()
        .is_ok()
    );
  }
}
